//! Ownership and bounds for the pinned native Qwen C ABI.
#![allow(
    clippy::redundant_pub_crate,
    reason = "Keep the audited native boundary visible only to its parent module."
)]

use std::{
    ffi::{CStr, c_char, c_int, c_void},
    io,
    ptr::NonNull,
};

#[repr(C)]
struct RawAudio {
    samples: *mut f32,
    sample_count: c_int,
    sample_rate: c_int,
    channels: c_int,
}

unsafe extern "C" {
    fn bonsai_tts_load(model: *const c_char, codec: *const c_char) -> *mut c_void;
    fn bonsai_tts_free(context: *mut c_void);
    fn bonsai_tts_synthesize(
        context: *mut c_void,
        text: *const c_char,
        voice: *const c_char,
        audio: *mut RawAudio,
    ) -> c_int;
    fn bonsai_tts_audio_free(audio: *mut RawAudio);
}

/// One exclusively borrowed native model, freed only after its blocking calls.
pub(super) struct Context(NonNull<c_void>);

impl Context {
    pub(super) fn load(model: &CStr, codec: &CStr) -> io::Result<Self> {
        // SAFETY: Both paths are owned NUL-terminated strings retained by the
        // Synthesizer. The pinned shim catches exceptions and returns ownership
        // of a newly allocated context, or null without transferring resources.
        let pointer = unsafe { bonsai_tts_load(model.as_ptr(), codec.as_ptr()) };
        NonNull::new(pointer).map(Self).ok_or_else(|| {
            io::Error::other("The native Qwen VoiceDesign model could not initialize.")
        })
    }

    pub(super) fn synthesize(&mut self, text: &CStr, voice: &CStr) -> io::Result<Vec<f32>> {
        let mut audio = NativeAudio(RawAudio {
            samples: std::ptr::null_mut(),
            sample_count: 0,
            sample_rate: 0,
            channels: 0,
        });
        // SAFETY: The exclusive context borrow prevents simultaneous calls.
        // Both bounded C strings stay alive until this blocking call returns.
        // The output points at a correctly laid out, initialized qt_audio POD;
        // the shim catches C++ exceptions and NativeAudio owns any result.
        let status = unsafe {
            bonsai_tts_synthesize(
                self.0.as_ptr(),
                text.as_ptr(),
                voice.as_ptr(),
                &raw mut audio.0,
            )
        };
        if status != 0 {
            return Err(io::Error::other("Native Qwen speech synthesis failed."));
        }
        audio.copy_samples()
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: This object owns the one successful initialization result.
        // No synthesis call outlives its exclusive borrow, so the native worker
        // can be joined and all model allocations freed exactly once here.
        unsafe { bonsai_tts_free(self.0.as_ptr()) };
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
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(count)
            .map_err(|_| io::Error::other("Could not allocate the bounded speech audio buffer."))?;
        // SAFETY: A successful pinned C API call returns an aligned allocation
        // of sample_count f32 elements owned by this guard. Count is positive,
        // bounded to 120 seconds and far below isize::MAX. The allocation stays
        // alive while copied and is freed only when this guard drops afterward.
        let source = unsafe { std::slice::from_raw_parts(self.0.samples, count) };
        samples.extend_from_slice(source);
        Ok(samples)
    }
}

impl Drop for NativeAudio {
    fn drop(&mut self) {
        // SAFETY: The initialized POD owns its native allocation, if any. The
        // matching allocator accepts an empty POD and clears it after freeing.
        unsafe { bonsai_tts_audio_free(&raw mut self.0) };
    }
}

fn invalid_audio() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Native Qwen returned an invalid audio buffer.",
    )
}
