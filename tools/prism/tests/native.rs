//! Real pinned ggml allocator and CPU arithmetic regression for the Rust C ABI.
//!
//! The only native layouts reproduced here are the public `ggml_init_params` and
//! repository-owned `bonsai_prism_layout`. All ggml object pointers stay opaque;
//! shape, stride, name, payload and allocation checks use public C functions.

#![allow(unsafe_code)]
#![allow(
    clippy::expect_used,
    reason = "Test setup failures need an explanatory assertion"
)]

use bonsai_prism::ffi::{
    BonsaiPrismLayout, BonsaiPrismPool, bonsai_prism_pool_cached_bytes, bonsai_prism_pool_find,
    bonsai_prism_pool_free, bonsai_prism_pool_new, bonsai_prism_pool_remember,
    bonsai_prism_pool_shared_bytes, bonsai_prism_pool_shared_tensors,
};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr::{self, NonNull};

const GGML_TYPE_F32: c_int = 0;
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

// Signatures and enum values are from the source.lock.json-pinned ggml headers.
unsafe extern "C" {
    fn ggml_init(params: InitParams) -> *mut c_void;
    fn ggml_free(context: *mut c_void);
    fn ggml_tensor_overhead() -> usize;
    fn ggml_graph_overhead_custom(size: usize, grads: bool) -> usize;
    fn ggml_new_tensor_2d(context: *mut c_void, kind: c_int, ne0: i64, ne1: i64) -> *mut c_void;
    fn ggml_dup_tensor(context: *mut c_void, source: *const c_void) -> *mut c_void;
    fn ggml_view_tensor(context: *mut c_void, source: *mut c_void) -> *mut c_void;
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
        // buffers and Rust pool. No tensor pointer survives the test.
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

struct PoolHandle(NonNull<BonsaiPrismPool>);

impl PoolHandle {
    fn new() -> Self {
        Self(NonNull::new(bonsai_prism_pool_new()).expect("Rust pool allocation failed"))
    }

    const fn as_ptr(&self) -> *mut BonsaiPrismPool {
        self.0.as_ptr()
    }
}

impl Drop for PoolHandle {
    fn drop(&mut self) {
        // SAFETY: The uniquely owned handle was allocated by pool_new and all
        // calls have completed. Contexts and tensor owners are still alive.
        unsafe { bonsai_prism_pool_free(self.as_ptr()) };
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
    let pool = PoolHandle::new();
    let bytes: Vec<u8> = (0_i16..5120)
        .flat_map(|index| (f32::from(index % 17 - 8) / 16.0).to_ne_bytes())
        .collect();
    let mut aliases = Vec::new();
    let mut originals = Vec::new();

    // SAFETY: Contexts, buffers, backend and pool have unique RAII owners and
    // remain live for all calls below. All tensor and graph pointers originate
    // in these contexts. CPU buffers are allocated before writes/computation.
    // Every byte span has exactly the tensor size checked by the pinned API;
    // names are C strings copied by ggml_set_name. Owner pointer provenance is
    // explicitly exposed before storing the C ABI token and recovered on hit.
    unsafe {
        let cpu = ggml_backend_cpu_buffer_type();
        assert!(!cpu.is_null(), "CPU buffer type unavailable");
        let layout = BonsaiPrismLayout {
            context: shared.as_ptr().addr(),
            buffer_type: cpu.addr(),
            cpu: true,
            f32: true,
            contiguous: true,
            dimensions: [1, B_ROWS, 1, 1],
            strides: [4, 4, B_BYTES, B_BYTES],
            byte_len: B_BYTES,
        };
        for index in 0..129 {
            let original = ggml_new_tensor_2d(copied.as_ptr(), GGML_TYPE_F32, 1, B_ROWS);
            assert!(!original.is_null(), "source tensor allocation failed");
            assert_eq!(ggml_nbytes(original), B_BYTES);
            assert!(ggml_is_contiguous(original));
            let owner = bonsai_prism_pool_find(
                pool.as_ptr(),
                &raw const layout,
                bytes.as_ptr(),
                bytes.len(),
            );
            let alias = if owner == 0 {
                let result = ggml_dup_tensor(shared.as_ptr(), original);
                assert!(!result.is_null(), "canonical tensor allocation failed");
                // This source is an ordinary new F32 tensor: duplication keeps
                // its known dimensions/strides. No native object is laid out in Rust.
                assert!(ggml_are_same_shape(result, original));
                assert!(ggml_are_same_stride(result, original));
                assert!(bonsai_prism_pool_remember(
                    pool.as_ptr(),
                    &raw const layout,
                    bytes.as_ptr(),
                    bytes.len(),
                    result.expose_provenance(),
                ));
                result
            } else {
                ggml_view_tensor(shared.as_ptr(), ptr::with_exposed_provenance_mut(owner))
            };
            assert!(!alias.is_null(), "view allocation failed");
            let name = CString::new(format!("b-{index}")).expect("test name contains NUL");
            ggml_set_name(alias, name.as_ptr());
            aliases.push(alias);
            originals.push(original);
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
        assert_eq!(bonsai_prism_pool_shared_tensors(pool.as_ptr()), 128);
        assert_eq!(
            bonsai_prism_pool_shared_bytes(pool.as_ptr()),
            EXPECTED_SAVING
        );
        assert_eq!(bonsai_prism_pool_cached_bytes(pool.as_ptr()), B_BYTES);
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
            ggml_backend_tensor_set(original, bytes.as_ptr().cast(), 0, bytes.len());
        }
        ggml_backend_tensor_set(aliases[0], bytes.as_ptr().cast(), 0, bytes.len());
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
