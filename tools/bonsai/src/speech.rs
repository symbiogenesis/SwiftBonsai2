//! Native speech artifact plans from the inference crates' compiled model locks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::identity::is_lower_hex;
use bonsai_artifacts::validate_external_path;
use serde::Deserialize;
use serde_json::{Value, json};

const ASR_LOCK: &str = include_str!("../../../src/speech-asr/model.lock.json");
const TTS_LOCK: &str = include_str!("../../../src/speech-tts/model.lock.json");
const TTS_Q4_LOCK: &str = include_str!("../../../src/speech-tts/model-q4.lock.json");
const MAX_BYTES: u64 = (1_u64 << 63) - 1;

#[derive(Default, Deserialize)]
#[serde(try_from = "String")]
enum Model {
    #[default]
    All,
    Canary,
    QwenTts,
}

impl TryFrom<String> for Model {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.to_ascii_lowercase().as_str() {
            "all" => Ok(Self::All),
            "canary" => Ok(Self::Canary),
            "qwentts" => Ok(Self::QwenTts),
            _ => Err("Unsupported native speech model selection.".into()),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    data_directory: PathBuf,
    #[serde(default)]
    model: Model,
    #[serde(default)]
    tts_quantization: Quantization,
}

#[derive(Default, Deserialize)]
enum Quantization {
    #[default]
    #[serde(rename = "Q8_0")]
    Q8,
    #[serde(rename = "Q4_K_M")]
    Q4,
}

impl Quantization {
    const fn lock(&self) -> &'static str {
        match self {
            Self::Q8 => TTS_LOCK,
            Self::Q4 => TTS_Q4_LOCK,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Identity {
    schema_version: u32,
    model: String,
    repository: String,
    revision: String,
}

#[derive(Deserialize)]
struct Artifact {
    filename: String,
    url: String,
    bytes: u64,
    sha256: String,
}

fn atom(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}

fn files(root: &Path, kind: &str, source: &str) -> crate::Result<Vec<Value>> {
    let pin: Value = serde_json::from_str(source).map_err(|error| error.to_string())?;
    let identity: Identity =
        serde_json::from_value(pin.clone()).map_err(|error| error.to_string())?;
    let (name, directory, keys): (&str, &str, &[&str]) = match kind {
        "Canary" => ("nvidia/canary-qwen-2.5b", "canary", &["model"]),
        "QwenTts" => (
            "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign",
            "qwen-tts",
            &["talker", "codec"],
        ),
        _ => return Err("Unsupported native speech model.".into()),
    };
    if identity.schema_version != 1
        || identity.model != name
        || !identity
            .repository
            .split_once('/')
            .is_some_and(|(owner, repository)| atom(owner) && atom(repository))
        || !is_lower_hex(&identity.revision, 40)
    {
        return Err("Invalid native speech model identity.".into());
    }
    let artifacts: BTreeMap<String, Artifact> = if kind == "Canary" {
        BTreeMap::from([(
            "model".into(),
            serde_json::from_value(pin).map_err(|error| error.to_string())?,
        )])
    } else {
        serde_json::from_value(pin["files"].clone()).map_err(|error| error.to_string())?
    };
    if artifacts.len() != keys.len() || keys.iter().any(|key| !artifacts.contains_key(*key)) {
        return Err("Native speech model lock omitted a required artifact.".into());
    }
    let directory = root
        .join("speech")
        .join("native")
        .join(directory)
        .join(&identity.revision);
    let mut result = Vec::with_capacity(keys.len());
    for key in keys {
        let artifact = &artifacts[*key];
        let url = format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            identity.repository, identity.revision, artifact.filename
        );
        if artifact.filename.is_empty()
            || !artifact
                .filename
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.+-".contains(&byte))
            || artifact.bytes == 0
            || artifact.bytes > MAX_BYTES
            || !is_lower_hex(&artifact.sha256, 64)
            || artifact.url != url
        {
            return Err("Invalid native speech artifact identity.".into());
        }
        let path = directory.join(&artifact.filename);
        validate_external_path(&path).map_err(|error| error.to_string())?;
        result.push(json!({
            "Kind": kind, "Key": key, "Path": path, "Uri": url,
            "Bytes": artifact.bytes, "Sha256": artifact.sha256,
        }));
    }
    Ok(result)
}

/// Return a native GGUF download plan without networking, creating storage or loading models.
///
/// # Errors
/// Rejects malformed requests, unsupported compiled pins and unsafe external paths.
pub fn run(request: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    validate_external_path(&request.data_directory).map_err(|error| error.to_string())?;
    let mut plans = Vec::with_capacity(3);
    if matches!(request.model, Model::All | Model::Canary) {
        plans.extend(files(&request.data_directory, "Canary", ASR_LOCK)?);
    }
    if matches!(request.model, Model::All | Model::QwenTts) {
        plans.extend(files(
            &request.data_directory,
            "QwenTts",
            request.tts_quantization.lock(),
        )?);
    }
    let mut result = json!({
        "CanaryModelPath": null, "TtsModelPath": null, "TtsCodecPath": null,
    });
    let mut total = 0_u64;
    for file in &plans {
        let property = match (file["Kind"].as_str(), file["Key"].as_str()) {
            (Some("Canary"), _) => "CanaryModelPath",
            (_, Some("talker")) => "TtsModelPath",
            _ => "TtsCodecPath",
        };
        result[property] = file["Path"].clone();
        total = total
            .checked_add(
                file["Bytes"]
                    .as_u64()
                    .ok_or("Invalid speech artifact bytes.")?,
            )
            .filter(|bytes| *bytes <= MAX_BYTES)
            .ok_or("Speech artifact byte total exceeded its bound.")?;
    }
    result["TotalBytes"] = json!(total);
    result["Plans"] = json!(plans);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_plan_contract() -> crate::Result<()> {
        let root = std::env::temp_dir().join("bonsai-native-speech-plan-uncreated");
        let all = run(json!({"dataDirectory": root, "model": "All"}))?;
        let plans = all["Plans"].as_array().ok_or("Expected plans.")?;
        assert_eq!(plans.len(), 3);
        assert_eq!(all["TotalBytes"], 5_131_533_376_u64);
        assert_eq!(all["CanaryModelPath"], plans[0]["Path"]);
        assert_eq!(all["TtsModelPath"], plans[1]["Path"]);
        assert_eq!(all["TtsCodecPath"], plans[2]["Path"]);
        for (selection, count, absent) in [
            ("Canary", 1, "TtsModelPath"),
            ("qWeNtTs", 2, "CanaryModelPath"),
        ] {
            let plan = run(json!({"dataDirectory": root, "model": selection}))?;
            assert_eq!(plan["Plans"].as_array().map(Vec::len), Some(count));
            assert!(plan[absent].is_null());
        }
        assert!(!root.exists());
        for request in [
            json!({"dataDirectory": "relative", "model": "All"}),
            json!({"dataDirectory": root, "model": "Unknown"}),
            json!({"dataDirectory": root, "extra": true}),
        ] {
            assert!(run(request).is_err());
        }
        Ok(())
    }

    #[test]
    fn native_pins_reject_identity_drift() -> crate::Result<()> {
        let root = std::env::temp_dir().join("bonsai-native-speech-pin-uncreated");
        for (field, value) in [
            ("filename", json!("../escape.gguf")),
            ("url", json!("https://unreviewed.invalid/model.gguf")),
            ("bytes", json!(0)),
            ("bytes", json!(-1)),
            ("sha256", json!("bad")),
            ("revision", json!("main")),
            ("model", json!("unreviewed/model")),
        ] {
            let mut pin: Value =
                serde_json::from_str(ASR_LOCK).map_err(|error| error.to_string())?;
            pin[field] = value;
            assert!(files(&root, "Canary", &pin.to_string()).is_err());
        }
        let mut pin: Value = serde_json::from_str(TTS_LOCK).map_err(|error| error.to_string())?;
        pin["files"]["extra"] = pin["files"]["codec"].clone();
        assert!(files(&root, "QwenTts", &pin.to_string()).is_err());
        Ok(())
    }

    #[test]
    fn q4_is_explicit_and_reuses_the_pinned_q8_codec() -> crate::Result<()> {
        let root = std::env::temp_dir().join("bonsai-q4-speech-plan-uncreated");
        let q8 = run(json!({"dataDirectory": root, "model": "QwenTts"}))?;
        let q4 = run(json!({
            "dataDirectory": root, "model": "QwenTts", "ttsQuantization": "Q4_K_M",
        }))?;
        assert_ne!(q8["TtsModelPath"], q4["TtsModelPath"]);
        assert_eq!(q8["TtsCodecPath"], q4["TtsCodecPath"]);
        assert_eq!(q4["TotalBytes"], 1_473_781_440_u64);
        assert_eq!(
            q4["Plans"][0]["Sha256"],
            "7605ed0cc5e72059f27468c27f70c070e05d1cc0c7b1c76bfb9cba717a59eee3"
        );
        assert!(
            run(json!({
                "dataDirectory": root, "model": "QwenTts", "ttsQuantization": "Q2",
            }))
            .is_err()
        );
        Ok(())
    }
}
