//! OS instruction observations, independent of topology and runtime dispatch.

use std::io;

use crate::Arm64Isa;

// Windows SDK 10.0.26100.0, winnt.h. The pinned windows-sys 0.61.2 predates
// this constant. This is NEON I8MM (66), not the separate SVE I8MM flag (57).
#[cfg(any(target_arch = "aarch64", test))]
const PF_ARM_V82_I8MM_INSTRUCTIONS_AVAILABLE: u32 = 66;

#[cfg(any(target_arch = "aarch64", test))]
fn observe(mut present: impl FnMut(u32) -> bool) -> Arm64Isa {
    use windows_sys::Win32::System::Threading::{
        PF_ARM_NEON_INSTRUCTIONS_AVAILABLE, PF_ARM_V82_DP_INSTRUCTIONS_AVAILABLE,
    };

    Arm64Isa {
        advsimd: present(PF_ARM_NEON_INSTRUCTIONS_AVAILABLE),
        dotprod: present(PF_ARM_V82_DP_INSTRUCTIONS_AVAILABLE),
        i8mm: present(PF_ARM_V82_I8MM_INSTRUCTIONS_AVAILABLE),
    }
}

#[cfg_attr(
    target_arch = "aarch64",
    expect(
        clippy::unnecessary_wraps,
        reason = "Other Windows process architectures must return Unsupported rather than native ARM64 ISA evidence."
    )
)]
pub(super) fn query() -> io::Result<Arm64Isa> {
    #[cfg(target_arch = "aarch64")]
    {
        use windows_sys::Win32::System::Threading::IsProcessorFeaturePresent;

        Ok(observe(|feature| {
            // SAFETY: The API takes a documented numeric feature ID, has no
            // pointer/handle arguments and retains nothing. It reports OS
            // availability without executing any optional instruction.
            unsafe { IsProcessorFeaturePresent(feature) != 0 }
        }))
    }
    #[cfg(not(target_arch = "aarch64"))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native ARM64 ISA observations require an ARM64 process",
    ))
}

#[cfg(test)]
mod tests {
    use super::observe;

    #[test]
    fn native_isa_queries_neon_dotprod_and_neon_i8mm_independently() {
        let mut queried = Vec::new();
        let observed = observe(|feature| {
            queried.push(feature);
            feature == 66
        });
        assert_eq!(queried, [19, 43, 66]);
        assert!(!observed.advsimd);
        assert!(!observed.dotprod);
        assert!(observed.i8mm);

        let unavailable = observe(|_| false);
        assert!(!unavailable.advsimd);
        assert!(!unavailable.dotprod);
        assert!(!unavailable.i8mm);
    }
}
