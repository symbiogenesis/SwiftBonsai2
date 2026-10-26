//! M1 request policy, bounded HTTP I/O and explicit development orchestration.
//!
//! `reception` bounds one supplied stream while preserving header multiplicity.
//! `response` frames a bounded, already HTTP-decoded backend response.
//! `backend` sends canonical requests and decodes HTTP on supplied private streams.
//! `scheduler` bounds reservations and a single active request; `supervisor`
//! joins reception, exact counting, generation and disconnect cancellation.
//! `service` can explicitly bind a numeric loopback listener for development.
//! `runtime` supplies an explicit development runner with bounded concurrency.
//! `launcher` explicitly owns a pinned development backend on Windows, using
//! separate audited Win32 dependencies for process and credential operations.
//! Backend bypass prevention and daily OS isolation still require qualification.
//! This library alone is not a qualified HTTP or process-isolation boundary.
#![forbid(unsafe_code)]

pub mod artifacts;
pub mod backend;
mod configuration;
#[cfg(windows)]
mod inference;
pub mod launch_policy;
#[cfg(windows)]
pub mod launcher;
pub mod reception;
pub mod response;
pub mod runtime;
pub mod scheduler;
pub mod service;
pub mod supervisor;
pub mod transport;
mod wire;

#[cfg(all(test, feature = "json-candidate"))]
mod json_candidate;

use http::{HeaderMap, HeaderValue, Method, Version, request::Parts, uri::PathAndQuery};
use serde::{Deserialize, Deserializer, Serialize, de};
use std::{borrow::Cow, fmt, marker::PhantomData};
use subtle::ConstantTimeEq;

/// Largest accepted client or canonical backend request body in bytes.
pub const MAX_BODY_BYTES: usize = 1024 * 1024;
/// Largest accepted HTTP header block in bytes.
pub const MAX_HEADER_BYTES: usize = 16 * 1024;
/// Largest accepted number of HTTP header fields, including duplicates.
pub const MAX_HEADERS: usize = 64;
/// Largest accepted number of messages in one chat history.
pub const MAX_MESSAGES: usize = 128;
/// Largest accepted number of text content parts in one message.
pub const MAX_CONTENT_PARTS: usize = 128;
/// Stable public alias for the broker's already selected and verified model.
pub const MODEL_ALIAS: &str = "bonsai";
/// Largest allowed completion token budget.
pub const MAX_COMPLETION_TOKENS: u32 = 4096;
/// Total prompt and completion context budget of the pinned model.
pub const CONTEXT_TOKENS: usize = 8192;
/// Context reserved for template accounting differences.
pub const TEMPLATE_MARGIN_TOKENS: usize = 64;

/// Declared inference capabilities of the broker's verified text-only profile.
///
/// Native speech recognition/synthesis belongs to the desktop client and does
/// not expand these model capabilities. Media parts remain rejected until a
/// verified matching model/projector and exact media-token admission exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ModelCapabilities {
    /// Stable public model alias; this never selects a caller-provided model.
    pub id: &'static str,
    /// Supported inference inputs, independent of native speech facilities.
    pub input_modalities: &'static [&'static str],
    /// Supported inference outputs, independent of native speech facilities.
    pub output_modalities: &'static [&'static str],
    /// Total prompt, completion and reserved context bound.
    pub context_tokens: usize,
    /// Maximum requested completion, including model reasoning.
    pub max_completion_tokens: u32,
}

/// Capabilities supported by both currently released broker launch profiles.
pub const MODEL_CAPABILITIES: ModelCapabilities = ModelCapabilities {
    id: MODEL_ALIAS,
    input_modalities: &["text"],
    output_modalities: &["text"],
    context_tokens: CONTEXT_TOKENS,
    max_completion_tokens: MAX_COMPLETION_TOKENS,
};

/// Deliberately contains no input values, parser diagnostics, keys or prompts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    /// The private endpoint port or credential format is invalid.
    Configuration,
    /// Header count, size, multiplicity or forwarding metadata is invalid.
    Headers,
    /// The private authorization credential is missing or invalid.
    Unauthorized,
    /// The Host header does not identify the private endpoint.
    Host,
    /// The supplied Origin header does not match the private endpoint.
    Origin,
    /// The HTTP method, version or request target is unsupported.
    Route,
    /// The request has unsupported or ambiguous HTTP framing.
    Framing,
    /// The request does not declare the supported JSON content type.
    ContentType,
    /// The declared, actual or canonical body length is invalid or exceeds the cap.
    BodySize,
    /// The body does not match the strict chat schema or role sequence.
    ChatSchema,
    /// The requested completion token budget is outside the allowed range.
    CompletionLimit,
    /// The trusted counter could not provide a positive prompt token count.
    TokenCountUnavailable,
    /// The prompt, completion and template margin exceed the context budget.
    ContextLimit,
}

