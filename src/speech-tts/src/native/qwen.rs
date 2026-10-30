//! Complete POD layouts for the pinned Qwen public ABI, version 6.
// Default initializers derive from qwen.cpp and sampling-defaults.h at
// ENGINE_REVISION; MIT licensed, see ../../LICENSE.qwen-core.

use std::ffi::{c_char, c_int, c_void};

#[repr(C)]
pub(super) struct InitParams {
    pub(super) abi_version: c_int,
    pub(super) talker_path: *const c_char,
    pub(super) codec_path: *const c_char,
    pub(super) use_fa: bool,
    pub(super) clamp_fp16: bool,
    pub(super) max_batch: c_int,
    pub(super) codec_chunk_sec: f32,
    pub(super) device: *const c_char,
}

// Port of qt_init_default_params at ENGINE_REVISION. Keep defaults in safe Rust;
// the linked native implementation remains an independent model-free test oracle.
impl Default for InitParams {
    fn default() -> Self {
        Self {
            abi_version: 6,
            talker_path: std::ptr::null(),
            codec_path: std::ptr::null(),
            use_fa: true,
            clamp_fp16: false,
            max_batch: 1,
            codec_chunk_sec: 24.0,
            device: std::ptr::null(),
        }
    }
}

#[repr(C)]
pub(super) struct TtsParams {
    pub(super) abi_version: c_int,
    pub(super) text: *const c_char,
    pub(super) lang: *const c_char,
    pub(super) instruct: *const c_char,
    pub(super) speaker: *const c_char,
    pub(super) ref_audio_24k: *const f32,
    pub(super) ref_n_samples: c_int,
    pub(super) ref_text: *const c_char,
    pub(super) seed: i64,
    pub(super) max_new_tokens: c_int,
    pub(super) temperature: f32,
    pub(super) top_k: c_int,
    pub(super) top_p: f32,
    pub(super) repetition_penalty: f32,
    pub(super) subtalker_temperature: f32,
    pub(super) subtalker_top_k: c_int,
    pub(super) subtalker_top_p: f32,
    pub(super) dump_dir: *const c_char,
    pub(super) cancel: Option<extern "C" fn(*mut c_void) -> bool>,
    pub(super) cancel_user_data: *mut c_void,
    pub(super) on_chunk: Option<extern "C" fn(*const f32, c_int, *mut c_void) -> bool>,
    pub(super) on_chunk_user_data: *mut c_void,
    pub(super) ref_spk_emb: *const f32,
    pub(super) ref_spk_dim: c_int,
    pub(super) ref_codes: *const i32,
    pub(super) ref_frames: c_int,
}

// Port of qt_tts_default_params and sampling-defaults.h at ENGINE_REVISION.
// Explicitly initialize the complete ABI, including unsupported trailing fields.
impl Default for TtsParams {
    fn default() -> Self {
        Self {
            abi_version: 6,
            text: std::ptr::null(),
            lang: std::ptr::null(),
            instruct: std::ptr::null(),
            speaker: std::ptr::null(),
            ref_audio_24k: std::ptr::null(),
            ref_n_samples: 0,
            ref_text: std::ptr::null(),
            seed: -1,
            max_new_tokens: 2048,
            temperature: 0.9,
            top_k: 50,
            top_p: 1.0,
            repetition_penalty: 1.05,
            subtalker_temperature: 0.9,
            subtalker_top_k: 50,
            subtalker_top_p: 1.0,
            dump_dir: std::ptr::null(),
            cancel: None,
            cancel_user_data: std::ptr::null_mut(),
            on_chunk: None,
            on_chunk_user_data: std::ptr::null_mut(),
            ref_spk_emb: std::ptr::null(),
            ref_spk_dim: 0,
            ref_codes: std::ptr::null(),
            ref_frames: 0,
        }
    }
}

