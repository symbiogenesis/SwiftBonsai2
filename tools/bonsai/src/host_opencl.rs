//! Read-only `OpenCL` feasibility observations in an unchanged headless worker Job.

use serde_json::{Value, json};

fn validate(request: &Value) -> crate::Result<()> {
    if request.as_object().is_some_and(serde_json::Map::is_empty) {
        Ok(())
    } else {
        Err("Native OpenCL inventory requires an empty request object.".into())
    }
}

fn unknown() -> Value {
    json!({"schema_version": 1, "kind": "runner-native-opencl-observations",
        "status": "unknown", "source": "system-opencl-loader",
        "architecture": std::env::consts::ARCH, "platforms": null,
        "loader": null, "execution_policy": "headless-worker",
        "qualification": "unqualified", "backend_ready": false,
        "error": "native-opencl-query-unavailable"})
}

/// Query installed `OpenCL` GPU capabilities in a bounded native ARM64 child.
///
/// Uses an unchanged headless worker policy, 512 MiB one-process Job, ten-second
/// deadline and 1 MiB output cap. Creates no contexts, queues, programs, kernels
/// or device allocations. An unavailable implementation stays explicitly unknown.
///
/// # Errors
/// Rejects nonempty requests, invalid scratch paths or failed evidence cleanup.
pub fn run(request: &Value) -> crate::Result<Value> {
    validate(request)?;
    #[cfg(windows)]
    {
        windows::run()
    }
    #[cfg(not(windows))]
    Ok(unknown())
}

/// Internal child entry point; callers must use the deadline-owned `host-opencl`.
///
/// No model, `OpenCL` context or executable program is created. Driver calls have
/// no intrinsic timeout and require the owning parent's bounded Job lifetime.
///
/// # Errors
/// Rejects nonempty requests, failed native queries or unavailable loader identity.
pub fn query(request: &Value) -> crate::Result<Value> {
    validate(request)?;
    #[cfg(windows)]
    {
        windows::query()
    }
    #[cfg(not(windows))]
    Ok(unknown())
}

#[cfg(any(windows, test))]
fn checked_text(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|text| text.len() < 16 * 1024 && !text.chars().any(char::is_control))
}

#[cfg(any(windows, test))]
fn checked_device(device: &Value) -> bool {
    if device["backend_ready"] != false
        || device["packed_swift_qualified"] != false
        || device["device_type"] != "gpu"
        || !device["available"].is_boolean()
        || !device["compiler_available"].is_boolean()
        || !matches!(device["address_bits"].as_u64(), Some(32 | 64))
        || device["vendor_id"]
            .as_u64()
            .is_none_or(|id| u32::try_from(id).is_err())
        || !device["memory_budget_bytes"].is_null()
        || ![
            "name",
            "vendor",
            "version",
            "driver_version",
            "opencl_c_version",
        ]
        .iter()
        .all(|key| checked_text(&device[key]))
        || ![
            "global_memory_capacity_bytes",
            "maximum_allocation_bytes",
            "local_memory_bytes",
        ]
        .iter()
        .all(|key| device[key].as_u64().is_some())
    {
        return false;
    }
    let Some(extensions) = device["extensions"]
        .as_array()
        .filter(|items| items.len() <= 512)
    else {
        return false;
    };
    let mut previous = None;
    for extension in extensions {
        let Some(name) = extension.as_str().filter(|name| {
            !name.is_empty()
                && name.len() <= 255
                && name.is_ascii()
                && !name.chars().any(char::is_whitespace)
                && !name.chars().any(char::is_control)
        }) else {
            return false;
        };
        if previous.is_some_and(|previous| previous >= name) {
            return false;
        }
        previous = Some(name);
    }
    true
}

