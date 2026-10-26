//! Fixed native companion CLI and retained compiled-artifact verification.

use crate::{Kind, credentials::Credential};
use bonsai_artifacts::VerifiedFile;
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

/// A supported companion command, with no arbitrary runtime or model overrides.
pub enum Command {
    /// Report binary and protocol identity without touching model files.
    Version,
    /// Verify the compiled model pins and serve this explicit loopback endpoint.
    Serve(ServiceArgs),
}

/// Existing local files and the fixed companion port; credential values are private.
pub struct ServiceArgs {
    /// Absolute external path of the pinned ASR or talker GGUF.
    pub model: PathBuf,
    /// Absolute external codec GGUF; required for TTS and forbidden for ASR.
    pub codec: Option<PathBuf>,
    /// Absolute pinned Base talker GGUF used to preserve a designed speaker.
    pub voice_model: Option<PathBuf>,
    /// Absolute external protected 64-byte lowercase hexadecimal credential.
    pub token_file: PathBuf,
    /// Unprivileged numeric-loopback port, defaulting to 8766 or 8767.
    pub port: u16,
    /// Optional TTS compute team size, bounded to 1 through 256.
    pub threads: Option<usize>,
    /// Explicit pinned talker quantization; ASR always uses the default value.
    pub quantization: TtsQuantization,
}

/// Reviewed TTS talker artifact selection, independent of checkpoint identity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TtsQuantization {
    /// Default eight-bit talker weights.
    #[default]
    Q8,
    /// Opt-in four-bit talker evaluation weights.
    Q4KM,
}

/// Read guards for all verified model files and the startup credential.
///
/// Keep this owner in the engine adapter until serving has stopped. Native model
/// loaders may reopen or map paths; these guards deny writes, renames and mutable
/// ancestor replacement for their complete lifetime on Windows.
pub struct ModelGuards {
    _models: Vec<VerifiedFile>,
    _credential: Credential,
}

/// Require a native ARM64 process on Windows before starting model work.
///
/// # Errors
/// Rejects every build target other than native Windows ARM64.
pub fn require_native_host() -> io::Result<()> {
    if !cfg!(windows) || std::env::consts::ARCH != "aarch64" {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Native speech requires Windows ARM64.",
        ));
    }
    Ok(())
}

/// Parse the exact fixed companion command line without opening model contents.
///
/// `--version` must appear alone. ASR accepts `--model`, `--token-file` and an
/// optional `--port`; TTS additionally requires `--codec` and `--voice-model`,
/// and accepts bounded `--threads` and pinned `--quantization Q8_0|Q4_K_M`.
/// Every supplied path must already name an external regular non-reparse file.
///
/// # Errors
/// Rejects missing, duplicate or unknown options; unsafe paths; unsupported codec
/// combinations; and ports outside 1024 through 65535. Errors contain no values.
pub fn parse(kind: Kind, arguments: impl IntoIterator<Item = OsString>) -> io::Result<Command> {
    let values: Vec<_> = arguments.into_iter().collect();
    if values.len() == 1 && values[0] == "--version" {
        return Ok(Command::Version);
    }
    let mut arguments = values.into_iter();
    let (mut model, mut codec, mut token_file, mut port) = (None, None, None, None);
    let mut voice_model = None;
    let (mut threads, mut quantization) = (None, None);
    while let Some(option) = arguments.next() {
        match option.to_str() {
            Some("--model") if model.is_none() => {
                model = Some(PathBuf::from(
                    arguments.next().ok_or_else(invalid_arguments)?,
                ));
            }
            Some("--codec") if kind == Kind::QwenTts && codec.is_none() => {
                codec = Some(PathBuf::from(
                    arguments.next().ok_or_else(invalid_arguments)?,
                ));
            }
            Some("--voice-model") if kind == Kind::QwenTts && voice_model.is_none() => {
                voice_model = Some(PathBuf::from(
                    arguments.next().ok_or_else(invalid_arguments)?,
                ));
            }
            Some("--token-file") if token_file.is_none() => {
                token_file = Some(PathBuf::from(
                    arguments.next().ok_or_else(invalid_arguments)?,
                ));
            }
            Some("--port") if port.is_none() => {
                let value = arguments.next().ok_or_else(invalid_arguments)?;
                let text = value.to_str().ok_or_else(invalid_arguments)?;
                if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(invalid_arguments());
                }
                port = Some(text.parse::<u16>().map_err(|_| invalid_arguments())?);
            }
            Some("--threads") if kind == Kind::QwenTts && threads.is_none() => {
                let value = arguments.next().ok_or_else(invalid_arguments)?;
                let text = value.to_str().ok_or_else(invalid_arguments)?;
                if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(invalid_arguments());
                }
                let count = text.parse::<usize>().map_err(|_| invalid_arguments())?;
                if !(1..=256).contains(&count) {
                    return Err(invalid_arguments());
                }
                threads = Some(count);
            }
            Some("--quantization") if kind == Kind::QwenTts && quantization.is_none() => {
                quantization = Some(
                    match arguments.next().as_deref().and_then(|value| value.to_str()) {
                        Some("Q8_0") => TtsQuantization::Q8,
                        Some("Q4_K_M") => TtsQuantization::Q4KM,
                        _ => return Err(invalid_arguments()),
                    },
                );
            }
            _ => return Err(invalid_arguments()),
        }
    }
    if kind == Kind::QwenTts && (codec.is_none() || voice_model.is_none()) {
        return Err(invalid_arguments());
    }
    let arguments = ServiceArgs {
        model: model.ok_or_else(invalid_arguments)?,
        codec,
        voice_model,
        token_file: token_file.ok_or_else(invalid_arguments)?,
        port: port.unwrap_or(match kind {
            Kind::Canary => 8766,
            Kind::QwenTts => 8767,
        }),
        threads,
        quantization: quantization.unwrap_or_default(),
    };
    if arguments.port < 1024 {
        return Err(invalid_arguments());
    }
    for path in [&arguments.model, &arguments.token_file]
        .into_iter()
        .chain(arguments.codec.as_ref())
        .chain(arguments.voice_model.as_ref())
    {
        validate_file(path)?;
    }
    Ok(Command::Serve(arguments))
}