/// No Debug/Serialize implementation: credentials must not enter diagnostics.
pub struct RequestPolicy {
    authority: String,
    origin: String,
    authorization: [u8; 71], // "Bearer " + 64 lowercase hex characters
    pinned_abliteration: bool,
}

impl RequestPolicy {
    /// The launcher must supply 32 cryptographically random bytes encoded as
    /// lowercase hex. Format validation here cannot establish randomness or ACLs.
    ///
    /// # Errors
    /// Returns [`Rejection::Configuration`] for a privileged port or invalid key.
    pub fn new(port: u16, key: &str) -> Result<Self, Rejection> {
        if port < 1024 || !configuration::valid_key(key.as_bytes()) {
            return Err(Rejection::Configuration);
        }
        let authority = format!("127.0.0.1:{port}");
        let mut authorization = [0; 71];
        authorization[..7].copy_from_slice(b"Bearer ");
        authorization[7..].copy_from_slice(key.as_bytes());
        Ok(Self {
            origin: format!("http://{authority}"),
            authority,
            authorization,
            pinned_abliteration: false,
        })
    }

    /// Check the parsed head before the transport accepts a bounded body.
    /// Only HTTP/1.1 origin-form POST /v1/chat/completions is supported.
    /// OPTIONS, readiness, UI, tokenization and administrative routes are closed.
    ///
    /// # Errors
    /// Returns a value-free [`Rejection`] for invalid credentials, headers, route,
    /// content type, framing or body length.
    pub fn check_head(&self, head: &Parts) -> Result<AuthorizedHead, Rejection> {
        let headers = &head.headers;
        if headers.len() > MAX_HEADERS {
            return Err(Rejection::Headers);
        }
        let bytes = headers.iter().try_fold(0usize, |total, (name, value)| {
            total.checked_add(name.as_str().len() + value.as_bytes().len() + 4)
        });
        if bytes.is_none_or(|n| n > MAX_HEADER_BYTES) {
            return Err(Rejection::Headers);
        }
        let auth = singleton(headers, "authorization")?.ok_or(Rejection::Unauthorized)?;
        if auth.len() != self.authorization.len()
            || !bool::from(auth.ct_eq(self.authorization.as_slice()))
        {
            return Err(Rejection::Unauthorized);
        }
        if singleton(headers, "host")? != Some(self.authority.as_bytes()) {
            return Err(Rejection::Host);
        }
        if let Some(origin) = singleton(headers, "origin")?
            && origin != self.origin.as_bytes()
        {
            return Err(Rejection::Origin);
        }
        if head.method != Method::POST
            || head.version != Version::HTTP_11
            || head.uri.scheme().is_some()
            || head.uri.authority().is_some()
            || head.uri.path_and_query().map(PathAndQuery::as_str) != Some("/v1/chat/completions")
        {
            return Err(Rejection::Route);
        }
        for name in [
            "transfer-encoding",
            "content-encoding",
            "trailer",
            "expect",
            "upgrade",
        ] {
            if headers.contains_key(name) {
                return Err(Rejection::Framing);
            }
        }
        if headers
            .keys()
            .any(|name| name.as_str() == "forwarded" || name.as_str().starts_with("x-forwarded-"))
        {
            return Err(Rejection::Headers);
        }
        if let Some(connection) = singleton(headers, "connection")?
            && !connection.eq_ignore_ascii_case(b"close")
            && !connection.eq_ignore_ascii_case(b"keep-alive")
        {
            return Err(Rejection::Framing);
        }
        let content_type = singleton(headers, "content-type")?.ok_or(Rejection::ContentType)?;
        if !content_type.eq_ignore_ascii_case(b"application/json")
            && !content_type.eq_ignore_ascii_case(b"application/json; charset=utf-8")
        {
            return Err(Rejection::ContentType);
        }
        let length = singleton(headers, "content-length")?.ok_or(Rejection::Framing)?;
        let length = wire::content_length(length).map_err(|error| match error {
            wire::LengthError::Syntax => Rejection::Framing,
            wire::LengthError::Overflow => Rejection::BodySize,
        })?;
        if length == 0 || length > MAX_BODY_BYTES {
            return Err(Rejection::BodySize);
        }
        Ok(AuthorizedHead {
            length,
            pinned_abliteration: self.pinned_abliteration,
        })
    }
}

