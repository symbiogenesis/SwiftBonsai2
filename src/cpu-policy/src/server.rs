//! Pure development server policy; storage and process checks remain with callers.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Profile, REQUIRED_AVAILABLE_BYTES, hex, policy, require};

/// Whether an environment name is an unsupported model or runtime override.
///
/// Values are never needed or retained. Matching is ASCII case insensitive,
/// consistent with the Windows broker's existing environment policy.
#[must_use]
pub fn forbidden_environment_name(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    ["LLAMA_", "LLAMACPP_", "GGML_", "AIP_", "MTMD_"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || ["HF_TOKEN", "HF_ENDPOINT", "MODEL_ENDPOINT"].contains(&name.as_str())
}

/// Reject inherited runtime/model override names without accepting their values.
///
/// # Errors
/// Lists only the forbidden names, preserving their original spelling.
pub fn validate_environment(names: impl IntoIterator<Item = String>) -> Result<(), String> {
    let mut overrides: Vec<_> = names
        .into_iter()
        .filter(|name| forbidden_environment_name(name))
        .collect();
    overrides.sort_by_cached_key(|name| name.to_ascii_uppercase());
    require(
        overrides.is_empty(),
        &format!(
            "Inherited server/model overrides are unsupported: {}",
            overrides.join(", ")
        ),
    )
}

/// Build the fixed, separately tokenized server invocation for a reviewed profile.
///
/// This function performs no filesystem access and never launches a process.
/// Callers must validate local storage and artifact/credential ownership.
///
/// # Errors
/// Rejects empty or NUL-containing paths and privileged or dynamic ports.
pub fn arguments(
    model_path: &str,
    key_file: &str,
    port: u16,
    profile: Profile,
) -> Result<Vec<String>, String> {
    require(
        [model_path, key_file]
            .into_iter()
            .all(|path| !path.trim().is_empty() && !path.contains('\0')),
        "Model and key file paths are required.",
    )?;
    require(port >= 1024, "A nonprivileged server port is required.")?;
    let mut result: Vec<_> = [
        "--model".into(),
        model_path.into(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--api-key-file".into(),
        key_file.into(),
        "--cors-origins".into(),
        format!("http://127.0.0.1:{port}"),
    ]
    .into_iter()
    .collect();
    result.extend(
        [
            "--cors-methods",
            "GET,POST,OPTIONS",
            "--cors-headers",
            "Authorization,Content-Type",
            "--no-cors-credentials",
            "--no-agent",
            "--no-ui-mcp-proxy",
            "--no-ui",
            "--no-slots",
            "--no-mmproj",
            "--ctx-size",
            "8192",
            "--parallel",
            "1",
        ]
        .map(str::to_owned),
    );
    result.extend([
        "--batch-size".into(),
        profile.batch_tokens().to_string(),
        "--ubatch-size".into(),
        profile.microbatch_tokens().to_string(),
    ]);
    result.extend(FIXED_ARGUMENTS.iter().copied().map(str::to_owned));
    Ok(result)
}

const FIXED_ARGUMENTS: &[&str] = &[
    "--cache-type-k",
    "f16",
    "--cache-type-v",
    "f16",
    "--ctx-checkpoints",
    "0",
    "--cache-ram",
    "0",
    "--no-cache-prompt",
    "--no-cache-idle-slots",
    "--no-context-shift",
    "--spec-type",
    "none",
    "--gpu-layers",
    "0",
    "--device",
    "none",
    "--no-kv-offload",
    "--no-op-offload",
    "--fit",
    "off",
    "--load-mode",
    "mmap",
    "--no-repack",
    "--offline",
    "--no-warmup",
    "--threads",
    "4",
    "--threads-batch",
    "4",
    "--poll",
    "0",
    "--poll-batch",
    "0",
    "--jinja",
    "--reasoning",
    "on",
    "--reasoning-effort",
    "xhigh",
    "--reasoning-budget",
    "-1",
    "--temp",
    "1.0",
    "--top-p",
    "0.95",
    "--top-k",
    "20",
    "--min-p",
    "0.0",
    "--presence-penalty",
    "0.0",
    "--frequency-penalty",
    "0.0",
    "--repeat-penalty",
    "1.0",
    "--n-predict",
    "4096",
    "--timeout",
    "60",
    "--no-perf",
    "--log-disable",
];

fn validate_reference_profile(profile: &Value, model_bytes: u64) -> Result<(), String> {
    let cpu = policy(Profile::ReferenceCpu, false, false)?;
    let expected = json!({
        "schemaVersion": 1, "name": "reference-cpu",
        "status": "development-only-unqualified", "backend": "cpu",
        "contextTokens": cpu["ContextTokens"], "parallel": 1,
        "batchTokens": cpu["BatchTokens"], "microbatchTokens": cpu["MicrobatchTokens"],
        "cacheTypeK": "f16", "cacheTypeV": "f16", "contextCheckpoints": 0,
        "promptCacheMiB": 0, "cachePrompt": false, "speculation": "none",
        "serviceCeilingBytes": cpu["ServiceCeilingBytes"], "estimatedWeightBytes": model_bytes,
        "estimatedKvBytes": cpu["EstimatedKvBytes"],
        "estimatedRecurrentBytes": cpu["EstimatedRecurrentBytes"],
        "estimatedRuntimeHeadroomBytes": cpu["EstimatedRuntimeHeadroomBytes"],
        "launchEnabled": false,
    });
    let expected = expected
        .as_object()
        .ok_or("Invalid compiled server policy.")?;
    let profile = profile
        .as_object()
        .ok_or("Reference profile must be an object.")?;
    require(
        profile.len() == expected.len() + 2
            && profile.keys().all(|name| {
                expected.contains_key(name)
                    || ["estimateScope", "remainingGates"].contains(&name.as_str())
            }),
        "Reference profile has missing or unreviewed fields.",
    )?;
    for (name, wanted) in expected {
        let actual = &profile[name];
        let valid = if wanted.is_number() {
            actual.as_u64().is_some() && actual.as_u64() == wanted.as_u64()
        } else {
            actual == wanted
        };
        require(
            valid,
            &format!("Unreviewed reference profile value/type: {name}"),
        )?;
    }
    require(
        profile["estimateScope"]
            .as_str()
            .is_some_and(|scope| !scope.trim().is_empty())
            && profile["remainingGates"].as_array().is_some_and(|gates| {
                !gates.is_empty()
                    && gates
                        .iter()
                        .all(|gate| gate.as_str().is_some_and(|text| !text.trim().is_empty()))
            }),
        "Reference profile needs explicit estimate scope and remaining gates.",
    )
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase", deny_unknown_fields)]
enum Request {
    Environment {
        names: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    Plan {
        profile: Value,
        model_path: String,
        key_file: String,
        port: u16,
        runtime_commit: String,
        model_revision: String,
        model_bytes: u64,
        environment_names: Vec<String>,
    },
}

/// Validate names or return the fixed launch-disabled reference server plan.
///
/// Requests select `operation: "environment"` with `names`, or `"plan"` with
/// `profile`, validated `modelPath`/`keyFile`, `port`, `runtimeCommit`,
/// `modelRevision`, `modelBytes`, and `environmentNames`. Only names cross the
/// bridge; inherited environment values must never be serialized.
///
/// # Errors
/// Rejects unknown or mistyped fields, profile drift, malformed artifact identity,
/// inherited model/runtime overrides, invalid path strings, and unsafe ports.
pub fn run(request: Value) -> Result<Value, String> {
    require(request.is_object(), "Server policy requires a JSON object.")?;
    let request: Request =
        serde_json::from_value(request).map_err(|_| "Invalid server policy request.")?;
    match request {
        Request::Environment { names } => {
            validate_environment(names)?;
            Ok(json!({ "Validated": true }))
        }
        Request::Plan {
            profile,
            model_path,
            key_file,
            port,
            runtime_commit,
            model_revision,
            model_bytes,
            environment_names,
        } => {
            validate_environment(environment_names)?;
            validate_reference_profile(&profile, model_bytes)?;
            require(
                hex(&runtime_commit, 40) && hex(&model_revision, 40) && model_bytes > 0,
                "Invalid model/runtime identity in server plan.",
            )?;
            let arguments = arguments(&model_path, &key_file, port, Profile::ReferenceCpu)?;
            Ok(json!({
                "SchemaVersion": 1, "Status": "preflight-only-launch-disabled",
                "LaunchEnabled": false, "ModelProcessStarted": false,
                "Profile": "reference-cpu", "RuntimeCommit": runtime_commit,
                "ModelRevision": model_revision, "ModelPath": model_path,
                "PlannedKeyFile": key_file, "Origin": format!("http://127.0.0.1:{port}"),
                "Arguments": arguments, "ServiceCeilingBytes": profile["serviceCeilingBytes"],
                "RequiredAvailableBytes": REQUIRED_AVAILABLE_BYTES,
                "ArtifactVerification": "not-performed", "RuntimeHelpVerification": "not-performed",
                "SecurityQualification": "not-qualified",
                "RemainingGates": [
                    "CPU loading/decode with memory admission; current watchdog is not an OS limit",
                    "Owner-private unpredictable key creation and authenticated lifecycle/readiness/cancellation",
                    "Host/Origin rejection, route allowlist and backend bypass prevention; CORS is not enforcement",
                    "Hard request/token/queue/duration limits; n-predict and timeout are defaults, not those limits",
                    "Restricted inference identity/filesystem and denied egress; offline is not an OS network boundary",
                    "Pinned/sanitized offline UI assets, publisher-template behavior and creative/comfort CI qualification",
                ],
            }))
        }
    }
}

#[cfg(test)]
mod tests;
