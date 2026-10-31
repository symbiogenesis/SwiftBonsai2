//! Ownership and bounds for the pinned native Qwen C ABI.
#![allow(
    clippy::redundant_pub_crate,
    reason = "Keep the audited native boundary visible only to its parent module."
)]

use std::{
    ffi::{CStr, c_char, c_int, c_uint, c_void},
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::{Mutex, Once},
};

mod cpu_threads;
mod qwen;
mod sampling;
mod tokenizer;
use qwen::RawAudio;

#[cfg(test)]
mod philox_tests;
#[cfg(test)]
mod sampling_tests;
#[cfg(test)]
mod tokenizer_tests;

// SAFETY: This unique private static-link symbol is declared by the reviewed
// Qwen overlay. Its scalar arguments carry no pointer or lifetime obligations.
// The pure Rust implementation allocates nothing and cannot unwind.
#[unsafe(no_mangle)]
extern "C" fn bonsai_qt_philox_uniform(seed: i64, subsequence: i64, counter: u32) -> f32 {
    super::philox::uniform(seed, subsequence, counter)
}

static CPU_THREADS: cpu_threads::CpuThreads = cpu_threads::CpuThreads::new();

// SAFETY: This unique private static-link symbol is declared by the reviewed
// overlay. Scalar CPU counts have no pointer/lifetime obligations. The Rust
// policy reads an atomic and uses bounded integer arithmetic without unwinding.
#[unsafe(no_mangle)]
extern "C" fn bonsai_qt_cpu_threads(logical_threads: c_uint) -> c_int {
    CPU_THREADS.get(logical_threads)
}

// A single public neutral fixture defines voice identity, never the text being
// read. Recreating a stopped worker uses the same phrase and sampler seed.
static REFERENCE_TEXT: &CStr = c"The morning light fills the quiet garden. I will read each sentence in the same clear, natural voice.";
const VOICE_SEED: i64 = 42;
const MAX_REFERENCE_SAMPLES: usize = 24_000 * 30;
const CODEBOOKS: i32 = 16;
const CODEC_HOP: usize = 1920;

const extern "C" fn discard_log(_: c_int, _: *const c_char, _: *mut c_void) {}
static LOGGING: Once = Once::new();
static INITIALIZE: Mutex<()> = Mutex::new(());

// A foreign exception may abort inside catch_unwind or return an opaque Err.
// In the latter case abort without inspecting/dropping that payload or reusing
// potentially damaged native state. The owning Windows Job isolates failure.
fn abort_on_unwind<T>(call: impl FnOnce() -> T) -> T {
    // No recovery uses the captured state, so unwinding cannot expose it again.
    catch_unwind(AssertUnwindSafe(call)).unwrap_or_else(|_| std::process::abort())
}

/// One exclusively borrowed native model, freed only after its blocking calls.
pub(super) struct Context(NonNull<c_void>);

// The returned pointers borrow their arguments and are used only during the
// synchronous native call; no parameter object escapes its borrowed storage.
fn synthesis_params(text: &CStr) -> qwen::TtsParams {
    qwen::TtsParams {
        text: text.as_ptr(),
        lang: c"auto".as_ptr(),
        seed: VOICE_SEED,
        max_new_tokens: 1500,
        ..qwen::TtsParams::default()
    }
}

fn design_params(voice: &CStr) -> qwen::TtsParams {
    let mut params = synthesis_params(REFERENCE_TEXT);
    params.instruct = voice.as_ptr();
    params
}

fn clone_params(text: &CStr, reference: &qwen::RawVoiceReference) -> qwen::TtsParams {
    let mut params = synthesis_params(text);
    params.ref_text = REFERENCE_TEXT.as_ptr();
    params.ref_spk_emb = reference.speaker;
    params.ref_spk_dim = reference.speaker_dimension;
    params.ref_codes = reference.codes;
    params.ref_frames = reference.frames;
    params
}

