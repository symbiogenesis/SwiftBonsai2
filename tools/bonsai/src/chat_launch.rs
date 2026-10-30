//! Native chat startup policy and orchestration behind the thin PowerShell entry point.

mod instance;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bonsai_artifacts::validate_external_path;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "The JSON boundary preserves the existing independent PowerShell switches."
)]
struct Request {
    data_directory: PathBuf,
    repository_root: PathBuf,
    #[serde(default)]
    working_directory: Option<PathBuf>,
    #[serde(default)]
    chat_path: Option<PathBuf>,
    #[serde(default)]
    broker_path: Option<PathBuf>,
    #[serde(default)]
    asr_path: Option<PathBuf>,
    #[serde(default)]
    tts_path: Option<PathBuf>,
    #[serde(default = "default_quantization")]
    tts_quantization: String,
    #[serde(default)]
    tts_threads: u16,
    #[serde(default)]
    no_abliterate_lora: bool,
    #[serde(default)]
    use_repacked_ptq1: bool,
    #[serde(default)]
    native_engine: bool,
    #[serde(default = "default_native_row_workers")]
    native_row_workers: u8,
    #[serde(default = "default_frontend_port")]
    frontend_port: u16,
    #[serde(default = "default_backend_port")]
    backend_port: u16,
    #[serde(default)]
    canary_key_file: Option<PathBuf>,
    #[serde(default = "default_canary_port")]
    canary_port: u16,
    #[serde(default)]
    tts_key_file: Option<PathBuf>,
    #[serde(default = "default_tts_port")]
    tts_port: u16,
    #[serde(default)]
    no_tts: bool,
    #[serde(default)]
    no_speech_models: bool,
    #[serde(default)]
    speech_models_only: bool,
    #[serde(default)]
    force_build: bool,
}

fn default_quantization() -> String {
    "Q8_0".into()
}

const fn default_native_row_workers() -> u8 {
    1
}

const fn default_frontend_port() -> u16 {
    5173
}

const fn default_backend_port() -> u16 {
    8081
}

const fn default_canary_port() -> u16 {
    8766
}

const fn default_tts_port() -> u16 {
    8767
}

struct Plan {
    request: Request,
    own_canary: bool,
    own_tts: bool,
    model_selection: Option<&'static str>,
    native_selection: Option<&'static str>,
    app_packages: Vec<&'static str>,
}

fn empty_path(path: &mut Option<PathBuf>) {
    if path
        .as_ref()
        .is_some_and(|value| value.as_os_str().is_empty())
    {
        *path = None;
    }
}

fn external(path: &Path) -> crate::Result<()> {
    validate_external_path(path).map_err(|error| error.to_string())
}

fn executable(path: &Path, name: &str) -> crate::Result<()> {
    external(path)?;
    if !path.is_file()
        || !path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return Err(format!(
            "{name} must name an existing executable outside Git."
        ));
    }
    Ok(())
}

fn repository(path: &Path) -> crate::Result<()> {
    if !path.is_absolute()
        || path.to_str().is_none_or(|text| text.contains(['%', '\0']))
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err("RepositoryRoot must be an expanded absolute path.".into());
    }
    Ok(())
}

const fn selection(canary: bool, tts: bool) -> Option<&'static str> {
    match (canary, tts) {
        (true, true) => Some("All"),
        (true, false) => Some("Canary"),
        (false, true) => Some("QwenTts"),
        (false, false) => None,
    }
}

