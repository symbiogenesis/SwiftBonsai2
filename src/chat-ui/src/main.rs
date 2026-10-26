//! Pure Rust desktop chat; the native boundary uses the operating system controls.
#![cfg_attr(windows, windows_subsystem = "windows")]
#![allow(
    clippy::multiple_crate_versions,
    reason = "Pinned Microsoft WinRT binding macros require syn 2 while pinned serde_derive requires syn 3; their incompatible major APIs cannot be unified."
)]

#[cfg(windows)]
mod canary;
#[cfg(windows)]
mod client;
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Audited native WAV playback boundary; owned buffers live until playback completes."
)]
mod speech;
#[cfg(windows)]
mod tts;
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Audited Win32 control and message-loop boundary."
)]
mod window;

#[cfg(windows)]
fn configuration(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<client::Config, &'static str> {
    use std::path::PathBuf;
    let mut args = args.into_iter();
    let (mut broker, mut data, mut evidence) = (None, None, None);
    let (mut frontend_port, mut backend_port) = (None, None);
    let (mut canary_port, mut canary_key_file) = (None, None);
    let (mut tts_port, mut tts_key_file) = (None, None);
    let mut no_abliterate_lora = false;
    while let Some(option) = args.next() {
        match option.to_str() {
            Some("--no-abliterate-lora") if !no_abliterate_lora => no_abliterate_lora = true,
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
            _ => {
                return Err(
                    "Usage: bonsai-chat --broker-path ABSOLUTE_EXE --data-directory ABSOLUTE_DIRECTORY --evidence-directory ABSOLUTE_DIRECTORY [--frontend-port 5173] [--backend-port 8081] [--no-abliterate-lora] [--canary-key-file ABSOLUTE_FILE --canary-port 8766] [--tts-key-file ABSOLUTE_FILE --tts-port 8767]",
                );
            }
        }
    }
    let config = client::Config {
        broker: broker.ok_or("A broker executable is required.")?,
        data: data.ok_or("A provisioned data directory is required.")?,
        evidence: evidence.ok_or("An external evidence directory is required.")?,
        frontend_port: frontend_port.unwrap_or(5173),
        backend_port: backend_port.unwrap_or(8081),
        use_abliterate_lora: !no_abliterate_lora,
        canary: canary_key_file.map(|key_file| canary::Config {
            port: canary_port.unwrap_or(8766),
            key_file,
        }),
        qwen_tts: tts_key_file.map(|key_file| tts::Config {
            port: tts_port.unwrap_or(8767),
            key_file,
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
    }
    Ok(())
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let result = configuration(std::env::args_os().skip(1))
        .map_err(std::io::Error::other)
        .and_then(window::run);
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
    fn demo_lora_is_default_and_opt_out_is_explicit() -> io::Result<()> {
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
