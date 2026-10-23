//! Synthetic requests and adversarial schema cases shared by every candidate.

use serde_json::json;

pub(super) struct Case {
    pub(super) name: &'static str,
    pub(super) body: Vec<u8>,
}

pub(super) fn workloads() -> Result<Vec<Case>, String> {
    let definitions = [
        ("small-plain", 1, 128, false),
        ("small-escaped", 1, 128, true),
        ("history-8", 8, 1024, true),
        ("history-32", 32, 2048, true),
        ("history-128", 128, 2048, true),
        ("near-body-limit", 1, 900_000, false),
    ];
    definitions
        .into_iter()
        .map(|(name, count, width, escaped)| {
            let fragment = if escaped {
                "A雪🎨\n\"\\"
            } else {
                "creative writing "
            };
            let content: String = fragment.chars().cycle().take(width).collect();
            let messages: Vec<_> = (0..count)
                .map(|index| {
                    let role = if count % 2 == 0 && index == 0 {
                        "system"
                    } else if (index + count % 2) % 2 == 1 {
                        "user"
                    } else {
                        "assistant"
                    };
                    json!({"role":role,"content":content})
                })
                .collect();
            let body = serde_json::to_vec(&json!({
                "messages":messages,"max_completion_tokens":512,"stream":true
            }))
            .map_err(|error| error.to_string())?;
            Ok(Case { name, body })
        })
        .collect()
}

pub(super) fn adversarial() -> Vec<Vec<u8>> {
    let mut bodies: Vec<Vec<u8>> = [
        r#"{"messages":[{"role":"user","content":"hi"}]}"#,
        r#" { "stream":true,"max_completion_tokens":4096,"messages":[{"content":"\u96ea\uD83C\uDFA8\u000a\u0022\u005c","role":"u\u0073er"}] } "#,
        r#"{"messages":[{"role":"user","content":"hi"}],"stream":false,"max_completion_tokens":1}"#,
        r#"{"messages":[{"role":"user","content":"hi"}],"stream":true,"str\u0065am":false}"#,
        r#"{"messages":[{"role":"user","r\u006fle":"user","content":"hi"}]}"#,
        r#"{"messages":[{"role":"user","content":"hi","cont\u0065nt":"other"}]}"#,
        r#"{"messages":[{"role":"user","content":"hi"}],"m\u0065ssages":[]}"#,
        r#"{"messages":[{"role":"user","content":"hi"}],"max_completion_tokens":1,"max_completion_\u0074okens":2}"#,
        r#"{"messages":[{"role":"user","content":"hi"}],"tools":[]}"#,
        r#"{"messages":[{"role":"user","content":"hi"}],"str\u0065am":null}"#,
        r#"{"messages":[{"role":"user","content":"\uD800"}]}"#,
        r#"{"messages":[{"role":"user","content":"\uDC00"}]}"#,
        r#"{"messages":[{"role":"user","content":"\x20"}]}"#,
        r#"{"messages":[{"role":{"user":null},"content":"hi"}]}"#,
        r#"{"messages":[["user","hi"]]}"#,
        r#"[[{"role":"user","content":"hi"}],4096,true]"#,
        r#"{"messages":[{"role":"user","content":null}]}"#,
        r#"{"messages":[{"role":"user"}]}"#,
        r#"{"messages":[{"content":"hi"}]}"#,
        r#"{"messages":[]}"#,
        r#"{"messages":null}"#,
        "{}",
        "null",
        r#"{"messages":[{"role":"user","content":"hi"}]} true"#,
        r#"{"messages":[{"role":"user","content":"hi"}],}"#,
        r#"{"messages":[{"role":"user","content":"hi"}]} // comment"#,
    ]
    .into_iter()
    .map(|body| body.as_bytes().to_vec())
    .collect();
    for cap in [
        "0",
        "4097",
        "-1",
        "1.0",
        "1e0",
        "null",
        "true",
        "\"1\"",
        "4294967296",
    ] {
        bodies.push(
            format!(
                r#"{{"messages":[{{"role":"user","content":"hi"}}],"max_completion_tokens":{cap}}}"#
            )
            .into_bytes(),
        );
    }
    for roles in [
        vec!["assistant"],
        vec!["system"],
        vec!["user", "user", "user"],
        vec!["user", "assistant"],
        vec!["user", "system", "user"],
        vec!["user"; 129],
    ] {
        let messages: Vec<_> = roles
            .into_iter()
            .map(|role| json!({"role":role,"content":"x"}))
            .collect();
        bodies.push(json!({"messages":messages}).to_string().into_bytes());
    }
    for count in [127, 128, 129] {
        let messages: Vec<_> = (0..count)
            .map(|index| {
                let role = if count % 2 == 0 && index == 0 {
                    "system"
                } else if (index + count % 2) % 2 == 1 {
                    "user"
                } else {
                    "assistant"
                };
                json!({"role":role,"content":"x"})
            })
            .collect();
        bodies.push(json!({"messages":messages}).to_string().into_bytes());
    }
    bodies.push(vec![b' '; bonsai_broker::MAX_BODY_BYTES + 1]);
    bodies.push(Vec::new());
    bodies.push(b"{\"messages\":[{\"role\":\"user\",\"content\":\"\xff\"}]}".to_vec());
    bodies.push(b"{\"messages\":[{\"role\":\"user\",\"content\":\"\x01\"}]}".to_vec());
    bodies
}
