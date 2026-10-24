#![forbid(unsafe_code)]
//! Fixed launch arguments and environment-policy regressions; no real model starts.
use bonsai_broker::launch_policy::{
    LaunchPlan, LaunchProfile, MEMORY_LIMIT_BYTES, PolicyError, REQUIRED_AVAILABLE_BYTES,
    validate_environment,
};
use std::{ffi::OsString, path::PathBuf};

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
    assert_eq!(plan.memory_limit_bytes, 12 * 1024 * 1024 * 1024);
    assert_eq!(plan.required_available_bytes, 16 * 1024 * 1024 * 1024);
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
