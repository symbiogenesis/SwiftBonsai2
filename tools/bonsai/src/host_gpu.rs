//! Model-free system Vulkan inventory in a bounded, owned native child process.

use serde_json::{Value, json};

fn validate(request: &Value) -> crate::Result<()> {
    if request.as_object().is_some_and(serde_json::Map::is_empty) {
        Ok(())
    } else {
        Err("Native GPU inventory requires an empty request object.".into())
    }
}

fn unknown() -> Value {
    json!({"schema_version": 1, "kind": "runner-native-gpu-observations",
        "status": "unknown", "source": "system-vulkan-loader",
        "architecture": std::env::consts::ARCH, "loader_api_version": null,
        "application_api_version": null,
        "instance_modules": null,
        "required_execution_policy": "headless-worker", "failure": null,
        "adapters": null, "qualification": "unqualified",
        "backend_ready": false, "error": "native-vulkan-query-unavailable"})
}

#[cfg(windows)]
fn instance_modules(modules: &bonsai_windows_process::VulkanInstanceModules) -> Value {
    let newly_observed =
        modules
            .before
            .as_ref()
            .zip(modules.after.as_ref())
            .map(|(before, after)| {
                after
                    .iter()
                    .filter(|name| before.binary_search(name).is_err())
                    .collect::<Vec<_>>()
            });
    let status = match (&modules.before, &modules.after) {
        (Some(_), Some(_)) => "observed",
        (None, None) => "unknown",
        _ => "partial",
    };
    json!({
        "scope": "current-process-instance-creation",
        "status": status,
        "before": modules.before, "after": modules.after,
        "newly_observed": newly_observed,
        "file_identity_verified": false, "dependencies_verified": false,
    })
}

#[cfg(windows)]
fn failed_query(error: &std::io::Error) -> Value {
    let mut report = unknown();
    let failure = error
        .get_ref()
        .and_then(|error| error.downcast_ref::<bonsai_windows_process::VulkanQueryFailure>());
    report["failure"] = json!({
        "stage": failure.map_or("native-query", |failure| failure.stage),
        "vulkan_result": failure.and_then(|failure| failure.vulkan_result),
        "classification": "query-failed-device-support-unknown",
    });
    report["loader_api_version"] = json!(failure.and_then(|failure| failure.loader_api_version));
    report["application_api_version"] =
        json!(failure.and_then(|failure| failure.application_api_version));
    report["instance_modules"] = failure
        .and_then(|failure| failure.instance_modules.as_ref())
        .map_or(Value::Null, instance_modules);
    report
}

/// Query Vulkan adapters in a ten-second, 512 MiB, one-process owned Job.
///
/// Only an empty request is accepted. A missing/incompatible loader, driver
/// failure, deadline or unknown platform preserves an explicit unknown report.
/// No logical device, kernel, model, window or accelerator profile is created.
/// Native observations do not establish installed-driver trust or performance.
///
/// # Errors
/// Rejects nonempty/malformed requests and invalid temporary storage paths.
pub fn run(request: &Value) -> crate::Result<Value> {
    validate(request)?;
    #[cfg(windows)]
    {
        windows::run()
    }
    #[cfg(not(windows))]
    Ok(unknown())
}

/// Internal child entry point for the deadline-owned `host-gpu` operation.
///
/// Callers must use `host-gpu`; native driver calls here have no intrinsic
/// timeout. This is separate from model execution and never qualifies a GPU.
///
/// # Errors
/// Rejects nonempty requests. Failed native queries return an unknown report
/// retaining value-free stage and raw Vulkan result when available.
pub fn query(request: &Value) -> crate::Result<Value> {
    validate(request)?;
    #[cfg(windows)]
    {
        let inventory = match bonsai_windows_process::vulkan_inventory() {
            Ok(inventory) => inventory,
            Err(error) => return Ok(failed_query(&error)),
        };
        let adapters: Vec<_> = inventory.adapters.iter().map(|adapter| json!({
            "name": adapter.name, "vendor_id": adapter.vendor_id, "device_id": adapter.device_id,
            "driver_version_raw": adapter.driver_version, "api_version": adapter.api_version,
            "device_type": adapter.device_type, "hardware_gpu": adapter.hardware_gpu,
            "compute_queues": adapter.compute_queues.iter().map(|(index, count)| json!({"family_index": index, "queue_count": count})).collect::<Vec<_>>(),
            "extensions": adapter.extensions,
            "memory_heaps": adapter.memory_heaps.iter().map(|(bytes, local)| json!({"capacity_bytes": bytes, "device_local": local})).collect::<Vec<_>>(),
            "shader_float16": adapter.shader_float16,
            "shader_integer_dot_product": adapter.shader_integer_dot_product,
            "subgroup_size": adapter.subgroup_size, "subgroup_operations_raw": adapter.subgroup_operations,
            "subgroup_stages_raw": adapter.subgroup_stages,
            "memory_budget_bytes": null, "driver_signature": null, "cooperative_matrix": null,
            "backend_ready": false
        })).collect();
        Ok(
            json!({"schema_version": 1, "kind": "runner-native-gpu-observations",
            "status": "observed", "source": "system-vulkan-loader",
            "architecture": std::env::consts::ARCH,
            "required_execution_policy": "headless-worker", "failure": null,
            "loader_api_version": inventory.loader_api_version, "adapters": adapters,
            "application_api_version": inventory.application_api_version,
            "instance_modules": instance_modules(&inventory.instance_modules),
            "qualification": "unqualified", "backend_ready": false, "error": null}),
        )
    }
    #[cfg(not(windows))]
    Ok(unknown())
}

