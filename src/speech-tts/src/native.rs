//! Ownership and bounds for the pinned native Qwen C ABI.
#![allow(
    clippy::redundant_pub_crate,
    reason = "Keep the audited native boundary visible only to its parent module."
)]

use std::{
    ffi::{CStr, c_char, c_int, c_void},
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::{Mutex, Once},
};

mod qwen;
use qwen::RawAudio;

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

// The returned pointers borrow text/voice and are used only by the synchronous
// synthesis call below; this helper never stores or dereferences them.
fn synthesis_params(text: &CStr, voice: &CStr) -> qwen::TtsParams {
    let mut params = qwen::TtsParams::default();
    // SAFETY: params is the complete, exclusively writable pinned ABI-v6 POD.
    unsafe { qwen::qt_tts_default_params(&raw mut params) };
    params.text = text.as_ptr();
    params.lang = c"auto".as_ptr();
    params.instruct = voice.as_ptr();
    params.max_new_tokens = 1500;
    params
}

impl Context {
    #[allow(
        ffi_unwind_calls,
        reason = "The potentially throwing Qwen call is contained by abort_on_unwind."
    )]
    pub(super) fn load(model: &CStr, codec: &CStr, threads: usize) -> io::Result<Self> {
        let threads = c_int::try_from(threads).map_err(|_| invalid_audio())?;
        let _initialize = INITIALIZE
            .lock()
            .map_err(|_| io::Error::other("Native initialization failed."))?;
        LOGGING.call_once(|| {
            // SAFETY: The callback never dereferences, allocates, logs or panics
            // and is valid on every worker for the process lifetime. Install it
            // once: the upstream callback user-data slot is not atomic.
            unsafe { qwen::qt_log_set(discard_log, std::ptr::null_mut()) };
        });
        let mut params = qwen::InitParams::default();
        // SAFETY: This is the complete, writable ABI-v6 POD layout.
        unsafe { qwen::qt_init_default_params(&raw mut params) };
        params.talker_path = model.as_ptr();
        params.codec_path = codec.as_ptr();
        params.device = c"CPU".as_ptr();
        params.use_fa = false;
        params.max_batch = 1;
        params.codec_chunk_sec = 8.0;
        // SAFETY: The reviewed overlay atomically stores a validated bounded
        // count. INITIALIZE prevents another context changing it during init.
        unsafe { qwen::bonsai_qt_set_cpu_threads(threads) };
        // SAFETY: Both paths are owned NUL-terminated strings retained by the
        // Synthesizer; params is the pinned ABI-v6 layout and survives the call.
        // C-unwind permits an escaped foreign exception into abort_on_unwind.
        let pointer = abort_on_unwind(|| unsafe { qwen::qt_init(&raw const params) });
        let context = NonNull::new(pointer).map(Self).ok_or_else(|| {
            io::Error::other("The native Qwen VoiceDesign model could not initialize.")
        })?;
        // SAFETY: context owns a live initialization result; the returned static
        // model-type string stays valid until that context is released.
        let model_type = unsafe { qwen::qt_model_type(context.0.as_ptr()) };
        if model_type.is_null()
            // SAFETY: The pinned API returns a NUL-terminated model type here.
            || unsafe { CStr::from_ptr(model_type) } != c"voice_design"
        {
            return Err(io::Error::other(
                "The native Qwen VoiceDesign model could not initialize.",
            ));
        }
        Ok(context)
    }

    #[allow(
        ffi_unwind_calls,
        reason = "The potentially throwing Qwen call is contained by abort_on_unwind."
    )]
    pub(super) fn synthesize(&mut self, text: &CStr, voice: &CStr) -> io::Result<Vec<f32>> {
        let mut audio = NativeAudio(RawAudio::default());
        let params = synthesis_params(text, voice);
        // SAFETY: The exclusive context borrow prevents simultaneous calls.
        // Both bounded C strings stay alive until this blocking call returns.
        // The output points at a correctly laid out, initialized qt_audio POD;
        // C-unwind permits foreign exceptions into the aborting boundary.
        let status = abort_on_unwind(|| unsafe {
            qwen::qt_synthesize(self.0.as_ptr(), &raw const params, &raw mut audio.0)
        });
        if status != 0 {
            return Err(io::Error::other("Native Qwen speech synthesis failed."));
        }
        audio.copy_samples()
    }

    #[allow(
        ffi_unwind_calls,
        reason = "The potentially throwing Qwen call is contained by abort_on_unwind."
    )]
    pub(super) fn synthesize_stream(
        &mut self,
        text: &CStr,
        voice: &CStr,
        sink: &mut (dyn FnMut(&[f32]) -> io::Result<()> + Send),
    ) -> io::Result<()> {
        let state = Mutex::new(StreamState {
            sink,
            samples: 0,
            failed: false,
        });
        let mut params = synthesis_params(text, voice);
        params.on_chunk = Some(stream_chunk);
        params.on_chunk_user_data = (&raw const state).cast_mut().cast();
        // SAFETY: The pinned ABI blocks until its worker finishes callbacks.
        // Text, voice, mutex and sink all outlive that call. The mutex serializes
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

impl NativeAudio {
    fn copy_samples(&self) -> io::Result<Vec<f32>> {
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
        // alive while copied and is freed only when this guard drops afterward.
        let source = unsafe { std::slice::from_raw_parts(self.0.samples, count) };
        super::sample_count(source, 0).ok_or_else(invalid_audio)?;
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(count)
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
    use super::{Context, NativeAudio, StreamState, abort_on_unwind, qwen};
    use std::{error::Error, ffi::CString, fs, io, process::Command, time::SystemTime};

    type TestResult = Result<(), Box<dyn Error>>;

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
    fn pinned_qwen_defaults_populate_complete_rust_abi() {
        let mut init = qwen::InitParams::default();
        // SAFETY: init is the complete initialized ABI-v6 POD, exclusively borrowed.
        unsafe { qwen::qt_init_default_params(&raw mut init) };
        assert_eq!(init.abi_version, 6);
        assert!(init.talker_path.is_null() && init.codec_path.is_null());
        assert!(init.use_fa && !init.clamp_fp16);
        assert_eq!(init.max_batch, 1);
        assert_eq!(init.codec_chunk_sec.to_bits(), 24.0_f32.to_bits());
        assert!(init.device.is_null());

        let mut tts = qwen::TtsParams::default();
        // SAFETY: tts is the complete initialized ABI-v6 POD, exclusively borrowed.
        unsafe { qwen::qt_tts_default_params(&raw mut tts) };
        assert_eq!(tts.abi_version, 6);
        assert!(tts.text.is_null() && tts.lang.is_null() && tts.instruct.is_null());
        assert_eq!(tts.seed, -1);
        assert!(tts.max_new_tokens > 0);
        assert!(tts.speaker.is_null() && tts.ref_audio_24k.is_null() && tts.ref_text.is_null());
        assert!(tts.dump_dir.is_null() && tts.cancel.is_none() && tts.on_chunk.is_none());
        assert!(tts.cancel_user_data.is_null() && tts.on_chunk_user_data.is_null());
        assert!(tts.ref_spk_emb.is_null() && tts.ref_codes.is_null());
        assert_eq!(tts.ref_spk_dim, 0);
        assert_eq!(tts.ref_frames, 0);
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
            let error = Context::load(&path, &path, 1)
                .err()
                .ok_or("Unexpected model load")?;
            assert_eq!(error.kind(), io::ErrorKind::Other);
            assert_eq!(
                error.to_string(),
                "The native Qwen VoiceDesign model could not initialize."
            );
        }
        let audio = NativeAudio(qwen::RawAudio::default());
        assert_eq!(
            audio.copy_samples().err().map(|error| error.kind()),
            Some(io::ErrorKind::InvalidData)
        );
        drop(audio);
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
