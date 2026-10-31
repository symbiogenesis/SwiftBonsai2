//! Fixed one-token native-reference collection with a Rust-owned Windows boundary.
//!
//! ABI declarations derive from llama.cpp 73bfe6cc485554822f80c473a3c0c4f25e80f41b
//! (MIT; see the source/license identities in the adjacent ABI provenance).
//! The caller verifies and holds the runtime, model, adapter and executable.
#![deny(warnings, unsafe_code)]
#![forbid(unsafe_op_in_unsafe_fn)]

#[cfg(not(all(windows, target_arch = "aarch64", target_endian = "little")))]
compile_error!("The pinned native collector requires little-endian Windows ARM64.");

use std::{
    ffi::{OsStr, OsString},
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::ExitCode,
};

const COMMIT: &str = "73bfe6cc485554822f80c473a3c0c4f25e80f41b";
const MAX_STATE: usize = 512 * 1024 * 1024;
const VOCABULARY: usize = 248_320;
const INPUT_TOKEN: i32 = 248_045;
const MODEL_NAME: &str = "Swift-Bonsai-2-current-repacked-PTQ1_0.gguf";
const ADAPTER_NAME: &str = "bonsai-abliterate-lora.gguf";

fn require(condition: bool, message: &'static str) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message))
    }
}

fn absolute(path: &OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt as _;
    let units: Vec<_> = path.encode_wide().collect();
    units.len() > 3
        && units.get(1) == Some(&u16::from(b':'))
        && matches!(units.get(2), Some(47 | 92))
        && !units.contains(&u16::from(b'%'))
        && !units.contains(&0)
        && !units.windows(2).any(|pair| pair == [46, 46])
}

fn basename_is(path: &OsStr, expected: &str) -> bool {
    use std::os::windows::ffi::OsStrExt as _;
    let units: Vec<_> = path.encode_wide().collect();
    let last = units.rsplit(|&unit| matches!(unit, 47 | 92)).next();
    last.is_some_and(|last| last.iter().copied().eq(expected.encode_utf16()))
}

struct Request {
    runtime: PathBuf,
    mode: Mode,
}

enum Mode {
    Probe,
    Observations,
    Capture {
        model: PathBuf,
        evidence: PathBuf,
        adapter: PathBuf,
    },
}

impl Request {
    fn parse(arguments: &[OsString]) -> io::Result<Self> {
        require(
            matches!(arguments.len(), 3 | 7),
            "usage: --abi-probe RUNTIME | capture RUNTIME PTQ1 EVIDENCE ADAPTER 1.3",
        )?;
        let mode = if arguments[1] == "--abi-probe" && arguments.len() == 3 {
            Mode::Probe
        } else if arguments[1] == "--abi-observations" && arguments.len() == 3 {
            Mode::Observations
        } else {
            require(
                arguments.len() == 7 && arguments[1] == "capture",
                "invalid capture mode",
            )?;
            let model = PathBuf::from(&arguments[3]);
            let evidence = PathBuf::from(&arguments[4]);
            let adapter = PathBuf::from(&arguments[5]);
            require(
                absolute(model.as_os_str()) && basename_is(model.as_os_str(), MODEL_NAME),
                "exact current-weight PTQ1 filename and absolute path required",
            )?;
            require(
                absolute(evidence.as_os_str()) && evidence.exists(),
                "existing absolute evidence directory required",
            )?;
            require(
                absolute(adapter.as_os_str()) && basename_is(adapter.as_os_str(), ADAPTER_NAME),
                "approved absolute adapter path required",
            )?;
            require(
                arguments[6] == "1.3",
                "only the fixed adapter 1.3 smoke is supported",
            )?;
            Mode::Capture {
                model,
                evidence,
                adapter,
            }
        };
        let runtime = PathBuf::from(&arguments[2]);
        require(
            absolute(runtime.as_os_str()) && basename_is(runtime.as_os_str(), COMMIT),
            "exact absolute pinned runtime directory required",
        )?;
        Ok(Self { runtime, mode })
    }
}

fn token_record(
    position: usize,
    token: i32,
    state_bytes: usize,
    logits: &[f32],
) -> io::Result<String> {
    require(logits.len() == VOCABULARY, "native vocabulary mismatch")?;
    let mut greedy = 0;
    for (index, &value) in logits.iter().enumerate() {
        require(value.is_finite(), "nonfinite native logit")?;
        if value > logits[greedy] {
            greedy = index;
        }
    }
    Ok(format!(
        "{{\"position\":{position},\"inputToken\":{token},\"greedyToken\":{greedy},\"logitsCount\":248320,\"sequenceStateBytes\":{state_bytes}}}"
    ))
}

fn report(record: &str) -> String {
    format!(
        "{{\"schemaVersion\":1,\"nativeCommit\":\"73bfe6cc485554822f80c473a3c0c4f25e80f41b\",\"modelSha256\":\"e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5\",\"adapterLoaded\":true,\"adapterScale\":1.3,\"contextTokens\":8192,\"threads\":4,\"stateFormat\":\"pinned-llama_state_seq_get_data-sequence-zero\",\"writerBasisQualified\":false,\"numericalParityQualified\":false,\"steps\":[{record}]}}\n"
    )
}

#[allow(
    unsafe_code,
    ffi_unwind_calls,
    reason = "Confine the pinned DLL ABI and Windows handles; every native C++ call is contained by native_call."
)]
mod native {
    use std::{
        ffi::{CStr, CString, c_char, c_void},
        io::{self, Write as _},
        marker::PhantomData,
        mem::{align_of, offset_of, size_of, size_of_val},
        os::windows::ffi::OsStrExt as _,
        panic::{AssertUnwindSafe, catch_unwind},
        path::Path,
        ptr::NonNull,
    };