// malloc-owned latent buffers returned by qt_extract_voice_ref.
#[repr(C)]
#[derive(Default)]
pub(super) struct RawVoiceReference {
    pub(super) speaker: *mut f32,
    pub(super) speaker_dimension: c_int,
    pub(super) codes: *mut i32,
    pub(super) frames: c_int,
    pub(super) codebooks: c_int,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct RawAudio {
    pub(super) samples: *mut f32,
    pub(super) sample_count: c_int,
    pub(super) sample_rate: c_int,
    pub(super) channels: c_int,
}

// These pinned functions only assign POD fields, access existing storage, or
// release a malloc allocation; none performs a throwing C++ operation.
unsafe extern "C" {
    // Runner's reviewed private build overlay adds this bounded atomic setter.
    pub(super) fn bonsai_qt_set_cpu_threads(threads: c_int);
    #[cfg(test)]
    pub(super) fn qt_init_default_params(params: *mut InitParams);
    #[cfg(test)]
    pub(super) fn qt_tts_default_params(params: *mut TtsParams);
    pub(super) fn qt_model_type(context: *const c_void) -> *const c_char;
    pub(super) fn qt_audio_free(audio: *mut RawAudio);
    pub(super) fn qt_voice_ref_free(reference: *mut RawVoiceReference);
    pub(super) fn qt_log_set(
        callback: extern "C" fn(c_int, *const c_char, *mut c_void),
        user_data: *mut c_void,
    );
}

// The public C API's implementation can allocate or throw outside its own
// catches. Never give these a non-unwind ABI: native::abort_on_unwind contains
// each call and terminates the supervised companion on an escaped exception.
unsafe extern "C-unwind" {
    pub(super) fn qt_init(params: *const InitParams) -> *mut c_void;
    pub(super) fn qt_free(context: *mut c_void);
    pub(super) fn qt_extract_voice_ref(
        context: *mut c_void,
        samples: *const f32,
        count: c_int,
        reference: *mut RawVoiceReference,
    ) -> c_int;
    pub(super) fn qt_synthesize(
        context: *mut c_void,
        params: *const TtsParams,
        audio: *mut RawAudio,
    ) -> c_int;
}

// Windows ARM64, audited against qwen.h at ENGINE_REVISION. Keep the entire
// structures, including unsupported trailing fields, before crossing the ABI.
const _: () = {
    use std::mem::offset_of;
    assert!(size_of::<InitParams>() == 48 && align_of::<InitParams>() == 8);
    assert!(offset_of!(InitParams, talker_path) == 8);
    assert!(offset_of!(InitParams, codec_path) == 16);
    assert!(offset_of!(InitParams, use_fa) == 24);
    assert!(offset_of!(InitParams, clamp_fp16) == 25);
    assert!(offset_of!(InitParams, max_batch) == 28);
    assert!(offset_of!(InitParams, codec_chunk_sec) == 32);
    assert!(offset_of!(InitParams, device) == 40);
    assert!(size_of::<TtsParams>() == 176 && align_of::<TtsParams>() == 8);
    assert!(offset_of!(TtsParams, text) == 8);
    assert!(offset_of!(TtsParams, ref_text) == 56);
    assert!(offset_of!(TtsParams, seed) == 64);
    assert!(offset_of!(TtsParams, max_new_tokens) == 72);
    assert!(offset_of!(TtsParams, dump_dir) == 104);
    assert!(offset_of!(TtsParams, cancel) == 112);
    assert!(offset_of!(TtsParams, on_chunk) == 128);
    assert!(offset_of!(TtsParams, ref_spk_emb) == 144);
    assert!(offset_of!(TtsParams, ref_codes) == 160);
    assert!(offset_of!(TtsParams, ref_frames) == 168);
    assert!(size_of::<RawVoiceReference>() == 32 && align_of::<RawVoiceReference>() == 8);
    assert!(offset_of!(RawVoiceReference, speaker) == 0);
    assert!(offset_of!(RawVoiceReference, speaker_dimension) == 8);
    assert!(offset_of!(RawVoiceReference, codes) == 16);
    assert!(offset_of!(RawVoiceReference, frames) == 24);
    assert!(offset_of!(RawVoiceReference, codebooks) == 28);
    assert!(size_of::<RawAudio>() == 24 && align_of::<RawAudio>() == 8);
    assert!(offset_of!(RawAudio, sample_count) == 8);
    assert!(offset_of!(RawAudio, sample_rate) == 12);
    assert!(offset_of!(RawAudio, channels) == 16);
};
