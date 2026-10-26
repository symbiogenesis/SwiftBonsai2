//! Native invocation plans derived from the validated qualification controls.

use bonsai_cpu_policy::{Profile, completion_arguments};
use serde::Serialize;

use super::{Policy, control_identity, require, schema::Controls};
use crate::Result;

#[cfg(test)]
#[path = "invocation_tests.rs"]
mod tests;

const ADAPTER_FILENAME: &str = "bonsai-abliterate-lora.gguf";

#[derive(Serialize)]
pub(super) struct SampleInvocation {
    id: String,
    arguments: Vec<String>,
}

pub(super) fn samples(
    policy: &Policy,
    controls: &Controls,
    model_path: &str,
) -> Result<Vec<SampleInvocation>> {
    // Do not let callers construct commands under a different control identity.
    control_identity(controls, policy)?;
    let mut result = Vec::with_capacity(policy.schedule.len() + policy.prompts.len());
    for sample in &policy.schedule {
        let prompt = policy
            .prompts
            .iter()
            .find(|prompt| prompt.id == sample.prompt_id)
            .ok_or("Qualification sample references an unknown prompt.")?;
        require(
            f64::from(sample.seed) == controls.seed,
            "Qualification sample seed differs from its controls.",
        )?;
        result.push(SampleInvocation {
            id: sample.sample_id.clone(),
            arguments: arguments(
                controls,
                model_path,
                ADAPTER_FILENAME,
                sample.arm,
                &prompt.text,
            )?,
        });
    }
    for prompt in &policy.prompts {
        result.push(SampleInvocation {
            id: format!("zero-{}", prompt.id),
            arguments: arguments(controls, model_path, ADAPTER_FILENAME, "zero", &prompt.text)?,
        });
    }
    Ok(result)
}

fn prefix(controls: &Controls, model_path: &str) -> Result<Vec<String>> {
    let mut result = completion_arguments(model_path, Profile::SwiftPq2Assessment)?;
    // The common module owns CPU flags and their order. Qualification supplies
    // every execution control from the same object used for its identity.
    for (flag, value) in [
        ("--ctx-size", controls.context_tokens),
        ("--threads", controls.threads),
        ("--threads-batch", controls.threads),
        ("--batch-size", controls.batch_tokens),
        ("--ubatch-size", controls.microbatch_tokens),
    ] {
        let index = result
            .iter()
            .position(|argument| argument == flag)
            .ok_or("Common CPU completion prefix is missing a qualification control.")?;
        *result
            .get_mut(index + 1)
            .ok_or("Common CPU completion prefix has an incomplete control.")? = value.to_string();
    }
    Ok(result)
}

fn arguments(
    controls: &Controls,
    model_path: &str,
    adapter_path: &str,
    arm: &str,
    prompt: &str,
) -> Result<Vec<String>> {
    arguments_at_scale(controls, model_path, adapter_path, arm, prompt, "1")
}

pub(super) fn screen_arguments(
    controls: &Controls,
    model_path: &str,
    arm: &str,
    prompt: &str,
) -> Result<Vec<String>> {
    arguments_at_scale(controls, model_path, ADAPTER_FILENAME, arm, prompt, "1.3")
}

fn arguments_at_scale(
    controls: &Controls,
    model_path: &str,
    adapter_path: &str,
    arm: &str,
    prompt: &str,
    scale: &str,
) -> Result<Vec<String>> {
    require(
        !model_path.trim().is_empty()
            && !model_path.contains('\0')
            && !prompt.trim().is_empty()
            && prompt.encode_utf16().count() <= 4096
            && !prompt.contains('\0'),
        "Evaluation requires a verified model path and one bounded fixed prompt.",
    )?;
    // The pinned parser splits FNAME:SCALE at every colon. Only the verified
    // scratch copy's simple relative filename is safe here, including baseline.
    require(
        adapter_path == ADAPTER_FILENAME,
        "Use the verified adapter copy in the native working directory.",
    )?;
    require(
        matches!(arm, "baseline" | "zero" | "adapter"),
        "Unsupported qualification arm.",
    )?;
    let mut result = prefix(controls, model_path)?;
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
        ]
        .map(str::to_owned),
    );
    for (flag, value) in [
        ("--n-predict", controls.max_output_tokens),
        ("--seed", controls.seed),
        ("--temp", controls.sampler.temperature),
        ("--top-k", controls.sampler.top_k),
        ("--top-p", controls.sampler.top_p),
        ("--min-p", controls.sampler.min_p),
        ("--repeat-penalty", controls.sampler.repeat_penalty),
        ("--presence-penalty", 0.0),
        ("--frequency-penalty", 0.0),
    ] {
        result.extend([flag.to_owned(), value.to_string()]);
    }
    match arm {
        "zero" => result.extend(["--lora-scaled".into(), format!("{adapter_path}:0")]),
        "adapter" => result.extend(["--lora-scaled".into(), format!("{adapter_path}:{scale}")]),
        _ => {}
    }
    result.extend(["--prompt".into(), prompt.into()]);
    Ok(result)
}