    use super::{INPUT_TOKEN, MAX_STATE, VOCABULARY, report, require, token_record};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ModelParams {
        dspark_head_source: *const c_void,
        devices: *mut *mut c_void,
        tensor_buft_overrides: *const c_void,
        n_gpu_layers: i32,
        split_mode: i32,
        load_mode: i32,
        main_gpu: i32,
        tensor_split: *const f32,
        progress_callback: Option<unsafe extern "C-unwind" fn(f32, *mut c_void) -> bool>,
        progress_callback_user_data: *mut c_void,
        kv_overrides: *const c_void,
        vocab_only: bool,
        check_tensors: bool,
        use_extra_bufts: bool,
        no_host: bool,
        no_alloc: bool,
        load_mtp: bool,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ContextParams {
        n_ctx: u32,
        n_batch: u32,
        n_ubatch: u32,
        n_seq_max: u32,
        n_rs_seq: u32,
        n_outputs_max: u32,
        n_outputs_max_per_seq: u32,
        n_threads: i32,
        n_threads_batch: i32,
        ctx_type: i32,
        rope_scaling_type: i32,
        pooling_type: i32,
        attention_type: i32,
        flash_attn_type: i32,
        rope_freq_base: f32,
        rope_freq_scale: f32,
        yarn_ext_factor: f32,
        yarn_attn_factor: f32,
        yarn_beta_fast: f32,
        yarn_beta_slow: f32,
        yarn_orig_ctx: u32,
        defrag_thold: f32,
        cb_eval: Option<unsafe extern "C-unwind" fn(*mut c_void, bool, *mut c_void) -> bool>,
        cb_eval_user_data: *mut c_void,
        type_k: i32,
        type_v: i32,
        path_kv_mean_center: *const c_char,
        abort_callback: Option<unsafe extern "C-unwind" fn(*mut c_void) -> bool>,
        abort_callback_data: *mut c_void,
        embeddings: bool,
        offload_kqv: bool,
        no_perf: bool,
        op_offload: bool,
        swa_full: bool,
        kv_unified: bool,
        samplers: *mut c_void,
        n_samplers: usize,
        ctx_other: *mut c_void,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Batch {
        n_tokens: i32,
        token: *mut i32,
        embd: *mut f32,
        pos: *mut i32,
        n_seq_id: *mut i32,
        seq_id: *mut *mut i32,
        logits: *mut i8,
    }

    const _: () = {
        assert!(size_of::<ModelParams>() == 80);
        assert!(size_of::<ContextParams>() == 168);
        assert!(size_of::<Batch>() == 56);
    };

    type ModelDefaults = unsafe extern "C-unwind" fn() -> ModelParams;
    type ContextDefaults = unsafe extern "C-unwind" fn() -> ContextParams;
    type NoArgs = unsafe extern "C-unwind" fn();
    type Release = unsafe extern "C-unwind" fn(*mut c_void);
    type LoadModel = unsafe extern "C-unwind" fn(*const c_char, ModelParams) -> *mut c_void;
    type InitContext = unsafe extern "C-unwind" fn(*mut c_void, ContextParams) -> *mut c_void;
    type Vocab = unsafe extern "C-unwind" fn(*const c_void) -> *const c_void;
    type VocabCount = unsafe extern "C-unwind" fn(*const c_void) -> i32;
    type LoadAdapter = unsafe extern "C-unwind" fn(*mut c_void, *const c_char) -> *mut c_void;
    type SetAdapter =
        unsafe extern "C-unwind" fn(*mut c_void, *mut *mut c_void, usize, *mut f32) -> i32;
    type OneBatch = unsafe extern "C-unwind" fn(*mut i32, i32) -> Batch;
    type Decode = unsafe extern "C-unwind" fn(*mut c_void, Batch) -> i32;
    type Logits = unsafe extern "C-unwind" fn(*mut c_void, i32) -> *mut f32;
    type StateSize = unsafe extern "C-unwind" fn(*mut c_void, i32) -> usize;
    type StateData = unsafe extern "C-unwind" fn(*mut c_void, *mut u8, usize, i32) -> usize;
    type LoadBackend = unsafe extern "C-unwind" fn(*const c_char) -> *mut c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryExW(path: *const u16, reserved: *mut c_void, flags: u32) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
        fn FreeLibrary(module: *mut c_void) -> i32;
        fn CreateFileW(
            path: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            creation: u32,
            flags: u32,
            template: *mut c_void,
        ) -> *mut c_void;
        fn WriteFile(
            file: *mut c_void,
            buffer: *const c_void,
            count: u32,
            written: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    // Foreign exceptions may abort inside catch_unwind or return an opaque Err.
    // The latter also aborts without inspecting/dropping that foreign payload.
    fn native_call<T>(call: impl FnOnce() -> T) -> T {
        catch_unwind(AssertUnwindSafe(call)).unwrap_or_else(|_| std::process::abort())
    }

    fn wide(path: &Path) -> io::Result<Vec<u16>> {
        let units: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
        require(
            !units[..units.len() - 1].contains(&0),
            "invalid or oversized UTF-16 path",
        )?;
        Ok(units)
    }

    fn utf8(path: &Path) -> io::Result<CString> {
        let text = path
            .to_str()
            .ok_or_else(|| io::Error::other("invalid or oversized UTF-16 path"))?;
        require(text.len() < 32_768, "invalid or oversized UTF-16 path")?;
        CString::new(text).map_err(|_| io::Error::other("UTF-8 conversion failed"))
    }

    struct Library(NonNull<c_void>);

    impl Library {
        fn load(path: &Path) -> io::Result<Self> {
            let path = wide(path)?;
            // SAFETY: The absolute pinned path is NUL-terminated. Search is
            // limited to that DLL directory and System32 (0x100 | 0x800).
            let pointer = unsafe { LoadLibraryExW(path.as_ptr(), std::ptr::null_mut(), 0x900) };
            Ok(Self(NonNull::new(pointer).ok_or_else(|| {
                io::Error::other("pinned DLL initialization failed")
            })?))
        }

        fn address(&self, name: &CStr) -> io::Result<*mut c_void> {
            // SAFETY: This guard retains the module and the name is a static
            // NUL-terminated symbol from the exact pinned public C header.
            let pointer = unsafe { GetProcAddress(self.0.as_ptr(), name.as_ptr()) };
            require(!pointer.is_null(), "required pinned DLL symbol missing")?;
            Ok(pointer)
        }
    }

    impl Drop for Library {
        fn drop(&mut self) {
            // SAFETY: This guard owns one module reference. Borrowed function
            // pointers and all native model resources have already been dropped.
            unsafe { FreeLibrary(self.0.as_ptr()) };
        }
    }

    macro_rules! symbol {
        ($library:expr, $name:expr, $kind:ty) => {{
            let address = $library.address($name)?;
            // SAFETY: The symbol's exact C header signature is represented by
            // this function-pointer type; the Api lifetime retains its DLL.
            unsafe { std::mem::transmute::<*mut c_void, $kind>(address) }
        }};
    }

    struct Defaults<'dll> {
        model: ModelParams,
        context: ContextParams,
        one_batch: OneBatch,
        _library: PhantomData<&'dll Library>,
    }

    impl<'dll> Defaults<'dll> {
        fn load(llama: &'dll Library) -> io::Result<Self> {
            let model = symbol!(llama, c"llama_model_default_params", ModelDefaults);
            let context = symbol!(llama, c"llama_context_default_params", ContextDefaults);
            let one_batch = symbol!(llama, c"llama_batch_get_one", OneBatch);
            // SAFETY: Both functions return the exact source-bound repr(C)
            // structures by value. They require no model/backend initialization.
            let model = native_call(|| unsafe { model() });
            // SAFETY: Same pinned by-value ABI, with no input/lifetime obligations.
            let context = native_call(|| unsafe { context() });
            require(
                model.dspark_head_source.is_null() && context.cb_eval.is_none(),
                "native default parameters differ from expected source ABI",
            )?;
            Ok(Self {
                model,
                context,
                one_batch,
                _library: PhantomData,
            })
        }
    }

    struct Api<'dll> {
        backend_init: NoArgs,
        backend_free: NoArgs,
        load_model: LoadModel,
        free_model: Release,
        init_context: InitContext,
        free_context: Release,
        vocab: Vocab,
        vocab_count: VocabCount,
        load_adapter: LoadAdapter,
        free_adapter: Release,
        set_adapter: SetAdapter,
        decode: Decode,
        logits: Logits,
        state_size: StateSize,
        state_data: StateData,
        load_backend: LoadBackend,
        _libraries: PhantomData<(&'dll Library, &'dll Library)>,
    }

    impl<'dll> Api<'dll> {
        fn load(llama: &'dll Library, ggml: &'dll Library) -> io::Result<Self> {
            Ok(Self {
                backend_init: symbol!(llama, c"llama_backend_init", NoArgs),
                backend_free: symbol!(llama, c"llama_backend_free", NoArgs),
                load_model: symbol!(llama, c"llama_model_load_from_file", LoadModel),
                free_model: symbol!(llama, c"llama_model_free", Release),
                init_context: symbol!(llama, c"llama_init_from_model", InitContext),
                free_context: symbol!(llama, c"llama_free", Release),
                vocab: symbol!(llama, c"llama_model_get_vocab", Vocab),
                vocab_count: symbol!(llama, c"llama_vocab_n_tokens", VocabCount),
                load_adapter: symbol!(llama, c"llama_adapter_lora_init", LoadAdapter),
                free_adapter: symbol!(llama, c"llama_adapter_lora_free", Release),
                set_adapter: symbol!(llama, c"llama_set_adapters_lora", SetAdapter),
                decode: symbol!(llama, c"llama_decode", Decode),
                logits: symbol!(llama, c"llama_get_logits_ith", Logits),
                state_size: symbol!(llama, c"llama_state_seq_get_size", StateSize),
                state_data: symbol!(llama, c"llama_state_seq_get_data", StateData),
                load_backend: symbol!(ggml, c"ggml_backend_load", LoadBackend),
                _libraries: PhantomData,
            })
        }
    }

    struct Backend<'api> {
        release: NoArgs,
        _api: PhantomData<&'api ()>,
    }

