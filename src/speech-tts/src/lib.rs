//! Native, process-owned Qwen3-TTS voice design with reusable Base conditioning.
//!
//! The caller verifies and retains the pinned GGUF model artifacts before
//! loading them. All inference runs locally through an audited static GGML
//! boundary; this library has no network, preset-voice or audio-device
//! dependency. Keep the engine in its separately supervised service process.
#![cfg(windows)]

use std::{
    ffi::{CStr, CString},
    io,
    path::Path,
};

#[allow(
    unsafe_code,
    reason = "Confine the audited Rust C ABI bindings and native audio buffer lifetime to one module."
)]
mod native;

mod philox;

#[cfg(test)]
#[path = "../build_overlay.rs"]
mod build_overlay;

/// Immutable native engine source revision compiled by the provisioning script.
pub const ENGINE_REVISION: &str = "5916bf7c79dbb4e66fc059cdc507a79a23a28bb4";
/// Exact upstream checkpoint represented by the pinned quantized GGUF files.
pub const MODEL: &str = "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign";
/// Mono PCM output sampling rate.
pub const SAMPLE_RATE: u32 = 24_000;
const MAX_TEXT: usize = 2048;
const MAX_VOICE: usize = 1024;
const MAX_SAMPLES: usize = 24_000 * 120;
const TOKEN_CEILING_SAMPLES: usize = (1500 - 2) * 1920;

/// Complete validated mono speech audio, retained only in memory.
pub struct Audio {
    /// Finite normalized floating-point PCM samples, in playback order.
    pub samples: Vec<f32>,
    /// Sampling rate, always [`SAMPLE_RATE`].
    pub sample_rate: u32,
}

/// Exclusive ownership of one native model and its selected reusable voice.
///
/// `VoiceDesign` creates a fixed reference once per profile. Base synthesis then
/// uses that reference for every text, including separate paragraphs/messages.
/// The two model contexts never coexist, preserving the speech memory policy.
pub struct Synthesizer {
    voices: VoiceCache<Voice>,
    design: Option<native::Context>,
    // Retain allocations because native loaders may cache their path pointers.
    model_path: CString,
    codec_path: CString,
    voice_model_path: CString,
    threads: usize,
}

struct Voice {
    context: native::Context,
    reference: native::VoiceReference,
}

// The cache key deliberately excludes utterance text, request identifiers,
// playback mode and chunk boundaries. Replacing a profile releases its model
// before constructing the next one; acquisition failures cannot fall back to
// an unrelated voice or retain two loaded models.
struct VoiceCache<T>(Option<(CString, T)>);

impl<T> VoiceCache<T> {
    fn get_or_create(
        &mut self,
        profile: &CStr,
        create: impl FnOnce() -> io::Result<T>,
    ) -> io::Result<&mut T> {
        if self
            .0
            .as_ref()
            .is_none_or(|(key, _)| key.as_c_str() != profile)
        {
            self.0 = None;
            let voice = create()?;
            self.0 = Some((profile.to_owned(), voice));
        }
        self.0
            .as_mut()
            .map(|(_, value)| value)
            .ok_or_else(|| io::Error::other("The selected speech voice is unavailable."))
    }
}

impl Synthesizer {
    /// Load verified `VoiceDesign`, codec and Base voice-model GGUFs.
    ///
    /// # Errors
    /// Rejects non-absolute or missing files, non-UTF-8 paths and a model that
    /// cannot initialize as `VoiceDesign`. Errors contain no supplied path text.
    pub fn load(model: &Path, codec: &Path, voice_model: &Path) -> io::Result<Self> {
        Self::load_with_threads(model, codec, voice_model, None)
    }

