//! One-shot, model-free Hexagon access control for an exact reviewed runtime.
//!
//! This is a diagnostic client of the existing signed ggml package, not a native
//! Prism implementation or a production inference dependency. The supervising
//! wrapper must verify package/build digests, catalog membership, native ARM64,
//! the aggregate deadline and owned-process teardown before accepting a result.
//! There is no CPU scheduler, model loader, optional shape or performance loop.
//!
//! C ABI source: ggml-org/llama.cpp @
//! 94256114c229674ef96e76eb2dea596e65b43818, ggml/include/{ggml.h,
//! ggml-backend.h,ggml-alloc.h,ggml-hexagon.h}. The implementation exposes the
//! explicit Hexagon registry; the historical hexagon_init header declaration
//! has no corresponding definition at this source pin.

#![deny(warnings)]
#![deny(unsafe_op_in_unsafe_fn)]

use std::path::PathBuf;
use std::process::ExitCode;

const SOURCE_REVISION: &str = "94256114c229674ef96e76eb2dea596e65b43818";
const COLUMNS: usize = 128;
const ROWS: usize = 32;
const BATCHES: [usize; 2] = [1, 32];

#[derive(Clone, Copy)]
struct Failure {
    outcome: &'static str,
    stage: &'static str,
    code: &'static str,
    native_code: Option<u32>,
}

impl Failure {
    const fn pending(stage: &'static str, code: &'static str) -> Self {
        Self {
            outcome: "pending",
            stage,
            code,
            native_code: None,
        }
    }

    const fn failed(stage: &'static str, code: &'static str) -> Self {
        Self {
            outcome: "failed",
            stage,
            code,
            native_code: None,
        }
    }
}

struct CaseResult {
    batch: usize,
    on_hexagon: bool,
}

struct Receipt {
    self_test: bool,
    backend_verified: bool,
    cases: Vec<CaseResult>,
    failure: Option<Failure>,
}

impl Receipt {
    fn new(self_test: bool) -> Self {
        Self {
            self_test,
            backend_verified: false,
            cases: Vec::with_capacity(2),
            failure: None,
        }
    }

    fn write(&self) -> std::io::Result<()> {
        use std::io::Write as _;
        let (outcome, stage, failure, native_code) = self.failure.map_or(
            (
                "passed",
                if self.self_test {
                    "host-control-complete"
                } else {
                    "numeric-dispatch-complete"
                },
                "null".to_owned(),
                "null".to_owned(),
            ),
            |error| {
                (
                    error.outcome,
                    error.stage,
                    format!("\"{}\"", error.code),
                    error
                        .native_code
                        .map_or_else(|| "null".to_owned(), |code| code.to_string()),
                )
            },
        );
        let cases = self.cases.iter().map(|case| format!(
            "{{\"Columns\":{COLUMNS},\"Rows\":{ROWS},\"Batch\":{},\"OutputValuesChecked\":{},\"MatmulParity\":true,\"ExecutedOnHexagon\":{}}}",
            case.batch, ROWS * case.batch, case.on_hexagon,
        )).collect::<Vec<_>>().join(",");
        let dispatch_verified = !self.self_test
            && self.failure.is_none()
            && self.backend_verified
            && self.cases.len() == BATCHES.len()
            && self.cases.iter().all(|case| case.on_hexagon);
        let backend = if self.backend_verified {
            "\"HTP0\""
        } else {
            "null"
        };
        // Only fixed strings and bounded integers enter this one-line protocol.
        // Runtime/vendor diagnostics may use stderr; the wrapper must require
        // exactly one matching JSON receipt and reject ambiguous output.
        writeln!(
            std::io::stdout().lock(),
            "{{\"SchemaVersion\":1,\"Kind\":\"runner-hexagon-dispatch-fixture\",\"SourceRevision\":\"{SOURCE_REVISION}\",\"Outcome\":\"{outcome}\",\"Stage\":\"{stage}\",\"Failure\":{failure},\"NativeCode\":{native_code},\"SelfTest\":{},\"BackendName\":{backend},\"DeviceDispatchVerified\":{dispatch_verified},\"DispatchEvidence\":\"explicit-hexagon-compute-sync-and-full-output-oracle\",\"CpuFallbackAllowed\":false,\"ModelLoaded\":false,\"SwiftQualified\":false,\"ProfileEligible\":false,\"SessionCleanup\":\"supervised-process-exit-required\",\"Cases\":[{cases}]}}",
            self.self_test
        )
    }
}