fn validate_paths(request: &Request) -> crate::Result<()> {
    repository(&request.repository_root)?;
    external(&request.data_directory)?;
    if !request.data_directory.is_dir() && !request.speech_models_only {
        if request.native_engine {
            return Err("Provision Swift first with scripts/Get-Model.ps1 -LockPath config/swift-assessment.lock.json and scripts/Get-Inference.ps1 using the same DataDirectory. See docs/development-guide.md#native-chat-app.".into());
        }
        return Err("Provision Swift first with scripts/Get-Model.ps1 -LockPath config/swift-assessment.lock.json and scripts/Get-Engine.ps1 using the same lock and DataDirectory. See docs/development-guide.md#native-chat-app.".into());
    }
    if let Some(working) = &request.working_directory {
        external(working)?;
    }
    for (name, path) in [
        ("ChatPath", &request.chat_path),
        ("BrokerPath", &request.broker_path),
        ("AsrPath", &request.asr_path),
        ("TtsPath", &request.tts_path),
    ] {
        if let Some(path) = path {
            executable(path, name)?;
        }
    }
    for (name, key) in [
        ("CanaryKeyFile", &request.canary_key_file),
        ("TtsKeyFile", &request.tts_key_file),
    ] {
        if let Some(path) = key {
            external(path)?;
            if !path.is_file() {
                return Err(format!("{name} must name an existing external key file."));
            }
        }
    }
    Ok(())
}

fn prepare(mut request: Request, native_host: bool) -> crate::Result<Plan> {
    if !matches!(request.native_row_workers, 1 | 4)
        || (request.native_row_workers == 4 && !request.native_engine)
    {
        return Err("NativeRowWorkers must be 1 or 4; four requires NativeEngine.".into());
    }
    if !request.speech_models_only && !native_host {
        return Err("Run chat in native ARM64 PowerShell on Windows ARM64.".into());
    }
    for path in [
        &mut request.chat_path,
        &mut request.broker_path,
        &mut request.asr_path,
        &mut request.tts_path,
        &mut request.canary_key_file,
        &mut request.tts_key_file,
    ] {
        empty_path(path);
    }
    request.tts_quantization = match request.tts_quantization.to_ascii_uppercase().as_str() {
        "Q8_0" => "Q8_0".into(),
        "Q4_K_M" => "Q4_K_M".into(),
        _ => return Err("TtsQuantization must be Q8_0 or Q4_K_M.".into()),
    };
    if request.tts_threads > 256 {
        return Err("TtsThreads must be between zero and 256.".into());
    }
    if [
        request.frontend_port,
        request.backend_port,
        request.canary_port,
        request.tts_port,
    ]
    .iter()
    .any(|port| *port < 1024)
    {
        return Err("Chat and speech ports must be between 1024 and 65535.".into());
    }
    if request.frontend_port == request.backend_port {
        return Err("FrontendPort and BackendPort must differ.".into());
    }
    if request.no_speech_models && request.speech_models_only {
        return Err("SpeechModelsOnly cannot be combined with NoSpeechModels.".into());
    }
    if request.no_tts && request.tts_key_file.is_some() {
        return Err("NoTts cannot be combined with TtsKeyFile.".into());
    }
    validate_paths(&request)?;
    let own_canary = request.canary_key_file.is_none() && !request.no_speech_models;
    let own_tts = request.tts_key_file.is_none() && !request.no_tts && !request.no_speech_models;
    if !own_tts && (request.tts_quantization != "Q8_0" || request.tts_threads != 0) {
        return Err("TtsQuantization and TtsThreads require automatically owned TTS.".into());
    }
    let canary = own_canary || request.canary_key_file.is_some();
    let tts = own_tts || request.tts_key_file.is_some();
    let chat_ports = [request.frontend_port, request.backend_port];
    if (canary && chat_ports.contains(&request.canary_port))
        || (tts && chat_ports.contains(&request.tts_port))
        || (canary && tts && request.canary_port == request.tts_port)
    {
        return Err("Every configured speech service needs a distinct local port.".into());
    }
    if (request.asr_path.is_some() && !own_canary) || (request.tts_path.is_some() && !own_tts) {
        return Err(
            "A prebuilt speech executable requires automatic setup for that service.".into(),
        );
    }
    let model_selection = if request.speech_models_only {
        Some("All")
    } else {
        selection(own_canary, own_tts)
    };
    let native_selection = selection(
        own_canary && request.asr_path.is_none(),
        own_tts && request.tts_path.is_none(),
    );
    let mut app_packages = Vec::with_capacity(2);
    if request.chat_path.is_none() {
        app_packages.push("bonsai-chat");
    }
    if request.broker_path.is_none() {
        app_packages.push("bonsai-broker");
    }
    Ok(Plan {
        request,
        own_canary,
        own_tts,
        model_selection,
        native_selection,
        app_packages,
    })
}