    /// Load verified weights with an optional bounded CPU compute team size.
    ///
    /// The default reserves two available CPUs for the UI and text runtime.
    ///
    /// # Errors
    /// Returns the same failures as [`Self::load`], or rejects zero, more than
    /// 256 threads. An explicit override may oversubscribe available CPUs for
    /// controlled evaluation; the default never does so.
    pub fn load_with_threads(
        model: &Path,
        codec: &Path,
        voice_model: &Path,
        threads: Option<usize>,
    ) -> io::Result<Self> {
        let available = std::thread::available_parallelism()?.get();
        let threads = select_threads(threads, available)?;
        let model = model_path(model)?;
        let codec = model_path(codec)?;
        let voice_model = model_path(voice_model)?;
        let design = native::Context::load(&model, &codec, threads, c"voice_design")?;
        Ok(Self {
            voices: VoiceCache(None),
            design: Some(design),
            model_path: model,
            codec_path: codec,
            voice_model_path: voice_model,
            threads,
        })
    }

    fn voice(&mut self, profile: &CStr) -> io::Result<&mut Voice> {
        let Self {
            voices,
            design,
            model_path,
            codec_path,
            voice_model_path,
            threads,
        } = self;
        voices.get_or_create(profile, || {
            let mut context = match design.take() {
                Some(context) => context,
                None => native::Context::load(model_path, codec_path, *threads, c"voice_design")?,
            };
            let samples = context.design_reference(profile)?;
            // Free Design's model and worker before loading Base. No user text
            // participates in reference generation, including the first call.
            drop(context);
            let mut context =
                native::Context::load(voice_model_path, codec_path, *threads, c"base")?;
            let reference = context.extract_voice(&samples)?;
            Ok(Voice { context, reference })
        })
    }

    /// Generate speech with the reusable voice selected by its description.
    ///
    /// # Errors
    /// Rejects blank, NUL-containing or oversized text/descriptions, failed
    /// inference and invalid, oversized or potentially token-truncated audio.
    /// Text, voice descriptions and native error messages are never logged.
    pub fn synthesize(&mut self, text: &str, voice: &str) -> io::Result<Audio> {
        let text = bounded_input(text, MAX_TEXT)?;
        let voice = bounded_input(voice, MAX_VOICE)?;
        let voice = self.voice(&voice)?;
        let audio = voice.context.synthesize(&text, &voice.reference)?;
        Ok(Audio {
            samples: audio,
            sample_rate: SAMPLE_RATE,
        })
    }

    /// Generate mono 24 kHz chunks while inference is still running.
    ///
    /// The sink executes on the native worker and must return promptly. Audio
    /// heard before a later failure remains partial; success is reported only
    /// after final sample-count and token-ceiling validation.
    ///
    /// # Errors
    /// Rejects the same inputs/output failures as [`Self::synthesize`], and any
    /// sink failure, without exposing text or native diagnostics.
    pub fn synthesize_stream(
        &mut self,
        text: &str,
        voice: &str,
        sink: &mut (dyn FnMut(&[f32]) -> io::Result<()> + Send),
    ) -> io::Result<()> {
        let text = bounded_input(text, MAX_TEXT)?;
        let voice = bounded_input(voice, MAX_VOICE)?;
        let voice = self.voice(&voice)?;
        voice
            .context
            .synthesize_stream(&text, &voice.reference, sink)
    }
}

// Shared by borrowed complete output and streaming callbacks. Native output is
// admitted before copying or delivering it, with one token-completion policy.
fn sample_count(samples: &[f32], previous: usize) -> Option<usize> {
    bonsai_audio::finite_sample_count(
        samples,
        previous,
        MAX_SAMPLES.min(TOKEN_CEILING_SAMPLES - 1),
    )
}

fn select_threads(requested: Option<usize>, available: usize) -> io::Result<usize> {
    let count = requested.unwrap_or_else(|| available.saturating_sub(2).clamp(1, 256));
    if count == 0 || count > 256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid speech CPU thread count.",
        ));
    }
    Ok(count)
}

fn model_path(path: &Path) -> io::Result<CString> {
    if !path.is_absolute() || !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Native speech needs existing absolute GGUF model files.",
        ));
    }
    let text = path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Native speech requires UTF-8 paths.",
        )
    })?;
    CString::new(text).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Native speech requires regular file paths.",
        )
    })
}

