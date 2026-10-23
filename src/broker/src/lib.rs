//! M1 request-policy core. No listener, model process, filesystem access or logging.
//!
//! A future transport must bound headers/body *while reading*, preserve duplicate
//! headers, reject invalid HTTP framing, and call `check_head` before reading the
//! body. It must generate a private random credential, use a separate backend key,
//! prevent backend bypass, and enforce concurrency, deadlines and cancellation.
//! This library alone is not a qualified HTTP or process-isolation boundary.
#![forbid(unsafe_code)]

use http::{HeaderMap, Method, Version, request::Parts};
use serde::{Deserialize, Deserializer, Serialize, de};
use std::{fmt, marker::PhantomData};
use subtle::ConstantTimeEq;

pub const MAX_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_HEADER_BYTES: usize = 16 * 1024;
pub const MAX_HEADERS: usize = 64;
pub const MAX_MESSAGES: usize = 128;
pub const MAX_COMPLETION_TOKENS: u32 = 4096;
pub const CONTEXT_TOKENS: usize = 8192;
pub const TEMPLATE_MARGIN_TOKENS: usize = 64;

/// Deliberately contains no input values, parser diagnostics, keys or prompts.
#[derive(Debug, PartialEq, Eq)]
pub enum Rejection {
    Configuration,
    Headers,
    Unauthorized,
    Host,
    Origin,
    Route,
    Framing,
    ContentType,
    BodySize,
    ChatSchema,
    CompletionLimit,
    TokenCountUnavailable,
    ContextLimit,
}

/// No Debug/Serialize implementation: credentials must not enter diagnostics.
pub struct RequestPolicy {
    authority: String,
    origin: String,
    authorization: [u8; 71], // "Bearer " + 64 lowercase hex characters
}

impl RequestPolicy {
    /// The launcher must supply 32 cryptographically random bytes encoded as
    /// lowercase hex. Format validation here cannot establish randomness or ACLs.
    pub fn new(port: u16, key: &str) -> Result<Self, Rejection> {
        if port < 1024
            || key.len() != 64
            || !key
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
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
        })
    }

    /// Check the parsed head before the transport accepts a bounded body.
    /// Only HTTP/1.1 origin-form POST /v1/chat/completions is supported.
    /// OPTIONS, readiness, UI, tokenization and administrative routes are closed.
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
            || head.uri.path_and_query().map(|s| s.as_str()) != Some("/v1/chat/completions")
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
        if length.is_empty() || !length.iter().all(u8::is_ascii_digit) {
            return Err(Rejection::Framing);
        }
        let length = length
            .iter()
            .try_fold(0usize, |n, b| {
                n.checked_mul(10)?.checked_add((b - b'0') as usize)
            })
            .ok_or(Rejection::BodySize)?;
        if length == 0 || length > MAX_BODY_BYTES {
            return Err(Rejection::BodySize);
        }
        Ok(AuthorizedHead { length })
    }
}

fn singleton<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a [u8]>, Rejection> {
    let mut values = headers.get_all(name).iter();
    let first = values.next().map(|v| v.as_bytes());
    if values.next().is_some() {
        return Err(Rejection::Headers);
    }
    Ok(first)
}

/// Constructed only by successful head validation; does not retain the key.
pub struct AuthorizedHead {
    length: usize,
}

impl AuthorizedHead {
    pub fn prepare(self, body: &[u8]) -> Result<PendingChat, Rejection> {
        if body.len() != self.length || body.len() > MAX_BODY_BYTES {
            return Err(Rejection::BodySize);
        }
        // Deserialize directly: maps/Value would silently erase duplicate keys.
        // Parser error strings can include private values; never propagate them.
        let Object(chat): Object<ClientChat> =
            serde_json::from_slice(body).map_err(|_| Rejection::ChatSchema)?;
        if !valid_history(&chat.messages) {
            return Err(Rejection::ChatSchema);
        }
        if !(1..=MAX_COMPLETION_TOKENS).contains(&chat.max_completion_tokens) {
            return Err(Rejection::CompletionLimit);
        }
        let max_completion_tokens = chat.max_completion_tokens;
        let body = serde_json::to_vec(&BackendChat {
            messages: &chat.messages,
            max_completion_tokens,
            stream: chat.stream,
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
        })
        .expect("canonical chat is serializable");
        Ok(PendingChat {
            body,
            max_completion_tokens,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientChat {
    #[serde(deserialize_with = "deserialize_messages")]
    messages: Vec<Message>,
    #[serde(default = "default_cap")]
    max_completion_tokens: u32,
    #[serde(default = "default_stream")]
    stream: bool,
}

fn default_cap() -> u32 {
    MAX_COMPLETION_TOKENS
}
fn default_stream() -> bool {
    true
}

/// Text only; strings resembling URLs remain plain text and are never fetched.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Message {
    role: Role,
    content: String,
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
        match String::deserialize(deserializer)?.as_str() {
            "system" => Ok(Self::System),
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            _ => Err(de::Error::custom("unsupported role")),
        }
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
) -> Result<Vec<Message>, D::Error> {
    Vec::<Object<Message>>::deserialize(deserializer).map(|messages| {
        messages
            .into_iter()
            .map(|Object(message)| message)
            .collect()
    })
}

fn valid_history(messages: &[Message]) -> bool {
    if messages.is_empty() || messages.len() > MAX_MESSAGES {
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
}

impl PendingChat {
    /// The trusted counter must send these exact canonical bytes to the pinned
    /// runtime's internal /v1/chat/completions/input_tokens endpoint. Generation
    /// must use the same immutable model/runtime/template and default thinking
    /// mode. No production transport exists yet; bytes/characters are not tokens.
    /// The counter must fail on missing/malformed counts or backend identity drift.
    pub fn admit(
        self,
        count_rendered_prompt: impl FnOnce(&[u8]) -> Result<usize, Rejection>,
    ) -> Result<AdmittedChat, Rejection> {
        let prompt = count_rendered_prompt(&self.body)?;
        if prompt == 0 {
            return Err(Rejection::TokenCountUnavailable);
        }
        let total = prompt
            .checked_add(self.max_completion_tokens as usize)
            .and_then(|n| n.checked_add(TEMPLATE_MARGIN_TOKENS));
        if total.is_none_or(|n| n > CONTEXT_TOKENS) {
            return Err(Rejection::ContextLimit);
        }
        Ok(AdmittedChat { body: self.body })
    }
}

/// Canonical body only: caller headers, credentials and JSON are never forwarded.
/// The future transport supplies its own backend URL/Host/key/Content-Length.
pub struct AdmittedChat {
    body: Vec<u8>,
}

impl AdmittedChat {
    pub fn backend_body(&self) -> &[u8] {
        &self.body
    }
}

#[derive(Serialize)]
struct BackendChat<'a> {
    messages: &'a [Message],
    max_completion_tokens: u32,
    stream: bool,
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
}
