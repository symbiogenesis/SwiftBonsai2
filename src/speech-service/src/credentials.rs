//! Retained private credential reader; authentication material has no Debug view.

#[cfg(windows)]
pub(super) use bonsai_windows_credentials::RetainedCredential as Credential;

#[cfg(not(windows))]
use std::{fs::File, io, path::Path};

#[cfg(not(windows))]
pub(super) struct Credential {
    key: [u8; 64],
    _reader: File,
}

#[cfg(not(windows))]
impl Credential {
    pub(super) fn open(_path: &Path) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Protected companion credentials require Windows.",
        ))
    }

    pub(super) const fn bytes(&self) -> &[u8; 64] {
        &self.key
    }
}

#[cfg(not(windows))]
impl Drop for Credential {
    fn drop(&mut self) {
        self.key.fill(0);
    }
}
