//! Full inherited UTF-16 environments with deterministic Windows-name overlays.

use std::os::windows::ffi::OsStrExt as _;
use std::{cmp::Ordering, collections::BTreeMap, ffi::OsString, io};

use windows_sys::Win32::Globalization::{
    CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN, CompareStringOrdinal,
};

use crate::invalid;

const PROTECTED: &[&str] = &[
    "SystemDrive",
    "SystemRoot",
    "windir",
    "ProgramData",
    "LOCALAPPDATA",
    "APPDATA",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "ALLUSERSPROFILE",
    "PUBLIC",
    "ComSpec",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "CommonProgramFiles",
    "CommonProgramFiles(x86)",
    "CommonProgramW6432",
];

fn compare(left: &[u16], right: &[u16]) -> io::Result<Ordering> {
    let left_len =
        i32::try_from(left.len()).map_err(|_| invalid("environment name is too long"))?;
    let right_len =
        i32::try_from(right.len()).map_err(|_| invalid("environment name is too long"))?;
    // SAFETY: Both slices remain readable for their checked UTF-16 lengths.
    // Explicit lengths preserve unpaired UTF-16 units; ordinal case comparison
    // is independent of locale and does not modify or retain either slice.
    match unsafe { CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) } {
        CSTR_LESS_THAN => Ok(Ordering::Less),
        CSTR_EQUAL => Ok(Ordering::Equal),
        CSTR_GREATER_THAN => Ok(Ordering::Greater),
        _ => Err(io::Error::last_os_error()),
    }
}

pub(super) fn build(
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
    overlay: &BTreeMap<String, String>,
) -> io::Result<Vec<u16>> {
    let mut entries: Vec<(Vec<u16>, Vec<u16>)> = inherited
        .into_iter()
        .map(|(name, value)| (name.encode_wide().collect(), value.encode_wide().collect()))
        .collect();
    let mut replaced: Vec<Vec<u16>> = Vec::new();
    for (name, value) in overlay {
        if name.is_empty() || name.contains(['=', '\0']) || value.contains('\0') {
            return Err(invalid("invalid child environment entry"));
        }
        let name: Vec<_> = name.encode_utf16().collect();
        for protected in PROTECTED {
            if compare(&name, &protected.encode_utf16().collect::<Vec<_>>())?.is_eq() {
                return Err(invalid(
                    "startup cannot replace a Windows system/profile environment variable",
                ));
            }
        }
        for prior in &replaced {
            if compare(&name, prior)?.is_eq() {
                return Err(invalid(
                    "duplicate case-insensitive child environment override",
                ));
            }
        }
        replaced.push(name.clone());
        let mut retained = Vec::with_capacity(entries.len() + 1);
        for entry in entries {
            if !compare(&name, &entry.0)?.is_eq() {
                retained.push(entry);
            }
        }
        retained.push((name, value.encode_utf16().collect()));
        entries = retained;
    }
    let mut comparison_error = None;
    entries.sort_by(|left, right| match compare(&left.0, &right.0) {
        Ok(order) => order,
        Err(error) => {
            comparison_error = Some(error);
            Ordering::Equal
        }
    });
    if let Some(error) = comparison_error {
        return Err(error);
    }
    let mut block = Vec::new();
    for (name, value) in entries {
        block.extend(name);
        block.push(61);
        block.extend(value);
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::ffi::OsStringExt as _;

    #[test]
    fn unicode_environment_preserves_profile_drive_entries_and_unpaired_units() -> io::Result<()> {
        let unusual = OsString::from_wide(&[0xd800, 88]);
        let inherited = vec![
            ("Path".into(), "original".into()),
            ("SystemRoot".into(), "C:\\Windows".into()),
            ("LOCALAPPDATA".into(), "C:\\profile\\local".into()),
            ("=C:".into(), "C:\\drive-working".into()),
            (unusual.clone(), unusual),
        ];
        let overlay = BTreeMap::from([
            ("pAtH".into(), "tool-path".into()),
            ("BONSAI_UNICODE".into(), "\u{30d1}\u{30b9}\u{1f332}".into()),
        ]);
        let block = build(inherited, &overlay)?;
        let entries: Vec<_> = block
            .split(|unit| *unit == 0)
            .filter(|entry| !entry.is_empty())
            .collect();
        for expected in [
            "pAtH=tool-path",
            "SystemRoot=C:\\Windows",
            "LOCALAPPDATA=C:\\profile\\local",
            "=C:=C:\\drive-working",
            "BONSAI_UNICODE=\u{30d1}\u{30b9}\u{1f332}",
        ] {
            assert!(entries.contains(&expected.encode_utf16().collect::<Vec<_>>().as_slice()));
        }
        assert!(
            !entries.contains(
                &"Path=original"
                    .encode_utf16()
                    .collect::<Vec<_>>()
                    .as_slice()
            )
        );
        assert!(entries.contains(&[0xd800, 88, 61, 0xd800, 88].as_slice()));
        assert!(block.ends_with(&[0, 0]));
        Ok(())
    }

    #[test]
    fn protected_duplicate_and_malformed_overrides_reject() {
        for (name, value) in [
            ("systemROOT", "C:\\unsafe"),
            ("AppData", "C:\\unsafe"),
            ("ProgramData", "C:\\unsafe"),
            ("USERPROFILE", "C:\\unsafe"),
            ("", "empty"),
            ("=C:", "drive override"),
            ("A=B", "equals"),
            ("NUL\0NAME", "nul"),
            ("NAME", "NUL\0VALUE"),
        ] {
            assert!(build(Vec::new(), &BTreeMap::from([(name.into(), value.into())])).is_err());
        }
        assert!(
            build(
                Vec::new(),
                &BTreeMap::from([
                    ("Path".into(), "first".into()),
                    ("PATH".into(), "second".into()),
                ])
            )
            .is_err()
        );
    }
}
