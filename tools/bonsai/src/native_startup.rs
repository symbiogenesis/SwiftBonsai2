//! Native speech provisioning and compiler orchestration outside presentation.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Sibling startup modules share an explicit crate-local implementation API."
)]

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bonsai_artifacts::validate_external_path;
use serde_json::{Value, json};

const TARGET: &str = "aarch64-pc-windows-msvc";
static NEXT_NONCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn nonce() -> crate::Result<String> {
    Ok(format!(
        "{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos(),
        NEXT_NONCE.fetch_add(1, Ordering::Relaxed)
    ))
}

fn text<'a>(value: &'a Value, key: &str) -> crate::Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Missing native startup field: {key}"))
}

fn bytes(value: &Value) -> crate::Result<u64> {
    value["bytes"]
        .as_u64()
        .ok_or_else(|| "Missing pinned artifact length.".into())
}

fn external(path: &Path) -> crate::Result<()> {
    validate_external_path(path).map_err(|error| error.to_string())
}

fn require_host() -> crate::Result<()> {
    if !cfg!(all(windows, target_arch = "aarch64")) {
        return Err("Native speech builds require native ARM64 Windows.".into());
    }
    Ok(())
}

fn checked_process(
    executable: &Path,
    arguments: &[OsString],
    working: &Path,
    name: &str,
    timeout: u64,
) -> crate::Result<String> {
    let log = working.join(format!("{name}-{}.log", nonce()?));
    let result = crate::startup_process::run(
        executable.as_os_str(),
        arguments,
        working,
        &BTreeMap::new(),
        Duration::from_secs(timeout),
        &log,
    )?;
    if result.code != 0 {
        return Err(format!(
            "Native build preparation failed ({name}); see {}: {}",
            log.display(),
            result.stderr.trim()
        ));
    }
    Ok(result.stdout)
}

fn receive(artifact: &Value, destination: &Path, working: &Path) -> crate::Result<()> {
    crate::startup_download::receive(
        text(artifact, "url")?,
        destination,
        bytes(artifact)?,
        text(artifact, "sha256")?,
        working,
    )
}

fn install_compiler(data: &Path, working: &Path, pin: &Value) -> crate::Result<PathBuf> {
    let directory = data.join("tools").join(text(pin, "directory")?);
    let archive = directory.join(text(pin, "filename")?);
    let root = directory.join(text(pin, "root")?);
    external(&root)?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let _guard = crate::startup_cache::lock_file(&directory.join("compiler-provision.lock"))?;
    if !root.exists() {
        receive(pin, &archive, working)?;
        let tar = crate::startup_download::system_tool("tar.exe")?;
        let listing = checked_process(
            &tar,
            &["-tvf".into(), archive.as_os_str().to_owned()],
            working,
            "llvm-types",
            600,
        )?;
        if listing
            .lines()
            .filter(|line| !line.is_empty())
            .any(|line| !line.starts_with(['-', 'd']))
        {
            return Err("LLVM archive contains a link or special file.".into());
        }
        let names = checked_process(
            &tar,
            &["-tf".into(), archive.as_os_str().to_owned()],
            working,
            "llvm-paths",
            600,
        )?;
        crate::speech_build::run(json!({
            "operation": "validate-names", "archiveRoot": text(pin, "root")?,
            "names": names.lines().filter(|line| !line.is_empty()).collect::<Vec<_>>()
        }))?;
        let stage = directory.join(format!("extract-{}", nonce()?));
        external(&stage)?;
        fs::create_dir(&stage).map_err(|error| error.to_string())?;
        checked_process(
            &tar,
            &[
                "-xf".into(),
                archive.as_os_str().to_owned(),
                "-C".into(),
                stage.as_os_str().to_owned(),
            ],
            working,
            "llvm-extract",
            1_200,
        )?;
        let extracted = stage.join(text(pin, "root")?);
        external(&extracted)?;
        fs::rename(&extracted, &root).map_err(|error| {
            format!(
                "Cannot publish pinned LLVM; evidence remains at {}: {error}",
                stage.display()
            )
        })?;
    }
    let executables = pin["executables"]
        .as_object()
        .ok_or("Missing compiler executable pins.")?;
    for (name, identity) in executables {
        let path = root.join("bin").join(name);
        crate::startup_download::verify(&path, bytes(identity)?, text(identity, "sha256")?)?;
        crate::startup_cache::assert_arm64(&path)?;
    }
    Ok(root)
}

