//! Canonical mono PCM16 WAV framing shared by speech clients and companions.
//!
//! This codec accepts only a 44-byte PCM header followed by nonempty, complete
//! little-endian samples. Callers select the sample rate and maximum sample
//! count; optional chunks and extensible formats need explicit normalization
//! before entering this boundary. Audio stays borrowed and no devices are opened.
#![forbid(unsafe_code)]

/// Returns the borrowed PCM payload of a canonical, bounded mono PCM16 WAV.
///
/// Rejects mismatched rates, lengths, channel/sample formats, empty or partial
/// samples, extra chunks and trailing data. `maximum_samples` counts individual
/// mono samples, independently of their two-byte representation.
#[must_use]
pub fn mono_pcm16_data(wave: &[u8], sample_rate: u32, maximum_samples: u32) -> Option<&[u8]> {
    let samples = wave.len().checked_sub(44)? / 2;
    if samples == 0 || u32::try_from(samples).ok()? > maximum_samples {
        return None;
    }
    let expected = mono_pcm16_header(sample_rate, samples)?;
    (wave.len() == samples * 2 + 44 && wave[..44] == expected).then_some(&wave[44..])
}

/// Builds the exact 44-byte header for a nonempty mono PCM16 WAV clip.
///
/// Returns `None` for a zero rate/count, an unrepresentable byte rate or a clip
/// whose RIFF length cannot fit the format's 32-bit field. The caller owns
/// duration admission and validation of sample values before sending the header.
#[must_use]
pub fn mono_pcm16_header(sample_rate: u32, samples: usize) -> Option<[u8; 44]> {
    if sample_rate == 0 || samples == 0 {
        return None;
    }
    let byte_rate = sample_rate.checked_mul(2)?;
    let length = u32::try_from(samples).ok()?.checked_mul(2)?;
    let riff_length = length.checked_add(36)?;
    let mut bytes = [0; 44];
    bytes[..4].copy_from_slice(b"RIFF");
    bytes[4..8].copy_from_slice(&riff_length.to_le_bytes());
    bytes[8..16].copy_from_slice(b"WAVEfmt ");
    bytes[16..20].copy_from_slice(&16_u32.to_le_bytes());
    bytes[20..22].copy_from_slice(&1_u16.to_le_bytes());
    bytes[22..24].copy_from_slice(&1_u16.to_le_bytes());
    bytes[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    bytes[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    bytes[32..34].copy_from_slice(&2_u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&16_u16.to_le_bytes());
    bytes[36..40].copy_from_slice(b"data");
    bytes[40..44].copy_from_slice(&length.to_le_bytes());
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::{mono_pcm16_data, mono_pcm16_header};

    // Independent wire fixture keeps header generation and validation honest.
    fn fixture(rate: u32) -> Vec<u8> {
        let mut wave = b"RIFF\x28\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0".to_vec();
        wave.extend_from_slice(&rate.to_le_bytes());
        wave.extend_from_slice(&(rate * 2).to_le_bytes());
        wave.extend_from_slice(b"\x02\0\x10\0data\x04\0\0\0\0\x80\xff\x7f");
        wave
    }

    #[test]
    fn speech_formats_preserve_exact_headers_payloads_and_bounds() {
        for (rate, maximum) in [(16_000, 480_000), (24_000, 2_880_000)] {
            let wave = fixture(rate);
            assert_eq!(
                mono_pcm16_header(rate, 2).as_ref(),
                wave[..44].try_into().ok()
            );
            assert_eq!(mono_pcm16_data(&wave, rate, maximum), Some(&wave[44..]));
            assert!(mono_pcm16_data(&wave, rate, 1).is_none());
            assert!(mono_pcm16_data(&wave, rate + 1, maximum).is_none());
            let mut maximum_wave = Vec::new();
            let count = usize::try_from(maximum).unwrap_or_default();
            maximum_wave.extend_from_slice(&mono_pcm16_header(rate, count).unwrap_or([0; 44]));
            maximum_wave.resize(count * 2 + 44, 0);
            assert!(mono_pcm16_data(&maximum_wave, rate, maximum).is_some());
            assert!(mono_pcm16_data(&maximum_wave, rate, maximum - 1).is_none());
        }
    }

    #[test]
    fn malformed_wave_headers_truncation_and_trailing_data_are_rejected() {
        let wave = fixture(16_000);
        for length in 0..wave.len() {
            assert!(mono_pcm16_data(&wave[..length], 16_000, 480_000).is_none());
        }
        for offset in 0..44 {
            let mut invalid = wave.clone();
            invalid[offset] ^= 1;
            assert!(mono_pcm16_data(&invalid, 16_000, 480_000).is_none());
        }
        let mut invalid = wave;
        invalid.extend_from_slice(b"JUNK\0\0\0\0");
        assert!(mono_pcm16_data(&invalid, 16_000, 480_000).is_none());
        let wave = fixture(16_000);
        let mut empty = wave[..44].to_vec();
        empty[4..8].copy_from_slice(&36_u32.to_le_bytes());
        empty[40..44].copy_from_slice(&0_u32.to_le_bytes());
        let mut duplicated_data = wave.clone();
        duplicated_data.extend_from_slice(b"data\x02\0\0\0\0\0");
        duplicated_data[4..8].copy_from_slice(&50_u32.to_le_bytes());
        let mut reordered = b"RIFF\x28\0\0\0WAVEdata\x04\0\0\0".to_vec();
        reordered.extend_from_slice(&wave[44..]);
        reordered.extend_from_slice(&wave[12..36]);
        let mut padded_chunk = wave[..36].to_vec();
        padded_chunk.extend_from_slice(b"JUNK\x01\0\0\0x\0");
        padded_chunk.extend_from_slice(&wave[36..]);
        padded_chunk[4..8].copy_from_slice(&50_u32.to_le_bytes());
        let mut odd_data = wave;
        odd_data[4..8].copy_from_slice(&39_u32.to_le_bytes());
        odd_data[40..44].copy_from_slice(&3_u32.to_le_bytes());
        odd_data.pop();
        for invalid in [empty, duplicated_data, reordered, padded_chunk, odd_data] {
            assert!(mono_pcm16_data(&invalid, 16_000, 480_000).is_none());
        }
        assert!(mono_pcm16_header(0, 1).is_none());
        assert!(mono_pcm16_header(16_000, 0).is_none());
        assert!(mono_pcm16_header(u32::MAX, 1).is_none());
        assert!(mono_pcm16_header(16_000, usize::MAX).is_none());
        assert!(mono_pcm16_header(16_000, 2_147_483_630).is_none());
    }
}
