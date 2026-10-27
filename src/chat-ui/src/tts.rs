//! Bounded local Qwen3 `VoiceDesign` synthesis with an independent credential.
//!
//! Voice descriptions stay plain model input. Returned PCM remains in memory;
//! this adapter never plays audio or substitutes a system voice.

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "Only the native speech worker uses this crate-private adapter."
)]

pub(crate) use crate::canary::Failure;
use crate::canary::{self, Call, Media};
use crate::native_speech::{Launch, Service};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Duration,
};

const MODEL: &str = "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign";
const MAX_TEXT: usize = 2048;
const MAX_VOICE: usize = 1024;
const SYNTHESIS_LIMIT: Duration = Duration::from_secs(180);
const READY_LIMIT: Duration = Duration::from_secs(5);

/// Explicit local endpoint; no voice text or credential bytes have Debug output.
#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) port: u16,
    pub(crate) key_file: PathBuf,
    pub(crate) launch: Option<Launch>,
}
/// Uses the same bounded transport policy with its own configured key and port.
pub(crate) struct Client(canary::Client, AtomicBool);

impl Client {
    pub(crate) fn new(config: &Config) -> Result<Self, &'static str> {
        canary::Client::new(&canary::Config {
            port: config.port,
            key_file: config.key_file.clone(),
            launch: None,
        })
        .map(|client| Self(client, AtomicBool::new(false)))
        .map_err(|_| "The Qwen TTS endpoint or protected credential file is unavailable.")
    }

    /// Verify the specific `VoiceDesign` model without generating or playing audio.
    pub(crate) fn ready(&self) -> Result<(), &'static str> {
        let current = AtomicU64::new(0);
        self.ready_for(0, &current)
            .map_err(|failure| match failure {
                Failure::Cancelled => "The Qwen TTS readiness check was cancelled.",
                Failure::Message(message) => message,
            })
    }

    fn ready_for(&self, generation: u64, current: &AtomicU64) -> Result<(), Failure> {
        let call = Call::new(generation, current, READY_LIMIT);
        let result = self.0.exchange("GET", "/health", None, &[], Media::Json, &call)
            .map_err(transport_failure)
            .and_then(|bytes| {
                call.check().map_err(transport_failure)?;
                let health: Health = serde_json::from_slice(&bytes).map_err(|_| {
                    Failure::Message("The Qwen TTS companion returned invalid capabilities.")
                })?;
                if health.model != MODEL || !health.ready || !health.voice_design
                    || health.sample_rate != 24000 || health.max_text_characters != MAX_TEXT
                    || health.max_voice_characters != MAX_VOICE
                {
                    return Err(Failure::Message("The Qwen TTS VoiceDesign model is not ready with the required audio format."));
                }
                if !health.consistent_voice {
                    return Err(Failure::Message("Update the Qwen TTS companion to support a consistent voice across paragraphs and messages."));
                }
                self.1.store(health.streaming_pcm, Ordering::Release);
                Ok(())
            });
        call.check().map_err(transport_failure).and(result)
    }

    /// Launch in an owned Job, then authenticate readiness before any text request.
    pub(crate) fn start_service(
        &self,
        config: &Config,
        generation: u64,
        current: &AtomicU64,
    ) -> Result<Option<Service>, Failure> {
        let call = Call::new(generation, current, Duration::from_mins(15));
        call.check().map_err(transport_failure)?;
        let Some(launch) = &config.launch else {
            self.ready_for(generation, current)?;
            return Ok(None);
        };
        if launch.codec.is_none() {
            return Err(Failure::Message(
                "Native Qwen TTS requires its speech codec.",
            ));
        }
        Service::start(
            launch,
            &config.key_file,
            config.port,
            generation,
            current,
            || self.ready_for(generation, current),
        )
        .map(Some)
    }
    /// Synthesize one clip; the worker sequences longer answers at word boundaries.
    #[cfg(test)]
    pub(crate) fn synthesize(
        &self,
        text: &str,
        voice: &str,
        generation: u64,
        current: &AtomicU64,
    ) -> Result<Vec<u8>, Failure> {
        let call = Call::new(generation, current, SYNTHESIS_LIMIT);
        let result = self.synthesize_clip(text, voice, &call);
        call.check().map_err(transport_failure).and(result)
    }

    /// Stream fixed PCM; companions without PCM support use validated WAV output.
    pub(crate) fn synthesize_pcm(
        &self,
        text: &str,
        voice: &str,
        generation: u64,
        current: &AtomicU64,
        cancelled: &AtomicBool,
        mut sink: impl FnMut(&[u8]) -> Result<(), Failure>,
    ) -> Result<(), Failure> {
        let call = Call::cancellable(generation, current, cancelled, SYNTHESIS_LIMIT);
        call.check().map_err(transport_failure)?;
        if !bounded_text(text, MAX_TEXT) || !bounded_text(voice, MAX_VOICE) {
            return Err(Failure::Message(
                "The speech text or voice description exceeded its limit.",
            ));
        }
        let result = if self.1.load(Ordering::Acquire) {
            let body = serde_json::to_vec(&SpeechRequest {
                model: MODEL,
                input: text,
                voice_description: voice,
                language: "Auto",
                response_format: "pcm",
            })
            .map_err(|_| Failure::Message("Could not prepare the Qwen TTS request."))?;
            self.0
                .stream_pcm(&body, &call, &mut sink)
                .map_err(transport_failure)
        } else {
            let wav = self.synthesize_clip(text, voice, &call)?;
            for chunk in wav[44..].chunks(16_384) {
                call.check().map_err(transport_failure)?;
                sink(chunk)?;
            }
            Ok(())
        };
        call.check().map_err(transport_failure).and(result)
    }

    fn synthesize_clip(
        &self,
        text: &str,
        voice: &str,
        call: &Call<'_>,
    ) -> Result<Vec<u8>, Failure> {
        call.check().map_err(transport_failure)?;
        if !bounded_text(text, MAX_TEXT) {
            return Err(Failure::Message(
                "Read-aloud clips require 1 through 2,048 plain-text characters.",
            ));
        }
        if !bounded_text(voice, MAX_VOICE) {
            return Err(Failure::Message(
                "Describe the voice using 1 through 1,024 plain-text characters.",
            ));
        }
        let body = serde_json::to_vec(&SpeechRequest {
            model: MODEL,
            input: text,
            voice_description: voice,
            language: "Auto",
            response_format: "wav",
        })
        .map_err(|_| Failure::Message("Could not prepare the Qwen TTS request."))?;
        call.check().map_err(transport_failure)?;
        let wav = self
            .0
            .exchange(
                "POST",
                "/v1/audio/speech",
                Some("application/json"),
                &body,
                Media::Wave,
                call,
            )
            .map_err(transport_failure)?;
        call.check().map_err(transport_failure)?;
        validate_wav(&wav)?;
        call.check().map_err(transport_failure)?;
        Ok(wav)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "The companion wire schema exposes independent readiness and capability flags."
)]
struct Health {
    model: String,
    ready: bool,
    voice_design: bool,
    sample_rate: u32,
    max_text_characters: usize,
    max_voice_characters: usize,
    #[serde(default)]
    streaming_pcm: bool,
    #[serde(default)]
    consistent_voice: bool,
}

