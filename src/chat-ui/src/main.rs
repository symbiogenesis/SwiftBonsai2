//! Pure Rust egui desktop chat with isolated native process and speech boundaries.
#![cfg_attr(windows, windows_subsystem = "windows")]
#![allow(
    clippy::multiple_crate_versions,
    reason = "The pinned native GUI, graphics and WinRT dependencies require incompatible major versions of shared implementation crates."
)]

#[cfg(windows)]
mod canary;
#[cfg(windows)]
mod client;
#[cfg(windows)]
mod native_speech;
#[cfg(windows)]
mod recovery;
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Audited native WAV playback boundary; owned buffers live until playback completes."
)]
mod speech;
#[cfg(windows)]
mod tts;
#[cfg(windows)]
mod window;

#[cfg(windows)]
#[allow(
    clippy::too_many_lines,
    reason = "Parse the bounded native launcher contract without accepting arbitrary service arguments."
)]
fn configuration(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<client::Config, &'static str> {
    use std::path::PathBuf;
    let mut args = args.into_iter();
    let (mut broker, mut data, mut evidence) = (None, None, None);
    let (mut frontend_port, mut backend_port) = (None, None);
    let (mut canary_port, mut canary_key_file) = (None, None);
    let (mut canary_executable, mut canary_model) = (None, None);
    let (mut tts_port, mut tts_key_file) = (None, None);
    let (mut tts_executable, mut tts_model, mut tts_codec) = (None, None, None);
    let (mut tts_quantization, mut tts_threads) = (None, None);
    let mut no_abliterate_lora = false;
    let mut use_repacked_ptq1 = false;
    while let Some(option) = args.next() {
        match option.to_str() {
            Some("--no-abliterate-lora") if !no_abliterate_lora => no_abliterate_lora = true,
            Some("--use-repacked-ptq1") if !use_repacked_ptq1 => use_repacked_ptq1 = true,
            Some("--broker-path") if broker.is_none() => broker = args.next().map(PathBuf::from),
            Some("--data-directory") if data.is_none() => data = args.next().map(PathBuf::from),
            Some("--evidence-directory") if evidence.is_none() => {
                evidence = args.next().map(PathBuf::from);
            }
            Some("--frontend-port") if frontend_port.is_none() => {
                frontend_port = Some(
                    args.next()
                        .and_then(|arg| arg.to_str().and_then(|text| text.parse::<u16>().ok()))
                        .ok_or("Invalid frontend port.")?,
                );
            }
            Some("--backend-port") if backend_port.is_none() => {
                backend_port = Some(
                    args.next()
                        .and_then(|arg| arg.to_str().and_then(|text| text.parse::<u16>().ok()))
                        .ok_or("Invalid backend port.")?,
                );
            }
            Some("--canary-port") if canary_port.is_none() => {
                canary_port = Some(
                    args.next()
                        .and_then(|arg| arg.to_str().and_then(|text| text.parse::<u16>().ok()))
                        .ok_or("Invalid Canary port.")?,
                );
            }
            Some("--canary-key-file") if canary_key_file.is_none() => {
                canary_key_file = args.next().map(PathBuf::from);
                if canary_key_file.is_none() {
                    return Err("A Canary key-file path is required.");
                }
            }
            Some("--tts-port") if tts_port.is_none() => {
                tts_port = Some(
                    args.next()
                        .and_then(|arg| arg.to_str().and_then(|text| text.parse::<u16>().ok()))
                        .ok_or("Invalid Qwen TTS port.")?,
                );
            }
            Some("--tts-key-file") if tts_key_file.is_none() => {
                tts_key_file = args.next().map(PathBuf::from);
                if tts_key_file.is_none() {
                    return Err("A Qwen TTS key-file path is required.");
                }
            }
            Some("--canary-executable") if canary_executable.is_none() => {
                canary_executable = Some(PathBuf::from(
                    args.next()
                        .ok_or("A native Canary executable path is required.")?,
                ));
            }
            Some("--canary-model") if canary_model.is_none() => {
                canary_model = Some(PathBuf::from(
                    args.next().ok_or("A Canary GGUF model path is required.")?,
                ));
            }
            Some("--tts-executable") if tts_executable.is_none() => {
                tts_executable = Some(PathBuf::from(
                    args.next()
                        .ok_or("A native Qwen TTS executable path is required.")?,
                ));
            }
            Some("--tts-model") if tts_model.is_none() => {
                tts_model = Some(PathBuf::from(
                    args.next()
                        .ok_or("A Qwen TTS GGUF model path is required.")?,
                ));
            }
            Some("--tts-codec") if tts_codec.is_none() => {
                tts_codec = Some(PathBuf::from(
                    args.next()
                        .ok_or("A Qwen TTS codec GGUF path is required.")?,
                ));
            }
            Some("--tts-quantization") if tts_quantization.is_none() => {
                let value = args.next().ok_or("A TTS quantization is required.")?;
                if !matches!(value.to_str(), Some("Q8_0" | "Q4_K_M")) {
                    return Err("TTS quantization must be Q8_0 or Q4_K_M.");
                }
                tts_quantization = value.into_string().ok();
            }
            Some("--tts-threads") if tts_threads.is_none() => {
                tts_threads = Some(
                    args.next()
                        .and_then(|value| {
                            value.to_str().and_then(|text| text.parse::<usize>().ok())
                        })
                        .filter(|count| (1..=256).contains(count))
                        .ok_or("TTS threads must be between 1 and 256.")?,
                );
            }
            _ => {
                return Err(
                    "Usage: bonsai-chat --broker-path ABSOLUTE_EXE --data-directory ABSOLUTE_DIRECTORY --evidence-directory ABSOLUTE_DIRECTORY [--frontend-port 5173] [--backend-port 8081] [--no-abliterate-lora] [--use-repacked-ptq1] [--canary-key-file ABSOLUTE_FILE --canary-port 8766 [--canary-executable ABSOLUTE_EXE --canary-model ABSOLUTE_GGUF]] [--tts-key-file ABSOLUTE_FILE --tts-port 8767 [--tts-executable ABSOLUTE_EXE --tts-model ABSOLUTE_GGUF --tts-codec ABSOLUTE_GGUF]]",
                );
            }
        }
    }
    let canary_launch = match (canary_executable, canary_model) {
        (None, None) => None,
        (Some(executable), Some(model)) if canary_key_file.is_some() => {
            Some(native_speech::Launch {
                executable,
                model,
                codec: None,
                quantization: None,
                threads: None,
                evidence: evidence
                    .clone()
                    .ok_or("An external evidence directory is required.")?,
            })
        }
        _ => return Err("Configure both native Canary launch paths with --canary-key-file."),
    };
    let tts_launch = match (tts_executable, tts_model, tts_codec) {
        (None, None, None) => None,
        (Some(executable), Some(model), Some(codec)) if tts_key_file.is_some() => {
            Some(native_speech::Launch {
                executable,
                model,
                codec: Some(codec),
                quantization: tts_quantization.clone(),
                threads: tts_threads,
                evidence: evidence
                    .clone()
                    .ok_or("An external evidence directory is required.")?,
            })
        }
        _ => return Err("Configure all three native Qwen TTS launch paths with --tts-key-file."),
    };
    if (tts_quantization.is_some() || tts_threads.is_some()) && tts_launch.is_none() {
        return Err("TTS quantization and thread settings require an owned native TTS service.");
    }
    let config = client::Config {
        broker: broker.ok_or("A broker executable is required.")?,
        data: data.ok_or("A provisioned data directory is required.")?,
        evidence: evidence.ok_or("An external evidence directory is required.")?,
        frontend_port: frontend_port.unwrap_or(5173),
        backend_port: backend_port.unwrap_or(8081),
        use_abliterate_lora: !no_abliterate_lora,
        use_repacked_ptq1,
        canary: canary_key_file.map(|key_file| canary::Config {
            port: canary_port.unwrap_or(8766),
            key_file,
            launch: canary_launch,
        }),
        qwen_tts: tts_key_file.map(|key_file| tts::Config {
            port: tts_port.unwrap_or(8767),
            key_file,
            launch: tts_launch,
        }),
    };
    validate_configuration(&config, canary_port.is_some(), tts_port.is_some())?;
    Ok(config)
}

