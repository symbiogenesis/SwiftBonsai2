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

fn initialize_startup(request: &mut serde_json::Value) -> bonsai_tools::Result<()> {
    let object = request.as_object_mut().ok_or("Invalid startup request.")?;
    let parent = match (
        object.remove("parentProcessId"),
        object.remove("parentCreationTimeFileTime"),
    ) {
        (None, None) => None,
        (Some(id), Some(creation)) => {
            let id = id
                .as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .filter(|id| *id != 0)
                .ok_or("Invalid startup caller identity.")?;
            let creation = creation
                .as_u64()
                .filter(|creation| *creation != 0)
                .ok_or("Invalid startup caller identity.")?;
            Some((id, creation))
        }
        _ => return Err("Incomplete startup caller identity.".into()),
    };
    bonsai_tools::startup_cancel::initialize(parent)
}

fn execute() -> bonsai_tools::Result<serde_json::Value> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() == 1 && arguments[0] == "--version" {
        return Ok(bonsai_tools::version());
    }
    if arguments.len() != 3 || arguments[1] != "--request" {
        return Err(format!(
            "Usage: bonsai-tools <{}> --request ABSOLUTE_JSON_PATH; or --version",
            bonsai_tools::COMMANDS.join("|")
        ));
    }
    let maximum = if arguments[0] == "qualification" {
        MAX_QUALIFICATION_REQUEST_BYTES
    } else {
        MAX_REQUEST_BYTES
    };
    let mut request = read_request(Path::new(&arguments[2]), maximum)?;
    let startup = matches!(
        arguments[0].to_str(),
        Some("source-build" | "chat-launch" | "chat-prepare")
    ) || (arguments[0] == "speech-build"
        && matches!(
            request["operation"].as_str(),
            Some("models" | "environment" | "build")
        ))
        || (arguments[0] == "chat-package" && request["operation"] == "launch");
    if startup {
        initialize_startup(&mut request)?;
    }
    match arguments[0].to_str() {
        Some("credentials") => bonsai_tools::credentials::run(request),
        Some("completion") => bonsai_tools::completion::run(request),
        Some("host-cpu") => bonsai_tools::host_cpu::run(&request),
        Some("cpu-evaluation") => bonsai_tools::cpu_evaluation::run(request),
        Some("cpu-check-plan") => bonsai_tools::completion::cpu_check_plan(request),
        Some("cpu-check-report") => bonsai_tools::completion::cpu_check_report(request),
        Some("server-policy") => bonsai_tools::server::run(request),
        Some("runtime-stage") => bonsai_tools::install::run(request),
        Some("runtime-verify") => bonsai_tools::runtime::run(request),
        Some("memory") if request.as_object().is_some_and(serde_json::Map::is_empty) => {
            bonsai_tools::completion::available_memory()
        }
        Some("qualification") => bonsai_tools::qualification::run(request),
        Some("runtime-promotion") => bonsai_tools::runtime_promotion::run(request),
        Some("speech-models") => bonsai_tools::speech::run(request),
        Some("inference-lock") => bonsai_tools::inference_lock::run(request),
        Some("inference-capabilities") => bonsai_tools::inference_capabilities::run(request),
        Some("speech-build") => bonsai_tools::speech_build::run(request),
        Some("speech-evaluation") => bonsai_tools::speech_evaluation::run(request),
        Some("source-build") => bonsai_tools::source_build::run(request),
        Some("chat-launch") => bonsai_tools::chat_launch::run(request),
        Some("chat-prepare") => bonsai_tools::chat_launch::prepare_run(request),
        Some("chat-package") => bonsai_tools::chat_package::run(request),
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