/// Owned, validated speaker and RVQ conditioning shared by every text segment.
pub(super) struct VoiceReference(qwen::RawVoiceReference);

impl VoiceReference {
    fn validate(&self) -> io::Result<()> {
        let raw = &self.0;
        let dimension = usize::try_from(raw.speaker_dimension).map_err(|_| invalid_audio())?;
        let frames = usize::try_from(raw.frames).map_err(|_| invalid_audio())?;
        if dimension == 0
            || dimension > 8192
            || frames == 0
            || frames > MAX_REFERENCE_SAMPLES / CODEC_HOP
            || raw.codebooks != CODEBOOKS
            || raw.speaker.is_null()
            || !raw.speaker.is_aligned()
            || raw.codes.is_null()
            || !raw.codes.is_aligned()
        {
            return Err(invalid_audio());
        }
        // SAFETY: Successful extraction from the pinned API allocates exactly
        // speaker_dimension f32s and codebooks * frames i32s owned by this guard.
        // Metadata is positive and bounded above; neither allocation is mutated
        // or freed until the guard drops after the blocking synthesis call.
        let speaker = unsafe { std::slice::from_raw_parts(raw.speaker, dimension) };
        // SAFETY: Same pinned allocation contract; 16 * 375 is below isize::MAX.
        let codes = unsafe { std::slice::from_raw_parts(raw.codes, frames * 16) };
        if !speaker.iter().all(|sample| sample.is_finite())
            || !codes.iter().all(|code| (0..2048).contains(code))
        {
            return Err(invalid_audio());
        }
        Ok(())
    }
}

impl Drop for VoiceReference {
    fn drop(&mut self) {
        // SAFETY: The initialized POD exclusively owns both malloc allocations,
        // if any. The matching release also accepts and clears an empty POD.
        unsafe { qwen::qt_voice_ref_free(&raw mut self.0) };
    }
}

