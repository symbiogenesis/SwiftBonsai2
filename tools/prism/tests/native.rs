//! Real pinned ggml allocator and CPU arithmetic regression for the Rust C ABI.
//!
//! The public `ggml_init_params` and `gguf_init_params` are the only native
//! layouts reproduced here. All ggml and GGUF objects stay opaque. The Rust
//! loader extracts real tensor metadata, reads a synthetic GGUF file, creates
//! canonical tensors/views, and populates the actual backend allocation.

#![allow(unsafe_code)]
#![allow(
    clippy::expect_used,
    reason = "Test setup failures need an explanatory assertion"
)]

use bonsai_prism::loader::{
    NativeLoader, bonsai_prism_loader_error, bonsai_prism_loader_free, bonsai_prism_loader_load,
    bonsai_prism_loader_make, bonsai_prism_loader_new, bonsai_prism_loader_shared_bytes,
    bonsai_prism_loader_shared_tensors,
};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::ptr::{self, NonNull};
use std::time::{SystemTime, UNIX_EPOCH};

const GGML_TYPE_F32: c_int = 0;
const GGML_TYPE_F16: c_int = 1;
const GGML_STATUS_SUCCESS: c_int = 0;
const B_ROWS: i64 = 5120;
const B_BYTES: usize = 20_480;
const EXPECTED_SAVING: usize = 2_621_440;

#[repr(C)]
struct InitParams {
    mem_size: usize,
    mem_buffer: *mut c_void,
    no_alloc: bool,
}

#[repr(C)]
struct GgufInitParams {
    no_alloc: bool,
    ctx: *mut *mut c_void,
}

// Signatures and enum values are from the source.lock.json-pinned ggml headers.
unsafe extern "C" {
    fn ggml_init(params: InitParams) -> *mut c_void;
    fn ggml_free(context: *mut c_void);
    fn ggml_tensor_overhead() -> usize;
    fn ggml_graph_overhead_custom(size: usize, grads: bool) -> usize;
    fn ggml_new_tensor_2d(context: *mut c_void, kind: c_int, ne0: i64, ne1: i64) -> *mut c_void;
    fn ggml_view_2d(
        context: *mut c_void,
        source: *mut c_void,
        ne0: i64,
        ne1: i64,
        nb1: usize,
        offset: usize,
    ) -> *mut c_void;
    fn ggml_mul_mat(context: *mut c_void, a: *mut c_void, b: *mut c_void) -> *mut c_void;
    fn ggml_is_contiguous(tensor: *const c_void) -> bool;
    fn ggml_are_same_shape(left: *const c_void, right: *const c_void) -> bool;
    fn ggml_are_same_stride(left: *const c_void, right: *const c_void) -> bool;
    fn ggml_nbytes(tensor: *const c_void) -> usize;
    fn ggml_get_data(tensor: *const c_void) -> *mut c_void;
    fn ggml_set_name(tensor: *mut c_void, name: *const c_char) -> *mut c_void;
    fn ggml_get_name(tensor: *const c_void) -> *const c_char;
    fn ggml_new_graph_custom(context: *mut c_void, size: usize, grads: bool) -> *mut c_void;
    fn ggml_build_forward_expand(graph: *mut c_void, tensor: *mut c_void);
    fn ggml_backend_cpu_buffer_type() -> *mut c_void;
    fn ggml_backend_alloc_ctx_tensors_from_buft(
        context: *mut c_void,
        buffer_type: *mut c_void,
    ) -> *mut c_void;
    fn ggml_backend_buffer_get_size(buffer: *mut c_void) -> usize;
    fn ggml_backend_buffer_free(buffer: *mut c_void);
    fn ggml_backend_tensor_set(
        tensor: *mut c_void,
        data: *const c_void,
        offset: usize,
        size: usize,
    );
    fn ggml_backend_tensor_get(
        tensor: *const c_void,
        data: *mut c_void,
        offset: usize,
        size: usize,
    );
    fn ggml_backend_cpu_init() -> *mut c_void;
    fn ggml_backend_cpu_set_n_threads(backend: *mut c_void, threads: c_int);
    fn ggml_backend_graph_compute(backend: *mut c_void, graph: *mut c_void) -> c_int;
    fn ggml_backend_free(backend: *mut c_void);
    fn gguf_init_empty() -> *mut c_void;
    fn gguf_add_tensor(context: *mut c_void, tensor: *const c_void);
    fn gguf_get_meta_size(context: *const c_void) -> usize;
    fn gguf_get_meta_data(context: *const c_void, data: *mut c_void);
    fn gguf_init_from_file(path: *const c_char, params: GgufInitParams) -> *mut c_void;
    fn gguf_get_data_offset(context: *const c_void) -> usize;
    fn gguf_get_tensor_offset(context: *const c_void, tensor: i64) -> usize;
    fn gguf_free(context: *mut c_void);
}