trait Operations {
    fn models(&mut self, request: &Request, model: &str, working: &Path) -> crate::Result<Value>;
    fn apps(
        &mut self,
        request: &Request,
        packages: &[&str],
        working: &Path,
    ) -> crate::Result<Value>;
    fn native(&mut self, request: &Request, model: &str, working: &Path) -> crate::Result<Value>;
    fn keys(&mut self, parent: &Path) -> crate::Result<PathBuf>;
    fn launch(
        &mut self,
        executable: &Path,
        arguments: &[OsString],
        working: &Path,
    ) -> crate::Result<u32>;
}

struct Native;

fn field<'a>(value: &'a Value, key: &str) -> crate::Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Startup operation omitted {key}."))
}

fn path_field(value: &Value, key: &str) -> crate::Result<PathBuf> {
    field(value, key).map(PathBuf::from)
}

impl Operations for Native {
    fn models(&mut self, request: &Request, model: &str, working: &Path) -> crate::Result<Value> {
        crate::native_startup::models(
            &request.data_directory,
            model,
            &request.tts_quantization,
            false,
            !request.speech_models_only,
            working,
        )
    }

    fn apps(
        &mut self,
        request: &Request,
        packages: &[&str],
        working: &Path,
    ) -> crate::Result<Value> {
        crate::source_build::run(json!({
            "operation": "chat", "repositoryRoot": request.repository_root,
            "dataDirectory": request.data_directory, "workingDirectory": working,
            "packages": packages, "forceBuild": request.force_build,
        }))
    }

    fn native(&mut self, request: &Request, model: &str, working: &Path) -> crate::Result<Value> {
        crate::native_startup::build(
            &request.repository_root,
            &request.data_directory,
            working,
            model,
            request.force_build,
        )
    }

    fn keys(&mut self, parent: &Path) -> crate::Result<PathBuf> {
        path_field(
            &crate::credentials::run(json!({"parentDirectory": parent}))?,
            "FrontendKeyFile",
        )
    }

    fn launch(
        &mut self,
        executable: &Path,
        arguments: &[OsString],
        working: &Path,
    ) -> crate::Result<u32> {
        let result = crate::startup_process::run(
            executable.as_os_str(),
            arguments,
            working,
            &BTreeMap::new(),
            Duration::ZERO,
            &working.join("chat.log"),
        )?;
        Ok(result.code)
    }
}

fn argument(arguments: &mut Vec<OsString>, name: &str, value: impl Into<OsString>) {
    arguments.push(name.into());
    arguments.push(value.into());
}