impl Context {
    #[allow(
        ffi_unwind_calls,
        reason = "The potentially throwing Qwen call is contained by abort_on_unwind."
    )]
    pub(super) fn load(
        model: &CStr,
        codec: &CStr,
        threads: usize,
        expected_type: &CStr,
    ) -> io::Result<Self> {
        let _initialize = INITIALIZE
            .lock()
            .map_err(|_| io::Error::other("Native initialization failed."))?;
        LOGGING.call_once(|| {
            // SAFETY: The callback never dereferences, allocates, logs or panics
            // and is valid on every worker for the process lifetime. Install it
            // once: the upstream callback user-data slot is not atomic.
            unsafe { qwen::qt_log_set(discard_log, std::ptr::null_mut()) };
        });
        let params = qwen::InitParams {
            talker_path: model.as_ptr(),
            codec_path: codec.as_ptr(),
            device: c"CPU".as_ptr(),
            use_fa: false,
            max_batch: 1,
            codec_chunk_sec: 8.0,
            ..qwen::InitParams::default()
        };
        // INITIALIZE keeps the selected count stable while native initialization
        // queries it for the talker, predictor and codec CPU backends.
        CPU_THREADS.set(threads)?;
        // SAFETY: Both paths are owned NUL-terminated strings retained by the
        // Synthesizer; params is the pinned ABI-v6 layout and survives the call.
        // C-unwind permits an escaped foreign exception into abort_on_unwind.
        let pointer = abort_on_unwind(|| unsafe { qwen::qt_init(&raw const params) });
        let context = NonNull::new(pointer).map(Self).ok_or_else(|| {
            io::Error::other("The native Qwen speech model could not initialize.")
        })?;
        // SAFETY: context owns a live initialization result; the returned static
        // model-type string stays valid until that context is released.
        let model_type = unsafe { qwen::qt_model_type(context.0.as_ptr()) };
        if model_type.is_null()
            // SAFETY: The pinned API returns a NUL-terminated model type here.
            || unsafe { CStr::from_ptr(model_type) } != expected_type
        {
            return Err(io::Error::other(
                "The native Qwen speech model could not initialize.",
            ));
        }
        Ok(context)
    }

    pub(super) fn design_reference(&mut self, voice: &CStr) -> io::Result<NativeSamples> {
        self.generate_audio(&design_params(voice))?
            .into_reference_samples()
    }

    #[allow(
        ffi_unwind_calls,
        reason = "The potentially throwing Qwen call is contained by abort_on_unwind."
    )]
    pub(super) fn extract_voice(&mut self, samples: &[f32]) -> io::Result<VoiceReference> {
        if !(CODEC_HOP..=MAX_REFERENCE_SAMPLES).contains(&samples.len())
            || !samples.iter().all(|sample| sample.is_finite())
        {
            return Err(invalid_audio());
        }
        let count = c_int::try_from(samples.len()).map_err(|_| invalid_audio())?;
        let mut reference = VoiceReference(qwen::RawVoiceReference::default());
        // SAFETY: The exclusive context is a Base model with the pinned speaker
        // encoder. Finite mono 24 kHz PCM stays alive until extraction's worker
        // returns; output is a fully initialized, exclusively writable ABI POD.
        let status = abort_on_unwind(|| unsafe {
            qwen::qt_extract_voice_ref(
                self.0.as_ptr(),
                samples.as_ptr(),
                count,
                &raw mut reference.0,
            )
        });
        if status != 0 {
            return Err(invalid_audio());
        }
        reference.validate()?;
        Ok(reference)
    }

    pub(super) fn synthesize(
        &mut self,
        text: &CStr,
        reference: &VoiceReference,
    ) -> io::Result<Vec<f32>> {
        self.synthesize_audio(&clone_params(text, &reference.0))
    }

    pub(super) fn synthesize_owned(
        &mut self,
        text: &CStr,
        reference: &VoiceReference,
    ) -> io::Result<NativeSamples> {
        self.generate_audio(&clone_params(text, &reference.0))?
            .into_samples()
    }

    fn synthesize_audio(&mut self, params: &qwen::TtsParams) -> io::Result<Vec<f32>> {
        self.generate_audio(params)?.copy_samples()
    }

    #[allow(
        ffi_unwind_calls,
        reason = "The potentially throwing Qwen call is contained by abort_on_unwind."
    )]
    fn generate_audio(&mut self, params: &qwen::TtsParams) -> io::Result<NativeAudio> {
        let mut audio = NativeAudio(RawAudio::default());
        // SAFETY: The exclusive context borrow prevents simultaneous calls.
        // All strings and reference buffers borrowed by params survive this
        // blocking call. Output is the complete initialized pinned audio POD;
        // C-unwind permits foreign exceptions into the aborting boundary.
        let status = abort_on_unwind(|| unsafe {
            qwen::qt_synthesize(self.0.as_ptr(), &raw const *params, &raw mut audio.0)
        });
        if status != 0 {
            return Err(io::Error::other("Native Qwen speech synthesis failed."));
        }
        Ok(audio)
    }

    #[allow(
        ffi_unwind_calls,
        reason = "The potentially throwing Qwen call is contained by abort_on_unwind."
    )]
    pub(super) fn synthesize_stream(
        &mut self,
        text: &CStr,
        reference: &VoiceReference,
        sink: &mut (dyn FnMut(&[f32]) -> io::Result<()> + Send),
    ) -> io::Result<()> {
        let state = Mutex::new(StreamState {
            sink,
            samples: 0,
            failed: false,
        });
        let mut params = clone_params(text, &reference.0);
        params.on_chunk = Some(stream_chunk);
        params.on_chunk_user_data = (&raw const state).cast_mut().cast();
        // SAFETY: The pinned ABI blocks until its worker finishes callbacks.
        // Text, reference, mutex and sink all outlive that call. The mutex serializes
        // accesses from the native worker; streaming permits a null audio out.
        let status = abort_on_unwind(|| unsafe {
            qwen::qt_synthesize(self.0.as_ptr(), &raw const params, std::ptr::null_mut())
        });
        let state = state.into_inner().map_err(|_| invalid_audio())?;
        if status != 0 || state.failed || state.samples == 0 {
            return Err(invalid_audio());
        }
        Ok(())
    }
}

