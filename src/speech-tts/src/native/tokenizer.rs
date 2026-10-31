//! Audited opaque ownership and borrowed-byte ABI for the safe Rust tokenizer.

use std::{
    ffi::c_void,
    io,
    panic::{AssertUnwindSafe, catch_unwind},
};

use crate::tokenizer::Tokenizer;

#[path = "tokenizer_loading.rs"]
mod loading;

struct Owner {
    tokenizer: Tokenizer,
}

#[cfg(test)]
std::thread_local! {
    static OWNER_COUNTS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
    static RESULT_COUNTS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
    static REJECT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Drop for Owner {
    fn drop(&mut self) {
        OWNER_COUNTS.with(|counts| {
            let (created, freed) = counts.get();
            counts.set((created, freed + 1));
        });
    }
}

#[cfg(test)]
fn reject_stage(stage: u8) -> io::Result<()> {
    if REJECT.with(|reject| reject.get() == stage) {
        return Err(io::Error::other("Injected tokenizer construction failure."));
    }
    Ok(())
}

#[cfg(not(test))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "Keep the production construction path identical to test-only stage fault injection."
)]
const fn reject_stage(_: u8) -> io::Result<()> {
    Ok(())
}

fn new_owner() -> io::Result<Box<Owner>> {
    reject_stage(1)?;
    let owner = Box::new(Owner {
        tokenizer: Tokenizer::new(),
    });
    #[cfg(test)]
    OWNER_COUNTS.with(|counts| {
        let (created, freed) = counts.get();
        counts.set((created + 1, freed));
    });
    Ok(owner)
}

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

// SAFETY: Handles are exclusively owned Box<Owner> pointers created below,
// live until their once-only release. Mutating calls are serialized by native
// setup; encoding uses an immutable finalized owner on its joined worker.
fn owner_mut<'a>(owner: *mut c_void) -> Option<&'a mut Tokenizer> {
    if owner.is_null() || !owner.cast::<Owner>().is_aligned() {
        return None;
    }
    // SAFETY: The handle is a live, exclusively borrowed Box<Owner> from
    // bonsai_qt_bpe_new; the reviewed wrapper never copies or aliases ownership.
    unsafe { owner.cast::<Owner>().as_mut() }.map(|owner| &mut owner.tokenizer)
}

// SAFETY: This unique private export transfers one Box owner to the native
// RAII wrapper; ordinary setup errors and Rust panics cannot unwind into C++.
#[cfg(test)]
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_new() -> *mut c_void {
    guard(|| new_owner().map_or(std::ptr::null_mut(), |owner| Box::into_raw(owner).cast()))
        .unwrap_or(std::ptr::null_mut())
}

// SAFETY: Native calls this exactly once for an owner created above, after its
// worker has joined and all borrowed references/results have been released.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_free(owner: *mut c_void) {
    if !owner.is_null() {
        let _ = guard(|| {
            // SAFETY: The reviewed move-disabled native owner provides this
            // exact original Box pointer once, with no outstanding borrows.
            drop(unsafe { Box::from_raw(owner.cast::<Owner>()) });
        });
    }
}

#[cfg(test)]
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
#[cfg(test)]
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
#[cfg(test)]
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
#[cfg(test)]
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
        if reject_stage(5).is_err() {
            return false;
        }
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
#[cfg(test)]
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_add_special_id(owner: *mut c_void, id: i32) -> bool {
    guard(|| owner_mut(owner).is_some_and(|owner| owner.add_special_id(id).is_ok()))
        .unwrap_or(false)
}

// SAFETY: Encoding immutably borrows a live finalized owner and disjoint text
// span. The returned result owns all IDs independently; no input borrow escapes.
#[cfg(test)]
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_encode(
    owner: *const c_void,
    data: *const u8,
    length: usize,
    add_eos: bool,
) -> *mut c_void {
    guard(|| {
        if owner.is_null() || !owner.cast::<Owner>().is_aligned() {
            return std::ptr::null_mut();
        }
        let Some(bytes) = bytes(data, length) else {
            return std::ptr::null_mut();
        };
        // SAFETY: Native setup has finished and its single worker immutably
        // borrows this live owner. No mutation or destruction overlaps the call.
        let owner = &unsafe { &*owner.cast::<Owner>() }.tokenizer;
        owner
            .encode(bytes, add_eos)
            .ok()
            .map_or(std::ptr::null_mut(), |ids| {
                Box::into_raw(Box::new(ids)).cast()
            })
    })
    .unwrap_or(std::ptr::null_mut())
}

