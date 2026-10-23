//! C ABI for the pinned engine's tensor creation boundary.
//!
//! Unsafe code is confined to translating caller-owned pointers and allocating
//! the opaque handle. The policy receives copied layout values and byte slices;
//! it never dereferences ggml contexts, buffer types, or owner tokens.

use crate::{Layout, Pool};
use std::alloc::{Layout as AllocationLayout, alloc};
use std::slice;

/// Value-only tensor description matching `bonsai_prism_layout` in the header.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BonsaiPrismLayout {
    /// Identity of the context in which a tensor will be created.
    pub context: usize,
    /// Identity of the requested ggml buffer type.
    pub buffer_type: usize,
    /// Whether this is the ordinary ggml CPU buffer type.
    pub cpu: bool,
    /// Whether the source tensor has ggml's F32 type.
    pub f32: bool,
    /// Whether ggml considers the source tensor contiguous.
    pub contiguous: bool,
    /// Dimensions in ggml order.
    pub dimensions: [i64; 4],
    /// Byte strides in ggml order.
    pub strides: [usize; 4],
    /// Total source tensor payload size.
    pub byte_len: usize,
}

impl From<BonsaiPrismLayout> for Layout {
    fn from(value: BonsaiPrismLayout) -> Self {
        Self {
            context: value.context,
            buffer_type: value.buffer_type,
            cpu: value.cpu,
            f32: value.f32,
            contiguous: value.contiguous,
            dimensions: value.dimensions,
            strides: value.strides,
            byte_len: value.byte_len,
        }
    }
}

/// Opaque, single-threaded C handle. Callers must use the exported functions.
pub struct BonsaiPrismPool {
    pool: Pool,
}

/// Allocates an empty sharing pool, returning null on allocation failure.
#[unsafe(no_mangle)]
pub extern "C" fn bonsai_prism_pool_new() -> *mut BonsaiPrismPool {
    // SAFETY: The nonzero layout has the correct alignment for the handle. A
    // successful allocation is initialized exactly once before being returned.
    unsafe {
        #[allow(
            clippy::cast_ptr_alignment,
            reason = "The allocation explicitly requests the handle alignment"
        )]
        let result = alloc(AllocationLayout::new::<BonsaiPrismPool>()).cast::<BonsaiPrismPool>();
        if !result.is_null() {
            result.write(BonsaiPrismPool { pool: Pool::new() });
        }
        result
    }
}

/// Frees a pool; a null handle is accepted.
///
/// # Safety
/// A nonnull handle must come from `bonsai_prism_pool_new`, must not have been
/// freed, and must have no outstanding access. Each handle may be freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bonsai_prism_pool_free(pool: *mut BonsaiPrismPool) {
    if !pool.is_null() {
        // SAFETY: The caller transfers exclusive ownership of the allocation
        // created with the global allocator and initialized by pool_new.
        unsafe { drop(Box::from_raw(pool)) };
    }
}

/// Reports whether a copied source layout is eligible for sharing.
///
/// # Safety
/// A nonnull layout must point to a live, aligned, initialized description.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bonsai_prism_pool_eligible(layout: *const BonsaiPrismLayout) -> bool {
    // SAFETY: Nonnull pointer validity and alignment are required of the caller.
    unsafe { layout.as_ref() }.is_some_and(|value| Layout::from(*value).eligible())
}

/// Finds an exact match and counts its saved payload, or returns zero.
///
/// # Safety
/// Nonnull pointers must be valid and aligned for their types for this call.
/// The pool must be exclusively borrowed; `bytes` must reference `len` readable
/// initialized bytes. Inputs must not overlap the pool allocation or its cache.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bonsai_prism_pool_find(
    pool: *mut BonsaiPrismPool,
    layout: *const BonsaiPrismLayout,
    bytes: *const u8,
    len: usize,
) -> usize {
    // SAFETY: Pointer validity and nonaliasing are part of the caller contract.
    let (Some(pool), Some(layout)) = (unsafe { pool.as_mut() }, unsafe { layout.as_ref() }) else {
        return 0;
    };
    let layout = Layout::from(*layout);
    if bytes.is_null() || len != layout.byte_len || !layout.eligible() {
        return 0;
    }
    // SAFETY: Eligibility bounds len to 64 KiB, below isize::MAX. The caller
    // guarantees a readable allocation of len bytes disjoint from pool state.
    let bytes = unsafe { slice::from_raw_parts(bytes, len) };
    pool.pool.find(&layout, bytes).unwrap_or(0)
}

