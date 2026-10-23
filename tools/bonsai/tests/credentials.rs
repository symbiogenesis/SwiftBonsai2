//! Public command shape checks before any credential filesystem operation.
#![forbid(unsafe_code)]

use bonsai_tools::credentials;
use serde_json::json;

#[test]
fn credential_command_rejects_positional_and_unknown_fields() {
    assert!(credentials::run(json!(["C:\\private"])).is_err());
    assert!(credentials::run(json!({"parentDirectory": ".", "reuse": true})).is_err());
    assert!(credentials::run(json!({})).is_err());
}
