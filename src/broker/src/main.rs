//! Explicit development-only broker with optional owned Windows backend launch.

use bonsai_broker::runtime::{DevelopmentConfig, RunExit, USAGE, run};
use std::{env, ffi::OsString, io::Write, process::ExitCode};

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1).peekable();
    if args.peek().is_some_and(|value| value == "--help") {
        args.next();
        let mut help = USAGE.to_owned();
        #[cfg(windows)]
        help.push_str(bonsai_broker::launcher::LAUNCH_USAGE);
        return if args.next().is_none() && std::io::stdout().write_all(help.as_bytes()).is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    let args: Vec<_> = args.collect();
    if args
        .iter()
        .any(|argument| argument == "--launch-development")
    {
        return launched(args);
    }
    let Ok(configuration) = DevelopmentConfig::parse(args) else {
        let _result = std::io::stderr().write_all(USAGE.as_bytes());
        return ExitCode::FAILURE;
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return failure(b"Development runtime initialization failed.\n");
    };
    runtime.block_on(async {
        let Ok(owner) = configuration.bind().await else {
            return failure(b"Development configuration, key-file access or binding failed.\n");
        };
        let service = owner.service();
        let notice = format!(
            "DEVELOPMENT ONLY: listening on http://127.0.0.1:{}; key ACLs and backend identity are unverified. Ctrl-C stops requests.\n",
            service.port()
        );
        if std::io::stdout().write_all(notice.as_bytes()).is_err() {
            service.close();
            return ExitCode::FAILURE;
        }
        match run(service, tokio::signal::ctrl_c()).await.exit {
            RunExit::Shutdown => ExitCode::SUCCESS,
            RunExit::ShutdownFailure => failure(b"Shutdown notification failed; requests stopped.\n"),
            RunExit::AdmissionClosed => {
                failure(b"Admission closed; establish a stopped or freshly owned backend before restarting.\n")
            }
            RunExit::AcceptFailure => failure(b"Development listener failed; requests stopped.\n"),
        }
    })
}

#[cfg(windows)]
fn launched(args: Vec<OsString>) -> ExitCode {
    use bonsai_broker::launcher::{LAUNCH_USAGE, LaunchConfig};

    let Ok(configuration) = LaunchConfig::parse(args) else {
        return failure(LAUNCH_USAGE.as_bytes());
    };
    if configuration.is_check_only() {
        return match configuration.check() {
            Ok(check) => {
                let message = format!(
                    "Pinned artifacts and reference launch policy verified. Current memory admission: {}. No credentials or process created.\n",
                    check.memory_admitted
                );
                if std::io::stdout().write_all(message.as_bytes()).is_ok() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(error) => {
                failure(format!("Development launch check failed: {error:?}.\n").as_bytes())
            }
        };
    }
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return failure(b"Development launch runtime initialization failed.\n");
    };
    runtime.block_on(async {
        let mut shutdown = std::pin::pin!(tokio::signal::ctrl_c());
        let owner = match configuration.start(shutdown.as_mut()).await {
            Ok(owner) => owner,
            Err(error) => {
                let message = format!("Development backend launch failed: {error:?}.\n");
                return failure(message.as_bytes());
            }
        };
        let notice = format!(
            "DEVELOPMENT ONLY: owned backend ready; listening on http://127.0.0.1:{}.\nProtected credentials and technical evidence: {}\nFilesystem, token and egress isolation remain unqualified. Ctrl-C stops the complete owned Job.\n",
            owner.port(), owner.evidence_directory().display()
        );
        if std::io::stdout().write_all(notice.as_bytes()).is_err() {
            return ExitCode::FAILURE;
        }
        match owner.run(shutdown.as_mut()).await {
            Ok(report) if report.exit == RunExit::Shutdown => ExitCode::SUCCESS,
            Ok(_) => failure(b"Admission closed; the owned backend has been stopped.\n"),
            Err(error) => {
                let message = format!("Owned backend stopped after launcher failure: {error:?}.\n");
                failure(message.as_bytes())
            }
        }
    })
}

#[cfg(not(windows))]
fn launched(_args: Vec<OsString>) -> ExitCode {
    failure(b"Owned backend launching requires Windows.\n")
}

fn failure(message: &[u8]) -> ExitCode {
    let _result = std::io::stderr().write_all(message);
    ExitCode::FAILURE
}
