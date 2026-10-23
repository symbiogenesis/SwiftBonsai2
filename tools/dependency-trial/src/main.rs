//! CI-only, interleaved broker dependency screening on Windows ARM64.
#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "binary-private modules expose trial interfaces only to their parent executable"
)]

mod corpus;
mod runtime;

use bonsai_broker::json_trial::{self, Engine};
use serde::Serialize;
use std::{env, hint::black_box, io::Write, process::ExitCode, time::Instant};

const REPETITIONS: usize = 9;

#[derive(Serialize)]
struct JsonCase {
    name: &'static str,
    input_bytes: usize,
    iterations: usize,
    elapsed_ns: Vec<u64>,
}

#[derive(Serialize)]
struct JsonCandidate {
    engine: &'static str,
    eligible: bool,
    exclusion: Option<String>,
    cases: Vec<JsonCase>,
}

#[derive(Serialize)]
struct RuntimeCase {
    name: String,
    input_bytes: usize,
    iterations: u64,
    connections: usize,
    frame_bytes: usize,
    elapsed_ns: Vec<u64>,
    operations: u64,
    bytes: u64,
    checksum: u64,
}

#[derive(Serialize)]
struct RuntimeCandidate {
    engine: &'static str,
    eligible: bool,
    exclusion: Option<String>,
    cases: Vec<RuntimeCase>,
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    revision: String,
    architecture: &'static str,
    repetitions: usize,
    correctness_cases: usize,
    json: Vec<JsonCandidate>,
    runtime: Vec<RuntimeCandidate>,
}