fn stage_sources(data: &Path, working: &Path, pin: &Value) -> crate::Result<PathBuf> {
    let cache = data.join("tools/native-speech");
    external(&cache)?;
    fs::create_dir_all(&cache).map_err(|error| error.to_string())?;
    let _guard = crate::startup_cache::lock_file(&cache.join("source-provision.lock"))?;
    let source = cache
        .join("sources")
        .join(text(&pin["sources"]["qwen"], "root")?);
    external(&source)?;
    let mut archives = BTreeMap::new();
    for key in ["qwen", "ggml"] {
        let artifact = &pin["sources"][key];
        let archive = cache.join("downloads").join(text(artifact, "filename")?);
        receive(artifact, &archive, working)?;
        archives.insert(key, archive);
    }
    crate::speech_build::run(json!({
        "operation": "stage-sources", "sourceDirectory": source,
        "qwenArchivePath": archives["qwen"], "ggmlArchivePath": archives["ggml"]
    }))?;
    Ok(source)
}

fn setup_environment(
    output: &str,
    compiler: &Path,
    cmake: &Path,
    ninja: &Path,
    source: &Path,
) -> crate::Result<BTreeMap<String, String>> {
    let mut environment = BTreeMap::new();
    for line in output.lines() {
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if let Some(canonical) = [
            "PATH",
            "INCLUDE",
            "LIB",
            "LIBPATH",
            "VSINSTALLDIR",
            "VCINSTALLDIR",
            "VCToolsInstallDir",
            "WindowsSdkDir",
            "WindowsSDKVersion",
            "UniversalCRTSdkDir",
            "UCRTVersion",
            "VSCMD_ARG_TGT_ARCH",
            "VSCMD_ARG_HOST_ARCH",
        ]
        .into_iter()
        .find(|allowed| allowed.eq_ignore_ascii_case(name))
        {
            environment.insert(canonical.to_owned(), value.to_owned());
        }
    }
    if environment
        .get("VSCMD_ARG_TGT_ARCH")
        .is_none_or(|value| value != "arm64")
        || environment.get("INCLUDE").is_none_or(String::is_empty)
        || environment.get("LIB").is_none_or(String::is_empty)
    {
        return Err("Visual Studio did not provide ARM64 compiler and SDK libraries.".into());
    }
    let bin = compiler.join("bin");
    let clang = bin.join("clang-cl.exe").to_string_lossy().into_owned();
    let inherited = environment
        .get("PATH")
        .ok_or("Visual Studio omitted PATH.")?;
    environment.insert(
        "PATH".into(),
        format!(
            "{};{};{};{inherited}",
            bin.display(),
            cmake.parent().ok_or("CMake has no parent.")?.display(),
            ninja.parent().ok_or("Ninja has no parent.")?.display()
        ),
    );
    for name in [
        "CC",
        "CXX",
        "CMAKE_C_COMPILER",
        "CMAKE_CXX_COMPILER",
        "BONSAI_TTS_COMPILER",
    ] {
        environment.insert(name.into(), clang.clone());
    }
    for name in ["CMAKE_C_COMPILER_TARGET", "CMAKE_CXX_COMPILER_TARGET"] {
        environment.insert(name.into(), TARGET.into());
    }
    for (name, value) in [
        ("CMAKE_GENERATOR", "Ninja"),
        ("CL", "/EHsc"),
        ("CARGO_BUILD_JOBS", "4"),
        ("CMAKE_BUILD_PARALLEL_LEVEL", "4"),
        ("BONSAI_CMAKE_GENERATOR", "Ninja Multi-Config"),
        (
            "TRANSCRIBE_CMAKE_ARGS",
            "-DGGML_NATIVE=OFF -DGGML_OPENMP=OFF -DTRANSCRIBE_USE_OPENMP=OFF -DTRANSCRIBE_USE_SYSTEM_BLAS=OFF -DCMAKE_C_COMPILER_TARGET=aarch64-pc-windows-msvc -DCMAKE_CXX_COMPILER_TARGET=aarch64-pc-windows-msvc",
        ),
    ] {
        environment.insert(name.into(), value.into());
    }
    environment.insert("CMAKE".into(), cmake.to_string_lossy().into_owned());
    environment.insert("BONSAI_CMAKE".into(), cmake.to_string_lossy().into_owned());
    environment.insert(
        "RC".into(),
        bin.join("llvm-rc.exe").to_string_lossy().into_owned(),
    );
    environment.insert(
        "BONSAI_TTS_SOURCE_DIR".into(),
        source.to_string_lossy().into_owned(),
    );
    Ok(environment)
}