fn singleton<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a [u8]>, Rejection> {
    let mut values = headers.get_all(name).iter();
    let first = values.next().map(HeaderValue::as_bytes);
    if values.next().is_some() {
        return Err(Rejection::Headers);
    }
    Ok(first)
}

/// Constructed only by successful head validation; does not retain the key.
pub struct AuthorizedHead {
    length: usize,
    pinned_abliteration: bool,
}

impl AuthorizedHead {
    /// Validate a bounded JSON chat body and construct canonical backend bytes.
    ///
    /// # Errors
    /// Returns [`Rejection`] for a mismatched or oversized canonical body, invalid
    /// schema, unsupported message history, completion limit or serialization failure.
    pub fn prepare(self, body: &[u8]) -> Result<PendingChat, Rejection> {
        self.check_body_size(body)?;
        // Deserialize directly: maps/Value would silently erase duplicate keys.
        // Parser error strings can include private values; never propagate them.
        let Object(chat): Object<ClientChat<'_>> =
            serde_json::from_slice(body).map_err(|_| Rejection::ChatSchema)?;
        self.prepare_chat(&chat)
    }

    /// Qualify the pinned SIMD-JSON candidate against ordinary request parsing.
    ///
    /// Copies and pads the bounded immutable body because SIMD-JSON parses in
    /// place. The padded tape path avoids a second internal input copy.
    /// Schema, history, settings and canonical serialization use the production
    /// policy. Requests with a decoded `abliteration_scale` field, and candidate
    /// parsing failures, use the production parser to retain its exact numeric
    /// and rejection semantics. No service route selects this parser.
    ///
    /// # Errors
    /// Returns the same value-free rejections as [`Self::prepare`]. Allocation
    /// failures return [`Rejection::ChatSchema`].
    #[cfg(feature = "json-candidate")]
    pub fn prepare_simd_json(self, body: &[u8]) -> Result<PendingChat, Rejection> {
        self.check_body_size(body)?;
        let mut mutable = Vec::new();
        mutable
            .try_reserve_exact(body.len() + simd_json::INPUT_PADDING)
            .map_err(|_| Rejection::ChatSchema)?;
        mutable.extend_from_slice(body);
        mutable.resize(body.len() + simd_json::INPUT_PADDING, b' ');
        let mut buffers = simd_json::Buffers::default();
        let mut tape = simd_json::Tape::null();
        let Ok(Object(chat)): Result<Object<ClientChat<'_>>, _> =
            simd_json::fill_tape_padded(&mut mutable, body.len(), &mut buffers, &mut tape)
                .and_then(|()| tape.deserialize())
        else {
            drop(mutable);
            return self.prepare(body);
        };
        // The parsers can round the same decimal scale to adjacent F64 values.
        // Retain the production lexeme conversion, including escaped key names
        // and numeric values near admission boundaries, instead of changing it.
        if chat.abliteration_scale.is_some() {
            drop(chat);
            drop(mutable);
            return self.prepare(body);
        }
        self.prepare_chat(&chat)
    }

    const fn check_body_size(&self, body: &[u8]) -> Result<(), Rejection> {
        if body.len() != self.length || body.len() > MAX_BODY_BYTES {
            Err(Rejection::BodySize)
        } else {
            Ok(())
        }
    }

