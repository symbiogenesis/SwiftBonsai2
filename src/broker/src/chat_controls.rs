//! Reject exact pinned token controls before caller text reaches chat rendering.
//!
//! The inventory is independently extracted from the original Swift GGUF's
//! `tokenizer.ggml.tokens` and `tokenizer.ggml.token_type` arrays. Its raw
//! tokenizer metadata digest is bound by the fixture below. The native Swift
//! renderer parses these type-3/type-4 spellings after template rendering.
//! Never remove text: deletion can expose another marker or change counting.

const SPELLINGS: [&str; 33] = [
    "<|endoftext|>",
    "<|im_start|>",
    "<|im_end|>",
    "<|object_ref_start|>",
    "<|object_ref_end|>",
    "<|box_start|>",
    "<|box_end|>",
    "<|quad_start|>",
    "<|quad_end|>",
    "<|vision_start|>",
    "<|vision_end|>",
    "<|vision_pad|>",
    "<|image_pad|>",
    "<|video_pad|>",
    "<tool_call>",
    "</tool_call>",
    "<|fim_prefix|>",
    "<|fim_middle|>",
    "<|fim_suffix|>",
    "<|fim_pad|>",
    "<|repo_name|>",
    "<|file_sep|>",
    "<tool_response>",
    "</tool_response>",
    "<think>",
    "</think>",
    "<|audio_start|>",
    "<|audio_end|>",
    "<tts_pad>",
    "<tts_text_bos>",
    "<tts_text_eod>",
    "<tts_text_bos_single>",
    "<|audio_pad|>",
];

/// No allocation or normalization; at most 33 fixed-size checks per `<` byte.
#[allow(
    clippy::redundant_pub_crate,
    reason = "Only the parent request policy uses this private module; a public export violates unreachable_pub."
)]
pub(super) fn contains(text: &str) -> bool {
    text.match_indices('<').any(|(offset, _)| {
        let remainder = &text[offset..];
        SPELLINGS
            .iter()
            .any(|spelling| remainder.starts_with(spelling))
    })
}

#[cfg(test)]
mod tests {
    use super::SPELLINGS;

    #[test]
    fn inventory_matches_locked_model_and_tokenizer_identity() -> Result<(), serde_json::Error> {
        let inventory: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/swift-control-tokens.json"))?;
        let metadata: serde_json::Value =
            serde_json::from_str(include_str!("../../../config/swift.metadata.json"))?;
        let lock: serde_json::Value =
            serde_json::from_str(include_str!("../../../config/swift-assessment.lock.json"))?;
        assert_eq!(inventory["model_repository"], lock["model"]["repository"]);
        assert_eq!(inventory["model_revision"], lock["model"]["revision"]);
        assert_eq!(inventory["model_sha256"], lock["model"]["sha256"]);
        assert_eq!(inventory["model_bytes"], lock["model"]["bytes"]);
        assert_eq!(
            inventory["tokenizer_metadata_sha256"],
            metadata["TokenizerMetadataSha256"]
        );
        let actual: Vec<_> = inventory["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|entry| entry["spelling"].as_str())
            .collect();
        let expected: Vec<_> = SPELLINGS.iter().copied().map(Some).collect();
        assert_eq!(actual, expected);
        assert!(
            inventory["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .all(|entry| matches!(entry["kind"].as_u64(), Some(3 | 4)))
        );
        Ok(())
    }
}