fn bounded_input(text: &str, maximum: usize) -> io::Result<CString> {
    if text.trim().is_empty() || text.chars().take(maximum + 1).count() > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Speech text or voice description exceeds its bound.",
        ));
    }
    CString::new(text).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Speech text and voice descriptions cannot contain NUL.",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_TEXT, MAX_VOICE, TOKEN_CEILING_SAMPLES, VoiceCache, bounded_input, sample_count,
        select_threads,
    };

    #[test]
    fn selected_voice_is_reused_until_profile_changes_and_failures_never_fallback()
    -> std::io::Result<()> {
        use std::{cell::Cell, io, rc::Rc};
        struct FakeVoice {
            identity: usize,
            alive: Rc<Cell<usize>>,
        }
        impl Drop for FakeVoice {
            fn drop(&mut self) {
                self.alive.set(self.alive.get() - 1);
            }
        }
        let alive = Rc::new(Cell::new(0));
        let acquisitions = Cell::new(0);
        let acquire = || {
            assert_eq!(alive.get(), 0, "Two speech model contexts coexist");
            acquisitions.set(acquisitions.get() + 1);
            alive.set(1);
            Ok(FakeVoice {
                identity: acquisitions.get(),
                alive: Rc::clone(&alive),
            })
        };
        let mut cache = VoiceCache(None);
        // Neither new utterances, paragraphs nor complete/stream output mode
        // are cache inputs: they must all use the selected identity unchanged.
        for _utterance in [
            "First paragraph.",
            "Second paragraph.\n\nA line break.",
            "Another message.",
        ] {
            assert_eq!(cache.get_or_create(c"Warm narrator", acquire)?.identity, 1);
        }
        assert_eq!(acquisitions.get(), 1);
        assert_eq!(cache.get_or_create(c"Clear alto", acquire)?.identity, 2);
        assert!(
            cache
                .get_or_create(c"New profile", || Err(io::Error::other(
                    "synthetic acquisition failure"
                )))
                .is_err()
        );
        assert_eq!(alive.get(), 0);
        assert!(
            cache.0.is_none(),
            "Failed voice acquisition left an unrelated fallback"
        );
        assert_eq!(cache.get_or_create(c"Warm narrator", acquire)?.identity, 3);
        drop(cache);
        assert_eq!(alive.get(), 0);
        Ok(())
    }

    #[test]
    fn complete_and_streamed_samples_share_token_ceiling_and_finite_validation() {
        assert_eq!(sample_count(&[0.0, 2.0], 0), Some(2));
        assert_eq!(
            sample_count(&[0.0], TOKEN_CEILING_SAMPLES - 2),
            Some(TOKEN_CEILING_SAMPLES - 1)
        );
        assert_eq!(sample_count(&[0.0], TOKEN_CEILING_SAMPLES - 1), None);
        assert_eq!(sample_count(&[0.0], usize::MAX), None);
        for samples in [&[][..], &[f32::NAN], &[f32::INFINITY], &[f32::NEG_INFINITY]] {
            assert_eq!(sample_count(samples, 0), None);
            assert_eq!(sample_count(samples, 1), None);
        }
    }

    #[test]
    fn cpu_threads_reserve_ui_capacity_and_bound_explicit_overrides() -> std::io::Result<()> {
        assert_eq!(select_threads(None, 12)?, 10);
        assert_eq!(select_threads(None, 1)?, 1);
        assert_eq!(select_threads(Some(12), 12)?, 12);
        assert!(select_threads(Some(0), 12).is_err());
        assert_eq!(select_threads(Some(16), 12)?, 16);
        assert!(select_threads(Some(257), 512).is_err());
        Ok(())
    }

    #[test]
    fn voice_design_input_bounds_count_unicode_without_truncating() {
        for (limit, text) in [
            (MAX_TEXT, "界".repeat(MAX_TEXT)),
            (MAX_VOICE, "🎙".repeat(MAX_VOICE)),
        ] {
            assert_eq!(
                bounded_input(&text, limit)
                    .ok()
                    .as_ref()
                    .map(std::ffi::CString::as_bytes),
                Some(text.as_bytes())
            );
            assert!(bounded_input(&(text + "x"), limit).is_err());
        }
        assert!(bounded_input(" \n\t", MAX_TEXT).is_err());
        assert!(bounded_input("voice\0description", MAX_VOICE).is_err());
    }
}