fn launch_arguments(
    request: &Request,
    models: Option<&Value>,
    own_canary: bool,
    own_tts: bool,
    working: &Path,
) -> crate::Result<Vec<OsString>> {
    let broker = request
        .broker_path
        .as_ref()
        .ok_or("Missing chat broker executable.")?;
    let mut arguments = Vec::with_capacity(36);
    argument(&mut arguments, "--broker-path", broker);
    argument(&mut arguments, "--data-directory", &request.data_directory);
    argument(&mut arguments, "--evidence-directory", working);
    argument(
        &mut arguments,
        "--frontend-port",
        request.frontend_port.to_string(),
    );
    argument(
        &mut arguments,
        "--backend-port",
        request.backend_port.to_string(),
    );
    if request.no_abliterate_lora {
        arguments.push("--no-abliterate-lora".into());
    }
    if request.use_repacked_ptq1 {
        arguments.push("--use-repacked-ptq1".into());
    }
    if request.native_engine {
        arguments.push("--native-engine".into());
    }
    if request.native_row_workers == 4 {
        argument(&mut arguments, "--native-row-workers", "4");
    }
    if let Some(key) = &request.canary_key_file {
        argument(&mut arguments, "--canary-key-file", key);
        argument(
            &mut arguments,
            "--canary-port",
            request.canary_port.to_string(),
        );
    }
    if let Some(key) = &request.tts_key_file {
        argument(&mut arguments, "--tts-key-file", key);
        argument(&mut arguments, "--tts-port", request.tts_port.to_string());
    }
    if own_canary || own_tts {
        let models = models.ok_or("Missing owned speech model paths.")?;
        if own_canary {
            argument(
                &mut arguments,
                "--canary-executable",
                request
                    .asr_path
                    .as_ref()
                    .ok_or("Missing Canary executable.")?,
            );
            argument(
                &mut arguments,
                "--canary-model",
                field(models, "CanaryModelPath")?,
            );
        }
        if own_tts {
            argument(
                &mut arguments,
                "--tts-executable",
                request.tts_path.as_ref().ok_or("Missing TTS executable.")?,
            );
            argument(
                &mut arguments,
                "--tts-model",
                field(models, "TtsModelPath")?,
            );
            argument(
                &mut arguments,
                "--tts-codec",
                field(models, "TtsCodecPath")?,
            );
            argument(
                &mut arguments,
                "--tts-voice-model",
                field(models, "TtsVoiceModelPath")?,
            );
            argument(
                &mut arguments,
                "--tts-quantization",
                &request.tts_quantization,
            );
            if request.tts_threads != 0 {
                argument(
                    &mut arguments,
                    "--tts-threads",
                    request.tts_threads.to_string(),
                );
            }
        }
    }
    Ok(arguments)
}

fn prepare_backend(
    plan: Plan,
    operations: &mut impl Operations,
    working: &Path,
) -> crate::Result<Value> {
    let Plan {
        mut request,
        own_canary,
        own_tts,
        model_selection,
        native_selection,
        app_packages,
    } = plan;
    if request.speech_models_only {
        fs::create_dir_all(&request.data_directory).map_err(|error| error.to_string())?;
    }
    let models = if let Some(model) = model_selection {
        Some(operations.models(&request, model, working)?)
    } else {
        None
    };
    if request.speech_models_only {
        return models.ok_or_else(|| "Missing explicit speech provisioning result.".into());
    }
    if !app_packages.is_empty() {
        let built = operations.apps(&request, &app_packages, working)?;
        if request.chat_path.is_none() {
            request.chat_path = Some(path_field(&built, "ChatPath")?);
        }
        if request.broker_path.is_none() {
            request.broker_path = Some(path_field(&built, "BrokerPath")?);
        }
    }
    if let Some(model) = native_selection {
        let built = operations.native(&request, model, working)?;
        if own_canary && request.asr_path.is_none() {
            request.asr_path = Some(path_field(&built, "AsrPath")?);
        }
        if own_tts && request.tts_path.is_none() {
            request.tts_path = Some(path_field(&built, "TtsPath")?);
        }
    }
    for (owned, name, key) in [
        (own_canary, "canary", &mut request.canary_key_file),
        (own_tts, "tts", &mut request.tts_key_file),
    ] {
        if owned {
            let parent = working.join(format!("{name}-credentials"));
            fs::create_dir_all(&parent).map_err(|error| error.to_string())?;
            *key = Some(operations.keys(&parent)?);
        }
    }
    let arguments = launch_arguments(&request, models.as_ref(), own_canary, own_tts, working)?;
    let arguments = arguments
        .into_iter()
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| "Chat arguments must be Unicode.".to_owned())
        })
        .collect::<crate::Result<Vec<_>>>()?;
    Ok(json!({"Arguments": arguments}))
}

