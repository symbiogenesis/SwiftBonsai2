//! Frozen legacy policy parity and fail-closed artifact selection checks.

use super::*;

const FROZEN: &str = include_str!("../tests/fixtures/cpu-policy-v1.json");

fn profile(value: &Value) -> Result<Profile, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

#[test]
fn complete_policies_arguments_and_artifacts_match_frozen_powershell() -> Result<(), String> {
    let frozen = parse(FROZEN)?;
    let model = frozen["modelPath"]
        .as_str()
        .ok_or("Missing frozen model path.")?;
    let adapter = frozen["adapterPath"]
        .as_str()
        .ok_or("Missing frozen adapter path.")?;
    let cases = frozen["cases"].as_array().ok_or("Missing frozen cases.")?;
    assert_eq!(cases.len(), 5);
    for case in cases {
        let selected = profile(&case["profile"])?;
        let use_adapter = case["useAbliterateLora"]
            .as_bool()
            .ok_or("Missing adapter flag.")?;
        let use_ptq1 = case["useRepackedPtq1"]
            .as_bool()
            .ok_or("Missing PTQ1 flag.")?;
        assert_eq!(policy(selected, use_adapter, use_ptq1)?, case["policy"]);
        let argv = check_arguments(model, selected, use_adapter, use_adapter.then_some(adapter))?;
        assert_eq!(json!(argv), case["arguments"]);
        let lock = parse(if selected == Profile::ReferenceCpu {
            REFERENCE_LOCK
        } else {
            SWIFT_LOCK
        })?;
        let derived = use_ptq1.then(|| parse(PTQ1_EVIDENCE)).transpose()?;
        assert_eq!(
            select_artifact(&lock, selected, use_ptq1, derived.as_ref())?,
            case["artifact"]
        );
    }
    Ok(())
}

#[test]
fn planning_selects_only_explicit_artifact_paths_and_keeps_null_adapter() -> Result<(), String> {
    let data = std::env::temp_dir().join("Bonsai policy fixture & spaces");
    let frozen = parse(FROZEN)?;
    let cases = frozen["cases"].as_array().ok_or("Missing frozen cases.")?;
    for case in cases {
        let output = run(json!({
            "operation": "plan", "dataDirectory": data,
            "profile": case["profile"],
            "useAbliterateLora": case["useAbliterateLora"],
            "useRepackedPtq1": case["useRepackedPtq1"],
        }))?;
        assert_eq!(output["Policy"], case["policy"]);
        assert_eq!(output["Artifact"], case["artifact"]);
        let filename = case["artifact"]["Filename"]
            .as_str()
            .ok_or("Missing artifact filename.")?;
        let revision = output["Lock"]["model"]["revision"]
            .as_str()
            .ok_or("Missing source revision.")?;
        let expected_model = data.join("models").join(revision).join(filename);
        assert_eq!(output["ModelPath"], json!(expected_model));
        let mut expected_arguments = case["arguments"].clone();
        expected_arguments[1] = output["ModelPath"].clone();
        if case["useAbliterateLora"] == true {
            let expected_adapter = data
                .join("adapters")
                .join("703028bb7fba03496e796b144839b161cf0f5409")
                .join("bonsai-abliterate-lora.gguf");
            assert_eq!(output["AdapterPath"], json!(expected_adapter));
            assert_eq!(output["AdapterLock"], parse(ADAPTER_LOCK)?);
            let args = expected_arguments
                .as_array_mut()
                .ok_or("Missing frozen arguments.")?;
            let index = args
                .iter()
                .position(|arg| arg == "--lora")
                .ok_or("Missing frozen adapter argument.")?;
            args[index + 1] = output["AdapterPath"].clone();
        } else {
            assert_eq!(output["AdapterPath"], Value::Null);
            assert_eq!(output["AdapterLock"], Value::Null);
        }
        assert_eq!(output["Arguments"], expected_arguments);
        assert_eq!(output.as_object().map(serde_json::Map::len), Some(7));
    }
    Ok(())
}

