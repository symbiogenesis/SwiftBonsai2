//! Fixed paired qualification, canonical control identities, and offline scoring.
#![allow(
    clippy::float_cmp,
    reason = "Fixed JSON controls and integer-valued identities require exact equality."
)]

mod schema;
mod score;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::Read as _;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};

use crate::Result;
use schema::{Controls, Prompt};

const CONTRACT: &str = include_str!("../../../../config/swift-lora.qualification.json");
const GIB: f64 = 1_073_741_824.0;
const JSON_LIMIT: u64 = 16 * 1024 * 1024;

struct Policy {
    contract: Value,
    contract_sha256: String,
    prompts: Vec<Prompt>,
    schedule: Vec<Scheduled>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Scheduled {
    sample_id: String,
    prompt_id: String,
    repeat: u32,
    arm: &'static str,
    order: u32,
    seed: u32,
}

fn digest(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(text.as_bytes())
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

fn decode<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|error| format!("Invalid qualification input: {error}"))
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn string(value: &str, maximum: usize) -> Result<()> {
    require(
        !value.trim().is_empty() && value.encode_utf16().count() <= maximum,
        "Qualification text must be nonempty and bounded.",
    )
}

fn hash(value: &str) -> Result<()> {
    require(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "Qualification digest must be a lowercase SHA-256.",
    )
}

fn optional_hash(value: Option<&str>) -> Result<()> {
    value.map_or(Ok(()), hash)
}

fn number(value: f64, minimum: f64, maximum: f64, integer: bool) -> Result<()> {
    require(
        value.is_finite()
            && value >= minimum
            && value <= maximum
            && (!integer || value.fract() == 0.0),
        "Qualification number is outside its finite valid range.",
    )
}

fn validate_fixed_contract(contract: &Value) -> Result<()> {
    // Keep acceptance constants in reviewed code, rather than allowing a JSON
    // edit alone to weaken an acceptance gate.
    for (path, expected) in [
        ("/schemaVersion", 1.0),
        ("/execution/repeatCount", 3.0),
        ("/execution/seed", 42.0),
        ("/execution/minimumDecodeTokens", 16.0),
        ("/controls/maxOutputTokens", 512.0),
        ("/controls/contextTokens", 8192.0),
        ("/controls/batchTokens", 128.0),
        ("/controls/microbatchTokens", 64.0),
        ("/controls/threads", 4.0),
        ("/controls/seed", 42.0),
        ("/controls/sampler/temperature", 0.0),
        ("/controls/sampler/topP", 1.0),
        ("/controls/sampler/topK", 1.0),
        ("/controls/sampler/minP", 0.0),
        ("/controls/sampler/repeatPenalty", 1.0),
        ("/gates/minimumPerPromptMedianTpsRatio", 1.0),
        ("/gates/minimumQualityScore", 3.0),
        ("/gates/minimumPerPromptDimensionMedianQualityDelta", 0.0),
        ("/gates/maximumPeakJobMemoryBytes", 12.0 * GIB),
        (
            "/gates/maximumPerPromptMedianJobMemoryPremiumBytes",
            0.75 * GIB,
        ),
        ("/gates/minimumAvailablePhysicalBytes", 4.0 * GIB),
    ] {
        require(
            contract.pointer(path).and_then(Value::as_f64) == Some(expected),
            "Fixed qualification contract changed.",
        )?;
    }
    for path in [
        "/execution/githubActionsRequired",
        "/gates/rejectUnwantedAdapterRefusals",
        "/gates/basisVerificationRequired",
        "/gates/zeroScaleResponseAndLogitsIdentityRequired",
        "/gates/nonzeroLogitsApplicationRequired",
    ] {
        require(
            contract.pointer(path).and_then(Value::as_bool) == Some(true),
            "Fixed qualification gate changed.",
        )?;
    }
    require(
        contract["execution"]["runnerArchitecture"] == "Arm64"
            && contract["execution"]["deviceModel"] == "Snapdragon X Elite",
        "Qualification device changed.",
    )?;
    for (value, expected) in [
        (
            &contract["controls"],
            &[
                "runtimeCommit",
                "templateSha256",
                "adapterSha256",
                "contextTokens",
                "batchTokens",
                "microbatchTokens",
                "threads",
                "seed",
                "maxOutputTokens",
                "sampler",
            ][..],
        ),
        (
            &contract["controls"]["sampler"],
            &["temperature", "topP", "topK", "minP", "repeatPenalty"][..],
        ),
    ] {
        let fields = value
            .as_object()
            .ok_or("Fixed qualification controls must be objects.")?;
        require(
            fields.len() == expected.len()
                && fields.keys().all(|key| expected.contains(&key.as_str())),
            "Fixed qualification control fields changed.",
        )?;
    }
    Ok(())
}

fn policy() -> Result<Policy> {
    let contract: Value = serde_json::from_str(CONTRACT).map_err(|error| error.to_string())?;
    validate_fixed_contract(&contract)?;
    for (contents, path, contract_path) in [
        (
            include_str!("../../../../config/swift-assessment.lock.json"),
            "/runtime/commit",
            "/controls/runtimeCommit",
        ),
        (
            include_str!("../../../../config/swift-assessment.lock.json"),
            "/model/sha256",
            "/representations/PQ2_0",
        ),
        (
            include_str!("../../../../config/swift-lora.lock.json"),
            "/adapter/sha256",
            "/controls/adapterSha256",
        ),
        (
            include_str!("../../../../config/swift.ptq1-export.json"),
            "/OutputSha256",
            "/representations/current-repacked-PTQ1_0",
        ),
        (
            include_str!("../../../../config/swift.metadata.json"),
            "/EmbeddedTemplates/tokenizer.chat_template/Sha256",
            "/controls/templateSha256",
        ),
    ] {
        let identity: Value = serde_json::from_str(contents).map_err(|error| error.to_string())?;
        require(
            identity.pointer(path) == contract.pointer(contract_path),
            "Qualification artifact pins disagree.",
        )?;
    }
    let prompts: Vec<Prompt> = decode(contract["prompts"].clone())?;
    require(
        prompts.len() == 4,
        "Qualification needs four fixed prompts.",
    )?;
    let mut ids = HashSet::new();
    let mut schedule = Vec::with_capacity(24);
    for (index, prompt) in prompts.iter().enumerate() {
        require(
            ids.insert(&prompt.id) && matches!(prompt.category.as_str(), "general" | "creative"),
            "Duplicate prompt or unsupported category.",
        )?;
        for repeat in 1_u32..=3 {
            let arms =
                if (usize::try_from(repeat).map_err(|error| error.to_string())? + index) % 2 == 0 {
                    ["adapter", "baseline"]
                } else {
                    ["baseline", "adapter"]
                };
            for arm in arms {
                let order = u32::try_from(schedule.len() + 1).map_err(|error| error.to_string())?;
                schedule.push(Scheduled {
                    sample_id: format!("s{order:04}"),
                    prompt_id: prompt.id.clone(),
                    repeat,
                    arm,
                    order,
                    seed: 42,
                });
            }
        }
    }
    Ok(Policy {
        contract,
        contract_sha256: digest(&CONTRACT.replace("\r\n", "\n")),
        prompts,
        schedule,
    })
}

fn control_identity(controls: &Controls, policy: &Policy) -> Result<(String, String)> {
    hash(&controls.model_sha256)?;
    require(
        matches!(
            controls.weight_representation.as_str(),
            "PQ2_0" | "current-repacked-PTQ1_0"
        ) && policy.contract["representations"][&controls.weight_representation]
            == controls.model_sha256,
        "Unpinned model or weight representation.",
    )?;
    for (actual, key) in [
        (&controls.runtime_commit, "runtimeCommit"),
        (&controls.adapter_sha256, "adapterSha256"),
        (&controls.template_sha256, "templateSha256"),
    ] {
        require(
            policy.contract["controls"][key].as_str() == Some(actual),
            "Unpinned qualification control.",
        )?;
    }
    for (actual, expected) in [
        (controls.context_tokens, 8192.0),
        (controls.batch_tokens, 128.0),
        (controls.microbatch_tokens, 64.0),
        (controls.threads, 4.0),
        (controls.seed, 42.0),
        (controls.max_output_tokens, 512.0),
        (controls.sampler.temperature, 0.0),
        (controls.sampler.top_p, 1.0),
        (controls.sampler.top_k, 1.0),
        (controls.sampler.min_p, 0.0),
        (controls.sampler.repeat_penalty, 1.0),
    ] {
        require(actual == expected, "Unmatched qualification control.")?;
    }
    let mut canonical = policy.contract["controls"].clone();
    canonical["sampler"] = json!({"minP": controls.sampler.min_p, "repeatPenalty": controls.sampler.repeat_penalty,
        "temperature": controls.sampler.temperature, "topK": controls.sampler.top_k, "topP": controls.sampler.top_p});
    // Legacy PowerShell writes these first two members before the sorted keys,
    // integer controls as integers, and sampler controls as doubles. This is a
    // versioned byte contract, not an arbitrary JSON serialization choice.
    let sorted: BTreeMap<_, _> = canonical
        .as_object()
        .ok_or("Canonical controls must be an object.")?
        .iter()
        .collect();
    let tail = serde_json::to_string(&sorted).map_err(|error| error.to_string())?;
    let canonical = format!(
        "{{\"modelSha256\":\"{}\",\"weightRepresentation\":\"{}\",{}",
        controls.model_sha256,
        controls.weight_representation,
        tail.strip_prefix('{')
            .ok_or("Canonical controls must be an object.")?
    );
    Ok((digest(&canonical), canonical))
}

fn take<T: for<'de> Deserialize<'de>>(request: &mut Map<String, Value>, key: &str) -> Result<T> {
    decode(
        request
            .remove(key)
            .ok_or_else(|| format!("Missing qualification request field: {key}"))?,
    )
}

/// Runs a plan, controls, score, or bounded file-score qualification request.
///
/// # Errors
/// Rejects malformed schemas, changed pins, mismatched evidence, unsafe external
/// paths, oversized/unreadable JSON, or unexpected request fields. Known valid
/// regressions return a rejected report; missing proofs/reviews remain pending.
pub fn run(request: Value) -> Result<Value> {
    let Value::Object(mut request) = request else {
        return Err("Qualification request must be an object.".into());
    };
    let operation: String = take(&mut request, "operation")?;
    let policy = policy()?;
    let result = match operation.as_str() {
        "plan" => {
            json!({"Contract": policy.contract, "ContractSha256": policy.contract_sha256, "Schedule": policy.schedule})
        }
        "controls" => {
            let controls = take(&mut request, "controls")?;
            let (sha256, canonical) = control_identity(&controls, &policy)?;
            json!({"Sha256": sha256, "CanonicalJson": canonical})
        }
        "score" => score::score(
            take(&mut request, "evidence")?,
            take(&mut request, "review")?,
            &policy,
        )?,
        "file-score" => {
            let evidence: String = take(&mut request, "evidencePath")?;
            let review: Option<String> = take(&mut request, "reviewPath")?;
            score::score(
                read_json(Path::new(&evidence))?,
                review
                    .as_deref()
                    .map(|path| read_json(Path::new(path)))
                    .transpose()?,
                &policy,
            )?
        }
        _ => return Err("Unknown qualification operation.".into()),
    };
    require(
        request.is_empty(),
        "Unrecognized qualification request field.",
    )?;
    Ok(result)
}

fn read_json(path: &Path) -> Result<Value> {
    validate_path(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1); // FILE_SHARE_READ denies writers and renames.
    }
    let file: File = options.open(path).map_err(|error| error.to_string())?;
    require(
        file.metadata().map_err(|error| error.to_string())?.len() <= JSON_LIMIT,
        "Qualification JSON exceeds its bounded input size.",
    )?;
    let mut bytes = Vec::new();
    file.take(JSON_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    require(
        u64::try_from(bytes.len()).map_err(|error| error.to_string())? <= JSON_LIMIT,
        "Qualification JSON exceeds its bounded input size.",
    )?;
    let text = decode_text(&bytes)?;
    let value: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    bounded_depth(&value, 32)?;
    Ok(value)
}

fn bounded_depth(value: &Value, remaining: usize) -> Result<()> {
    match value {
        Value::Array(items) => {
            require(remaining > 0, "Qualification JSON exceeds its depth limit.")?;
            for item in items {
                bounded_depth(item, remaining - 1)?;
            }
        }
        Value::Object(fields) => {
            require(remaining > 0, "Qualification JSON exceeds its depth limit.")?;
            for item in fields.values() {
                bounded_depth(item, remaining - 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_path(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        bonsai_windows_credentials::validate_external_path(path).map_err(|error| error.to_string())
    }
    #[cfg(not(windows))]
    {
        require(path.is_absolute(), "Qualification paths must be absolute.")?;
        for ancestor in path.ancestors() {
            if let Ok(metadata) = ancestor.symlink_metadata() {
                require(
                    !metadata.file_type().is_symlink() && !ancestor.join(".git").exists(),
                    "Qualification paths must be external without links.",
                )?;
            }
        }
        Ok(())
    }
}

fn decode_text(bytes: &[u8]) -> Result<String> {
    // Match StreamReader's BOM detection while retaining strict decoding.
    if bytes.starts_with(&[0xff, 0xfe, 0, 0]) || bytes.starts_with(&[0, 0, 0xfe, 0xff]) {
        require(
            (bytes.len() - 4).is_multiple_of(4),
            "Invalid UTF-32 qualification JSON.",
        )?;
        let little = bytes[0] == 0xff;
        bytes[4..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| {
                char::from_u32(if little {
                    u32::from_le_bytes(*chunk)
                } else {
                    u32::from_be_bytes(*chunk)
                })
                .ok_or_else(|| "Invalid UTF-32 qualification JSON.".into())
            })
            .collect()
    } else if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        require(
            (bytes.len() - 2).is_multiple_of(2),
            "Invalid UTF-16 qualification JSON.",
        )?;
        let little = bytes[0] == 0xff;
        let words: Vec<_> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| {
                if little {
                    u16::from_le_bytes(*chunk)
                } else {
                    u16::from_be_bytes(*chunk)
                }
            })
            .collect();
        String::from_utf16(&words).map_err(|error| error.to_string())
    } else {
        String::from_utf8(
            bytes
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(bytes)
                .to_vec(),
        )
        .map_err(|error| error.to_string())
    }
}
