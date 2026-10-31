//! Audited opaque ownership and borrowed-byte ABI for the safe Rust tokenizer.

use std::{
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
};

use crate::tokenizer::Tokenizer;

fn guard<T>(call: impl FnOnce() -> T) -> Option<T> {
    catch_unwind(AssertUnwindSafe(call)).ok()
}

// SAFETY: Byte spans borrow initialized native storage for one synchronous
// call. The caller supplies no alias to a mutable Rust owner/result. Empty
// spans may use null; all nonempty extents must fit isize and remain live.
const fn bytes<'a>(data: *const u8, length: usize) -> Option<&'a [u8]> {
    if length == 0 {
        return Some(&[]);
    }
    if data.is_null() || length > isize::MAX.unsigned_abs() {
        return None;
    }
    // SAFETY: The private native wrapper supplies the initialized, live byte
    // extent described above. No Rust reference to this storage escapes the call.
    Some(unsafe { std::slice::from_raw_parts(data, length) })
}

// SAFETY: Handles are exclusively owned Box<Tokenizer> pointers created below,
// live until their once-only release. Mutating calls are serialized by native
// setup; encoding uses an immutable finalized owner on its joined worker.
fn owner_mut<'a>(owner: *mut c_void) -> Option<&'a mut Tokenizer> {
    if owner.is_null() || !owner.cast::<Tokenizer>().is_aligned() {
        return None;
    }
    // SAFETY: The handle is a live, exclusively borrowed Box<Tokenizer> from
    // bonsai_qt_bpe_new; the reviewed wrapper never copies or aliases ownership.
    unsafe { owner.cast::<Tokenizer>().as_mut() }
}

// SAFETY: This unique private export transfers one Box owner to the native
// RAII wrapper; ordinary setup errors and Rust panics cannot unwind into C++.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_new() -> *mut c_void {
    guard(|| Box::into_raw(Box::new(Tokenizer::new())).cast()).unwrap_or(std::ptr::null_mut())
}

// SAFETY: Native calls this exactly once for an owner created above, after its
// worker has joined and all borrowed references/results have been released.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_free(owner: *mut c_void) {
    if !owner.is_null() {
        let _ = guard(|| {
            // SAFETY: The reviewed move-disabled native owner provides this
            // exact original Box pointer once, with no outstanding borrows.
            drop(unsafe { Box::from_raw(owner.cast::<Tokenizer>()) });
        });
    }
}

fn add(owner: *mut c_void, data: *const u8, length: usize, id: i32, merge: bool) -> bool {
    guard(|| {
        let Some(owner) = owner_mut(owner) else {
            return false;
        };
        let Some(bytes) = bytes(data, length) else {
            return false;
        };
        if merge {
            owner.add_merge(bytes, id).is_ok()
        } else {
            owner.add_token(bytes, id).is_ok()
        }
    })
    .unwrap_or(false)
}

// SAFETY: Native supplies one exclusive owner and a disjoint borrowed GGUF
// string extent. Rust copies the bounded spelling into its sole table owner.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_add_token(
    owner: *mut c_void,
    data: *const u8,
    length: usize,
    id: i32,
) -> bool {
    add(owner, data, length, id, false)
}

// SAFETY: Same borrowed storage contract as add_token, with the native merge
// array index passed as the rank. No native pointer is retained.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_add_merge(
    owner: *mut c_void,
    data: *const u8,
    length: usize,
    rank: i32,
) -> bool {
    add(owner, data, length, rank, true)
}

// SAFETY: Native serializes setup on one exclusive owner. Failure rejects the
// candidate owner, which RAII releases without publishing a partial tokenizer.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_finish(owner: *mut c_void) -> bool {
    guard(|| owner_mut(owner).is_some_and(|owner| owner.finish().is_ok())).unwrap_or(false)
}

// SAFETY: The spelling span is live and disjoint from the exclusively borrowed
// owner. The native setup caller retains neither Rust references nor table copies.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_add_special(
    owner: *mut c_void,
    data: *const u8,
    length: usize,
    id: i32,
) -> bool {
    guard(|| {
        let Some(owner) = owner_mut(owner) else {
            return false;
        };
        let Some(bytes) = bytes(data, length) else {
            return false;
        };
        owner.add_special(bytes, id).is_ok()
    })
    .unwrap_or(false)
}

// SAFETY: Native supplies one exclusive owner and a scalar GGUF token ID.
// Rust owns reverse lookup, invalid/empty no-op handling and duplicate policy.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_add_special_id(owner: *mut c_void, id: i32) -> bool {
    guard(|| owner_mut(owner).is_some_and(|owner| owner.add_special_id(id).is_ok()))
        .unwrap_or(false)
}

// SAFETY: Encoding immutably borrows a live finalized owner and disjoint text
// span. The returned result owns all IDs independently; no input borrow escapes.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_encode(
    owner: *const c_void,
    data: *const u8,
    length: usize,
    add_eos: bool,
) -> *mut c_void {
    guard(|| {
        if owner.is_null() || !owner.cast::<Tokenizer>().is_aligned() {
            return std::ptr::null_mut();
        }
        let Some(bytes) = bytes(data, length) else {
            return std::ptr::null_mut();
        };
        // SAFETY: Native setup has finished and its single worker immutably
        // borrows this live owner. No mutation or destruction overlaps the call.
        let owner = unsafe { &*owner.cast::<Tokenizer>() };
        owner
            .encode(bytes, add_eos)
            .ok()
            .map_or(std::ptr::null_mut(), |ids| {
                Box::into_raw(Box::new(ids)).cast()
            })
    })
    .unwrap_or(std::ptr::null_mut())
}

fn result<'a>(result: *const c_void) -> Option<&'a Vec<i32>> {
    if result.is_null() || !result.cast::<Vec<i32>>().is_aligned() {
        return None;
    }
    // SAFETY: The native result RAII guard retains one Box<Vec<i32>> produced
    // by encode, with no mutation or destruction until the borrowed IDs are copied.
    unsafe { result.cast::<Vec<i32>>().as_ref() }
}

// SAFETY: Native borrows the live result for its synchronous vector copy only.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_result_data(owner: *const c_void) -> *const i32 {
    result(owner).map_or(std::ptr::null(), |ids| {
        if ids.is_empty() {
            std::ptr::null()
        } else {
            ids.as_ptr()
        }
    })
}

// SAFETY: Native queries only a live opaque result created by encode.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_result_len(owner: *const c_void) -> usize {
    result(owner).map_or(0, Vec::len)
}

// SAFETY: Native releases each result exactly once after copying its ID range;
// the result allocation and its payload were both allocated by Rust.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_result_free(owner: *mut c_void) {
    if !owner.is_null() {
        let _ = guard(|| {
            // SAFETY: The native result RAII guard transfers the original Box
            // pointer back exactly once after the data borrow has ended.
            drop(unsafe { Box::from_raw(owner.cast::<Vec<i32>>()) });
        });
    }
}

#[cfg(test)]
pub(super) fn copy_result_for_test(owner: *const c_void) -> Option<Vec<i32>> {
    result(owner).cloned()
}
