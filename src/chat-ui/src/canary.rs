//! Bounded authenticated loopback adapter for the local Canary Qwen companion.
//!
//! Audio and transcripts remain in memory. The supported HTTP subset uses one
//! fixed-length response per connection, with no redirects, retries or proxies.

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "Only the native speech worker uses this crate-private adapter."
)]

use crate::native_speech::{Launch, Service};
use serde::Deserialize;
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

const MODEL: &str = "nvidia/canary-qwen-2.5b";
const SAMPLE_RATE: u32 = 16000;
const CLIP_SECONDS: u32 = 30;
const MAX_REQUEST: usize = 1024 * 1024;
const MAX_RESPONSE: usize = 64 * 1024;
const MAX_WAVE_RESPONSE: usize = 5_760_044;
const MAX_HEAD: usize = 16 * 1024;
const MAX_TEXT: usize = 32768;
const IO_POLL: Duration = Duration::from_millis(250);
const READY_LIMIT: Duration = Duration::from_secs(5);
const TRANSCRIBE_LIMIT: Duration = Duration::from_secs(120);
const REPARSE_POINT: u32 = 0x400;
const OPEN_REPARSE_POINT: u32 = 0x0020_0000;

mod pcm;

/// Explicit companion configuration; authentication bytes never have Debug output.
#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) port: u16,
    pub(crate) key_file: PathBuf,
    pub(crate) launch: Option<Launch>,
}

/// Retains its credential handle to deny file writes and deletion while in use.
pub(crate) struct Client {
    port: u16,
    key: String,
    _key_file: File,
}

/// Value-free failures are safe to present without exposing audio or credentials.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Cancelled,
    Message(&'static str),
}

/// Fixed response media and bounds for the two explicitly configured companions.
#[derive(Clone, Copy)]
pub(crate) enum Media {
    Json,
    Wave,
}

impl Media {
    const fn limit(self) -> usize {
        match self {
            Self::Json => MAX_RESPONSE,
            Self::Wave => MAX_WAVE_RESPONSE,
        }
    }

    fn accepts(self, value: &[u8]) -> bool {
        match self {
            Self::Json => json_media_type(value),
            Self::Wave => value.eq_ignore_ascii_case(b"audio/wav"),
        }
    }
}

type Result<T> = std::result::Result<T, Failure>;