fn fixture(batch: usize) -> (Vec<f32>, Vec<f32>) {
    let mut weights = vec![0.0; COLUMNS * ROWS];
    for row in 0..ROWS {
        for group in 0..4 {
            weights[row * COLUMNS + group * ROWS + row] = 1.0;
        }
    }
    let mut activations = vec![0.0; COLUMNS * batch];
    for token in 0..batch {
        for column in 0..COLUMNS {
            // All inputs and every possible accumulated output are small exact
            // integers in both FP16 and FP32. The row/token dependence differs
            // so a transposed square output does not accidentally pass.
            activations[token * COLUMNS + column] =
                ((column / ROWS + 1) * (token + 1) + column % ROWS) as f32;
        }
    }
    (weights, activations)
}

fn expected(row: usize, token: usize) -> f32 {
    // Independent closed form for the four nonzero coordinates of each row:
    // (1+2+3+4)*(token+1) + 4*row. No runtime matrix product is the oracle.
    (10 * (token + 1) + 4 * row) as f32
}

fn matches_reference(output: &[f32], batch: usize) -> bool {
    output.len() == ROWS * batch
        && output
            .iter()
            .enumerate()
            .all(|(index, value)| value.to_bits() == expected(index % ROWS, index / ROWS).to_bits())
}

fn self_test(receipt: &mut Receipt) -> Result<(), Failure> {
    for batch in BATCHES {
        let (weights, activations) = fixture(batch);
        let mut scalar = vec![0.0; ROWS * batch];
        for token in 0..batch {
            for row in 0..ROWS {
                let mut sum = 0.0;
                for column in 0..COLUMNS {
                    sum += weights[row * COLUMNS + column] * activations[token * COLUMNS + column];
                }
                scalar[token * ROWS + row] = sum;
            }
        }
        if !matches_reference(&scalar, batch) {
            return Err(Failure::failed(
                "host-control",
                "independent-oracle-mismatch",
            ));
        }
        if batch == ROWS {
            let transposed = (0..scalar.len())
                .map(|index| scalar[(index % ROWS) * ROWS + index / ROWS])
                .collect::<Vec<_>>();
            if matches_reference(&transposed, batch) {
                return Err(Failure::failed(
                    "host-control",
                    "transposed-output-accepted",
                ));
            }
        }
        scalar[0] += 1.0;
        if matches_reference(&scalar, batch) {
            return Err(Failure::failed("host-control", "corrupted-output-accepted"));
        }
        receipt.cases.push(CaseResult {
            batch,
            on_hexagon: false,
        });
    }
    #[cfg(all(windows, target_arch = "aarch64"))]
    device::check_abi()?;
    Ok(())
}

fn command() -> Result<Option<PathBuf>, Failure> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.len() == 1 && arguments[0] == "--self-test" {
        return Ok(None);
    }
    if arguments.len() == 2 && arguments[0] == "--runtime-directory" {
        return Ok(Some(PathBuf::from(&arguments[1])));
    }
    Err(Failure::pending("arguments", "invalid-command-line"))
}

fn main() -> ExitCode {
    let command = command();
    let mut receipt = Receipt::new(matches!(command, Ok(None)));
    let result = match command {
        Ok(None) => self_test(&mut receipt),
        Ok(Some(path)) => {
            #[cfg(all(windows, target_arch = "aarch64"))]
            {
                device::run(&path, &mut receipt)
            }
            #[cfg(not(all(windows, target_arch = "aarch64")))]
            {
                let _ = path;
                Err(Failure::pending(
                    "platform",
                    "native-windows-arm64-required",
                ))
            }
        }
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        receipt.failure = Some(error);
    }
    if receipt.write().is_err() {
        return ExitCode::FAILURE;
    }
    match receipt.failure {
        None => ExitCode::SUCCESS,
        Some(Failure {
            outcome: "pending", ..
        }) => ExitCode::from(2),
        Some(_) => ExitCode::FAILURE,
    }
}

