//! Native, process-owned Qwen3-TTS 1.7B `VoiceDesign` inference.
//!
//! The caller verifies and retains the pinned GGUF model artifacts before
//! loading them. All inference runs locally through an audited static GGML
//! boundary; this library has no network, preset-voice or audio-device
//! dependency. Keep the engine in its separately supervised service process.
#![cfg(windows)]

use std::{ffi::CString, io, path::Path};

#[allow(
    unsafe_code,
    reason = "Confine the audited Rust C ABI bindings and native audio buffer lifetime to one module."
)]
mod native;

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

/// Exclusive ownership of the native `VoiceDesign` model and its worker thread.
pub struct Synthesizer {
    context: native::Context,
    // The documented C API consumes strings during initialization. Retaining
    // their allocations also protects against a future internal path cache.
    _model_path: CString,
    _codec_path: CString,
}

impl Synthesizer {
    /// Load the independently verified pinned talker and speech-codec GGUFs.
    ///
    /// # Errors
    /// Rejects non-absolute or missing files, non-UTF-8 paths and a model that
    /// cannot initialize as `VoiceDesign`. Errors contain no supplied path text.
    pub fn load(model: &Path, codec: &Path) -> io::Result<Self> {
        let model = model_path(model)?;
        let codec = model_path(codec)?;
        let context = native::Context::load(&model, &codec)?;
        Ok(Self {
            context,
            _model_path: model,
            _codec_path: codec,
        })
    }

    /// Generate speech using the supplied plain-language voice description.
    ///
    /// # Errors
    /// Rejects blank, NUL-containing or oversized text/descriptions, failed
    /// inference and invalid, oversized or potentially token-truncated audio.
    /// Text, voice descriptions and native error messages are never logged.
    pub fn synthesize(&mut self, text: &str, voice: &str) -> io::Result<Audio> {
        let text = bounded_input(text, MAX_TEXT)?;
        let voice = bounded_input(voice, MAX_VOICE)?;
        let audio = self.context.synthesize(&text, &voice)?;
        if audio.is_empty()
            || audio.len() > MAX_SAMPLES
            || audio.len() >= TOKEN_CEILING_SAMPLES
            || audio.iter().any(|sample| !sample.is_finite())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "The native speech model returned invalid or incomplete audio.",
            ));
        }
        Ok(Audio {
            samples: audio,
            sample_rate: SAMPLE_RATE,
        })
    }
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
    use super::{MAX_TEXT, MAX_VOICE, bounded_input};

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