impl Client {
    /// Read an existing external token file without creating or modifying it.
    pub(crate) fn new(config: &Config) -> std::result::Result<Self, &'static str> {
        if config.port < 1024 {
            return Err("Use an unprivileged local Canary port.");
        }
        bonsai_windows_credentials::validate_external_path(&config.key_file)
            .map_err(|_| "Use an existing absolute local Canary key path outside Git.")?;
        let mut file = OpenOptions::new()
            .read(true)
            .share_mode(1) // FILE_SHARE_READ; deny write and delete sharing.
            .custom_flags(OPEN_REPARSE_POINT)
            .open(&config.key_file)
            .map_err(|_| "The Canary credential file is unavailable.")?;
        let metadata = file
            .metadata()
            .map_err(|_| "The Canary credential file is unavailable.")?;
        if !metadata.is_file()
            || metadata.len() != 64
            || metadata.file_attributes() & REPARSE_POINT != 0
        {
            return Err("The Canary credential must be a regular 64-byte token file.");
        }
        let mut key = [0_u8; 64];
        file.read_exact(&mut key)
            .map_err(|_| "The Canary credential could not be read.")?;
        if !key
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        {
            return Err("The Canary credential must contain 64 lowercase hexadecimal bytes.");
        }
        let key = String::from_utf8(key.to_vec())
            .map_err(|_| "The Canary credential has an invalid encoding.")?;
        // Path checks and retained handles do not attest ACLs or freeze mutable
        // ancestors. The companion must create its key under a protected parent.
        Ok(Self {
            port: config.port,
            key,
            _key_file: file,
        })
    }

    /// Verify the exact model identity and admitted capture format, without audio.
    pub(crate) fn ready(&self) -> std::result::Result<(), &'static str> {
        let current = AtomicU64::new(0);
        self.ready_for(0, &current)
            .map_err(|failure| match failure {
                Failure::Cancelled => "The Canary readiness check was cancelled.",
                Failure::Message(message) => message,
            })
    }

    fn ready_for(&self, generation: u64, current: &AtomicU64) -> Result<()> {
        let call = Call::new(generation, current, READY_LIMIT);
        let result = self.exchange("GET", "/health", None, &[], Media::Json, &call)
            .and_then(|body| {
                let health: Health = serde_json::from_slice(&body).map_err(|_| {
                    Failure::Message("The Canary companion returned an invalid capability response.")
                })?;
                if health.model != MODEL
                    || !health.ready
                    || health.sample_rate != SAMPLE_RATE
                    || health.max_clip_seconds != CLIP_SECONDS
                    || health.input_format != "pcm_s16le_wav"
                    || health.channels != 1
                {
                    return Err(Failure::Message(
                        "The Canary companion is not ready with the required model and audio format.",
                    ));
                }
                Ok(())
            });
        call.check().and(result)
    }

    /// Start only for an explicitly captured clip and retain ownership between clips.
    pub(crate) fn start_service(
        &self,
        config: &Config,
        generation: u64,
        wav: &[u8],
        current: &AtomicU64,
    ) -> Result<Option<Service>> {
        let call = Call::new(generation, current, READY_LIMIT);
        call.check()?;
        let _validated = normalize_wav(wav, &call)?;
        let Some(launch) = &config.launch else {
            self.ready_for(generation, current)?;
            return Ok(None);
        };
        if launch.codec.is_some() {
            return Err(Failure::Message(
                "Native Canary does not accept a speech codec.",
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

    /// Transcribe one bounded mono PCM clip; a cancelled generation never returns text.
    pub(crate) fn transcribe(
        &self,
        generation: u64,
        wav: &[u8],
        current: &AtomicU64,
    ) -> Result<String> {
        let call = Call {
            generation,
            current,
            until: Instant::now() + TRANSCRIBE_LIMIT,
            cancelled: None,
        };
        let result = self.transcribe_clip(wav, &call);
        call.check().and(result)
    }

    fn transcribe_clip(&self, wav: &[u8], call: &Call<'_>) -> Result<String> {
        call.check()?;
        let wav = normalize_wav(wav, call)?;
        let boundary = boundary(&wav, call)?;
        let body = multipart(&wav, &boundary)?;
        call.check()?;
        let content_type = format!("multipart/form-data; boundary={boundary}");
        let response = self.exchange(
            "POST",
            "/v1/audio/transcriptions",
            Some(&content_type),
            &body,
            Media::Json,
            call,
        )?;
        call.check()?;
        let transcript: Transcript = serde_json::from_slice(&response).map_err(|_| {
            Failure::Message("The Canary companion returned an invalid transcription.")
        })?;
        if transcript.model != MODEL {
            return Err(Failure::Message(
                "The Canary transcription came from an unexpected model.",
            ));
        }
        if transcript.text.contains('\0')
            || transcript.text.encode_utf16().take(MAX_TEXT + 1).count() > MAX_TEXT
        {
            return Err(Failure::Message(
                "The Canary transcript exceeded the supported text limit.",
            ));
        }
        call.check()?;
        Ok(transcript.text)
    }

    pub(crate) fn exchange(
        &self,
        method: &str,
        route: &str,
        content_type: Option<&str>,
        body: &[u8],
        expected: Media,
        call: &Call<'_>,
    ) -> Result<Vec<u8>> {
        let mut stream = self.connection(method, route, content_type, body, call)?;
        read_response(&mut stream, call, expected)
    }

    pub(crate) fn stream_pcm(
        &self,
        body: &[u8],
        call: &Call<'_>,
        sink: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        let mut stream = self.connection(
            "POST",
            "/v1/audio/speech",
            Some("application/json"),
            body,
            call,
        )?;
        pcm::decode(
            |bytes| loop {
                call.check()?;
                let result = stream.read(bytes);
                call.check()?;
                match result {
                    Ok(count) => break Ok(count),
                    Err(error) if retry_poll(&error) => {}
                    Err(_) => {
                        break Err(Failure::Message("The local speech stream was interrupted."));
                    }
                }
            },
            sink,
        )
    }

    fn connection(
        &self,
        method: &str,
        route: &str,
        content_type: Option<&str>,
        body: &[u8],
        call: &Call<'_>,
    ) -> Result<TcpStream> {
        call.check()?;
        if body.len() > MAX_REQUEST {
            return Err(Failure::Message(
                "The Canary request exceeded its size limit.",
            ));
        }
        let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.port));
        let timeout = IO_POLL.min(call.until.saturating_duration_since(Instant::now()));
        let mut stream = TcpStream::connect_timeout(&address, timeout)
            .map_err(|_| Failure::Message("Could not connect to the local Canary companion."))?;
        call.check()?;
        stream
            .set_read_timeout(Some(IO_POLL))
            .and_then(|()| stream.set_write_timeout(Some(IO_POLL)))
            .map_err(|_| Failure::Message("Could not configure the Canary connection timeout."))?;
        let media =
            content_type.map_or_else(String::new, |media| format!("Content-Type: {media}\r\n"));
        let head = format!(
            "{method} {route} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nAccept: application/json\r\nAccept-Encoding: identity\r\nContent-Length: {}\r\n{media}Connection: close\r\n\r\n",
            self.port,
            self.key,
            body.len()
        );
        write_all(&mut stream, head.as_bytes(), call)?;
        write_all(&mut stream, body, call)?;
        Ok(stream)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Health {
    model: String,
    ready: bool,
    sample_rate: u32,
    max_clip_seconds: u32,
    input_format: String,
    channels: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Transcript {
    text: String,
    model: String,
}

pub(crate) struct Call<'a> {
    generation: u64,
    current: &'a AtomicU64,
    until: Instant,
    cancelled: Option<&'a AtomicBool>,
}

impl<'a> Call<'a> {
    pub(crate) fn new(generation: u64, current: &'a AtomicU64, limit: Duration) -> Self {
        Self {
            generation,
            current,
            until: Instant::now() + limit,
            cancelled: None,
        }
    }

    pub(crate) fn cancellable(
        generation: u64,
        current: &'a AtomicU64,
        cancelled: &'a AtomicBool,
        limit: Duration,
    ) -> Self {
        Self {
            cancelled: Some(cancelled),
            ..Self::new(generation, current, limit)
        }
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.current.load(Ordering::Acquire) != self.generation
            || self
                .cancelled
                .is_some_and(|flag| flag.load(Ordering::Acquire))
        {
            Err(Failure::Cancelled)
        } else if Instant::now() >= self.until {
            Err(Failure::Message(
                "The Canary request exceeded its time limit.",
            ))
        } else {
            Ok(())
        }
    }
}

fn validate_wav(wav: &[u8]) -> Result<()> {
    let invalid = Failure::Message(
        "Canary requires a 16 kHz mono 16-bit PCM WAV clip of at most 30 seconds.",
    );
    if wav.len() < 46 || wav.len() > MAX_REQUEST {
        return Err(invalid);
    }
    let u16_at = |at| u16::from_le_bytes([wav[at], wav[at + 1]]);
    let u32_at = |at| u32::from_le_bytes([wav[at], wav[at + 1], wav[at + 2], wav[at + 3]]);
    let data_length = u32_at(40);
    if &wav[..4] != b"RIFF"
        || usize::try_from(u32_at(4)).ok() != Some(wav.len() - 8)
        || &wav[8..16] != b"WAVEfmt "
        || u32_at(16) != 16
        || u16_at(20) != 1
        || u16_at(22) != 1
        || u32_at(24) != SAMPLE_RATE
        || u32_at(28) != SAMPLE_RATE * 2
        || u16_at(32) != 2
        || u16_at(34) != 16
        || &wav[36..40] != b"data"
        || data_length == 0
        || data_length % 2 != 0
        || data_length > SAMPLE_RATE * 2 * CLIP_SECONDS
        || usize::try_from(data_length).ok() != Some(wav.len() - 44)
    {
        return Err(invalid);
    }
    Ok(())
}

fn normalize_wav(wav: &[u8], call: &Call<'_>) -> Result<Vec<u8>> {
    let invalid = Failure::Message(
        "Canary requires a 16 kHz mono 16-bit PCM WAV clip of at most 30 seconds.",
    );
    if wav.len() < 12
        || wav.len() > MAX_REQUEST
        || &wav[..4] != b"RIFF"
        || &wav[8..12] != b"WAVE"
        || usize::try_from(u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]])).ok()
            != Some(wav.len() - 8)
    {
        return Err(invalid);
    }
    let mut offset = 12;
    let mut format = false;
    let mut data = None;
    let mut chunks = 0;
    while offset < wav.len() {
        call.check()?;
        chunks += 1;
        let head_end = offset.checked_add(8).ok_or(invalid)?;
        if chunks > 128 || head_end > wav.len() {
            return Err(invalid);
        }
        let id = &wav[offset..offset + 4];
        let length = usize::try_from(u32::from_le_bytes([
            wav[offset + 4],
            wav[offset + 5],
            wav[offset + 6],
            wav[offset + 7],
        ]))
        .map_err(|_| invalid)?;
        let end = head_end.checked_add(length).ok_or(invalid)?;
        let padded_end = end.checked_add(length % 2).ok_or(invalid)?;
        if padded_end > wav.len() {
            return Err(invalid);
        }
        match id {
            b"fmt " => {
                if format || !pcm_format(&wav[head_end..end]) {
                    return Err(invalid);
                }
                format = true;
            }
            b"data" => {
                if data.replace(&wav[head_end..end]).is_some() {
                    return Err(invalid);
                }
            }
            _ => {}
        }
        offset = padded_end;
    }
    let data = data.ok_or(invalid)?;
    if !format || data.is_empty() || data.len() % 2 != 0 || data.len() > 960_000 {
        return Err(invalid);
    }
    let length = u32::try_from(data.len()).map_err(|_| invalid)?;
    let mut canonical = Vec::with_capacity(44 + data.len());
    canonical.extend_from_slice(b"RIFF");
    canonical.extend_from_slice(&(36 + length).to_le_bytes());
    canonical.extend_from_slice(b"WAVEfmt ");
    canonical.extend_from_slice(&16_u32.to_le_bytes());
    canonical.extend_from_slice(&1_u16.to_le_bytes());
    canonical.extend_from_slice(&1_u16.to_le_bytes());
    canonical.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    canonical.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    canonical.extend_from_slice(&2_u16.to_le_bytes());
    canonical.extend_from_slice(&16_u16.to_le_bytes());
    canonical.extend_from_slice(b"data");
    canonical.extend_from_slice(&length.to_le_bytes());
    canonical.extend_from_slice(data);
    validate_wav(&canonical)?;
    call.check()?;
    Ok(canonical)
}

