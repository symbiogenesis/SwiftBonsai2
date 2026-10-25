//! Pinned deterministic reference K-block row quantizers.

#![allow(
    clippy::float_cmp,
    clippy::similar_names,
    clippy::suspicious_operation_groupings,
    reason = "Pinned scale fitting retains exact-zero branches, native normal-equation names and arithmetic products."
)]

use super::super::{finite, half_bits};
use super::{block_bytes, scale_min};
use crate::{Result, invalid};
use bonsai_ternary::half_to_float;

fn nearest(value: f32) -> Result<i32> {
    if !value.is_finite() || value.abs() > 4_194_303.0 {
        return Err(invalid(
            "K quantizer integer conversion outside native range",
        ));
    }
    Ok(((value + 12_582_912.0).to_bits() & 0x007f_ffff).cast_signed() - 0x0040_0000)
}

fn half(value: f32) -> Result<u16> {
    finite(value)?;
    let bits = half_bits(value);
    finite(half_to_float(bits))?;
    Ok(bits)
}

fn signed_maximum(input: &[f32]) -> f32 {
    let mut maximum = 0.0_f32;
    let mut magnitude = 0.0_f32;
    for &value in input {
        if value.abs() > magnitude {
            maximum = value;
            magnitude = value.abs();
        }
    }
    maximum
}

#[allow(
    clippy::too_many_arguments,
    reason = "Preserve every pinned affine quantizer search parameter."
)]
fn affine(
    input: &[f32],
    weights: &[f32],
    nmax: i32,
    rmin: f32,
    steps: usize,
    mad: bool,
) -> Result<(f32, f32, Vec<u8>)> {
    let mut minimum = input[0];
    let mut maximum = input[0];
    let mut sum_w = weights[0];
    let mut sum_x = sum_w * input[0];
    for (&value, &weight) in input[1..].iter().zip(&weights[1..]) {
        if value < minimum {
            minimum = value;
        }
        if value > maximum {
            maximum = value;
        }
        sum_w += weight;
        sum_x += weight * value;
    }
    if minimum > 0.0 {
        minimum = 0.0;
    }
    let mut codes = vec![0; input.len()];
    if maximum == minimum {
        return Ok((0.0, -minimum, codes));
    }
    let inverse = nmax as f32 / (maximum - minimum);
    let mut scale = 1.0 / inverse;
    let mut best_error = 0.0_f32;
    for ((&value, &weight), code) in input.iter().zip(weights).zip(&mut codes) {
        *code = nearest(inverse * (value - minimum))?.clamp(0, nmax) as u8;
        let difference = scale * f32::from(*code) + minimum - value;
        best_error += weight
            * if mad {
                difference.abs()
            } else {
                difference * difference
            };
    }
    let mut candidate = vec![0; input.len()];
    for step in 0..=steps {
        let inverse = (rmin + 0.1 * step as f32 + nmax as f32) / (maximum - minimum);
        let mut sum_l = 0.0_f32;
        let mut sum_l2 = 0.0_f32;
        let mut sum_xl = 0.0_f32;
        for ((&value, &weight), code) in input.iter().zip(weights).zip(&mut candidate) {
            *code = nearest(inverse * (value - minimum))?.clamp(0, nmax) as u8;
            let level = f32::from(*code);
            sum_l += weight * level;
            sum_l2 += weight * level * level;
            sum_xl += weight * level * value;
        }
        let determinant = sum_w * sum_l2 - sum_l * sum_l;
        if determinant <= 0.0 {
            continue;
        }
        let mut next_scale = (sum_w * sum_xl - sum_x * sum_l) / determinant;
        let mut next_minimum = (sum_l2 * sum_x - sum_l * sum_xl) / determinant;
        if next_minimum > 0.0 {
            next_minimum = 0.0;
            next_scale = sum_xl / sum_l2;
        }
        let mut error = 0.0_f32;
        for ((&value, &weight), &code) in input.iter().zip(weights).zip(&candidate) {
            let difference = next_scale * f32::from(code) + next_minimum - value;
            error += weight
                * if mad {
                    difference.abs()
                } else {
                    difference * difference
                };
        }
        if error < best_error {
            codes.copy_from_slice(&candidate);
            best_error = error;
            scale = next_scale;
            minimum = next_minimum;
        }
    }
    Ok((finite(scale)?, finite(-minimum)?, codes))
}