fn launch_pending(
    mut request: Request,
    operations: &mut impl Operations,
    working: &Path,
    tool: &Path,
) -> crate::Result<Value> {
    // Only the presentation executable is needed before the native window can
    // open. Backend builds, speech downloads and credentials run in the UI's
    // owned preparation worker after its first frame.
    if request.chat_path.is_none() {
        let built = operations.apps(&request, &["bonsai-chat"], working)?;
        request.chat_path = Some(path_field(&built, "ChatPath")?);
    }
    request.working_directory = Some(working.into());
    let path = working.join(format!(
        "chat-startup-{}.json",
        crate::startup_process::unique()
    ));
    external(&path)?;
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| error.to_string())?;
    serde_json::to_writer(file, &request).map_err(|error| error.to_string())?;
    let chat = request
        .chat_path
        .as_ref()
        .ok_or("Missing chat executable.")?;
    let mut arguments = Vec::with_capacity(6);
    argument(&mut arguments, "--startup-tool", tool);
    argument(&mut arguments, "--startup-request", &path);
    argument(
        &mut arguments,
        "--startup-data-directory",
        &request.data_directory,
    );
    let code = operations.launch(chat, &arguments, working)?;
    if code != 0 {
        return Err(format!(
            "Chat stopped with exit code {code}. Evidence is retained under {}.",
            working.display()
        ));
    }
    Ok(json!({"Status": "chat-stopped", "ExitCode": code, "EvidenceDirectory": working}))
}

fn working_directory(requested: Option<&Path>) -> crate::Result<PathBuf> {
    if let Some(path) = requested {
        external(path)?;
        fs::create_dir_all(path).map_err(|error| error.to_string())?;
        return Ok(path.into());
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    for attempt in 0..100 {
        let path = std::env::temp_dir().join(format!(
            "SwiftBonsai-Chat-{}-{nonce}-{attempt}",
            std::process::id()
        ));
        external(&path)?;
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("Cannot create a fresh external chat session directory.".into())
}

/// Opens native chat before preparing its backend, credentials and speech files.
///
/// Speech-only provisioning returns model paths without compiling or starting an
/// application. Ordinary startup checks cached speech lengths; native services
/// retain full compiled SHA-256 verification before their first model use.
///
/// # Errors
/// Rejects unsupported hosts, malformed or conflicting options, unsafe paths,
/// failed provisioning/builds, and a nonzero chat exit. Evidence remains external.
pub fn run(value: Value) -> crate::Result<Value> {
    crate::startup_cancel::checkpoint()?;
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    if !request.speech_models_only && !cfg!(all(windows, target_arch = "aarch64")) {
        return Err("Run chat in native ARM64 PowerShell on Windows ARM64.".into());
    }
    let _instance = if request.speech_models_only {
        None
    } else {
        let directory = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or("LOCALAPPDATA is required for native chat admission.")?
            .join("SwiftBonsai2");
        let Some(guard) = instance::acquire(&directory)? else {
            return Ok(json!({
                "Status": "already-running",
                "Message": "Swift Bonsai 2 is already starting or running. No second instance was opened."
            }));
        };
        Some(guard)
    };
    let plan = prepare(request, cfg!(all(windows, target_arch = "aarch64")))?;
    let working = working_directory(plan.request.working_directory.as_deref())?;
    if plan.request.speech_models_only {
        return prepare_backend(plan, &mut Native, &working);
    }
    let tool = std::env::current_exe().map_err(|error| error.to_string())?;
    launch_pending(plan.request, &mut Native, &working, &tool)
}

/// Prepares the backend off the native UI thread and returns its validated arguments.
///
/// # Errors
/// Retains the launcher's option, artifact, build, path and credential checks.
/// The caller must own this process and its descendants until preparation stops.
pub fn prepare_run(value: Value) -> crate::Result<Value> {
    crate::startup_cancel::checkpoint()?;
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    let plan = prepare(request, cfg!(all(windows, target_arch = "aarch64")))?;
    if plan.request.speech_models_only {
        return Err("Use chat-launch for SpeechModelsOnly provisioning.".into());
    }
    if plan.request.chat_path.is_none() {
        return Err("Chat preparation requires the already launched chat executable.".into());
    }
    let working = working_directory(plan.request.working_directory.as_deref())?;
    prepare_backend(plan, &mut Native, &working)
}

#[cfg(test)]
mod tests;
