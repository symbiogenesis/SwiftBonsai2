//! Synthetic protocol checks; no model, microphone or playback is activated.

use crate::{Audio, Engine, Kind, Output, Request, http, request, wave};
use std::{
    fmt::Write as _,
    io::{self, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

const KEY: [u8; 64] = [b'a'; 64];

struct Fake {
    kind: Kind,
    calls: Arc<AtomicUsize>,
    transcript: String,
}
impl Engine for Fake {
    fn kind(&self) -> Kind {
        self.kind
    }
    fn infer(&mut self, request: Request<'_>) -> io::Result<Output> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        match request {
            Request::Transcribe {
                samples,
                sample_rate,
            } => {
                assert_eq!(sample_rate, 16_000);
                assert_eq!(samples.len(), 1);
                Ok(Output::Transcript(self.transcript.clone()))
            }
            Request::Synthesize {
                text,
                voice_description,
                language,
            } => {
                assert_eq!(text, "Hello.");
                assert_eq!(voice_description, "Warm and clear.");
                assert_eq!(language, "Auto");
                Ok(Output::Audio(Audio {
                    samples: vec![-1.0, 0.0, 1.0],
                    sample_rate: 24_000,
                }))
            }
        }
    }
}

struct StreamFake {
    failure: bool,
    invalid: bool,
}

impl Engine for StreamFake {
    fn kind(&self) -> Kind {
        Kind::QwenTts
    }
    fn infer(&mut self, _: Request<'_>) -> io::Result<Output> {
        Err(io::Error::other("Streaming fixture requires PCM."))
    }
    fn streaming_pcm(&self) -> bool {
        true
    }
    fn infer_stream(
        &mut self,
        _: Request<'_>,
        sink: &mut (dyn FnMut(&[f32]) -> io::Result<()> + Send),
    ) -> io::Result<()> {
        sink(&[-1.0, 0.0, 1.0])?;
        if self.invalid {
            // Even an engine ignoring a rejected chunk cannot frame success.
            let _rejected = sink(&[f32::INFINITY]);
            return Ok(());
        }
        if self.failure {
            return Err(io::Error::other("Synthetic truncated generation."));
        }
        sink(&[0.5])
    }
}

#[test]
fn streaming_pcm_frames_complete_output_and_leaves_failures_incomplete() -> io::Result<()> {
    for (failure, invalid) in [(false, false), (true, false), (false, true)] {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let server = thread::spawn(move || -> io::Result<()> {
            let (mut peer, _) = listener.accept()?;
            http::handle(&mut peer, port, &KEY, &mut StreamFake { failure, invalid });
            Ok(())
        });
        let body = String::from_utf8(speech())
            .map_err(io::Error::other)?
            .replace("\"wav\"", "\"pcm\"");
        let mut peer = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
        peer.set_read_timeout(Some(Duration::from_secs(3)))?;
        let key = std::str::from_utf8(&KEY).map_err(io::Error::other)?;
        write!(
            peer,
            "POST /v1/audio/speech HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )?;
        let mut response = Vec::new();
        let read_result = peer.read_to_end(&mut response);
        server
            .join()
            .map_err(|_| io::Error::other("Synthetic streaming server failed."))??;
        match read_result {
            Ok(_) => {}
            // Deliberately incomplete responses may close with RST on Windows.
            // Successful synthesis must still provide a clean, complete stream.
            Err(error)
                if (failure || invalid) && error.kind() == io::ErrorKind::ConnectionReset => {}
            Err(error) => return Err(error),
        }
        assert!(response.starts_with(b"HTTP/1.1 200"));
        assert!(
            response
                .windows(b"Transfer-Encoding: chunked".len())
                .any(|part| part == b"Transfer-Encoding: chunked")
        );
        assert!(
            response
                .windows(b"Content-Type: audio/pcm".len())
                .any(|part| part == b"Content-Type: audio/pcm")
        );
        assert_eq!(response.ends_with(b"0\r\n\r\n"), !failure && !invalid);
        assert_eq!(
            response
                .windows(b"HTTP/1.1".len())
                .filter(|part| *part == b"HTTP/1.1")
                .count(),
            1
        );
        let boundary = response
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .ok_or_else(|| io::Error::other("Missing streaming response headers."))?;
        assert!(response[boundary + 4..].starts_with(b"6\r\n\x01\x80\x00\x00\xff\x7f\r\n"));
        if !failure && !invalid {
            assert_eq!(
                &response[boundary + 4..],
                b"6\r\n\x01\x80\x00\x00\xff\x7f\r\n2\r\n\x00\x40\r\n0\r\n\r\n"
            );
        }
    }
    Ok(())
}

fn exchange(
    kind: Kind,
    route: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<(Vec<u8>, usize)> {
    exchange_transcript(kind, route, headers, body, "recognized words".to_owned())
}

fn exchange_transcript(
    kind: Kind,
    route: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    transcript: String,
) -> io::Result<(Vec<u8>, usize)> {
    let calls = Arc::new(AtomicUsize::new(0));
    let response = exchange_engine(
        route,
        headers,
        body,
        Fake {
            kind,
            calls: Arc::clone(&calls),
            transcript,
        },
    )?;
    Ok((response, calls.load(Ordering::Acquire)))
}

fn exchange_engine(
    route: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    mut engine: impl Engine + Send + 'static,
) -> io::Result<Vec<u8>> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    let server = thread::spawn(move || -> io::Result<()> {
        let (mut peer, _) = listener.accept()?;
        http::handle(&mut peer, port, &KEY, &mut engine);
        Ok(())
    });
    let mut peer = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
    peer.set_read_timeout(Some(Duration::from_secs(3)))?;
    let auth = String::from_utf8(KEY.to_vec()).map_err(io::Error::other)?;
    let mut head = format!(
        "{route} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {auth}\r\nConnection: close\r\n"
    );
    for (name, value) in headers {
        write!(head, "{name}: {value}\r\n").map_err(io::Error::other)?;
    }
    head.push_str("\r\n");
    peer.write_all(head.as_bytes())?;
    peer.write_all(body)?;
    let mut response = Vec::new();
    peer.read_to_end(&mut response)?;
    server
        .join()
        .map_err(|_| io::Error::other("Fake speech server failed"))??;
    Ok(response)
}

fn speech() -> Vec<u8> {
    br#"{"model":"Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign","input":"Hello.","voice_description":"Warm and clear.","language":"Auto","response_format":"wav"}"#.to_vec()
}

fn input_wav() -> Vec<u8> {
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&38_u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&16_000_u32.to_le_bytes());
    bytes.extend_from_slice(&32_000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&100_i16.to_le_bytes());
    bytes
}

#[test]
fn native_http_preserves_exact_health_and_voice_design_audio() -> io::Result<()> {
    for kind in [Kind::Canary, Kind::QwenTts] {
        let (response, calls) = exchange(kind, "GET /health", &[], &[])?;
        assert!(response.starts_with(b"HTTP/1.1 200"));
        let boundary = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .ok_or_else(|| io::Error::other("Missing response head"))?;
        let health: serde_json::Value = serde_json::from_slice(&response[boundary + 4..])?;
        assert_eq!(health["model"], kind.model());
        assert_eq!(health["ready"], true);
        assert_eq!(calls, 0);
        assert_eq!(
            health.as_object().map(serde_json::Map::len),
            Some(if kind == Kind::QwenTts { 7 } else { 6 })
        );
    }
    let body = speech();
    let (response, calls) = exchange(
        Kind::QwenTts,
        "POST /v1/audio/speech",
        &[
            ("Content-Type", "application/json"),
            ("Content-Length", &body.len().to_string()),
        ],
        &body,
    )?;
    assert_eq!(calls, 1);
    assert!(response.starts_with(b"HTTP/1.1 200"));
    let boundary = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .ok_or_else(|| io::Error::other("Missing response head"))?;
    let audio = &response[boundary + 4..];
    assert_eq!(&audio[..4], b"RIFF");
    assert_eq!(&audio[24..28], &24_000_u32.to_le_bytes());
    assert_eq!(audio.len(), 50);
    Ok(())
}

#[test]
fn native_multipart_transcription_maps_pcm_without_disk_files() -> io::Result<()> {
    let body = multipart_audio();
    let (response, calls) = exchange(
        Kind::Canary,
        "POST /v1/audio/transcriptions",
        &[
            ("Content-Type", "multipart/form-data; boundary=fixture"),
            ("Content-Length", &body.len().to_string()),
        ],
        &body,
    )?;
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert_eq!(calls, 1);
    assert!(
        response
            .windows(b"recognized words".len())
            .any(|bytes| bytes == b"recognized words")
    );
    Ok(())
}

fn multipart_audio() -> Vec<u8> {
    let mut body = format!("--fixture\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\n{}\r\n--fixture\r\nContent-Disposition: form-data; name=\"file\"; filename=\"speech.wav\"\r\nContent-Type: audio/wav\r\n\r\n", crate::CANARY_MODEL).into_bytes();
    body.extend_from_slice(&input_wav());
    body.extend_from_slice(b"\r\n--fixture--\r\n");
    body
}

#[test]
fn escaped_transcripts_cannot_exceed_the_clients_serialized_response_limit() -> io::Result<()> {
    let body = multipart_audio();
    let (response, calls) = exchange_transcript(
        Kind::Canary,
        "POST /v1/audio/transcriptions",
        &[
            ("Content-Type", "multipart/form-data; boundary=fixture"),
            ("Content-Length", &body.len().to_string()),
        ],
        &body,
        "\"".repeat(32768),
    )?;
    assert!(response.starts_with(b"HTTP/1.1 502"));
    assert_eq!(calls, 1);
    assert!(response.len() < 65536);
    assert!(
        response
            .windows(b"model_failure".len())
            .any(|bytes| bytes == b"model_failure")
    );
    Ok(())
}

#[test]
fn malformed_framing_and_browser_headers_never_reach_the_engine() -> io::Result<()> {
    for headers in [
        vec![("Origin", "http://example.invalid")],
        vec![("Host", "127.0.0.1:1")],
        vec![("Transfer-Encoding", "chunked")],
        vec![("Authorization", "Bearer invalid")],
        vec![("Content-Length", "1"), ("Content-Length", "1")],
    ] {
        let (response, calls) = exchange(Kind::QwenTts, "POST /v1/audio/speech", &headers, &[])?;
        assert!(!response.starts_with(b"HTTP/1.1 200"));
        assert_eq!(calls, 0);
    }
    Ok(())
}

#[test]
fn strict_json_and_wave_validation_rejects_untrusted_payloads() {
    for bytes in [
        b"null".as_slice(),
        br#"{"model":"x","model":"y"}"#,
        br#"{"input":"\ud800"}"#,
        &[0xff],
    ] {
        assert!(request::body(Kind::QwenTts, "application/json", bytes).is_err());
    }
    let valid = input_wav();
    assert!(wave::decode(&valid).is_ok());
    for offset in [4, 16, 20, 22, 24, 28, 32, 34, 40] {
        let mut bytes = valid.clone();
        bytes[offset] ^= 1;
        assert!(wave::decode(&bytes).is_err());
    }
    assert!(
        wave::header(&Audio {
            samples: vec![f32::NAN],
            sample_rate: 24_000
        })
        .is_err()
    );
    assert!(
        wave::header(&Audio {
            samples: vec![0.0],
            sample_rate: 16_000
        })
        .is_err()
    );
}

#[test]
fn native_voice_design_rejects_language_overrides_before_inference() -> io::Result<()> {
    let mut value: serde_json::Value = serde_json::from_slice(&speech())?;
    value["language"] = "English".into();
    let body = serde_json::to_vec(&value)?;
    assert_eq!(
        request::body(Kind::QwenTts, "application/json", &body).err(),
        Some(http::Error::bad("unsupported_language"))
    );
    Ok(())
}

#[test]
#[ignore = "Only spawned by the watchdog regression as an isolated disposable companion."]
fn watchdog_child() -> io::Result<()> {
    let port = std::env::var("BONSAI_SPEECH_TEST_PORT")
        .map_err(io::Error::other)?
        .parse::<u16>()
        .map_err(io::Error::other)?;
    let peer = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
    let limit = if std::env::var("BONSAI_SPEECH_TEST_DISCONNECT").is_ok() {
        Duration::from_secs(10)
    } else {
        Duration::from_millis(100)
    };
    let _guard = crate::watchdog::Watchdog::start(&peer, limit)?;
    thread::sleep(Duration::from_secs(15));
    Ok(())
}

#[test]
fn watchdog_terminates_only_its_companion_on_disconnect_extra_data_and_deadline() -> io::Result<()>
{
    use std::{
        process::{Command, Stdio},
        time::{Instant, SystemTime, UNIX_EPOCH},
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("Bonsai-SpeechWatchdog-{stamp}"));
    std::fs::create_dir(&directory)?;
    for mode in ["disconnect", "extra", "deadline"] {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args(["--exact", "tests::watchdog_child", "--ignored"])
            .env(
                "BONSAI_SPEECH_TEST_PORT",
                listener.local_addr()?.port().to_string(),
            )
            .current_dir(&directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if mode == "deadline" {
            command.env_remove("BONSAI_SPEECH_TEST_DISCONNECT");
        } else {
            command.env("BONSAI_SPEECH_TEST_DISCONNECT", "1");
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn()?;
        listener.set_nonblocking(true)?;
        let until = Instant::now() + Duration::from_secs(5);
        let mut peer = loop {
            match listener.accept() {
                Ok((peer, _)) => break peer,
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < until =>
                {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => {
                    let _killed = child.kill();
                    let _waited = child.wait();
                    return Err(error);
                }
            }
        };
        if mode == "extra" {
            peer.write_all(b"queued-data")?;
        }
        let _kept_open = if mode == "disconnect" {
            drop(peer);
            None
        } else {
            Some(peer)
        };
        loop {
            if let Some(status) = child.try_wait()? {
                assert_eq!(status.code(), Some(70));
                break;
            }
            if Instant::now() >= until {
                child.kill()?;
                let _waited = child.wait()?;
                return Err(io::Error::other(
                    "Companion watchdog failed to terminate inference",
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn protected_credential_keeps_a_read_handle_that_denies_writes() -> io::Result<()> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let parent = std::env::temp_dir().join(format!("Bonsai-SpeechCredential-{stamp}"));
    std::fs::create_dir(&parent)?;
    let keys = bonsai_windows_credentials::PrivateCredentials::create(&parent)?;
    let path = keys.frontend_path().to_owned();
    let retained = crate::credentials::Credential::open(&path)?;
    drop(keys);
    assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err());
    drop(retained);
    assert!(std::fs::OpenOptions::new().write(true).open(&path).is_ok());
    Ok(())
}

struct AudioFake(Option<Audio>);

impl Engine for AudioFake {
    fn kind(&self) -> Kind {
        Kind::QwenTts
    }

    fn infer(&mut self, _: Request<'_>) -> io::Result<Output> {
        self.0
            .take()
            .map(Output::Audio)
            .ok_or_else(|| io::Error::other("Synthetic audio already taken."))
    }
}

#[test]
fn wave_response_validates_before_headers_and_preserves_pcm_bytes() -> io::Result<()> {
    let body = speech();
    for audio in [
        Audio {
            samples: vec![0.0, f32::NAN],
            sample_rate: 24_000,
        },
        Audio {
            samples: vec![0.0],
            sample_rate: 16_000,
        },
        Audio {
            samples: vec![],
            sample_rate: 24_000,
        },
        Audio {
            samples: vec![-2.0, -1.0, -0.5, -0.0, 0.0, 0.5, 1.0, 2.0],
            sample_rate: 24_000,
        },
    ] {
        let valid = audio.samples.len() == 8;
        let response = exchange_engine(
            "POST /v1/audio/speech",
            &[
                ("Content-Type", "application/json"),
                ("Content-Length", &body.len().to_string()),
            ],
            &body,
            AudioFake(Some(audio)),
        )?;
        assert_eq!(
            response
                .windows(8)
                .filter(|bytes| *bytes == b"HTTP/1.1")
                .count(),
            1
        );
        if !valid {
            assert!(response.starts_with(b"HTTP/1.1 502"));
            assert!(
                response
                    .windows(20)
                    .any(|bytes| bytes == b"invalid_audio_output")
            );
            continue;
        }
        assert!(response.starts_with(b"HTTP/1.1 200"));
        assert!(
            response
                .windows(20)
                .any(|bytes| bytes == b"Content-Length: 60\r\n")
        );
        let boundary = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .ok_or_else(|| io::Error::other("Missing WAV response headers."))?;
        let wave = &response[boundary + 4..];
        assert_eq!(wave.len(), 60);
        assert_eq!(&wave[..4], b"RIFF");
        assert_eq!(
            &wave[44..],
            &[
                1, 128, 1, 128, 0, 192, 0, 0, 0, 0, 0, 64, 255, 127, 255, 127
            ]
        );
    }
    Ok(())
}