fn pcm_format(format: &[u8]) -> bool {
    if format.len() < 16 {
        return false;
    }
    let u16_at = |at| u16::from_le_bytes([format[at], format[at + 1]]);
    let u32_at =
        |at| u32::from_le_bytes([format[at], format[at + 1], format[at + 2], format[at + 3]]);
    if u16_at(2) != 1
        || u32_at(4) != SAMPLE_RATE
        || u32_at(8) != SAMPLE_RATE * 2
        || u16_at(12) != 2
        || u16_at(14) != 16
    {
        return false;
    }
    match u16_at(0) {
        1 => format.len() == 16 || (format.len() == 18 && u16_at(16) == 0),
        0xfffe => {
            format.len() == 40
                && u16_at(16) == 22
                && u16_at(18) == 16
                && u32_at(20).count_ones() <= 1
                && format[24..] == [1, 0, 0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113]
        }
        _ => false,
    }
}

fn boundary(wav: &[u8], call: &Call<'_>) -> Result<String> {
    const PREFIX: &[u8] = b"bonsai-canary-";
    let mut used = HashSet::new();
    for (index, window) in wav.windows(PREFIX.len() + 16).enumerate() {
        if index % 8192 == 0 {
            call.check()?;
        }
        if let Some(suffix) = window.strip_prefix(PREFIX)
            && let Ok(hex) = std::str::from_utf8(suffix)
            && let Ok(value) = u64::from_str_radix(hex, 16)
        {
            used.insert(value);
        }
    }
    // There are at most used.len() occupied counter values. The first N+1
    // values contain a free one, so the selected boundary cannot occur in WAV
    // bytes even for adversarial PCM payloads. No random-boundary assumption.
    let count = u64::try_from(used.len())
        .map_err(|_| Failure::Message("Could not prepare the Canary upload boundary."))?;
    let counter = (0..=count)
        .find(|candidate| !used.contains(candidate))
        .ok_or(Failure::Message(
            "Could not prepare the Canary upload boundary.",
        ))?;
    call.check()?;
    Ok(format!("bonsai-canary-{counter:016x}"))
}

