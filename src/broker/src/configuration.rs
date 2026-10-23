//! Shared input syntax for explicit development configuration.

#![allow(
    clippy::redundant_pub_crate,
    reason = "Keep shared configuration helpers explicitly crate scoped"
)]

use std::ffi::OsString;

pub(super) fn decimal_u16(value: Option<OsString>) -> Option<u16> {
    let value = value?;
    let value = value.to_str()?;
    if value.is_empty() || value.len() > 5 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}
