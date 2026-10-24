#![forbid(unsafe_code)]
//! Regression coverage for strict request parsing and context admission.
#![expect(
    clippy::unwrap_used,
    reason = "test setup and successful-operation assertions must panic on failure"
)]

use bonsai_broker::{
    CONTEXT_TOKENS, MAX_BODY_BYTES, MAX_COMPLETION_TOKENS, MAX_HEADER_BYTES, MAX_HEADERS,
    MAX_MESSAGES, PendingChat, Rejection, RequestPolicy, TEMPLATE_MARGIN_TOKENS,
};
use http::{HeaderValue, Method, Request, Version, request::Parts};

// Synthetic fixture only: production randomness/private storage is a later gate.
const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const CHAT: &[u8] = br#"{"messages":[{"role":"user","content":"Draft a scene with rain."}]}"#;

fn policy() -> RequestPolicy {
    RequestPolicy::new(8080, KEY).unwrap()
}

fn head(size: usize) -> Parts {
    Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .version(Version::HTTP_11)
        .header("host", "127.0.0.1:8080")
        .header("origin", "http://127.0.0.1:8080")
        .header("authorization", format!("Bearer {KEY}"))
        .header("content-type", "application/json")
        .header("content-length", size.to_string())
        .body(())
        .unwrap()
        .into_parts()
        .0
}

fn prepare(body: &[u8]) -> Result<PendingChat, Rejection> {
    policy().check_head(&head(body.len()))?.prepare(body)
}

fn rejected_head(parts: &Parts, expected: Rejection) {
    assert!(matches!(policy().check_head(parts), Err(e) if e == expected));
}

fn rejected_body(body: &[u8], expected: Rejection) {
    assert!(matches!(prepare(body), Err(e) if e == expected));
}

#[test]
fn configuration_requires_private_key_format_and_nonprivileged_port() {
    for key in ["", "short", &"G".repeat(64), &"a".repeat(65)] {
        assert!(matches!(
            RequestPolicy::new(8080, key),
            Err(Rejection::Configuration)
        ));
    }
    assert!(matches!(
        RequestPolicy::new(80, KEY),
        Err(Rejection::Configuration)
    ));
}

#[test]
fn rejects_ambiguous_headers() {
    for name in [
        "host",
        "origin",
        "authorization",
        "content-length",
        "content-type",
        "connection",
    ] {
        let mut h = head(CHAT.len());
        if !h.headers.contains_key(name) {
            h.headers.insert(name, HeaderValue::from_static("close"));
        }
        let value = h.headers[name].clone();
        h.headers.append(name, value);
        rejected_head(&h, Rejection::Headers);
    }
    for name in [
        "forwarded",
        "x-forwarded-host",
        "x-forwarded-for",
        "x-forwarded-proto",
    ] {
        let mut h = head(CHAT.len());
        h.headers
            .insert(name, HeaderValue::from_static("attacker.invalid"));
        rejected_head(&h, Rejection::Headers);
    }
}

#[test]
fn native_clients_still_require_authentication() {
    let mut h = head(CHAT.len());
    h.headers.remove("origin");
    assert!(policy().check_head(&h).is_ok());
    h.headers.remove("authorization");
    rejected_head(&h, Rejection::Unauthorized);
    for auth in [
        format!("Bearer {}", "f".repeat(64)),
        format!("Basic {KEY}"),
        KEY.to_string(),
    ] {
        h.headers
            .insert("authorization", HeaderValue::from_str(&auth).unwrap());
        rejected_head(&h, Rejection::Unauthorized);
    }
}