#[cfg(windows)]
mod windows {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use bonsai_artifacts::validate_external_path;
    use bonsai_windows_process::ProcessSpec;

    use super::{Value, json, unknown};

    pub(super) fn run() -> crate::Result<Value> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let scratch = std::env::temp_dir().join(format!(
            "SwiftBonsai-GpuInventory-{}-{stamp}",
            std::process::id()
        ));
        validate_external_path(&scratch).map_err(|error| error.to_string())?;
        fs::create_dir(&scratch).map_err(|error| error.to_string())?;
        let request = scratch.join("request.json");
        fs::write(&request, b"{}").map_err(|error| error.to_string())?;
        let spec = ProcessSpec {
            executable: std::env::current_exe().map_err(|error| error.to_string())?,
            arguments: vec![
                "host-gpu-query".into(),
                "--request".into(),
                request.into_os_string(),
            ],
            working_directory: scratch.clone(),
            stdout: scratch.join("stdout.json"),
            stderr: scratch.join("stderr.txt"),
            memory_limit_bytes: 512 * 1024 * 1024,
            active_process_limit: 1,
        };
        let report =
            crate::runtime_probe::run(&spec, 1024 * 1024, "Native GPU inventory", || Ok(()))
                .and_then(|output| {
                    serde_json::from_str::<Value>(&output.stdout).map_err(|error| error.to_string())
                });
        if let Ok(mut report) = report
            && report["schema_version"] == 1
            && report["kind"] == "runner-native-gpu-observations"
            && report["qualification"] == "unqualified"
            && report["backend_ready"] == false
            && report["required_execution_policy"] == "headless-worker"
            && (report["status"] == "observed" || report["status"] == "unknown")
        {
            report["execution_policy"] = json!("headless-worker");
            if report["status"] == "unknown" {
                // A completed discovery failure is evidence, not unsupported
                // hardware. Keep the exact bounded child report for review.
                report["evidence_directory"] = json!(scratch);
                return Ok(report);
            }
            // The owned probe has exited and its Job was reaped. Delete only
            // these exact files, without recursive removal or directory walking.
            for path in [&spec.stdout, &spec.stderr, &scratch.join("request.json")] {
                fs::remove_file(path).map_err(|error| error.to_string())?;
            }
            fs::remove_dir(&scratch).map_err(|error| error.to_string())?;
            Ok(report)
        } else {
            // Retain bounded failure diagnostics, including cases where native
            // teardown could not confirm completion, and expose their location.
            let mut report = unknown();
            report["failure"] = json!({
                "stage": "worker-supervision-or-report",
                "vulkan_result": null,
                "classification": "query-failed-device-support-unknown",
            });
            report["evidence_directory"] = json!(scratch);
            Ok(report)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_gpu_inventory_rejects_paths_layers_and_execution_overrides() {
        for request in [
            json!(null),
            json!([]),
            json!({"driver": "custom.dll"}),
            json!({"layers": []}),
            json!({"execute": true}),
        ] {
            assert!(run(&request).is_err());
            assert!(query(&request).is_err());
        }
        assert_eq!(unknown()["adapters"], Value::Null);
        assert_eq!(unknown()["backend_ready"], false);
    }

    #[cfg(windows)]
    #[test]
    fn native_gpu_failures_retain_stage_result_and_observed_api_without_driver_text() {
        let error = std::io::Error::other(bonsai_windows_process::VulkanQueryFailure {
            stage: "instance-creation",
            vulkan_result: Some(-9),
            loader_api_version: Some(4_206_893),
            application_api_version: Some(4_206_592),
            instance_modules: Some(bonsai_windows_process::VulkanInstanceModules {
                before: Some(vec!["vulkan-1.dll".into()]),
                after: Some(vec!["driver.dll".into(), "vulkan-1.dll".into()]),
            }),
        });
        let report = failed_query(&error);
        assert_eq!(report["status"], "unknown");
        assert_eq!(report["required_execution_policy"], "headless-worker");
        assert_eq!(report["failure"]["stage"], "instance-creation");
        assert_eq!(report["failure"]["vulkan_result"], -9);
        assert_eq!(report["loader_api_version"], 4_206_893);
        assert_eq!(report["application_api_version"], 4_206_592);
        assert_eq!(report["instance_modules"]["status"], "observed");
        assert_eq!(
            report["instance_modules"]["newly_observed"],
            json!(["driver.dll"])
        );
        assert_eq!(report["instance_modules"]["dependencies_verified"], false);
        assert!(report["adapters"].is_null());
        assert_eq!(report["backend_ready"], false);

        let report = failed_query(&std::io::Error::other("sensitive driver path"));
        assert_eq!(report["failure"]["stage"], "native-query");
        assert!(report["failure"]["vulkan_result"].is_null());
        assert!(report["loader_api_version"].is_null());
        assert!(report["instance_modules"].is_null());
        assert!(!report.to_string().contains("sensitive"));
    }

    #[cfg(windows)]
    #[test]
    fn native_gpu_module_delta_requires_both_snapshots() {
        for (before, after, status) in [
            (None, None, "unknown"),
            (Some(vec!["a.dll".into()]), None, "partial"),
            (None, Some(vec!["a.dll".into()]), "partial"),
        ] {
            let report =
                instance_modules(&bonsai_windows_process::VulkanInstanceModules { before, after });
            assert_eq!(report["status"], status);
            assert!(report["newly_observed"].is_null());
            assert_eq!(report["file_identity_verified"], false);
        }
    }
}