/// Prepares verified compiler/source inputs and returns a child-only environment.
pub(crate) fn environment(data: &Path, working: &Path) -> crate::Result<Value> {
    crate::startup_cancel::checkpoint()?;
    require_host()?;
    external(data)?;
    external(working)?;
    fs::create_dir_all(working).map_err(|error| error.to_string())?;
    let pin = crate::speech_build::build_plan()?;
    let compiler = install_compiler(data, working, &pin["compiler"])?;
    let source = stage_sources(data, working, &pin)?;
    let program_files = PathBuf::from(
        std::env::var_os("ProgramFiles(x86)")
            .ok_or("Missing Windows x86 Program Files directory.")?,
    );
    external(&program_files)?;
    let vswhere = program_files.join("Microsoft Visual Studio/Installer/vswhere.exe");
    let visual_studio = PathBuf::from(
        checked_process(
            &vswhere,
            &[
                "-latest".into(),
                "-products".into(),
                "*".into(),
                "-property".into(),
                "installationPath".into(),
            ],
            working,
            "visual-studio-discovery",
            60,
        )?
        .trim(),
    );
    external(&visual_studio)?;
    let devcmd = visual_studio.join("Common7/Tools/VsDevCmd.bat");
    let cmake =
        visual_studio.join("Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe");
    let ninja = visual_studio.join("Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja/ninja.exe");
    for path in [&devcmd, &cmake, &ninja] {
        if !path.is_file() {
            return Err(format!(
                "Visual Studio ARM64 build prerequisite is missing: {}",
                path.display()
            ));
        }
    }
    let command = devcmd
        .to_str()
        .ok_or("Visual Studio path is not Unicode.")?;
    if command.contains(['"', '%', '\r', '\n']) {
        return Err("Unsupported Visual Studio installation path.".into());
    }
    let batch = working.join(format!("visual-studio-env-{}.cmd", nonce()?));
    external(&batch)?;
    fs::write(&batch, format!("@call \"{command}\" -no_logo -arch=arm64 -host_arch=arm64 >nul\r\n@if errorlevel 1 exit /b 1\r\n@set\r\n")).map_err(|error| error.to_string())?;
    let output = checked_process(
        &crate::startup_download::system_tool("cmd.exe")?,
        &["/d".into(), "/c".into(), batch.as_os_str().to_owned()],
        working,
        "visual-studio-environment",
        60,
    )?;
    let environment = setup_environment(&output, &compiler, &cmake, &ninja, &source)?;
    Ok(
        json!({ "Environment": environment, "SourceDirectory": source,
        "CompilerPath": compiler.join("bin/clang-cl.exe"), "CMakePath": cmake }),
    )
}

fn packages(model: &str) -> crate::Result<Vec<String>> {
    match model.to_ascii_lowercase().as_str() {
        "all" => Ok(vec!["bonsai-speech-asr".into(), "bonsai-speech-tts".into()]),
        "canary" => Ok(vec!["bonsai-speech-asr".into()]),
        "qwentts" => Ok(vec!["bonsai-speech-tts".into()]),
        _ => Err("Unsupported native speech build selection.".into()),
    }
}

/// Reuses checked native outputs before provisioning any compiler or sources.
pub(crate) fn build(
    repo: &Path,
    data: &Path,
    working: &Path,
    model: &str,
    force: bool,
) -> crate::Result<Value> {
    require_host()?;
    external(data)?;
    external(working)?;
    let channel = crate::source_build::channel(repo)?;
    let target_directory = data
        .join("build/native-speech")
        .join(crate::source_build::cache_key(repo, &channel));
    let identity = format!(
        "{}\n{}",
        crate::speech_build::build_plan()?,
        crate::startup_cache::digest_file(
            &std::env::current_exe().map_err(|error| error.to_string())?
        )?
    );
    let spec = crate::source_build::BuildSpec {
        repository_root: repo.to_path_buf(),
        working_directory: working.to_path_buf(),
        target_directory,
        packages: packages(model)?,
        channel,
        target: Some(TARGET.into()),
        identity,
        force,
    };
    let outputs = crate::source_build::cached_build(&spec, || {
        serde_json::from_value(environment(data, working)?["Environment"].clone())
            .map_err(|error| error.to_string())
    })?;
    Ok(
        json!({"AsrPath": outputs.get("bonsai-speech-asr"), "TtsPath": outputs.get("bonsai-speech-tts")}),
    )
}