fn multipart(wav: &[u8], boundary: &str) -> Result<Vec<u8>> {
    let fields = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\n{MODEL}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"clip.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
    );
    let ending = format!("\r\n--{boundary}--\r\n");
    let length = fields.len() + wav.len() + ending.len();
    if length > MAX_REQUEST {
        return Err(Failure::Message(
            "The Canary upload exceeded its size limit.",
        ));
    }
    let mut body = Vec::with_capacity(length);
    body.extend_from_slice(fields.as_bytes());
    body.extend_from_slice(wav);
    body.extend_from_slice(ending.as_bytes());
    Ok(body)
}

fn write_all(stream: &mut TcpStream, mut bytes: &[u8], call: &Call<'_>) -> Result<()> {
    while !bytes.is_empty() {
        call.check()?;
        let result = stream.write(bytes);
        call.check()?;
        match result {
            Ok(0) => {
                return Err(Failure::Message(
                    "The Canary connection closed during upload.",
                ));
            }
            Ok(count) => bytes = &bytes[count..],
            Err(error) if retry_poll(&error) => {}
            Err(_) => return Err(Failure::Message("The Canary upload was interrupted.")),
        }
    }
    call.check()
}

fn read(stream: &mut TcpStream, bytes: &mut [u8], call: &Call<'_>) -> Result<usize> {
    loop {
        call.check()?;
        let result = stream.read(bytes);
        call.check()?;
        match result {
            Ok(0) => {
                return Err(Failure::Message(
                    "The Canary response ended before its framing completed.",
                ));
            }
            Ok(count) => return Ok(count),
            Err(error) if retry_poll(&error) => {}
            Err(_) => return Err(Failure::Message("The Canary response was interrupted.")),
        }
    }
}

