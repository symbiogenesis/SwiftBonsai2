//! Audited opaque-resource ownership for the Rust decode-graph cache.

use std::{
    ffi::{c_int, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
};

use super::super::decode_graph::Cache;

type Destroy = unsafe extern "C" fn(*mut c_void);
type Build = unsafe extern "C" fn(*mut c_void) -> *mut c_void;

struct Resource {
    pointer: NonNull<c_void>,
    destroy: Destroy,
}

impl Drop for Resource {
    fn drop(&mut self) {
        // SAFETY: The noexcept native builder transfers exactly one graph
        // object to this owner. Native work has completed before eviction/drop;
        // the matching noexcept callback frees its GGML resources and object.
        unsafe { (self.destroy)(self.pointer.as_ptr()) };
    }
}

type Owner = Cache<Resource>;

fn guard<T>(call: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(call)).unwrap_or_else(|_| std::process::abort())
}

// SAFETY: A scalar window extent creates a unique opaque Box cache. The native
// pipeline owns this handle until teardown after its compute worker has joined.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_decode_cache_new(windows: usize) -> *mut c_void {
    guard(|| {
        Owner::new(windows).map_or(std::ptr::null_mut(), |cache| {
            Box::into_raw(Box::new(cache)).cast()
        })
    })
}

// SAFETY: cache is null or the one original Box<Owner> from cache_new, exclusively
// returned once after all native computation and borrowed output access finish.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_decode_cache_free(cache: *mut c_void) {
    if !cache.is_null() {
        guard(|| {
            // SAFETY: The private native holder supplies its original unique
            // Box handle exactly once. Cache Drop aborts if a lease is active.
            drop(unsafe { Box::from_raw(cache.cast::<Owner>()) });
        });
    }
}

// SAFETY: The native CPU worker exclusively borrows one live cache. Callbacks
// are noexcept and remain linked until cache destruction; build borrows userdata
// synchronously and transfers a graph only on success. No pointers are retained
// from userdata. The result is borrowed until cache_complete after synchronization.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_decode_cache_begin(
    cache: *mut c_void,
    padded_tokens: c_int,
    width: c_int,
    userdata: *mut c_void,
    build: Option<Build>,
    destroy: Option<Destroy>,
) -> *mut c_void {
    guard(|| {
        if cache.is_null() || !cache.cast::<Owner>().is_aligned() {
            return std::ptr::null_mut();
        }
        let (Some(build), Some(destroy)) = (build, destroy) else {
            return std::ptr::null_mut();
        };
        // SAFETY: The serialized native worker exclusively borrows its one
        // original live Box<Owner> for this synchronous acquisition.
        let owner = unsafe { &mut *cache.cast::<Owner>() };
        owner
            .begin(padded_tokens, width, || {
                // SAFETY: The reviewed noexcept callback borrows userdata and
                // returns null after cleaning failed partial allocations, or
                // transfers one completed graph to the matching destroy callback.
                NonNull::new(unsafe { build(userdata) })
                    .map(|pointer| Resource { pointer, destroy })
            })
            .map_or(std::ptr::null_mut(), |resource| resource.pointer.as_ptr())
    })
}

// SAFETY: The native lease guard calls this exactly once after synchronizing
// backend work and completing all reads through its borrowed graph pointer.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_decode_cache_complete(cache: *mut c_void) {
    guard(|| {
        if cache.is_null() || !cache.cast::<Owner>().is_aligned() {
            std::process::abort();
        }
        // SAFETY: The lease guard retains its original live cache handle; no
        // cache mutation or destruction overlaps this completion call.
        let owner = unsafe { &mut *cache.cast::<Owner>() };
        if !owner.complete() {
            std::process::abort();
        }
    });
}