struct Context(NonNull<c_void>);

impl Context {
    fn new() -> Self {
        // SAFETY: Params matches the pinned public C layout. ggml owns the arena
        // allocation; no_alloc makes each tensor's data await backend allocation.
        let value = unsafe {
            ggml_init(InitParams {
                mem_size: 512 * ggml_tensor_overhead() + ggml_graph_overhead_custom(64, false),
                mem_buffer: ptr::null_mut(),
                no_alloc: true,
            })
        };
        Self(NonNull::new(value).expect("ggml context allocation failed"))
    }

    const fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: This uniquely owned context is freed once, after its backend
        // buffers and Rust loader. No tensor pointer survives the test.
        unsafe { ggml_free(self.as_ptr()) };
    }
}

struct Buffer(NonNull<c_void>);

impl Drop for Buffer {
    fn drop(&mut self) {
        // SAFETY: The backend returned this uniquely owned live buffer.
        unsafe { ggml_backend_buffer_free(self.0.as_ptr()) };
    }
}

struct Backend(NonNull<c_void>);

impl Drop for Backend {
    fn drop(&mut self) {
        // SAFETY: This uniquely owned backend is no longer executing a graph.
        unsafe { ggml_backend_free(self.0.as_ptr()) };
    }
}

struct LoaderHandle(NonNull<NativeLoader>);

impl LoaderHandle {
    fn new(fixture: &GgufFixture) -> Self {
        // SAFETY: The UTF-8 path is a live NUL-terminated C string. The loader
        // owns its opened file, so the borrowed path need only outlive this call.
        let value = unsafe { bonsai_prism_loader_new(fixture.path_c_string().as_ptr()) };
        Self(NonNull::new(value).expect("Rust loader construction failed"))
    }

    const fn as_ptr(&self) -> *mut NativeLoader {
        self.0.as_ptr()
    }

    fn assert_error(&self) {
        // SAFETY: The live loader returns a NUL-terminated error string valid
        // until its next mutable operation. No operation occurs during access.
        unsafe {
            let error = bonsai_prism_loader_error(self.as_ptr());
            assert!(!error.is_null());
            assert_ne!(CStr::from_ptr(error).to_bytes(), b"");
        }
    }
}

impl Drop for LoaderHandle {
    fn drop(&mut self) {
        // SAFETY: This uniquely owned handle was returned by loader_new. Its
        // operations have completed, and its tensor owners remain alive.
        unsafe { bonsai_prism_loader_free(self.as_ptr()) };
    }
}

struct GgufContext(NonNull<c_void>);

impl Drop for GgufContext {
    fn drop(&mut self) {
        // SAFETY: This uniquely owned metadata context came from gguf_init_*.
        unsafe { gguf_free(self.0.as_ptr()) };
    }
}

struct GgufFixture {
    metadata: GgufContext,
    path: PathBuf,
    directory: PathBuf,
}