fn retry_poll(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::Interrupted
    )
}

fn read_response(stream: &mut TcpStream, call: &Call<'_>, expected: Media) -> Result<Vec<u8>> {
    let mut received = Vec::new();
    let mut buffer = [0; 4096];
    let end = loop {
        if let Some(separator) = received.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break separator + 4;
        }
        if received.len() >= MAX_HEAD {
            return Err(Failure::Message(
                "The Canary response header exceeded its limit.",
            ));
        }
        let size = buffer.len().min(MAX_HEAD - received.len());
        let count = read(stream, &mut buffer[..size], call)?;
        received.extend_from_slice(&buffer[..count]);
    };
    let length = response_length(&received[..end], expected)?;
    if received.len() - end > length {
        return Err(Failure::Message(
            "The Canary response exceeded its declared length.",
        ));
    }
    let mut body = Vec::with_capacity(length);
    body.extend_from_slice(&received[end..]);
    while body.len() < length {
        let size = buffer.len().min(length - body.len());
        let count = read(stream, &mut buffer[..size], call)?;
        body.extend_from_slice(&buffer[..count]);
    }
    call.check()?;
    Ok(body)
}

fn response_length(head: &[u8], expected: Media) -> Result<usize> {
    let invalid = Failure::Message("The Canary companion returned unsupported HTTP framing.");
    if !head.starts_with(b"HTTP/1.1 ")
        || head.iter().enumerate().any(|(index, &byte)| {
            (byte == b'\n' && (index == 0 || head[index - 1] != b'\r'))
                || (byte == b'\r' && head.get(index + 1) != Some(&b'\n'))
        })
    {
        return Err(invalid);
    }
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut response = httparse::Response::new(&mut headers);
    if response.parse(head).ok() != Some(httparse::Status::Complete(head.len()))
        || response.version != Some(1)
        || response.code != Some(200)
    {
        return Err(Failure::Message(
            "The Canary companion rejected the request.",
        ));
    }
    let mut length = None;
    let mut media = false;
    for (index, header) in response.headers.iter().enumerate() {
        if response.headers[..index]
            .iter()
            .any(|previous| previous.name.eq_ignore_ascii_case(header.name))
            || [
                "transfer-encoding",
                "content-encoding",
                "trailer",
                "upgrade",
            ]
            .iter()
            .any(|name| header.name.eq_ignore_ascii_case(name))
        {
            return Err(invalid);
        }
        if header.name.eq_ignore_ascii_case("content-length") {
            if header.value.is_empty() || !header.value.iter().all(u8::is_ascii_digit) {
                return Err(invalid);
            }
            length = std::str::from_utf8(header.value)
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|&value| value <= expected.limit());
            if length.is_none() {
                return Err(invalid);
            }
        } else if header.name.eq_ignore_ascii_case("content-type") {
            media = expected.accepts(header.value);
        } else if header.name.eq_ignore_ascii_case("connection")
            && !header.value.eq_ignore_ascii_case(b"close")
        {
            return Err(invalid);
        }
    }
    if !media {
        return Err(invalid);
    }
    length.ok_or(invalid)
}

