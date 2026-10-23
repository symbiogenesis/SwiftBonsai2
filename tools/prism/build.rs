//! Links opt-in regression tests to an externally built, pinned ggml engine.

use std::env;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-env-changed=BONSAI_PRISM_NATIVE_LIB_DIR");
    println!("cargo:rerun-if-env-changed=BONSAI_PRISM_NATIVE_LINK_KIND");
    if env::var_os("CARGO_FEATURE_NATIVE_TESTS").is_none() {
        return Ok(());
    }
    let directories = env::var_os("BONSAI_PRISM_NATIVE_LIB_DIR").ok_or(
        "native-tests requires BONSAI_PRISM_NATIVE_LIB_DIR with absolute ggml library directories",
    )?;
    let mut found_directory = false;
    for directory in env::split_paths(&directories) {
        if !directory.is_absolute()
            || !directory.is_dir()
            || directory.to_string_lossy().contains('%')
        {
            return Err("BONSAI_PRISM_NATIVE_LIB_DIR entries must be existing, expanded absolute directories".into());
        }
        println!("cargo:rustc-link-search=native={}", directory.display());
        found_directory = true;
    }
    if !found_directory {
        return Err("BONSAI_PRISM_NATIVE_LIB_DIR must contain a directory".into());
    }
    let kind = env::var("BONSAI_PRISM_NATIVE_LINK_KIND").unwrap_or_else(|_| "static".to_owned());
    if kind != "static" && kind != "dylib" {
        return Err("BONSAI_PRISM_NATIVE_LINK_KIND must be static or dylib".into());
    }
    for library in ["ggml", "ggml-cpu", "ggml-base"] {
        println!("cargo:rustc-link-lib={kind}={library}");
    }
    if kind == "static" {
        match env::var("CARGO_CFG_TARGET_OS")?.as_str() {
            "linux" => {
                for library in ["stdc++", "m", "dl", "pthread"] {
                    println!("cargo:rustc-link-lib={library}");
                }
            }
            "macos" => println!("cargo:rustc-link-lib=c++"),
            // MSVC-built ggml carries its runtime's default-library directives.
            "windows" => {}
            other => {
                return Err(format!(
                    "native-tests does not configure ggml runtime libraries for {other}"
                )
                .into());
            }
        }
    }
    Ok(())
}