/// Provisions selected speech models, deferring cached hashes only for launch use.
pub(crate) fn models(
    data: &Path,
    model: &str,
    quantization: &str,
    verify_only: bool,
    defer: bool,
    working: &Path,
) -> crate::Result<Value> {
    if verify_only && defer {
        return Err("VerifyOnly cannot be combined with DeferCachedVerification.".into());
    }
    external(data)?;
    external(working)?;
    let quantization = quantization.to_ascii_uppercase();
    let result = crate::speech::run(
        json!({"dataDirectory": data, "model": model, "ttsQuantization": quantization}),
    )?;
    provision_models(result, verify_only, defer, working)
}

fn provision_models(
    mut result: Value,
    verify_only: bool,
    defer: bool,
    working: &Path,
) -> crate::Result<Value> {
    for plan in result["Plans"]
        .as_array()
        .ok_or("Missing speech model plan.")?
    {
        let path = Path::new(text(plan, "Path")?);
        let bytes = plan["Bytes"]
            .as_u64()
            .ok_or("Missing speech model length.")?;
        if defer && path.exists() {
            crate::startup_download::verify_length(path, bytes)?;
        } else if verify_only {
            crate::startup_download::verify(path, bytes, text(plan, "Sha256")?)?;
        } else {
            crate::startup_download::receive(
                text(plan, "Uri")?,
                path,
                bytes,
                text(plan, "Sha256")?,
                working,
            )?;
        }
    }
    result
        .as_object_mut()
        .ok_or("Missing speech model result.")?
        .remove("Plans");
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_environment_preserves_system_settings_and_pinned_compilers() -> crate::Result<()> {
        let output = "Path=original\r\nInclude=include\r\nLIB=lib\r\nVSCMD_ARG_TGT_ARCH=arm64\r\nSystemRoot=unsafe override\r\nAPPDATA=unsafe override\r\n";
        let result = setup_environment(
            output,
            Path::new("compiler"),
            Path::new("cmake/bin/cmake.exe"),
            Path::new("ninja/ninja.exe"),
            Path::new("source"),
        )?;
        assert!(result["PATH"].ends_with(";original"));
        assert_eq!(result["CC"], result["BONSAI_TTS_COMPILER"]);
        assert_eq!(result["CMAKE_C_COMPILER_TARGET"], TARGET);
        assert_eq!(result["BONSAI_CMAKE_GENERATOR"], "Ninja Multi-Config");
        assert!(!result.contains_key("SystemRoot"));
        assert!(!result.contains_key("APPDATA"));
        assert!(
            setup_environment(
                "VSCMD_ARG_TGT_ARCH=x64",
                Path::new("compiler"),
                Path::new("cmake"),
                Path::new("ninja"),
                Path::new("source")
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn native_selection_and_model_modes_reject_unsupported_requests() {
        assert_eq!(packages("All").map(|value| value.len()), Ok(2));
        assert_eq!(packages("Canary").map(|value| value.len()), Ok(1));
        assert!(packages("other").is_err());
        assert!(
            models(
                Path::new("relative"),
                "All",
                "Q8_0",
                true,
                true,
                Path::new("relative")
            )
            .is_err()
        );
    }

    #[test]
    fn ordinary_model_setup_defers_only_cached_hashes() -> crate::Result<()> {
        use sha2::{Digest as _, Sha256};
        let working = std::env::temp_dir().join(format!("Bonsai-ModelStartupTest-{}", nonce()?));
        external(&working)?;
        fs::create_dir(&working).map_err(|error| error.to_string())?;
        let path = working.join("model.gguf");
        fs::write(&path, b"wrong").map_err(|error| error.to_string())?;
        let plan = json!({"CanaryModelPath": path, "Plans": [{"Path": path, "Bytes": 5,
            "Sha256": crate::startup_cache::hex(&Sha256::digest(b"right")), "Uri": "https://example.invalid/model"}]});
        assert!(
            provision_models(plan.clone(), false, true, &working)?
                .get("Plans")
                .is_none()
        );
        assert!(provision_models(plan.clone(), true, false, &working).is_err());
        assert!(provision_models(plan.clone(), false, false, &working).is_err());
        fs::write(&path, b"shorter").map_err(|error| error.to_string())?;
        assert!(provision_models(plan, false, true, &working).is_err());
        Ok(())
    }
}