/// Caches a canonical owner, returning false when it cannot be remembered.
///
/// # Safety
/// Pointer and borrowing requirements are the same as `bonsai_prism_pool_find`.
/// A nonzero owner token must remain live for the pool's entire lifetime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bonsai_prism_pool_remember(
    pool: *mut BonsaiPrismPool,
    layout: *const BonsaiPrismLayout,
    bytes: *const u8,
    len: usize,
    owner: usize,
) -> bool {
    // SAFETY: Pointer validity and nonaliasing are part of the caller contract.
    let (Some(pool), Some(layout)) = (unsafe { pool.as_mut() }, unsafe { layout.as_ref() }) else {
        return false;
    };
    let layout = Layout::from(*layout);
    if owner == 0 || bytes.is_null() || len != layout.byte_len || !layout.eligible() {
        return false;
    }
    // SAFETY: Eligibility bounds len to 64 KiB, below isize::MAX. The caller
    // guarantees a readable allocation of len bytes disjoint from pool state.
    let bytes = unsafe { slice::from_raw_parts(bytes, len) };
    pool.pool.remember(&layout, bytes, owner)
}

/// Returns the number of matching tensors, or zero for a null pool.
///
/// # Safety
/// A nonnull pool must be a live handle and cannot be concurrently mutated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bonsai_prism_pool_shared_tensors(pool: *const BonsaiPrismPool) -> usize {
    // SAFETY: The caller guarantees validity and a shared borrow of the handle.
    unsafe { pool.as_ref() }.map_or(0, |value| value.pool.shared_tensors())
}

/// Returns saved tensor payload bytes, or zero for a null pool.
///
/// # Safety
/// A nonnull pool must be a live handle and cannot be concurrently mutated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bonsai_prism_pool_shared_bytes(pool: *const BonsaiPrismPool) -> usize {
    // SAFETY: The caller guarantees validity and a shared borrow of the handle.
    unsafe { pool.as_ref() }.map_or(0, |value| value.pool.shared_bytes())
}

/// Returns cached comparison payload bytes, or zero for a null pool.
///
/// # Safety
/// A nonnull pool must be a live handle and cannot be concurrently mutated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bonsai_prism_pool_cached_bytes(pool: *const BonsaiPrismPool) -> usize {
    // SAFETY: The caller guarantees validity and a shared borrow of the handle.
    unsafe { pool.as_ref() }.map_or(0, |value| value.pool.cached_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn ffi_null_and_exact_match() {
        let pool = bonsai_prism_pool_new();
        assert!(!pool.is_null());
        let bytes = 1.0_f32.to_ne_bytes();
        let layout = BonsaiPrismLayout {
            context: 1,
            buffer_type: 2,
            cpu: true,
            f32: true,
            contiguous: true,
            dimensions: [1; 4],
            strides: [4; 4],
            byte_len: 4,
        };
        // SAFETY: All nonnull inputs are live and disjoint; pool is exclusively
        // accessed and freed once. The owner is an opaque nonzero test identity.
        unsafe {
            assert!(!bonsai_prism_pool_eligible(ptr::null()));
            assert_eq!(bonsai_prism_pool_shared_tensors(ptr::null()), 0);
            assert_eq!(bonsai_prism_pool_shared_bytes(ptr::null()), 0);
            assert_eq!(bonsai_prism_pool_cached_bytes(ptr::null()), 0);
            assert_eq!(
                bonsai_prism_pool_find(pool, ptr::null(), bytes.as_ptr(), 4),
                0
            );
            assert_eq!(
                bonsai_prism_pool_find(pool, &raw const layout, ptr::null(), 4),
                0
            );
            assert_eq!(
                bonsai_prism_pool_find(ptr::null_mut(), &raw const layout, bytes.as_ptr(), 4),
                0
            );
            assert!(!bonsai_prism_pool_remember(
                pool,
                &raw const layout,
                bytes.as_ptr(),
                3,
                9
            ));
            assert!(!bonsai_prism_pool_remember(
                pool,
                &raw const layout,
                bytes.as_ptr(),
                4,
                0
            ));
            assert!(bonsai_prism_pool_remember(
                pool,
                &raw const layout,
                bytes.as_ptr(),
                4,
                9
            ));
            assert_eq!(
                bonsai_prism_pool_find(pool, &raw const layout, bytes.as_ptr(), 4),
                9
            );
            assert_eq!(bonsai_prism_pool_shared_tensors(pool), 1);
            assert_eq!(bonsai_prism_pool_shared_bytes(pool), 4);
            assert_eq!(bonsai_prism_pool_cached_bytes(pool), 4);
            bonsai_prism_pool_free(pool);
            bonsai_prism_pool_free(ptr::null_mut());
        }
    }
}