fn symmetric(input: &[f32], nmax: i32, three: bool) -> Result<(f32, Vec<u8>)> {
    let maximum = signed_maximum(input);
    let mut levels = vec![0_i32; input.len()];
    if maximum.abs() < 1.0e-15 {
        return Ok((0.0, vec![0; input.len()]));
    }
    let inverse = -(nmax as f32) / maximum;
    let mut sum_lx = 0.0_f32;
    let mut sum_l2 = 0.0_f32;
    for (&value, level) in input.iter().zip(&mut levels) {
        *level = nearest(inverse * value)?.clamp(-nmax, nmax - 1);
        let weight = value * value;
        sum_lx += weight * value * *level as f32;
        sum_l2 += weight * *level as f32 * *level as f32;
    }
    if three {
        for _ in 0..5 {
            let mut changed = false;
            for (&value, level) in input.iter().zip(&mut levels) {
                let weight = value * value;
                let mut next_lx = sum_lx - weight * value * *level as f32;
                if next_lx <= 0.0 {
                    continue;
                }
                let mut next_l2 = sum_l2 - weight * *level as f32 * *level as f32;
                let next_level = nearest(value * next_l2 / next_lx)?.clamp(-nmax, nmax - 1);
                if next_level == *level {
                    continue;
                }
                next_lx += weight * value * next_level as f32;
                next_l2 += weight * next_level as f32 * next_level as f32;
                if next_l2 > 0.0 && next_lx * next_lx * sum_l2 > sum_lx * sum_lx * next_l2 {
                    *level = next_level;
                    sum_lx = next_lx;
                    sum_l2 = next_l2;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let scale = if sum_l2 > 0.0 { sum_lx / sum_l2 } else { 0.0 };
        return Ok((
            finite(scale)?,
            levels.iter().map(|level| (level + nmax) as u8).collect(),
        ));
    }
    let mut scale = if sum_l2 == 0.0 { 0.0 } else { sum_lx / sum_l2 };
    let mut best = scale * sum_lx;
    for step in -9..=9 {
        if step == 0 {
            continue;
        }
        let inverse = -(nmax as f32 + 0.1 * step as f32) / maximum;
        let mut sum_lx = 0.0_f32;
        let mut sum_l2 = 0.0_f32;
        for &value in input {
            let level = nearest(inverse * value)?.clamp(-nmax, nmax - 1) as f32;
            let weight = value * value;
            sum_lx += weight * value * level;
            sum_l2 += weight * level * level;
        }
        if sum_l2 > 0.0 && sum_lx * sum_lx > best * sum_l2 {
            for (&value, level) in input.iter().zip(&mut levels) {
                *level = nearest(inverse * value)?.clamp(-nmax, nmax - 1);
            }
            scale = sum_lx / sum_l2;
            best = scale * sum_lx;
        }
    }
    Ok((
        finite(scale)?,
        levels.iter().map(|level| (level + nmax) as u8).collect(),
    ))
}

fn pack_affine(
    kind: u32,
    input: &[f32],
    codes: &mut [u8; 256],
    scales: &[f32; 16],
    minima: &[f32; 16],
    output: &mut [u8],
) -> Result<()> {
    let groups = if kind == 10 { 16 } else { 8 };
    let group_size = 256 / groups;
    let limit = if kind == 10 { 15.0 } else { 63.0 };
    let mut max_scale = 0.0_f32;
    let mut max_minimum = 0.0_f32;
    for group in 0..groups {
        if scales[group] > max_scale {
            max_scale = scales[group];
        }
        if minima[group] > max_minimum {
            max_minimum = minima[group];
        }
    }
    let scale_offset = if kind == 10 { 0 } else { 4 };
    let inverse_scale = if max_scale > 0.0 {
        limit / max_scale
    } else {
        0.0
    };
    let inverse_minimum = if max_minimum > 0.0 {
        limit / max_minimum
    } else {
        0.0
    };
    for group in 0..groups {
        let scale = nearest(inverse_scale * scales[group])?.min(limit as i32) as u8;
        let minimum = nearest(inverse_minimum * minima[group])?.min(limit as i32) as u8;
        if kind == 10 {
            output[group] = scale | (minimum << 4);
        } else if group < 4 {
            output[4 + group] = scale;
            output[8 + group] = minimum;
        } else {
            output[8 + group] = (scale & 15) | ((minimum & 15) << 4);
            output[group] |= (scale >> 4) << 6;
            output[4 + group] |= (minimum >> 4) << 6;
        }
    }
    let d = half(max_scale / limit)?;
    let m = half(max_minimum / limit)?;
    let d_offset = if kind == 10 { 80 } else { 0 };
    output[d_offset..d_offset + 2].copy_from_slice(&d.to_le_bytes());
    output[d_offset + 2..d_offset + 4].copy_from_slice(&m.to_le_bytes());
    let max_code = match kind {
        10 => 3,
        12 => 15,
        _ => 31,
    };
    for group in 0..groups {
        let (scale, minimum) = if kind == 10 {
            (i16::from(output[group] & 15), i16::from(output[group] >> 4))
        } else {
            scale_min(group, &output[scale_offset..scale_offset + 12])
        };
        let scale = half_to_float(d) * f32::from(scale);
        if scale == 0.0 {
            continue;
        }
        let minimum = half_to_float(m) * f32::from(minimum);
        for index in group * group_size..(group + 1) * group_size {
            codes[index] = nearest((input[index] + minimum) / scale)?.clamp(0, max_code) as u8;
        }
    }
    for (index, &code) in codes.iter().enumerate() {
        if kind == 10 {
            output[16 + index / 128 * 32 + index % 32] |= code << (2 * (index % 128 / 32));
        } else {
            let offset = if kind == 12 { 16 } else { 48 };
            output[offset + index / 64 * 32 + index % 32] |= (code & 15) << (4 * (index % 64 / 32));
            if kind == 13 && code > 15 {
                output[16 + index % 32] |= 1 << (index / 32);
            }
        }
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep all native scale/code/bit-packing phases in one source-ordered dispatcher."
)]
pub(super) fn quantize(kind: u32, input: &[f32]) -> Result<Vec<u8>> {
    let size = block_bytes(kind).ok_or_else(|| invalid("unknown K quant type"))?;
    if input.is_empty()
        || !input.len().is_multiple_of(256)
        || input.iter().any(|value| !value.is_finite())
    {
        return Err(invalid("K quantizer requires complete finite blocks"));
    }
    let mut output = vec![0_u8; input.len() / 256 * size];
    for (input, output) in input
        .as_chunks::<256>()
        .0
        .iter()
        .zip(output.chunks_exact_mut(size))
    {
        let mut codes = [0_u8; 256];
        let mut scales = [0.0_f32; 16];
        let mut minima = [0.0_f32; 16];
        if matches!(kind, 10 | 12 | 13) {
            let group_size = if kind == 10 { 16 } else { 32 };
            let nmax = match kind {
                10 => 3,
                12 => 15,
                _ => 31,
            };
            for (group, values) in input.chunks_exact(group_size).enumerate() {
                let mut squares = 0.0_f32;
                for &value in values {
                    squares += value * value;
                }
                let average = (squares / 32.0).sqrt();
                let weights = values
                    .iter()
                    .map(|value| {
                        if kind == 10 {
                            value.abs()
                        } else {
                            average + value.abs()
                        }
                    })
                    .collect::<Vec<_>>();
                let (scale, minimum, levels) = affine(
                    values,
                    &weights,
                    nmax,
                    if kind == 12 { -1.0 } else { -0.5 },
                    if kind == 12 { 20 } else { 15 },
                    kind == 10,
                )?;
                scales[group] = scale;
                minima[group] = minimum;
                codes[group * group_size..(group + 1) * group_size].copy_from_slice(&levels);
            }
            pack_affine(kind, input, &mut codes, &scales, &minima, output)?;
            continue;
        }
        let mut maximum = 0.0_f32;
        for (group, values) in input.as_chunks::<16>().0.iter().enumerate() {
            let (scale, levels) = symmetric(values, if kind == 11 { 4 } else { 32 }, kind == 11)?;
            scales[group] = scale;
            if scale.abs() > maximum.abs() {
                maximum = scale;
            }
            codes[group * 16..(group + 1) * 16].copy_from_slice(&levels);
        }
        if kind == 14 && maximum.abs() < 1.0e-15 {
            continue;
        }
        let inverse = if maximum == 0.0 {
            0.0
        } else {
            (if kind == 11 { -32.0 } else { -128.0 }) / maximum
        };
        let d = half(if maximum == 0.0 { 0.0 } else { 1.0 / inverse })?;
        let d_offset = if kind == 11 { 108 } else { 208 };
        output[d_offset..d_offset + 2].copy_from_slice(&d.to_le_bytes());
        let mut subscales = [0_i32; 16];
        for group in 0..16 {
            let scale = if kind == 11 {
                nearest(inverse * scales[group])?.clamp(-32, 31)
            } else {
                nearest(inverse * scales[group])?.min(127)
            };
            subscales[group] = scale;
            if kind == 11 {
                let packed = (scale + 32) as u8;
                output[96 + group % 8] |= (packed & 15) << (4 * (group / 8));
                output[104 + group % 4] |= (packed >> 4) << (2 * (group / 4));
            } else {
                output[192 + group] = scale as i8 as u8;
            }
        }
        if kind == 11 && maximum == 0.0 {
            output[96..108].fill(0);
            subscales.fill(-32);
        }
        for (group, &subscale) in subscales.iter().enumerate() {
            let scale = half_to_float(d) * subscale as f32;
            if scale == 0.0 {
                continue;
            }
            let limit = if kind == 11 { 4 } else { 32 };
            for index in group * 16..(group + 1) * 16 {
                codes[index] =
                    (nearest(input[index] / scale)?.clamp(-limit, limit - 1) + limit) as u8;
            }
        }
        for (index, &code) in codes.iter().enumerate() {
            if kind == 11 {
                if code > 3 {
                    output[index % 32] |= 1 << (index / 32);
                }
                output[32 + index / 128 * 32 + index % 32] |=
                    (code & 3) << (2 * (index % 128 / 32));
            } else {
                let part = index % 128 / 32;
                output[index / 128 * 64 + part % 2 * 32 + index % 32] |=
                    (code & 15) << (4 * (part / 2));
                output[128 + index / 128 * 32 + index % 32] |= (code >> 4) << (2 * part);
            }
        }
    }
    Ok(output)
}
