//! Strict JSON and multipart supported subsets; all payloads stay in memory.

use crate::{CANARY_MODEL, Kind, Request, TTS_MODEL, http::Error, wave};
use serde::Deserialize;
use std::collections::HashMap;

pub(super) enum Prepared {
    Health,
    Transcribe(Vec<f32>),
    Synthesize(Speech),
}

impl Prepared {
    pub(super) fn streaming_pcm(&self) -> bool {
        matches!(self, Self::Synthesize(speech) if speech.response_format == "pcm")
    }

    pub(super) fn engine_request(&self) -> Option<Request<'_>> {
        match self {
            Self::Health => None,
            Self::Transcribe(samples) => Some(Request::Transcribe {
                samples,
                sample_rate: 16_000,
            }),
            Self::Synthesize(speech) => Some(Request::Synthesize {
                text: &speech.input,
                voice_description: &speech.voice_description,
                language: &speech.language,
            }),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Speech {
    model: String,
    input: String,
    voice_description: String,
    language: String,
    response_format: String,
}

pub(super) fn body(kind: Kind, media: &str, bytes: &[u8]) -> Result<Prepared, Error> {
    match kind {
        Kind::Canary => multipart(media, bytes).map(Prepared::Transcribe),
        Kind::QwenTts => {
            if !media.eq_ignore_ascii_case("application/json")
                && !media.eq_ignore_ascii_case("application/json; charset=utf-8")
            {
                return Err(Error::new(415, "unsupported_media_type"));
            }
            let speech: Speech =
                serde_json::from_slice(bytes).map_err(|_| Error::bad("invalid_json"))?;
            if speech.model != TTS_MODEL {
                return Err(Error::bad("unsupported_model"));
            }
            if !matches!(speech.response_format.as_str(), "wav" | "pcm") {
                return Err(Error::bad("unsupported_audio_format"));
            }
            if speech.language != "Auto" {
                return Err(Error::bad("unsupported_language"));
            }
            if !text(&speech.input, 2048) {
                return Err(Error::bad("text_limit"));
            }
            if !text(&speech.voice_description, 1024) {
                return Err(Error::bad("voice_description_limit"));
            }
            Ok(Prepared::Synthesize(speech))
        }
    }
}

fn text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty()
        && !value.contains('\0')
        && value.chars().take(limit + 1).count() <= limit
}

fn boundary(media: &str) -> Result<&str, Error> {
    let (name, parameter) = media
        .split_once(';')
        .ok_or(Error::new(415, "unsupported_media_type"))?;
    if !name.trim_end().eq_ignore_ascii_case("multipart/form-data") {
        return Err(Error::new(415, "unsupported_media_type"));
    }
    let (name, value) = parameter
        .trim()
        .split_once('=')
        .ok_or(Error::new(415, "unsupported_media_type"))?;
    if !name.eq_ignore_ascii_case("boundary") {
        return Err(Error::new(415, "unsupported_media_type"));
    }
    let value = if let Some(quoted) = value.strip_prefix('"') {
        quoted
            .strip_suffix('"')
            .ok_or(Error::new(415, "unsupported_media_type"))?
    } else {
        value
    };
    if value.is_empty()
        || value.len() > 70
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(Error::new(415, "unsupported_media_type"));
    }
    Ok(value)
}

pub(super) fn split_bytes<'a>(bytes: &'a [u8], delimiter: &[u8]) -> Vec<&'a [u8]> {
    let mut result = Vec::new();
    let mut remaining = bytes;
    while let Some(index) = remaining
        .windows(delimiter.len())
        .position(|window| window == delimiter)
    {
        result.push(&remaining[..index]);
        remaining = &remaining[index + delimiter.len()..];
    }
    result.push(remaining);
    result
}

fn multipart(media: &str, body: &[u8]) -> Result<Vec<f32>, Error> {
    let boundary = boundary(media)?;
    let start = format!("--{boundary}\r\n");
    let end = format!("\r\n--{boundary}--\r\n");
    let separator = format!("\r\n--{boundary}\r\n");
    let interior = body
        .strip_prefix(start.as_bytes())
        .and_then(|value| value.strip_suffix(end.as_bytes()))
        .ok_or(Error::bad("invalid_multipart"))?;
    let parts = split_bytes(interior, separator.as_bytes());
    if !(2..=3).contains(&parts.len()) {
        return Err(Error::bad("invalid_multipart"));
    }
    let mut fields = HashMap::new();
    for part in parts {
        let at = part
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or(Error::bad("invalid_multipart"))?;
        if at > 1024 {
            return Err(Error::bad("invalid_multipart"));
        }
        let mut headers = HashMap::new();
        for line in split_bytes(&part[..at], b"\r\n") {
            let colon = line
                .iter()
                .position(|byte| *byte == b':')
                .ok_or(Error::bad("invalid_multipart"))?;
            let name = std::str::from_utf8(&line[..colon])
                .map_err(|_| Error::bad("invalid_multipart"))?
                .to_ascii_lowercase();
            if !matches!(name.as_str(), "content-disposition" | "content-type")
                || headers
                    .insert(name, line[colon + 1..].trim_ascii())
                    .is_some()
            {
                return Err(Error::bad("invalid_multipart"));
            }
        }
        let disposition = headers
            .get("content-disposition")
            .ok_or(Error::bad("invalid_multipart"))?;
        let (name, filename) = disposition_field(disposition)?;
        if name == "file" {
            if !filename
                || !headers.get("content-type").is_some_and(|value| {
                    [
                        b"audio/wav".as_slice(),
                        b"audio/x-wav",
                        b"application/octet-stream",
                    ]
                    .contains(value)
                })
            {
                return Err(Error::bad("invalid_audio"));
            }
        } else if filename || headers.contains_key("content-type") {
            return Err(Error::bad("invalid_multipart"));
        }
        if fields.insert(name, &part[at + 4..]).is_some() {
            return Err(Error::bad("invalid_multipart"));
        }
    }
    if fields.get("model").copied() != Some(CANARY_MODEL.as_bytes()) {
        return Err(Error::bad("unsupported_model"));
    }
    if fields.get("language").is_some_and(|value| *value != b"en") {
        return Err(Error::bad("unsupported_language"));
    }
    let audio = fields.get("file").ok_or(Error::bad("invalid_audio"))?;
    wave::decode(audio)
}

fn disposition_field(bytes: &[u8]) -> Result<(&str, bool), Error> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::bad("invalid_multipart"))?;
    let value = text
        .strip_prefix("form-data;")
        .ok_or(Error::bad("invalid_multipart"))?
        .trim_start();
    let value = value
        .strip_prefix("name=\"")
        .ok_or(Error::bad("invalid_multipart"))?;
    let (name, rest) = value
        .split_once('"')
        .ok_or(Error::bad("invalid_multipart"))?;
    if !matches!(name, "model" | "language" | "file") {
        return Err(Error::bad("invalid_multipart"));
    }
    if rest.is_empty() {
        return Ok((name, false));
    }
    let value = rest
        .strip_prefix(';')
        .ok_or(Error::bad("invalid_multipart"))?
        .trim_start();
    let filename = value
        .strip_prefix("filename=\"")
        .and_then(|value| value.strip_suffix('"'))
        .ok_or(Error::bad("invalid_multipart"))?;
    if filename.is_empty() || filename.len() > 128 || filename.contains(['"', '\r', '\n', '\0']) {
        return Err(Error::bad("invalid_multipart"));
    }
    Ok((name, true))
}
