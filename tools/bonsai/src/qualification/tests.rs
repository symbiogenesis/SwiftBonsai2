//! Independently frozen PowerShell byte contracts and scorer receipts.

use serde_json::{Value, json};

use super::{CONTRACT, control_identity, decode, decode_text, digest, run};
use crate::Result;

const GOLDEN: &str = include_str!("../../../../tests/fixtures/lora-qualification-v1.json");

fn golden() -> Result<Value> {
    serde_json::from_str(GOLDEN).map_err(|error| error.to_string())
}

#[test]
fn frozen_plan_control_bytes_and_receipts() -> Result<()> {
    let golden = golden()?;
    let plan = run(json!({"operation": "plan"}))?;
    assert_eq!(plan, golden["plan"]);
    let policy = super::policy()?;
    for vector in golden["controlVectors"]
        .as_array()
        .ok_or("Missing frozen controls.")?
    {
        let controls = decode(vector["controls"].clone())?;
        let (sha256, bytes) = control_identity(&controls, &policy)?;
        assert_eq!(bytes, vector["canonicalJson"]);
        assert_eq!(sha256, vector["sha256"]);
        assert_eq!(
            run(json!({"operation":"controls", "controls": vector["controls"]}))?,
            json!({"Sha256": sha256, "CanonicalJson": bytes})
        );
    }
    for vector in golden["textVectors"]
        .as_array()
        .ok_or("Missing frozen text identities.")?
    {
        assert_eq!(
            digest(vector["text"].as_str().ok_or("Invalid text vector.")?),
            vector["sha256"]
        );
    }
    assert_eq!(
        digest(&CONTRACT.replace("\r\n", "\n")),
        digest(
            &CONTRACT
                .replace("\r\n", "\n")
                .replace('\n', "\r\n")
                .replace("\r\n", "\n")
        )
    );
    for scenario in golden["scenarios"]
        .as_array()
        .ok_or("Missing frozen scenarios.")?
    {
        let result = run(
            json!({"operation":"score", "evidence": scenario["evidence"], "review": scenario["review"]}),
        )?;
        assert_eq!(
            result, scenario["expected"],
            "Frozen scenario: {}",
            scenario["name"]
        );
    }
    Ok(())
}

#[test]
fn strict_shapes_ranges_and_required_nullable_fields() -> Result<()> {
    let golden = golden()?;
    let accepted = &golden["scenarios"][0];
    let base = json!({"operation":"score", "evidence": accepted["evidence"], "review": accepted["review"]});
    for pointer in [
        "/evidence",
        "/evidence/execution",
        "/evidence/controls",
        "/evidence/controls/sampler",
        "/evidence/basis",
        "/evidence/samples/0",
        "/evidence/zeroScaleChecks/0",
        "/evidence/nonzeroAdapterCheck",
        "/review",
        "/review/reviews/0",
        "/review/reviews/0/scores",
    ] {
        for invalid in [
            json!([]),
            json!([[]]),
            Value::Null,
            json!(true),
            json!("text"),
        ] {
            if pointer == "/review" && invalid.is_null() {
                continue;
            }
            let mut request = base.clone();
            *request
                .pointer_mut(pointer)
                .ok_or("Missing mutation pointer.")? = invalid;
            assert!(run(request).is_err(), "Accepted non-object at {pointer}");
        }
    }
    for pointer in [
        "/evidence/basis",
        "/evidence/zeroScaleChecks/0",
        "/evidence/nonzeroAdapterCheck",
    ] {
        let object = base
            .pointer(pointer)
            .and_then(Value::as_object)
            .ok_or("Missing nullable object.")?;
        for key in object.keys() {
            let mut request = base.clone();
            request
                .pointer_mut(pointer)
                .and_then(Value::as_object_mut)
                .ok_or("Missing mutable object.")?
                .remove(key);
            assert!(
                run(request).is_err(),
                "Accepted absent required field {pointer}/{key}"
            );
        }
    }
    for pointer in [
        "/evidence/schemaVersion",
        "/evidence/samples/0/decodeTokens",
        "/evidence/samples/0/repeat",
        "/evidence/samples/0/peakJobMemoryBytes",
        "/evidence/controls/contextTokens",
        "/review/reviews/0/scores/correctness",
    ] {
        for invalid in [json!(true), json!("1"), Value::Null, json!(-1), json!(1.5)] {
            let mut request = base.clone();
            *request.pointer_mut(pointer).ok_or("Missing metric.")? = invalid;
            assert!(
                run(request).is_err(),
                "Accepted invalid numeric field at {pointer}"
            );
        }
    }
    for invalid in [json!(0), json!(-1), json!("NaN"), json!(1e100)] {
        let mut request = base.clone();
        request["evidence"]["samples"][0]["decodeMilliseconds"] = invalid;
        assert!(run(request).is_err());
    }
    for timestamp in [
        "2026-02-30T12:00:00Z",
        "0000-01-01T00:00:00Z",
        "2026-10-23T15:00:00+00:00",
        "2026-10-23T24:00:00Z",
    ] {
        let mut request = base.clone();
        request["review"]["reviews"][0]["reviewedAtUtc"] = json!(timestamp);
        assert!(run(request).is_err());
    }
    let mut request = base.clone();
    request["evidence"]["samples"][0]["diagnostics"] = Value::Null;
    assert!(run(request).is_err());
    let mut request = base;
    request["evidence"]["controls"]["extraArgument"] = json!("--unreviewed");
    assert!(run(request).is_err());
    Ok(())
}

#[test]
fn rejection_precedes_missing_reviews_and_proofs() -> Result<()> {
    let golden = golden()?;
    let scenario = &golden["scenarios"][1];
    let mut request =
        json!({"operation":"score", "evidence": scenario["evidence"], "review": Value::Null});
    request["evidence"]["samples"][0]["minimumAvailablePhysicalBytes"] = json!(0);
    let result = run(request)?;
    assert_eq!(result["Status"], "rejected");
    assert_eq!(result["Gates"]["Quality"], "pending");
    assert_eq!(result["Gates"]["MemoryAllowance"], "failed");
    Ok(())
}

#[test]
fn legacy_bom_detection_preserves_strict_unicode() -> Result<()> {
    let text = "{\"response\":\"snowman \u{2603}\"}";
    let mut utf8 = vec![0xef, 0xbb, 0xbf];
    utf8.extend_from_slice(text.as_bytes());
    assert_eq!(decode_text(&utf8)?, text);
    for little in [true, false] {
        let mut utf16 = if little {
            vec![0xff, 0xfe]
        } else {
            vec![0xfe, 0xff]
        };
        for unit in text.encode_utf16() {
            utf16.extend_from_slice(&if little {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        assert_eq!(decode_text(&utf16)?, text);
    }
    assert!(decode_text(&[0xff, 0xfe, 0]).is_err());
    assert!(decode_text(&[0xc0, 0x80]).is_err());
    Ok(())
}