fn main() -> ExitCode {
    match execute() {
        Ok(report) => {
            let mut output = std::io::stdout().lock();
            if serde_json::to_writer(&mut output, &report).is_ok()
                && output.write_all(b"\n").is_ok()
            {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            let _result = writeln!(std::io::stderr().lock(), "{error}");
            ExitCode::FAILURE
        }
    }
}

fn execute() -> Result<Report, String> {
    let arguments: Vec<_> = env::args().skip(1).collect();
    let check_only = arguments == ["--check"];
    if !check_only
        && (!arguments.is_empty()
            || !cfg!(all(target_os = "windows", target_arch = "aarch64"))
            || env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || env::var("GITHUB_REF").as_deref() != Ok("refs/heads/main")
            || env::var("GITHUB_REPOSITORY").as_deref() != Ok("symbiogenesis/SwiftBonsai2"))
    {
        return Err("Measurements require native Windows ARM64 GitHub Actions on main; --check runs only differential correctness.".into());
    }
    let workloads = corpus::workloads()?;
    for case in &workloads {
        json_trial::prepare(&case.body, Engine::SerdeJson)
            .map_err(|error| format!("Invalid baseline workload {}: {error:?}", case.name))?;
    }
    let mut validation = corpus::adversarial();
    validation.extend(workloads.iter().map(|case| case.body.clone()));
    let mut json: Vec<_> = Engine::ALL
        .into_iter()
        .map(|engine| {
            let failure = validate(engine, &validation);
            JsonCandidate {
                engine: engine.name(),
                eligible: failure.is_none(),
                exclusion: failure,
                cases: workloads
                    .iter()
                    .map(|case| JsonCase {
                        name: case.name,
                        input_bytes: case.body.len(),
                        iterations: (16 * 1024 * 1024 / case.body.len()).clamp(16, 65_536),
                        elapsed_ns: Vec::with_capacity(REPETITIONS),
                    })
                    .collect(),
            }
        })
        .collect();
    let runtimes = if check_only {
        Vec::new()
    } else {
        // Rotate order each round to spread warm-cache and machine drift effects.
        for repetition in 0..=REPETITIONS {
            for offset in 0..Engine::ALL.len() {
                let index = (offset + repetition) % Engine::ALL.len();
                let engine = Engine::ALL[index];
                if !json[index].eligible {
                    continue;
                }
                for (case, result) in workloads.iter().zip(&mut json[index].cases) {
                    let started = Instant::now();
                    for _ in 0..result.iterations {
                        let prepared = json_trial::prepare(black_box(&case.body), engine).map_err(
                            |error| {
                                format!("{} rejected a timed workload: {error:?}", engine.name())
                            },
                        )?;
                        black_box(prepared);
                    }
                    let elapsed = u64::try_from(started.elapsed().as_nanos())
                        .map_err(|error| error.to_string())?;
                    if repetition != 0 {
                        result.elapsed_ns.push(elapsed);
                    }
                }
            }
        }
        measure_runtimes()?
    };
    Ok(Report {
        schema_version: 1,
        revision: env::var("GITHUB_SHA").unwrap_or_default(),
        architecture: env::consts::ARCH,
        repetitions: if check_only { 0 } else { REPETITIONS },
        correctness_cases: validation.len(),
        json,
        runtime: runtimes,
    })
}

fn validate(engine: Engine, bodies: &[Vec<u8>]) -> Option<String> {
    for (index, body) in bodies.iter().enumerate() {
        let expected = json_trial::prepare(body, Engine::SerdeJson);
        let actual = json_trial::prepare(body, engine);
        let matches = match (expected, actual) {
            (Ok(expected), Ok(actual)) => {
                expected.body() == actual.body()
                    && expected.max_completion_tokens() == actual.max_completion_tokens()
                    && expected.streams_response() == actual.streams_response()
            }
            (Err(expected), Err(actual)) => expected == actual,
            _ => false,
        };
        if !matches {
            return Some(format!(
                "Differential policy mismatch at synthetic case {index}"
            ));
        }
    }
    None
}

fn measure_runtimes() -> Result<Vec<RuntimeCandidate>, String> {
    let cases = [(1, 256), (1, 4096), (9, 256), (9, 4096)];
    let mut results: Vec<_> = runtime::Engine::ALL
        .into_iter()
        .map(|engine| RuntimeCandidate {
            engine: engine.name(),
            eligible: true,
            exclusion: None,
            cases: cases
                .iter()
                .map(|&(connections, frame_bytes)| RuntimeCase {
                    name: format!("connections-{connections}-frame-{frame_bytes}"),
                    input_bytes: frame_bytes,
                    iterations: 0,
                    connections,
                    frame_bytes,
                    elapsed_ns: Vec::with_capacity(REPETITIONS),
                    operations: 0,
                    bytes: 0,
                    checksum: 0,
                })
                .collect(),
        })
        .collect();
    for repetition in 0..=REPETITIONS {
        for offset in 0..runtime::Engine::ALL.len() {
            let index = (offset + repetition) % runtime::Engine::ALL.len();
            if !results[index].eligible {
                continue;
            }
            for case in &mut results[index].cases {
                match runtime::measure(
                    runtime::Engine::ALL[index],
                    runtime::Workload {
                        connections: case.connections,
                        frame_bytes: case.frame_bytes,
                        rounds: 64,
                    },
                ) {
                    Ok(sample) => {
                        case.operations = sample.operations;
                        case.iterations = sample.operations;
                        case.bytes = sample.bytes;
                        case.checksum = sample.checksum;
                        if repetition != 0 {
                            case.elapsed_ns.push(
                                u64::try_from(sample.elapsed_ns)
                                    .map_err(|error| error.to_string())?,
                            );
                        }
                    }
                    Err(error) => {
                        results[index].eligible = false;
                        results[index].exclusion = Some(error);
                        break;
                    }
                }
            }
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    #[test]
    fn synthetic_workloads_are_accepted_by_the_current_policy() -> Result<(), String> {
        for case in super::corpus::workloads()? {
            super::json_trial::prepare(&case.body, super::Engine::SerdeJson)
                .map_err(|error| format!("{}: {error:?}", case.name))?;
        }
        Ok(())
    }
}
