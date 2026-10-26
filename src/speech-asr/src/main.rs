//! Native ARM64 Canary service; no Python interpreter or model downloads.
#![forbid(unsafe_code)]

use bonsai_speech_service::{
    Engine, Kind, Output, Request,
    launch::{self, Command, ModelGuards},
};
use std::{
    io::{self, Write as _},
    process::ExitCode,
};

struct NativeEngine {
    recognizer: bonsai_speech_asr::Recognizer,
    _models: ModelGuards,
}

impl Engine for NativeEngine {
    fn kind(&self) -> Kind {
        Kind::Canary
    }
    fn infer(&mut self, request: Request<'_>) -> io::Result<Output> {
        let Request::Transcribe {
            samples,
            sample_rate: 16_000,
        } = request
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Unsupported native recognition request.",
            ));
        };
        self.recognizer
            .transcribe(samples)
            .map(Output::Transcript)
            .map_err(|_| io::Error::other("Native recognition failed."))
    }
}

fn run() -> io::Result<()> {
    launch::require_native_host()?;
    match launch::parse(Kind::Canary, std::env::args_os().skip(1))? {
        Command::Version => {
            serde_json::to_writer(io::stdout().lock(), &launch::version(Kind::Canary))?;
            writeln!(io::stdout().lock())
        }
        Command::Serve(arguments) => {
            let guards = launch::verify_models(
                Kind::Canary,
                &arguments,
                include_str!("../model.lock.json"),
            )?;
            let recognizer = bonsai_speech_asr::Recognizer::load(&arguments.model)
                .map_err(|_| io::Error::other("Native Canary model loading failed."))?;
            let engine = NativeEngine {
                recognizer,
                _models: guards,
            };
            bonsai_speech_service::serve_with_ready(
                engine,
                arguments.port,
                &arguments.token_file,
                |port| {
                    writeln!(
                        io::stdout().lock(),
                        "Native Canary ready at 127.0.0.1:{port}"
                    )
                },
            )
        }
    }
}

fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|_| {
        let _written = writeln!(io::stderr().lock(), "Native Canary speech service failed.");
    }));
    if run().is_ok() {
        ExitCode::SUCCESS
    } else {
        let _written = writeln!(
            io::stderr().lock(),
            "Native Canary speech service failed; check verified models and local configuration."
        );
        ExitCode::FAILURE
    }
}
