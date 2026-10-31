//! Complete canonical PCM input and output; never opens an audio device.

use crate::http::Error;

pub(super) fn decode<'a>(bytes: &[u8], samples: &'a mut Vec<f32>) -> Result<&'a [f32], Error> {
    samples.clear();
    let pcm = bonsai_audio::mono_pcm16_data(bytes, 16_000, 16_000 * 30)
        .ok_or_else(|| Error::bad("invalid_audio"))?;
    samples
        .try_reserve_exact(pcm.len() / 2)
        .map_err(|_| Error::new(503, "allocation_failed"))?;
    samples.extend(
        pcm.as_chunks::<2>()
            .0
            .iter()
            .map(|sample| f32::from(i16::from_le_bytes([sample[0], sample[1]])) / 32768.0),
    );
    Ok(samples)
}

/// Fixed conversion payload, independent of the duration of an admitted clip.
pub(super) const PCM_SAMPLES: usize = 16_384;
pub(super) const PCM_BYTES: usize = PCM_SAMPLES * 2;

pub(super) const MAX_SAMPLES: usize = 2_880_000;

// Validate all output before a caller can commit successful response headers.
pub(super) fn header(samples: &[f32], sample_rate: u32) -> Result<[u8; 44], Error> {
    if sample_rate != 24_000 || bonsai_audio::finite_sample_count(samples, 0, MAX_SAMPLES).is_none()
    {
        return Err(Error::new(502, "invalid_audio_output"));
    }
    bonsai_audio::mono_pcm16_header(sample_rate, samples.len())
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
    use super::{decode, header, pcm_into};
    use crate::{Audio, http::Error};

    #[test]
    fn recognition_decoding_reuses_storage_and_clears_failed_clips() -> Result<(), Error> {
        let mut wave = bonsai_audio::mono_pcm16_header(16_000, 4)
            .ok_or(Error::bad("invalid_fixture"))?
            .to_vec();
        for sample in [i16::MIN, -1, 0, i16::MAX] {
            wave.extend_from_slice(&sample.to_le_bytes());
        }
        let mut samples = Vec::with_capacity(16);
        let storage = samples.as_ptr();
        for _ in 0..3 {
            let decoded = decode(&wave, &mut samples)?;
            assert_eq!(decoded.as_ptr(), storage);
            assert_eq!(decoded, &[-1.0, -1.0 / 32768.0, 0.0, 32767.0 / 32768.0]);
        }
        wave[40..44].copy_from_slice(&7_u32.to_le_bytes());
        assert_eq!(
            decode(&wave, &mut samples).err(),
            Some(Error::bad("invalid_audio"))
        );
        assert_eq!(samples.len(), 0);
        assert_eq!(samples.as_ptr(), storage);
        wave[40..44].copy_from_slice(&8_u32.to_le_bytes());
        assert_eq!(decode(&wave, &mut samples)?.len(), 4);
        Ok(())
    }

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
        let bytes = header(&audio.samples, audio.sample_rate).ok();
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
        assert!(header(&audio.samples, audio.sample_rate).is_err());
        audio.samples[2_879_999] = f32::INFINITY;
        assert!(header(&audio.samples, audio.sample_rate).is_err());
        audio.samples[2_879_999] = 0.0;
        audio.samples.push(0.0);
        assert!(header(&audio.samples, audio.sample_rate).is_err());
        audio.samples.clear();
        assert!(header(&audio.samples, audio.sample_rate).is_err());
    }
}