#[cfg(windows)]
fn validate_configuration(
    config: &client::Config,
    has_canary_port: bool,
    has_tts_port: bool,
) -> Result<(), &'static str> {
    for path in [&config.broker, &config.data, &config.evidence] {
        bonsai_windows_credentials::validate_external_path(path)
            .map_err(|_| "Use existing expanded absolute local paths outside Git.")?;
    }
    if !config.broker.is_file() || !config.data.is_dir() || !config.evidence.is_dir() {
        return Err("The broker, model data and evidence directory must already exist.");
    }
    if config.frontend_port < 1024
        || config.backend_port < 1024
        || config.frontend_port == config.backend_port
    {
        return Err("Use distinct unprivileged frontend and backend ports.");
    }
    if has_canary_port && config.canary.is_none() {
        return Err("Configure --canary-key-file with --canary-port.");
    }
    if let Some(canary) = &config.canary {
        bonsai_windows_credentials::validate_external_path(&canary.key_file)
            .map_err(|_| "Use an expanded absolute external Canary key-file path.")?;
        if !canary.key_file.is_file()
            || canary.port < 1024
            || canary.port == config.frontend_port
            || canary.port == config.backend_port
        {
            return Err(
                "Canary needs an existing key file and a distinct unprivileged local port.",
            );
        }
        if let Some(launch) = &canary.launch {
            launch.validate()?;
        }
    }
    if has_tts_port && config.qwen_tts.is_none() {
        return Err("Configure --tts-key-file with --tts-port.");
    }
    if let Some(tts) = &config.qwen_tts {
        bonsai_windows_credentials::validate_external_path(&tts.key_file)
            .map_err(|_| "Use an expanded absolute external Qwen TTS key-file path.")?;
        if !tts.key_file.is_file()
            || tts.port < 1024
            || tts.port == config.frontend_port
            || tts.port == config.backend_port
            || config
                .canary
                .as_ref()
                .is_some_and(|canary| canary.port == tts.port)
        {
            return Err(
                "Qwen TTS needs an existing key file and a distinct unprivileged local port.",
            );
        }
        if let Some(launch) = &tts.launch {
            launch.validate()?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let result = if args.len() == 1 && args[0] == "--preview" {
        window::run_preview()
    } else {
        configuration(args)
            .map_err(std::io::Error::other)
            .and_then(window::run)
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            window::show_error(&error.to_string());
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::configuration;
    use std::{ffi::OsString, fs, io};

    #[test]
    fn native_speech_configuration_requires_complete_groups_and_rejects_legacy_options()
    -> io::Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-native-speech-config-{stamp}"));
        fs::create_dir(&directory)?;
        let broker = directory.join("broker.exe");
        let model = directory.join("model.gguf");
        let codec = directory.join("codec.gguf");
        let key = directory.join("speech.key");
        for path in [&broker, &model, &codec, &key] {
            fs::write(path, b"")?;
        }
        let base = vec![
            OsString::from("--broker-path"),
            broker.clone().into_os_string(),
            OsString::from("--data-directory"),
            directory.clone().into_os_string(),
            OsString::from("--evidence-directory"),
            directory.clone().into_os_string(),
        ];
        for (key_flag, launch_flags) in [
            (
                "--canary-key-file",
                vec![("--canary-executable", &broker), ("--canary-model", &model)],
            ),
            (
                "--tts-key-file",
                vec![
                    ("--tts-executable", &broker),
                    ("--tts-model", &model),
                    ("--tts-codec", &codec),
                ],
            ),
        ] {
            let mut enabled = base.clone();
            for (flag, path) in &launch_flags {
                enabled.extend([OsString::from(flag), path.as_os_str().to_owned()]);
            }
            assert!(configuration(enabled.clone()).is_err());
            enabled.extend([OsString::from(key_flag), key.clone().into_os_string()]);
            let config = configuration(enabled.clone()).map_err(io::Error::other)?;
            if key_flag == "--tts-key-file" {
                let mut tuned = enabled.clone();
                tuned.extend([
                    OsString::from("--tts-quantization"),
                    OsString::from("Q4_K_M"),
                    OsString::from("--tts-threads"),
                    OsString::from("10"),
                ]);
                let tuned = configuration(tuned).map_err(io::Error::other)?;
                let launch = tuned
                    .qwen_tts
                    .and_then(|config| config.launch)
                    .ok_or_else(|| io::Error::other("Missing owned TTS configuration"))?;
                assert_eq!(launch.quantization.as_deref(), Some("Q4_K_M"));
                assert_eq!(launch.threads, Some(10));
                for (flag, value) in [
                    ("--tts-threads", "0"),
                    ("--tts-threads", "257"),
                    ("--tts-threads", "-1"),
                    ("--tts-quantization", "Q2_K"),
                ] {
                    let mut invalid = enabled.clone();
                    invalid.extend([OsString::from(flag), OsString::from(value)]);
                    assert!(configuration(invalid).is_err());
                }
            }
            assert!(
                config
                    .canary
                    .as_ref()
                    .is_some_and(|config| config.launch.is_some())
                    || config
                        .qwen_tts
                        .as_ref()
                        .is_some_and(|config| config.launch.is_some())
            );
            let mut partial = base.clone();
            partial.extend([
                OsString::from(key_flag),
                key.clone().into_os_string(),
                OsString::from(launch_flags[0].0),
                broker.clone().into_os_string(),
            ]);
            assert!(configuration(partial).is_err());
            let mut repeated = enabled;
            repeated.extend([
                OsString::from(launch_flags[0].0),
                broker.clone().into_os_string(),
            ]);
            assert!(configuration(repeated).is_err());
        }
        for old_flag in ["--tts-python", "--tts-service", "--tts-snapshot"] {
            let mut legacy = base.clone();
            legacy.extend([OsString::from(old_flag), broker.clone().into_os_string()]);
            assert!(configuration(legacy).is_err());
        }
        for (flag, value) in [("--tts-threads", "10"), ("--tts-quantization", "Q4_K_M")] {
            let mut unowned = base.clone();
            unowned.extend([OsString::from(flag), OsString::from(value)]);
            assert!(configuration(unowned).is_err());
        }
        for path in [&broker, &model, &codec, &key] {
            fs::remove_file(path)?;
        }
        fs::remove_dir(directory)?;
        Ok(())
    }
    #[test]
    fn canary_configuration_requires_explicit_key_and_distinct_port() -> io::Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-canary-config-{stamp}"));
        fs::create_dir(&directory)?;
        let broker = directory.join("broker.exe");
        let key = directory.join("canary.key");
        fs::write(&broker, b"")?;
        fs::write(&key, "a".repeat(64))?;
        let args = vec![
            OsString::from("--broker-path"),
            broker.clone().into_os_string(),
            OsString::from("--data-directory"),
            directory.clone().into_os_string(),
            OsString::from("--evidence-directory"),
            directory.clone().into_os_string(),
        ];
        assert!(
            configuration(args.clone())
                .map_err(io::Error::other)?
                .canary
                .is_none()
        );
        let mut port_only = args.clone();
        port_only.extend([OsString::from("--canary-port"), OsString::from("8766")]);
        assert!(configuration(port_only).is_err());
        let mut enabled = args;
        enabled.extend([
            OsString::from("--canary-key-file"),
            key.clone().into_os_string(),
        ]);
        let config = configuration(enabled.clone()).map_err(io::Error::other)?;
        assert_eq!(config.canary.as_ref().map(|canary| canary.port), Some(8766));
        let mut colliding = enabled.clone();
        colliding.extend([OsString::from("--canary-port"), OsString::from("8081")]);
        assert!(configuration(colliding).is_err());
        let mut duplicated = enabled;
        duplicated.extend([
            OsString::from("--canary-key-file"),
            key.clone().into_os_string(),
        ]);
        assert!(configuration(duplicated).is_err());
        fs::remove_file(key)?;
        fs::remove_file(broker)?;
        fs::remove_dir(directory)?;
        Ok(())
    }

    #[test]
    fn model_packing_and_lora_defaults_require_explicit_opt_ins() -> io::Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-demo-config-{stamp}"));
        fs::create_dir(&directory)?;
        let broker = directory.join("broker.exe");
        fs::write(&broker, b"")?;
        let args = vec![
            OsString::from("--broker-path"),
            broker.clone().into_os_string(),
            OsString::from("--data-directory"),
            directory.clone().into_os_string(),
            OsString::from("--evidence-directory"),
            directory.clone().into_os_string(),
        ];
        let default = configuration(args.clone()).map_err(io::Error::other)?;
        assert!(default.use_abliterate_lora);
        assert!(!default.use_repacked_ptq1);
        let mut repacked = args.clone();
        repacked.push("--use-repacked-ptq1".into());
        let alternate = configuration(repacked.clone()).map_err(io::Error::other)?;
        assert!(alternate.use_repacked_ptq1);
        assert!(alternate.use_abliterate_lora);
        repacked.push("--use-repacked-ptq1".into());
        assert!(configuration(repacked).is_err());
        let mut disabled = args;
        disabled.push("--no-abliterate-lora".into());
        let opted_out = configuration(disabled.clone()).map_err(io::Error::other)?;
        assert!(!opted_out.use_abliterate_lora);
        disabled.push("--no-abliterate-lora".into());
        assert!(configuration(disabled).is_err());
        // Neither mode requires adapter bytes at UI parsing time; the broker
        // verifies the selected artifacts before creating an inference process.
        assert!(!directory.join("adapters").exists());
        fs::remove_file(broker)?;
        fs::remove_dir(directory)?;
        Ok(())
    }
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::FAILURE
}
