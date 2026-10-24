//! Retargeted assertions from two pinned, MIT-licensed Prism test sources.
//!
//! The C++ exporters retain the upstream fixture generators and implementations.
//! Run this identical harness with `--target cpp` before `--target rust` to check
//! both engines against the same fixtures. See tools/prism/upstream/README.md.

#![forbid(unsafe_code)]
#![allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "The upstream assertion deliberately rounds its f64 reference dot to f32."
)]

use std::env;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bonsai_prism_runtime::kernels::{matvec, row};
use bonsai_prism_runtime::model::Tensor;
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const UPSTREAM: &str = "b87ca7225456b06b0975b8239c66b5aecf07e37c";
const PQ2_ID: &str = "tests/test-pq2-row-shapes.cpp";
const PTQ1_ID: &str = "tests/test-ptq1_0-element-map.cpp";
const MAX_FIXTURE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy)]
enum Target {
    Cpp,
    Rust,
}

struct Options {
    target: Target,
    case: Option<String>,
    pq2: Option<PathBuf>,
    ptq1: Option<PathBuf>,
    output: PathBuf,
}

fn invalid(message: impl Into<String>) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into()).into()
}

fn options() -> Result<Options> {
    let mut target = None;
    let mut case = None;
    let mut pq2 = None;
    let mut ptq1 = None;
    let mut output = None;
    let mut arguments = env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| invalid("every option requires a value"))?;
        match argument.to_str() {
            Some("--target") => {
                target = Some(match value.to_str() {
                    Some("cpp") => Target::Cpp,
                    Some("rust") => Target::Rust,
                    _ => return Err(invalid("--target must be cpp or rust")),
                });
            }
            Some("--case") => {
                let name = value
                    .to_str()
                    .ok_or_else(|| invalid("--case must be valid UTF-8"))?;
                case = Some(match name {
                    "test-pq2-row-shapes" | PQ2_ID => PQ2_ID.to_owned(),
                    "test-ptq1_0-element-map" | PTQ1_ID => PTQ1_ID.to_owned(),
                    _ => return Err(invalid("unknown upstream test case")),
                });
            }
            Some("--pq2-fixtures") => pq2 = Some(PathBuf::from(value)),
            Some("--ptq1-fixtures") => ptq1 = Some(PathBuf::from(value)),
            Some("--output") => output = Some(PathBuf::from(value)),
            _ => return Err(invalid("unknown upstream adapter option")),
        }
    }
    let output = output.ok_or_else(|| invalid("--output is required"))?;
    if !output.is_absolute() {
        return Err(invalid("--output must be an absolute path"));
    }
    Ok(Options {
        target: target.ok_or_else(|| invalid("--target is required"))?,
        case,
        pq2,
        ptq1,
        output,
    })
}

struct Fixture {
    bytes: Cursor<Vec<u8>>,
}

impl Fixture {
    fn open(path: &Path, magic: [u8; 8]) -> Result<Self> {
        let metadata = fs::metadata(path)?;
        if metadata.len() > MAX_FIXTURE_BYTES {
            return Err(invalid("upstream fixture exceeds the bounded file size"));
        }
        let mut fixture = Self {
            bytes: Cursor::new(fs::read(path)?),
        };
        if fixture.block(8)? != magic {
            return Err(invalid("upstream fixture magic/version mismatch"));
        }
        Ok(fixture)
    }

    fn block(&mut self, length: usize) -> Result<Vec<u8>> {
        let mut value = vec![0; length];
        self.bytes.read_exact(&mut value)?;
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32> {
        let mut bytes = [0; 4];
        self.bytes.read_exact(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn floats(&mut self, count: usize) -> Result<Vec<f32>> {
        (0..count)
            .map(|_| {
                let value = f32::from_bits(self.u32()?);
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err(invalid("nonfinite upstream fixture value"))
                }
            })
            .collect()
    }

    fn finish(&self) -> Result<()> {
        if self.bytes.position() != u64::try_from(self.bytes.get_ref().len())? {
            return Err(invalid("trailing bytes in upstream fixture"));
        }
        Ok(())
    }
}

fn result(id: &str, passed: bool, detail: String) -> Value {
    json!({"Id": id, "Status": if passed { "pass" } else { "fail" }, "Detail": Value::String(detail)})
}

// Verbatim formula from the upstream rel_rms: use double accumulation, compare
// the entire M*N result, and preserve its special all-zero reference behavior.
#[allow(
    clippy::suboptimal_flops,
    reason = "Preserve the upstream C++ formula's separate multiply and add operations."
)]
fn rel_rms(actual: &[f32], reference: &[f32]) -> f64 {
    let mut squared_error = 0.0;
    let mut squared_reference = 0.0;
    for (&actual, &reference) in actual.iter().zip(reference) {
        let delta = f64::from(actual) - f64::from(reference);
        squared_error += delta * delta;
        squared_reference += f64::from(reference) * f64::from(reference);
    }
    if squared_reference == 0.0 {
        if squared_error == 0.0 {
            0.0
        } else {
            f64::INFINITY
        }
    } else {
        (squared_error / squared_reference).sqrt()
    }
}