    impl Drop for Backend<'_> {
        fn drop(&mut self) {
            // SAFETY: The matching backend was initialized once. All context,
            // adapter and model guards have completed before this release.
            native_call(|| unsafe { (self.release)() });
        }
    }

    struct Owned<'api> {
        pointer: NonNull<c_void>,
        release: Release,
        _api: PhantomData<&'api ()>,
    }

    impl<'api> Owned<'api> {
        fn new(
            pointer: *mut c_void,
            release: Release,
            _api: &'api Api<'_>,
            error: &'static str,
        ) -> io::Result<Self> {
            Ok(Self {
                pointer: NonNull::new(pointer).ok_or_else(|| io::Error::other(error))?,
                release,
                _api: PhantomData,
            })
        }
    }

    impl Drop for Owned<'_> {
        fn drop(&mut self) {
            // SAFETY: This guard uniquely owns one live object from its paired
            // API allocator; declaration order releases dependent objects first.
            native_call(|| unsafe { (self.release)(self.pointer.as_ptr()) });
        }
    }

    struct File(Option<NonNull<c_void>>);

    impl File {
        fn close(mut self) -> bool {
            let Some(handle) = self.0.take() else {
                return false;
            };
            // SAFETY: This takes and closes the guard's unique live handle once.
            unsafe { CloseHandle(handle.as_ptr()) != 0 }
        }
    }

    impl Drop for File {
        fn drop(&mut self) {
            if let Some(handle) = self.0.take() {
                // SAFETY: Error/unwind cleanup returns this guard's unique live
                // file handle once; ordinary paths check close explicitly.
                unsafe { CloseHandle(handle.as_ptr()) };
            }
        }
    }

    fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
        require(
            bytes.len() <= MAX_STATE,
            "snapshot exceeds fixed byte bound",
        )?;
        let path = wide(path)?;
        // SAFETY: The NUL-terminated path is live; GENERIC_WRITE, FILE_SHARE_READ,
        // CREATE_NEW and FILE_ATTRIBUTE_NORMAL preserve original file semantics.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                0x4000_0000,
                1,
                std::ptr::null_mut(),
                1,
                0x80,
                std::ptr::null_mut(),
            )
        };
        require(
            handle != std::ptr::without_provenance_mut::<c_void>(usize::MAX),
            "snapshot already exists or cannot be created",
        )?;
        let file = File(Some(NonNull::new(handle).ok_or_else(|| {
            io::Error::other("snapshot already exists or cannot be created")
        })?));
        let mut success = true;
        for chunk in bytes.chunks(1_048_576) {
            let count = u32::try_from(chunk.len()).map_err(io::Error::other)?;
            let mut written = 0;
            // SAFETY: The file handle is uniquely retained and chunk is a live
            // initialized read-only span of count bytes for this blocking write.
            let result = unsafe {
                WriteFile(
                    handle,
                    chunk.as_ptr().cast(),
                    count,
                    &raw mut written,
                    std::ptr::null_mut(),
                )
            };
            if result == 0 || written != count {
                success = false;
                break;
            }
        }
        let closed = file.close();
        require(
            success && closed,
            "snapshot write failed; partial evidence retained",
        )
    }

    fn model_settings(mut parameters: ModelParams) -> ModelParams {
        parameters.n_gpu_layers = 0;
        parameters.load_mode = 1; // LLAMA_LOAD_MODE_MMAP
        parameters.use_extra_bufts = false;
        parameters.load_mtp = false;
        parameters
    }

    fn context_settings(mut parameters: ContextParams) -> ContextParams {
        parameters.n_ctx = 8192;
        parameters.n_batch = 128;
        parameters.n_ubatch = 64;
        parameters.n_seq_max = 1;
        parameters.n_rs_seq = 0;
        parameters.n_threads = 4;
        parameters.n_threads_batch = 4;
        parameters.type_k = 1; // GGML_TYPE_F16
        parameters.type_v = 1;
        parameters.flash_attn_type = 0; // LLAMA_FLASH_ATTN_TYPE_DISABLED
        parameters.offload_kqv = false;
        parameters.op_offload = false;
        parameters.no_perf = true;
        parameters
    }

    fn capture(
        api: &Api<'_>,
        defaults: &Defaults<'_>,
        runtime: &Path,
        model_path: &Path,
        evidence: &Path,
        adapter_path: &Path,
        output: &mut impl io::Write,
    ) -> io::Result<()> {
        // Retain all CStrings until native context/model destruction: the pinned
        // loader may retain path pointers rather than making independent copies.
        let cpu_path = utf8(&runtime.join("ggml-cpu.dll"))?;
        let model_path = utf8(model_path)?;
        let adapter_path = utf8(adapter_path)?;
        // SAFETY: The complete verified absolute CPU DLL path is NUL-terminated
        // and retained throughout backend/model lifetime.
        let backend = native_call(|| unsafe { (api.load_backend)(cpu_path.as_ptr()) });
        require(!backend.is_null(), "exact CPU backend registration failed")?;
        // SAFETY: Initialize once before any model operation on this process.
        native_call(|| unsafe { (api.backend_init)() });
        let _backend = Backend {
            release: api.backend_free,
            _api: PhantomData,
        };
        // SAFETY: The live path and exact source-bound by-value parameters match
        // this pinned loader; the returned object is transferred to one guard.
        let model = native_call(|| unsafe {
            (api.load_model)(model_path.as_ptr(), model_settings(defaults.model))
        });
        let model = Owned::new(model, api.free_model, api, "native model loading failed")?;
        // SAFETY: The live model owns its immutable vocabulary until model drop.
        let vocabulary = native_call(|| unsafe { (api.vocab)(model.pointer.as_ptr()) });
        require(!vocabulary.is_null(), "native vocabulary mismatch")?;
        // SAFETY: The vocabulary pointer is nonnull, live and model-owned.
        let count = native_call(|| unsafe { (api.vocab_count)(vocabulary) });
        require(count == 248_320, "native vocabulary mismatch")?;
        // SAFETY: This model and the retained adapter path remain live until
        // after the adapter's matching release.
        let adapter = native_call(|| unsafe {
            (api.load_adapter)(model.pointer.as_ptr(), adapter_path.as_ptr())
        });
        let adapter = Owned::new(
            adapter,
            api.free_adapter,
            api,
            "approved native adapter loading failed",
        )?;
        // SAFETY: The live model and exact by-value context parameters follow
        // the pinned header ABI; one guard owns the returned context.
        let context = native_call(|| unsafe {
            (api.init_context)(model.pointer.as_ptr(), context_settings(defaults.context))
        });
        let context = Owned::new(
            context,
            api.free_context,
            api,
            "native context allocation failed",
        )?;
        let mut pointer = adapter.pointer.as_ptr();
        let mut scale = 1.3_f32;
        // SAFETY: One live adapter pointer and its one matching scale are
        // borrowed synchronously; both outlive all subsequent context work.
        let applied = native_call(|| unsafe {
            (api.set_adapter)(
                context.pointer.as_ptr(),
                &raw mut pointer,
                1,
                &raw mut scale,
            )
        });
        require(applied == 0, "native adapter application failed")?;
        let mut token = INPUT_TOKEN;
        // SAFETY: The batch borrows this one live token. The following decode
        // consumes it synchronously without transferring/freeing its storage.
        let batch = native_call(|| unsafe { (defaults.one_batch)(&raw mut token, 1) });
        // SAFETY: The exclusive context and complete one-token batch remain live
        // for the synchronous call, with no concurrent native operation.
        let decoded = native_call(|| unsafe { (api.decode)(context.pointer.as_ptr(), batch) });
        require(decoded == 0, "native decode failed")?;
        // SAFETY: A successful one-token decode provides one model-vocabulary
        // logits row; it stays valid until the next decode/context destruction.
        let logits = native_call(|| unsafe { (api.logits)(context.pointer.as_ptr(), -1) });
        require(
            !logits.is_null() && logits.is_aligned(),
            "native logits missing",
        )?;
        // SAFETY: The model vocabulary was checked exactly. No decode or context
        // mutation occurs while the row is read and written to evidence.
        let logits = unsafe { std::slice::from_raw_parts(logits, VOCABULARY) };
        // SAFETY: Query complete committed sequence zero on the live context.
        let size = native_call(|| unsafe { (api.state_size)(context.pointer.as_ptr(), 0) });
        require(
            size > 0 && size <= MAX_STATE,
            "native sequence-state byte count exceeds bound",
        )?;
        let mut state = Vec::new();
        state.try_reserve_exact(size).map_err(io::Error::other)?;
        state.resize(size, 0);
        // SAFETY: The exclusive destination covers exactly the checked state
        // extent and remains live throughout synchronous serialization.
        let written = native_call(|| unsafe {
            (api.state_data)(context.pointer.as_ptr(), state.as_mut_ptr(), state.len(), 0)
        });
        require(
            written == size,
            "native sequence-state serialization truncated",
        )?;
        let record = token_record(0, token, size, logits)?;
        // SAFETY: The read-only row contains VOCABULARY initialized f32s. On
        // the explicitly supported little-endian target this is exact f32le.
        let bytes =
            unsafe { std::slice::from_raw_parts(logits.as_ptr().cast(), size_of_val(logits)) };
        write_new(&evidence.join("logits-0.f32le"), bytes)?;
        write_new(&evidence.join("state-0.native-seq"), &state)?;
        writeln!(output, "{record}")?;
        output.flush()?;
        write_new(
            &evidence.join("native-capture.json"),
            report(&record).as_bytes(),
        )
    }

    pub(super) fn run(request: &super::Request, output: &mut impl io::Write) -> io::Result<()> {
        // Declaration order deliberately unloads llama, CPU, ggml, then base,
        // after capture's context/adapter/model/backend guards have finished.
        let _base = Library::load(&request.runtime.join("ggml-base.dll"))?;
        let ggml = Library::load(&request.runtime.join("ggml.dll"))?;
        let _cpu = Library::load(&request.runtime.join("ggml-cpu.dll"))?;
        let llama = Library::load(&request.runtime.join("llama.dll"))?;
        let defaults = Defaults::load(&llama)?;
        match &request.mode {
            super::Mode::Probe => writeln!(
                output,
                "{{\"status\":\"native-default-parameter-abi-probe-passed\",\"modelParamsBytes\":{},\"contextParamsBytes\":{},\"batchBytes\":{},\"modelLoaded\":false}}",
                size_of::<ModelParams>(),
                size_of::<ContextParams>(),
                size_of::<Batch>(),
            ),
            super::Mode::Observations => output.write_all(observations(&defaults).as_bytes()),
            super::Mode::Capture {
                model,
                evidence,
                adapter,
            } => {
                let api = Api::load(&llama, &ggml)?;
                capture(
                    &api,
                    &defaults,
                    &request.runtime,
                    model,
                    evidence,
                    adapter,
                    output,
                )
            }
        }
    }

    fn observations(defaults: &Defaults<'_>) -> String {
        let mut output = String::new();
        let mut value = |key: &str, number: u64| {
            output.push_str(&format!("{key}={number}\n"));
        };
        for (name, size, alignment) in [
            ("model", size_of::<ModelParams>(), align_of::<ModelParams>()),
            (
                "context",
                size_of::<ContextParams>(),
                align_of::<ContextParams>(),
            ),
            ("batch", size_of::<Batch>(), align_of::<Batch>()),
        ] {
            value(&format!("{name}.size"), usize_bits(size));
            value(&format!("{name}.align"), usize_bits(alignment));
        }
        macro_rules! fields {
            ($name:literal, $kind:ty, $instance:expr, [$($field:ident),* $(,)?]) => {$(
                value(concat!($name, ".offset.", stringify!($field)), usize_bits(offset_of!($kind, $field)));
                value(concat!($name, ".width.", stringify!($field)), usize_bits(size_of_val(&$instance.$field)));
            )*};
        }
        fields!(
            "model",
            ModelParams,
            defaults.model,
            [
                dspark_head_source,
                devices,
                tensor_buft_overrides,
                n_gpu_layers,
                split_mode,
                load_mode,
                main_gpu,
                tensor_split,
                progress_callback,
                progress_callback_user_data,
                kv_overrides,
                vocab_only,
                check_tensors,
                use_extra_bufts,
                no_host,
                no_alloc,
                load_mtp,
            ]
        );
        fields!(
            "context",
            ContextParams,
            defaults.context,
            [
                n_ctx,
                n_batch,
                n_ubatch,
                n_seq_max,
                n_rs_seq,
                n_outputs_max,
                n_outputs_max_per_seq,
                n_threads,
                n_threads_batch,
                ctx_type,
                rope_scaling_type,
                pooling_type,
                attention_type,
                flash_attn_type,
                rope_freq_base,
                rope_freq_scale,
                yarn_ext_factor,
                yarn_attn_factor,
                yarn_beta_fast,
                yarn_beta_slow,
                yarn_orig_ctx,
                defrag_thold,
                cb_eval,
                cb_eval_user_data,
                type_k,
                type_v,
                path_kv_mean_center,
                abort_callback,
                abort_callback_data,
                embeddings,
                offload_kqv,
                no_perf,
                op_offload,
                swa_full,
                kv_unified,
                samplers,
                n_samplers,
                ctx_other,
            ]
        );
        let mut token = INPUT_TOKEN;
        // SAFETY: The returned batch borrows one live token until observations
        // are complete; no backend or model is loaded by this metadata probe.
        let batch = native_call(|| unsafe { (defaults.one_batch)(&raw mut token, 1) });
        fields!(
            "batch",
            Batch,
            batch,
            [n_tokens, token, embd, pos, n_seq_id, seq_id, logits]
        );
        append_parameters(&mut value, defaults, "default");
        value("batch.one.n_tokens", signed_bits(batch.n_tokens));
        value(
            "batch.one.token_is_input",
            u64::from(batch.token == &raw mut token),
        );
        // The fixed input's own value proves signed token handoff without
        // dereferencing a foreign pointer if a broken ABI returns the wrong one.
        value("batch.one.token0", signed_bits(token));
        for (name, null) in [
            ("embd", batch.embd.is_null()),
            ("pos", batch.pos.is_null()),
            ("n_seq_id", batch.n_seq_id.is_null()),
            ("seq_id", batch.seq_id.is_null()),
            ("logits", batch.logits.is_null()),
        ] {
            value(&format!("batch.one.{name}_null"), u64::from(null));
        }
        append_types(&mut value);
        append_parameters(
            &mut value,
            &Defaults {
                model: model_settings(defaults.model),
                context: context_settings(defaults.context),
                one_batch: defaults.one_batch,
                _library: PhantomData,
            },
            "configured",
        );
        value("config.input_token", signed_bits(INPUT_TOKEN));
        value("config.adapter_scale", u64::from(1.3_f32.to_bits()));
        value("config.vocabulary", usize_bits(VOCABULARY));
        value("config.max_state_bytes", usize_bits(MAX_STATE));
        let mut lines: Vec<_> = output.lines().collect();
        lines.sort_unstable();
        lines.join("\n") + "\n"
    }

    fn signed_bits(value: i32) -> u64 {
        u64::from_le_bytes(i64::from(value).to_le_bytes())
    }

    const fn usize_bits(value: usize) -> u64 {
        u64::from_le_bytes(value.to_le_bytes())
    }

    fn append_parameters(value: &mut impl FnMut(&str, u64), defaults: &Defaults<'_>, kind: &str) {
        macro_rules! signed {
            ($prefix:literal, $instance:expr, [$($field:ident),* $(,)?]) => {$(
                value(&format!("{}.{kind}.{}", $prefix, stringify!($field)), signed_bits($instance.$field));
            )*};
        }
        macro_rules! unsigned {
            ($prefix:literal, $instance:expr, [$($field:ident),* $(,)?]) => {$(
                value(&format!("{}.{kind}.{}", $prefix, stringify!($field)), u64::from($instance.$field));
            )*};
        }
        macro_rules! pointer {
            ($prefix:literal, $instance:expr, [$($field:ident),* $(,)?]) => {$(
                value(&format!("{}.{kind}_null.{}", $prefix, stringify!($field)), u64::from($instance.$field.is_null()));
            )*};
        }
        signed!(
            "model",
            defaults.model,
            [n_gpu_layers, split_mode, load_mode, main_gpu]
        );
        unsigned!(
            "model",
            defaults.model,
            [
                vocab_only,
                check_tensors,
                use_extra_bufts,
                no_host,
                no_alloc,
                load_mtp
            ]
        );
        pointer!(
            "model",
            defaults.model,
            [
                dspark_head_source,
                devices,
                tensor_buft_overrides,
                tensor_split,
                progress_callback_user_data,
                kv_overrides
            ]
        );
        value(
            &format!("model.{kind}_null.progress_callback"),
            u64::from(defaults.model.progress_callback.is_none()),
        );
        unsigned!(
            "context",
            defaults.context,
            [
                n_ctx,
                n_batch,
                n_ubatch,
                n_seq_max,
                n_rs_seq,
                n_outputs_max,
                n_outputs_max_per_seq,
                yarn_orig_ctx,
                embeddings,
                offload_kqv,
                no_perf,
                op_offload,
                swa_full,
                kv_unified
            ]
        );
        signed!(
            "context",
            defaults.context,
            [
                n_threads,
                n_threads_batch,
                ctx_type,
                rope_scaling_type,
                pooling_type,
                attention_type,
                flash_attn_type,
                type_k,
                type_v
            ]
        );
        pointer!(
            "context",
            defaults.context,
            [
                cb_eval_user_data,
                path_kv_mean_center,
                abort_callback_data,
                samplers,
                ctx_other
            ]
        );
        value(
            &format!("context.{kind}_null.cb_eval"),
            u64::from(defaults.context.cb_eval.is_none()),
        );
        value(
            &format!("context.{kind}_null.abort_callback"),
            u64::from(defaults.context.abort_callback.is_none()),
        );
        value(
            &format!("context.{kind}.n_samplers"),
            usize_bits(defaults.context.n_samplers),
        );
        macro_rules! floats {
            ($($field:ident),* $(,)?) => {$(
                value(&format!("context.{kind}.{}", stringify!($field)), u64::from(defaults.context.$field.to_bits()));
            )*};
        }
        floats!(
            rope_freq_base,
            rope_freq_scale,
            yarn_ext_factor,
            yarn_attn_factor,
            yarn_beta_fast,
            yarn_beta_slow,
            defrag_thold
        );
    }

    fn append_types(value: &mut impl FnMut(&str, u64)) {
        macro_rules! scalar {
            ($name:literal, $kind:ty, $signed:expr) => {
                value(
                    concat!("type.", $name, ".size"),
                    usize_bits(size_of::<$kind>()),
                );
                value(
                    concat!("type.", $name, ".align"),
                    usize_bits(align_of::<$kind>()),
                );
                value(concat!("type.", $name, ".signed"), $signed);
            };
        }
        scalar!("i8", i8, 1);
        scalar!("i32", i32, 1);
        scalar!("u32", u32, 0);
        scalar!("i64", i64, 1);
        scalar!("u64", u64, 0);
        scalar!("usize", usize, 0);
        scalar!("bool", bool, 0);
        scalar!("f32", f32, 0);
        scalar!("llama_token", i32, 1);
        scalar!("llama_pos", i32, 1);
        scalar!("llama_seq_id", i32, 1);

        // Exact integer constants from the source-bound public C enums.
        value("enum.llama_split_mode.size", usize_bits(size_of::<i32>()));
        value("enum.llama_split_mode.signed", 1);
        value("enum.LLAMA_SPLIT_MODE_NONE", signed_bits(0));
        value("enum.LLAMA_SPLIT_MODE_LAYER", signed_bits(1));
        value("enum.LLAMA_SPLIT_MODE_ROW", signed_bits(2));
        value("enum.LLAMA_SPLIT_MODE_TENSOR", signed_bits(3));
        value("enum.llama_load_mode.size", usize_bits(size_of::<i32>()));
        value("enum.llama_load_mode.signed", 1);
        value("enum.LLAMA_LOAD_MODE_AUTO", signed_bits(-1));
        value("enum.LLAMA_LOAD_MODE_NONE", signed_bits(0));
        value("enum.LLAMA_LOAD_MODE_MMAP", signed_bits(1));
        value("enum.LLAMA_LOAD_MODE_MLOCK", signed_bits(2));
        value("enum.LLAMA_LOAD_MODE_MMAP_MLOCK", signed_bits(3));
        value("enum.LLAMA_LOAD_MODE_DIRECT_IO", signed_bits(4));
        value("enum.llama_context_type.size", usize_bits(size_of::<i32>()));
        value("enum.llama_context_type.signed", 1);
        value("enum.LLAMA_CONTEXT_TYPE_DEFAULT", signed_bits(0));
        value("enum.LLAMA_CONTEXT_TYPE_MTP", signed_bits(1));
        value(
            "enum.llama_rope_scaling_type.size",
            usize_bits(size_of::<i32>()),
        );
        value("enum.llama_rope_scaling_type.signed", 1);
        value("enum.LLAMA_ROPE_SCALING_TYPE_UNSPECIFIED", signed_bits(-1));
        value("enum.LLAMA_ROPE_SCALING_TYPE_NONE", signed_bits(0));
        value("enum.LLAMA_ROPE_SCALING_TYPE_LINEAR", signed_bits(1));
        value("enum.LLAMA_ROPE_SCALING_TYPE_YARN", signed_bits(2));
        value("enum.LLAMA_ROPE_SCALING_TYPE_LONGROPE", signed_bits(3));
        value("enum.LLAMA_ROPE_SCALING_TYPE_MAX_VALUE", signed_bits(3));
        value("enum.llama_pooling_type.size", usize_bits(size_of::<i32>()));
        value("enum.llama_pooling_type.signed", 1);
        value("enum.LLAMA_POOLING_TYPE_UNSPECIFIED", signed_bits(-1));
        value("enum.LLAMA_POOLING_TYPE_NONE", signed_bits(0));
        value("enum.LLAMA_POOLING_TYPE_MEAN", signed_bits(1));
        value("enum.LLAMA_POOLING_TYPE_CLS", signed_bits(2));
        value("enum.LLAMA_POOLING_TYPE_LAST", signed_bits(3));
        value("enum.LLAMA_POOLING_TYPE_RANK", signed_bits(4));
        value(
            "enum.llama_attention_type.size",
            usize_bits(size_of::<i32>()),
        );
        value("enum.llama_attention_type.signed", 1);
        value("enum.LLAMA_ATTENTION_TYPE_UNSPECIFIED", signed_bits(-1));
        value("enum.LLAMA_ATTENTION_TYPE_CAUSAL", signed_bits(0));
        value("enum.LLAMA_ATTENTION_TYPE_NON_CAUSAL", signed_bits(1));
        value(
            "enum.llama_flash_attn_type.size",
            usize_bits(size_of::<i32>()),
        );
        value("enum.llama_flash_attn_type.signed", 1);
        value("enum.LLAMA_FLASH_ATTN_TYPE_AUTO", signed_bits(-1));
        value("enum.LLAMA_FLASH_ATTN_TYPE_DISABLED", signed_bits(0));
        value("enum.LLAMA_FLASH_ATTN_TYPE_ENABLED", signed_bits(1));
        value("enum.ggml_type.size", usize_bits(size_of::<i32>()));
        value("enum.ggml_type.signed", 1);
        value("enum.GGML_TYPE_F32", signed_bits(0));
        value("enum.GGML_TYPE_F16", signed_bits(1));
        value("enum.GGML_TYPE_Q4_0", signed_bits(2));
        value("enum.GGML_TYPE_Q4_1", signed_bits(3));
        value("enum.GGML_TYPE_Q5_0", signed_bits(6));
        value("enum.GGML_TYPE_Q5_1", signed_bits(7));
        value("enum.GGML_TYPE_Q8_0", signed_bits(8));
        value("enum.GGML_TYPE_Q8_1", signed_bits(9));
        value("enum.GGML_TYPE_Q2_K", signed_bits(10));
        value("enum.GGML_TYPE_Q3_K", signed_bits(11));
        value("enum.GGML_TYPE_Q4_K", signed_bits(12));
        value("enum.GGML_TYPE_Q5_K", signed_bits(13));
        value("enum.GGML_TYPE_Q6_K", signed_bits(14));
        value("enum.GGML_TYPE_Q8_K", signed_bits(15));
        value("enum.GGML_TYPE_IQ2_XXS", signed_bits(16));
        value("enum.GGML_TYPE_IQ2_XS", signed_bits(17));
        value("enum.GGML_TYPE_IQ3_XXS", signed_bits(18));
        value("enum.GGML_TYPE_IQ1_S", signed_bits(19));
        value("enum.GGML_TYPE_IQ4_NL", signed_bits(20));
        value("enum.GGML_TYPE_IQ3_S", signed_bits(21));
        value("enum.GGML_TYPE_IQ2_S", signed_bits(22));
        value("enum.GGML_TYPE_IQ4_XS", signed_bits(23));
        value("enum.GGML_TYPE_I8", signed_bits(24));
        value("enum.GGML_TYPE_I16", signed_bits(25));
        value("enum.GGML_TYPE_I32", signed_bits(26));
        value("enum.GGML_TYPE_I64", signed_bits(27));
        value("enum.GGML_TYPE_F64", signed_bits(28));
        value("enum.GGML_TYPE_IQ1_M", signed_bits(29));
        value("enum.GGML_TYPE_BF16", signed_bits(30));
        value("enum.GGML_TYPE_TQ1_0", signed_bits(34));
        value("enum.GGML_TYPE_TQ2_0", signed_bits(35));
        value("enum.GGML_TYPE_MXFP4", signed_bits(39));
        value("enum.GGML_TYPE_NVFP4", signed_bits(40));
        value("enum.GGML_TYPE_Q1_0", signed_bits(41));
        value("enum.GGML_TYPE_Q2_0", signed_bits(42));
        value("enum.GGML_TYPE_PQ2_0", signed_bits(142));
        value("enum.GGML_TYPE_PTQ1_0", signed_bits(143));
        value("enum.GGML_TYPE_COUNT", signed_bits(144));
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn snapshot_creation_preserves_original_bytes_and_refuses_overwrite() -> io::Result<()> {
            let path = std::env::temp_dir().join(format!(
                "Bonsai-RustCollector-{}-{}.bin",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(io::Error::other)?
                    .as_nanos(),
            ));
            struct Cleanup(std::path::PathBuf);
            impl Drop for Cleanup {
                fn drop(&mut self) {
                    let _ = std::fs::remove_file(&self.0);
                }
            }
            let _cleanup = Cleanup(path.clone());
            let data: Vec<_> = (0_u32..1_048_577).flat_map(u32::to_le_bytes).collect();
            write_new(&path, &data)?;
            assert_eq!(std::fs::read(&path)?, data);
            assert!(write_new(&path, b"overwrite").is_err());
            assert_eq!(std::fs::read(&path)?, data);
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    let result = Request::parse(&std::env::args_os().collect::<Vec<_>>())
        .and_then(|request| native::run(&request, &mut io::stdout().lock()));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "Native correctness capture failed: {error}"
            );
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collector_rejects_unsupported_and_ambiguous_paths_before_loading() {
        for path in [
            "",
            "relative",
            "C:/",
            "C:/../runtime",
            "C:/%LOCALAPPDATA%/runtime",
        ] {
            assert!(!absolute(OsStr::new(path)));
        }
        assert!(absolute(OsStr::new("C:/runtime with spaces")));
        assert!(absolute(OsStr::new("C:\\runtime with spaces")));
        assert!(Request::parse(&["collector".into(), "--abi-probe".into()]).is_err());
        assert!(
            Request::parse(&[
                "collector".into(),
                "--abi-probe".into(),
                "C:/wrong-runtime".into(),
            ])
            .is_err()
        );
    }

    #[test]
    fn token_control_preserves_first_finite_maximum_and_exact_json() -> io::Result<()> {
        let mut logits = vec![f32::NEG_INFINITY; VOCABULARY];
        assert!(token_record(0, INPUT_TOKEN, 1, &logits).is_err());
        logits.fill(-0.0);
        logits[11] = 2.0;
        logits[12] = 2.0;
        let record = token_record(0, INPUT_TOKEN, 42, &logits)?;
        assert_eq!(
            record,
            "{\"position\":0,\"inputToken\":248045,\"greedyToken\":11,\"logitsCount\":248320,\"sequenceStateBytes\":42}"
        );
        assert!(report(&record).ends_with(&format!("\"steps\":[{record}]}}\n")));
        for invalid in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            logits[17] = invalid;
            assert!(token_record(0, INPUT_TOKEN, 1, &logits).is_err());
        }
        assert!(token_record(0, INPUT_TOKEN, 1, &[]).is_err());
        Ok(())
    }
}
