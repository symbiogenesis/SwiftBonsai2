//! Complete canonical PCM input and output; never opens an audio device.

use crate::{Audio, http::Error};

pub(super) fn decode(bytes: &[u8]) -> Result<Vec<f32>, Error> {
    if bytes.len() < 46
        || bytes.len() > 960_044
        || &bytes[..4] != b"RIFF"
        || &bytes[8..16] != b"WAVEfmt "
    {
        return Err(Error::bad("invalid_audio"));
    }
    let u16_at = |offset| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
    let u32_at = |offset| {
        u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ])
    };
    if usize::try_from(u32_at(4)).ok() != Some(bytes.len() - 8)
        || u32_at(16) != 16
        || u16_at(20) != 1
        || u16_at(22) != 1
        || u32_at(24) != 16_000
        || u32_at(28) != 32_000
        || u16_at(32) != 2
        || u16_at(34) != 16
        || &bytes[36..40] != b"data"
        || usize::try_from(u32_at(40)).ok() != Some(bytes.len() - 44)
        || (bytes.len() - 44) % 2 != 0
    {
        return Err(Error::bad("invalid_audio"));
    }
    Ok(bytes[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|sample| f32::from(i16::from_le_bytes([sample[0], sample[1]])) / 32768.0)
        .collect())
}

pub(super) fn encode(audio: &Audio) -> Result<Vec<u8>, Error> {
    if audio.sample_rate != 24_000
        || audio.samples.is_empty()
        || audio.samples.len() > 2_880_000
        || audio.samples.iter().any(|sample| !sample.is_finite())
    {
        return Err(Error::new(502, "invalid_audio_output"));
    }
    let length = u32::try_from(audio.samples.len() * 2)
        .map_err(|_| Error::new(502, "invalid_audio_output"))?;
    let mut bytes = Vec::with_capacity(audio.samples.len() * 2 + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(length + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&24_000_u32.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&length.to_le_bytes());
    for sample in &audio.samples {
        bytes.extend_from_slice(&pcm(*sample).to_le_bytes());
    }
    Ok(bytes)
}

#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "Finite samples are explicitly clamped and rounded into the representable PCM16 range."
)]
pub(super) fn pcm(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16
}
