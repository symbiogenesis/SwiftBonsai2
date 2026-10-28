//! Reusable scratch for the optional, qualification-only JSON candidate.

use crate::{AuthorizedHead, ClientChat, Object, PendingChat, Rejection};
use serde::Deserialize;

/// Bounded input and parser allocations retained between qualification calls.
pub(super) struct JsonCandidateScratch {
    input: Vec<u8>,
    buffers: simd_json::Buffers,
    // Resetting removes every input borrow before the tape is retained.
    tape: simd_json::Tape<'static>,
}

impl Default for JsonCandidateScratch {
    fn default() -> Self {
        Self {
            input: Vec::new(),
            buffers: simd_json::Buffers::default(),
            tape: simd_json::Tape(Vec::new()),
        }
    }
}

impl JsonCandidateScratch {
    /// Prepare the same canonical request as the production policy.
    pub(super) fn prepare(
        &mut self,
        head: AuthorizedHead,
        body: &[u8],
    ) -> Result<PendingChat, Rejection> {
        head.check_body_size(body)?;
        self.input
            .try_reserve_exact(body.len() + simd_json::INPUT_PADDING)
            .map_err(|_| Rejection::ChatSchema)?;
        self.input.extend_from_slice(body);
        self.input
            .resize(body.len() + simd_json::INPUT_PADDING, b' ');

        // The empty replacement allocates nothing. The retained tape can borrow
        // this call's input until reset clears its nodes and restores 'static.
        let mut tape: simd_json::Tape<'_> =
            std::mem::replace(&mut self.tape, simd_json::Tape(Vec::new()));
        let result = if simd_json::fill_tape_padded(
            &mut self.input,
            body.len(),
            &mut self.buffers,
            &mut tape,
        )
        .is_ok()
        {
            let mut deserializer = simd_json::Deserializer::from_tape(tape);
            let result = match Object::<ClientChat<'_>>::deserialize(&mut deserializer) {
                Ok(Object(chat)) if chat.abliteration_scale.is_none() => head.prepare_chat(&chat),
                // Preserve production decimal conversion and value-free errors,
                // including escaped scale keys and partially consumed tapes.
                Ok(_) | Err(_) => head.prepare(body),
            };
            self.tape = deserializer.into_tape().reset();
            result
        } else {
            // Failed parsing can also leave borrowed nodes in a partial tape.
            self.tape = tape.reset();
            head.prepare(body)
        };
        self.input.clear();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::JsonCandidateScratch;
    use crate::{AuthorizedHead, MAX_BODY_BYTES, Rejection};

    fn compare(
        scratch: &mut JsonCandidateScratch,
        body: &[u8],
        declared_length: usize,
        pinned: bool,
    ) {
        let original = body.to_vec();
        let head = || AuthorizedHead {
            length: declared_length,
            pinned_abliteration: pinned,
        };
        let summarize =
            |chat: crate::PendingChat| (chat.body, chat.max_completion_tokens, chat.stream);
        assert_eq!(
            scratch.prepare(head(), body).map(summarize),
            head().prepare(body).map(summarize)
        );
        assert_eq!(body, original);
        assert!(scratch.input.is_empty());
        assert!(scratch.tape.0.is_empty());
    }

    #[test]
    fn reuse_preserves_parity_through_failures_and_size_changes() {
        let mut scratch = JsonCandidateScratch::default();
        let valid = br#"{"messages":[{"role":"user","content":"rain"}]}"#;
        for _ in 0..3 {
            for body in [
                valid.as_slice(),
                br#"{"messages":[{"role":"user","content":"\ud800"}]}"#,
                br#"{"messages":[{"role":"user","content":"unterminated"}]"#,
                br#"{"messages":[{"role":"user","content":"a","content":"b"}]}"#,
                br#"{"messages":[{"role":"user","content":"a","extra":0}]}"#,
                br#"{"messages":[["user","a"]]}"#,
                br#"{"messages":[{"role":"assistant","content":"a"}]}"#,
                br#"{"messages":[{"role":"user","content":"a"}],"max_completion_tokens":0}"#,
                br#"{"messages":[{"role":"user","content":"snow: \u96ea; tree: \ud83c\udf33\n"}],"stream":true}"#,
            ] {
                compare(&mut scratch, body, body.len(), false);
                compare(&mut scratch, valid, valid.len(), false);
            }
            for count in [4096, 16, 64 * 1024, 1] {
                let body = format!(
                    r#"{{"messages":[{{"role":"user","content":"{}"}}]}}"#,
                    "x".repeat(count)
                );
                compare(&mut scratch, body.as_bytes(), body.len(), false);
            }
        }
        compare(&mut scratch, valid, valid.len() + 1, false);
        let oversized = vec![b' '; MAX_BODY_BYTES + 1];
        compare(&mut scratch, &oversized, oversized.len(), false);
        assert_eq!(
            scratch
                .prepare(
                    AuthorizedHead {
                        length: oversized.len(),
                        pinned_abliteration: false,
                    },
                    &oversized,
                )
                .err(),
            Some(Rejection::BodySize)
        );
        compare(&mut scratch, valid, valid.len(), false);
    }

    #[test]
    fn reuse_preserves_decimal_fallback_and_retains_allocations() {
        let mut scratch = JsonCandidateScratch::default();
        let prime = br#"{"messages":[{"role":"system","content":"scene"},{"role":"user","content":"rain"}],"stream":true,"max_completion_tokens":4096}"#;
        compare(&mut scratch, prime, prime.len(), false);
        let input_pointer = scratch.input.as_ptr();
        let input_capacity = scratch.input.capacity();
        let tape_pointer = scratch.tape.0.as_ptr();
        let tape_capacity = scratch.tape.0.capacity();
        assert!(tape_capacity > 0);
        for _ in 0..3 {
            for body in [
                br#"{"messages":[{"role":"user","content":"rain"}]}"#.as_slice(),
                br#"{"messages":[{"role":"user","content":"rain"}],"abliteration_scale":1.2785641882898677}"#,
                br#"{"messages":[{"role":"user","content":"rain"}],"abliteration_\u0073cale":1.6447349639294147}"#,
                br#"{"messages":[{"role":"user","content":"rain"}],"abliteration_scale":2.0000000000000001}"#,
                br#"{"messages":[{"role":"user","content":"rain"}],"abliteration_scale":null}"#,
                br#"{"messages":[{"role":"user","content":"rain"}],"abliteration_scale":1e9999}"#,
                br#"{"messages":[{"role":"user","content":"\ud800"}]}"#,
                br#"{"messages":[{"role":"user","content":"rain","extra":0}]}"#,
            ] {
                for pinned in [false, true] {
                    compare(&mut scratch, body, body.len(), pinned);
                    assert_eq!(scratch.input.as_ptr(), input_pointer);
                    assert_eq!(scratch.input.capacity(), input_capacity);
                    assert_eq!(scratch.tape.0.as_ptr(), tape_pointer);
                    assert_eq!(scratch.tape.0.capacity(), tape_capacity);
                }
            }
        }
    }
}