impl GgufFixture {
    /// Writes native GGUF metadata followed by caller-provided aligned payloads.
    /// Empty payloads may intentionally omit tensor data for malformed fixtures.
    ///
    /// # Safety
    /// Each tensor must be a live named ggml tensor for the entire call.
    unsafe fn new(tensors: &[*mut c_void], payloads: &[&[u8]]) -> Self {
        assert_eq!(tensors.len(), payloads.len());
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock predates the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "SwiftBonsai-PrismRustNative-{}-{stamp}",
            std::process::id(),
        ));
        assert!(directory.is_absolute());
        fs::create_dir(&directory).expect("cannot create external native test lane");
        let path = directory.join("synthetic-adapter.gguf");
        // SAFETY: All tensors are live by the caller contract. get_meta_data
        // writes exactly the allocation size returned by get_meta_size.
        let mut contents = unsafe {
            let writer = GgufContext(NonNull::new(gguf_init_empty()).expect("GGUF writer failed"));
            for &tensor in tensors {
                gguf_add_tensor(writer.0.as_ptr(), tensor);
            }
            let mut metadata = vec![0_u8; gguf_get_meta_size(writer.0.as_ptr())];
            gguf_get_meta_data(writer.0.as_ptr(), metadata.as_mut_ptr().cast());
            metadata
        };
        for payload in payloads {
            contents.extend_from_slice(payload);
            contents.resize(contents.len().next_multiple_of(32), 0);
        }
        fs::write(&path, contents).expect("cannot write synthetic GGUF");
        let path_c_string = CString::new(path.to_str().expect("test path is not UTF-8"))
            .expect("test path contains NUL");
        // SAFETY: The path C string is live, and null ctx requests only native
        // metadata parsing. The returned context owns all parsed descriptions.
        let metadata = unsafe {
            GgufContext(
                NonNull::new(gguf_init_from_file(
                    path_c_string.as_ptr(),
                    GgufInitParams {
                        no_alloc: true,
                        ctx: ptr::null_mut(),
                    },
                ))
                .expect("native parser rejected synthetic GGUF metadata"),
            )
        };
        Self {
            metadata,
            path,
            directory,
        }
    }

    const fn metadata_ptr(&self) -> *mut c_void {
        self.metadata.0.as_ptr()
    }

    fn path_c_string(&self) -> CString {
        CString::new(self.path.to_str().expect("test path is not UTF-8"))
            .expect("test path contains NUL")
    }
}

