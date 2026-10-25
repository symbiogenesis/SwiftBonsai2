//! Nonlaunching server policy shared with the broker's fixed invocation.

/// Validate environment names or prepare the fixed, launch-disabled server plan.
///
/// Paths supplied by PowerShell have already passed its local-drive and external
/// storage checks. This operation performs no filesystem or process actions.
///
/// # Errors
/// Rejects malformed requests, inherited overrides, unsafe ports or profile drift.
pub fn run(request: serde_json::Value) -> crate::Result<serde_json::Value> {
    bonsai_cpu_policy::server::run(request)
}
