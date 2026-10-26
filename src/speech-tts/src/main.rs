//! Native ARM64 Qwen `VoiceDesign` service with retained compiled artifact guards.
#![forbid(unsafe_code)]

use bonsai_speech_service::{
    Kind,
    launch::{self, Command, TtsQuantization},
};
use std::{
    io::{self, Write as _},
    process::ExitCode,
};

#[cfg(windows)]
struct NativeEngine {
    synthesizer: bonsai_speech_tts::Synthesizer,
    _models: launch::ModelGuards,
}

#[cfg(windows)]
impl bonsai_speech_service::Engine for NativeEngine {
    fn streaming_pcm(&self) -> bool {
        true
    }

    fn consistent_voice(&self) -> bool {
        true
    }

    fn infer_stream(
        &mut self,
        request: bonsai_speech_service::Request<'_>,
        sink: &mut (dyn FnMut(&[f32]) -> io::Result<()> + Send),
    ) -> io::Result<()> {
        let bonsai_speech_service::Request::Synthesize {
            text,
            voice_description,
            language: "Auto",
        } = request
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Unsupported native synthesis request.",
            ));
        };
        self.synthesizer
            .synthesize_stream(text, voice_description, sink)
    }

    fn kind(&self) -> Kind {
        Kind::QwenTts
    }
    fn infer(
        &mut self,
        request: bonsai_speech_service::Request<'_>,
    ) -> io::Result<bonsai_speech_service::Output> {
        let bonsai_speech_service::Request::Synthesize {
            text,
            voice_description,
            language: "Auto",
        } = request
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Unsupported native synthesis request.",
            ));
        };
        let audio = self
            .synthesizer
            .synthesize(text, voice_description)
            .map_err(|_| io::Error::other("Native synthesis failed."))?;
        Ok(bonsai_speech_service::Output::Audio(
            bonsai_speech_service::Audio {
                samples: audio.samples,
                sample_rate: audio.sample_rate,
            },
        ))
    }
}

#[cfg(windows)]
fn run() -> io::Result<()> {
    launch::require_native_host()?;
    match launch::parse(Kind::QwenTts, std::env::args_os().skip(1))? {
        Command::Version => {
            serde_json::to_writer(io::stdout().lock(), &launch::version(Kind::QwenTts))?;
            writeln!(io::stdout().lock())
        }
        Command::Serve(arguments) => {
            let guards = launch::verify_models(
                Kind::QwenTts,
                &arguments,
                match arguments.quantization {
                    TtsQuantization::Q8 => include_str!("../model.lock.json"),
                    TtsQuantization::Q4KM => include_str!("../model-q4.lock.json"),
                },
            )?;
            let codec = arguments
                .codec
                .as_ref()
                .ok_or_else(|| io::Error::other("Native speech codec is required."))?;
            let voice_model = arguments
                .voice_model
                .as_ref()
                .ok_or_else(|| io::Error::other("Native speech voice model is required."))?;
            let synthesizer = bonsai_speech_tts::Synthesizer::load_with_threads(
                &arguments.model,
                codec,
                voice_model,
                arguments.threads,
            )
            .map_err(|_| io::Error::other("Native Qwen model loading failed."))?;
            let engine = NativeEngine {
                synthesizer,
                _models: guards,
            };
            bonsai_speech_service::serve_with_ready(
                engine,
                arguments.port,
                &arguments.token_file,
                |port| {
                    writeln!(
                        io::stdout().lock(),
                        "Native Qwen VoiceDesign ready at 127.0.0.1:{port}"
                    )
                },
            )
        }
    }
}

#[cfg(not(windows))]
fn run() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Native speech requires Windows ARM64.",
    ))
}

fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|_| {
        let _written = writeln!(io::stderr().lock(), "Native Qwen speech service failed.");
    }));
    if run().is_ok() {
        ExitCode::SUCCESS
    } else {
        let _written = writeln!(
            io::stderr().lock(),
            "Native Qwen speech service failed; check verified models and local configuration."
        );
        ExitCode::FAILURE
    }
}