#[cfg(any(windows, test))]
fn checked_report(report: &Value) -> bool {
    if report["schema_version"] != 1
        || report["kind"] != "runner-native-opencl-observations"
        || report["source"] != "system-opencl-loader"
        || report["architecture"] != "aarch64"
        || report["execution_policy"] != "headless-worker"
        || report["qualification"] != "unqualified"
        || report["backend_ready"] != false
        || report["status"] != "observed"
        || !report["error"].is_null()
        || !report["loader"]["sha256"]
            .as_str()
            .is_some_and(|hash| crate::identity::is_lower_hex(hash, 64))
        || !report["loader"]["bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0 && bytes <= 16 * 1024 * 1024)
        || report["loader"]["file"] != "OpenCL.dll"
        || report["loader"]["signature_verified"] != false
        || report["loader"]["dependencies_verified"] != false
    {
        return false;
    }
    let Some(platforms) = report["platforms"]
        .as_array()
        .filter(|items| items.len() <= 8)
    else {
        return false;
    };
    let mut count = 0;
    for platform in platforms {
        if !["name", "vendor", "version"]
            .iter()
            .all(|key| checked_text(&platform[key]))
        {
            return false;
        }
        let Some(devices) = platform["devices"].as_array() else {
            return false;
        };
        count += devices.len();
        if count > 16 {
            return false;
        }
        if !devices.iter().all(checked_device) {
            return false;
        }
    }
    true
}

#[cfg(windows)]
mod windows {
    use std::{
        fs,
        io::Read as _,
        time::{SystemTime, UNIX_EPOCH},
    };

    use bonsai_artifacts::validate_external_path;
    use bonsai_windows_process::ProcessSpec;
    use sha2::{Digest as _, Sha256};

    use super::{Value, checked_report, json, unknown};

    pub(super) fn query() -> crate::Result<Value> {
        let inventory =
            bonsai_windows_process::opencl_inventory().map_err(|error| error.to_string())?;
        // This bounded file observation is not a signature/provenance verdict or
        // a claim to have enumerated every dependency loaded by the system ICD.
        let file = fs::File::open(&inventory.loader_path).map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        file.take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
            return Err("System OpenCL loader identity exceeds its bound.".into());
        }
        let loader = json!({"file": "OpenCL.dll", "bytes": bytes.len(), "sha256": crate::identity::hex(&Sha256::digest(&bytes)), "signature_verified": false, "dependencies_verified": false});
        let platforms: Vec<_> = inventory.platforms.iter().map(|platform| json!({
            "name": platform.name, "vendor": platform.vendor, "version": platform.version,
            "devices": platform.devices.iter().map(|device| json!({
                "name": device.name, "vendor": device.vendor, "vendor_id": device.vendor_id,
                "version": device.version, "driver_version": device.driver_version,
                "opencl_c_version": device.opencl_c_version,
                "available": device.available, "compiler_available": device.compiler_available,
                "device_type": "gpu", "address_bits": device.address_bits,
                "global_memory_capacity_bytes": device.global_memory_bytes,
                "maximum_allocation_bytes": device.maximum_allocation_bytes,
                "local_memory_bytes": device.local_memory_bytes, "extensions": device.extensions,
                "memory_budget_bytes": null, "backend_ready": false,
                "packed_swift_qualified": false
            })).collect::<Vec<_>>()
        })).collect();
        Ok(
            json!({"schema_version": 1, "kind": "runner-native-opencl-observations",
            "status": "observed", "source": "system-opencl-loader",
            "architecture": std::env::consts::ARCH, "platforms": platforms,
            "loader": loader, "execution_policy": "headless-worker",
            "qualification": "unqualified", "backend_ready": false, "error": null}),
        )
    }

    pub(super) fn run() -> crate::Result<Value> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let scratch = std::env::temp_dir().join(format!(
            "SwiftBonsai-OpenClInventory-{}-{stamp}",
            std::process::id()
        ));
        validate_external_path(&scratch).map_err(|error| error.to_string())?;
        fs::create_dir(&scratch).map_err(|error| error.to_string())?;
        let request = scratch.join("request.json");
        fs::write(&request, b"{}").map_err(|error| error.to_string())?;
        let spec = ProcessSpec {
            executable: std::env::current_exe().map_err(|error| error.to_string())?,
            arguments: vec![
                "host-opencl-query".into(),
                "--request".into(),
                request.clone().into_os_string(),
            ],
            working_directory: scratch.clone(),
            stdout: scratch.join("stdout.json"),
            stderr: scratch.join("stderr.txt"),
            memory_limit_bytes: 512 * 1024 * 1024,
            active_process_limit: 1,
        };
        let report =
            crate::runtime_probe::run(&spec, 1024 * 1024, "Native OpenCL inventory", || Ok(()))
                .and_then(|output| {
                    serde_json::from_str::<Value>(&output.stdout).map_err(|error| error.to_string())
                });
        match report {
            Ok(report) if checked_report(&report) => {
                for path in [&spec.stdout, &spec.stderr, &request] {
                    fs::remove_file(path).map_err(|error| error.to_string())?;
                }
                fs::remove_dir(&scratch).map_err(|error| error.to_string())?;
                Ok(report)
            }
            failed => {
                let error = match failed {
                    Err(error) if error.len() <= 1024 && !error.chars().any(char::is_control) => {
                        error
                    }
                    _ => "Native OpenCL query returned an invalid report.".into(),
                };
                let mut report = unknown();
                report["probe_error"] = json!(error);
                report["evidence_directory"] = json!(scratch);
                Ok(report)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencl_inventory_rejects_overrides_and_unqualified_claim_changes() {
        for request in [
            json!(null),
            json!([]),
            json!({"driver":"custom.dll"}),
            json!({"compile":true}),
            json!({"device":0}),
        ] {
            assert!(run(&request).is_err());
            assert!(query(&request).is_err());
        }
        assert_eq!(unknown()["platforms"], Value::Null);
        assert_eq!(unknown()["backend_ready"], false);
        assert!(!checked_report(&unknown()));
        let mut report = unknown();
        report["architecture"] = json!("aarch64");
        report["status"] = json!("observed");
        report["error"] = Value::Null;
        report["loader"] = json!({"file":"OpenCL.dll", "sha256": "a".repeat(64), "bytes": 535_112, "signature_verified":false, "dependencies_verified":false});
        report["platforms"] = json!([]);
        assert!(checked_report(&report));
        for (key, value) in [
            ("backend_ready", json!(true)),
            ("qualification", json!("qualified")),
            ("execution_policy", json!("tool")),
            ("platforms", json!([{}])),
        ] {
            let mut invalid = report.clone();
            invalid[key] = value;
            assert!(!checked_report(&invalid));
        }
        report["platforms"] = json!(
            (0..9)
                .map(|_| json!({"name":"p", "vendor":"v", "version":"v", "devices":[]}))
                .collect::<Vec<_>>()
        );
        assert!(!checked_report(&report));
        report["platforms"] = json!([{"name":"p", "vendor":"v", "version":"v", "devices":[{}]}]);
        assert!(!checked_report(&report));
        let device = json!({"name":"gpu", "vendor":"vendor", "vendor_id":1, "version":"OpenCL 3.0", "driver_version":"1", "opencl_c_version":"OpenCL C 1.2", "available":true, "compiler_available":true, "device_type":"gpu", "address_bits":64, "global_memory_capacity_bytes":1024, "maximum_allocation_bytes":512, "local_memory_bytes":256, "extensions":["cl_khr_fp16"], "memory_budget_bytes":null, "backend_ready":false, "packed_swift_qualified":false});
        report["platforms"][0]["devices"] = json!([device]);
        assert!(checked_report(&report));
        report["platforms"][0]["devices"][0]["packed_swift_qualified"] = json!(true);
        assert!(!checked_report(&report));
        report["platforms"][0]["devices"] = json!(vec![device; 17]);
        assert!(!checked_report(&report));
    }
}