impl Drop for GgufFixture {
    fn drop(&mut self) {
        // LoaderHandle is declared after the fixture and closes its file first.
        // Preserve a failed fixture as evidence; successful fixtures are private
        // disposable files within the external directory allocated above.
        if !std::thread::panicking() {
            assert!(self.path.starts_with(&self.directory));
            fs::remove_file(&self.path).expect("cannot remove private synthetic GGUF");
            fs::remove_dir(&self.directory).expect("cannot remove private fixture directory");
        }
    }
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "One fixture keeps the native object lifetimes and validation together"
)]
fn native_storage_and_math() {
    let shared = Context::new();
    let copied = Context::new();
    let bytes: Vec<u8> = (0_i16..5120)
        .flat_map(|index| (f32::from(index % 17 - 8) / 16.0).to_ne_bytes())
        .collect();
    let mut aliases = Vec::new();
    let mut originals = Vec::new();

    // SAFETY: Contexts, buffers, backend and loader have unique RAII owners and
    // remain live for all calls below. All tensor and graph pointers originate
    // in these contexts. CPU buffers are allocated before writes/computation.
    // Every byte span has exactly the tensor size checked by the pinned API;
    // names are C strings copied by ggml_set_name. Native GGUF metadata is
    // parsed from the same on-disk fixture opened by the actual Rust loader.
    unsafe {
        let cpu = ggml_backend_cpu_buffer_type();
        assert!(!cpu.is_null(), "CPU buffer type unavailable");
        for index in 0..129 {
            let original = ggml_new_tensor_2d(copied.as_ptr(), GGML_TYPE_F32, 1, B_ROWS);
            assert!(!original.is_null(), "source tensor allocation failed");
            assert_eq!(ggml_nbytes(original), B_BYTES);
            assert!(ggml_is_contiguous(original));
            let source_name =
                CString::new(format!("b-{index}.lora_b")).expect("test name contains NUL");
            ggml_set_name(original, source_name.as_ptr());
            originals.push(original);
        }
        let payloads = vec![bytes.as_slice(); originals.len()];
        let fixture = GgufFixture::new(&originals, &payloads);
        let loader = LoaderHandle::new(&fixture);
        for (index, &original) in originals.iter().enumerate() {
            let alias = bonsai_prism_loader_make(
                loader.as_ptr(),
                fixture.metadata_ptr(),
                shared.as_ptr(),
                original,
                cpu,
            );
            assert!(!alias.is_null(), "view allocation failed");
            let name = CString::new(format!("b-{index}")).expect("test name contains NUL");
            ggml_set_name(alias, name.as_ptr());
            aliases.push(alias);
        }
        let rank = ggml_new_tensor_2d(shared.as_ptr(), GGML_TYPE_F32, 1, 2);
        let copy_rank = ggml_new_tensor_2d(copied.as_ptr(), GGML_TYPE_F32, 1, 2);
        assert!(!rank.is_null() && !copy_rank.is_null());
        let actual = ggml_mul_mat(shared.as_ptr(), aliases[128], rank);
        let expected = ggml_mul_mat(copied.as_ptr(), originals[128], copy_rank);
        assert!(!actual.is_null() && !expected.is_null());
        let shared_buffer = Buffer(
            NonNull::new(ggml_backend_alloc_ctx_tensors_from_buft(
                shared.as_ptr(),
                cpu,
            ))
            .expect("shared backend allocation failed"),
        );
        let copied_buffer = Buffer(
            NonNull::new(ggml_backend_alloc_ctx_tensors_from_buft(
                copied.as_ptr(),
                cpu,
            ))
            .expect("copied backend allocation failed"),
        );
        assert_eq!(bonsai_prism_loader_shared_tensors(loader.as_ptr()), 128);
        assert_eq!(
            bonsai_prism_loader_shared_bytes(loader.as_ptr()),
            EXPECTED_SAVING
        );
        assert_eq!(
            ggml_backend_buffer_get_size(copied_buffer.0.as_ptr())
                .checked_sub(ggml_backend_buffer_get_size(shared_buffer.0.as_ptr())),
            Some(EXPECTED_SAVING),
            "the real allocator must eliminate exactly 2.5 MiB",
        );
        assert!(!ggml_get_data(aliases[0]).is_null());
        for (index, (&alias, &original)) in aliases.iter().zip(&originals).enumerate() {
            assert_eq!(ggml_get_data(alias), ggml_get_data(aliases[0]));
            assert_eq!(
                CStr::from_ptr(ggml_get_name(alias)).to_bytes(),
                format!("b-{index}").as_bytes(),
            );
            assert!(ggml_are_same_shape(alias, original));
            assert!(ggml_are_same_stride(alias, original));
            assert!(bonsai_prism_loader_load(
                loader.as_ptr(),
                fixture.metadata_ptr(),
                original,
                alias,
            ));
            ggml_backend_tensor_set(original, bytes.as_ptr().cast(), 0, bytes.len());
        }
        let mut owner_payload = vec![0_u8; B_BYTES];
        ggml_backend_tensor_get(
            aliases[0],
            owner_payload.as_mut_ptr().cast(),
            0,
            owner_payload.len(),
        );
        assert_eq!(
            owner_payload, bytes,
            "Rust loader populated incorrect owner bytes"
        );
        let factors: Vec<u8> = [0.375_f32, -0.625_f32]
            .into_iter()
            .flat_map(f32::to_ne_bytes)
            .collect();
        ggml_backend_tensor_set(rank, factors.as_ptr().cast(), 0, factors.len());
        ggml_backend_tensor_set(copy_rank, factors.as_ptr().cast(), 0, factors.len());
        let backend = Backend(NonNull::new(ggml_backend_cpu_init()).expect("CPU init failed"));
        ggml_backend_cpu_set_n_threads(backend.0.as_ptr(), 1);
        let actual_graph = ggml_new_graph_custom(shared.as_ptr(), 64, false);
        let expected_graph = ggml_new_graph_custom(copied.as_ptr(), 64, false);
        assert!(!actual_graph.is_null() && !expected_graph.is_null());
        ggml_build_forward_expand(actual_graph, actual);
        ggml_build_forward_expand(expected_graph, expected);
        assert_eq!(
            ggml_backend_graph_compute(backend.0.as_ptr(), actual_graph),
            GGML_STATUS_SUCCESS,
        );
        assert_eq!(
            ggml_backend_graph_compute(backend.0.as_ptr(), expected_graph),
            GGML_STATUS_SUCCESS,
        );
        assert_eq!(ggml_nbytes(actual), ggml_nbytes(expected));
        let mut actual_bytes = vec![0_u8; ggml_nbytes(actual)];
        let mut expected_bytes = vec![0_u8; ggml_nbytes(expected)];
        ggml_backend_tensor_get(
            actual,
            actual_bytes.as_mut_ptr().cast(),
            0,
            actual_bytes.len(),
        );
        ggml_backend_tensor_get(
            expected,
            expected_bytes.as_mut_ptr().cast(),
            0,
            expected_bytes.len(),
        );
        assert_eq!(
            actual_bytes, expected_bytes,
            "shared B changed MUL_MAT output bytes"
        );
    }
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "One fixture documents fallback allocations and malformed native descriptors together"
)]
fn native_loader_rejects_invalid_reads_and_keeps_fallbacks() {
    let source_context = Context::new();
    let destination_context = Context::new();
    let f16_bytes: Vec<u8> = (0..5120).flat_map(|_| 0x3c00_u16.to_ne_bytes()).collect();
    let f32_bytes = vec![0_u8; B_BYTES];

    // SAFETY: All contexts and native metadata have RAII owners. Every source
    // is created by ggml and named before native GGUF metadata captures it.
    // Fallback tensors are allocated before load writes. Malformed files retain
    // valid native metadata while omitting data; no invalid pointer is forged.
    unsafe {
        let cpu = ggml_backend_cpu_buffer_type();
        let f16 = ggml_new_tensor_2d(source_context.as_ptr(), GGML_TYPE_F16, 1, B_ROWS);
        let wide = ggml_new_tensor_2d(source_context.as_ptr(), GGML_TYPE_F32, 2, B_ROWS);
        let strided = ggml_view_2d(source_context.as_ptr(), wide, 1, B_ROWS, 8, 0);
        let ordinary = ggml_new_tensor_2d(source_context.as_ptr(), GGML_TYPE_F32, 1, B_ROWS);
        assert!(!f16.is_null() && !strided.is_null() && !ordinary.is_null());
        assert!(!ggml_is_contiguous(strided));
        ggml_set_name(f16, c"f16.lora_b".as_ptr());
        ggml_set_name(strided, c"strided.lora_b".as_ptr());
        ggml_set_name(ordinary, c"ordinary.lora_b".as_ptr());
        let strided_bytes = vec![0_u8; ggml_nbytes(strided)];
        let fallback_fixture = GgufFixture::new(&[f16, strided], &[&f16_bytes, &strided_bytes]);
        let fallback_loader = LoaderHandle::new(&fallback_fixture);
        let first_f16 = bonsai_prism_loader_make(
            fallback_loader.as_ptr(),
            fallback_fixture.metadata_ptr(),
            destination_context.as_ptr(),
            f16,
            cpu,
        );
        let second_f16 = bonsai_prism_loader_make(
            fallback_loader.as_ptr(),
            fallback_fixture.metadata_ptr(),
            destination_context.as_ptr(),
            f16,
            cpu,
        );
        let first_strided = bonsai_prism_loader_make(
            fallback_loader.as_ptr(),
            fallback_fixture.metadata_ptr(),
            destination_context.as_ptr(),
            strided,
            cpu,
        );
        let second_strided = bonsai_prism_loader_make(
            fallback_loader.as_ptr(),
            fallback_fixture.metadata_ptr(),
            destination_context.as_ptr(),
            strided,
            cpu,
        );
        // A non-CPU request must fall back before any missing payload lookup.
        let other_buffer = bonsai_prism_loader_make(
            fallback_loader.as_ptr(),
            fallback_fixture.metadata_ptr(),
            destination_context.as_ptr(),
            ordinary,
            ptr::null_mut(),
        );
        assert!(!first_f16.is_null() && !second_f16.is_null());
        assert!(!first_strided.is_null() && !second_strided.is_null());
        assert!(!other_buffer.is_null());
        assert!(ggml_are_same_shape(f16, first_f16));
        assert!(ggml_are_same_stride(f16, first_f16));
        assert!(ggml_are_same_shape(strided, first_strided));
        assert!(ggml_is_contiguous(first_strided));
        let truncated_destination =
            ggml_new_tensor_2d(destination_context.as_ptr(), GGML_TYPE_F32, 1, B_ROWS);
        assert!(!truncated_destination.is_null());
        let _buffer = Buffer(
            NonNull::new(ggml_backend_alloc_ctx_tensors_from_buft(
                destination_context.as_ptr(),
                cpu,
            ))
            .expect("fallback backend allocation failed"),
        );
        assert_ne!(ggml_get_data(first_f16), ggml_get_data(second_f16));
        assert_ne!(ggml_get_data(first_strided), ggml_get_data(second_strided));
        assert_eq!(
            bonsai_prism_loader_shared_tensors(fallback_loader.as_ptr()),
            0
        );
        assert_eq!(
            bonsai_prism_loader_shared_bytes(fallback_loader.as_ptr()),
            0
        );
        assert!(bonsai_prism_loader_load(
            fallback_loader.as_ptr(),
            fallback_fixture.metadata_ptr(),
            f16,
            first_f16,
        ));
        let mut loaded_f16 = vec![0_u8; f16_bytes.len()];
        ggml_backend_tensor_get(
            first_f16,
            loaded_f16.as_mut_ptr().cast(),
            0,
            loaded_f16.len(),
        );
        assert_eq!(loaded_f16, f16_bytes, "F16 fallback bytes changed");

        // Parse genuine metadata first, then truncate before the loader opens
        // its read-only handle (Windows denies a concurrent file truncation).
        let truncated_fixture = GgufFixture::new(&[ordinary], &[&f32_bytes]);
        let file = OpenOptions::new()
            .write(true)
            .open(&truncated_fixture.path)
            .expect("cannot open private truncation fixture");
        let truncated_len = gguf_get_data_offset(truncated_fixture.metadata_ptr()) + B_BYTES - 1;
        file.set_len(u64::try_from(truncated_len).expect("file length exceeds u64"))
            .expect("cannot truncate private fixture");
        drop(file);
        let truncated_loader = LoaderHandle::new(&truncated_fixture);
        assert!(
            bonsai_prism_loader_make(
                truncated_loader.as_ptr(),
                truncated_fixture.metadata_ptr(),
                destination_context.as_ptr(),
                ordinary,
                cpu,
            )
            .is_null()
        );
        truncated_loader.assert_error();
        assert!(!bonsai_prism_loader_load(
            truncated_loader.as_ptr(),
            truncated_fixture.metadata_ptr(),
            ordinary,
            truncated_destination,
        ));
        truncated_loader.assert_error();

        // Native GGUF metadata describes a large preceding tensor, so the B
        // offset lies well beyond this intentionally short file. No huge data
        // allocation or private native struct mutation is necessary.
        let prefix = ggml_new_tensor_2d(source_context.as_ptr(), GGML_TYPE_F32, 1, 1_i64 << 32);
        assert!(!prefix.is_null());
        ggml_set_name(prefix, c"omitted-prefix".as_ptr());
        let offset_fixture = GgufFixture::new(&[prefix, ordinary], &[&[], &f32_bytes]);
        assert!(
            gguf_get_tensor_offset(offset_fixture.metadata_ptr(), 1)
                > usize::try_from(
                    fs::metadata(&offset_fixture.path)
                        .expect("fixture disappeared")
                        .len()
                )
                .expect("file size exceeds usize")
        );
        let offset_loader = LoaderHandle::new(&offset_fixture);
        assert!(
            bonsai_prism_loader_make(
                offset_loader.as_ptr(),
                offset_fixture.metadata_ptr(),
                destination_context.as_ptr(),
                ordinary,
                cpu,
            )
            .is_null()
        );
        offset_loader.assert_error();
        assert!(!bonsai_prism_loader_load(
            offset_loader.as_ptr(),
            offset_fixture.metadata_ptr(),
            ordinary,
            truncated_destination,
        ));
        offset_loader.assert_error();
        assert_eq!(
            bonsai_prism_loader_shared_tensors(offset_loader.as_ptr()),
            0
        );
        assert_eq!(bonsai_prism_loader_shared_bytes(offset_loader.as_ptr()), 0);
    }
}
