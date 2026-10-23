//! Credential provisioning without exposing secret values in receipts.

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    parent_directory: String,
}

/// Creates two owner-private credentials and returns their paths only.
///
/// The underlying files remain after this operation. Retained read handles end
/// with this command; callers requiring lifetime protection must retain a
/// `bonsai_windows_credentials::PrivateCredentials` owner instead.
///
/// # Errors
/// Rejects malformed requests, unsupported hosts, unsafe parents, and failed
/// protected file creation. Existing credentials are never reused or replaced.
pub fn run(request: Value) -> crate::Result<Value> {
    if !request.is_object() {
        return Err("Credential request must be a JSON object.".into());
    }
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    create(&request.parent_directory)
}

#[cfg(windows)]
fn create(parent: &str) -> crate::Result<Value> {
    use bonsai_windows_credentials::PrivateCredentials;
    use std::path::Path;

    let credentials = PrivateCredentials::create(Path::new(parent))
        .map_err(|error| format!("Private credential creation failed: {error}"))?;
    Ok(serde_json::json!({
        "Status": "development-keys-created",
        "Directory": credentials.directory(),
        "FrontendKeyFile": credentials.frontend_path(),
        "BackendKeyFile": credentials.backend_path(),
        "Qualification": "owner-and-LocalSystem ACLs; same-user access and parent replacement remain possible; no daily isolation claim"
    }))
}

#[cfg(not(windows))]
fn create(_parent: &str) -> crate::Result<Value> {
    Err("Development key creation requires Windows ACL support.".into())
}
