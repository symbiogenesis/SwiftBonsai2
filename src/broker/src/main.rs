//! Explicit development-only broker executable; never launches a model backend.

use bonsai_broker::runtime::{DevelopmentConfig, RunExit, USAGE, run};
use std::{env, io::Write, process::ExitCode};

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1).peekable();
    if args.peek().is_some_and(|value| value == "--help") {
        args.next();
        return if args.next().is_none() && std::io::stdout().write_all(USAGE.as_bytes()).is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
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

fn failure(message: &[u8]) -> ExitCode {
    let _result = std::io::stderr().write_all(message);
    ExitCode::FAILURE
}
