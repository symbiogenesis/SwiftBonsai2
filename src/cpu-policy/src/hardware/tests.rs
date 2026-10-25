//! Cross-repository wire compatibility and evidence invalidation regressions.

use super::*;
use serde_json::{Value, json};

const RUNTIME: &str = include_str!("../../tests/fixtures/runtime-capabilities-v1.json");

fn key() -> Value {
    json!({
        "schema_version": 1,
        "objective": "interactive",
        "model_sha256": "1".repeat(64),
        "adapter": {"kind": "none"},
        "runtime": {
            "archive_sha256": "2".repeat(64),
            "source_revision": "3".repeat(40),
            "target": {"architecture": "aarch64", "os": "windows", "environment": "msvc"}
        },
        "capability_class_sha256": "4".repeat(64),
        "backend_driver_sha256": "5".repeat(64),
        "graph_contract": "qwen35-text-v1:nonflash-f16:kv-f16:gdn-f32:prism-arm64-fused-v1",
        "state_layout": "kv-f16:gdn-f32",
        "settings_sha256": "6".repeat(64),
        "workload": {
            "context_tokens": 8192,
            "minimum_prompt_tokens": 1,
            "maximum_prompt_tokens": 4096,
            "maximum_output_tokens": 4096,
            "state": "fresh"
        }
    })
}

fn receipt(compatibility: &Value) -> Value {
    json!({
        "schema_version": 1,
        "compatibility": compatibility,
        "tested_main_revision": "7".repeat(40),
        "ci_run_id": "123456789",
        "capture_sha256": "8".repeat(64),
        "verdict": "unknown",
        "energy_verdict": "unknown"
    })
}

#[test]
fn frozen_prism_report_is_scoped_discovery_and_rejects_hostile_claims() -> Result<(), String> {
    let report = RuntimeCapabilities::from_json(RUNTIME)?;
    assert!(report.target().is_windows_arm64());
    assert!(report.neon_build_eligible());
    assert!(
        report.declares_contract("qwen35-text-v1:nonflash-f16:kv-f16:gdn-f32:prism-arm64-fused-v1")
    );
    assert!(!report.declares_contract("vulkan"));
    let valid: Value = parse(RUNTIME)?;
    for (pointer, replacement) in [
        ("/schema_version", json!(2)),
        ("/schema_version", json!("1")),
        ("/kind", json!("runner-host-capabilities")),
        ("/runtime/name", json!("other-runtime")),
        ("/runtime/version", json!("x".repeat(193))),
        ("/runtime/target/architecture", json!("x86_64")),
        ("/runtime/target/os", json!("linux")),
        ("/cpu/scalar", json!(false)),
        ("/cpu/neon_build_eligible", json!("true")),
        ("/backends/vulkan", json!("available")),
        ("/qualification", json!("passed")),
        ("/supported_contracts", json!([])),
        ("/supported_contracts", json!(["same", "same"])),
        ("/supported_contracts", json!(vec!["x"; 17])),
        ("/supported_contracts", json!(["invalid\ncontract"])),
    ] {
        let mut changed = valid.clone();
        *changed
            .pointer_mut(pointer)
            .ok_or("Invalid test pointer.")? = replacement;
        assert!(
            RuntimeCapabilities::from_json(&changed.to_string()).is_err(),
            "{pointer}"
        );
    }
    for pointer in ["", "/runtime", "/runtime/target", "/cpu", "/backends"] {
        let mut changed = valid.clone();
        changed
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("Invalid fixture object.")?
            .insert("unreviewed".into(), json!(true));
        assert!(RuntimeCapabilities::from_json(&changed.to_string()).is_err());
    }
    assert!(RuntimeCapabilities::from_json(&" ".repeat(MAX_DOCUMENT_BYTES + 1)).is_err());
    assert!(
        RuntimeCapabilities::from_json(&RUNTIME.replacen(
            "\"schema_version\": 1",
            "\"schema_version\": 1, \"schema_version\": 1",
            1
        ))
        .is_err()
    );
    let mut scalar_only = valid;
    scalar_only["runtime"]["target"]["architecture"] = json!("x86_64");
    scalar_only["cpu"]["neon_build_eligible"] = json!(false);
    let scalar_only = RuntimeCapabilities::from_json(&scalar_only.to_string())?;
    assert!(!scalar_only.target().is_windows_arm64());
    assert!(!scalar_only.neon_build_eligible());
    Ok(())
}

