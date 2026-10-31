//! Paired TTS scheduling and bounded, separately supervised memory observations.

use serde::Deserialize;
use serde_json::{Value, json};

#[cfg(windows)]
mod windows;

#[derive(Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
enum Request {
    #[serde(rename = "plan")]
    Plan {
        threads: Vec<usize>,
        repetitions: u8,
    },
    #[cfg(windows)]
    #[serde(rename = "session")]
    Session(windows::Request),
}

fn plan(threads: &[usize], repetitions: u8) -> crate::Result<Value> {
    if threads.is_empty()
        || threads.len() > 3
        || threads.iter().any(|count| !(1..=256).contains(count))
        || threads
            .iter()
            .enumerate()
            .any(|(index, count)| threads[..index].contains(count))
        || !(1..=3).contains(&repetitions)
    {
        return Err(
            "TTS evaluation requires one to three distinct thread counts and repetitions.".into(),
        );
    }
    let mut schedule = Vec::new();
    for count in threads {
        for repetition in 1..=repetitions {
            let order = if repetition % 2 == 1 {
                ["Q8_0", "Q4_K_M"]
            } else {
                ["Q4_K_M", "Q8_0"]
            };
            for (index, quantization) in order.into_iter().enumerate() {
                schedule.push(json!({"Threads": count, "Repetition": repetition,
                    "PairPosition": index + 1, "Quantization": quantization,
                    "Id": format!("{count}-{repetition}-{quantization}")}));
            }
        }
    }
    Ok(
        json!({"SchemaVersion": 1, "ModelProcessStarted": false, "Schedule": schedule,
        "RequestsPerProcess": ["bootstrap", "warm"],
        "RequiredAvailableBytes": 9_u64 * 1024 * 1024 * 1024,
        "PhysicalReserveBytes": 5_u64 * 1024 * 1024 * 1024,
        "CommitLimitBytes": 16_u64 * 1024 * 1024 * 1024,
        "SampleIntervalMilliseconds": 25,
        "Definitions": {
            "startup": "Native process creation through authenticated readiness; artifact verification precedes process creation.",
            "bootstrap": "First request, including fixed reference design, Design release, Base loading and synthesis.",
            "warm": "Second request reusing the same Base model and speaker conditioning.",
            "SampledPeaks": "25 ms observations are lower bounds on phase maxima; short allocations may be missed.",
            "PeakWorkingSetBytes": "Operating-system process lifetime peak, including mapped residency; not a phase peak.",
            "PeakJobMemoryBytes": "Operating-system Job commit high-water counter; excludes mapped weight residency and may include denied allocation requests.",
            "AvailablePhysicalBytes": "System-wide available RAM; affected by unrelated processes.",
            "TalkerResidency": "Design and Base are sequential, so their serialized reductions are not added as RAM savings."
        }}),
    )
}

#[cfg(any(windows, test))]
fn validate_context(
    architecture: &str,
    variable: impl Fn(&str) -> Option<String>,
) -> crate::Result<()> {
    if variable("RUNNER_ENVIRONMENT").as_deref() != Some("github-hosted") {
        return Err(
            "The full TTS comparison requires GitHub-hosted CI; owner-device speech qualification remains pending a hosted-prepared workload within the complete five-minute budget.".into(),
        );
    }
    for (name, expected) in [
        ("GITHUB_ACTIONS", "true"),
        ("GITHUB_REF", "refs/heads/main"),
        ("GITHUB_REPOSITORY", "symbiogenesis/SwiftBonsai2Runner"),
        ("RUNNER_ARCH", "ARM64"),
    ] {
        if variable(name).as_deref() != Some(expected) {
            return Err(
                "TTS measurements require this repository's Windows ARM64 main CI lane.".into(),
            );
        }
    }
    if architecture != "aarch64"
        || !variable("GITHUB_RUN_ID").is_some_and(|value| {
            !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
        })
        || variable("RUNNER_NAME").is_none_or(|value| value.trim().is_empty())
    {
        return Err("TTS measurements require an identified native ARM64 CI runner.".into());
    }
    Ok(())
}

