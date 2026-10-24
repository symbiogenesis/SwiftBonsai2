//! Exact response identity and diagnostics for completed paired samples.

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::Result;

pub(super) fn decode(bytes: Vec<u8>) -> Result<String> {
    // Decode the bytes directly: text readers can consume a leading U+FEFF as
    // an encoding preamble, changing the response and its reviewed identity.
    String::from_utf8(bytes)
        .map_err(|error| format!("Native captured output is not valid UTF-8: {error}"))
}

pub(super) fn assemble(text: String, receipt: &Value) -> Value {
    let timing = &receipt["Timing"];
    let counts = &receipt["Counts"];
    let sha256: String = Sha256::digest(text.as_bytes())
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 15])
        .map(|part| char::from(b"0123456789abcdef"[usize::from(part)]))
        .collect();
    let mut captured = json!({
        "SchemaVersion": 1,
        "Sha256": sha256,
        "Diagnostics": {
            "WallMilliseconds": receipt["WallMilliseconds"],
            "PromptTokens": timing["PromptTokens"],
            "PromptMilliseconds": timing["PromptMilliseconds"],
            "DecodeTokensPerSecond": timing["DecodeTokensPerSecond"],
            "EogMarkerObserved": text.contains(" [end of text]"),
            "FirstTokenLatencyMilliseconds": null,
            "PromptTokenCount": counts["PromptTokenCount"],
            "SamplerAcceptedTokens": counts["SamplerAcceptedTokens"],
            "GeneratedSampledTokens": counts["GeneratedSampledTokens"],
            "LogitsAvailable": false,
            "ResponseScope": "Exact native stdout including reasoning, end marker and trailing newlines",
        },
    });
    captured["Text"] = Value::String(text);
    captured
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{assemble, decode};

    #[test]
    fn response_digest_keeps_empty_and_trailing_newline_identities() {
        let empty = assemble(String::new(), &json!({}));
        assert_eq!(
            empty["Sha256"],
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_ne!(
            assemble("answer\n".into(), &json!({}))["Sha256"],
            assemble("answer".into(), &json!({}))["Sha256"]
        );
    }

    #[test]
    fn strict_decode_preserves_bom_unicode_and_all_trailing_newlines() -> crate::Result<()> {
        let text = "\u{feff}caf\u{e9} \u{1f331} [end of text]\r\n\n";
        let captured = assemble(decode(text.as_bytes().to_vec())?, &json!({}));
        assert_eq!(captured["Text"], text);
        assert_eq!(
            captured["Sha256"],
            "df83f021218f20b60f48cbb5306a794f327336012fd749567bd1a8094531a0c1"
        );
        assert_eq!(captured["Diagnostics"]["EogMarkerObserved"], true);
        assert!(decode(vec![0xff]).is_err());
        assert!(decode(vec![0xc3, 0x28]).is_err());
        Ok(())
    }

    #[test]
    fn diagnostics_keep_the_final_supervision_observations() {
        let receipt = json!({
            "WallMilliseconds": 350,
            "Timing": {"PromptTokens": 15, "PromptMilliseconds": 100.5,
                "DecodeTokensPerSecond": 15.0},
            "Counts": {"PromptTokenCount": 15, "SamplerAcceptedTokens": 19,
                "GeneratedSampledTokens": 4},
        });
        let captured = assemble("response\n".into(), &receipt);
        assert_eq!(
            captured["Diagnostics"],
            json!({
                "WallMilliseconds": 350, "PromptTokens": 15, "PromptMilliseconds": 100.5,
                "DecodeTokensPerSecond": 15.0, "EogMarkerObserved": false,
                "FirstTokenLatencyMilliseconds": null, "PromptTokenCount": 15,
                "SamplerAcceptedTokens": 19, "GeneratedSampledTokens": 4,
                "LogitsAvailable": false,
                "ResponseScope": "Exact native stdout including reasoning, end marker and trailing newlines",
            })
        );
    }
}