struct Encoding(Vec<i32>);

#[cfg(test)]
impl Drop for Encoding {
    fn drop(&mut self) {
        RESULT_COUNTS.with(|counts| {
            let (created, freed) = counts.get();
            counts.set((created, freed + 1));
        });
    }
}

fn encode_to(
    owner: &Tokenizer,
    text: &[u8],
    add_eos: bool,
    copy: impl FnOnce(&[i32]) -> bool,
) -> io::Result<bool> {
    let ids = Encoding(owner.encode(text, add_eos)?);
    #[cfg(test)]
    RESULT_COUNTS.with(|counts| {
        let (created, freed) = counts.get();
        counts.set((created + 1, freed));
    });
    Ok(copy(&ids.0))
}

// SAFETY: Native borrows one finalized Owner and initialized text bytes for
// this synchronous call. copy must not unwind or retain the borrowed ID span;
// its userdata belongs exclusively to the native caller until return.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_encode_to(
    owner: *const c_void,
    data: *const u8,
    length: usize,
    add_eos: bool,
    userdata: *mut c_void,
    copy: Option<unsafe extern "C" fn(*mut c_void, *const i32, usize) -> bool>,
) -> bool {
    guard(|| {
        if owner.is_null() || !owner.cast::<Owner>().is_aligned() {
            return false;
        }
        let (Some(bytes), Some(copy)) = (bytes(data, length), copy) else {
            return false;
        };
        // SAFETY: The generated native object borrows its one live Owner
        // immutably; setup, replacement and destruction cannot overlap encoding.
        let owner = &unsafe { &*owner.cast::<Owner>() }.tokenizer;
        encode_to(owner, bytes, add_eos, |ids| {
            let pointer = if ids.is_empty() {
                std::ptr::null()
            } else {
                ids.as_ptr()
            };
            // SAFETY: The caller's noexcept callback copies only this live Rust
            // Vec's initialized span and cannot retain its pointer. Rust owns
            // and releases the result even if copying reports failure.
            unsafe { copy(userdata, pointer, ids.len()) }
        })
        .unwrap_or(false)
    })
    .unwrap_or(false)
}

#[cfg(test)]
pub(super) fn encode_copy_for_test(
    owner: *const c_void,
    text: &[u8],
    add_eos: bool,
) -> Option<Vec<i32>> {
    unsafe extern "C" fn copy(userdata: *mut c_void, data: *const i32, count: usize) -> bool {
        // SAFETY: The test helper exclusively borrows its local Vec through
        // userdata until the synchronous export returns.
        let output = unsafe { &mut *userdata.cast::<Vec<i32>>() };
        if count != 0 {
            // SAFETY: The export supplies exactly count initialized live IDs;
            // nonempty results always use the Vec's nonnull aligned pointer.
            output.extend_from_slice(unsafe { std::slice::from_raw_parts(data, count) });
        }
        true
    }
    let mut output = Vec::new();
    bonsai_qt_bpe_encode_to(
        owner,
        text.as_ptr(),
        text.len(),
        add_eos,
        (&raw mut output).cast(),
        Some(copy),
    )
    .then_some(output)
}

#[cfg(test)]
fn result<'a>(result: *const c_void) -> Option<&'a Vec<i32>> {
    if result.is_null() || !result.cast::<Vec<i32>>().is_aligned() {
        return None;
    }
    // SAFETY: The native result RAII guard retains one Box<Vec<i32>> produced
    // by encode, with no mutation or destruction until the borrowed IDs are copied.
    unsafe { result.cast::<Vec<i32>>().as_ref() }
}

// SAFETY: Native borrows the live result for its synchronous vector copy only.
#[cfg(test)]
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
#[cfg(test)]
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_result_len(owner: *const c_void) -> usize {
    result(owner).map_or(0, Vec::len)
}

// SAFETY: Native releases each result exactly once after copying its ID range;
// the result allocation and its payload were both allocated by Rust.
#[cfg(test)]
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
