//! Allocation-free native adapters for the safe Qwen sampler policy.
#![allow(
    clippy::float_cmp,
    reason = "Exact comparison with one is the pinned sampler's required no-op branch."
)]

use std::ffi::c_int;

// SAFETY: The pinned greedy sampler supplies vocabulary initialized f32s,
// borrowed read-only for this synchronous call. No reference or pointer escapes.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_greedy_argmax(logits: *const f32, vocabulary: c_int) -> c_int {
    let vocabulary = usize::try_from(vocabulary).unwrap_or_else(|_| std::process::abort());
    if vocabulary == 0 {
        return 0;
    }
    if logits.is_null() || !logits.is_aligned() {
        std::process::abort();
    }
    // SAFETY: The native caller retains the aligned initialized vocabulary
    // extent throughout this call, with no concurrent mutation or deallocation.
    let logits = unsafe { std::slice::from_raw_parts(logits, vocabulary) };
    c_int::try_from(super::super::sampling::greedy_argmax(logits))
        .unwrap_or_else(|_| std::process::abort())
}

// SAFETY: This private static-link ABI is called only by the pinned sampler
// wrapper. For positive vocabulary, logits points to that many exclusively
// borrowed initialized f32s. The wrapper retains storage for this synchronous
// call; scalar interval arguments impose no further lifetime obligations.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_apply_suppress(
    logits: *mut f32,
    vocabulary: c_int,
    low: c_int,
    high: c_int,
    keep: c_int,
) {
    let vocabulary = usize::try_from(vocabulary).unwrap_or_else(|_| std::process::abort());
    if vocabulary == 0 {
        return;
    }
    if logits.is_null() || !logits.is_aligned() {
        std::process::abort();
    }
    // SAFETY: The reviewed native wrapper supplies the live, aligned and
    // exclusively borrowed vocabulary extent described above. Windows ARM64
    // c_int-sized f32 extents fit within isize::MAX.
    let logits = unsafe { std::slice::from_raw_parts_mut(logits, vocabulary) };
    super::super::sampling::suppress(logits, low, high, keep);
}

// SAFETY: The pinned wrapper supplies disjoint initialized storage: vocabulary
// writable f32 logits, history_count readable i32 history, and vocabulary
// writable workspace bytes. All borrows end before this synchronous call
// returns. Empty/no-op calls may supply null pointers and never dereference them.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_apply_repetition_penalty(
    logits: *mut f32,
    vocabulary: c_int,
    history: *const i32,
    history_count: c_int,
    penalty: f32,
    seen: *mut u8,
) {
    if penalty == 1.0 || history_count <= 0 {
        return;
    }
    let vocabulary = usize::try_from(vocabulary).unwrap_or_else(|_| std::process::abort());
    if vocabulary == 0 {
        return;
    }
    let history_count = usize::try_from(history_count).unwrap_or_else(|_| std::process::abort());
    if logits.is_null()
        || !logits.is_aligned()
        || history.is_null()
        || !history.is_aligned()
        || seen.is_null()
    {
        std::process::abort();
    }
    // SAFETY: The wrapper retains exclusive logits storage, disjoint from
    // history and workspace, for exactly the declared vocabulary extent.
    let logits = unsafe { std::slice::from_raw_parts_mut(logits, vocabulary) };
    // SAFETY: The wrapper's live i32 history covers history_count entries and
    // is disjoint from both writable regions for this synchronous call.
    let history = unsafe { std::slice::from_raw_parts(history, history_count) };
    // SAFETY: The native thread-local vector supplies vocabulary initialized
    // bytes, exclusively borrowed and disjoint from logits and history.
    let seen = unsafe { std::slice::from_raw_parts_mut(seen, vocabulary) };
    if !super::super::sampling::repetition_penalty(logits, history, penalty, seen) {
        std::process::abort();
    }
}
