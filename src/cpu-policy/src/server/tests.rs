//! Frozen PowerShell parity and strict server policy request regressions.

use super::*;

fn frozen() -> Result<Value, String> {
    serde_json::from_str(include_str!("../../tests/fixtures/server-policy-v1.json"))
        .map_err(|error| error.to_string())
}

fn request() -> Result<Value, String> {
    let fixture = frozen()?;
    let plan = &fixture["plan"];
    Ok(json!({
        "operation": "plan", "profile": fixture["profile"],
        "modelPath": plan["ModelPath"], "keyFile": plan["PlannedKeyFile"],
        "port": 18080, "runtimeCommit": plan["RuntimeCommit"],
        "modelRevision": plan["ModelRevision"], "modelBytes": 13_146_393_504_u64,
        "environmentNames": ["SystemRoot", "TEMP", "USERPROFILE"],
    }))
}

#[test]
fn complete_plan_and_exact_arguments_match_frozen_powershell() -> Result<(), String> {
    assert_eq!(run(request()?)?, frozen()?["plan"]);
    Ok(())
}

#[test]
fn every_fixed_profile_field_rejects_value_and_type_drift() -> Result<(), String> {
    let request = request()?;
    let profile = request["profile"]
        .as_object()
        .ok_or("Missing fixture profile.")?;
    for (name, expected) in profile {
        if ["estimateScope", "remainingGates"].contains(&name.as_str()) {
            continue;
        }
        for value in [Value::Null, json!({}), json!([]), json!("unreviewed")] {
            let mut drifted = request.clone();
            drifted["profile"][name] = value;
            assert!(run(drifted).is_err(), "Accepted type drift: {name}");
        }
        let changed = match expected {
            Value::Number(_) => json!(expected.as_u64().ok_or("Noninteger profile fixture.")? + 1),
            Value::Bool(value) => json!(!value),
            Value::String(value) => json!(value.to_uppercase()),
            _ => return Err("Unexpected fixed fixture type.".into()),
        };
        let mut drifted = request.clone();
        drifted["profile"][name] = changed;
        assert!(run(drifted).is_err(), "Accepted value drift: {name}");
        if let Some(number) = expected.as_u64() {
            // A mathematically integral floating point value is still mistyped.
            let mut drifted = request.clone();
            drifted["profile"][name] =
                serde_json::from_str(&format!("{number}.0")).map_err(|error| error.to_string())?;
            assert!(run(drifted).is_err(), "Accepted floating point: {name}");
        }
    }
    Ok(())
}

#[test]
fn profile_requires_exact_field_set_and_explicit_qualification_gaps() -> Result<(), String> {
    for (name, value) in [
        ("extraArguments", json!(["--tools", "all"])),
        ("estimateScope", json!(" \t\r\n")),
        ("estimateScope", json!(true)),
        ("remainingGates", json!([])),
        ("remainingGates", json!([" "])),
        ("remainingGates", json!([true])),
        ("remainingGates", json!("pending")),
    ] {
        let mut input = request()?;
        input["profile"][name] = value;
        assert!(run(input).is_err(), "Accepted profile mutation: {name}");
    }
    for name in ["schemaVersion", "estimateScope", "remainingGates"] {
        let mut input = request()?;
        let profile = input["profile"].as_object_mut().ok_or("Missing profile.")?;
        let removed = profile.remove(name).ok_or("Missing profile field.")?;
        assert!(
            run(input.clone()).is_err(),
            "Accepted missing field: {name}"
        );
        input["profile"][name.to_uppercase()] = removed;
        assert!(run(input).is_err(), "Accepted wrong field case: {name}");
    }
    let mut input = request()?;
    input["profile"]["estimateScope"] = json!("Reviewed source estimate, not observed.");
    input["profile"]["remainingGates"] = json!(["Explicitly still unqualified"]);
    assert!(run(input).is_ok());
    Ok(())
}

#[test]
fn environment_policy_uses_names_without_echoing_values() -> Result<(), String> {
    for name in [
        "llama_arg_agent",
        "LLAMACPP_FOO",
        "GGML_BACKEND_PATH",
        "AIP_MODE",
        "aip_http_port",
        "MTMD_BACKEND_DEVICE",
        "HF_TOKEN",
        "HF_ENDPOINT",
        "MODEL_ENDPOINT",
    ] {
        let input = json!({ "operation": "environment", "names": [name] });
        let error = run(input).err().ok_or("Accepted forbidden name.")?;
        assert!(error.contains(name));
        let mut input = request()?;
        input["environmentNames"] = json!([name]);
        assert!(run(input).is_err());
    }
    let allowed = [
        "SystemDrive",
        "SystemRoot",
        "windir",
        "ProgramData",
        "LOCALAPPDATA",
        "APPDATA",
        "USERPROFILE",
        "HF_TOKEN_SUFFIX",
        "PREFIX_LLAMA_TEST",
        "AIP",
        "MTMD",
    ];
    assert_eq!(
        run(json!({ "operation": "environment", "names": allowed }))?,
        json!({ "Validated": true })
    );
    let error = run(json!({
        "operation": "environment", "names": ["HF_TOKEN"],
        "values": ["PRIVATE-VALUE-NEVER-PRINT"],
    }))
    .err()
    .ok_or("Accepted environment values.")?;
    assert!(!error.contains("PRIVATE-VALUE-NEVER-PRINT"));
    Ok(())
}

#[test]
fn requests_fail_closed_and_shared_arguments_use_reviewed_batches() -> Result<(), String> {
    for (name, value) in [
        ("operation", json!("launch")),
        ("extraArguments", json!([])),
        ("port", json!(0)),
        ("port", json!(1023)),
        ("port", json!(65536)),
        ("port", json!("18080")),
        ("modelPath", json!("")),
        ("keyFile", json!("C:\\key\0suffix")),
        ("runtimeCommit", json!("main")),
        ("modelRevision", json!("f".repeat(39))),
        ("modelBytes", json!(0)),
        ("environmentNames", json!({ "HF_TOKEN": "secret" })),
    ] {
        let mut input = request()?;
        input[name] = value;
        assert!(run(input).is_err(), "Accepted request mutation: {name}");
    }
    for input in [Value::Null, json!([]), json!({}), json!([request()?])] {
        assert!(run(input).is_err());
    }
    for profile in [Profile::ReferenceCpu, Profile::SwiftPq2Assessment] {
        let args = arguments("C:\\model path & x", "C:\\key path & y", 65535, profile)?;
        assert_eq!(args[1], "C:\\model path & x");
        assert_eq!(args[7], "C:\\key path & y");
        assert_eq!(args[9], "http://127.0.0.1:65535");
        for (flag, value) in [
            ("--batch-size", profile.batch_tokens()),
            ("--ubatch-size", profile.microbatch_tokens()),
        ] {
            let index = args
                .iter()
                .position(|arg| arg == flag)
                .ok_or("Missing batch.")?;
            assert_eq!(args[index + 1], value.to_string());
        }
    }
    Ok(())
}
