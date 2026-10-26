//! Complete canonical PCM input and output; never opens an audio device.

use crate::{Audio, http::Error};

pub(super) fn decode(bytes: &[u8]) -> Result<Vec<f32>, Error> {
    let pcm = bonsai_audio::mono_pcm16_data(bytes, 16_000, 16_000 * 30)
        .ok_or_else(|| Error::bad("invalid_audio"))?;
    Ok(pcm
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
    bonsai_audio::mono_pcm16_header(audio.sample_rate, audio.samples.len())
        .ok_or_else(|| Error::new(502, "invalid_audio_output"))
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
