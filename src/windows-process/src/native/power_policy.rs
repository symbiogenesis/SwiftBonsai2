//! Read-only power-policy observations through documented Windows APIs.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;
use std::{io, ptr};

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::System::Power::{
    EFFECTIVE_POWER_MODE, EFFECTIVE_POWER_MODE_V2, EffectivePowerModeBalanced as BALANCED,
    EffectivePowerModeBatterySaver as BATTERY_SAVER,
    EffectivePowerModeBetterBattery as BETTER_BATTERY, EffectivePowerModeGameMode as GAME_MODE,
    EffectivePowerModeHighPerformance as HIGH_PERFORMANCE,
    EffectivePowerModeMaxPerformance as MAX_PERFORMANCE,
    EffectivePowerModeMixedReality as MIXED_REALITY, PowerGetActiveScheme,
    PowerRegisterForEffectivePowerModeNotifications,
    PowerUnregisterFromEffectivePowerModeNotifications,
};
use windows_sys::core::GUID;

use crate::{EffectivePowerMode, PowerPolicy};

// Serialize registrations so a missing initial notification cannot reuse a
// previous query's mode. The callback's only state has process lifetime, even
// if Windows fails to unregister. Such a failure disables subsequent queries.
static NOTIFICATIONS: Notifications = Notifications::new();

struct Notifications {
    query: Mutex<()>,
    ready: Mutex<()>,
    notification: Condvar,
    mode: AtomicI32,
    cleanup_failed: AtomicBool,
}

impl Notifications {
    const fn new() -> Self {
        Self {
            query: Mutex::new(()),
            ready: Mutex::new(()),
            notification: Condvar::new(),
            mode: AtomicI32::new(-1),
            cleanup_failed: AtomicBool::new(false),
        }
    }

    fn observe<H>(
        &self,
        register: impl FnOnce() -> io::Result<Option<H>>,
        unregister: impl FnOnce(H) -> io::Result<()>,
        timeout: Duration,
    ) -> io::Result<EffectivePowerMode> {
        let _guard = self
            .query
            .lock()
            .map_err(|_| io::Error::other("power-mode query lock is poisoned"))?;
        if self.cleanup_failed.load(Ordering::Acquire) {
            return Err(io::Error::other("power-mode notification cleanup failed"));
        }
        self.mode.store(-1, Ordering::Release);
        let Some(registration) = register()? else {
            self.cleanup_failed.store(true, Ordering::Release);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "power-mode registration returned no handle",
            ));
        };
        let observed = self.wait_for_notification(timeout);
        if let Err(error) = unregister(registration) {
            self.cleanup_failed.store(true, Ordering::Release);
            return Err(error);
        }
        observed?;
        mode(self.mode.load(Ordering::Acquire))
    }

    fn wait_for_notification(&self, timeout: Duration) -> io::Result<()> {
        let (guard, _) = self
            .notification
            .wait_timeout_while(
                self.ready
                    .lock()
                    .map_err(|_| io::Error::other("power-mode notification lock is poisoned"))?,
                timeout,
                |()| self.mode.load(Ordering::Acquire) == -1,
            )
            .map_err(|_| io::Error::other("power-mode notification wait is poisoned"))?;
        let missing = self.mode.load(Ordering::Acquire) == -1;
        drop(guard);
        if missing {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "initial power-mode notification did not arrive within its bound",
            ));
        }
        Ok(())
    }

    fn notify(&self, mode: EFFECTIVE_POWER_MODE) {
        // Holding the separate readiness lock closes the check/wait race.
        // The callback never attempts to acquire the serialized query lock.
        if let Ok(_guard) = self.ready.lock() {
            self.mode.store(mode, Ordering::Release);
            self.notification.notify_all();
        }
    }
}

unsafe extern "system" fn notified(mode: EFFECTIVE_POWER_MODE, _context: *const c_void) {
    NOTIFICATIONS.notify(mode);
}

fn mode(value: EFFECTIVE_POWER_MODE) -> io::Result<EffectivePowerMode> {
    match value {
        BATTERY_SAVER => Ok(EffectivePowerMode::BatterySaver),
        BETTER_BATTERY => Ok(EffectivePowerMode::BetterBattery),
        BALANCED => Ok(EffectivePowerMode::Balanced),
        HIGH_PERFORMANCE => Ok(EffectivePowerMode::HighPerformance),
        MAX_PERFORMANCE => Ok(EffectivePowerMode::MaxPerformance),
        GAME_MODE => Ok(EffectivePowerMode::GameMode),
        MIXED_REALITY => Ok(EffectivePowerMode::MixedReality),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "effective power mode is missing or unknown",
        )),
    }
}

fn effective_mode() -> io::Result<EffectivePowerMode> {
    NOTIFICATIONS.observe(register, unregister, Duration::from_secs(1))
}

fn register() -> io::Result<Option<ptr::NonNull<c_void>>> {
    let mut registration = ptr::null_mut();
    // SAFETY: The ABI-matching callback touches only process-lifetime synchronized
    // state, never unwinds and dereferences no context. The output handle is
    // writable and retained through unregister; version two is documented.
    let result = unsafe {
        PowerRegisterForEffectivePowerModeNotifications(
            EFFECTIVE_POWER_MODE_V2,
            Some(notified),
            ptr::null(),
            &raw mut registration,
        )
    };
    if result != 0 {
        return Err(io::Error::other(format!(
            "register power-mode notifications failed: {result:#x}"
        )));
    }
    Ok(ptr::NonNull::new(registration))
}

