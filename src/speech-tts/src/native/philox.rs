//! Allocation-free scalar ABI for the safe Rust Philox generator.

// SAFETY: This unique private static-link symbol is declared by the reviewed
// Qwen overlay. Scalar arguments carry no pointer or lifetime obligations.
// The pure Rust implementation allocates nothing and cannot unwind.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_philox_uniform(
    seed: i64,
    subsequence: i64,
    counter: u32,
) -> f32 {
    crate::philox::uniform(seed, subsequence, counter)
}
