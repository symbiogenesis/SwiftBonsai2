//! Small boundary and metamorphic fixtures for strict request policy.
#![expect(
    clippy::unwrap_used,
    reason = "successful fixture setup and operations must fail the test on error"
)]

use bonsai_broker::{
    CONTEXT_TOKENS, MAX_COMPLETION_TOKENS, MAX_HEADER_BYTES, MAX_HEADERS, Rejection, RequestPolicy,
    TEMPLATE_MARGIN_TOKENS,
};
use http::{HeaderValue, Request, request::Parts};

const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const CHAT: &[u8] = br#"{"messages":[{"role":"user","content":"hi"}]}"#;

fn head(length: usize) -> Parts {
    Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("host", "127.0.0.1:8080")
        .header("authorization", format!("Bearer {KEY}"))
        .header("content-type", "application/json")
        .header("content-length", length.to_string())
        .body(())
        .unwrap()
        .into_parts()
        .0
}

#[test]
fn header_count_accepts_the_exact_limit_and_rejects_one_more() {
    let policy = RequestPolicy::new(8080, KEY).unwrap();
    let mut parts = head(CHAT.len());
    while parts.headers.len() < MAX_HEADERS {
        // Repeated extension headers must count as separate values, while
        // remaining legal because they do not change request framing.
        parts
            .headers
            .append("x-extension", HeaderValue::from_static("x"));
    }
    assert!(policy.check_head(&parts).is_ok());
    parts
        .headers
        .append("x-extension", HeaderValue::from_static("x"));
    assert!(matches!(policy.check_head(&parts), Err(Rejection::Headers)));
}

#[test]
fn header_bytes_include_delimiters_at_the_exact_limit() {
    let policy = RequestPolicy::new(8080, KEY).unwrap();
    let mut parts = head(CHAT.len());
    let used: usize = parts
        .headers
        .iter()
        .map(|(name, value)| name.as_str().len() + value.as_bytes().len() + 4)
        .sum();
    let padding = MAX_HEADER_BYTES - used - "x-padding: \r\n".len();
    parts.headers.insert(
        "x-padding",
        HeaderValue::from_str(&"x".repeat(padding)).unwrap(),
    );
    assert!(policy.check_head(&parts).is_ok());
    parts.headers.insert(
        "x-padding",
        HeaderValue::from_str(&"x".repeat(padding + 1)).unwrap(),
    );
    assert!(matches!(policy.check_head(&parts), Err(Rejection::Headers)));
}

#[test]
fn every_credential_position_is_checked_including_the_last_byte() {
    let policy = RequestPolicy::new(8080, KEY).unwrap();
    let mut parts = head(CHAT.len());
    let original = parts.headers["authorization"].as_bytes().to_vec();
    assert!(policy.check_head(&parts).is_ok());
    for position in 0..original.len() {
        let mut changed = original.clone();
        changed[position] = if changed[position] == b'a' {
            b'b'
        } else {
            b'a'
        };
        parts
            .headers
            .insert("authorization", HeaderValue::from_bytes(&changed).unwrap());
        assert!(
            matches!(policy.check_head(&parts), Err(Rejection::Unauthorized)),
            "credential byte {position}"
        );
    }
    parts
        .headers
        .insert("authorization", HeaderValue::from_bytes(&original).unwrap());
    assert!(policy.check_head(&parts).is_ok());
}

#[test]
fn equivalent_json_encodings_produce_identical_count_and_generation_bytes() {
    let policy = RequestPolicy::new(8080, KEY).unwrap();
    let bodies = [
        r#"{"messages":[{"role":"user","content":"雪\n\"\\"}],"stream":true,"max_completion_tokens":4096}"#,
        r#" { "max_completion_tokens":4096, "messages":[{"content":"\u96ea\u000a\u0022\u005c","role":"u\u0073er"}], "stream":true } "#,
    ];
    let canonical: Vec<_> = bodies
        .iter()
        .map(|body| {
            let mut counted = Vec::new();
            let admitted = policy
                .check_head(&head(body.len()))
                .unwrap()
                .prepare(body.as_bytes())
                .unwrap()
                .admit(|bytes| {
                    counted.extend_from_slice(bytes);
                    Ok(1)
                })
                .unwrap();
            assert_eq!(counted, admitted.backend_body());
            assert!(admitted.streams_response());
            counted
        })
        .collect();
    assert_eq!(canonical[0], canonical[1]);
}

#[test]
fn escaped_duplicate_field_names_cannot_bypass_the_schema() {
    let policy = RequestPolicy::new(8080, KEY).unwrap();
    for body in [
        r#"{"messages":[{"role":"user","content":"hi"}],"stream":true,"str\u0065am":false}"#,
        r#"{"messages":[{"role":"user","content":"hi"}],"max_completion_tokens":1,"max_completion_\u0074okens":4096}"#,
        r#"{"messages":[{"role":"user","r\u006fle":"user","content":"hi"}]}"#,
        r#"{"messages":[{"role":"user","content":"hi","cont\u0065nt":"other"}]}"#,
        r#"{"messages":[{"role":"user","content":"hi"}],"m\u0065ssages":[]}"#,
    ] {
        assert!(matches!(
            policy
                .check_head(&head(body.len()))
                .unwrap()
                .prepare(body.as_bytes()),
            Err(Rejection::ChatSchema)
        ));
    }
}

#[test]
fn context_boundary_tracks_the_requested_completion_budget() {
    let policy = RequestPolicy::new(8080, KEY).unwrap();
    for cap in [
        1,
        2,
        127,
        128,
        MAX_COMPLETION_TOKENS - 1,
        MAX_COMPLETION_TOKENS,
    ] {
        let body = format!(
            r#"{{"messages":[{{"role":"user","content":"hi"}}],"max_completion_tokens":{cap}}}"#
        );
        let available = CONTEXT_TOKENS - TEMPLATE_MARGIN_TOKENS - usize::try_from(cap).unwrap();
        for count in [available - 1, available, available + 1, usize::MAX] {
            let result = policy
                .check_head(&head(body.len()))
                .unwrap()
                .prepare(body.as_bytes())
                .unwrap()
                .admit(|_| Ok(count));
            if count <= available {
                assert!(result.is_ok(), "completion {cap}, prompt {count}");
            } else {
                assert!(
                    matches!(result, Err(Rejection::ContextLimit)),
                    "completion {cap}, prompt {count}"
                );
            }
        }
    }
}