fn unregister(registration: ptr::NonNull<c_void>) -> io::Result<()> {
    // SAFETY: This exact successful registration is unregistered once, outside
    // the callback. Windows waits for callbacks before success. Static callback
    // state remains valid on failure; no later query will register again.
    let result =
        unsafe { PowerUnregisterFromEffectivePowerModeNotifications(registration.as_ptr()) };
    if result != 0 {
        return Err(io::Error::other(format!(
            "unregister power-mode notifications failed: {result:#x}"
        )));
    }
    Ok(())
}

fn format_guid(guid: GUID) -> String {
    let tail = u64::from_be_bytes(guid.data4);
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        guid.data1,
        guid.data2,
        guid.data3,
        tail >> 48,
        tail & 0x0000_ffff_ffff_ffff,
    )
}

fn active_scheme() -> io::Result<String> {
    let mut allocation = ptr::null_mut();
    // SAFETY: The reserved registry key is null; the output pointer is writable.
    // Success transfers one LocalFree-owned GUID allocation to the caller.
    let result = unsafe { PowerGetActiveScheme(ptr::null_mut(), &raw mut allocation) };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result.cast_signed()));
    }
    if allocation.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "active power scheme returned no GUID",
        ));
    }
    // SAFETY: A successful non-null API allocation contains one initialized,
    // aligned GUID. Copy it while its LocalFree-owned storage remains live.
    let guid = unsafe { *allocation };
    // SAFETY: This pointer is the original allocation returned by the API and
    // is released exactly once with its documented allocator after the copy.
    if !unsafe { LocalFree(allocation.cast()) }.is_null() {
        return Err(io::Error::last_os_error());
    }
    if guid.data1 == 0 && guid.data2 == 0 && guid.data3 == 0 && guid.data4 == [0; 8] {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "active power scheme returned a zero GUID",
        ));
    }
    Ok(format_guid(guid))
}

pub(super) fn query() -> io::Result<PowerPolicy> {
    Ok(PowerPolicy {
        active_scheme: active_scheme()?,
        effective_mode: effective_mode()?,
    })
}

#[cfg(test)]
mod tests {
    use super::{Notifications, format_guid, mode};
    use crate::EffectivePowerMode;
    use std::io;
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    use windows_sys::core::GUID;

    #[test]
    fn power_policy_decodes_all_version_two_modes_and_rejects_unknowns() -> io::Result<()> {
        for (value, expected) in [
            (0, EffectivePowerMode::BatterySaver),
            (1, EffectivePowerMode::BetterBattery),
            (2, EffectivePowerMode::Balanced),
            (3, EffectivePowerMode::HighPerformance),
            (4, EffectivePowerMode::MaxPerformance),
            (5, EffectivePowerMode::GameMode),
            (6, EffectivePowerMode::MixedReality),
        ] {
            assert_eq!(mode(value)?, expected);
        }
        for value in [i32::MIN, -1, 7, i32::MAX] {
            assert_eq!(
                mode(value).err().map(|error| error.kind()),
                Some(io::ErrorKind::InvalidData)
            );
        }
        assert_eq!(
            format_guid(GUID::from_u128(0x381b4222_f694_41f0_9685_ff5bb260df2e)),
            "381b4222-f694-41f0-9685-ff5bb260df2e"
        );
        Ok(())
    }

    #[test]
    fn power_policy_queries_current_host_without_changing_settings() -> io::Result<()> {
        let policy = crate::power_policy()?;
        assert_eq!(policy.active_scheme.len(), 36);
        assert_ne!(policy.effective_mode.as_str(), "");
        Ok(())
    }

    #[test]
    fn power_policy_notification_lifecycle_rejects_missing_and_failed_observations()
    -> io::Result<()> {
        let state = Notifications::new();
        assert_eq!(
            state.observe(
                || {
                    state.mode.store(2, Ordering::Release);
                    Ok(Some(()))
                },
                |()| {
                    // The last callback completed during cleanup is authoritative.
                    state.mode.store(4, Ordering::Release);
                    Ok(())
                },
                Duration::ZERO,
            )?,
            EffectivePowerMode::MaxPerformance
        );
        // Resetting the sentinel prevents a zero-callback query using old data.
        assert!(
            state
                .observe(|| Ok(Some(())), |()| Ok(()), Duration::ZERO)
                .is_err()
        );
        assert!(
            state
                .observe(
                    || Err::<Option<()>, _>(io::Error::other("registration failure")),
                    |()| unreachable!(),
                    Duration::ZERO
                )
                .is_err()
        );
        assert!(
            state
                .observe(
                    || Ok(Some(())),
                    |()| Err(io::Error::other("cleanup failure")),
                    Duration::ZERO
                )
                .is_err()
        );
        assert!(
            state
                .observe(
                    || unreachable!("failed cleanup must prevent another registration"),
                    |()| Ok(()),
                    Duration::ZERO
                )
                .is_err()
        );
        let missing_handle = Notifications::new();
        assert!(
            missing_handle
                .observe(|| Ok(None::<()>), |()| unreachable!(), Duration::ZERO)
                .is_err()
        );
        assert!(
            missing_handle
                .observe(
                    || unreachable!("missing handle must prevent another registration"),
                    |()| Ok(()),
                    Duration::ZERO,
                )
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn power_policy_notification_wait_observes_async_initial_callback() -> io::Result<()> {
        let state = Notifications::new();
        std::thread::scope(|scope| {
            assert_eq!(
                state.observe(
                    || {
                        scope.spawn(|| state.notify(2));
                        Ok(Some(()))
                    },
                    |()| Ok(()),
                    Duration::from_secs(1),
                )?,
                EffectivePowerMode::Balanced
            );
            Ok(())
        })
    }
}
