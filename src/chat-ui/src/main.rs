//! Pure Rust desktop chat; the native boundary uses the operating system controls.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod client;
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Audited Windows speech COM boundary, confined to its worker thread."
)]
mod speech;
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
            _ => {
                return Err(
                    "Usage: bonsai-chat --broker-path ABSOLUTE_EXE --data-directory ABSOLUTE_DIRECTORY --evidence-directory ABSOLUTE_DIRECTORY [--frontend-port 5173] [--backend-port 8081] [--no-abliterate-lora]",
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
    };
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
    Ok(config)
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
