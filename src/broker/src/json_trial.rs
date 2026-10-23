//! Opt-in dependency experiments over the production broker schema.
//!
//! These entry points include validation and canonical serialization. They do
//! not authorize HTTP requests or bypass prompt token admission. The mutable
//! input copy required by `simd-json` is deliberately included in its path.

use super::{
    AuthorizedHead, ClientChat, MAX_BODY_BYTES, MAX_COMPLETION_TOKENS, MAX_MESSAGES, Message,
    Object, PendingChat, Rejection, Role, backend_chat, canonicalize_chat, default_cap,
    default_stream,
};
use serde::{Deserialize, Deserializer, de};
use std::fmt;

/// An end-to-end candidate for bounded JSON validation and canonicalization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// Production Serde schema and `serde_json` reader and writer.
    SerdeJson,
    /// Sonic's reader with the production schema and canonical writer.
    Sonic,
    /// SIMD-JSON's reader, including its input copy, with the canonical writer.
    SimdJson,
    /// Schema-specific streaming transcoding over Serde's JSON lexer.
    BespokeSerde,
}

impl Engine {
    /// All candidates, in a stable reporting order.
    pub const ALL: [Self; 4] = [
        Self::SerdeJson,
        Self::Sonic,
        Self::SimdJson,
        Self::BespokeSerde,
    ];

    /// Stable name suitable for measurement artifacts.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::SerdeJson => "serde-json",
            Self::Sonic => "sonic",
            Self::SimdJson => "simd-json-copy",
            Self::BespokeSerde => "bespoke-serde",
        }
    }
}

/// Canonical bytes and validated request flags, before token admission.
pub struct Prepared {
    pending: PendingChat,
}

impl Prepared {
    /// Bytes that must be used unchanged for token counting and generation.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.pending.body
    }

    /// Validated completion budget, including the production default.
    #[must_use]
    pub const fn max_completion_tokens(&self) -> u32 {
        self.pending.max_completion_tokens
    }

    /// Validated streaming flag, including the production default.
    #[must_use]
    pub const fn streams_response(&self) -> bool {
        self.pending.stream
    }
}

/// Validate and canonicalize one bounded request with the selected candidate.
///
/// This deliberately starts from immutable input for every engine, matching
/// the production reception interface. No benchmark may reuse SIMD-JSON's
/// already modified input to omit its per-request ownership cost.
///
/// # Errors
/// Returns the production value-free rejection for invalid size or schema,
/// unsupported history, invalid completion budget, or serialization failure.
pub fn prepare(body: &[u8], engine: Engine) -> Result<Prepared, Rejection> {
    if body.is_empty() || body.len() > MAX_BODY_BYTES {
        return Err(Rejection::BodySize);
    }
    let pending = match engine {
        Engine::SerdeJson => AuthorizedHead { length: body.len() }.prepare(body)?,
        Engine::Sonic => {
            let Object(chat): Object<ClientChat<'_>> =
                sonic_rs::from_slice(body).map_err(|_| Rejection::ChatSchema)?;
            canonicalize_chat(&chat)?
        }
        Engine::SimdJson => {
            let mut mutable = body.to_vec();
            let Object(chat): Object<ClientChat<'_>> =
                simd_json::serde::from_slice(&mut mutable).map_err(|_| Rejection::ChatSchema)?;
            canonicalize_chat(&chat)?
        }
        Engine::BespokeSerde => prepare_bespoke(body)?,
    };
    Ok(Prepared { pending })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalChat {
    messages: CanonicalMessages,
    #[serde(default = "default_cap")]
    max_completion_tokens: u32,
    #[serde(default = "default_stream")]
    stream: bool,
}

// The bespoke candidate keeps each decoded Message only until it has been
// serialized. It avoids a Vec<Message>, retaining only the final output buffer
// and, when needed, a single escaped string's decoded allocation. Object and
// Message are the exact same strict schema adapters used in production.
struct CanonicalMessages(Vec<u8>);

const EMPTY_MESSAGES_PREFIX: &[u8] = br#"{"messages":[]"#;

impl<'de> Deserialize<'de> for CanonicalMessages {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CanonicalMessagesVisitor;

        impl<'de> de::Visitor<'de> for CanonicalMessagesVisitor {
            type Value = CanonicalMessages;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a bounded array of message objects")
            }

            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut output = br#"{"messages":["#.to_vec();
                let mut count = 0;
                let mut turns = 0;
                while let Some(Object(message)) = sequence.next_element::<Object<Message<'de>>>()? {
                    if count == MAX_MESSAGES {
                        return Err(de::Error::custom("too many messages"));
                    }
                    if count != 0 || message.role != Role::System {
                        let expected = if turns % 2 == 0 {
                            Role::User
                        } else {
                            Role::Assistant
                        };
                        if message.role != expected {
                            return Err(de::Error::custom("invalid history"));
                        }
                        turns += 1;
                    }
                    if count != 0 {
                        output.push(b',');
                    }
                    serde_json::to_writer(&mut output, &message)
                        .map_err(|_| de::Error::custom("canonical serialization failed"))?;
                    count += 1;
                }
                if turns == 0 || turns % 2 == 0 {
                    return Err(de::Error::custom("invalid history"));
                }
                output.push(b']');
                Ok(CanonicalMessages(output))
            }
        }

        deserializer.deserialize_seq(CanonicalMessagesVisitor)
    }
}

