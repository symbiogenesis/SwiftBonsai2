//! Current power-status observations, separate from energy measurements.

use std::io;

use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

use crate::PowerStatus;

pub(super) fn query() -> io::Result<PowerStatus> {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: The synchronous API writes to one initialized, correctly aligned
    // native status value and retains no pointer. No process/device policy is set.
    if unsafe { GetSystemPowerStatus(&raw mut status) } == 0 {
        return Err(io::Error::last_os_error());
    }
    decode(
        status.ACLineStatus,
        status.BatteryLifePercent,
        status.SystemStatusFlag,
    )
}

fn decode(ac: u8, percent: u8, saver: u8) -> io::Result<PowerStatus> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid Windows power status");
    let ac_online = match ac {
        0 => Some(false),
        1 => Some(true),
        255 => None,
        _ => return Err(invalid()),
    };
    let battery_percent = match percent {
        0..=100 => Some(percent),
        255 => None,
        _ => return Err(invalid()),
    };
    let battery_saver = match saver {
        0 => false,
        1 => true,
        _ => return Err(invalid()),
    };
    Ok(PowerStatus {
        ac_online,
        battery_saver,
        battery_percent,
    })
}

#[cfg(test)]
mod tests {
    use super::decode;
    use crate::PowerStatus;
    use std::io;

    #[test]
    fn power_status_preserves_unknowns_and_rejects_undocumented_values() -> io::Result<()> {
        assert_eq!(
            decode(255, 255, 0)?,
            PowerStatus {
                ac_online: None,
                battery_saver: false,
                battery_percent: None,
            }
        );
        for (ac, online) in [(0, false), (1, true)] {
            for percent in [0, 100] {
                for (saver, enabled) in [(0, false), (1, true)] {
                    assert_eq!(
                        decode(ac, percent, saver)?,
                        PowerStatus {
                            ac_online: Some(online),
                            battery_saver: enabled,
                            battery_percent: Some(percent),
                        }
                    );
                }
            }
        }
        for invalid in 2..=254 {
            assert_eq!(
                decode(invalid, 100, 0).err().map(|error| error.kind()),
                Some(io::ErrorKind::InvalidData)
            );
        }
        for invalid in 101..=254 {
            assert_eq!(
                decode(1, invalid, 0).err().map(|error| error.kind()),
                Some(io::ErrorKind::InvalidData)
            );
        }
        for invalid in 2..=255 {
            assert_eq!(
                decode(1, 100, invalid).err().map(|error| error.kind()),
                Some(io::ErrorKind::InvalidData)
            );
        }
        Ok(())
    }

    #[test]
    fn power_status_queries_current_windows_host_without_changing_policy() -> io::Result<()> {
        let status = crate::power_status()?;
        assert!(status.battery_percent.is_none_or(|percent| percent <= 100));
        Ok(())
    }
}