    fn prepare_chat(self, chat: &ClientChat<'_>) -> Result<PendingChat, Rejection> {
        if !valid_history(&chat.messages) {
            return Err(Rejection::ChatSchema);
        }
        if !(1..=MAX_COMPLETION_TOKENS).contains(&chat.max_completion_tokens) {
            return Err(Rejection::CompletionLimit);
        }
        if chat.abliteration_scale.is_some() && !self.pinned_abliteration {
            return Err(Rejection::ChatSchema);
        }
        let lora = self.pinned_abliteration.then(|| {
            [PinnedLora {
                id: 0,
                scale: chat.abliteration_scale.unwrap_or(1.3),
            }]
        });
        let max_completion_tokens = chat.max_completion_tokens;
        let body = serde_json::to_vec(&BackendChat {
            messages: &chat.messages,
            max_completion_tokens,
            stream: chat.stream,
            lora,
            n: 1,
            cache_prompt: false,
            n_cache_reuse: 0,
            ignore_eos: false,
            temperature: 1.0,
            top_p: 0.95,
            top_k: 20,
            min_p: 0.0,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            repeat_penalty: 1.0,
            chat_template_kwargs: ChatTemplateKwargs {
                enable_thinking: chat.enable_thinking,
                reasoning_effort: "xhigh",
                preserve_thinking: true,
            },
        })
        .map_err(|_| Rejection::ChatSchema)?;
        // Fixed sampler/template fields can grow an otherwise valid client
        // body past the private backend's identical request-byte limit.
        if body.len() > MAX_BODY_BYTES {
            return Err(Rejection::BodySize);
        }
        Ok(PendingChat {
            body,
            max_completion_tokens,
            stream: chat.stream,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientChat<'a> {
    #[serde(default, rename = "model", deserialize_with = "deserialize_model")]
    _model: (),
    #[serde(borrow, deserialize_with = "deserialize_messages")]
    messages: Vec<Message<'a>>,
    #[serde(default = "default_cap")]
    max_completion_tokens: u32,
    #[serde(default = "default_stream")]
    stream: bool,
    #[serde(default)]
    enable_thinking: bool,
    #[serde(default, deserialize_with = "deserialize_abliteration_scale")]
    abliteration_scale: Option<f64>,
}

fn deserialize_abliteration_scale<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<f64>, D::Error> {
    // Deserialize the number directly so an explicit null cannot mean omission.
    // Validate before any narrower representation can round an excess to 2.0.
    let scale = f64::deserialize(deserializer)?;
    if !scale.is_finite() || !(0.0..=2.0).contains(&scale) {
        return Err(de::Error::custom("unsupported abliteration scale"));
    }
    Ok(Some(scale))
}

const fn default_cap() -> u32 {
    MAX_COMPLETION_TOKENS
}
const fn default_stream() -> bool {
    true
}

/// Text only; strings resembling URLs remain plain text and are never fetched.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Message<'a> {
    role: Role,
    // JSON text without escapes can be borrowed until canonical serialization;
    // escaped text still uses Serde's owned decoding without changing its bytes.
    #[serde(borrow, deserialize_with = "deserialize_content")]
    content: Cow<'a, str>,
    #[serde(
        default,
        borrow,
        deserialize_with = "deserialize_reasoning_content",
        skip_serializing_if = "Option::is_none"
    )]
    reasoning_content: Option<Cow<'a, str>>,
}

fn deserialize_model<'de, D: Deserializer<'de>>(deserializer: D) -> Result<(), D::Error> {
    struct ModelVisitor;
    impl de::Visitor<'_> for ModelVisitor {
        type Value = ();

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("the fixed public model alias")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<(), E> {
            if value == MODEL_ALIAS {
                Ok(())
            } else {
                Err(E::custom("unsupported model alias"))
            }
        }
    }
    deserializer.deserialize_str(ModelVisitor)
}

fn deserialize_content<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Cow<'de, str>, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TextPart<'a> {
        #[serde(borrow, rename = "type")]
        kind: Cow<'a, str>,
        #[serde(borrow)]
        text: Cow<'a, str>,
    }

    struct ContentVisitor;
    impl<'de> de::Visitor<'de> for ContentVisitor {
        type Value = Cow<'de, str>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a string or bounded nonempty array of text objects")
        }

        fn visit_borrowed_str<E: de::Error>(self, value: &'de str) -> Result<Self::Value, E> {
            Ok(Cow::Borrowed(value))
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            Ok(Cow::Owned(value.to_owned()))
        }

        fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
            Ok(Cow::Owned(value))
        }

        fn visit_seq<A: de::SeqAccess<'de>>(
            self,
            mut sequence: A,
        ) -> Result<Self::Value, A::Error> {
            let mut content = String::new();
            let mut count = 0;
            while let Some(Object(part)) = sequence.next_element::<Object<TextPart<'de>>>()? {
                if count == MAX_CONTENT_PARTS || part.kind != "text" {
                    return Err(de::Error::custom("unsupported content part"));
                }
                content.push_str(&part.text);
                count += 1;
            }
            if count == 0 {
                return Err(de::Error::custom("empty content parts"));
            }
            Ok(Cow::Owned(content))
        }
    }

    deserializer.deserialize_any(ContentVisitor)
}