struct StreamState<'a> {
    sink: &'a mut (dyn FnMut(&[f32]) -> io::Result<()> + Send),
    samples: usize,
    failed: bool,
}

impl StreamState<'_> {
    fn push(&mut self, samples: &[f32]) -> bool {
        if self.failed {
            return false;
        }
        let Some(total) = super::sample_count(samples, self.samples) else {
            self.failed = true;
            return false;
        };
        self.samples = total;
        if (self.sink)(samples).is_err() {
            self.failed = true;
        }
        !self.failed
    }
}

extern "C" fn stream_chunk(samples: *const f32, count: c_int, user: *mut c_void) -> bool {
    // A callback must never unwind into C++. Poisoning its mutex ensures a
    // caught sink panic also makes final completion fail closed.
    catch_unwind(AssertUnwindSafe(|| {
        if user.is_null() {
            return false;
        }
        // SAFETY: user points to the live mutex installed for this synchronous
        // qt_synthesize call. Only this callback accesses it on the worker.
        let state = unsafe { &*user.cast::<Mutex<StreamState<'_>>>() };
        let Ok(mut state) = state.lock() else {
            return false;
        };
        let Ok(count) = usize::try_from(count) else {
            state.failed = true;
            return false;
        };
        if samples.is_null() || !samples.is_aligned() || count == 0 || count > super::MAX_SAMPLES {
            state.failed = true;
            return false;
        }
        // SAFETY: The pinned callback owns count aligned readable f32 samples
        // for the duration of the callback; they are borrowed only here.
        state.push(unsafe { std::slice::from_raw_parts(samples, count) })
    }))
    .unwrap_or(false)
}

impl Drop for Context {
    #[allow(
        ffi_unwind_calls,
        reason = "The potentially throwing Qwen call is contained by abort_on_unwind."
    )]
    fn drop(&mut self) {
        // SAFETY: This object owns the one successful initialization result.
        // No synthesis call outlives its exclusive borrow, so the native worker
        // can be joined and all model allocations freed exactly once here.
        abort_on_unwind(|| unsafe { qwen::qt_free(self.0.as_ptr()) });
    }
}

struct NativeAudio(RawAudio);

/// An immutable validated allocation, released by its original native allocator.
pub(super) struct NativeSamples {
    audio: NativeAudio,
    count: usize,
}

impl AsRef<[f32]> for NativeSamples {
    fn as_ref(&self) -> &[f32] {
        // SAFETY: into_samples validated this independent malloc allocation's
        // pointer, alignment, count, format and finite values before constructing
        // this owner. Neither the allocation nor metadata can be mutated again.
        // Its native release guard outlives this borrow and frees it exactly once.
        unsafe { std::slice::from_raw_parts(self.audio.0.samples, self.count) }
    }
}

impl NativeAudio {
    fn samples(&self) -> io::Result<&[f32]> {
        let count = usize::try_from(self.0.sample_count).map_err(|_| invalid_audio())?;
        if count == 0
            || count > super::MAX_SAMPLES
            || self.0.sample_rate != 24_000
            || self.0.channels != 1
            || self.0.samples.is_null()
            || !self.0.samples.is_aligned()
        {
            return Err(invalid_audio());
        }
        // SAFETY: A successful pinned C API call returns an aligned allocation
        // of sample_count f32 elements owned by this guard. Count is positive,
        // bounded to 120 seconds and far below isize::MAX. The allocation stays
        // alive throughout this borrow and is freed only when this guard drops.
        let source = unsafe { std::slice::from_raw_parts(self.0.samples, count) };
        super::sample_count(source, 0).ok_or_else(invalid_audio)?;
        Ok(source)
    }

    fn into_samples(self) -> io::Result<NativeSamples> {
        let count = self.samples()?.len();
        Ok(NativeSamples { audio: self, count })
    }

