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
use bonsai_windows_process::{
    OwnedProcess, ProcessSpec, available_memory_bytes, listener_process_id,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs, io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MODEL: &str = "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign";
const MAX_TEXT: usize = 2048;
const MAX_VOICE: usize = 1024;
const MAX_AUDIO: usize = 5_760_044;
const SYNTHESIS_LIMIT: Duration = Duration::from_secs(180);
const READY_LIMIT: Duration = Duration::from_secs(5);
const GIB: u64 = 1024 * 1024 * 1024;
const START_AVAILABLE: u64 = 9 * GIB;
const RUNNING_RESERVE: u64 = 5 * GIB;
// PyTorch's startup private commitment can exceed its resident weight size.
// The separate physical-memory monitor protects the shared host reserve.
const SERVICE_COMMIT_LIMIT: usize = 16 * 1024 * 1024 * 1024;
const RESOURCE_POLL: Duration = Duration::from_millis(25);
const RESOURCE_STOP: Duration = Duration::from_secs(5);
const LOW_MEMORY: u8 = 1;
const MEMORY_UNAVAILABLE: u8 = 2;
const MEMORY_STOPPED: &str = "Read aloud stopped because Qwen was using the memory needed to keep text chat running. Close other apps and try Read aloud again; the completed answer is retained.";

/// Explicit local endpoint; no voice text or credential bytes have Debug output.
#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) port: u16,
    pub(crate) key_file: PathBuf,
    pub(crate) launch: Option<Launch>,
}

/// Reviewed local service inputs; the process starts only for explicit read-aloud.
#[derive(Clone)]
pub(crate) struct Launch {
    pub(crate) python: PathBuf,
    pub(crate) service: PathBuf,
    pub(crate) snapshot: PathBuf,
    pub(crate) evidence: PathBuf,
}

/// Retaining this owner bounds the server and its model-worker descendants.
pub(crate) struct Service {
    process: Arc<OwnedProcess>,
    resource_status: Arc<AtomicU8>,
    stop_monitor: Sender<()>,
    monitor: Option<JoinHandle<()>>,
}

impl Service {
    fn new(process: OwnedProcess) -> Result<Self, Failure> {
        Self::monitored(process, available_memory_bytes)
    }

    fn monitored(
        process: OwnedProcess,
        available: impl FnMut() -> io::Result<u64> + Send + 'static,
    ) -> Result<Self, Failure> {
        let process = Arc::new(process);
        let resource_status = Arc::new(AtomicU8::new(0));
        let (stop_monitor, stopped) = mpsc::channel();
        let monitored_process = Arc::clone(&process);
        let status = Arc::clone(&resource_status);
        let monitor = thread::Builder::new()
            .name("bonsai-tts-memory".to_owned())
            .spawn(move || monitor_resources(&monitored_process, &status, &stopped, available))
            .map_err(|_| {
                let _stopped = process.stop(RESOURCE_STOP);
                Failure::Message(
                    "Could not start Qwen TTS memory monitoring. Text chat remains available.",
                )
            })?;
        Ok(Self {
            process,
            resource_status,
            stop_monitor,
            monitor: Some(monitor),
        })
    }

    /// The independent monitor keeps running while model HTTP calls block.
    pub(crate) fn check(&self) -> Result<(), Failure> {
        match self.resource_status.load(Ordering::Acquire) {
            LOW_MEMORY => return Err(Failure::Message(MEMORY_STOPPED)),
            MEMORY_UNAVAILABLE => {
                return Err(Failure::Message(
                    "Read aloud stopped because available memory could not be checked. Text chat remains available.",
                ));
            }
            _ => {}
        }
        if self
            .process
            .try_wait()
            .map_err(|_| Failure::Message("Could not inspect the Qwen TTS process."))?
            .is_some()
        {
            if self
                .process
                .peak_job_memory_bytes()
                .is_ok_and(|peak| peak >= SERVICE_COMMIT_LIMIT)
            {
                return Err(Failure::Message(
                    "Read aloud stopped because Qwen reached its speech memory limit. Text chat and the completed answer remain available.",
                ));
            }
            return Err(Failure::Message(
                "Qwen TTS could not keep its model running. Check its retained service evidence; text chat remains available.",
            ));
        }
        Ok(())
    }
}