#[derive(Serialize)]
struct SpeechRequest<'a> {
    model: &'static str,
    input: &'a str,
    voice_description: &'a str,
    language: &'static str,
    response_format: &'static str,
}

fn bounded_text(text: &str, limit: usize) -> bool {
    !text.trim().is_empty() && !text.contains('\0') && text.chars().take(limit + 1).count() <= limit
}

const fn transport_failure(failure: Failure) -> Failure {
    match failure {
        Failure::Cancelled => Failure::Cancelled,
        Failure::Message(_) => Failure::Message(
            "The local Qwen TTS request failed. Check the VoiceDesign companion and audio settings.",
        ),
    }
}

fn validate_wav(wav: &[u8]) -> Result<(), Failure> {
    let invalid = Failure::Message(
        "Qwen TTS must return a mono 24 kHz 16-bit PCM WAV clip of at most 120 seconds.",
    );
    bonsai_audio::mono_pcm16_data(wav, 24_000, 24_000 * 120)
        .map(|_| ())
        .ok_or(invalid)
}

#[cfg(test)]
mod tests {
    use super::{Client, Config, Failure, MODEL, bounded_text, validate_wav};
    use std::{
        fs,
        io::{self, Read, Write},
        net::TcpListener,
        sync::atomic::AtomicU64,
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    fn client(port: u16) -> io::Result<(bonsai_windows_credentials::PrivateCredentials, Client)> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let parent = std::env::temp_dir().join(format!("Bonsai-TTS-Protocol-{stamp}"));
        fs::create_dir(&parent)?;
        let credentials = bonsai_windows_credentials::PrivateCredentials::create(&parent)?;
        let client = Client::new(&Config {
            port,
            key_file: credentials.frontend_path().to_owned(),
            launch: None,
        })
        .map_err(io::Error::other)?;
        Ok((credentials, client))
    }