#[test]
fn malformed_requests_and_swift_only_reference_options_are_rejected() -> Result<(), String> {
    let valid = json!({
        "operation": "plan", "dataDirectory": std::env::temp_dir(),
        "profile": "reference-cpu", "useAbliterateLora": false, "useRepackedPtq1": false,
    });
    for (key, value) in [
        ("operation", json!("launch")),
        ("dataDirectory", json!("relative")),
        ("dataDirectory", json!("C:\\%UNEXPANDED%\\models")),
        ("dataDirectory", json!("\0")),
        ("profile", json!("ReferenceCpu")),
        ("profile", Value::Null),
        ("useAbliterateLora", json!(true)),
        ("useRepackedPtq1", json!(true)),
        ("useAbliterateLora", json!("false")),
        ("useRepackedPtq1", json!(0)),
        ("arguments", json!(["--unsafe"])),
    ] {
        let mut changed = valid.clone();
        changed[key] = value;
        assert!(run(changed).is_err(), "accepted invalid {key}");
    }
    let keys: Vec<String> = valid
        .as_object()
        .ok_or("Invalid test fixture.")?
        .keys()
        .cloned()
        .collect();
    for key in keys {
        let mut missing = valid.clone();
        missing
            .as_object_mut()
            .ok_or("Invalid fixture.")?
            .remove(&key);
        assert!(run(missing).is_err(), "accepted missing {key}");
    }
    for invalid in [json!([]), Value::Null, json!(true), json!("plan")] {
        assert!(run(invalid).is_err());
    }
    assert!(policy(Profile::ReferenceCpu, true, false).is_err());
    assert!(policy(Profile::ReferenceCpu, false, true).is_err());
    for spelling in ["100% literal", "literal %%"] {
        let mut literal = valid.clone();
        literal["dataDirectory"] = json!(std::env::temp_dir().join(spelling));
        assert!(run(literal).is_ok());
    }
    Ok(())
}

#[test]
fn malformed_ptq1_evidence_is_rejected() -> Result<(), String> {
    let lock = parse(SWIFT_LOCK)?;
    let evidence = parse(PTQ1_EVIDENCE)?;
    let selected = Profile::SwiftPq2Assessment;
    assert!(select_artifact(&lock, selected, true, None).is_err());
    assert!(select_artifact(&lock, selected, false, Some(&evidence)).is_err());
    assert!(select_artifact(&lock, Profile::ReferenceCpu, true, Some(&evidence)).is_err());
    for (key, value) in [
        ("SourceRevision", json!("0".repeat(40))),
        ("SourceSha256", json!("0".repeat(64))),
        ("SourceBytes", json!(1)),
        ("SourceBytes", json!("7206168928")),
        ("SourceBytes", json!(7_206_168_928.0)),
        ("CodecCommit", json!("0".repeat(40))),
        ("OutputFilename", json!("../unreviewed.gguf")),
        ("OutputSha256", json!("invalid")),
        ("OutputSha256", json!("A".repeat(64))),
        ("OutputBytes", json!(1)),
        ("OutputBytes", json!(-1)),
        ("OutputBytes", json!("5946648928")),
        ("ExactPq2ReconstructionVerified", json!(false)),
        ("ExactPq2ReconstructionVerified", json!("true")),
        ("NonPq2TensorBytesCopiedVerbatim", json!(false)),
        ("ConvertedTensorCount", json!(401)),
        ("ConvertedBlocks", json!(1)),
        ("WorkingBufferBytes", json!(0)),
        ("SavingBytes", json!(0)),
        ("SourceFilename", json!("other.gguf")),
        ("SourceRepository", json!("other/model")),
        ("TokenizerMetadataSha256", json!("invalid")),
        ("ModelProcessStarted", json!(true)),
        ("SchemaVersion", json!(2)),
        ("extraArguments", json!(["--unsafe"])),
    ] {
        let mut changed = evidence.clone();
        changed[key] = value;
        assert!(
            select_artifact(&lock, selected, true, Some(&changed)).is_err(),
            "accepted invalid {key}"
        );
    }
    let keys: Vec<String> = evidence
        .as_object()
        .ok_or("Invalid evidence fixture.")?
        .keys()
        .cloned()
        .collect();
    for key in keys {
        let mut missing = evidence.clone();
        missing
            .as_object_mut()
            .ok_or("Invalid fixture.")?
            .remove(&key);
        assert!(
            select_artifact(&lock, selected, true, Some(&missing)).is_err(),
            "accepted missing {key}"
        );
    }
    for malformed in [json!([]), json!([1, 2, 3]), Value::Null, json!(true)] {
        assert!(select_artifact(&lock, selected, true, Some(&malformed)).is_err());
    }
    Ok(())
}

