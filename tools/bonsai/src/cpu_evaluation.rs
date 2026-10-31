//! Bounded X2 CPU thread screening plans; execution and promotion remain separate.

use bonsai_cpu_policy::{Profile, completion_arguments, hardware::SnapdragonFamily};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Result;

mod conditions;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    operation: String,
    data_directory: String,
    processor_name: String,
    logical_processors: u16,
}

#[derive(Clone, Copy)]
struct Threads {
    decode: u16,
    prefill: u16,
}

impl Threads {
    fn id(self) -> String {
        format!("d{}-p{}", self.decode, self.prefill)
    }
}

fn candidates(available: u16) -> Vec<Threads> {
    // One candidate is the entire owner-device screen, not a slice of a matrix.
    [(6, 6)]
        .into_iter()
        .filter(|(decode, prefill)| *decode <= available && *prefill <= available)
        .map(|(decode, prefill)| Threads { decode, prefill })
        .collect()
}

fn workloads() -> [(&'static str, String); 1] {
    let short = "Write a hopeful scene about a lighthouse keeper opening a letter from their future self. Include a sensory detail and one line of dialogue. Continue the scene for at least 200 words.";
    [("short", short.into())]
}

fn arguments(model: &str, threads: Threads, prompt: &str) -> Result<Vec<String>> {
    let mut result = completion_arguments(model, Profile::SwiftPq2Assessment)?;
    for (flag, value) in [
        ("--threads", threads.decode),
        ("--threads-batch", threads.prefill),
    ] {
        let index = result
            .iter()
            .position(|argument| argument == flag)
            .ok_or("Common CPU prefix is missing a thread control.")?;
        *result
            .get_mut(index + 1)
            .ok_or("Incomplete CPU thread control.")? = value.to_string();
    }
    result.extend(
        [
            "--perf",
            "--log-verbosity",
            "3",
            "--no-log-prefix",
            "--no-log-timestamps",
            "--log-colors",
            "off",
            "--color",
            "off",
            "--simple-io",
            "--no-display-prompt",
            "--verbose-prompt",
            "--conversation",
            "--jinja",
            "--single-turn",
            "--n-predict",
            "16",
            "--seed",
            "42",
            "--reasoning",
            "on",
            "--reasoning-effort",
            "xhigh",
            "--reasoning-budget",
            "-1",
            "--temp",
            "1",
            "--top-p",
            "0.95",
            "--top-k",
            "20",
            "--min-p",
            "0",
            "--repeat-penalty",
            "1",
            "--presence-penalty",
            "0",
            "--frequency-penalty",
            "0",
            "--lora-scaled",
            "bonsai-abliterate-lora.gguf:1.3",
            "--prompt",
            prompt,
        ]
        .map(str::to_owned),
    );
    Ok(result)
}

/// Plan a fixed X2 CPU screen or compare its observed execution conditions.
///
/// The caller supplies observed processor identity/count, verifies native topology
/// and retains the report. Plans never establish evidence, change launch defaults
/// or infer ISA support. Only the separate CI wrapper executes these invocations.
///
/// # Errors
/// Rejects unknown fields, unsupported operations/devices, unbounded CPU counts,
/// unavailable candidate budgets and invalid compiled CPU plans.
pub fn run(request: Value) -> Result<Value> {
    if request["operation"] == "compare-sample-conditions" {
        return conditions::compare_samples(request);
    }
    if request["operation"] == "compare-conditions" {
        return conditions::compare(request);
    }
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    if request.operation != "plan"
        || SnapdragonFamily::from_processor_name(&request.processor_name)
            != Some(SnapdragonFamily::X2EliteExtreme)
        || !(6..=256).contains(&request.logical_processors)
    {
        return Err("CPU screening requires observed X2 Elite Extreme identity and 6..256 logical processors.".into());
    }
    let cpu = bonsai_cpu_policy::run(json!({
        "operation": "plan", "dataDirectory": request.data_directory,
        "profile": "swift-pq2-assessment", "useAbliterateLora": true, "useRepackedPtq1": false,
    }))?;
    let model = cpu["ModelPath"]
        .as_str()
        .ok_or("Missing compiled CPU model path.")?;
    let candidates = candidates(request.logical_processors);
    let workloads = workloads();
    let baseline = Threads {
        decode: 4,
        prefill: 4,
    };
    let mut schedule = Vec::new();
    for (workload, prompt) in &workloads {
        for candidate in &candidates {
            for repetition in 1..=2 {
                let pair = if repetition == 1 {
                    [baseline, *candidate]
                } else {
                    [*candidate, baseline]
                };
                for (position, threads) in pair.into_iter().enumerate() {
                    let role = if threads.decode == 4 && threads.prefill == 4 {
                        "baseline"
                    } else {
                        "candidate"
                    };
                    schedule.push(json!({
                        "Id": format!("{workload}-{}-r{repetition}-{role}", candidate.id()),
                        "CandidateId": candidate.id(), "Role": role, "Workload": workload,
                        "Repetition": repetition, "PairPosition": position + 1,
                        "DecodeThreads": threads.decode, "PrefillThreads": threads.prefill,
                        "Arguments": arguments(model, threads, prompt)?,
                    }));
                }
            }
        }
    }
    Ok(json!({
        "SchemaVersion": 1, "Kind": "runner-x2-cpu-evaluation-plan", "ModelProcessStarted": false,
        "DeviceModel": SnapdragonFamily::X2EliteExtreme.device_model(),
        "ProcessorName": request.processor_name, "LogicalProcessors": request.logical_processors,
        "CpuPlan": cpu, "Candidates": candidates.iter().map(|threads| json!({
            "Id": threads.id(), "DecodeThreads": threads.decode, "PrefillThreads": threads.prefill,
        })).collect::<Vec<_>>(),
        "Workloads": workloads.iter().map(|(id, text)| json!({"Id": id, "Text": text})).collect::<Vec<_>>(),
        "Repetitions": 2, "GeneratedTokenCap": 16, "PerSampleTimeoutSeconds": 45,
        "MaximumSamples": 4, "MaximumModelExecutionSeconds": schedule.len() * 45,
        "WorkBudgetSeconds": 240, "WholeJobMinutes": 5,
        "TemplateMode": "pinned-completion-template-defaults",
        "Scope": "One candidate and short prompt only, with two reversed pairs under one shared setup/verification/loading/comparison budget. Not a partition of a longer matrix. Fresh processes; OS file cache is not flushed. CLI template defaults differ from chat. No significance, energy, sustained-comfort, quality or default-profile verdict.",
        "Schedule": schedule,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(count: u16) -> Value {
        json!({"operation": "plan", "dataDirectory": "C:\\BonsaiData",
            "processorName": "Snapdragon(R) X2 Elite Extreme - X2E94100 - Qualcomm Oryon(TM) CPU",
            "logicalProcessors": count})
    }

    #[test]
    fn paired_screen_reverses_order_and_prunes_cpu_budgets() -> Result<()> {
        let plan = run(request(18))?;
        let schedule = plan["Schedule"].as_array().ok_or("Missing schedule.")?;
        assert_eq!(schedule.len(), 4);
        assert_eq!(plan["MaximumModelExecutionSeconds"], 180);
        assert_eq!(plan["WorkBudgetSeconds"], 240);
        assert_eq!(plan["WholeJobMinutes"], 5);
        assert_eq!(plan["GeneratedTokenCap"], 16);
        assert_eq!(plan["Candidates"].as_array().map(Vec::len), Some(1));
        assert_eq!(plan["Workloads"].as_array().map(Vec::len), Some(1));
        for pair in schedule.as_chunks::<2>().0 {
            assert_eq!(pair[0]["CandidateId"], pair[1]["CandidateId"]);
            assert_eq!(pair[0]["Workload"], pair[1]["Workload"]);
            assert_eq!(pair[0]["Repetition"], pair[1]["Repetition"]);
            assert_ne!(pair[0]["Id"], pair[1]["Id"]);
            assert_ne!(pair[0]["Role"], pair[1]["Role"]);
            assert_eq!(
                pair[0]["Role"],
                if pair[0]["Repetition"] == 1 {
                    "baseline"
                } else {
                    "candidate"
                }
            );
        }
        let small = run(request(8))?;
        assert_eq!(
            small["Schedule"]
                .as_array()
                .ok_or("Missing schedule.")?
                .len(),
            4
        );
        for trial in small["Schedule"].as_array().ok_or("Missing schedule.")? {
            assert!(
                trial["DecodeThreads"]
                    .as_u64()
                    .is_some_and(|value| value <= 8)
            );
            assert!(
                trial["PrefillThreads"]
                    .as_u64()
                    .is_some_and(|value| value <= 8)
            );
        }
        Ok(())
    }

    #[test]
    fn screening_changes_only_threads_in_common_cpu_policy() -> Result<()> {
        let baseline = arguments(
            "C:\\model.gguf",
            Threads {
                decode: 4,
                prefill: 4,
            },
            "Fixed prompt.",
        )?;
        let candidate = arguments(
            "C:\\model.gguf",
            Threads {
                decode: 8,
                prefill: 18,
            },
            "Fixed prompt.",
        )?;
        let differences: Vec<_> = baseline
            .iter()
            .zip(&candidate)
            .enumerate()
            .filter(|(_, (left, right))| left != right)
            .map(|(index, _)| index)
            .collect();
        assert_eq!(differences.len(), 2);
        for index in differences {
            assert!(matches!(
                baseline[index - 1].as_str(),
                "--threads" | "--threads-batch"
            ));
        }
        for (flag, expected) in [
            ("--poll", "0"),
            ("--poll-batch", "0"),
            ("--gpu-layers", "0"),
            ("--ctx-size", "8192"),
            ("--batch-size", "128"),
            ("--ubatch-size", "64"),
            ("--lora-scaled", "bonsai-abliterate-lora.gguf:1.3"),
            ("--n-predict", "16"),
        ] {
            let index = candidate
                .iter()
                .position(|argument| argument == flag)
                .ok_or("Missing control.")?;
            assert_eq!(candidate[index + 1], expected);
        }
        Ok(())
    }

    #[test]
    fn screening_rejects_foreign_devices_unbounded_counts_and_overrides() {
        for count in [0, 4, 257] {
            assert!(run(request(count)).is_err());
        }
        for name in ["Snapdragon X Elite", "Snapdragon X2 Elite", "Unknown CPU"] {
            let mut input = request(18);
            input["processorName"] = name.into();
            assert!(run(input).is_err());
        }
        let mut input = request(18);
        input["poll"] = 50.into();
        assert!(run(input).is_err());
    }
}