/// Plans paired fresh-process trials or runs one bounded native TTS capture session.
///
/// Planning is model-free. Sessions require this repository's Windows ARM64 main
/// hosted CI lane, retain verified artifacts, and use fixed speech resource guards.
/// The complete matrix cannot establish bounded owner-device qualification.
///
/// # Errors
/// Rejects malformed plans, unsupported platforms, unqualified CI contexts,
/// changed artifacts, unsafe paths, caller death or resource/capture failures.
pub fn run(request: Value) -> crate::Result<Value> {
    match serde_json::from_value(request).map_err(|error| error.to_string())? {
        Request::Plan {
            threads,
            repetitions,
        } => plan(&threads, repetitions),
        #[cfg(windows)]
        Request::Session(request) => windows::run(&request),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_schedule_alternates_fresh_processes_and_keeps_bootstrap_separate() -> crate::Result<()>
    {
        let plan = run(json!({"operation": "plan", "threads": [6, 10], "repetitions": 3}))?;
        let samples = plan["Schedule"].as_array().ok_or("Missing schedule.")?;
        assert_eq!(samples.len(), 12);
        for pair in samples.as_chunks::<2>().0 {
            assert_eq!(pair[0]["Threads"], pair[1]["Threads"]);
            assert_eq!(pair[0]["Repetition"], pair[1]["Repetition"]);
            assert_ne!(pair[0]["Quantization"], pair[1]["Quantization"]);
            assert_ne!(pair[0]["Id"], pair[1]["Id"]);
        }
        assert_eq!(samples[0]["Quantization"], "Q8_0");
        assert_eq!(samples[2]["Quantization"], "Q4_K_M");
        assert_eq!(plan["RequestsPerProcess"], json!(["bootstrap", "warm"]));
        assert_eq!(plan["RequiredAvailableBytes"], 9_663_676_416_u64);
        assert_eq!(plan["PhysicalReserveBytes"], 5_368_709_120_u64);
        Ok(())
    }

    #[test]
    fn evaluation_rejects_unbounded_duplicate_and_unknown_plan_inputs() {
        for (threads, repetitions) in [
            (vec![], 1),
            (vec![0], 1),
            (vec![257], 1),
            (vec![4, 4], 1),
            (vec![1, 2, 3, 4], 1),
            (vec![4], 0),
            (vec![4], 4),
        ] {
            assert!(
                run(json!({"operation": "plan", "threads": threads, "repetitions": repetitions}))
                    .is_err()
            );
        }
        assert!(
            run(
                json!({"operation": "plan", "threads": [4], "repetitions": 1,
            "physicalReserveBytes": 0})
            )
            .is_err()
        );
    }

    #[test]
    fn evaluation_context_rejects_local_foreign_branch_and_unidentified_hosts() -> crate::Result<()>
    {
        let identity = std::collections::BTreeMap::from([
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_REF", "refs/heads/main"),
            ("GITHUB_REPOSITORY", "symbiogenesis/SwiftBonsai2Runner"),
            ("RUNNER_ARCH", "ARM64"),
            ("GITHUB_RUN_ID", "1234"),
            ("RUNNER_NAME", "hosted-arm64"),
            ("RUNNER_ENVIRONMENT", "github-hosted"),
        ]);
        validate_context("aarch64", |key| identity.get(key).map(ToString::to_string))?;
        assert!(
            validate_context("x86_64", |key| identity.get(key).map(ToString::to_string)).is_err()
        );
        for key in identity.keys() {
            assert!(
                validate_context("aarch64", |name| {
                    (name != *key)
                        .then(|| identity.get(name).map(ToString::to_string))
                        .flatten()
                })
                .is_err()
            );
        }
        for (name, bad) in [
            ("GITHUB_ACTIONS", "false"),
            ("GITHUB_REF", "refs/heads/work/test"),
            ("GITHUB_REPOSITORY", "someone/fork"),
            ("GITHUB_RUN_ID", ""),
            ("RUNNER_NAME", " "),
            ("RUNNER_ENVIRONMENT", "self-hosted"),
            ("RUNNER_ENVIRONMENT", "unknown"),
        ] {
            assert!(
                validate_context("aarch64", |key| if name == key {
                    Some(bad.into())
                } else {
                    identity.get(key).map(ToString::to_string)
                })
                .is_err()
            );
        }
        Ok(())
    }
}