    fn into_reference_samples(self) -> io::Result<NativeSamples> {
        let samples = self.into_samples()?;
        if !(CODEC_HOP..=MAX_REFERENCE_SAMPLES).contains(&samples.count) {
            return Err(invalid_audio());
        }
        Ok(samples)
    }

    fn copy_samples(&self) -> io::Result<Vec<f32>> {
        let source = self.samples()?;
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(source.len())
            .map_err(|_| io::Error::other("Could not allocate the bounded speech audio buffer."))?;
        samples.extend_from_slice(source);
        Ok(samples)
    }
}

impl Drop for NativeAudio {
    fn drop(&mut self) {
        // SAFETY: The initialized POD owns its native allocation, if any. The
        // matching allocator accepts an empty POD and clears it after freeing.
        unsafe { qwen::qt_audio_free(&raw mut self.0) };
    }
}

fn invalid_audio() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Native Qwen returned an invalid audio buffer.",
    )
}

#[cfg(test)]
mod tests {
    use super::{
        Context, NativeAudio, REFERENCE_TEXT, StreamState, VOICE_SEED, abort_on_unwind,
        clone_params, design_params, qwen,
    };
    use std::{error::Error, ffi::CString, fs, io, process::Command, time::SystemTime};

    type TestResult = Result<(), Box<dyn Error>>;

    // The pinned qt_audio_free uses the same linked UCRT free function.
    unsafe extern "C" {
        fn malloc(size: usize) -> *mut std::ffi::c_void;
    }

    fn native_audio(samples: &[f32]) -> io::Result<NativeAudio> {
        let sample_count = i32::try_from(samples.len()).map_err(io::Error::other)?;
        // SAFETY: Request a bounded nonzero allocation; malloc returns null on
        // failure or suitably aligned storage for this many f32 values.
        let storage = unsafe { malloc(size_of_val(samples)) };
        let storage = std::ptr::NonNull::new(storage)
            .ok_or_else(|| io::Error::other("Synthetic native allocation failed."))?;
        let audio = NativeAudio(qwen::RawAudio {
            samples: storage.as_ptr().cast(),
            sample_count,
            sample_rate: 24_000,
            channels: 1,
        });
        // SAFETY: The distinct malloc allocation owns samples.len() writable
        // f32 slots. The NativeAudio guard now guarantees its matching release.
        unsafe { std::ptr::copy_nonoverlapping(samples.as_ptr(), audio.0.samples, samples.len()) };
        Ok(audio)
    }

    #[test]
    fn complete_native_waveform_retains_original_allocation_and_validates_before_borrow()
    -> TestResult {
        let expected = [0.25, -0.5, 1.0];
        let audio = native_audio(&expected)?;
        let storage = audio.0.samples.cast_const();
        let samples = audio.into_samples()?;
        assert_eq!(samples.as_ref().as_ptr(), storage);
        assert_eq!(samples.as_ref(), expected);
        assert_eq!(samples.as_ref().as_ptr(), storage);
        drop(samples); // Calls the actual pinned qt_audio_free on this allocation.

        for case in 0..6 {
            let mut audio = native_audio(&expected)?;
            match case {
                0 => audio.0.sample_count = 0,
                1 => audio.0.sample_count = -1,
                2 => audio.0.sample_count = i32::MAX,
                3 => audio.0.sample_rate = 16_000,
                4 => audio.0.channels = 2,
                _ => {
                    // SAFETY: This guard exclusively owns the initialized three
                    // f32 allocation. Mutate a sample before validation only.
                    unsafe { audio.0.samples.wrapping_add(2).write(f32::NAN) };
                }
            }
            assert_eq!(
                audio.into_samples().err().map(|error| error.kind()),
                Some(io::ErrorKind::InvalidData)
            );
        }
        Ok(())
    }