fn monitor_resources(
    process: &OwnedProcess,
    status: &AtomicU8,
    stopped: &Receiver<()>,
    mut available: impl FnMut() -> io::Result<u64>,
) {
    loop {
        let failure = match available() {
            Ok(bytes) if bytes >= RUNNING_RESERVE => 0,
            Ok(_) => LOW_MEMORY,
            Err(_) => MEMORY_UNAVAILABLE,
        };
        if failure != 0 {
            status.store(failure, Ordering::Release);
            // Stop only the speech Job, leaving the independent Swift owner.
            let _stopped = process.stop(RESOURCE_STOP);
            return;
        }
        match stopped.recv_timeout(RESOURCE_POLL) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

const fn admit_memory(available: u64) -> Result<(), Failure> {
    if available < START_AVAILABLE {
        return Err(Failure::Message(
            "Read aloud needs 9 GiB of available memory before loading Qwen alongside Swift. Close other apps and try again; text chat and the completed answer are retained.",
        ));
    }
    Ok(())
}

impl Drop for Service {
    fn drop(&mut self) {
        // Closing the noninheritable Job also kills descendants if cleanup fails
        // or this application terminates before the explicit bounded stop.
        let _stopped = self.process.stop(RESOURCE_STOP);
        let _sent = self.stop_monitor.send(());
        if let Some(monitor) = self.monitor.take() {
            let _joined = monitor.join();
        }
    }
}

/// Uses the same bounded transport policy with its own configured key and port.
pub(crate) struct Client(canary::Client);

impl Client {
    pub(crate) fn new(config: &Config) -> Result<Self, &'static str> {
        canary::Client::new(&canary::Config {
            port: config.port,
            key_file: config.key_file.clone(),
        })
        .map(Self)
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
        if listener_process_id(config.port)
            .map_err(|_| Failure::Message("Could not check the local Qwen TTS port."))?
            .is_some()
        {
            return Err(Failure::Message(
                "The Qwen TTS port is already occupied. Close the conflicting service or choose a distinct TTS port.",
            ));
        }
        admit_memory(available_memory_bytes().map_err(|_| {
            Failure::Message("Could not check available memory before loading Qwen. Text chat remains available.")
        })?)?;
        let process = Service::new(OwnedProcess::start(&service_specification(launch, config)?)
            .map_err(|_| Failure::Message("Could not start the local Qwen TTS runtime. Check its installed environment and retained service evidence."))?)?;
        loop {
            call.check().map_err(transport_failure)?;
            process.check()?;
            match self.ready_for(generation, current) {
                Ok(()) => {
                    if listener_process_id(config.port).ok().flatten() != Some(process.process.id())
                    {
                        return Err(Failure::Message(
                            "The local Qwen TTS listener does not belong to the owned service.",
                        ));
                    }
                    call.check().map_err(transport_failure)?;
                    process.check()?;
                    return Ok(Some(process));
                }
                Err(Failure::Cancelled) => return Err(Failure::Cancelled),
                Err(Failure::Message(_)) => thread::sleep(Duration::from_millis(100)),
            }
        }
    }

    /// Synthesize one clip; the worker sequences longer answers at word boundaries.
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

fn service_specification(launch: &Launch, config: &Config) -> Result<ProcessSpec, Failure> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Failure::Message("Could not create the Qwen TTS evidence lane."))?
        .as_nanos();
    let directory = launch
        .evidence
        .join(format!("qwen-tts-{}-{stamp}", std::process::id()));
    fs::create_dir(&directory)
        .map_err(|_| Failure::Message("Could not create the Qwen TTS evidence lane."))?;
    Ok(ProcessSpec {
        executable: launch.python.clone(),
        arguments: vec![
            OsString::from("-I"),
            OsString::from("-B"),
            launch.service.clone().into_os_string(),
            OsString::from("--snapshot"),
            launch.snapshot.clone().into_os_string(),
            OsString::from("--token-file"),
            config.key_file.clone().into_os_string(),
            OsString::from("--port"),
            OsString::from(config.port.to_string()),
            OsString::from("--device"),
            OsString::from("cpu"),
            OsString::from("--precision"),
            OsString::from("bfloat16"),
        ],
        working_directory: directory.clone(),
        stdout: directory.join("service.stdout"),
        stderr: directory.join("service.stderr"),
        memory_limit_bytes: SERVICE_COMMIT_LIMIT,
        active_process_limit: 4,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Health {
    model: String,
    ready: bool,
    voice_design: bool,
    sample_rate: u32,
    max_text_characters: usize,
    max_voice_characters: usize,
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
    if wav.len() < 46 || wav.len() > MAX_AUDIO {
        return Err(invalid);
    }
    let u16_at = |at| u16::from_le_bytes([wav[at], wav[at + 1]]);
    let u32_at = |at| u32::from_le_bytes([wav[at], wav[at + 1], wav[at + 2], wav[at + 3]]);
    if &wav[..4] != b"RIFF"
        || usize::try_from(u32_at(4)).ok() != Some(wav.len() - 8)
        || &wav[8..16] != b"WAVEfmt "
        || u32_at(16) != 16
        || u16_at(20) != 1
        || u16_at(22) != 1
        || u32_at(24) != 24000
        || u32_at(28) != 48000
        || u16_at(32) != 2
        || u16_at(34) != 16
        || &wav[36..40] != b"data"
        || u32_at(40) == 0
        || u32_at(40) % 2 != 0
        || usize::try_from(u32_at(40)).ok() != Some(wav.len() - 44)
    {
        return Err(invalid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Client, Config, Failure, GIB, MODEL, RUNNING_RESERVE, START_AVAILABLE, Service,
        admit_memory, bounded_text, validate_wav,
    };
    use bonsai_windows_process::{OwnedProcess, ProcessSpec};
    use std::{
        ffi::OsString,
        fs,
        io::{self, Read, Write},
        net::TcpListener,
        sync::atomic::AtomicU64,
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn speech_memory_admission_preserves_room_for_the_running_chat_model() {
        assert!(admit_memory(START_AVAILABLE).is_ok());
        assert!(admit_memory(START_AVAILABLE - 1).is_err());
        assert!(admit_memory(RUNNING_RESERVE).is_err());
        assert!(admit_memory(4 * GIB).is_err());
    }

    #[test]
    #[ignore = "Spawned only by the resource monitor regression as a disposable child."]
    fn resource_guard_fake_service() {
        thread::sleep(Duration::from_secs(30));
    }

    #[test]
    fn speech_memory_monitor_stops_only_its_owned_service() -> io::Result<()> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-TTS-Memory-{stamp}"));
        fs::create_dir(&directory)?;
        let start = |name: &str| {
            OwnedProcess::start(&ProcessSpec {
                executable: std::env::current_exe()?,
                arguments: vec![
                    OsString::from("--exact"),
                    OsString::from("tts::tests::resource_guard_fake_service"),
                    OsString::from("--ignored"),
                ],
                working_directory: directory.clone(),
                stdout: directory.join(format!("{name}.stdout")),
                stderr: directory.join(format!("{name}.stderr")),
                memory_limit_bytes: 512 * 1024 * 1024,
                active_process_limit: 1,
            })
        };
        let unrelated = start("independent-chat-sentinel")?;
        let speech = Service::monitored(start("speech")?, || Ok(RUNNING_RESERVE - 1))
            .map_err(|_| io::Error::other("Could not start the test memory monitor"))?;
        assert!(
            speech
                .process
                .wait_for_exit(Duration::from_secs(5))?
                .is_some(),
            "Speech memory pressure did not stop the owned child"
        );
        assert!(
            matches!(speech.check(), Err(Failure::Message(message)) if message.contains("memory") && message.contains("text chat"))
        );
        assert!(
            unrelated.try_wait()?.is_none(),
            "Speech pressure stopped an independent Job"
        );
        drop(speech);
        unrelated.stop(Duration::from_secs(5))?;
        Ok(())
    }

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
                    br#"{"model":"Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign","ready":true,"voice_design":true,"sample_rate":24000,"max_text_characters":2048,"max_voice_characters":1024}"#.to_vec()
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
