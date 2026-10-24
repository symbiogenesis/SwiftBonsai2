//! Complete Windows Job supervision for bounded completion and synthetic probes.

#[cfg(windows)]
mod capture;
#[cfg(windows)]
mod timing;
#[cfg(windows)]
mod windows;

/// Runs one explicit bounded operation, retaining verified inputs and process ownership.
///
/// The request is a versioned JSON object. Native stdout and stderr remain in
/// external evidence files; the returned object contains accounting only.
/// Successful CI-only samples additionally publish a strict UTF-8 capture record
/// and diagnostics before their completed supervision receipt becomes visible.
///
/// # Errors
/// Rejects malformed requests, unsafe paths, changed artifacts, caller death,
/// admission/resource/output limits, failed native execution or evidence writes.
pub fn run(request: serde_json::Value) -> crate::Result<serde_json::Value> {
    #[cfg(windows)]
    {
        windows::run(request)
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err("Completion supervision requires Windows.".into())
    }
}

/// Returns an observation of currently available Windows physical memory.
///
/// # Errors
/// Returns unsupported-platform or failed native memory-status queries.
pub fn available_memory() -> crate::Result<serde_json::Value> {
    #[cfg(windows)]
    {
        bonsai_windows_process::available_memory_bytes()
            .map(|bytes| serde_json::json!({"AvailablePhysicalBytes": bytes}))
            .map_err(|error| format!("Physical memory query failed: {error}"))
    }
    #[cfg(not(windows))]
    {
        Err("Physical memory admission requires Windows.".into())
    }
}