fn prepare_bespoke(body: &[u8]) -> Result<PendingChat, Rejection> {
    let Object(chat): Object<CanonicalChat> =
        serde_json::from_slice(body).map_err(|_| Rejection::ChatSchema)?;
    if !(1..=MAX_COMPLETION_TOKENS).contains(&chat.max_completion_tokens) {
        return Err(Rejection::CompletionLimit);
    }
    let CanonicalMessages(mut body) = chat.messages;
    let tail_start = body.len();
    // Generate the options with the production serializer so every constant,
    // field order and float encoding has a single source of truth. Appending
    // then removing the empty-messages prefix keeps one final output buffer.
    serde_json::to_writer(
        &mut body,
        &backend_chat(&[], chat.max_completion_tokens, chat.stream),
    )
    .map_err(|_| Rejection::ChatSchema)?;
    if !body[tail_start..].starts_with(EMPTY_MESSAGES_PREFIX) {
        return Err(Rejection::ChatSchema);
    }
    body.copy_within(tail_start + EMPTY_MESSAGES_PREFIX.len().., tail_start);
    body.truncate(body.len() - EMPTY_MESSAGES_PREFIX.len());
    Ok(PendingChat {
        body,
        max_completion_tokens: chat.max_completion_tokens,
        stream: chat.stream,
    })
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "synthetic fixtures and successful test assertions must fail on error"
    )]

    use super::{Engine, MAX_BODY_BYTES, MAX_MESSAGES, Prepared, Rejection, prepare};

    fn comparable(result: Result<Prepared, Rejection>) -> Result<(Vec<u8>, u32, bool), Rejection> {
        result.map(|value| {
            (
                value.body().to_vec(),
                value.max_completion_tokens(),
                value.streams_response(),
            )
        })
    }

    fn differential(body: &[u8]) {
        let expected = comparable(prepare(body, Engine::SerdeJson));
        // SIMD-JSON is still screened by the executable, but its pinned reader
        // accepts a lone high surrogate. It is not a strict-policy candidate.
        for engine in [Engine::SerdeJson, Engine::Sonic, Engine::BespokeSerde] {
            assert_eq!(
                comparable(prepare(body, engine)),
                expected,
                "engine {} on synthetic fixture {:?}",
                engine.name(),
                body
            );
        }
    }

    #[test]
    fn simd_candidate_is_ineligible_for_the_strict_unicode_contract() {
        let body = br#"{"messages":[{"role":"user","content":"\ud800"}]}"#;
        assert!(matches!(
            prepare(body, Engine::SerdeJson),
            Err(Rejection::ChatSchema)
        ));
        assert!(prepare(body, Engine::SimdJson).is_ok());
    }

    #[test]
    fn candidates_preserve_canonical_bytes_flags_and_unicode() {
        for body in [
            r#"{"messages":[{"role":"user","content":"hi"}]}"#,
            r#" { "max_completion_tokens":1, "messages":[{"content":"\u96ea\u000a\u0022\u005c\u0000\ud83d\ude03","role":"u\u0073er"}], "stream":false } "#,
            r#"{"stream":true,"messages":[{"role":"system","content":"雪"},{"role":"user","content":"/plain"},{"role":"assistant","content":"\b\f\r\t"},{"content":"","role":"user"}],"max_completion_tokens":4096}"#,
        ] {
            assert!(prepare(body.as_bytes(), Engine::SerdeJson).is_ok());
            differential(body.as_bytes());
        }
    }

    #[test]
    fn candidates_reject_the_same_malformed_schema_and_encodings() {
        for body in [
            "",
            "null",
            "[]",
            "{}",
            r#"{"messages":[]}"#,
            r#"{"messages":null}"#,
            r#"{"messages":[["user","hi"]]}"#,
            r#"[[{"role":"user","content":"hi"}],1,false]"#,
            r#"{"messages":[{"role":{"user":null},"content":"hi"}]}"#,
            r#"{"messages":[{"role":null,"content":"hi"}]}"#,
            r#"{"messages":[{"role":true,"content":"hi"}]}"#,
            r#"{"messages":[{"role":0,"content":"hi"}]}"#,
            r#"{"messages":[{"role":["user"],"content":"hi"}]}"#,
            r#"{"messages":[{"role":"tool","content":"hi"}]}"#,
            r#"{"messages":[{"role":"user","content":null}]}"#,
            r#"{"messages":[{"role":"user","content":[]}]}"#,
            r#"{"messages":[{"role":"user"}]}"#,
            r#"{"messages":[{"role":"user","content":"hi","unknown":1}]}"#,
            r#"{"messages":[{"role":"user","content":"hi"}],"unknown":1}"#,
            r#"{"messages":[{"role":"user","content":"hi"}],"stream":null}"#,
            r#"{"messages":[{"role":"user","content":"hi"}],"max_completion_tokens":null}"#,
            r#"{"messages":[{"role":"user","content":"hi"}],"max_completion_tokens":-1}"#,
            r#"{"messages":[{"role":"user","content":"hi"}],"max_completion_tokens":1.0}"#,
            r#"{"messages":[{"role":"user","content":"hi"}],"max_completion_tokens":1e0}"#,
            r#"{"messages":[{"role":"user","content":"hi"}],"max_completion_tokens":"1"}"#,
            r#"{"messages":[{"role":"user","content":"hi"}],"max_completion_tokens":4294967296}"#,
            r#"{"messages":[{"role":"user","content":"hi"}],"stream":true,"str\u0065am":false}"#,
            r#"{"messages":[{"role":"user","content":"hi"}],"max_completion_tokens":1,"max_completion_\u0074okens":4096}"#,
            r#"{"messages":[{"role":"user","r\u006fle":"user","content":"hi"}]}"#,
            r#"{"messages":[{"role":"user","content":"hi","cont\u0065nt":"other"}]}"#,
            r#"{"messages":[{"role":"user","content":"hi"}],"m\u0065ssages":[]}"#,
            r#"{"messages":[{"role":"user","content":"\ud800"}]}"#,
            r#"{"messages":[{"role":"user","content":"\udc00"}]}"#,
            r#"{"messages":[{"role":"user","content":"\ud800\u0000"}]}"#,
            r#"{"messages":[{"role":"user","content":"\x00"}]}"#,
            r#"{"messages":[{"role":"user","content":"hi"}]} {}"#,
            "{\"messages\":[{\"role\":\"user\",\"content\":\"raw\nline\"}]}",
        ] {
            assert!(prepare(body.as_bytes(), Engine::SerdeJson).is_err());
            differential(body.as_bytes());
        }
        for cap in [0, 4097, u32::MAX] {
            differential(
                format!(
                    r#"{{"messages":[{{"role":"user","content":"hi"}}],"max_completion_tokens":{cap}}}"#
                )
                .as_bytes(),
            );
        }
        for invalid in [
            b"\xff".as_slice(),
            b"{\"messages\":[{\"role\":\"user\",\"content\":\"\xff\"}]}",
            b"{\"messages\":[{\"role\":\"user\",\"content\":\"\xc0\xaf\"}]}",
            b"{\"messages\":[{\"role\":\"user\",\"content\":\"\xed\xa0\x80\"}]}",
        ] {
            differential(invalid);
        }
    }

    #[test]
    fn candidates_agree_on_all_short_role_histories_and_truncations() {
        for length in 0..=6 {
            for mut word in 0..3_usize.pow(length) {
                let messages: Vec<_> = (0..length)
                    .map(|_| {
                        let role = ["system", "user", "assistant"][word % 3];
                        word /= 3;
                        serde_json::json!({"role":role,"content":"synthetic"})
                    })
                    .collect();
                differential(
                    &serde_json::to_vec(&serde_json::json!({"messages":messages})).unwrap(),
                );
            }
        }
        let body = br#"{"messages":[{"role":"user","content":"quote:\" and \ud83d\ude03"}],"stream":false,"max_completion_tokens":1}"#;
        for end in 0..body.len() {
            differential(&body[..end]);
        }
    }

    #[test]
    fn candidates_preserve_exact_message_and_body_limits() {
        for count in [MAX_MESSAGES - 1, MAX_MESSAGES, MAX_MESSAGES + 1] {
            let messages: Vec<_> = (0..count)
                .map(|index| {
                    let role = if count % 2 == 0 && index == 0 {
                        "system"
                    } else if index % 2 == count % 2 {
                        "assistant"
                    } else {
                        "user"
                    };
                    serde_json::json!({"role":role,"content":"\"雪\n"})
                })
                .collect();
            differential(&serde_json::to_vec(&serde_json::json!({"messages":messages})).unwrap());
        }
        let mut body = br#"{"messages":[{"role":"user","content":""#.to_vec();
        let suffix = br#""}]}"#;
        body.resize(MAX_BODY_BYTES - suffix.len(), b'x');
        body.extend_from_slice(suffix);
        assert_eq!(body.len(), MAX_BODY_BYTES);
        assert!(prepare(&body, Engine::SerdeJson).is_ok());
        differential(&body);
        body.push(b' ');
        differential(&body);
    }
}
