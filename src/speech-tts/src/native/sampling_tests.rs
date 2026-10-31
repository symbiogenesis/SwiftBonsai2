//! Exact ABI comparisons against the unmodified pinned C++ masking/penalty code.
#![forbid(unsafe_code)]

use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use super::sampling::{bonsai_qt_apply_repetition_penalty, bonsai_qt_apply_suppress};

struct Case {
    logits: Vec<f32>,
    history: Vec<i32>,
    penalty: f32,
    low: i32,
    high: i32,
    keep: i32,
}

fn compile_oracle(working: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let compiler = PathBuf::from(
        env::var_os("BONSAI_TTS_COMPILER")
            .or_else(|| option_env!("BONSAI_TTS_COMPILER").map(Into::into))
            .ok_or("Set the provisioned BONSAI_TTS_COMPILER for the sampling oracle.")?,
    );
    assert!(compiler.is_absolute() && compiler.is_file());
    fs::write(
        working.join("philox.h"),
        include_bytes!("../../tests/reference/philox.h"),
    )?;
    fs::write(
        working.join("sampling.h"),
        include_bytes!("../../tests/reference/sampling.h"),
    )?;
    fs::write(
        working.join("oracle.cpp"),
        include_bytes!("../../tests/sampling_oracle.cpp"),
    )?;
    let executable = working.join("oracle.exe");
    let compile = Command::new(compiler)
        .args([
            "/nologo",
            "/std:c++17",
            "/EHsc",
            "/W4",
            "/WX",
            "/O2",
            "/fp:precise",
        ])
        .arg(working.join("oracle.cpp"))
        .arg(format!("/Fe{}", executable.display()))
        .current_dir(working)
        .output()?;
    fs::write(working.join("compiler.stdout"), &compile.stdout)?;
    fs::write(working.join("compiler.stderr"), &compile.stderr)?;
    assert!(
        compile.status.success(),
        "Sampling oracle compilation failed; retained {}",
        working.display()
    );
    Ok(executable)
}

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let edge = vec![
        0.0,
        -0.0,
        4.0,
        -4.0,
        f32::MIN_POSITIVE,
        f32::from_bits(1),
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::from_bits(0x7fc0_1234),
    ];
    for penalty in [
        0.0,
        -0.0,
        1.0,
        1.05,
        2.0,
        0.5,
        -2.0,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::from_bits(0x7fc0_2345),
    ] {
        for (low, high, keep) in [
            (i32::MIN, i32::MAX, 2),
            (0, 0, -1),
            (3, 1, 2),
            (0, -1, -1),
            (20, 30, -1),
            (0, 9, 8),
            (2, 7, 4),
        ] {
            for history in [
                vec![],
                vec![-1, i32::MIN, i32::MAX, 9],
                vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 2, 3, 0],
            ] {
                cases.push(Case {
                    logits: edge.clone(),
                    history,
                    penalty,
                    low,
                    high,
                    keep,
                });
            }
        }
    }
    // Bounded deterministic corpus independent of either policy implementation.
    let mut state = 0x1234_5678_u32;
    for index in 0..512 {
        let width = index % 67;
        let mut logits = Vec::new();
        let mut history = Vec::new();
        for _ in 0..width {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            logits.push(f32::from_bits(state));
            history.push(i32::from_ne_bytes((state % 80).to_ne_bytes()) - 8);
        }
        cases.push(Case {
            logits,
            history,
            penalty: 1.05,
            low: 7,
            high: 31,
            keep: 12,
        });
    }
    cases
}

#[test]
fn rust_sampler_helpers_match_pinned_cpp_bits() -> Result<(), Box<dyn Error>> {
    let working = env::temp_dir().join(format!(
        "Bonsai-Sampling-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    assert!(working.is_absolute() && !working.to_string_lossy().contains('%'));
    fs::create_dir(&working)?;
    let executable = compile_oracle(&working)?;
    let cases = cases();
    let mut input = Vec::new();
    let mut actual = Vec::new();
    for case in cases {
        let vocabulary = i32::try_from(case.logits.len())?;
        let history_count = i32::try_from(case.history.len())?;
        input.extend(vocabulary.to_le_bytes());
        input.extend(history_count.to_le_bytes());
        input.extend(case.penalty.to_bits().to_le_bytes());
        input.extend(case.low.to_le_bytes());
        input.extend(case.high.to_le_bytes());
        input.extend(case.keep.to_le_bytes());
        input.extend(
            case.logits
                .iter()
                .flat_map(|logit| logit.to_bits().to_le_bytes()),
        );
        input.extend(case.history.iter().flat_map(|token| token.to_le_bytes()));
        let mut logits = case.logits;
        let mut workspace = vec![255; logits.len()];
        bonsai_qt_apply_suppress(
            logits.as_mut_ptr(),
            vocabulary,
            case.low,
            case.high,
            case.keep,
        );
        actual.extend(
            logits
                .iter()
                .flat_map(|logit| logit.to_bits().to_le_bytes()),
        );
        bonsai_qt_apply_repetition_penalty(
            logits.as_mut_ptr(),
            vocabulary,
            case.history.as_ptr(),
            history_count,
            case.penalty,
            workspace.as_mut_ptr(),
        );
        actual.extend(
            logits
                .iter()
                .flat_map(|logit| logit.to_bits().to_le_bytes()),
        );
    }
    let input_path = working.join("cases.bin");
    fs::write(&input_path, input)?;
    let oracle = Command::new(executable)
        .arg(input_path)
        .current_dir(&working)
        .output()?;
    fs::write(working.join("oracle.stderr"), &oracle.stderr)?;
    assert!(
        oracle.status.success(),
        "Sampling oracle failed; retained {}",
        working.display()
    );
    assert_eq!(actual.len(), oracle.stdout.len());
    for (index, (actual, expected)) in actual
        .as_chunks::<4>()
        .0
        .iter()
        .zip(oracle.stdout.as_chunks::<4>().0)
        .enumerate()
    {
        assert_eq!(
            actual,
            expected,
            "Sampler bits differ at output word {index}; retained {}",
            working.display()
        );
    }
    fs::remove_dir_all(&working)?;
    Ok(())
}

#[test]
fn empty_and_no_op_sampler_abi_calls_accept_null_storage() {
    bonsai_qt_apply_suppress(std::ptr::null_mut(), 0, i32::MIN, i32::MAX, -1);
    for history_count in [-1, 0] {
        bonsai_qt_apply_repetition_penalty(
            std::ptr::null_mut(),
            8,
            std::ptr::null(),
            history_count,
            2.0,
            std::ptr::null_mut(),
        );
    }
    bonsai_qt_apply_repetition_penalty(
        std::ptr::null_mut(),
        8,
        std::ptr::null(),
        8,
        1.0,
        std::ptr::null_mut(),
    );
    bonsai_qt_apply_repetition_penalty(
        std::ptr::null_mut(),
        0,
        std::ptr::null(),
        8,
        2.0,
        std::ptr::null_mut(),
    );
}
