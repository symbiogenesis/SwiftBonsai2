//! Retained private credential reader; authentication material has no Debug view.

use std::{fs::File, io, path::Path};
#[cfg(windows)]
use std::{fs::OpenOptions, io::Read as _};

pub(super) struct Credential {
    key: [u8; 64],
    _reader: File,
}

impl Credential {
    #[cfg(windows)]
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
        bonsai_windows_credentials::validate_external_path(path)?;
        let mut reader = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .custom_flags(0x0020_0000)
            .open(path)?;
        let metadata = reader.metadata()?;
        if !metadata.is_file() || metadata.len() != 64 || metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other(
                "Use a regular protected speech credential file.",
            ));
        }
        let mut key = [0; 64];
        reader.read_exact(&mut key)?;
        if !key
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        {
            return Err(io::Error::other(
                "Use a lowercase hexadecimal speech credential.",
            ));
        }
        Ok(Self {
            key,
            _reader: reader,
        })
    }

    #[cfg(not(windows))]
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

impl Drop for Credential {
    fn drop(&mut self) {
        self.key.fill(0);
    }
}