fn pq2_shapes(path: &Path, target: Target) -> Result<Value> {
    let mut fixture = Fixture::open(path, *b"BSPQ2V1\n")?;
    if fixture.u32()? != 15 {
        return Err(invalid("PQ2 fixture must contain all 15 upstream shapes"));
    }
    let mut failures = Vec::new();
    let mut worst = 0.0_f64;
    for k in [128_usize, 256, 384, 512, 640] {
        for n in [1_usize, 4, 16] {
            let m = 4;
            let shape = [fixture.u32()?, fixture.u32()?, fixture.u32()?];
            if shape != [u32::try_from(k)?, u32::try_from(m)?, u32::try_from(n)?]
                || fixture.u32()? != u32::try_from(k / 128 * m * 34)?
            {
                return Err(invalid("PQ2 fixture shape/order/packed extent mismatch"));
            }
            let packed = fixture.block(k / 128 * m * 34)?;
            let activations = fixture.floats(k * n)?;
            let decoded = fixture.floats(k * m)?;
            let cpp_output = fixture.floats(m * n)?;
            let reference: Vec<f32> = (0..n)
                .flat_map(|j| {
                    let decoded = &decoded;
                    let activations = &activations;
                    (0..m).map(move |i| {
                        let sum: f64 = (0..k)
                            .map(|column| {
                                f64::from(decoded[i * k + column])
                                    * f64::from(activations[j * k + column])
                            })
                            .sum();
                        sum as f32
                    })
                })
                .collect();
            let actual = match target {
                Target::Cpp => Ok(cpp_output),
                Target::Rust => {
                    let tensor = Tensor {
                        name: "upstream-pq2-weights".to_owned(),
                        kind: 142,
                        dimensions: vec![k, m],
                        data: Arc::from(packed),
                    };
                    // The runtime's public CPU API accepts one activation
                    // column per call. Preserve the original N columns/order.
                    let mut output = Vec::with_capacity(m * n);
                    let mut error = None;
                    for activation in activations.chunks_exact(k) {
                        match matvec(&tensor, activation) {
                            Ok(values) => output.extend(values),
                            Err(reason) => {
                                error = Some(reason);
                                break;
                            }
                        }
                    }
                    error.map_or(Ok(output), Err)
                }
            };
            match actual {
                Ok(actual) if actual.len() == reference.len() => {
                    let error = rel_rms(&actual, &reference);
                    worst = worst.max(error);
                    if !error.is_finite() || error >= 0.05 {
                        failures.push(format!("K={k} M={m} N={n}: rel_rms={error:.8}"));
                    }
                }
                Ok(_) => failures.push(format!("K={k} M={m} N={n}: output extent mismatch")),
                Err(error) => failures.push(format!("K={k} M={m} N={n}: {error}")),
            }
        }
    }
    fixture.finish()?;
    Ok(result(
        PQ2_ID,
        failures.is_empty(),
        format!(
            "15 upstream shapes; K=128/256/384/512/640, M=4, N=1/4/16; \
             rel_rms < 0.05; worst={worst:.8}; failed_shapes={}; {}",
            failures.len(),
            failures.join("; ")
        ),
    ))
}

fn ptq1_mapping(path: &Path, target: Target) -> Result<Value> {
    let mut fixture = Fixture::open(path, *b"BSPTQV1\n")?;
    let mut rejected = 0;
    let mut compared = 0;
    let mut mismatches = 0;
    let mut first_failure = None;
    for trial in 0..20_000 {
        let packed = fixture.block(28)?;
        let expected = fixture.block(128)?;
        let cpp = fixture.block(128)?;
        if expected.iter().chain(&cpp).any(|&value| value > 2) {
            return Err(invalid("PTQ1 fixture trit outside -1/0/1"));
        }
        let actual = match target {
            Target::Cpp => Ok(cpp.iter().map(|&value| f32::from(value) - 1.0).collect()),
            Target::Rust => row(
                &Tensor {
                    name: "upstream-ptq1-block".to_owned(),
                    kind: 143,
                    dimensions: vec![128],
                    data: Arc::from(packed),
                },
                0,
            ),
        };
        match actual {
            Ok(actual) => {
                if actual.len() != 128 {
                    return Err(invalid("PTQ1 row decoder returned invalid extent"));
                }
                for (element, (&actual, &reference)) in actual.iter().zip(&expected).enumerate() {
                    compared += 1;
                    // Scale is exactly one, so all three integer values have
                    // exact float representations and upstream exact equality.
                    if actual.to_bits() != (f32::from(reference) - 1.0).to_bits() {
                        mismatches += 1;
                        first_failure.get_or_insert_with(|| {
                            format!(
                                "trial={trial} element={element}: {actual} != {}",
                                i16::from(reference) - 1
                            )
                        });
                    }
                }
            }
            Err(error) => {
                rejected += 1;
                first_failure.get_or_insert_with(|| format!("trial={trial}: {error}"));
            }
        }
    }
    fixture.finish()?;
    Ok(result(
        PTQ1_ID,
        mismatches == 0 && rejected == 0,
        format!(
            "20000 original LCG blocks; 2560000 required positions; compared={compared}; \
             mismatches={mismatches}; rejected_blocks={rejected}; {}",
            first_failure.unwrap_or_default()
        ),
    ))
}

fn main() -> Result<()> {
    let options = options()?;
    let mut cases = Vec::new();
    if options.case.as_deref().is_none_or(|case| case == PQ2_ID) {
        cases.push(pq2_shapes(
            options
                .pq2
                .as_deref()
                .ok_or_else(|| invalid("--pq2-fixtures is required"))?,
            options.target,
        )?);
    }
    if options.case.as_deref().is_none_or(|case| case == PTQ1_ID) {
        cases.push(ptq1_mapping(
            options
                .ptq1
                .as_deref()
                .ok_or_else(|| invalid("--ptq1-fixtures is required"))?,
            options.target,
        )?);
    }
    let report = json!({
        "SchemaVersion": 1,
        "UpstreamCommit": UPSTREAM,
        "Target": match options.target { Target::Cpp => "cpp", Target::Rust => "rust" },
        "Cases": cases,
    });
    fs::write(options.output, serde_json::to_vec_pretty(&report)?)?;
    // A measured regression belongs in the report. Nonzero process exit is
    // reserved for corrupt/missing fixtures or another infrastructure failure.
    Ok(())
}