fn deserialize_reasoning_content<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Cow<'de, str>>, D::Error> {
    // An absent field defaults to None; a present field must be text, even when
    // empty. Deserializing Option directly would also accept an unsupported null.
    // The derived wrapper enables Serde's borrowing support for Cow while
    // retaining owned decoding for escaped JSON strings.
    #[derive(Deserialize)]
    struct Text<'a>(#[serde(borrow)] Cow<'a, str>);

    Text::deserialize(deserializer).map(|Text(text)| Some(text))
}

#[derive(Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Role {
    System,
    User,
    Assistant,
}

impl<'de> Deserialize<'de> for Role {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // A derived unit enum also accepts externally tagged objects in JSON.
        struct RoleVisitor;
        impl de::Visitor<'_> for RoleVisitor {
            type Value = Role;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a role string")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Role, E> {
                match value {
                    "system" => Ok(Role::System),
                    "user" => Ok(Role::User),
                    "assistant" => Ok(Role::Assistant),
                    _ => Err(E::custom("unsupported role")),
                }
            }
        }
        deserializer.deserialize_str(RoleVisitor)
    }
}

// Serde's derived structs accept positional arrays. Demand an object, then
// delegate its streaming map directly to the derive without losing duplicate keys.
struct Object<T>(T);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> de::Visitor<'de> for ObjectVisitor<T> {
            type Value = Object<T>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object")
            }
            fn visit_map<A: de::MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                T::deserialize(de::value::MapAccessDeserializer::new(map)).map(Object)
            }
        }
        deserializer.deserialize_map(ObjectVisitor(PhantomData))
    }
}

fn deserialize_messages<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Message<'de>>, D::Error> {
    struct MessagesVisitor;

    impl<'de> de::Visitor<'de> for MessagesVisitor {
        type Value = Vec<Message<'de>>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a bounded array of message objects")
        }

        fn visit_seq<A: de::SeqAccess<'de>>(
            self,
            mut sequence: A,
        ) -> Result<Self::Value, A::Error> {
            let mut messages = Vec::new();
            while let Some(Object(message)) = sequence.next_element::<Object<Message<'de>>>()? {
                // Stop on the first excess message instead of allocating and
                // parsing every remaining message in an oversized history.
                if messages.len() == MAX_MESSAGES {
                    return Err(de::Error::custom("too many messages"));
                }
                messages.push(message);
            }
            Ok(messages)
        }
    }

    deserializer.deserialize_seq(MessagesVisitor)
}

fn valid_history(messages: &[Message<'_>]) -> bool {
    if messages.is_empty()
        || messages.len() > MAX_MESSAGES
        || messages
            .iter()
            .any(|message| message.reasoning_content.is_some() && message.role != Role::Assistant)
    {
        return false;
    }
    let turns = if messages[0].role == Role::System {
        &messages[1..]
    } else {
        messages
    };
    !turns.is_empty()
        && turns.len() % 2 == 1
        && turns.iter().enumerate().all(|(i, m)| {
            m.role
                == if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                }
        })
}

/// Exposes canonical bytes only to a trusted counter before context admission.
pub struct PendingChat {
    body: Vec<u8>,
    max_completion_tokens: u32,
    stream: bool,
}

impl PendingChat {
    /// The trusted counter must send these exact canonical bytes to the pinned
    /// runtime's internal `/v1/chat/completions/input_tokens` endpoint. Generation
    /// must use the same immutable model/runtime/template and explicit thinking
    /// settings. [`backend::count_and_admit`] implements the supplied-stream exchange;
    /// bytes/characters are never tokens. The supervisor must prevent identity drift.
    ///
    /// # Errors
    /// Propagates the trusted counter's rejection, or returns
    /// [`Rejection::TokenCountUnavailable`] for zero tokens and
    /// [`Rejection::ContextLimit`] when the checked context budget is exceeded.
    pub fn admit(
        self,
        count_rendered_prompt: impl FnOnce(&[u8]) -> Result<usize, Rejection>,
    ) -> Result<AdmittedChat, Rejection> {
        let prompt = count_rendered_prompt(&self.body)?;
        if prompt == 0 {
            return Err(Rejection::TokenCountUnavailable);
        }
        let completion =
            usize::try_from(self.max_completion_tokens).map_err(|_| Rejection::ContextLimit)?;
        let total = prompt
            .checked_add(completion)
            .and_then(|n| n.checked_add(TEMPLATE_MARGIN_TOKENS));
        if total.is_none_or(|n| n > CONTEXT_TOKENS) {
            return Err(Rejection::ContextLimit);
        }
        Ok(AdmittedChat {
            body: self.body,
            stream: self.stream,
        })
    }
}