    #[test]
    fn voice_bootstrap_retains_native_reference_storage_and_its_separate_bounds() -> TestResult {
        for count in [super::CODEC_HOP, super::MAX_REFERENCE_SAMPLES] {
            let expected = vec![0.25; count];
            let audio = native_audio(&expected)?;
            let original = audio.0.samples.cast_const();
            let samples = audio.into_reference_samples()?;
            assert_eq!(samples.as_ref().as_ptr(), original);
            assert_eq!(samples.as_ref(), expected);
            drop(samples); // Actual native release, independent of any model.
        }
        for count in [super::CODEC_HOP - 1, super::MAX_REFERENCE_SAMPLES + 1] {
            assert_eq!(
                native_audio(&vec![0.0; count])?
                    .into_reference_samples()
                    .err()
                    .map(|error| error.kind()),
                Some(io::ErrorKind::InvalidData)
            );
        }
        let mut invalid = vec![0.0; super::CODEC_HOP];
        invalid[super::CODEC_HOP - 1] = f32::INFINITY;
        assert!(native_audio(&invalid)?.into_reference_samples().is_err());
        Ok(())
    }

    #[test]
    fn every_utterance_borrows_the_same_speaker_codes_and_reference_transcript() {
        let mut speaker = [0.5_f32; 8];
        let mut codes = [1_i32; 32];
        let reference = qwen::RawVoiceReference {
            speaker: speaker.as_mut_ptr(),
            speaker_dimension: 8,
            codes: codes.as_mut_ptr(),
            frames: 2,
            codebooks: 16,
        };
        for text in [
            c"First paragraph.",
            c"Second paragraph.\n\nA line break.",
            c"Another message.",
        ] {
            let params = clone_params(text, &reference);
            assert_eq!(params.text, text.as_ptr());
            assert_eq!(params.ref_spk_emb, speaker.as_ptr());
            assert_eq!(params.ref_spk_dim, 8);
            assert_eq!(params.ref_codes, codes.as_ptr());
            assert_eq!(params.ref_frames, 2);
            assert_eq!(params.ref_text, REFERENCE_TEXT.as_ptr());
            assert!(params.instruct.is_null() && params.speaker.is_null());
            assert!(params.ref_audio_24k.is_null());
            assert_eq!(params.seed, VOICE_SEED);
            assert!(params.seed >= 0);
        }
        for profile in [c"Warm narrator", c"Clear alto"] {
            let design = design_params(profile);
            assert_eq!(design.text, REFERENCE_TEXT.as_ptr());
            assert_eq!(design.instruct, profile.as_ptr());
            assert_eq!(design.seed, VOICE_SEED);
            assert!(design.ref_spk_emb.is_null() && design.ref_codes.is_null());
        }
    }

    #[test]
    fn streaming_samples_reject_nonfinite_overflow_and_sink_failure() {
        let mut count = 0;
        let mut sink = |samples: &[f32]| {
            count += samples.len();
            Ok(())
        };
        let mut state = StreamState {
            sink: &mut sink,
            samples: 0,
            failed: false,
        };
        assert!(state.push(&[0.0, 0.5]));
        assert!(!state.push(&[f32::NAN]));
        assert!(!state.push(&[0.0]));
        assert_eq!(state.samples, 2);
        assert_eq!(count, 2);
        let mut sink = |_: &[f32]| Ok(());
        let mut state = StreamState {
            sink: &mut sink,
            samples: super::super::TOKEN_CEILING_SAMPLES - 1,
            failed: false,
        };
        assert!(!state.push(&[0.0]));
        let mut sink = |_: &[f32]| Err(io::Error::other("synthetic sink failure"));
        let mut state = StreamState {
            sink: &mut sink,
            samples: 0,
            failed: false,
        };
        assert!(!state.push(&[0.0]));
    }