    fn wav() -> Vec<u8> {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&40_u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&24000_u32.to_le_bytes());
        bytes.extend_from_slice(&48000_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes
    }

    #[test]
    fn voice_design_health_and_synthesis_preserve_plain_voice_description() -> io::Result<()> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let (credentials, client) = client(listener.local_addr()?.port())?;
        let authorization = format!("Authorization: Bearer {}\r\n", credentials.frontend_key());
        let server = thread::spawn(move || -> io::Result<()> {
            for synthesis in [false, true] {
                let (mut stream, _) = listener.accept()?;
                stream.set_read_timeout(Some(Duration::from_secs(3)))?;
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte)?;
                    head.push(byte[0]);
                    assert!(head.len() <= 16384);
                }
                assert!(
                    head.windows(authorization.len())
                        .any(|bytes| bytes == authorization.as_bytes())
                );
                let body = if synthesis {
                    assert!(head.starts_with(b"POST /v1/audio/speech HTTP/1.1\r\n"));
                    let mut headers = [httparse::EMPTY_HEADER; 64];
                    let mut request = httparse::Request::new(&mut headers);
                    request.parse(&head).map_err(io::Error::other)?;
                    let length = request
                        .headers
                        .iter()
                        .find(|header| header.name.eq_ignore_ascii_case("content-length"))
                        .and_then(|header| std::str::from_utf8(header.value).ok())
                        .and_then(|value| value.parse::<usize>().ok())
                        .ok_or_else(|| io::Error::other("Missing upload length"))?;
                    assert!(length < 16384);
                    let mut body = vec![0; length];
                    stream.read_exact(&mut body)?;
                    let json: serde_json::Value =
                        serde_json::from_slice(&body).map_err(io::Error::other)?;
                    assert_eq!(json["model"], MODEL);
                    assert_eq!(json["input"], "Read this sample.");
                    assert_eq!(
                        json["voice_description"],
                        "<voice>Calm, clear, and warm.</voice>"
                    );
                    assert_eq!(json["language"], "Auto");
                    assert_eq!(json["response_format"], "wav");
                    assert_eq!(json.as_object().map(serde_json::Map::len), Some(5));
                    wav()
                } else {
                    assert!(head.starts_with(b"GET /health HTTP/1.1\r\n"));
                    br#"{"model":"Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign","ready":true,"voice_design":true,"sample_rate":24000,"max_text_characters":2048,"max_voice_characters":1024,"consistent_voice":true}"#.to_vec()
                };
                let media = if synthesis {
                    "audio/wav"
                } else {
                    "application/json"
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {media}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(response.as_bytes())?;
                stream.write_all(&body)?;
            }
            Ok(())
        });
        client.ready().map_err(io::Error::other)?;
        let result = client
            .synthesize(
                "Read this sample.",
                "<voice>Calm, clear, and warm.</voice>",
                1,
                &AtomicU64::new(1),
            )
            .map_err(|_| io::Error::other("Synthesis protocol failed"))?;
        assert_eq!(result, wav());
        server
            .join()
            .map_err(|_| io::Error::other("Fake TTS companion failed"))??;
        Ok(())
    }

    #[test]
    fn readiness_requires_reusable_speaker_conditioning() -> io::Result<()> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let (_credentials, client) = client(listener.local_addr()?.port())?;
        let server = thread::spawn(move || -> io::Result<()> {
            for consistent in [None, Some(false), Some(true)] {
                let (mut stream, _) = listener.accept()?;
                stream.set_read_timeout(Some(Duration::from_secs(3)))?;
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte)?;
                    head.push(byte[0]);
                    assert!(head.len() <= 16384);
                }
                assert!(head.starts_with(b"GET /health HTTP/1.1\r\n"));
                let mut health = serde_json::json!({
                    "model": MODEL, "ready": true, "voice_design": true,
                    "sample_rate": 24000, "max_text_characters": 2048,
                    "max_voice_characters": 1024,
                });
                if let Some(consistent) = consistent {
                    health["consistent_voice"] = serde_json::Value::Bool(consistent);
                }
                let body = serde_json::to_vec(&health)?;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )?;
                stream.write_all(&body)?;
            }
            Ok(())
        });
        for _ in 0..2 {
            let error = client
                .ready()
                .err()
                .ok_or_else(|| io::Error::other("Legacy random voices must be rejected"))?;
            assert!(error.contains("consistent voice"));
        }
        client.ready().map_err(io::Error::other)?;
        server
            .join()
            .map_err(|_| io::Error::other("Fake TTS companion failed"))??;
        Ok(())
    }

    #[test]
    fn text_voice_and_audio_bounds_reject_bad_clips_before_playback() {
        assert!(bounded_text(&"🙂".repeat(2048), 2048));
        assert!(!bounded_text(&"a".repeat(2049), 2048));
        assert!(!bounded_text("text\0hidden", 2048));
        assert!(!bounded_text(" ", 1024));
        assert!(!bounded_text(&"a".repeat(1025), 1024));
        assert!(validate_wav(&wav()).is_ok());
        for offset in [4, 16, 20, 22, 24, 28, 32, 34, 40] {
            let mut invalid = wav();
            invalid[offset] ^= 1;
            assert!(validate_wav(&invalid).is_err());
        }
    }

    #[test]
    fn pcm_adapter_delivers_audio_before_the_success_terminal() -> io::Result<()> {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let (_credentials, client) = client(listener.local_addr()?.port())?;
        client.1.store(true, Ordering::Release);
        let (delivered, observed) = mpsc::channel();
        let server = thread::spawn(move || -> io::Result<()> {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(Duration::from_secs(3)))?;
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte)?;
                head.push(byte[0]);
                assert!(head.len() <= 16384);
            }
            let text = std::str::from_utf8(&head).map_err(io::Error::other)?;
            let length: usize = text
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .ok_or_else(|| io::Error::other("Missing upload length"))?
                .trim()
                .parse()
                .map_err(io::Error::other)?;
            let mut body = vec![0; length];
            stream.read_exact(&mut body)?;
            let request: serde_json::Value =
                serde_json::from_slice(&body).map_err(io::Error::other)?;
            assert_eq!(request["response_format"], "pcm");
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2\r\n\x01\x00\r\n")?;
            observed
                .recv_timeout(Duration::from_secs(3))
                .map_err(io::Error::other)?;
            stream.write_all(b"0\r\n\r\n")?;
            Ok(())
        });
        let mut pcm = Vec::new();
        client
            .synthesize_pcm(
                "Hello.",
                "Calm",
                3,
                &AtomicU64::new(3),
                &AtomicBool::new(false),
                |bytes| {
                    pcm.extend_from_slice(bytes);
                    delivered.send(()).map_err(|_| Failure::Cancelled)
                },
            )
            .map_err(|_| io::Error::other("Streaming adapter failed"))?;
        assert_eq!(pcm, [1, 0]);
        server
            .join()
            .map_err(|_| io::Error::other("PCM server failed"))??;
        Ok(())
    }

    #[test]
    fn cancelled_generation_never_contacts_the_tts_service() -> io::Result<()> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let (_credentials, client) = client(listener.local_addr()?.port())?;
        assert_eq!(
            client.synthesize("Read this.", "Warm voice", 1, &AtomicU64::new(2)),
            Err(Failure::Cancelled)
        );
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock)
        );
        Ok(())
    }
}
