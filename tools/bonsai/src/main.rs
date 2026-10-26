//! JSON command entry point for packaged Bonsai maintenance tools.
#![forbid(unsafe_code)]

use std::fs::OpenOptions;
use std::io::{Read as _, Write as _};
use std::path::Path;
use std::process::ExitCode;

const MAX_REQUEST_BYTES: u64 = 16 * 1024 * 1024;
// Legacy in-memory scoring permits 24 responses of 1 Mi UTF-16 units each.
// JSON escaping can use six bytes per unit, plus the bounded review records.
const MAX_QUALIFICATION_REQUEST_BYTES: u64 = 160 * 1024 * 1024;
const COMMANDS: &[&str] = &[
    "credentials",
    "memory",
    "cpu-check-plan",
    "server-policy",
    "runtime-stage",
    "runtime-verify",
    "completion",
    "qualification",
    "speech-models",
    "inference-lock",
    "speech-build",
];

fn read_request(path: &Path, maximum: u64) -> bonsai_tools::Result<serde_json::Value> {
    if !path.is_absolute() {
        return Err("Request path must be expanded and absolute.".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > maximum {
        return Err(format!("Command request exceeds {maximum} bytes."));
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).map_err(|error| error.to_string())? > maximum {
        return Err(format!("Command request exceeds {maximum} bytes."));
    }
    let request: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if !request.is_object() {
        return Err("Command request must be a JSON object.".into());
    }
    Ok(request)
}

fn execute() -> bonsai_tools::Result<serde_json::Value> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() == 1 && arguments[0] == "--version" {
        return Ok(serde_json::json!({
            "Name": "bonsai-tools", "Version": env!("CARGO_PKG_VERSION"),
            "ProtocolVersion": 5, "Commands": COMMANDS
        }));
    }
    if arguments.len() != 3 || arguments[1] != "--request" {
        return Err(format!(
            "Usage: bonsai-tools <{}> --request ABSOLUTE_JSON_PATH; or --version",
            COMMANDS.join("|")
        ));
    }
    let maximum = if arguments[0] == "qualification" {
        MAX_QUALIFICATION_REQUEST_BYTES
    } else {
        MAX_REQUEST_BYTES
    };
    let request = read_request(Path::new(&arguments[2]), maximum)?;
    match arguments[0].to_str() {
        Some("credentials") => bonsai_tools::credentials::run(request),
        Some("completion") => bonsai_tools::completion::run(request),
        Some("cpu-check-plan") => bonsai_tools::completion::cpu_check_plan(request),
        Some("server-policy") => bonsai_tools::server::run(request),
        Some("runtime-stage") => bonsai_tools::install::run(request),
        Some("runtime-verify") => bonsai_tools::runtime::run(request),
        Some("memory") if request.as_object().is_some_and(serde_json::Map::is_empty) => {
            bonsai_tools::completion::available_memory()
        }
        Some("qualification") => bonsai_tools::qualification::run(request),
        Some("speech-models") => bonsai_tools::speech::run(request),
        Some("inference-lock") => bonsai_tools::inference_lock::run(request),
        Some("speech-build") => bonsai_tools::speech_build::run(request),
        _ => Err("Unknown maintenance command.".into()),
    }
}

fn main() -> ExitCode {
    match execute() {
        Ok(value) => {
            let mut output = std::io::stdout().lock();
            if serde_json::to_writer(&mut output, &value).is_err()
                || output.write_all(b"\n").is_err()
            {
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
