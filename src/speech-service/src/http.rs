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
    net::{Shutdown, TcpStream},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq as _;

const MAX_HEAD: usize = 16 * 1024;
const INTAKE: Duration = Duration::from_secs(10);
const WRITE: Duration = Duration::from_secs(5);
const RETAINED_WIRE_BYTES: usize = 64 * 1024;

/// One bounded intake allocation reused between synchronous requests.
#[derive(Default)]
pub(super) struct Scratch {
    wire: Vec<u8>,
}

impl Scratch {
    fn recycle(&mut self) {
        self.wire.clear();
        // Large transcription uploads need no idle retained wire allocation.
        if self.wire.capacity() > RETAINED_WIRE_BYTES {
            self.wire = Vec::new();
        }
    }
}

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

#[derive(Default)]
struct HeaderEnd {
    next: usize,
    #[cfg(test)]
    comparisons: usize,
}

impl HeaderEnd {
    fn find(&mut self, bytes: &[u8]) -> Option<usize> {
        for (offset, window) in bytes[self.next..].array_windows().enumerate() {
            #[cfg(test)]
            {
                self.comparisons += 1;
            }
            if window == b"\r\n\r\n" {
                return Some(self.next + offset + 4);
            }
        }
        // An unchanged prefix cannot gain a delimiter; retain only its overlap.
        self.next = bytes.len().saturating_sub(3);
        None
    }
}

pub(super) fn handle(
    peer: &mut TcpStream,
    port: u16,
    key: &[u8; 64],
    engine: &mut impl Engine,
    scratch: &mut Scratch,
) {
    let result = receive(peer, port, key, engine.kind(), scratch)
        .and_then(|prepared| respond(engine, &prepared, peer));
    scratch.recycle();
    if let Err(error) = result {
        if error.status == 0 {
            return;
        }
        let body = serde_json::json!({"error": {"code": error.code}}).to_string();
        if send(peer, error.status, "application/json", body.as_bytes()).is_err() {
            return;
        }
    }
    // Complete framed responses end gracefully before the socket is dropped.
    // Incomplete streams return above without manufacturing a successful end.
    let _closed = peer.shutdown(Shutdown::Write);
}

