//! Command entry point for workspace impact selection and aggregate acceptance.
#![forbid(unsafe_code)]

use std::io::Write as _;
use std::process::ExitCode;

fn main() -> ExitCode {
    match bonsai_ci::command::run(&std::env::args_os().skip(1).collect::<Vec<_>>()) {
        Ok(output) => {
            if writeln!(std::io::stdout().lock(), "{output}").is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(std::io::stderr().lock(), "{error}");
            ExitCode::FAILURE
        }
    }
}