/// Exact no-model-load identity receipt for PowerShell binary provisioning.
#[must_use]
pub fn version(kind: Kind) -> serde_json::Value {
    serde_json::json!({
        "Name": match kind { Kind::Canary => "bonsai-speech-asr", Kind::QwenTts => "bonsai-speech-tts" },
        "Version": env!("CARGO_PKG_VERSION"), "Architecture": std::env::consts::ARCH,
        "Model": kind.model(), "ProtocolVersion": 1,
    })
}

/// Verify only the compiled checkpoint identities and retain their file guards.
///
/// `compiled_lock` must come from `include_str!` in the companion binary. No
/// command-line hash, model identity, repository or lock-file override is accepted.
/// The guarded credential is checked before loading or hashing large artifacts.
///
/// # Errors
/// Rejects malformed pins, the wrong checkpoint family, unsafe input paths, bad
/// credentials and any byte-length or SHA-256 mismatch. Errors are value-free.
pub fn verify_models(
    kind: Kind,
    arguments: &ServiceArgs,
    compiled_lock: &str,
) -> io::Result<ModelGuards> {
    let credential = Credential::open(&arguments.token_file).map_err(|_| invalid_models())?;
    let pin: serde_json::Value =
        serde_json::from_str(compiled_lock).map_err(|_| invalid_models())?;
    if pin.get("schemaVersion").and_then(serde_json::Value::as_u64) != Some(1)
        || pin.get("model").and_then(serde_json::Value::as_str) != Some(kind.model())
    {
        return Err(invalid_models());
    }
    let models = match kind {
        Kind::Canary => {
            if arguments.codec.is_some() || arguments.voice_model.is_some() {
                return Err(invalid_models());
            }
            vec![verify_artifact(&arguments.model, &pin)?]
        }
        Kind::QwenTts => {
            let files = pin.get("files").ok_or_else(invalid_models)?;
            let codec = arguments.codec.as_ref().ok_or_else(invalid_models)?;
            let voice_model = arguments.voice_model.as_ref().ok_or_else(invalid_models)?;
            vec![
                verify_artifact(
                    &arguments.model,
                    files.get("talker").ok_or_else(invalid_models)?,
                )?,
                verify_artifact(codec, files.get("codec").ok_or_else(invalid_models)?)?,
                verify_artifact(
                    voice_model,
                    files.get("voice_talker").ok_or_else(invalid_models)?,
                )?,
            ]
        }
    };
    Ok(ModelGuards {
        _models: models,
        _credential: credential,
    })
}