fn respond(
    engine: &mut impl Engine,
    prepared: &Prepared<'_>,
    peer: &mut TcpStream,
) -> Result<(), Error> {
    let kind = engine.kind();
    let Some(input) = prepared.engine_request() else {
        let value = match kind {
            Kind::Canary => {
                serde_json::json!({"model":kind.model(),"ready":true,"sample_rate":16000,"max_clip_seconds":30,"input_format":"pcm_s16le_wav","channels":1})
            }
            Kind::QwenTts => {
                serde_json::json!({"model":kind.model(),"ready":true,"voice_design":true,"sample_rate":24000,"max_text_characters":2048,"max_voice_characters":1024,"streaming_pcm":engine.streaming_pcm(),"consistent_voice":engine.consistent_voice()})
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
        (Kind::QwenTts, Ok(audio @ (Output::Audio(_) | Output::Waveform { .. }))) => {
            return complete_wave(audio, |samples, header| send_wave(peer, samples, header));
        }
        _ => return Err(Error::new(502, "model_failure")),
    }
    .map_err(|_| Error::new(0, "response_failed"))
}

fn complete_wave<S: AsRef<[f32]>>(
    audio: Output<S>,
    send: impl FnOnce(&[f32], &[u8; 44]) -> io::Result<()>,
) -> Result<(), Error> {
    let (samples, sample_rate) = match &audio {
        Output::Audio(audio) => (audio.samples.as_slice(), audio.sample_rate),
        Output::Waveform {
            samples,
            sample_rate,
        } => (samples.as_ref(), *sample_rate),
        Output::Transcript(_) => return Err(Error::new(502, "model_failure")),
    };
    let header = wave::header(samples, sample_rate)?;
    // Both storage owners stay alive until bounded conversion/writes finish.
    // Returning through validation or I/O errors releases the same owner.
    let result = send(samples, &header).map_err(|_| Error::new(0, "response_failed"));
    drop(audio);
    result
}

fn stream(
    engine: &mut impl Engine,
    request: crate::Request<'_>,
    peer: &mut TcpStream,
) -> Result<(), Error> {
    let watchdog = Watchdog::start(peer, Duration::from_secs(180))
        .map_err(|_| Error::new(503, "supervision_unavailable"))?;
    peer.set_write_timeout(Some(WRITE))
        .and_then(|()| peer.write_all(b"HTTP/1.1 200 Response\r\nContent-Type: audio/pcm\r\nTransfer-Encoding: chunked\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\r\n"))
        .map_err(|_| Error::new(0, "response_failed"))?;
    let mut frame = PcmFrame::new();
    let mut total = 0_usize;
    let mut failed = false;
    let result = engine.infer_stream(request, &mut |samples| {
        if failed {
            return Err(io::Error::other("Invalid streaming speech audio."));
        }
        total = bonsai_audio::finite_sample_count(samples, total, wave::MAX_SAMPLES).ok_or_else(
            || {
                failed = true;
                io::Error::other("Invalid streaming speech audio.")
            },
        )?;
        for chunk in samples.chunks(wave::PCM_SAMPLES) {
            if peer.write_all(frame.encode(chunk)).is_err() {
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
    // Complete native output has been validated. Stop and join cancellation
    // before publishing success: the peer can close as soon as it sees this
    // terminal chunk, while an armed peek would interpret that EOF as cancel.
    // Final framing retains the existing bounded write timeout.
    drop(watchdog);
    peer.write_all(b"0\r\n\r\n")
        .map_err(|_| Error::new(0, "response_failed"))
}

fn receive<'a>(
    peer: &mut TcpStream,
    port: u16,
    key: &[u8; 64],
    kind: Kind,
    scratch: &'a mut Scratch,
) -> Result<Prepared<'a>, Error> {
    let until = Instant::now() + INTAKE;
    let wire = &mut scratch.wire;
    wire.clear();
    wire.try_reserve_exact(MAX_HEAD)
        .map_err(|_| Error::new(503, "allocation_failed"))?;
    let mut header_end = HeaderEnd::default();
    let end = loop {
        if let Some(end) = header_end.find(wire) {
            break end;
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
    let prefix = wire.len() - end;
    if prefix > head.length {
        return Err(Error::bad("invalid_framing"));
    }
    if head.health {
        return Ok(Prepared::Health);
    }
    body_buffer(wire, end, head.length)?;
    let mut filled = prefix;
    while filled < head.length {
        filled += read(peer, &mut wire[filled..], until)?;
    }
    request::body(kind, &head.media, wire)
}

fn body_buffer(wire: &mut Vec<u8>, end: usize, length: usize) -> Result<(), Error> {
    // Reuse the authorized header allocation for the body, including coalesced
    // bytes. Large audio intake then retains one wire buffer, and reading into
    // its final slice avoids a second per-read staging copy.
    wire.copy_within(end.., 0);
    wire.truncate(wire.len() - end);
    wire.try_reserve_exact(length - wire.len())
        .map_err(|_| Error::new(503, "allocation_failed"))?;
    wire.resize(length, 0);
    Ok(())
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
        .array_windows()
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

// One fixed payload buffer is retained across every native callback. Chunk
// framing occupies its spare edges, so each payload needs one write_all call.
const CHUNK_PREFIX: usize = 2 * size_of::<usize>() + 2;

struct PcmFrame {
    bytes: Box<[u8]>,
}

impl PcmFrame {
    fn new() -> Self {
        Self {
            bytes: vec![0; CHUNK_PREFIX + wave::PCM_BYTES + 2].into_boxed_slice(),
        }
    }

    fn encode(&mut self, samples: &[f32]) -> &[u8] {
        let length = wave::pcm_into(samples, &mut self.bytes[CHUNK_PREFIX..]).len();
        self.bytes[CHUNK_PREFIX - 2..CHUNK_PREFIX].copy_from_slice(b"\r\n");
        let mut first = CHUNK_PREFIX - 2;
        let mut size = length;
        while size != 0 {
            first -= 1;
            self.bytes[first] = b"0123456789abcdef"[size & 0xf];
            size >>= 4;
        }
        let end = CHUNK_PREFIX + length;
        self.bytes[end..end + 2].copy_from_slice(b"\r\n");
        &self.bytes[first..end + 2]
    }
}

fn response_head(status: u16, media: &str, length: usize) -> String {
    format!(
        "HTTP/1.1 {status} Response\r\nContent-Type: {media}\r\nContent-Length: {length}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\r\n"
    )
}

fn send_wave(peer: &mut TcpStream, samples: &[f32], header: &[u8; 44]) -> io::Result<()> {
    let until = Instant::now() + WRITE;
    let head = response_head(200, "audio/wav", header.len() + samples.len() * 2);
    write_before(peer, head.as_bytes(), until)?;
    write_before(peer, header, until)?;
    write_pcm(samples, |bytes| write_before(peer, bytes, until))
}

fn write_pcm(samples: &[f32], mut write: impl FnMut(&[u8]) -> io::Result<()>) -> io::Result<()> {
    let mut bytes = vec![0; wave::PCM_BYTES].into_boxed_slice();
    for samples in samples.chunks(wave::PCM_SAMPLES) {
        write(wave::pcm_into(samples, &mut bytes))?;
    }
    Ok(())
}

fn send(peer: &mut TcpStream, status: u16, media: &str, bytes: &[u8]) -> io::Result<()> {
    let until = Instant::now() + WRITE;
    let head = response_head(status, media, bytes.len());
    write_before(peer, head.as_bytes(), until)?;
    write_before(peer, bytes, until)
}

fn write_before(peer: &mut TcpStream, mut remaining: &[u8], until: Instant) -> io::Result<()> {
    while !remaining.is_empty() {
        let timeout = until
            .checked_duration_since(Instant::now())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Speech response deadline"))?;
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
    Ok(())
}

#[cfg(test)]
mod intake_tests {
    use super::{
        Error, HeaderEnd, Kind, MAX_HEAD, RETAINED_WIRE_BYTES, Scratch, body_buffer, parse_head,
    };

    #[test]
    fn intake_scratch_reuses_small_requests_and_releases_large_uploads() {
        let mut scratch = Scratch {
            wire: vec![b'x'; MAX_HEAD],
        };
        let storage = scratch.wire.as_ptr();
        scratch.recycle();
        assert!(scratch.wire.is_empty());
        assert_eq!(scratch.wire.as_ptr(), storage);
        scratch.wire.resize(RETAINED_WIRE_BYTES + 1, b'x');
        scratch.recycle();
        assert!(scratch.wire.is_empty());
        assert_eq!(scratch.wire.capacity(), 0);
    }

    #[test]
    fn fragmented_headers_scan_each_window_once_and_preserve_coalesced_audio() {
        let mut head = vec![b'x'; MAX_HEAD - 4];
        head.extend_from_slice(b"\r\n\r\n");
        for fragment in [1, 2, 3, 7, 4096, MAX_HEAD] {
            let mut scan = HeaderEnd::default();
            let mut received = Vec::new();
            for chunk in head.chunks(fragment) {
                received.extend_from_slice(chunk);
                let expected = (received.len() == head.len()).then_some(head.len());
                assert_eq!(scan.find(&received), expected);
            }
            assert_eq!(scan.comparisons, head.len() - 3);
        }
        for split in 1..=4 {
            let mut scan = HeaderEnd::default();
            assert_eq!(scan.find(&head[..head.len() - split]), None);
            let mut wire = head.clone();
            wire.extend_from_slice(b"\0\x80\xff\x7f\r\n\r\n");
            assert_eq!(scan.find(&wire), Some(head.len()));
            assert_eq!(&wire[head.len()..], b"\0\x80\xff\x7f\r\n\r\n");
            assert_eq!(scan.comparisons, head.len() - 3);
        }
    }

    #[test]
    fn fragmented_request_headers_keep_exact_limit_framing_and_coalesced_audio() -> Result<(), Error>
    {
        let key = [b'a'; 64];
        let mut head = format!(
            "POST /v1/audio/transcriptions HTTP/1.1\r\nHost: 127.0.0.1:1234\r\nAuthorization: Bearer {}\r\nContent-Length: 4\r\nContent-Type: multipart/form-data; boundary=a\r\nX-Padding: ",
            "a".repeat(64)
        )
        .into_bytes();
        head.resize(MAX_HEAD - 4, b'x');
        head.extend_from_slice(b"\r\n\r\n");
        for split in 1..=4 {
            let mut scan = HeaderEnd::default();
            assert_eq!(scan.find(&head[..MAX_HEAD - split]), None);
            let mut wire = head.clone();
            wire.extend_from_slice(b"\0\x80\xff\x7f");
            let storage = wire.as_ptr();
            let end = scan.find(&wire).ok_or(Error::bad("incomplete_request"))?;
            assert_eq!(end, MAX_HEAD);
            let parsed = parse_head(&wire[..end], 1234, &key, Kind::Canary)?;
            assert!(!parsed.health);
            assert_eq!(parsed.length, 4);
            assert_eq!(parsed.media, "multipart/form-data; boundary=a");
            body_buffer(&mut wire, end, parsed.length)?;
            assert_eq!(wire, b"\0\x80\xff\x7f");
            assert_eq!(wire.as_ptr(), storage);
        }
        Ok(())
    }

    #[test]
    fn memory_body_intake_reuses_headers_and_preserves_coalesced_audio() -> Result<(), Error> {
        let mut wire = Vec::with_capacity(MAX_HEAD);
        wire.extend_from_slice(b"headers\r\n\r\n\0\x80\xff\x7f");
        let storage = wire.as_ptr();
        let capacity = wire.capacity();
        body_buffer(&mut wire, 11, 12)?;
        assert_eq!(wire.as_ptr(), storage);
        assert_eq!(wire.capacity(), capacity);
        assert_eq!(&wire[..4], b"\0\x80\xff\x7f");
        assert_eq!(&wire[4..], &[0; 8]);

        body_buffer(&mut wire, 0, 1024 * 1024)?;
        assert_eq!(wire.len(), 1024 * 1024);
        assert_eq!(&wire[..4], b"\0\x80\xff\x7f");
        assert!(wire[4..].iter().all(|byte| *byte == 0));
        Ok(())
    }
}

#[cfg(test)]
mod pcm_tests {
    use super::{Error, Output, PcmFrame, complete_wave, wave, write_pcm};
    use std::{cell::Cell, io, rc::Rc};

    struct SampleOwner {
        samples: Vec<f32>,
        drops: Rc<Cell<usize>>,
        borrows: Rc<Cell<usize>>,
    }

    impl AsRef<[f32]> for SampleOwner {
        fn as_ref(&self) -> &[f32] {
            assert_eq!(self.drops.get(), 0);
            self.borrows.set(self.borrows.get() + 1);
            &self.samples
        }
    }

    impl Drop for SampleOwner {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }

    #[test]
    fn complete_waveform_borrows_original_storage_and_releases_on_validation_or_write_failure() {
        for case in 0..7 {
            let drops = Rc::new(Cell::new(0));
            let borrows = Rc::new(Cell::new(0));
            let samples = match case {
                1 => vec![0.0, f32::NAN],
                2 => vec![0.0, f32::INFINITY],
                3 => vec![],
                4 => vec![0.0; wave::MAX_SAMPLES + 1],
                _ => vec![-1.0, 0.0, 0.5, 1.0],
            };
            let storage = samples.as_ptr();
            let audio = Output::Waveform {
                samples: SampleOwner {
                    samples,
                    drops: Rc::clone(&drops),
                    borrows: Rc::clone(&borrows),
                },
                sample_rate: if case == 5 { 16_000 } else { 24_000 },
            };
            let mut writes = 0;
            let result = complete_wave(audio, |samples, header| {
                writes += 1;
                assert_eq!(drops.get(), 0, "Original sample owner was released early");
                assert_eq!(samples.as_ptr(), storage, "A full waveform copy was made");
                assert_eq!(&header[40..], &8_u32.to_le_bytes());
                if case == 6 {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "Closed fixture"));
                }
                let mut bytes = [0; 8];
                assert_eq!(
                    wave::pcm_into(samples, &mut bytes),
                    &[1, 128, 0, 0, 0, 64, 255, 127]
                );
                Ok(())
            });
            assert_eq!(borrows.get(), 1);
            assert_eq!(drops.get(), 1);
            assert_eq!(writes, usize::from(case == 0 || case == 6));
            assert_eq!(
                result,
                match case {
                    0 => Ok(()),
                    6 => Err(Error::new(0, "response_failed")),
                    _ => Err(Error::new(502, "invalid_audio_output")),
                }
            );
        }
    }

    #[test]
    fn complete_pcm_reuses_one_bounded_buffer_and_stops_on_write_failure() -> io::Result<()> {
        let samples = vec![0.5; 2_880_000];
        let mut first = None;
        let mut writes = 0;
        let mut total = 0;
        write_pcm(&samples, |bytes| {
            assert!(bytes.len() <= wave::PCM_BYTES);
            assert!(
                bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|sample| *sample == [0, 64])
            );
            assert_eq!(*first.get_or_insert(bytes.as_ptr()), bytes.as_ptr());
            writes += 1;
            total += bytes.len();
            Ok(())
        })?;
        assert_eq!(writes, samples.len().div_ceil(wave::PCM_SAMPLES));
        assert_eq!(total, samples.len() * 2);
        assert_eq!(wave::PCM_BYTES, 32_768);
        let mut attempted = 0;
        let result = write_pcm(&samples, |_| {
            attempted += 1;
            if attempted == 2 {
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Synthetic closed writer.",
                ))
            } else {
                Ok(())
            }
        });
        assert_eq!(
            result.err().map(|error| error.kind()),
            Some(io::ErrorKind::BrokenPipe)
        );
        assert_eq!(attempted, 2);
        Ok(())
    }

    #[test]
    fn streaming_pcm_reuses_one_frame_across_hex_boundaries_and_shorter_tails() {
        let mut frame = PcmFrame::new();
        let storage = frame.bytes.as_ptr();
        let samples = vec![0.5; wave::PCM_SAMPLES];
        for count in [
            1,
            7,
            8,
            127,
            128,
            wave::PCM_SAMPLES - 1,
            wave::PCM_SAMPLES,
            1,
        ] {
            let encoded = frame.encode(&samples[..count]);
            let prefix = format!("{:x}\r\n", count * 2);
            assert_eq!(&encoded[..prefix.len()], prefix.as_bytes());
            assert_eq!(&encoded[encoded.len() - 2..], b"\r\n");
            let body = &encoded[prefix.len()..encoded.len() - 2];
            assert_eq!(body.len(), count * 2);
            assert!(
                body.as_chunks::<2>()
                    .0
                    .iter()
                    .all(|sample| *sample == [0, 64])
            );
            assert_eq!(frame.bytes.as_ptr(), storage);
        }
    }
}
