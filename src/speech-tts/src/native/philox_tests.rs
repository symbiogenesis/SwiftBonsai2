//! Model-free exact comparison with the unmodified pinned C++ generator.
#![forbid(unsafe_code)]

use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};

fn compile_oracle(working: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let compiler = PathBuf::from(
        env::var_os("BONSAI_TTS_COMPILER")
            .or_else(|| option_env!("BONSAI_TTS_COMPILER").map(Into::into))
            .ok_or("Set the provisioned BONSAI_TTS_COMPILER for the Philox oracle.")?,
    );
    assert!(compiler.is_absolute() && compiler.is_file());
    fs::write(
        working.join("philox.h"),
        include_bytes!("../../tests/reference/philox.h"),
    )?;
    fs::write(
        working.join("oracle.cpp"),
        include_bytes!("../../tests/philox_oracle.cpp"),
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
        "Oracle compilation failed; retained {}",
        working.display()
    );
    Ok(executable)
}

fn draw_cases() -> Vec<(i64, i64, u32)> {
    let mut cases = Vec::new();
    for seed in [0, 1, 42, -1, i64::MIN, i64::MAX, 0x1234_5678_9abc_def0] {
        for subsequence in [
            0,
            1,
            15,
            16,
            -1,
            i64::MIN,
            i64::MAX,
            0xffff_ffff,
            0x1_0000_0000,
        ] {
            for counter in [0, 1, 0x7fff_ffff, 0x8000_0000, u32::MAX] {
                cases.push((seed, subsequence, counter));
            }
        }
    }
    // Fixed, bounded integer corpus, independent of the generator under test.
    let mut state = 0x1234_5678_9abc_def0_u64;
    for _ in 0..4096 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let seed = i64::from_le_bytes(state.to_le_bytes());
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let subsequence = i64::from_le_bytes(state.to_le_bytes());
        let [a, b, c, d, ..] = state.to_le_bytes();
        cases.push((seed, subsequence, u32::from_le_bytes([a, b, c, d])));
    }
    cases
}

#[test]
fn rust_philox_scalar_abi_matches_pinned_cpp_bits() -> Result<(), Box<dyn Error>> {
    let working = env::temp_dir().join(format!(
        "Bonsai-Philox-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_nanos()
    ));
    assert!(working.is_absolute() && !working.to_string_lossy().contains('%'));
    fs::create_dir(&working)?;
    let executable = compile_oracle(&working)?;
    let cases = draw_cases();
    let input: Vec<u8> = cases
        .iter()
        .flat_map(|(seed, subsequence, counter)| {
            seed.to_le_bytes()
                .into_iter()
                .chain(subsequence.to_le_bytes())
                .chain(counter.to_le_bytes())
        })
        .collect();
    let input_path = working.join("cases.bin");
    fs::write(&input_path, input)?;
    let oracle = Command::new(&executable)
        .arg(&input_path)
        .current_dir(&working)
        .output()?;
    fs::write(working.join("oracle.stderr"), &oracle.stderr)?;
    assert!(
        oracle.status.success(),
        "Oracle failed; retained {}",
        working.display()
    );
    assert_eq!(oracle.stdout.len(), cases.len() * 4);
    for ((seed, subsequence, counter), expected) in
        cases.into_iter().zip(oracle.stdout.as_chunks::<4>().0)
    {
        assert_eq!(
            super::bonsai_qt_philox_uniform(seed, subsequence, counter)
                .to_bits()
                .to_le_bytes(),
            *expected,
            "Philox bits differ for seed={seed}, subsequence={subsequence}, counter={counter}; retained {}",
            working.display()
        );
    }
    fs::remove_dir_all(&working)?;
    Ok(())
}