#[cfg(all(windows, target_arch = "aarch64"))]
mod device {
    use super::{BATCHES, COLUMNS, CaseResult, Failure, ROWS, Receipt, fixture, matches_reference};
    use std::ffi::{c_char, c_int, c_void};
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::fs::MetadataExt as _;
    use std::path::Path;
    use std::ptr::{NonNull, null, null_mut};

    const LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR: u32 = 0x0000_0100;
    const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x0000_0800;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    const GGML_TYPE_F32: c_int = 0;
    const GGML_STATUS_SUCCESS: c_int = 0;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
        fn FreeLibrary(module: *mut c_void) -> c_int;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
        fn GetLastError() -> u32;
        fn SetDefaultDllDirectories(flags: u32) -> c_int;
    }

    #[repr(C)]
    struct InitParams {
        mem_size: usize,
        mem_buffer: *mut c_void,
        no_alloc: bool,
    }

    pub(super) fn check_abi() -> Result<(), Failure> {
        if std::mem::size_of::<InitParams>() != 24
            || std::mem::align_of::<InitParams>() != 8
            || std::mem::offset_of!(InitParams, mem_size) != 0
            || std::mem::offset_of!(InitParams, mem_buffer) != 8
            || std::mem::offset_of!(InitParams, no_alloc) != 16
        {
            return Err(Failure::pending("abi", "unsupported-init-params-layout"));
        }
        Ok(())
    }

    fn native_failure(stage: &'static str, code: &'static str) -> Failure {
        // SAFETY: GetLastError takes no pointers and reads this thread's error.
        let native_code = unsafe { GetLastError() };
        Failure {
            native_code: Some(native_code),
            ..Failure::pending(stage, code)
        }
    }

    struct Library {
        module: NonNull<c_void>,
        pin_until_process_exit: bool,
    }

    impl Library {
        fn load(path: &Path) -> Result<Self, Failure> {
            let wide = path
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>();
            if wide[..wide.len() - 1].contains(&0) {
                return Err(Failure::pending("load", "invalid-runtime-path"));
            }
            // SAFETY: The wrapper verified the selected exact package. The path
            // is absolute, NUL terminated and alive through the call; only its
            // own directory and Windows System32 can supply dependencies.
            let raw = unsafe {
                LoadLibraryExW(
                    wide.as_ptr(),
                    null_mut(),
                    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
                )
            };
            let module = NonNull::new(raw)
                .ok_or_else(|| native_failure("load", "runtime-library-load-failed"))?;
            Ok(Self {
                module,
                pin_until_process_exit: false,
            })
        }

        fn symbol(&self, name: &'static [u8]) -> Result<*mut c_void, Failure> {
            // SAFETY: Handle remains loaded; every caller supplies a static,
            // explicitly NUL-terminated exact C export name.
            let raw = unsafe { GetProcAddress(self.module.as_ptr(), name.as_ptr()) };
            if raw.is_null() {
                return Err(native_failure("symbols", "required-c-abi-export-missing"));
            }
            Ok(raw)
        }
    }

    impl Drop for Library {
        fn drop(&mut self) {
            if !self.pin_until_process_exit {
                // SAFETY: Before registry creation this wrapper owns the load
                // reference and no registered session can still use its code.
                let _ = unsafe { FreeLibrary(self.module.as_ptr()) };
            }
        }
    }

    // The C ABI uses opaque context/tensor/graph/backend/buffer pointers. No
    // ggml tensor, graph, registry or C++ object layout is copied into Rust.
    type Init = unsafe extern "C" fn(InitParams) -> *mut c_void;
    type Free = unsafe extern "C" fn(*mut c_void);
    type NewTensor = unsafe extern "C" fn(*mut c_void, c_int, i64, i64) -> *mut c_void;
    type MulMat = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void;
    type NewGraph = unsafe extern "C" fn(*mut c_void, usize, bool) -> *mut c_void;
    type Expand = unsafe extern "C" fn(*mut c_void, *mut c_void);
    type TensorBytes = unsafe extern "C" fn(*const c_void) -> usize;
    type TensorSet = unsafe extern "C" fn(*mut c_void, *const c_void, usize, usize);
    type TensorGet = unsafe extern "C" fn(*const c_void, *mut c_void, usize, usize);
    type Registry = unsafe extern "C" fn() -> *mut c_void;
    type DeviceCount = unsafe extern "C" fn(*mut c_void) -> usize;
    type DeviceGet = unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void;
    type DeviceInit = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
    type BackendName = unsafe extern "C" fn(*mut c_void) -> *const c_char;
    type BackendIsHexagon = unsafe extern "C" fn(*mut c_void) -> bool;
    type SupportsOp = unsafe extern "C" fn(*mut c_void, *const c_void) -> bool;
    type Allocate = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
    type Compute = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
    type LogCallback = unsafe extern "C" fn(c_int, *const c_char, *mut c_void);
    type LogSet = unsafe extern "C" fn(Option<LogCallback>, *mut c_void);

    struct Api {
        init: Init,
        context_free: Free,
        new_tensor: NewTensor,
        mul_mat: MulMat,
        new_graph: NewGraph,
        expand: Expand,
        tensor_bytes: TensorBytes,
        tensor_set: TensorSet,
        tensor_get: TensorGet,
        registry: Registry,
        device_count: DeviceCount,
        device_get: DeviceGet,
        device_init: DeviceInit,
        backend_name: BackendName,
        backend_is_hexagon: BackendIsHexagon,
        backend_free: Free,
        supports_op: SupportsOp,
        allocate: Allocate,
        buffer_free: Free,
        compute: Compute,
        synchronize: Free,
        log_set: LogSet,
    }

    impl Api {
        fn load(base: &Library, hexagon: &Library) -> Result<Self, Failure> {
            macro_rules! export {
                ($library:expr, $name:literal, $ty:ty) => {{
                    let raw = $library.symbol(concat!($name, "\0").as_bytes())?;
                    // SAFETY: Each concrete function signature is transcribed
                    // from the exact source-pinned public C declaration above.
                    unsafe { std::mem::transmute::<*mut c_void, $ty>(raw) }
                }};
            }
            Ok(Self {
                init: export!(base, "ggml_init", Init),
                context_free: export!(base, "ggml_free", Free),
                new_tensor: export!(base, "ggml_new_tensor_2d", NewTensor),
                mul_mat: export!(base, "ggml_mul_mat", MulMat),
                new_graph: export!(base, "ggml_new_graph_custom", NewGraph),
                expand: export!(base, "ggml_build_forward_expand", Expand),
                tensor_bytes: export!(base, "ggml_nbytes", TensorBytes),
                tensor_set: export!(base, "ggml_backend_tensor_set", TensorSet),
                tensor_get: export!(base, "ggml_backend_tensor_get", TensorGet),
                registry: export!(hexagon, "ggml_backend_hexagon_reg", Registry),
                device_count: export!(base, "ggml_backend_reg_dev_count", DeviceCount),
                device_get: export!(base, "ggml_backend_reg_dev_get", DeviceGet),
                device_init: export!(base, "ggml_backend_dev_init", DeviceInit),
                backend_name: export!(base, "ggml_backend_name", BackendName),
                backend_is_hexagon: export!(hexagon, "ggml_backend_is_hexagon", BackendIsHexagon),
                backend_free: export!(base, "ggml_backend_free", Free),
                supports_op: export!(base, "ggml_backend_supports_op", SupportsOp),
                allocate: export!(base, "ggml_backend_alloc_ctx_tensors", Allocate),
                buffer_free: export!(base, "ggml_backend_buffer_free", Free),
                compute: export!(base, "ggml_backend_graph_compute", Compute),
                synchronize: export!(base, "ggml_backend_synchronize", Free),
                log_set: export!(base, "ggml_log_set", LogSet),
            })
        }
    }

    struct Owned {
        pointer: NonNull<c_void>,
        release: Free,
    }

    impl Owned {
        fn checked(
            raw: *mut c_void,
            release: Free,
            stage: &'static str,
            code: &'static str,
        ) -> Result<Self, Failure> {
            let pointer = NonNull::new(raw).ok_or_else(|| Failure::pending(stage, code))?;
            Ok(Self { pointer, release })
        }

        fn raw(&self) -> *mut c_void {
            self.pointer.as_ptr()
        }
    }

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: Every constructor pairs a nonnull object with its exact
            // owning C destructor. Buffers drop before context/backend; graph
            // work synchronizes before this scope can drop a submitted buffer.
            unsafe { (self.release)(self.raw()) };
        }
    }

    unsafe extern "C" fn discard_log(_level: c_int, _text: *const c_char, _context: *mut c_void) {
        // Do not dereference vendor text or allow it into the JSON protocol.
    }

    fn valid_directory(path: &Path) -> bool {
        let spelling = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if spelling.len() < 3
            || spelling.len() > 260
            || spelling[1] != u16::from(b':')
            || !matches!(spelling[2], 47 | 92)
            || !path.is_absolute()
            || spelling.contains(&0)
        {
            return false;
        }
        path.ancestors().all(|ancestor| {
            ancestor.symlink_metadata().is_ok_and(|metadata| {
                metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
            })
        })
    }

    fn reject_overrides(path: &Path) -> Result<(), Failure> {
        for (key, value) in std::env::vars_os() {
            let key = key.to_string_lossy().to_ascii_uppercase();
            if [
                "GGML_", "GENIEX_", "QNN_", "QAIRT_", "HEXAGON_", "CDSP_", "FASTRPC_", "RPCMEM_",
            ]
            .iter()
            .any(|prefix| key.starts_with(*prefix))
                || (key.starts_with("ADSP_")
                    && (key != "ADSP_LIBRARY_PATH" || Path::new(&value) != path))
            {
                return Err(Failure::pending(
                    "environment",
                    "inherited-runtime-override-rejected",
                ));
            }
        }
        Ok(())
    }

    fn run_case(api: &Api, backend: &Owned, batch: usize) -> Result<(), Failure> {
        eprintln!("hexagon-fixture:stage=context");
        // SAFETY: This repr(C) by-value record matches the checked 24-byte ABI;
        // ggml owns the fixed two-MiB metadata pool, with tensor data unallocated.
        let raw = unsafe {
            (api.init)(InitParams {
                mem_size: 2 * 1024 * 1024,
                mem_buffer: null_mut(),
                no_alloc: true,
            })
        };
        let context = Owned::checked(
            raw,
            api.context_free,
            "context",
            "context-allocation-failed",
        )?;
        // SAFETY: Context is live, F32 is the pinned enum value zero, and all
        // dimensions are fixed small positive integers bounded by this fixture.
        let weights =
            unsafe { (api.new_tensor)(context.raw(), GGML_TYPE_F32, COLUMNS as i64, ROWS as i64) };
        // SAFETY: Same context/type/dimension contract as the weights above.
        let activations =
            unsafe { (api.new_tensor)(context.raw(), GGML_TYPE_F32, COLUMNS as i64, batch as i64) };
        if weights.is_null() || activations.is_null() {
            return Err(Failure::pending("graph", "tensor-construction-failed"));
        }
        // SAFETY: Both compatible fixed-shape tensors belong to this context.
        let output = unsafe { (api.mul_mat)(context.raw(), weights, activations) };
        // SAFETY: Graph metadata belongs to the live context; 16 nodes exceeds
        // the single fixed matmul's complete graph and gradients are disabled.
        let graph = unsafe { (api.new_graph)(context.raw(), 16, false) };
        if output.is_null() || graph.is_null() {
            return Err(Failure::pending("graph", "graph-construction-failed"));
        }
        // SAFETY: Output belongs to the same context and graph metadata is live.
        unsafe { (api.expand)(graph, output) };
        // SAFETY: Backend and output tensor remain live; this asks only the
        // explicit Hexagon backend, with no scheduler or CPU backend present.
        if !unsafe { (api.supports_op)(backend.raw(), output) } {
            return Err(Failure::pending(
                "operation-support",
                "fixed-f32-matmul-unsupported",
            ));
        }
        eprintln!("hexagon-fixture:stage=allocation");
        // SAFETY: The context contains only the fixed graph tensors. The returned
        // buffer is owned separately and is dropped before its context/backend.
        let raw_buffer = unsafe { (api.allocate)(context.raw(), backend.raw()) };
        let _buffer = Owned::checked(
            raw_buffer,
            api.buffer_free,
            "allocation",
            "hexagon-buffer-allocation-failed",
        )?;
        let (weight_values, activation_values) = fixture(batch);
        let mut output_values = vec![f32::NAN; ROWS * batch];
        // SAFETY: All pointers are live context tensors; reported byte extents
        // are checked against owned Rust slices before either set or get call.
        let extents_match = unsafe {
            (api.tensor_bytes)(weights) == std::mem::size_of_val(weight_values.as_slice())
                && (api.tensor_bytes)(activations)
                    == std::mem::size_of_val(activation_values.as_slice())
                && (api.tensor_bytes)(output) == std::mem::size_of_val(output_values.as_slice())
        };
        if !extents_match {
            return Err(Failure::failed("abi", "tensor-byte-extent-mismatch"));
        }
        eprintln!("hexagon-fixture:stage=transfer");
        // SAFETY: Synchronous tensor_set copies exactly the checked slice extent
        // into allocated buffers. Sources stay owned through graph synchronization.
        unsafe {
            (api.tensor_set)(
                weights,
                weight_values.as_ptr().cast(),
                0,
                std::mem::size_of_val(weight_values.as_slice()),
            )
        };
        // SAFETY: Same synchronous-copy and exact-extent contract as above.
        unsafe {
            (api.tensor_set)(
                activations,
                activation_values.as_ptr().cast(),
                0,
                std::mem::size_of_val(activation_values.as_slice()),
            )
        };
        // SAFETY: Poison every output value so missing writes cannot pass using
        // stale device bytes. This allocated output extent was checked above.
        unsafe {
            (api.tensor_set)(
                output,
                output_values.as_ptr().cast(),
                0,
                std::mem::size_of_val(output_values.as_slice()),
            )
        };
        eprintln!("hexagon-fixture:stage=dispatch");
        // SAFETY: All graph storage and input slices outlive synchronous compute.
        // It is invoked directly on verified Hexagon, with no CPU fallback.
        let status = unsafe { (api.compute)(backend.raw(), graph) };
        // SAFETY: Retain every buffer until all backend use ends, including the
        // returned-error path. A vendor abort/hang is handled by process teardown
        // in the wrapper rather than freeing potentially active device buffers.
        unsafe { (api.synchronize)(backend.raw()) };
        if status != GGML_STATUS_SUCCESS {
            return Err(Failure::failed("dispatch", "hexagon-graph-compute-failed"));
        }
        eprintln!("hexagon-fixture:stage=readback");
        let output_bytes = std::mem::size_of_val(output_values.as_slice());
        // SAFETY: Synchronization completed; the destination is exclusively owned
        // and spans exactly the checked output tensor extent.
        unsafe { (api.tensor_get)(output, output_values.as_mut_ptr().cast(), 0, output_bytes) };
        if !matches_reference(&output_values, batch) {
            return Err(Failure::failed(
                "comparison",
                "independent-f32-oracle-mismatch",
            ));
        }
        Ok(())
    }

    pub(super) fn run(path: &Path, receipt: &mut Receipt) -> Result<(), Failure> {
        check_abi()?;
        if !valid_directory(path) {
            return Err(Failure::pending(
                "arguments",
                "expanded-local-runtime-directory-required",
            ));
        }
        reject_overrides(path)?;
        // SAFETY: Restricts bare-name loads inside this one-shot process to
        // System32. Explicit runtime libraries add their own verified directory
        // per LoadLibraryExW call; no global machine search policy is changed.
        if unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) } == 0 {
            return Err(native_failure(
                "load-policy",
                "restricted-dll-search-unavailable",
            ));
        }
        eprintln!("hexagon-fixture:stage=load");
        let mut base = Library::load(&path.join("ggml-base.dll"))?;
        let mut hexagon = Library::load(&path.join("ggml-hexagon.dll"))?;
        let api = Api::load(&base, &hexagon)?;
        // SAFETY: Callback never dereferences either borrowed pointer. It remains
        // in this executable until process exit and accepts no mutable context.
        unsafe { (api.log_set)(Some(discard_log), null_mut()) };
        // Registry-owned sessions have no public destroy API at the source pin.
        // Never unload their executable code beneath possible driver threads;
        // the supervisor must observe this one-shot process's complete exit.
        base.pin_until_process_exit = true;
        hexagon.pin_until_process_exit = true;
        eprintln!("hexagon-fixture:stage=registry");
        // SAFETY: Resolved exact Hexagon export, not generic backend discovery.
        let registry = unsafe { (api.registry)() };
        if registry.is_null() {
            return Err(Failure::pending(
                "registry",
                "hexagon-driver-initialization-unavailable",
            ));
        }
        // SAFETY: Registry came from the same live Hexagon library; count does
        // not transfer ownership. Only its sole default physical device is used.
        if unsafe { (api.device_count)(registry) } != 1 {
            return Err(Failure::pending("registry", "one-hexagon-device-required"));
        }
        // SAFETY: Exact registry has one device, so index zero is in range.
        let device = unsafe { (api.device_get)(registry, 0) };
        if device.is_null() {
            return Err(Failure::pending("device", "hexagon-device-unavailable"));
        }
        eprintln!("hexagon-fixture:stage=backend-init");
        // SAFETY: Device is live and null supplies the pinned default parameters.
        let raw_backend = unsafe { (api.device_init)(device, null()) };
        let backend = Owned::checked(
            raw_backend,
            api.backend_free,
            "backend-init",
            "hexagon-backend-unavailable",
        )?;
        // SAFETY: The exact library's identity predicate receives its own live
        // backend. It rejects an accidental alternate implementation.
        if !unsafe { (api.backend_is_hexagon)(backend.raw()) } {
            return Err(Failure::failed(
                "backend-identity",
                "explicit-backend-is-not-hexagon",
            ));
        }
        // SAFETY: The pinned backend returns a valid NUL-terminated constant or
        // owned name for its lifetime. Read only the five expected HTP0+NUL bytes.
        let name = unsafe { (api.backend_name)(backend.raw()) };
        if name.is_null() {
            return Err(Failure::failed(
                "backend-identity",
                "backend-name-unavailable",
            ));
        }
        for (index, expected) in b"HTP0\0".iter().enumerate() {
            // SAFETY: The public API promises a NUL-terminated name. A shorter
            // name mismatches and returns at its terminator, so the next byte
            // is never read. A longer name mismatches at the expected NUL.
            let actual = unsafe { *name.cast::<u8>().add(index) };
            if actual != *expected {
                return Err(Failure::failed(
                    "backend-identity",
                    "unexpected-hexagon-session",
                ));
            }
        }
        receipt.backend_verified = true;
        for batch in BATCHES {
            run_case(&api, &backend, batch)?;
            receipt.cases.push(CaseResult {
                batch,
                on_hexagon: true,
            });
        }
        // Backend drops after every graph buffer/context has synchronized and
        // dropped. Process exit owns residual upstream registry/session teardown.
        Ok(())
    }
}