    #[test]
    fn rust_parameter_defaults_match_every_pinned_cpp_field() {
        // Compare fields, never padding bytes or function-pointer addresses.
        macro_rules! equal_fields {
            ($rust:ident, $native:ident, $($field:ident),+ $(,)?) => {
                $(assert_eq!($rust.$field, $native.$field, stringify!($field));)+
            };
        }
        macro_rules! equal_float_bits {
            ($rust:ident, $native:ident, $($field:ident),+ $(,)?) => {
                $(assert_eq!(
                    $rust.$field.to_bits(), $native.$field.to_bits(), stringify!($field)
                );)+
            };
        }

        let rust_init = qwen::InitParams::default();
        let mut init = qwen::InitParams::default();
        // SAFETY: init is the complete initialized ABI-v6 POD, exclusively borrowed.
        unsafe { qwen::qt_init_default_params(&raw mut init) };
        equal_fields!(
            rust_init,
            init,
            abi_version,
            talker_path,
            codec_path,
            use_fa,
            clamp_fp16,
            max_batch,
            device,
        );
        equal_float_bits!(rust_init, init, codec_chunk_sec);

        let rust_tts = qwen::TtsParams::default();
        let mut tts = qwen::TtsParams::default();
        // SAFETY: tts is the complete initialized ABI-v6 POD, exclusively borrowed.
        unsafe { qwen::qt_tts_default_params(&raw mut tts) };
        equal_fields!(
            rust_tts,
            tts,
            abi_version,
            text,
            lang,
            instruct,
            speaker,
            ref_audio_24k,
            ref_n_samples,
            ref_text,
            seed,
            max_new_tokens,
            top_k,
            subtalker_top_k,
            dump_dir,
            cancel_user_data,
            on_chunk_user_data,
            ref_spk_emb,
            ref_spk_dim,
            ref_codes,
            ref_frames,
        );
        equal_float_bits!(
            rust_tts,
            tts,
            temperature,
            top_p,
            repetition_penalty,
            subtalker_temperature,
            subtalker_top_p,
        );
        assert!(rust_tts.cancel.is_none() && tts.cancel.is_none());
        assert!(rust_tts.on_chunk.is_none() && tts.on_chunk.is_none());
    }

    #[test]
    fn native_load_failure_is_redacted_and_empty_audio_is_releasable() -> TestResult {
        let missing = std::env::temp_dir().join(format!(
            "Bonsai-Qwen-Missing-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_nanos()
        ));
        let path = CString::new(missing.to_str().ok_or("Non-UTF-8 temp path")?)?;
        for _ in 0..2 {
            let error = Context::load(&path, &path, 1, c"voice_design")
                .err()
                .ok_or("Unexpected model load")?;
            assert_eq!(error.kind(), io::ErrorKind::Other);
            assert_eq!(
                error.to_string(),
                "The native Qwen speech model could not initialize."
            );
        }
        let audio = NativeAudio(qwen::RawAudio::default());
        assert_eq!(
            audio.copy_samples().err().map(|error| error.kind()),
            Some(io::ErrorKind::InvalidData)
        );
        drop(audio);
        drop(super::VoiceReference(qwen::RawVoiceReference::default()));
        Ok(())
    }

    #[test]
    #[allow(
        clippy::panic,
        reason = "The isolated child deliberately exercises the aborting unwind boundary."
    )]
    fn unexpected_unwind_terminates_companion() -> TestResult {
        const CHILD: &str = "BONSAI_QWEN_UNWIND_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            abort_on_unwind(|| panic!("synthetic native-boundary unwind"));
            return Err("Unwind boundary returned instead of aborting".into());
        }
        let working = std::env::temp_dir().join(format!(
            "Bonsai-Qwen-Unwind-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_nanos()
        ));
        assert!(working.is_absolute() && !working.to_string_lossy().contains('%'));
        fs::create_dir(&working)?;
        let output = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "native::tests::unexpected_unwind_terminates_companion",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .current_dir(&working)
            .output()?;
        assert!(!output.status.success());
        assert_ne!(
            output.status.code(),
            Some(101),
            "The child failed as an ordinary Rust test instead of aborting."
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("synthetic native-boundary unwind")
        );
        fs::remove_dir(&working)?;
        Ok(())
    }
}
