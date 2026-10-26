//! Synthetic headless completion fixture, built only with `test-probe`.
#![forbid(unsafe_code)]

use std::fs;
use std::io::{self, Write as _};
use std::process::ExitCode;
use std::time::Duration;

const TIMING: &[u8] = b"main: number of tokens in prompt = 15\ncommon_perf_print: prompt eval time = 100.5 ms / 15 tokens ( 6.7 ms per token, 149.25 tokens per second)\ncommon_perf_print: eval time = 200.0 ms / 3 runs ( 66.7 ms per token, 15 tokens per second)\ncommon_perf_print: samplers time = 0.50 ms / 19 tokens\n";

fn main() -> io::Result<ExitCode> {
    let mut arguments = std::env::args().skip(1);
    let mode = arguments
        .next()
        .ok_or_else(|| io::Error::other("missing completion probe mode"))?;
    match mode.as_str() {
        "stdout" | "capture" => {
            let text = arguments
                .next()
                .ok_or_else(|| io::Error::other("missing completion probe text"))?;
            emit(
                text.as_bytes(),
                if mode == "capture" { TIMING } else { b"" },
            )
        }
        "retained-input" => {
            if fs::write("pinned.txt", b"changed").is_ok() {
                return Ok(ExitCode::from(9));
            }
            emit(b"retained-input", b"")
        }
        "invalid-utf8" => emit(&[255], b""),
        "streams" | "--version" | "--help" => emit(b"output", b"diagnostic"),
        "partial-failure" => {
            let _ = emit(b"partial-response", b"")?;
            Ok(ExitCode::from(7))
        }
        "touch" => {
            fs::write("next-started.txt", b"unexpected")?;
            Ok(ExitCode::SUCCESS)
        }
        "supervision" => {
            let marker = arguments
                .next()
                .ok_or_else(|| io::Error::other("missing supervision marker path"))?;
            let mode = arguments
                .next()
                .ok_or_else(|| io::Error::other("missing supervision mode"))?;
            if !matches!(mode.as_str(), "ExitCode" | "ParentDeath" | "Timeout") {
                return Err(io::Error::other("unknown supervision mode"));
            }
            fs::write(marker, format!("{{\"ProcessId\":{}}}", std::process::id()))?;
            if mode == "ExitCode" {
                let _ = emit(b"retained-failure-output", b"")?;
                return Ok(ExitCode::from(7));
            }
            std::thread::sleep(Duration::from_secs(120));
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(io::Error::other("unknown completion probe mode")),
    }
}

fn emit(stdout: &[u8], stderr: &[u8]) -> io::Result<ExitCode> {
    let mut output = io::stdout().lock();
    output.write_all(stdout)?;
    output.flush()?;
    let mut error = io::stderr().lock();
    error.write_all(stderr)?;
    error.flush()?;
    Ok(ExitCode::SUCCESS)
}