#[test]
fn evidence_keys_invalidate_on_each_immutable_compatibility_change() -> Result<(), String> {
    let valid = key();
    let current = ProfileCompatibilityKey::from_json(&valid.to_string())?;
    let evidence = ProfileEvidence::from_json(&receipt(&valid).to_string())?;
    // The unknown verdict deliberately still matches structurally; this API
    // cannot grant qualification or admission from a receipt's asserted status.
    assert!(evidence.applies_to(&current));
    for (pointer, replacement) in [
        ("/objective", json!("battery")),
        ("/model_sha256", json!("a".repeat(64))),
        (
            "/adapter",
            json!({"kind": "artifact", "sha256": "a".repeat(64)}),
        ),
        ("/runtime/archive_sha256", json!("a".repeat(64))),
        ("/runtime/source_revision", json!("a".repeat(40))),
        ("/runtime/target/architecture", json!("x86_64")),
        ("/capability_class_sha256", json!("a".repeat(64))),
        ("/backend_driver_sha256", json!("a".repeat(64))),
        ("/graph_contract", json!("other-contract")),
        ("/state_layout", json!("other-state")),
        ("/settings_sha256", json!("a".repeat(64))),
        ("/workload/context_tokens", json!(16384)),
        ("/workload/minimum_prompt_tokens", json!(2)),
        ("/workload/maximum_prompt_tokens", json!(4095)),
        ("/workload/maximum_output_tokens", json!(4095)),
        ("/workload/state", json!("reused")),
    ] {
        let mut changed = valid.clone();
        *changed
            .pointer_mut(pointer)
            .ok_or("Invalid test pointer.")? = replacement;
        let changed = ProfileCompatibilityKey::from_json(&changed.to_string())?;
        assert!(!evidence.applies_to(&changed), "{pointer}");
    }
    assert_eq!(
        serde_json::to_value(&current).map_err(|error| error.to_string())?,
        valid
    );
    Ok(())
}

#[test]
fn unknown_identity_and_impossible_reservations_never_become_keys() -> Result<(), String> {
    let valid = key();
    for (pointer, replacement) in [
        ("/schema_version", json!(2)),
        ("/model_sha256", Value::Null),
        ("/model_sha256", json!("A".repeat(64))),
        ("/adapter", Value::Null),
        ("/adapter", json!({"kind": "unknown"})),
        ("/adapter", json!({"kind": "none", "sha256": null})),
        ("/adapter", json!({"kind": "artifact", "sha256": "unknown"})),
        (
            "/adapter",
            json!({"kind": "none", "sha256": "a".repeat(64)}),
        ),
        ("/capability_class_sha256", json!("unknown")),
        ("/backend_driver_sha256", Value::Null),
        ("/runtime/source_revision", json!("main")),
        ("/workload/maximum_output_tokens", json!(0)),
        ("/workload/maximum_output_tokens", json!(4097)),
        ("/workload/maximum_prompt_tokens", json!(u32::MAX)),
        ("/workload/minimum_prompt_tokens", json!(4097)),
        ("/workload/minimum_prompt_tokens", json!(0)),
        ("/workload/state", json!("unknown")),
    ] {
        let mut changed = valid.clone();
        *changed
            .pointer_mut(pointer)
            .ok_or("Invalid test pointer.")? = replacement;
        assert!(
            ProfileCompatibilityKey::from_json(&changed.to_string()).is_err(),
            "{pointer}"
        );
    }
    for name in valid.as_object().ok_or("Invalid fixture.")?.keys() {
        let mut changed = valid.clone();
        changed
            .as_object_mut()
            .ok_or("Invalid fixture.")?
            .remove(name);
        assert!(
            ProfileCompatibilityKey::from_json(&changed.to_string()).is_err(),
            "{name}"
        );
    }
    let duplicate = valid.to_string().replace(
        "\"kind\":\"none\"",
        "\"kind\":\"artifact\",\"kind\":\"none\"",
    );
    assert!(ProfileCompatibilityKey::from_json(&duplicate).is_err());
    Ok(())
}

#[test]
fn battery_pass_requires_energy_evidence_and_fallback_reasons_are_value_free() -> Result<(), String>
{
    let mut battery = key();
    battery["objective"] = json!("battery");
    let mut evidence = receipt(&battery);
    evidence["verdict"] = json!("passed");
    assert!(ProfileEvidence::from_json(&evidence.to_string()).is_err());
    evidence["energy_verdict"] = json!("passed");
    assert!(ProfileEvidence::from_json(&evidence.to_string()).is_ok());
    for (name, replacement) in [
        ("schema_version", json!(2)),
        ("tested_main_revision", json!("main")),
        ("capture_sha256", json!("unknown")),
        ("ci_run_id", json!("https://example.test/run")),
        ("ci_run_id", json!(123)),
        ("ci_run_id", json!("")),
        ("verdict", json!("accepted")),
        ("additional", json!(true)),
    ] {
        let mut changed = evidence.clone();
        changed[name] = replacement;
        assert!(
            ProfileEvidence::from_json(&changed.to_string()).is_err(),
            "{name}"
        );
    }
    assert_eq!(
        serde_json::to_value(FallbackReason::StaleEvidence).map_err(|error| error.to_string())?,
        json!("stale-evidence")
    );
    assert!(serde_json::from_value::<FallbackReason>(json!("driver-name:private")).is_err());
    Ok(())
}