fn verify_artifact(path: &Path, pin: &serde_json::Value) -> io::Result<VerifiedFile> {
    validate_file(path)?;
    let bytes = pin
        .get("bytes")
        .and_then(serde_json::Value::as_u64)
        .filter(|bytes| *bytes > 0)
        .ok_or_else(invalid_models)?;
    let hash = pin
        .get("sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(invalid_models)?;
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid_models());
    }
    VerifiedFile::verify(path, bytes, hash).map_err(|_| invalid_models())
}

fn validate_file(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    bonsai_windows_credentials::validate_read_path(path).map_err(|_| invalid_arguments())?;
    if !path.is_absolute() {
        return Err(invalid_arguments());
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid_arguments())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid_arguments());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid_arguments());
        }
    }
    Ok(())
}

fn invalid_arguments() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "Use the fixed native speech options and existing absolute external files.",
    )
}
fn invalid_models() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "The compiled speech model artifacts or credential could not be verified.",
    )
}

#[cfg(all(test, windows))]
mod tests {
    use super::{Command, ServiceArgs, parse, verify_models};
    use crate::Kind;
    use std::{
        ffi::OsString,
        fs, io,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn fixture() -> io::Result<(PathBuf, Vec<OsString>)> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-NativeSpeechCli-{stamp}"));
        fs::create_dir(&directory)?;
        let model = directory.join("model.gguf");
        let key = directory.join("token.key");
        fs::write(&model, b"abc")?;
        fs::write(&key, [b'a'; 64])?;
        let values = vec![
            OsString::from("--model"),
            model.into_os_string(),
            OsString::from("--token-file"),
            key.into_os_string(),
        ];
        Ok((directory, values))
    }

    #[test]
    fn native_cli_requires_fixed_complete_paths_and_rejects_duplicate_overrides() -> io::Result<()>
    {
        let (directory, values) = fixture()?;
        let Command::Serve(arguments) = parse(Kind::Canary, values.clone())? else {
            return Err(io::Error::other("Missing serve arguments"));
        };
        assert_eq!(arguments.port, 8766);
        assert!(arguments.codec.is_none());
        assert!(arguments.voice_model.is_none());
        for extra in [
            vec!["--model", "C:\\replacement"],
            vec!["--hash", "override"],
            vec!["--port", "1023"],
            vec!["--port", "+8766"],
            vec!["--version"],
            vec!["--codec", "C:\\codec"],
            vec!["--voice-model", "C:\\voice"],
        ] {
            let mut invalid = values.clone();
            invalid.extend(extra.into_iter().map(OsString::from));
            assert!(parse(Kind::Canary, invalid).is_err());
        }
        assert!(parse(Kind::QwenTts, values.clone()).is_err());
        let codec = directory.join("codec.gguf");
        fs::write(&codec, b"abc")?;
        let mut tts = values;
        tts.extend([OsString::from("--codec"), codec.clone().into_os_string()]);
        assert!(parse(Kind::QwenTts, tts.clone()).is_err());
        tts.extend([OsString::from("--voice-model"), codec.into_os_string()]);
        assert!(
            matches!(parse(Kind::QwenTts, tts)?, Command::Serve(arguments) if arguments.port == 8767 && arguments.codec.is_some() && arguments.voice_model.is_some())
        );
        assert!(matches!(
            parse(Kind::QwenTts, [OsString::from("--version")])?,
            Command::Version
        ));
        Ok(())
    }

    #[test]
    fn compiled_artifact_guards_reject_changed_bytes_and_wrong_model_identity() -> io::Result<()> {
        let (_, values) = fixture()?;
        let Command::Serve(arguments) = parse(Kind::Canary, values)? else {
            return Err(io::Error::other("Missing serve arguments"));
        };
        let pin = r#"{"schemaVersion":1,"model":"nvidia/canary-qwen-2.5b","bytes":3,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"}"#;
        let guard = verify_models(Kind::Canary, &arguments, pin)?;
        assert!(
            fs::OpenOptions::new()
                .write(true)
                .open(&arguments.model)
                .is_err()
        );
        drop(guard);
        fs::write(&arguments.model, b"abd")?;
        assert!(verify_models(Kind::Canary, &arguments, pin).is_err());
        assert!(verify_models(Kind::QwenTts, &arguments, pin).is_err());
        let wrong = ServiceArgs {
            model: arguments.model,
            token_file: arguments.token_file,
            codec: None,
            voice_model: None,
            port: 8767,
            threads: None,
            quantization: super::TtsQuantization::default(),
        };
        assert!(verify_models(Kind::QwenTts, &wrong, pin).is_err());
        Ok(())
    }

