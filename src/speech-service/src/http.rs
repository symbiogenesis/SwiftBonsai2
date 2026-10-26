//! Single-request strict HTTP framing and value-free responses.

use crate::{
    Engine, Kind, Output,
    request::{self, Prepared},
    watchdog::Watchdog,
    wave,
};
use std::{
    collections::HashMap,
    io::{self, Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq as _;

const MAX_HEAD: usize = 16 * 1024;
const INTAKE: Duration = Duration::from_secs(10);
const WRITE: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Error {
    status: u16,
    code: &'static str,
}

impl Error {
    pub(super) const fn new(status: u16, code: &'static str) -> Self {
        Self { status, code }
    }
    pub(super) const fn bad(code: &'static str) -> Self {
        Self::new(400, code)
    }
}

struct Head {
    health: bool,
    length: usize,
    media: String,
}

pub(super) fn handle(peer: &mut TcpStream, port: u16, key: &[u8; 64], engine: &mut impl Engine) {
    let result = receive(peer, port, key, engine.kind())
        .and_then(|prepared| respond(engine, &prepared, peer));
    if let Err(error) = result {
        if error.status == 0 {
            return;
        }
        let body = serde_json::json!({"error": {"code": error.code}}).to_string();
        let _written = send(peer, error.status, "application/json", body.as_bytes());
    }
}

fn respond(
    engine: &mut impl Engine,
    prepared: &Prepared,
    peer: &mut TcpStream,
) -> Result<(), Error> {
    let kind = engine.kind();
    let Some(input) = prepared.engine_request() else {
        let value = match kind {
            Kind::Canary => {
                serde_json::json!({"model":kind.model(),"ready":true,"sample_rate":16000,"max_clip_seconds":30,"input_format":"pcm_s16le_wav","channels":1})
            }
            Kind::QwenTts => {
                serde_json::json!({"model":kind.model(),"ready":true,"voice_design":true,"sample_rate":24000,"max_text_characters":2048,"max_voice_characters":1024,"streaming_pcm":engine.streaming_pcm()})
            }
        };
        return send(peer, 200, "application/json", value.to_string().as_bytes())
            .map_err(|_| Error::new(0, "response_failed"));
    };
    if prepared.streaming_pcm() {
        if !engine.streaming_pcm() {
            return Err(Error::bad("unsupported_audio_format"));
        }
        return stream(engine, input, peer);
    }
    let limit = match kind {
        Kind::Canary => Duration::from_secs(120),
        Kind::QwenTts => Duration::from_secs(180),
    };
    let watchdog =
        Watchdog::start(peer, limit).map_err(|_| Error::new(503, "supervision_unavailable"))?;
    let result = engine.infer(input);
    drop(watchdog);
    match (kind, result) {
        (Kind::Canary, Ok(Output::Transcript(text)))
            if text.len() <= 32768 && !text.contains('\0') =>
        {
            let body = serde_json::json!({"text":text,"model":kind.model()}).to_string();
            if body.len() > 65536 {
                return Err(Error::new(502, "model_failure"));
            }
            send(peer, 200, "application/json", body.as_bytes())
        }
        (Kind::QwenTts, Ok(Output::Audio(audio))) => {
            let bytes = wave::encode(&audio)?;
            send(peer, 200, "audio/wav", &bytes)
        }
        _ => return Err(Error::new(502, "model_failure")),
    }
    .map_err(|_| Error::new(0, "response_failed"))
}

fn stream(
    engine: &mut impl Engine,
    request: crate::Request<'_>,
    peer: &mut TcpStream,
) -> Result<(), Error> {
    let _watchdog = Watchdog::start(peer, Duration::from_secs(180))
        .map_err(|_| Error::new(503, "supervision_unavailable"))?;
    peer.set_write_timeout(Some(WRITE))
        .and_then(|()| peer.write_all(b"HTTP/1.1 200 Response\r\nContent-Type: audio/pcm\r\nTransfer-Encoding: chunked\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\r\n"))
        .map_err(|_| Error::new(0, "response_failed"))?;
    let mut total = 0_usize;
    let mut failed = false;
    let result = engine.infer_stream(request, &mut |samples| {
        if failed || samples.is_empty() || samples.iter().any(|sample| !sample.is_finite()) {
            failed = true;
            return Err(io::Error::other("Invalid streaming speech audio."));
        }
        total = total.checked_add(samples.len()).ok_or_else(|| {
            failed = true;
            io::Error::other("Streaming speech exceeds its bound.")
        })?;
        if total > 24_000 * 120 {
            failed = true;
            return Err(io::Error::other("Streaming speech exceeds its bound."));
        }
        for chunk in samples.chunks(16_384) {
            let mut bytes = Vec::with_capacity(chunk.len() * 2);
            for sample in chunk {
                bytes.extend_from_slice(&wave::pcm(*sample).to_le_bytes());
            }
            if write!(peer, "{:x}\r\n", bytes.len())
                .and_then(|()| peer.write_all(&bytes))
                .and_then(|()| peer.write_all(b"\r\n"))
                .is_err()
            {
                failed = true;
                return Err(io::Error::other("Streaming speech response failed."));
            }
        }
        Ok(())
    });
    if result.is_err() || failed || total == 0 {
        // Headers already committed: omit the final chunk, so partial audible
        // output is never confused with a successful complete synthesis.
        return Err(Error::new(0, "model_failure"));
    }
    peer.write_all(b"0\r\n\r\n")
        .map_err(|_| Error::new(0, "response_failed"))
}

fn receive(peer: &mut TcpStream, port: u16, key: &[u8; 64], kind: Kind) -> Result<Prepared, Error> {
    let until = Instant::now() + INTAKE;
    let mut wire = Vec::with_capacity(MAX_HEAD);
    let end = loop {
        if let Some(at) = wire.windows(4).position(|window| window == b"\r\n\r\n") {
            break at + 4;
        }
        if wire.len() >= MAX_HEAD {
            return Err(Error::new(431, "header_limit"));
        }
        let mut buffer = [0; 4096];
        let count = read(
            peer,
            &mut buffer[..(MAX_HEAD - wire.len()).min(4096)],
            until,
        )?;
        wire.extend_from_slice(&buffer[..count]);
    };
    let head = parse_head(&wire[..end], port, key, kind)?;
    let mut body = wire.split_off(end);
    if body.len() > head.length {
        return Err(Error::bad("invalid_framing"));
    }
    if head.health {
        return Ok(Prepared::Health);
    }
    body.reserve(head.length - body.len());
    while body.len() < head.length {
        let mut buffer = [0; 16384];
        let count = read(
            peer,
            &mut buffer[..(head.length - body.len()).min(16384)],
            until,
        )?;
        body.extend_from_slice(&buffer[..count]);
    }
    request::body(kind, &head.media, &body)
}

fn read(peer: &mut TcpStream, buffer: &mut [u8], until: Instant) -> Result<usize, Error> {
    let remaining = until
        .checked_duration_since(Instant::now())
        .ok_or(Error::new(408, "intake_timeout"))?;
    peer.set_read_timeout(Some(remaining))
        .map_err(|_| Error::bad("read_failed"))?;
    match peer.read(buffer) {
        Ok(0) => Err(Error::bad("incomplete_request")),
        Ok(count) => Ok(count),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ) =>
        {
            Err(Error::new(408, "intake_timeout"))
        }
        Err(_) => Err(Error::bad("read_failed")),
    }
}

fn parse_head(bytes: &[u8], port: u16, key: &[u8; 64], kind: Kind) -> Result<Head, Error> {
    if bytes.iter().enumerate().any(|(index, byte)| {
        (*byte == b'\r' && bytes.get(index + 1) != Some(&b'\n'))
            || (*byte == b'\n' && (index == 0 || bytes[index - 1] != b'\r'))
    }) {
        return Err(Error::bad("invalid_headers"));
    }
    let mut slots = [httparse::EMPTY_HEADER; 64];
    let mut parsed = httparse::Request::new(&mut slots);
    if parsed
        .parse(bytes)
        .map_err(|_| Error::bad("invalid_headers"))?
        != httparse::Status::Complete(bytes.len())
        || parsed.version != Some(1)
    {
        return Err(Error::bad("invalid_headers"));
    }
    let mut headers = HashMap::new();
    for header in parsed.headers.iter() {
        if header
            .value
            .iter()
            .any(|byte| (*byte < 32 && *byte != b'\t') || *byte == 127)
            || headers
                .insert(header.name.to_ascii_lowercase(), header.value.trim_ascii())
                .is_some()
        {
            return Err(Error::bad("invalid_headers"));
        }
    }
    let mut expected = [0; 71];
    expected[..7].copy_from_slice(b"Bearer ");
    expected[7..].copy_from_slice(key);
    if !headers
        .get("authorization")
        .is_some_and(|value| bool::from(value.ct_eq(&expected)))
    {
        return Err(Error::new(401, "unauthorized"));
    }
    if headers.get("host").copied() != Some(format!("127.0.0.1:{port}").as_bytes()) {
        return Err(Error::bad("invalid_host"));
    }
    if headers.keys().any(|name| {
        [
            "transfer-encoding",
            "content-encoding",
            "expect",
            "trailer",
            "upgrade",
            "origin",
            "forwarded",
            "proxy-authorization",
            "proxy-connection",
        ]
        .contains(&name.as_str())
            || name.starts_with("x-forwarded-")
    }) {
        return Err(Error::bad("unsupported_headers"));
    }
    if headers
        .get("connection")
        .is_some_and(|value| !value.eq_ignore_ascii_case(b"close"))
    {
        return Err(Error::bad("unsupported_connection"));
    }
    let first_end = bytes
        .windows(2)
        .position(|window| window == b"\r\n")
        .ok_or(Error::bad("invalid_headers"))?;
    let first_line = &bytes[..first_end];
    let health = first_line == b"GET /health HTTP/1.1";
    let route = match kind {
        Kind::Canary => "/v1/audio/transcriptions",
        Kind::QwenTts => "/v1/audio/speech",
    };
    if !health && first_line != format!("POST {route} HTTP/1.1").as_bytes() {
        return Err(Error::new(404, "unsupported_route"));
    }
    let (length, media) = framing(&headers, health, kind)?;
    Ok(Head {
        health,
        length,
        media,
    })
}

fn framing(
    headers: &HashMap<String, &[u8]>,
    health: bool,
    kind: Kind,
) -> Result<(usize, String), Error> {
    let raw_length =
        headers
            .get("content-length")
            .copied()
            .unwrap_or(if health { b"0" } else { b"" });
    if health && raw_length != b"0" {
        return Err(Error::bad("invalid_framing"));
    }
    let length =
        if (1..=8).contains(&raw_length.len()) && raw_length.iter().all(u8::is_ascii_digit) {
            std::str::from_utf8(raw_length)
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
        } else {
            None
        }
        .ok_or(Error::new(413, "body_limit"))?;
    let maximum = match kind {
        Kind::Canary => 1024 * 1024,
        Kind::QwenTts => 65536,
    };
    if health && length != 0 {
        return Err(Error::bad("invalid_framing"));
    }
    if !health && !(1..=maximum).contains(&length) {
        return Err(Error::new(413, "body_limit"));
    }
    let media = std::str::from_utf8(headers.get("content-type").copied().unwrap_or_default())
        .map_err(|_| Error::new(415, "unsupported_media_type"))?
        .to_owned();
    Ok((length, media))
}

fn send(peer: &mut TcpStream, status: u16, media: &str, bytes: &[u8]) -> io::Result<()> {
    let until = Instant::now() + WRITE;
    let head = format!(
        "HTTP/1.1 {status} Response\r\nContent-Type: {media}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\r\n",
        bytes.len()
    );
    for mut remaining in [head.as_bytes(), bytes] {
        while !remaining.is_empty() {
            let timeout = until
                .checked_duration_since(Instant::now())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::TimedOut, "Speech response deadline")
                })?;
            peer.set_write_timeout(Some(timeout))?;
            let count = peer.write(remaining)?;
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "Speech response closed",
                ));
            }
            remaining = &remaining[count..];
        }
    }
    Ok(())
}
