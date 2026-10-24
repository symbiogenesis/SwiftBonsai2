#![forbid(unsafe_code)]
//! Fixed launch arguments and environment-policy regressions; no real model starts.
use bonsai_broker::launch_policy::{
    LaunchPlan, MEMORY_LIMIT_BYTES, PolicyError, REQUIRED_AVAILABLE_BYTES, validate_environment,
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
