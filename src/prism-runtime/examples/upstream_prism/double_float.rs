//! Source-local numerical proof from test-double-float.cpp, in safe Rust.
//!
//! The upstream source calls no engine API. Its Windows ARM64 path exhausts
//! all binary32 patterns and compares float/double rounding. Its F16C-only
//! half-precision branch is outside the requested ARM64 target.
//!
//! C++ float-to-int8 conversion is undefined outside [-128,127]. Instead of
//! inventing a result for that cast, compare the rounded numbers themselves:
//! equality proves the original assertion wherever its cast is defined and
//! imposes a stronger, defined condition on every other finite input.
//! Upstream MIT attribution: tools/prism/upstream/LICENSE.

use serde_json::{Value, json};

use super::engine::Engine;
use super::{Result, invalid, result};

const ID: &str = "tests/test-double-float.cpp";
const PATTERNS: u64 = 1_u64 << 32;
const FINITE: u64 = 2 * 255 * (1 << 23);

fn rounded(value: f32) -> (f64, f64) {
    (f64::from(value).round(), f64::from(value.round()))
}

pub(super) fn run(_engine: &Engine) -> Result<Value> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("GITHUB_REF").as_deref() != Ok("refs/heads/main")
    {
        return Err(invalid(
            "the exhaustive binary32 proof runs only in main-branch CI",
        ));
    }
    if !cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        return Err(invalid(
            "this source port covers the requested Windows ARM64 path",
        ));
    }
    let mut visited = 0_u64;
    let mut finite = 0_u64;
    let mut undefined_casts = 0_u64;
    let mut failure = None;
    for bits in (0..=u32::MAX).rev() {
        visited += 1;
        let value = f32::from_bits(bits);
        if value.is_finite() {
            finite += 1;
            let (double, single) = rounded(value);
            undefined_casts += u64::from(!(-128.0..128.0).contains(&double));
            #[allow(
                clippy::float_cmp,
                reason = "The original proof requires exact rounding equality, not approximate error."
            )]
            if double != single {
                failure =
                    Some(json!({"InputBits":bits,"DoubleRounded":double,"FloatRounded":single}));
                break;
            }
        }
    }
    let passed = visited == PATTERNS && finite == FINITE && failure.is_none();
    Ok(result(ID, passed, json!({
        "SourceSha256":"1a967f5a89c959260d1884740ac0b2f25a8db9dcc96f4c6eed05fcdb35c9627b",
        "Scope":"source-local arithmetic proof; no upstream or Rust runtime engine API exists in this source",
        "VisitedPatterns":visited,"ExpectedPatterns":PATTERNS,"FinitePredicates":finite,"ExpectedFinitePredicates":FINITE,
        "UndefinedOriginalCastInputs":undefined_casts,"DefinedOriginalCastInputs":finite-undefined_casts,
        "Comparison":"exact rounded-number equality; implies original int8 equality on its defined domain",
        "ExcludedBranch":"x86 F16C half-SiLU code does not apply to Windows ARM64",
        "FirstFailure":failure,
    }).to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_boundary_and_tie_rounding_witnesses() {
        for bits in [
            0,
            0x8000_0000,
            1,
            0x007f_ffff,
            0x0080_0000,
            0x3eff_ffff,
            0x3f00_0000,
            0x3f00_0001,
            0xbf00_0000,
            0x42ff_0000,
            0xc300_0000,
            0x7f7f_ffff,
            0xff7f_ffff,
        ] {
            let (double, single) = rounded(f32::from_bits(bits));
            assert_eq!(double.to_bits(), single.to_bits());
        }
        assert_eq!(FINITE, 4_278_190_080);
        assert_eq!(PATTERNS - FINITE, 16_777_216);
    }
}
