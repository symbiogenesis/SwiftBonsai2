//! Object-only evidence schemas. Required nullable members remain required.

use serde::{Deserialize, Deserializer, de::Error as _};
use serde_json::{Map, Value};

macro_rules! object {
    ($name:ident { $($field:ident : $ty:ty => $key:literal),* $(,)? } $(optional { $($opt:ident : $opt_ty:ty => $opt_key:literal),* $(,)? })?) => {
        #[allow(clippy::struct_field_names, reason = "Names mirror the committed evidence schema.")]
        pub(super) struct $name {
            $(pub(super) $field: $ty,)*
            $($(pub(super) $opt: Option<$opt_ty>,)*)?
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                // Map deserialization rejects positional arrays, including nested
                // arrays that ordinary derived Rust structs would accept.
                let mut fields = Map::<String, Value>::deserialize(deserializer)?;
                let result = Self {
                    $($field: serde_json::from_value(fields.remove($key)
                        .ok_or_else(|| D::Error::missing_field($key))?)
                        .map_err(D::Error::custom)?,)*
                    $($($opt: fields.remove($opt_key).map(serde_json::from_value)
                        .transpose().map_err(D::Error::custom)?,)*)?
                };
                if !fields.is_empty() {
                    return Err(D::Error::custom("Unrecognized qualification field."));
                }
                Ok(result)
            }
        }
    };
}

object!(Controls {
    model_sha256: String => "modelSha256",
    weight_representation: String => "weightRepresentation",
    runtime_commit: String => "runtimeCommit",
    template_sha256: String => "templateSha256",
    adapter_sha256: String => "adapterSha256",
    context_tokens: f64 => "contextTokens",
    batch_tokens: f64 => "batchTokens",
    microbatch_tokens: f64 => "microbatchTokens",
    threads: f64 => "threads",
    seed: f64 => "seed",
    max_output_tokens: f64 => "maxOutputTokens",
    sampler: Sampler => "sampler",
} optional { adapter_scale: f64 => "adapterScale" });
object!(Sampler {
    temperature: f64 => "temperature",
    top_p: f64 => "topP",
    top_k: f64 => "topK",
    min_p: f64 => "minP",
    repeat_penalty: f64 => "repeatPenalty",
});
object!(Evidence {
    schema_version: f64 => "schemaVersion",
    run_id: String => "runId",
    contract_sha256: String => "contractSha256",
    execution: Execution => "execution",
    controls: Controls => "controls",
    basis: Basis => "basis",
    samples: Vec<Sample> => "samples",
    zero_scale_checks: Vec<ZeroScale> => "zeroScaleChecks",
    nonzero_adapter_check: Nonzero => "nonzeroAdapterCheck",
});
object!(Execution {
    github_actions: bool => "githubActions",
    run_url: String => "runUrl",
    runner_architecture: String => "runnerArchitecture",
    device_model: String => "deviceModel",
    runner_name: String => "runnerName",
} optional { processor_name: String => "processorName" });
object!(Basis {
    status: String => "status",
    evidence_sha256: Option<String> => "evidenceSha256",
    reviewer_id: Option<String> => "reviewerId",
    note: Option<String> => "note",
});
object!(Sample {
    sample_id: String => "sampleId",
    prompt_id: String => "promptId",
    repeat: f64 => "repeat",
    arm: String => "arm",
    order: f64 => "order",
    seed: f64 => "seed",
    controls_sha256: String => "controlsSha256",
    decode_tokens: f64 => "decodeTokens",
    decode_milliseconds: f64 => "decodeMilliseconds",
    peak_job_memory_bytes: f64 => "peakJobMemoryBytes",
    minimum_available_physical_bytes: f64 => "minimumAvailablePhysicalBytes",
    completed: bool => "completed",
    response_text: String => "responseText",
    response_sha256: String => "responseSha256",
} optional { _diagnostics: Map<String, Value> => "diagnostics" });
object!(ZeroScale {
    prompt_id: String => "promptId",
    seed: f64 => "seed",
    baseline_response_sha256: String => "baselineResponseSha256",
    zero_scale_response_sha256: String => "zeroScaleResponseSha256",
    baseline_logits_sha256: Option<String> => "baselineLogitsSha256",
    zero_scale_logits_sha256: Option<String> => "zeroScaleLogitsSha256",
});
object!(Nonzero {
    status: String => "status",
    scale: f64 => "scale",
    adapter_sha256: String => "adapterSha256",
    logits_changed: Option<bool> => "logitsChanged",
    evidence_sha256: Option<String> => "evidenceSha256",
});
object!(Review {
    schema_version: f64 => "schemaVersion",
    run_id: String => "runId",
    contract_sha256: String => "contractSha256",
    source: String => "source",
    attestation: String => "attestation",
    reviews: Vec<ReviewRecord> => "reviews",
});
object!(ReviewRecord {
    sample_id: String => "sampleId",
    response_sha256: String => "responseSha256",
    reviewer_id: String => "reviewerId",
    human_reviewed: bool => "humanReviewed",
    reviewed_at_utc: String => "reviewedAtUtc",
    answer_complete: bool => "answerComplete",
    refused: bool => "refused",
    scores: Map<String, Value> => "scores",
    rationale: String => "rationale",
});
object!(Prompt {
    id: String => "id",
    category: String => "category",
    text: String => "text",
    _review_notes: String => "reviewNotes",
});
