//! The only caller-controlled adapter value is a bounded scale for pinned slot 0.

use super::{PendingChat, Rejection, RequestPolicy};
use http::Request;
use serde_json::{Value, json};

const CHAT: &[u8] = br#"{"messages":[{"role":"user","content":"Write a scene."}]}"#;

fn prepare(body: &[u8], enabled: bool) -> Result<PendingChat, Rejection> {
    let key = "a".repeat(64);
    let mut policy = RequestPolicy::new(8080, &key)?;
    // Production can set this only inside the owned launcher's verified service.
    policy.pinned_abliteration = enabled;
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("host", "127.0.0.1:8080")
        .header("authorization", format!("Bearer {key}"))
        .header("content-type", "application/json")
        .header("content-length", body.len())
        .body(())
        .map_err(|_| Rejection::Configuration)?;
    policy.check_head(&request.into_parts().0)?.prepare(body)
}

fn body_with(scale: &str) -> String {
    format!(
        r#"{{"messages":[{{"role":"user","content":"Write a scene."}}],"abliteration_scale":{scale}}}"#
    )
}

fn canonical(body: &[u8], enabled: bool) -> Result<Value, Rejection> {
    let admitted = prepare(body, enabled)?.admit(|_| Ok(1))?;
    serde_json::from_slice(admitted.backend_body()).map_err(|_| Rejection::ChatSchema)
}

#[test]
fn pinned_abliteration_defaults_to_1_3_without_changing_other_policy() -> Result<(), Rejection> {
    let baseline = canonical(CHAT, false)?;
    assert!(baseline.get("lora").is_none());
    let mut enabled = canonical(CHAT, true)?;
    let lora = enabled.as_object_mut().and_then(|body| body.remove("lora"));
    assert_eq!(lora, Some(json!([{ "id": 0, "scale": 1.3 }])));
    assert_eq!(enabled, baseline);
    Ok(())
}

#[test]
fn pinned_abliteration_scale_is_identical_for_counting_and_generation() -> Result<(), Rejection> {
    for (scale, expected) in [("0", json!(0.0)), ("1.3", json!(1.3)), ("2", json!(2.0))] {
        let pending = prepare(body_with(scale).as_bytes(), true)?;
        let counted = pending.body.clone();
        let decoded: Value = serde_json::from_slice(&counted).map_err(|_| Rejection::ChatSchema)?;
        assert_eq!(decoded["lora"], json!([{ "id": 0, "scale": expected }]));
        assert!(decoded.get("abliteration_scale").is_none());
        let admitted = pending.admit(|body| {
            assert_eq!(body, counted);
            Ok(1)
        })?;
        assert_eq!(admitted.backend_body(), counted);
    }
    Ok(())
}

#[test]
fn pinned_abliteration_rejects_invalid_scales_and_arbitrary_adapters() {
    for scale in [
        "null",
        "true",
        "false",
        r#""1.3""#,
        "[]",
        "{}",
        "-0.1",
        "2.00000001",
        "3",
        "1e309",
        "NaN",
        "Infinity",
    ] {
        assert!(matches!(
            prepare(body_with(scale).as_bytes(), true),
            Err(Rejection::ChatSchema)
        ));
    }
    for extra in [
        r#""abliteration_scale":1.3,"abliteration_scale":1.3"#,
        r#""lora":[{"id":0,"scale":1.3}]"#,
        r#""abliteration_scale":1.3,"lora":[]"#,
    ] {
        let body = format!(r#"{{"messages":[{{"role":"user","content":"text"}}],{extra}}}"#);
        assert!(matches!(
            prepare(body.as_bytes(), true),
            Err(Rejection::ChatSchema)
        ));
    }
}

#[test]
fn pinned_abliteration_scale_is_unavailable_without_verified_launch() {
    for scale in ["0", "1.3", "2"] {
        assert!(matches!(
            prepare(body_with(scale).as_bytes(), false),
            Err(Rejection::ChatSchema)
        ));
    }
}