fn json_media_type(value: &[u8]) -> bool {
    let Ok(value) = std::str::from_utf8(value) else {
        return false;
    };
    let mut parts = value.split(';');
    if !parts
        .next()
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"))
    {
        return false;
    }
    let Some(parameter) = parts.next() else {
        return true;
    };
    let Some((name, charset)) = parameter.split_once('=') else {
        return false;
    };
    name.trim().eq_ignore_ascii_case("charset")
        && (charset.trim().eq_ignore_ascii_case("utf-8")
            || charset.trim().eq_ignore_ascii_case("\"utf-8\""))
        && parts.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::{
        Call, Client, Config, Failure, MODEL, Media, boundary, multipart, normalize_wav,
        response_length, validate_wav,
    };
    use std::{
        fs,
        io::{self, Read, Write},
        net::{TcpListener, TcpStream},
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    fn wav(data: &[u8]) -> io::Result<Vec<u8>> {
        let length = u32::try_from(data.len()).map_err(io::Error::other)?;
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&(36 + length).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&16000_u32.to_le_bytes());
        bytes.extend_from_slice(&32000_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(data);
        Ok(bytes)
    }

    fn client(port: u16) -> io::Result<(bonsai_windows_credentials::PrivateCredentials, Client)> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let parent = std::env::temp_dir().join(format!("Bonsai-Canary-Protocol-{stamp}"));
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

    fn request(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        let mut bytes = Vec::new();
        let end = loop {
            let mut byte = [0];
            stream.read_exact(&mut byte)?;
            bytes.push(byte[0]);
            if bytes.ends_with(b"\r\n\r\n") {
                break bytes.len();
            }
            assert!(bytes.len() <= 16384);
        };
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut request = httparse::Request::new(&mut headers);
        request.parse(&bytes).map_err(io::Error::other)?;
        let length = request
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case("content-length"))
            .and_then(|header| std::str::from_utf8(header.value).ok())
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| io::Error::other("Missing test request length"))?;
        assert!(length <= 1024 * 1024);
        bytes.resize(end + length, 0);
        stream.read_exact(&mut bytes[end..])?;
        Ok(bytes)
    }

    fn respond(stream: &mut TcpStream, body: &[u8]) -> io::Result<()> {
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes())?;
        stream.write_all(body)
    }

    #[test]
    fn invalid_capture_is_rejected_before_native_port_admission_or_model_loading() -> io::Result<()>
    {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let (credentials, client) = client(port)?;
        let evidence = credentials
            .frontend_path()
            .parent()
            .ok_or_else(|| io::Error::other("Missing test credential parent."))?
            .join("model-evidence");
        fs::create_dir(&evidence)?;
        let config = Config {
            port,
            key_file: credentials.frontend_path().to_owned(),
            launch: Some(crate::native_speech::Launch {
                executable: evidence.join("unavailable-native.exe"),
                model: evidence.join("unavailable-model.gguf"),
                codec: None,
                quantization: None,
                threads: None,
                evidence: evidence.clone(),
            }),
        };
        assert!(
            matches!(client.start_service(&config, 7, &[], &AtomicU64::new(7)),
            Err(Failure::Message(message)) if message.contains("16 kHz mono 16-bit PCM WAV"))
        );
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock)
        );
        assert!(fs::read_dir(&evidence)?.next().is_none());
        fs::remove_dir(evidence)?;
        Ok(())
    }

    #[test]
    fn authenticated_health_and_multipart_transcription_use_exact_canary_contract() -> io::Result<()>
    {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let (credentials, client) = client(listener.local_addr()?.port())?;
        let authorization = format!("Authorization: Bearer {}\r\n", credentials.frontend_key());
        let audio = wav(&[0, 0, 1, 0])?;
        let expected = audio.clone();
        let server = thread::spawn(move || -> io::Result<()> {
            let (mut stream, _) = listener.accept()?;
            let health = request(&mut stream)?;
            assert!(health.starts_with(b"GET /health HTTP/1.1\r\n"));
            assert!(
                health
                    .windows(authorization.len())
                    .any(|bytes| bytes == authorization.as_bytes())
            );
            respond(&mut stream, br#"{"model":"nvidia/canary-qwen-2.5b","ready":true,"sample_rate":16000,"max_clip_seconds":30,"input_format":"pcm_s16le_wav","channels":1}"#)?;
            drop(stream);
            for response in [
                br#"{"text":"Test words","model":"nvidia/canary-qwen-2.5b"}"#.as_slice(),
                br#"{"text":"","model":"nvidia/canary-qwen-2.5b"}"#.as_slice(),
            ] {
                let (mut stream, _) = listener.accept()?;
                let upload = request(&mut stream)?;
                assert!(upload.starts_with(b"POST /v1/audio/transcriptions HTTP/1.1\r\n"));
                assert!(
                    upload
                        .windows(authorization.len())
                        .any(|bytes| bytes == authorization.as_bytes())
                );
                assert!(
                    upload
                        .windows(expected.len())
                        .any(|bytes| bytes == expected)
                );
                assert!(
                    upload
                        .windows(MODEL.len())
                        .any(|bytes| bytes == MODEL.as_bytes())
                );
                assert!(
                    upload
                        .windows(19)
                        .any(|bytes| bytes == b"filename=\"clip.wav\"")
                );
                respond(&mut stream, response)?;
            }
            Ok(())
        });
        client.ready().map_err(io::Error::other)?;
        let current = AtomicU64::new(7);
        assert_eq!(
            client
                .transcribe(7, &audio, &current)
                .map_err(|_| io::Error::other("Transcription failed"))?,
            "Test words"
        );
        assert_eq!(
            client
                .transcribe(7, &audio, &current)
                .map_err(|_| io::Error::other("Silent clip failed"))?,
            ""
        );
        server
            .join()
            .map_err(|_| io::Error::other("Fake companion failed"))??;
        Ok(())
    }

    #[test]
    fn cancellation_discards_a_pending_transcription_without_retrying() -> io::Result<()> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let (_credentials, client) = client(listener.local_addr()?.port())?;
        let current = Arc::new(AtomicU64::new(9));
        let cancel = Arc::clone(&current);
        let (done, finished) = mpsc::channel();
        let server = thread::spawn(move || -> io::Result<()> {
            let (mut stream, _) = listener.accept()?;
            let _request = request(&mut stream)?;
            stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n",
            )?;
            cancel.store(10, Ordering::Release);
            finished
                .recv_timeout(Duration::from_secs(3))
                .map_err(io::Error::other)?;
            Ok(())
        });
        let result = client.transcribe(9, &wav(&[0, 0])?, &current);
        done.send(()).map_err(io::Error::other)?;
        assert_eq!(result, Err(Failure::Cancelled));
        server
            .join()
            .map_err(|_| io::Error::other("Fake companion failed"))??;
        Ok(())
    }

    #[test]
    fn wav_validation_and_boundary_selection_reject_ambiguous_uploads() -> io::Result<()> {
        let audio = wav(b"bonsai-canary-0000000000000000")?;
        assert!(validate_wav(&audio).is_ok());
        let current = AtomicU64::new(1);
        let call = Call {
            generation: 1,
            current: &current,
            until: Instant::now() + Duration::from_secs(1),
            cancelled: None,
        };
        let marker =
            boundary(&audio, &call).map_err(|_| io::Error::other("Boundary preparation failed"))?;
        assert_eq!(marker, "bonsai-canary-0000000000000001");
        assert!(multipart(&audio, &marker).is_ok());
        for offset in [4, 16, 20, 22, 24, 28, 32, 34, 40] {
            let mut invalid = audio.clone();
            invalid[offset] ^= 1;
            assert!(validate_wav(&invalid).is_err());
        }
        assert!(validate_wav(&wav(&[])?).is_err());
        assert!(validate_wav(&wav(&[0])?).is_err());
        Ok(())
    }

    #[test]
    fn windows_extensible_capture_normalizes_ancillary_chunks_and_rejects_corruption()
    -> io::Result<()> {
        let canonical = wav(&[0, 0, 1, 0])?;
        let mut extensible = b"RIFF\0\0\0\0WAVEJUNK\x03\0\0\0abc\0fmt \x28\0\0\0".to_vec();
        let mut format = canonical[20..36].to_vec();
        format[..2].copy_from_slice(&0xfffe_u16.to_le_bytes());
        format.extend_from_slice(&22_u16.to_le_bytes());
        format.extend_from_slice(&16_u16.to_le_bytes());
        format.extend_from_slice(&4_u32.to_le_bytes());
        format.extend_from_slice(&[1, 0, 0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113]);
        extensible.extend_from_slice(&format);
        extensible.extend_from_slice(&canonical[36..]);
        let riff_length = u32::try_from(extensible.len() - 8).map_err(io::Error::other)?;
        extensible[4..8].copy_from_slice(&riff_length.to_le_bytes());
        let current = AtomicU64::new(1);
        let call = Call {
            generation: 1,
            current: &current,
            until: Instant::now() + Duration::from_secs(1),
            cancelled: None,
        };
        assert_eq!(
            normalize_wav(&extensible, &call)
                .map_err(|_| io::Error::other("Capture normalization failed"))?,
            canonical
        );
        let mut duplicate = extensible.clone();
        duplicate.extend_from_slice(&canonical[36..]);
        let length = u32::try_from(duplicate.len() - 8).map_err(io::Error::other)?;
        duplicate[4..8].copy_from_slice(&length.to_le_bytes());
        assert!(normalize_wav(&duplicate, &call).is_err());
        let mut float_format = extensible.clone();
        float_format[56] = 3; // Extensible subtype changes from PCM to IEEE float.
        assert!(normalize_wav(&float_format, &call).is_err());
        extensible.pop();
        assert!(normalize_wav(&extensible, &call).is_err());
        Ok(())
    }

    #[test]
    fn strict_json_http_framing_rejects_duplicates_encodings_and_oversize() {
        let valid = b"HTTP/1.1 200 OK\r\nContent-Type: APPLICATION/JSON; charset=\"UTF-8\"\r\nContent-Length: 1\r\n\r\n";
        assert_eq!(response_length(valid, Media::Json), Ok(1));
        for headers in [
            "Content-Length: 1\r\nContent-Length: 1\r\n",
            "Content-Length: 1\r\nTransfer-Encoding: chunked\r\n",
            "Content-Length: 1\r\nContent-Encoding: identity\r\n",
            "Content-Length: 65537\r\n",
            "Content-Length: +1\r\n",
            "Transfer-Encoding: chunked\r\n",
        ] {
            let head =
                format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{headers}\r\n");
            assert!(response_length(head.as_bytes(), Media::Json).is_err());
        }
        assert!(
            response_length(
                b"HTTP/1.1 200 OK\nContent-Type: application/json\nContent-Length: 1\n\n",
                Media::Json
            )
            .is_err()
        );
    }
}