#[test]
fn rejects_alternate_hosts_and_browser_origins() {
    for host in [
        "localhost:8080",
        "127.0.0.1",
        "127.0.0.1:8081",
        "[::1]:8080",
        "attacker.invalid:8080",
    ] {
        let mut h = head(CHAT.len());
        h.headers
            .insert("host", HeaderValue::from_str(host).unwrap());
        rejected_head(&h, Rejection::Host);
    }
    for origin in [
        "null",
        "*",
        "https://127.0.0.1:8080",
        "http://localhost:8080",
        "http://127.0.0.1:8081",
        "http://127.0.0.1:8080.attacker.invalid",
    ] {
        let mut h = head(CHAT.len());
        h.headers
            .insert("origin", HeaderValue::from_str(origin).unwrap());
        rejected_head(&h, Rejection::Origin);
    }
}

#[test]
fn only_exact_chat_route_is_public() {
    for uri in [
        "/health",
        "/props",
        "/slots",
        "/tools",
        "/models/load",
        "/v1/chat/completions/input_tokens",
        "/v1/chat/completions/",
        "/v1/chat/completions?x=1",
        "/v1/chat/%63ompletions",
        "http://127.0.0.1:8080/v1/chat/completions",
    ] {
        let mut h = head(CHAT.len());
        h.uri = uri.parse().unwrap();
        rejected_head(&h, Rejection::Route);
    }
    for method in [
        Method::GET,
        Method::OPTIONS,
        Method::CONNECT,
        Method::DELETE,
    ] {
        let mut h = head(CHAT.len());
        h.method = method;
        rejected_head(&h, Rejection::Route);
    }
    let mut h = head(CHAT.len());
    h.version = Version::HTTP_2;
    rejected_head(&h, Rejection::Route);
}

#[test]
fn rejects_unreviewed_framing_and_compression() {
    for (name, value) in [
        ("transfer-encoding", "chunked"),
        ("content-encoding", "gzip"),
        ("trailer", "Host"),
        ("expect", "100-continue"),
        ("upgrade", "websocket"),
        ("connection", "content-length"),
    ] {
        let mut h = head(CHAT.len());
        h.headers.insert(name, HeaderValue::from_static(value));
        rejected_head(&h, Rejection::Framing);
    }
    for length in ["-1", "+1", "1, 1", "1 1", "", "1.0"] {
        let mut h = head(CHAT.len());
        h.headers
            .insert("content-length", HeaderValue::from_str(length).unwrap());
        rejected_head(&h, Rejection::Framing);
    }
    let mut h = head(CHAT.len());
    h.headers.remove("content-length");
    rejected_head(&h, Rejection::Framing);
}

#[test]
fn bounds_both_advertised_and_actual_body_bytes() {
    for length in [
        "0".to_string(),
        (MAX_BODY_BYTES + 1).to_string(),
        "9".repeat(128),
    ] {
        let mut h = head(CHAT.len());
        h.headers
            .insert("content-length", HeaderValue::from_str(&length).unwrap());
        rejected_head(&h, Rejection::BodySize);
    }
    let authorized = policy().check_head(&head(CHAT.len())).unwrap();
    assert!(matches!(
        authorized.prepare(&CHAT[..CHAT.len() - 1]),
        Err(Rejection::BodySize)
    ));
    let authorized = policy().check_head(&head(MAX_BODY_BYTES)).unwrap();
    assert!(matches!(
        authorized.prepare(&vec![b' '; MAX_BODY_BYTES + 1]),
        Err(Rejection::BodySize)
    ));
}

#[test]
fn bounds_header_count_and_bytes() {
    let mut h = head(CHAT.len());
    for i in 0..MAX_HEADERS {
        h.headers.append(
            format!("x-header-{i}").parse::<http::HeaderName>().unwrap(),
            HeaderValue::from_static("x"),
        );
    }
    rejected_head(&h, Rejection::Headers);
    let mut h = head(CHAT.len());
    h.headers.insert(
        "user-agent",
        HeaderValue::from_str(&"x".repeat(MAX_HEADER_BYTES)).unwrap(),
    );
    rejected_head(&h, Rejection::Headers);
}

