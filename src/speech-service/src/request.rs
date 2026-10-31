//! Strict JSON and multipart supported subsets; all payloads stay in memory.

use crate::{CANARY_MODEL, Request, TTS_MODEL, http::Error, wave};
use serde::Deserialize;
use std::{borrow::Cow, collections::HashMap};

pub(super) enum Prepared<'a> {
    Health,
    Transcribe(&'a [f32]),
    Synthesize(Speech<'a>),
}

impl Prepared<'_> {
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
pub(super) struct Speech<'a> {
    #[serde(borrow)]
    model: Cow<'a, str>,
    #[serde(borrow)]
    input: Cow<'a, str>,
    #[serde(borrow)]
    voice_description: Cow<'a, str>,
    #[serde(borrow)]
    language: Cow<'a, str>,
    #[serde(borrow)]
    response_format: Cow<'a, str>,
}

pub(super) fn synthesize<'a>(media: &str, bytes: &'a [u8]) -> Result<Speech<'a>, Error> {
    if !media.eq_ignore_ascii_case("application/json")
        && !media.eq_ignore_ascii_case("application/json; charset=utf-8")
    {
        return Err(Error::new(415, "unsupported_media_type"));
    }
    let speech: Speech<'_> =
        serde_json::from_slice(bytes).map_err(|_| Error::bad("invalid_json"))?;
    if speech.model != TTS_MODEL {
        return Err(Error::bad("unsupported_model"));
    }
    if !matches!(speech.response_format.as_ref(), "wav" | "pcm") {
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
    Ok(speech)
}

pub(super) fn transcribe<'a>(
    media: &str,
    bytes: &[u8],
    samples: &'a mut Vec<f32>,
) -> Result<&'a [f32], Error> {
    // Multipart errors must not leave samples from an earlier request visible.
    samples.clear();
    wave::decode(multipart(media, bytes)?, samples)
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

fn split_bytes<'a, 'd>(
    bytes: &'a [u8],
    delimiter: &'d [u8],
) -> impl Iterator<Item = &'a [u8]> + use<'a, 'd> {
    let mut remaining = Some(bytes);
    std::iter::from_fn(move || {
        let bytes = remaining.take()?;
        Some(
            bytes
                .windows(delimiter.len())
                .position(|window| window == delimiter)
                .map_or(bytes, |index| {
                    remaining = Some(&bytes[index + delimiter.len()..]);
                    &bytes[..index]
                }),
        )
    })
}

fn multipart_parts<'a>(bytes: &'a [u8], separator: &[u8]) -> Result<[Option<&'a [u8]>; 3], Error> {
    // The protocol allows only model, file and optional language. Do not first
    // collect an attacker-controlled number of delimiter matches into a Vec.
    let mut split = split_bytes(bytes, separator);
    let parts = std::array::from_fn(|_| split.next());
    if parts[1].is_none() || split.next().is_some() {
        return Err(Error::bad("invalid_multipart"));
    }
    Ok(parts)
}

fn multipart<'a>(media: &str, body: &'a [u8]) -> Result<&'a [u8], Error> {
    let boundary = boundary(media)?;
    let start = format!("--{boundary}\r\n");
    let end = format!("\r\n--{boundary}--\r\n");
    let separator = format!("\r\n--{boundary}\r\n");
    let interior = body
        .strip_circumfix(start.as_bytes(), end.as_bytes())
        .ok_or(Error::bad("invalid_multipart"))?;
    let parts = multipart_parts(interior, separator.as_bytes())?;
    let mut fields = HashMap::new();
    for part in parts.into_iter().flatten() {
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
    Ok(audio)
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

#[cfg(test)]
mod tests {
    use super::{
        Cow, Error, TTS_MODEL, multipart, multipart_parts, split_bytes, synthesize, transcribe,
    };

    #[test]
    fn failed_multipart_clears_previous_recognition_samples() {
        let mut samples = vec![0.5; 16];
        assert_eq!(
            transcribe("multipart/form-data; boundary=a", b"invalid", &mut samples).err(),
            Some(Error::bad("invalid_multipart"))
        );
        assert_eq!(samples.len(), 0);
    }

    #[test]
    fn synthesis_fields_borrow_plain_json_and_own_escaped_unicode() -> Result<(), Error> {
        let bytes = format!(
            r#"{{"model":"{TTS_MODEL}","input":"Hello 🌱","voice_description":"Warm and clear.","language":"Auto","response_format":"wav"}}"#
        );
        let speech = synthesize("application/json", bytes.as_bytes())?;
        assert!(matches!(speech.model, Cow::Borrowed(_)));
        assert!(matches!(speech.input, Cow::Borrowed(_)));
        assert!(matches!(speech.voice_description, Cow::Borrowed(_)));
        assert!(matches!(speech.language, Cow::Borrowed(_)));
        assert!(matches!(speech.response_format, Cow::Borrowed(_)));
        assert_eq!(speech.input, "Hello 🌱");
        let escaped = bytes.replace("Hello 🌱", r"Hello \ud83c\udf31\n");
        let speech = synthesize("application/json", escaped.as_bytes())?;
        assert!(matches!(speech.input, Cow::Owned(_)));
        assert_eq!(speech.input, "Hello 🌱\n");
        Ok(())
    }

    #[test]
    fn memory_multipart_parts_borrow_a_fixed_number_of_fields() -> Result<(), Error> {
        let bytes = b"model\r\n--a\r\nfile\r\n--a\r\nlanguage";
        let parts = multipart_parts(bytes, b"\r\n--a\r\n")?;
        assert_eq!(
            parts,
            [
                Some(&b"model"[..]),
                Some(&b"file"[..]),
                Some(&b"language"[..])
            ]
        );
        assert_eq!(parts[0].map(<[u8]>::as_ptr), Some(bytes.as_ptr()));
        assert_eq!(parts[1].map(<[u8]>::as_ptr), Some(bytes[12..].as_ptr()));
        assert_eq!(
            multipart_parts(b"model\r\n--a\r\nfile", b"\r\n--a\r\n")?,
            [Some(&b"model"[..]), Some(&b"file"[..]), None]
        );
        let mut lines = split_bytes(b"\r\nheader\r\n", b"\r\n");
        assert_eq!(lines.next(), Some(&b""[..]));
        assert_eq!(lines.next(), Some(&b"header"[..]));
        assert_eq!(lines.next(), Some(&b""[..]));
        assert_eq!(lines.next(), None);
        assert_eq!(lines.next(), None);
        Ok(())
    }

    #[test]
    fn memory_multipart_field_overflow_is_rejected_before_field_decoding() {
        let mut body = b"--a\r\n".to_vec();
        // Near the admitted wire limit, the old eager split retained over
        // 120,000 borrowed slices before discovering that only three fit.
        for _ in 0..120_000 {
            body.extend_from_slice(b"x\r\n--a\r\n");
        }
        body.extend_from_slice(b"invalid headers\r\n--a--\r\n");
        assert!(body.len() < 1024 * 1024);
        assert_eq!(
            multipart("multipart/form-data; boundary=a", &body).err(),
            Some(Error::bad("invalid_multipart"))
        );
        assert_eq!(
            multipart_parts(b"only one field", b"\r\n--a\r\n").err(),
            Some(Error::bad("invalid_multipart"))
        );
    }
}
