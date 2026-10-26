//! Retained caller identity for bounded native startup operations.
use std::sync::OnceLock;

#[cfg(windows)]
use bonsai_windows_process::ParentProcess;

static CONTEXT: OnceLock<Context> = OnceLock::new();

struct Context {
    #[cfg(windows)]
    parent: Option<ParentProcess>,
}

impl Context {
    fn new(parent: Option<(u32, u64)>) -> crate::Result<Self> {
        #[cfg(windows)]
        {
            let parent = parent
                .map(|(id, creation)| ParentProcess::open(id, creation))
                .transpose()
                .map_err(|error| {
                    format!("Startup caller identity could not be verified: {error}")
                })?;
            Ok(Self { parent })
        }
        #[cfg(not(windows))]
        {
            if parent.is_some() {
                return Err("Startup caller identity requires Windows.".into());
            }
            Ok(Self {})
        }
    }

    fn checkpoint(&self) -> crate::Result<()> {
        #[cfg(windows)]
        if let Some(parent) = &self.parent {
            parent
                .ensure_alive()
                .map_err(|error| format!("Startup cancelled: {error}"))?;
        }
        Ok(())
    }
}

/// Retains the optional caller identity for this startup command.
///
/// The creation identity is a Windows UTC creation FILETIME, in 100-nanosecond
/// ticks since 1601. A retained noninheritable handle prevents PID reuse from
/// making an exited caller appear alive. Call this once before dispatching a
/// startup command; direct CLI callers may omit the parent identity.
///
/// # Errors
/// Rejects an inaccessible, exited or mismatched caller, unsupported caller
/// tracking on another platform, or repeated initialization.
pub fn initialize(parent: Option<(u32, u64)>) -> crate::Result<()> {
    CONTEXT
        .set(Context::new(parent)?)
        .map_err(|_| "Startup cancellation context was already initialized.".into())
}

pub(crate) fn checkpoint() -> crate::Result<()> {
    CONTEXT.get().map_or(Ok(()), Context::checkpoint)
}

#[cfg(test)]
mod tests {
    use super::Context;

    #[test]
    fn optional_caller_does_not_require_global_state() -> crate::Result<()> {
        let context = Context::new(None)?;
        context.checkpoint()?;
        context.checkpoint()
    }

    #[test]
    fn invalid_caller_identity_rejects() {
        assert!(Context::new(Some((0, 0))).is_err());
        assert!(Context::new(Some((std::process::id(), 1))).is_err());
    }
}
