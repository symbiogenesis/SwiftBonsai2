#![forbid(unsafe_code)]
//! Fixed launch arguments and environment-policy regressions; no real model starts.
use bonsai_broker::launch_policy::{
    LaunchPlan, LaunchProfile, MEMORY_LIMIT_BYTES, PolicyError, REQUIRED_AVAILABLE_BYTES,
    validate_environment,
};
use std::{ffi::OsString, path::PathBuf};

#[test]
fn repacked_ptq1_selects_current_weight_identity_with_same_runtime_limits_and_adapter()
-> Result<(), PolicyError> {
    let data = std::env::temp_dir().join("Bonsai PTQ1 data & spaces");
    let key = std::env::temp_dir().join("Bonsai PTQ1 backend.key");
    let profile = LaunchProfile::SwiftPq2Assessment;
    let source = LaunchPlan::for_profile_with_lora(&data, 49152, &key, profile, true)?;
    let repacked = LaunchPlan::for_profile_with_options(&data, 49152, &key, profile, true, true)?;
    assert_eq!(
        repacked.model_path,
        data.join("models")
            .join("a3bdac086bbb6b04d87d0108d18943b5f63868f7")
            .join("Swift-Bonsai-2-current-repacked-PTQ1_0.gguf")
    );
    assert_ne!(repacked.model_path, source.model_path);
    assert_eq!(PathBuf::from(&repacked.arguments[1]), repacked.model_path);
    let mut expected = source.arguments;
    expected[1] = repacked.model_path.as_os_str().to_owned();
    assert_eq!(repacked.arguments, expected);
    assert_eq!(repacked.runtime_directory, source.runtime_directory);
    assert_eq!(repacked.archive_path, source.archive_path);
    assert_eq!(repacked.executable, source.executable);
    assert_eq!(repacked.memory_limit_bytes, source.memory_limit_bytes);
    assert_eq!(
        repacked.required_available_bytes,
        source.required_available_bytes
    );
    let adapter = repacked.adapter.ok_or(PolicyError::Configuration)?;
    let source_adapter = source.adapter.ok_or(PolicyError::Configuration)?;
    assert_eq!(adapter.path, source_adapter.path);
    assert_eq!(adapter.sha256, source_adapter.sha256);
    assert_eq!(adapter.bytes, source_adapter.bytes);
    assert!(
        LaunchPlan::for_profile_with_options(
            &data,
            49152,
            &key,
            LaunchProfile::ReferenceCpu,
            false,
            true,
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn lora_plan_uses_only_pinned_adapter_without_changing_cpu_limits() -> Result<(), PolicyError> {
    let data = std::env::temp_dir().join("Bonsai LoRA data & spaces");
    let key = std::env::temp_dir()
        .join("Bonsai LoRA key")
        .join("backend.key");
    let profile = LaunchProfile::SwiftPq2Assessment;
    let base = LaunchPlan::for_profile(&data, 49152, &key, profile)?;
    assert!(base.adapter.is_none());
    let mut enabled = LaunchPlan::for_profile_with_lora(&data, 49152, &key, profile, true)?;
    let adapter = enabled.adapter.as_ref().ok_or(PolicyError::Configuration)?;
    let expected = data
        .join("adapters")
        .join("703028bb7fba03496e796b144839b161cf0f5409")
        .join("bonsai-abliterate-lora.gguf");
    assert_eq!(adapter.path, expected);
    assert_eq!(adapter.bytes, 9_682_464);
    assert_eq!(
        adapter.sha256,
        "f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42"
    );
    assert_eq!(enabled.arguments.pop(), Some(expected.into_os_string()));
    assert_eq!(enabled.arguments.pop(), Some("--lora".into()));
    assert_eq!(enabled.arguments, base.arguments);
    assert_eq!(enabled.memory_limit_bytes, base.memory_limit_bytes);
    assert_eq!(
        enabled.required_available_bytes,
        base.required_available_bytes
    );
    assert!(
        LaunchPlan::for_profile_with_lora(&data, 49152, &key, LaunchProfile::ReferenceCpu, true)
            .is_err()
    );
    Ok(())
}

#[test]
fn inherited_overrides_are_case_insensitive_and_value_free() {
    for name in [
        "LLAMA_MODEL",
        "llamacpp_port",
        "GGML_BACKEND_PATH",
        "AIP_MODEL",
        "MTMD_MODEL",
        "HF_TOKEN",
        "HF_ENDPOINT",
        "MODEL_ENDPOINT",
    ] {
        assert_eq!(
            validate_environment([OsString::from(name)]),
            Err(PolicyError::Environment)
        );
    }
    assert!(validate_environment([OsString::from("PATH"), OsString::from("HF_HOME")]).is_ok());
}

#[test]
fn swift_plan_selects_only_pinned_pq2_runtime_and_assessment_limits() {
    let data = std::env::temp_dir().join("Bonsai Swift policy unused data");
    let key = std::env::temp_dir()
        .join("Bonsai Swift policy unused key")
        .join("backend.key");
    let plan = LaunchPlan::for_profile(&data, 49152, &key, LaunchProfile::SwiftPq2Assessment);
    assert!(plan.is_ok());
    let Ok(plan) = plan else {
        return;
    };
    assert_eq!(plan.memory_limit_bytes, 12_348_030_976);
    assert_eq!(plan.required_available_bytes, 16_642_998_272);
    assert_eq!(
        u64::try_from(plan.memory_limit_bytes),
        Ok(plan.required_available_bytes - 4 * 1024 * 1024 * 1024)
    );
    assert_eq!(
        plan.model_path,
        data.join("models")
            .join("a3bdac086bbb6b04d87d0108d18943b5f63868f7")
            .join("Swift-Bonsai-2-PQ2_0.gguf")
    );
    assert_eq!(
        plan.runtime_directory,
        data.join("runtime")
            .join("73bfe6cc485554822f80c473a3c0c4f25e80f41b")
    );
    assert_eq!(
        plan.archive_path,
        data.join("downloads")
            .join("llama-prism-b10787-73bfe6c-bin-win-cpu-arm64.zip")
    );
    assert_eq!(
        plan.executable,
        plan.runtime_directory.join("llama-server.exe")
    );
    let pairs: Vec<_> = plan.arguments.windows(2).collect();
    for pair in [
        ["--batch-size", "128"],
        ["--ubatch-size", "64"],
        ["--ctx-size", "8192"],
        ["--parallel", "1"],
        ["--cache-type-k", "f16"],
        ["--cache-type-v", "f16"],
        ["--gpu-layers", "0"],
        ["--device", "none"],
    ] {
        assert!(
            pairs
                .iter()
                .any(|actual| actual[0] == pair[0] && actual[1] == pair[1])
        );
    }
    for required in [
        "--offline",
        "--no-ui",
        "--no-agent",
        "--no-ui-mcp-proxy",
        "--no-repack",
        "--no-mmproj",
        "--api-key-file",
    ] {
        assert!(plan.arguments.iter().any(|argument| argument == required));
    }
    for forbidden in ["--lora", "--lora-scaled", "--model-url", "--hf-repo"] {
        assert!(!plan.arguments.iter().any(|argument| argument == forbidden));
    }
    assert!(LaunchPlan::for_profile(&data, 80, &key, LaunchProfile::SwiftPq2Assessment).is_err());
}

#[test]
fn pinned_plan_uses_independent_tokens_and_fixed_memory_budget() {
    let data = std::env::temp_dir().join("Bonsai launch policy unused data");
    let key = std::env::temp_dir()
        .join("Bonsai launch policy unused key")
        .join("backend.key");
    let plan = LaunchPlan::reference(&data, 49152, &key);
    assert!(plan.is_ok());
    let Ok(plan) = plan else {
        return;
    };
    assert_eq!(plan.memory_limit_bytes, MEMORY_LIMIT_BYTES);
    assert_eq!(plan.required_available_bytes, REQUIRED_AVAILABLE_BYTES);
    assert_eq!(MEMORY_LIMIT_BYTES, 18 * 1024 * 1024 * 1024);
    assert_eq!(REQUIRED_AVAILABLE_BYTES, 22 * 1024 * 1024 * 1024);
    assert_eq!(plan.arguments[0], "--model");
    assert_eq!(PathBuf::from(&plan.arguments[1]), plan.model_path);
    assert_eq!(plan.arguments[6], "--api-key-file");
    assert_eq!(PathBuf::from(&plan.arguments[7]), key);
    assert!(plan.arguments.iter().any(|arg| arg == "--offline"));
    assert!(plan.arguments.iter().any(|arg| arg == "--no-ui"));
    assert!(plan.arguments.iter().any(|arg| arg == "--no-agent"));
    for expected in [
        ["--reasoning", "on"],
        ["--reasoning-effort", "xhigh"],
        ["--reasoning-budget", "-1"],
        ["--presence-penalty", "0.0"],
        ["--frequency-penalty", "0.0"],
        ["--repeat-penalty", "1.0"],
        ["--ctx-size", "8192"],
        ["--n-predict", "4096"],
        ["--cache-type-k", "f16"],
        ["--cache-type-v", "f16"],
    ] {
        assert!(plan.arguments.windows(2).any(|pair| pair == expected));
    }
    assert!(!plan.arguments.iter().any(|arg| arg == "--system-prompt"));
    assert!(
        !plan
            .arguments
            .iter()
            .any(|arg| arg == "--reasoning-max-tokens-floor")
    );
    assert!(LaunchPlan::reference(&data, 80, &key).is_err());
    assert!(LaunchPlan::reference(&PathBuf::from("relative"), 49152, &key).is_err());
}