#[test]
fn source_identity_and_runtime_url_drift_are_rejected() -> Result<(), String> {
    let lock = parse(SWIFT_LOCK)?;
    let evidence = parse(PTQ1_EVIDENCE)?;
    let selected = Profile::SwiftPq2Assessment;
    for (pointer, replacement) in [
        ("/model/revision", json!("0".repeat(40))),
        ("/model/sha256", json!("0".repeat(64))),
        ("/model/filename", json!("../other.gguf")),
        ("/model/bytes", json!(7_206_168_929_u64)),
        ("/model/bytes", json!("7206168928")),
        ("/runtime/commit", json!("0".repeat(40))),
        ("/runtime/asset/bytes", json!(1)),
        (
            "/runtime/asset/url",
            json!("https://other.example/runtime.zip"),
        ),
        ("/schemaVersion", json!(2)),
    ] {
        let mut changed = lock.clone();
        *changed
            .pointer_mut(pointer)
            .ok_or("Missing fixture pointer.")? = replacement;
        assert!(
            select_artifact(&changed, selected, false, None).is_err(),
            "accepted invalid {pointer}"
        );
        assert!(
            select_artifact(&changed, selected, true, Some(&evidence)).is_err(),
            "accepted invalid PTQ source {pointer}"
        );
    }
    Ok(())
}

#[test]
fn adapter_compatibility_and_argument_pairing_fail_closed() -> Result<(), String> {
    let lock = parse(SWIFT_LOCK)?;
    let adapter = parse(ADAPTER_LOCK)?;
    assert!(validate_adapter(&adapter, &lock).is_ok());
    for (pointer, replacement) in [
        ("/compatibility/modelRevision", json!("0".repeat(40))),
        ("/compatibility/runtimeCommit", json!("0".repeat(40))),
        ("/compatibility/modelRepository", json!("other/model")),
        ("/adapter/bytes", json!(1)),
        ("/adapter/sha256", json!("0".repeat(64))),
        ("/adapter/filename", json!("../other.gguf")),
        ("/adapter/url", json!("https://other.example/adapter.gguf")),
    ] {
        let mut changed = adapter.clone();
        *changed
            .pointer_mut(pointer)
            .ok_or("Missing adapter fixture pointer.")? = replacement;
        assert!(
            validate_adapter(&changed, &lock).is_err(),
            "accepted invalid {pointer}"
        );
    }
    for (enabled, path) in [
        (true, None),
        (false, Some("adapter.gguf")),
        (true, Some("  ")),
        (true, Some("a\0b")),
    ] {
        assert!(check_arguments("model.gguf", Profile::SwiftPq2Assessment, enabled, path).is_err());
    }
    let model = "C:\\external model & fixtures\\Swift.gguf";
    let arguments = completion_arguments(model, Profile::SwiftPq2Assessment)?;
    assert_eq!(arguments[1], model);
    for flag in ["--temp", "--reasoning", "--n-predict", "--lora", "--prompt"] {
        assert!(!arguments.iter().any(|argument| argument == flag));
    }
    for invalid in ["", "  ", "model\0.gguf"] {
        assert!(completion_arguments(invalid, Profile::ReferenceCpu).is_err());
    }
    assert_eq!(
        Profile::ReferenceCpu.memory_limit_bytes(),
        MEMORY_LIMIT_BYTES
    );
    assert_eq!(
        Profile::ReferenceCpu.required_available_bytes(),
        REQUIRED_AVAILABLE_BYTES
    );
    assert_eq!(
        Profile::SwiftPq2Assessment.memory_limit_bytes(),
        12_616_466_432
    );
    Ok(())
}

#[test]
fn swift_development_budget_keeps_four_gib_reserve_for_every_artifact() -> Result<(), String> {
    let selected = Profile::SwiftPq2Assessment;
    let ceiling =
        u64::try_from(selected.memory_limit_bytes()).map_err(|error| error.to_string())?;
    assert_eq!(ceiling, 12_616_466_432);
    assert_eq!(selected.required_available_bytes(), 16_911_433_728);
    assert_eq!(
        selected.required_available_bytes() - ceiling,
        4 * 1024 * 1024 * 1024
    );
    for use_adapter in [false, true] {
        for use_ptq1 in [false, true] {
            let fixed = policy(selected, use_adapter, use_ptq1)?;
            assert_eq!(fixed["ServiceCeilingBytes"], ceiling);
            assert_eq!(fixed["RequiredAvailableBytes"], 16_911_433_728_u64);
            assert_eq!(fixed["AvailableMemoryFloorBytes"], 4_294_967_296_u64);
            assert_eq!(fixed["ContextTokens"], 8192);
            assert_eq!(fixed["BatchTokens"], 128);
            assert_eq!(fixed["MicrobatchTokens"], 64);
            assert_eq!(
                fixed["EstimatedRuntimeHeadroomBytes"].as_u64(),
                fixed["EstimatedKnownSubtotalBytes"]
                    .as_u64()
                    .map(|known| ceiling - known)
            );
        }
    }
    assert_eq!(Profile::ReferenceCpu.memory_limit_bytes(), 19_327_352_832);
    assert_eq!(
        Profile::ReferenceCpu.required_available_bytes(),
        23_622_320_128
    );
    Ok(())
}
