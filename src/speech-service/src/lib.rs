//! Native companion transport shared by Canary recognition and Qwen `VoiceDesign`.
//!
//! Load the actual engine before calling [`serve`]. This library belongs in a
//! separately owned speech companion process: its inference watchdog terminates
//! that process on client cancellation or an expired model deadline. It never
//! records microphone input, plays sound, logs model input or writes audio files.
#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "Explicit parent-only visibility keeps protocol and credential internals out of the public engine API."
)]

use socket2::{Domain, Protocol, Socket, Type};
use std::{
    io,
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
    path::Path,
};

mod credentials;
mod http;
pub mod launch;
mod request;
mod watchdog;
mod wave;

#[cfg(test)]
mod tests;

/// The only supported recognition checkpoint identity.
pub const CANARY_MODEL: &str = "nvidia/canary-qwen-2.5b";
/// The only supported voice-design checkpoint identity.
pub const TTS_MODEL: &str = "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign";

/// Fixed speech service capabilities, independent of engine implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// Canary Qwen speech recognition with 16 kHz mono input.
    Canary,
    /// Qwen3-TTS 1.7B `VoiceDesign` with 24 kHz mono output.
    QwenTts,
}

impl Kind {
    /// Returns the immutable model identity exposed by this service.
    #[must_use]
    pub const fn model(self) -> &'static str {
        match self {
            Self::Canary => CANARY_MODEL,
            Self::QwenTts => TTS_MODEL,
        }
    }
}

/// One validated, bounded model request; no credential bytes reach the engine.
pub enum Request<'a> {
    /// Recognize a complete mono PCM clip of at most 30 seconds.
    Transcribe {
        /// Normalized finite signed PCM samples in the range -1 through 1.
        samples: &'a [f32],
        /// The fixed input sample rate, 16,000 Hz.
        sample_rate: u32,
    },
    /// Design a voice from plain text and synthesize one complete bounded clip.
    Synthesize {
        /// Plain text containing at most 2,048 Unicode scalar values.
        text: &'a str,
        /// Plain voice description containing at most 1,024 scalar values.
        voice_description: &'a str,
        /// Fixed Auto mode; the native model follows the input text's language.
        language: &'a str,
    },
}

/// A complete generated mono waveform. Truncated generation must return an error.
pub struct Audio {
    /// Finite samples, clipped only when encoded into signed PCM16 output.
    pub samples: Vec<f32>,
    /// Output must use the fixed 24,000 Hz service rate.
    pub sample_rate: u32,
}

/// Completed inference output, retained in memory until its HTTP response ends.
pub enum Output<S = Vec<f32>> {
    /// Plain recognition output, never automatically submitted as a chat prompt.
    Transcript(String),
    /// Complete voice-design waveform with no reference recording or preset.
    Audio(Audio),
    /// Complete waveform retained by its original allocation owner.
    ///
    /// Native engines can keep their matching release guard here instead of
    /// copying the full clip into a Rust vector. The service borrows samples
    /// only after inference returns, validates the complete clip before sending
    /// headers, and drops this owner after the response or any failure.
    Waveform {
        /// Immutable complete samples, owned for the lifetime of this object.
        samples: S,
        /// Output must use the fixed 24,000 Hz service rate.
        sample_rate: u32,
    },
}

/// An already loaded native model implementation.
pub trait Engine {
    /// Concrete complete-waveform owner, retained through validation and delivery.
    type Waveform: AsRef<[f32]>;

    /// Returns the fixed protocol and checkpoint family implemented here.
    fn kind(&self) -> Kind;
    /// Runs exactly one bounded inference. Errors must not contain user data.
    ///
    /// # Errors
    /// Return an error for failed or incomplete generation. The protocol emits
    /// only a value-free model-failure code, without formatting this error.
    fn infer(&mut self, request: Request<'_>) -> io::Result<Output<Self::Waveform>>;
    /// Whether this engine implements bounded incremental mono 24 kHz output.
    fn streaming_pcm(&self) -> bool {
        false
    }
    /// Whether one designed speaker reference is reused across synthesis requests.
    ///
    /// Description-only generation with a fresh voice per request must return false.
    fn consistent_voice(&self) -> bool {
        false
    }
    /// Generate finite mono 24 kHz samples incrementally, blocking until complete.
    ///
    /// The sink may run on the native compute worker. Stop and return an error
    /// if the sink rejects a chunk. Successful return certifies complete output.
    ///
    /// # Errors
    /// Returns an error for unsupported, failed, cancelled or truncated output.
    fn infer_stream(
        &mut self,
        _request: Request<'_>,
        _sink: &mut (dyn FnMut(&[f32]) -> io::Result<()> + Send),
    ) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Streaming speech is unavailable.",
        ))
    }
}

/// Serves an authenticated numeric-loopback endpoint after the engine is loaded.
///
/// One connection is handled at a time, with an OS backlog of one. Inference is
/// bounded to 120 seconds for recognition and 180 seconds for synthesis. A peer
/// disconnect, extra queued request data or inference timeout terminates only
/// this companion process; its caller must own the companion's full lifetime.
///
/// # Errors
/// Rejects privileged ports, unsafe credentials, failed exclusive binding and
/// failed listener accepts. Malformed individual requests receive bounded errors.
pub fn serve(engine: impl Engine, port: u16, key_file: &Path) -> io::Result<()> {
    serve_with_ready(engine, port, key_file, |_| Ok(()))
}

/// Serves a loaded companion and invokes a value-free notification after binding.
///
/// The callback runs only after credential validation and exclusive listening
/// have succeeded. It must not log credential bytes, text or audio.
///
/// # Errors
/// Returns the same listener/credential failures as [`serve`], or callback I/O
/// failure. The engine must already have loaded its independently verified files.
pub fn serve_with_ready(
    mut engine: impl Engine,
    port: u16,
    key_file: &Path,
    on_ready: impl FnOnce(u16) -> io::Result<()>,
) -> io::Result<()> {
    if port < 1024 {
        return Err(io::Error::other("Use an unprivileged speech port."));
    }
    let credential = credentials::Credential::open(key_file)?;
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
    #[cfg(windows)]
    bonsai_windows_process::require_exclusive_address(&socket)?;
    socket.bind(&SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into())?;
    socket.listen(1)?;
    let listener: TcpListener = socket.into();
    on_ready(port)?;
    let mut scratch = http::Scratch::default();
    loop {
        let (mut peer, address) = listener.accept()?;
        if address.ip() != Ipv4Addr::LOCALHOST {
            continue;
        }
        http::handle(
            &mut peer,
            port,
            credential.bytes(),
            &mut engine,
            &mut scratch,
        );
    }
}
