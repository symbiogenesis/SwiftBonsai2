//! Bounded immutable observations used to partition host-native build caches.

use std::io;

#[cfg(any(target_arch = "aarch64", test))]
fn decode_registry_text(buffer: &[u16], bytes: u32) -> io::Result<String> {
    let units = usize::try_from(bytes / 2)
        .map_err(|_| io::Error::other("registry string length is not representable"))?;
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid CPU registry identity");
    if !bytes.is_multiple_of(2) || units < 2 || units > buffer.len() {
        return Err(invalid());
    }
    let text = &buffer[..units - 1];
    if buffer[units - 1] != 0 || text.contains(&0) {
        return Err(invalid());
    }
    let text = String::from_utf16(text).map_err(|_| invalid())?;
    if text.trim().is_empty()
        || text.trim().eq_ignore_ascii_case("unknown")
        || text.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    Ok(text)
}

#[cfg(target_arch = "aarch64")]
fn registry_text(value: &str) -> io::Result<String> {
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RRF_ZEROONFAILURE, RegGetValueW,
    };

    let value: Vec<_> = value.encode_utf16().chain(Some(0)).collect();
    let mut buffer = [0_u16; 512];
    let mut bytes = 1024_u32;
    // SAFETY: The fixed subkey and caller-owned value names are NUL-terminated.
    // The initialized aligned buffer has the advertised 1024-byte capacity and
    // remains live throughout the call. RegGetValueW retains no pointers/handles.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            windows_sys::core::w!("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0"),
            value.as_ptr(),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY | RRF_ZEROONFAILURE,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &raw mut bytes,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status.cast_signed()));
    }
    decode_registry_text(&buffer, bytes)
}

pub(super) fn query() -> io::Result<crate::NativeBuildHost> {
    #[cfg(target_arch = "aarch64")]
    {
        use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
        use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
        use windows_sys::Win32::System::Threading::IsProcessorFeaturePresent;

        let mut version = OSVERSIONINFOW {
            dwOSVersionInfoSize: u32::try_from(size_of::<OSVERSIONINFOW>())
                .map_err(|_| io::Error::other("invalid native version structure size"))?,
            ..OSVERSIONINFOW::default()
        };
        // SAFETY: The version structure is initialized with its exact native size,
        // writable for the duration of the call, and not retained by RtlGetVersion.
        let status = unsafe { RtlGetVersion(&raw mut version) };
        if status < 0 || version.dwMajorVersion == 0 || version.dwBuildNumber == 0 {
            return Err(io::Error::other(
                "running Windows kernel identity is unavailable",
            ));
        }
        // Probe all currently relevant Windows feature IDs rather than only the
        // three packed-kernel flags: GGML's native compile also checks SVE/SME,
        // and clang's native CPU selection can include FP16 and other features.
        let processor_features = (0..256)
            .filter(|feature| {
                // SAFETY: The API accepts a numeric feature ID, has no pointer
                // arguments, retains nothing and returns false for unknown IDs.
                unsafe { IsProcessorFeaturePresent(*feature) != 0 }
            })
            .collect();
        Ok(crate::NativeBuildHost {
            processor_name: registry_text("ProcessorNameString")?,
            processor_identifier: registry_text("Identifier")?,
            vendor_identifier: registry_text("VendorIdentifier")?,
            windows_version: [
                version.dwMajorVersion,
                version.dwMinorVersion,
                version.dwBuildNumber,
            ],
            processor_features,
        })
    }
    #[cfg(not(target_arch = "aarch64"))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native build-host observations require an ARM64 process",
    ))
}

#[cfg(test)]
mod tests {
    use super::decode_registry_text;

    #[test]
    fn cpu_registry_identity_rejects_malformed_or_missing_strings() -> std::io::Result<()> {
        assert_eq!(decode_registry_text(&[88, 50, 0], 6)?, "X2");
        for (buffer, bytes) in [
            (vec![0], 2),
            (vec![32, 0], 4),
            (vec![88, 0], 3),
            (vec![88], 4),
            (vec![88, 50], 4),
            (vec![88, 0, 0], 6),
            (vec![88, 10, 0], 6),
            (vec![0xd800, 0], 4),
        ] {
            assert!(decode_registry_text(&buffer, bytes).is_err());
        }
        assert!(decode_registry_text(&[85, 110, 107, 110, 111, 119, 110, 0], 16).is_err());
        Ok(())
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn native_build_host_observation_has_concrete_stable_facts() -> std::io::Result<()> {
        let host = super::query()?;
        assert_ne!(host.processor_name, "");
        assert_ne!(host.processor_identifier, "");
        assert_ne!(host.vendor_identifier, "");
        assert!(host.windows_version[0] >= 10);
        assert!(host.windows_version[2] > 0);
        assert!(
            host.processor_features
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
        assert_eq!(host, super::query()?);
        Ok(())
    }
}