#[test]
fn json_media_type_is_required() {
    for media in [
        "text/plain",
        "application/json; charset=latin1",
        "multipart/form-data",
    ] {
        let mut h = head(CHAT.len());
        h.headers
            .insert("content-type", HeaderValue::from_str(media).unwrap());
        rejected_head(&h, Rejection::ContentType);
    }
    let mut h = head(CHAT.len());
    h.headers.insert(
        "content-type",
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    assert!(policy().check_head(&h).is_ok());
}

#[test]
fn typed_schema_rejects_aliases_controls_tools_and_media() {
    for field in [
        r#""max_tokens":1"#,
        r#""n_predict":1"#,
        r#""n":2"#,
        r#""model":"other""#,
        r#""tools":[]"#,
        r#""chat_template_kwargs":{"enable_thinking":false}"#,
        r#""chat_template_kwargs":{"enable_thinking":true,"reasoning_effort":"xhigh"}"#,
        r#""chat_template_kwargs":{"preserve_thinking":false}"#,
        r#""reasoning_effort":"none""#,
        r#""reasoning_budget_tokens":0"#,
        r#""thinking_budget_tokens":0"#,
        r#""cache_prompt":true"#,
        r#""temperature":0"#,
        r#""grammar":"root ::= \"x\"""#,
        r#""lora":[]"#,
    ] {
        let body = format!(r#"{{"messages":[{{"role":"user","content":"hi"}}],{field}}}"#);
        rejected_body(body.as_bytes(), Rejection::ChatSchema);
    }
    for message in [
        r#"{"role":"tool","content":"hi"}"#,
        r#"{"role":"user","content":[{"type":"image_url","image_url":{"url":"http://127.0.0.1/"}}]}"#,
        r#"{"role":"user","content":"hi","tool_calls":[]}"#,
    ] {
        rejected_body(
            format!(r#"{{"messages":[{message}]}}"#).as_bytes(),
            Rejection::ChatSchema,
        );
    }
}

#[test]
fn duplicate_keys_and_wrong_types_cannot_erase_limits() {
    for field in [
        r#""stream":true,"stream":false"#,
        r#""max_completion_tokens":1,"max_completion_tokens":4096"#,
        r#""stream":null"#,
        r#""max_completion_tokens":null"#,
        r#""max_completion_tokens":1.0"#,
        r#""max_completion_tokens":"1""#,
        r#""max_completion_tokens":-1"#,
    ] {
        let body = format!(r#"{{"messages":[{{"role":"user","content":"hi"}}],{field}}}"#);
        rejected_body(body.as_bytes(), Rejection::ChatSchema);
    }
    rejected_body(
        br#"{"messages":[{"role":"user","content":"a","content":"b"}]}"#,
        Rejection::ChatSchema,
    );
    rejected_body(
        br#"{"messages":[{"role":"user","content":"a"}],"messages":[]}"#,
        Rejection::ChatSchema,
    );
    let mut trailing = CHAT.to_vec();
    trailing.extend_from_slice(b"{}");
    rejected_body(&trailing, Rejection::ChatSchema);
    rejected_body(b"\xff", Rejection::ChatSchema);
}

#[test]
fn strict_json_shapes_reject_positional_structs_and_tagged_roles() {
    for body in [
        br#"[[["user","hi"]],1,false]"#.as_slice(),
        br#"[[{"role":"user","content":"hi"}],1,false]"#.as_slice(),
        br#"{"messages":[["user","hi"]]}"#.as_slice(),
        br#"{"messages":[{"role":{"user":null},"content":"hi"}]}"#.as_slice(),
        br#"{"messages":[{"role":null,"content":"hi"}]}"#.as_slice(),
        br#"{"messages":[{"role":0,"content":"hi"}]}"#.as_slice(),
        br#"{"messages":[{"role":true,"content":"hi"}]}"#.as_slice(),
        br#"{"messages":[{"role":["user"],"content":"hi"}]}"#.as_slice(),
    ] {
        rejected_body(body, Rejection::ChatSchema);
    }
}

#[test]
fn escaped_role_strings_preserve_the_canonical_history() {
    let body = br#"{"messages":[{"role":"s\u0079stem","content":"hi"},{"role":"u\u0073er","content":"hi"},{"role":"a\u0073sistant","content":"hi"},{"role":"user","content":"hi"}]}"#;
    let admitted = prepare(body).unwrap().admit(|_| Ok(100)).unwrap();
    let canonical: serde_json::Value = serde_json::from_slice(admitted.backend_body()).unwrap();
    assert_eq!(canonical["messages"][0]["role"], "system");
    assert_eq!(canonical["messages"][1]["role"], "user");
    assert_eq!(canonical["messages"][2]["role"], "assistant");
    assert_eq!(canonical["messages"][3]["role"], "user");
}

#[test]
fn completion_limit_is_positive_and_bounded() {
    for cap in [0, MAX_COMPLETION_TOKENS + 1, u32::MAX] {
        let body = format!(
            r#"{{"messages":[{{"role":"user","content":"hi"}}],"max_completion_tokens":{cap}}}"#
        );
        rejected_body(body.as_bytes(), Rejection::CompletionLimit);
    }
}

#[test]
fn rejects_invalid_history_and_assistant_prefill() {
    for messages in [
        "[]",
        r#"[{"role":"system","content":"hi"}]"#,
        r#"[{"role":"assistant","content":"prefill"}]"#,
        r#"[{"role":"user","content":"hi"},{"role":"assistant","content":"prefill"}]"#,
        r#"[{"role":"user","content":"hi"},{"role":"system","content":"later"},{"role":"user","content":"hi"}]"#,
    ] {
        rejected_body(
            format!(r#"{{"messages":{messages}}}"#).as_bytes(),
            Rejection::ChatSchema,
        );
    }
    let messages: Vec<_> = (0..=MAX_MESSAGES)
        .map(|i| {
            let role = if i % 2 == 0 { "user" } else { "assistant" };
            serde_json::json!({"role": role, "content": "x"})
        })
        .collect();
    rejected_body(
        &serde_json::to_vec(&serde_json::json!({"messages": messages})).unwrap(),
        Rejection::ChatSchema,
    );
}

#[test]
fn message_limit_preserves_maximum_valid_histories() {
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
                serde_json::json!({"role": role, "content": "quote: \"; newline:\n; snow: 雪"})
            })
            .collect();
        let body = serde_json::to_vec(&serde_json::json!({"messages": messages})).unwrap();
        if count > MAX_MESSAGES {
            rejected_body(&body, Rejection::ChatSchema);
        } else {
            let admitted = prepare(&body).unwrap().admit(|_| Ok(1)).unwrap();
            let canonical: serde_json::Value =
                serde_json::from_slice(admitted.backend_body()).unwrap();
            assert_eq!(canonical["messages"], serde_json::json!(messages));
        }
    }
}

#[test]
fn exact_count_sees_identical_canonical_body_and_preserved_text() {
    let body = br#"{"messages":[{"role":"system","content":"Preserve my voice."},{"role":"user","content":"First draft"},{"role":"assistant","content":"A scene."},{"role":"user","content":"Revise.\nhttps://example.invalid is plain text."}],"max_completion_tokens":512,"stream":false}"#;
    let mut counted = Vec::new();
    let admitted = prepare(body)
        .unwrap()
        .admit(|canonical| {
            counted = canonical.to_vec();
            Ok(100)
        })
        .unwrap();
    assert_eq!(counted, admitted.backend_body());
    let canonical: serde_json::Value = serde_json::from_slice(admitted.backend_body()).unwrap();
    let original: serde_json::Value = serde_json::from_slice(body).unwrap();
    assert_eq!(canonical["messages"], original["messages"]);
    assert_eq!(canonical["max_completion_tokens"], 512);
    assert_eq!(canonical["stream"], false);
    assert_eq!(canonical["n"], 1);
    assert_eq!(canonical["cache_prompt"], false);
    assert_eq!(canonical["n_cache_reuse"], 0);
    assert_eq!(canonical["ignore_eos"], false);
    assert_eq!(canonical["temperature"], 1.0);
    assert_eq!(canonical["top_p"], 0.95);
    assert_eq!(canonical["top_k"], 20);
    assert_eq!(canonical["min_p"], 0.0);
    assert_eq!(canonical["presence_penalty"], 0.0);
    assert_eq!(canonical["frequency_penalty"], 0.0);
    assert_eq!(canonical["repeat_penalty"], 1.0);
    assert!(canonical.get("authorization").is_none());
    assert_eq!(
        canonical["chat_template_kwargs"],
        serde_json::json!({
            "enable_thinking": true,
            "reasoning_effort": "xhigh",
            "preserve_thinking": true,
        })
    );
}

#[test]
fn assistant_reasoning_is_preserved_in_exact_count_and_generation() {
    for reasoning in ["Check the chronology.\nThe snow falls first: 雪", ""] {
        let messages = serde_json::json!([
            {"role": "user", "content": "Draft a scene."},
            {"role": "assistant", "content": "The snow fell.", "reasoning_content": reasoning},
            {"role": "user", "content": "Continue the scene."},
        ]);
        let body = serde_json::to_vec(&serde_json::json!({"messages": messages})).unwrap();
        let mut counted = Vec::new();
        let admitted = prepare(&body)
            .unwrap()
            .admit(|canonical| {
                counted = canonical.to_vec();
                let counted: serde_json::Value = serde_json::from_slice(canonical).unwrap();
                assert_eq!(counted["messages"][1]["reasoning_content"], reasoning);
                Ok(100)
            })
            .unwrap();
        assert_eq!(counted, admitted.backend_body());
        let generated: serde_json::Value = serde_json::from_slice(admitted.backend_body()).unwrap();
        assert_eq!(generated["messages"], messages);
        assert_eq!(generated["chat_template_kwargs"]["preserve_thinking"], true);
    }
}

#[test]
fn reasoning_history_rejects_null_types_duplicates_and_nonassistant_roles() {
    for reasoning in ["null", "false", "0", "[]", "{}"] {
        let body = format!(
            r#"{{"messages":[{{"role":"user","content":"hi"}},{{"role":"assistant","content":"answer","reasoning_content":{reasoning}}},{{"role":"user","content":"continue"}}]}}"#
        );
        rejected_body(body.as_bytes(), Rejection::ChatSchema);
    }
    for messages in [
        r#"[{"role":"user","content":"hi","reasoning_content":""}]"#,
        r#"[{"role":"system","content":"hi","reasoning_content":""},{"role":"user","content":"hi"}]"#,
        r#"[{"role":"user","content":"hi"},{"role":"assistant","content":"answer","reasoning_content":"a","reasoning_content":"b"},{"role":"user","content":"continue"}]"#,
    ] {
        rejected_body(
            format!(r#"{{"messages":{messages}}}"#).as_bytes(),
            Rejection::ChatSchema,
        );
    }
}

#[test]
fn context_admission_fails_closed_at_boundary_and_overflow() {
    let exact =
        CONTEXT_TOKENS - usize::try_from(MAX_COMPLETION_TOKENS).unwrap() - TEMPLATE_MARGIN_TOKENS;
    assert!(prepare(CHAT).unwrap().admit(|_| Ok(exact)).is_ok());
    for count in [exact + 1, usize::MAX] {
        assert!(matches!(
            prepare(CHAT).unwrap().admit(|_| Ok(count)),
            Err(Rejection::ContextLimit)
        ));
    }
    assert!(matches!(
        prepare(CHAT).unwrap().admit(|_| Ok(0)),
        Err(Rejection::TokenCountUnavailable)
    ));
    assert!(matches!(
        prepare(CHAT)
            .unwrap()
            .admit(|_| Err(Rejection::TokenCountUnavailable)),
        Err(Rejection::TokenCountUnavailable)
    ));
}