/// Canonical body only: caller headers, credentials and JSON are never forwarded.
/// The backend adapter supplies its own fixed route/Host/key/Content-Length.
pub struct AdmittedChat {
    body: Vec<u8>,
    stream: bool,
}

impl AdmittedChat {
    /// Borrow the canonical request body admitted by the trusted token counter.
    #[must_use]
    pub fn backend_body(&self) -> &[u8] {
        &self.body
    }

    /// Retained from the canonical request, never inferred from backend headers.
    #[must_use]
    pub const fn streams_response(&self) -> bool {
        self.stream
    }
}

#[derive(Serialize)]
struct BackendChat<'a, 'b> {
    messages: &'a [Message<'b>],
    max_completion_tokens: u32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    lora: Option<[PinnedLora; 1]>,
    n: u8,
    cache_prompt: bool,
    n_cache_reuse: u8,
    ignore_eos: bool,
    temperature: f32,
    top_p: f32,
    top_k: u8,
    min_p: f32,
    presence_penalty: f32,
    frequency_penalty: f32,
    repeat_penalty: f32,
    chat_template_kwargs: ChatTemplateKwargs,
}

#[derive(Serialize)]
struct PinnedLora {
    id: u8,
    scale: f64,
}

#[derive(Serialize)]
struct ChatTemplateKwargs {
    enable_thinking: bool,
    reasoning_effort: &'static str,
    preserve_thinking: bool,
}

#[cfg(test)]
mod abliteration_tests;

#[cfg(test)]
mod tests {
    use super::{ClientChat, Cow, Message, Object, Role, valid_history};

    #[test]
    fn every_short_role_history_matches_the_accepted_language() {
        // The complete accepted corpus through six messages, including the
        // optional system preamble. Enumerating other words catches misplaced
        // system messages, repeated speakers and assistant-prefill acceptance.
        let accepted = ["u", "su", "uau", "suau", "uauau", "suauau"];
        for length in 0..=6 {
            for mut word in 0..3_usize.pow(length) {
                let mut spelling = String::new();
                let mut messages = Vec::new();
                for _ in 0..length {
                    let (letter, role) = match word % 3 {
                        0 => ('s', Role::System),
                        1 => ('u', Role::User),
                        _ => ('a', Role::Assistant),
                    };
                    word /= 3;
                    spelling.push(letter);
                    messages.push(Message {
                        role,
                        content: Cow::Borrowed(""),
                        reasoning_content: None,
                    });
                }
                assert_eq!(
                    valid_history(&messages),
                    accepted.contains(&spelling.as_str()),
                    "history {spelling:?}"
                );
            }
        }
    }

    #[test]
    fn message_content_borrows_plain_text_and_decodes_escapes() -> Result<(), serde_json::Error> {
        let Object(chat): Object<ClientChat<'_>> = serde_json::from_slice(
            br#"{"messages":[{"role":"user","content":"plain text"},{"role":"assistant","content":"line\n\u96ea"}]}"#,
        )?;
        assert!(matches!(
            chat.messages[0].content,
            Cow::Borrowed("plain text")
        ));
        assert!(matches!(chat.messages[1].content, Cow::Owned(_)));
        assert_eq!(chat.messages[1].content, "line\n雪");
        Ok(())
    }

    #[test]
    fn message_reasoning_borrows_plain_text_and_decodes_escapes() -> Result<(), serde_json::Error> {
        let Object(chat): Object<ClientChat<'_>> = serde_json::from_slice(
            br#"{"messages":[{"role":"assistant","content":"","reasoning_content":"plain text"},{"role":"assistant","content":"","reasoning_content":"line\n\u96ea"}]}"#,
        )?;
        assert!(matches!(
            chat.messages[0].reasoning_content,
            Some(Cow::Borrowed("plain text"))
        ));
        assert!(matches!(
            chat.messages[1].reasoning_content,
            Some(Cow::Owned(_))
        ));
        assert_eq!(
            chat.messages[1].reasoning_content.as_deref(),
            Some("line\n雪")
        );
        Ok(())
    }
}