    #[test]
    fn tts_guards_verify_and_retain_the_base_voice_model() -> io::Result<()> {
        let (directory, mut values) = fixture()?;
        let codec = directory.join("codec.gguf");
        let voice = directory.join("voice.gguf");
        fs::write(&codec, b"abc")?;
        fs::write(&voice, b"abc")?;
        values.extend([
            OsString::from("--codec"),
            codec.into_os_string(),
            OsString::from("--voice-model"),
            voice.clone().into_os_string(),
        ]);
        let Command::Serve(mut arguments) = parse(Kind::QwenTts, values.clone())? else {
            return Err(io::Error::other("Missing TTS serve arguments"));
        };
        let artifact = serde_json::json!({
            "bytes": 3,
            "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        });
        let pin = serde_json::json!({
            "schemaVersion": 1, "model": Kind::QwenTts.model(),
            "files": {"talker": artifact, "codec": artifact, "voice_talker": artifact},
        });
        let guard = verify_models(Kind::QwenTts, &arguments, &pin.to_string())?;
        assert!(fs::OpenOptions::new().write(true).open(&voice).is_err());
        drop(guard);
        fs::write(&voice, b"abd")?;
        assert!(verify_models(Kind::QwenTts, &arguments, &pin.to_string()).is_err());
        arguments.voice_model = None;
        assert!(verify_models(Kind::QwenTts, &arguments, &pin.to_string()).is_err());
        let mut duplicate = values.clone();
        duplicate.extend([OsString::from("--voice-model"), voice.into_os_string()]);
        assert!(parse(Kind::QwenTts, duplicate).is_err());
        let mut relative = values;
        let last = relative
            .last_mut()
            .ok_or_else(|| io::Error::other("Missing voice path"))?;
        *last = OsString::from("relative.gguf");
        assert!(parse(Kind::QwenTts, relative).is_err());
        Ok(())
    }

    #[test]
    fn native_cli_rejects_model_reparse_files() -> io::Result<()> {
        let (directory, values) = fixture()?;
        let link = directory.join("redirect.gguf");
        std::os::windows::fs::symlink_file(directory.join("model.gguf"), &link)?;
        let mut invalid = values;
        invalid[1] = link.into_os_string();
        assert!(parse(Kind::Canary, invalid).is_err());
        Ok(())
    }

    #[test]
    fn tts_cli_bounds_thread_counts_and_pinned_quantization() -> io::Result<()> {
        let (directory, mut values) = fixture()?;
        let codec = directory.join("codec.gguf");
        fs::write(&codec, b"abc")?;
        values.extend([
            OsString::from("--codec"),
            codec.clone().into_os_string(),
            OsString::from("--voice-model"),
            codec.into_os_string(),
        ]);
        let mut tuned = values.clone();
        tuned.extend(["--threads", "10", "--quantization", "Q4_K_M"].map(OsString::from));
        let Command::Serve(arguments) = parse(Kind::QwenTts, tuned)? else {
            return Err(io::Error::other("Expected TTS settings."));
        };
        assert_eq!(arguments.threads, Some(10));
        assert_eq!(arguments.quantization, super::TtsQuantization::Q4KM);
        for extra in [
            vec!["--threads", "0"],
            vec!["--threads", "257"],
            vec!["--threads", "+2"],
            vec!["--threads", "2", "--threads", "3"],
            vec!["--quantization", "unknown"],
            vec!["--quantization", "Q8_0", "--quantization", "Q4_K_M"],
        ] {
            let mut invalid = values.clone();
            invalid.extend(extra.into_iter().map(OsString::from));
            assert!(parse(Kind::QwenTts, invalid).is_err());
        }
        Ok(())
    }
}
