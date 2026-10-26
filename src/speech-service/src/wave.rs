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

/// Fixed conversion payload, independent of the duration of an admitted clip.
pub(super) const PCM_SAMPLES: usize = 16_384;
pub(super) const PCM_BYTES: usize = PCM_SAMPLES * 2;

// Validate all output before a caller can commit successful response headers.
pub(super) fn header(audio: &Audio) -> Result<[u8; 44], Error> {
    if audio.sample_rate != 24_000
        || audio.samples.is_empty()
        || audio.samples.len() > 2_880_000
        || audio.samples.iter().any(|sample| !sample.is_finite())
    {
        return Err(Error::new(502, "invalid_audio_output"));
    }
    let length = u32::try_from(audio.samples.len() * 2)
        .map_err(|_| Error::new(502, "invalid_audio_output"))?;
    let mut bytes = [0; 44];
    bytes[..4].copy_from_slice(b"RIFF");
    bytes[4..8].copy_from_slice(&(length + 36).to_le_bytes());
    bytes[8..16].copy_from_slice(b"WAVEfmt ");
    bytes[16..20].copy_from_slice(&16_u32.to_le_bytes());
    bytes[20..22].copy_from_slice(&1_u16.to_le_bytes());
    bytes[22..24].copy_from_slice(&1_u16.to_le_bytes());
    bytes[24..28].copy_from_slice(&24_000_u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&48_000_u32.to_le_bytes());
    bytes[32..34].copy_from_slice(&2_u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&16_u16.to_le_bytes());
    bytes[36..40].copy_from_slice(b"data");
    bytes[40..44].copy_from_slice(&length.to_le_bytes());
    Ok(bytes)
}

// Callers provide at most PCM_SAMPLES samples and a PCM_BYTES output buffer.
// Reuse that storage for every chunk instead of retaining a second full clip.
pub(super) fn pcm_into<'a>(samples: &[f32], bytes: &'a mut [u8]) -> &'a [u8] {
    let bytes = &mut bytes[..samples.len() * 2];
    for (sample, target) in samples.iter().zip(bytes.as_chunks_mut::<2>().0) {
        *target = pcm(*sample).to_le_bytes();
    }
    bytes
}
#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "Finite samples are explicitly clamped and rounded into the representable PCM16 range."
)]
pub(super) fn pcm(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

#[cfg(test)]
mod tests {
    use super::{Audio, header, pcm_into};

    #[test]
    fn pcm_bytes_preserve_clipping_rounding_and_signed_zero() {
        let samples = [-2.0, -1.0, -0.5, -0.0, 0.0, 0.5, 1.0, 2.0];
        let mut bytes = [0; 16];
        assert_eq!(
            pcm_into(&samples, &mut bytes),
            &[
                1, 128, 1, 128, 0, 192, 0, 0, 0, 0, 0, 64, 255, 127, 255, 127
            ]
        );
        let mut bytes = [0; 4];
        assert_eq!(
            pcm_into(&[-0.5 / 32767.0, 0.5 / 32767.0], &mut bytes),
            &[255, 255, 1, 0]
        );
    }

    #[test]
    fn wave_header_validates_the_complete_clip_and_keeps_exact_lengths() {
        let mut audio = Audio {
            samples: vec![0.0; 2_880_000],
            sample_rate: 24_000,
        };
        let bytes = header(&audio).ok();
        assert_eq!(
            bytes.as_ref().map(|bytes| &bytes[..4]),
            Some(b"RIFF".as_slice())
        );
        assert_eq!(
            bytes.as_ref().map(|bytes| &bytes[4..8]),
            Some(5_760_036_u32.to_le_bytes().as_slice())
        );
        assert_eq!(
            bytes.as_ref().map(|bytes| &bytes[40..44]),
            Some(5_760_000_u32.to_le_bytes().as_slice())
        );
        audio.samples[2_879_999] = f32::NAN;
        assert!(header(&audio).is_err());
        audio.samples[2_879_999] = f32::INFINITY;
        assert!(header(&audio).is_err());
        audio.samples[2_879_999] = 0.0;
        audio.samples.push(0.0);
        assert!(header(&audio).is_err());
        audio.samples.clear();
        assert!(header(&audio).is_err());
    }
}
