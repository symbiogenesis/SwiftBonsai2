//! Whole native argument vectors frozen from the pre-migration PowerShell.

use serde_json::{Value, json};

use super::super::{control_identity, decode, policy, run, schema::Controls};
use super::{ADAPTER_FILENAME, arguments, samples};
use crate::Result;

const ARGUMENTS: &str = include_str!("../../tests/fixtures/lora-arguments-v1.json");
const QUALIFICATION: &str = include_str!("../../../../tests/fixtures/lora-qualification-v1.json");

fn frozen() -> Result<Value> {
    serde_json::from_str(QUALIFICATION).map_err(|error| error.to_string())
}

fn controls() -> Result<Controls> {
    decode(frozen()?["controlVectors"][0]["controls"].clone())
}

fn frozen_arguments(arm: &str, prompt: &str) -> Result<Value> {
    let fixture: Value = serde_json::from_str(ARGUMENTS).map_err(|error| error.to_string())?;
    let mut arguments = fixture["vectors"]
        .as_array()
        .ok_or("Missing frozen argument vectors.")?
        .iter()
        .find(|vector| vector["arm"] == arm)
        .ok_or("Missing frozen arm arguments.")?["arguments"]
        .as_array()
        .ok_or("Missing frozen arguments.")?
        .clone();
    *arguments.last_mut().ok_or("Missing frozen prompt.")? = json!(prompt);
    Ok(json!(arguments))
}

#[test]
fn full_argument_vectors_preserve_legacy_spelling_order_and_prompt_bytes() -> Result<()> {
    let fixture: Value = serde_json::from_str(ARGUMENTS).map_err(|error| error.to_string())?;
    let controls = controls()?;
    let policy = policy()?;
    control_identity(&controls, &policy)?;
    for vector in fixture["vectors"]
        .as_array()
        .ok_or("Missing frozen argument vectors.")?
    {
        let actual = arguments(
            &controls,
            fixture["modelPath"].as_str().ok_or("Missing model path.")?,
            ADAPTER_FILENAME,
            vector["arm"].as_str().ok_or("Missing arm.")?,
            vector["prompt"].as_str().ok_or("Missing prompt.")?,
        )?;
        assert_eq!(json!(actual), vector["arguments"]);
    }
    Ok(())
}

#[test]
fn invocation_plan_uses_fixed_schedule_prompts_controls_and_zero_checks() -> Result<()> {
    let fixture = frozen()?;
    let policy = policy()?;
    let controls = controls()?;
    let model = "C:\\external model & fixtures\\Swift-Bonsai-2-PQ2_0.gguf";
    let invocations = serde_json::to_value(samples(&policy, &controls, model)?)
        .map_err(|error| error.to_string())?;
    let actual = invocations.as_array().ok_or("Missing invocations.")?;
    assert_eq!(actual.len(), 28);
    let schedule = fixture["plan"]["Schedule"]
        .as_array()
        .ok_or("Missing frozen schedule.")?;
    let prompts = fixture["plan"]["Contract"]["prompts"]
        .as_array()
        .ok_or("Missing frozen prompts.")?;
    for (invocation, scheduled) in actual.iter().zip(schedule) {
        assert_eq!(invocation["id"], scheduled["sampleId"]);
        let prompt = prompts
            .iter()
            .find(|prompt| prompt["id"] == scheduled["promptId"])
            .ok_or("Missing scheduled prompt.")?;
        assert_eq!(
            invocation["arguments"],
            frozen_arguments(
                scheduled["arm"].as_str().ok_or("Missing scheduled arm.")?,
                prompt["text"].as_str().ok_or("Missing prompt text.")?,
            )?
        );
    }
    for (invocation, prompt) in actual.iter().skip(schedule.len()).zip(prompts) {
        assert_eq!(
            invocation["id"],
            format!(
                "zero-{}",
                prompt["id"].as_str().ok_or("Missing prompt id.")?
            )
        );
        assert_eq!(
            invocation["arguments"],
            frozen_arguments(
                "zero",
                prompt["text"].as_str().ok_or("Missing prompt text.")?,
            )?
        );
    }
    for vector in fixture["controlVectors"]
        .as_array()
        .ok_or("Missing controls vectors.")?
    {
        let request =
            json!({"operation":"invocations", "controls":vector["controls"], "modelPath":model});
        let result = run(request)?;
        assert_eq!(result["ControlsSha256"], vector["sha256"]);
        assert_eq!(result["Samples"], invocations);
    }
    Ok(())
}

#[test]
fn invocation_rejects_invalid_path_prompt_arm_and_changed_controls() -> Result<()> {
    let controls = controls()?;
    for prompt in [
        String::new(),
        " \t".into(),
        "bad\0prompt".into(),
        "a".repeat(4097),
        "😀".repeat(2049),
    ] {
        assert!(
            arguments(
                &controls,
                "model.gguf",
                ADAPTER_FILENAME,
                "baseline",
                &prompt
            )
            .is_err()
        );
    }
    for prompt in ["a".repeat(4096), "😀".repeat(2048)] {
        assert!(
            arguments(
                &controls,
                "model.gguf",
                ADAPTER_FILENAME,
                "baseline",
                &prompt
            )
            .is_ok()
        );
    }
    for model in ["", " \t", "model\0.gguf"] {
        assert!(arguments(&controls, model, ADAPTER_FILENAME, "baseline", "prompt").is_err());
    }
    for adapter in [
        "",
        "C:\\bonsai-abliterate-lora.gguf",
        "./bonsai-abliterate-lora.gguf",
        "bonsai-abliterate-lora.gguf:1",
        "BONSAI-ABLITERATE-LORA.GGUF",
    ] {
        for arm in ["baseline", "zero", "adapter"] {
            assert!(arguments(&controls, "model.gguf", adapter, arm, "prompt").is_err());
        }
    }
    for arm in ["", "Adapter", "baseline --lora", "unknown"] {
        assert!(arguments(&controls, "model.gguf", ADAPTER_FILENAME, arm, "prompt").is_err());
    }
    let policy = policy()?;
    let base = frozen()?["controlVectors"][0]["controls"].clone();
    for path in [
        "/contextTokens",
        "/batchTokens",
        "/microbatchTokens",
        "/threads",
        "/seed",
        "/maxOutputTokens",
        "/sampler/temperature",
        "/sampler/topP",
        "/sampler/topK",
        "/sampler/minP",
        "/sampler/repeatPenalty",
    ] {
        let mut changed = base.clone();
        *changed.pointer_mut(path).ok_or("Missing control.")? = json!(23);
        assert!(samples(&policy, &decode(changed)?, "model.gguf").is_err());
    }
    let mut mismatched_schedule = policy;
    mismatched_schedule.schedule[0].seed = 7;
    assert!(samples(&mismatched_schedule, &controls, "model.gguf").is_err());
    Ok(())
}
