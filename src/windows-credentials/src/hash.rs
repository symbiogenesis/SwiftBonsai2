//! Streaming SHA-256 through Windows' CPU-dispatched CNG implementation.

use std::{ffi::c_void, io, ptr, ptr::NonNull};

use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_SHA256_ALG_HANDLE, BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData,
};

/// A streaming SHA-256 hash using the Windows CNG provider's CPU dispatch.
///
/// Owns one non-reusable hash handle. No input pointer survives an update, and
/// finishing consumes the state, preventing further updates or finalization.
pub struct Sha256(NonNull<c_void>);

impl Sha256 {
    /// Create an empty SHA-256 state without opening an algorithm provider.
    ///
    /// # Errors
    /// Returns an unavailable provider or native allocation failure.
    pub fn new() -> io::Result<Self> {
        let mut handle = ptr::null_mut();
        // SAFETY: the Windows 10 SHA-256 pseudo-handle needs no close. CNG
        // allocates its own hash storage when the object pointer/size are null/0.
        // The writable output slot is live and no secret or reusable flag is set.
        let status = unsafe {
            BCryptCreateHash(
                BCRYPT_SHA256_ALG_HANDLE,
                &raw mut handle,
                ptr::null_mut(),
                0,
                ptr::null(),
                0,
                0,
            )
        };
        check(status)?;
        NonNull::new(handle)
            .map(Self)
            .ok_or_else(|| io::Error::other("Windows returned an invalid SHA-256 state."))
    }

    /// Append exactly the supplied bytes to this hash.
    ///
    /// # Errors
    /// Rejects an input exceeding the native length bound or a provider failure.
    pub fn update(&mut self, bytes: &[u8]) -> io::Result<()> {
        let length = u32::try_from(bytes.len()).map_err(io::Error::other)?;
        if bytes.is_empty() {
            return Ok(());
        }
        // SAFETY: this owner holds the live hash exclusively; the immutable
        // input has exactly length readable bytes and CNG retains no pointer.
        check(unsafe { BCryptHashData(self.0.as_ptr(), bytes.as_ptr(), length, 0) })
    }

    /// Consume this state and return its exact 32-byte SHA-256 digest.
    ///
    /// # Errors
    /// Returns a native finalization failure. The hash is destroyed on all paths.
    pub fn finish(self) -> io::Result<[u8; 32]> {
        let mut digest = [0; 32];
        // SAFETY: the live SHA-256 handle has not been finalized before. The
        // output is writable for exactly the algorithm's 32-byte digest size.
        check(unsafe { BCryptFinishHash(self.0.as_ptr(), digest.as_mut_ptr(), 32, 0) })?;
        Ok(digest)
    }
}

impl Drop for Sha256 {
    fn drop(&mut self) {
        // SAFETY: this owner uniquely holds the CNG-created hash and its
        // provider-allocated storage; destroy once after all synchronous calls.
        unsafe { BCryptDestroyHash(self.0.as_ptr()) };
    }
}

fn check(status: i32) -> io::Result<()> {
    if status >= 0 {
        Ok(())
    } else {
        Err(io::Error::other("Windows SHA-256 operation failed."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(parts: &[&[u8]]) -> io::Result<String> {
        let mut hash = Sha256::new()?;
        for part in parts {
            hash.update(part)?;
        }
        Ok(super::super::hex(&hash.finish()?))
    }

    #[test]
    fn cng_sha256_matches_known_empty_short_and_multiblock_vectors() -> io::Result<()> {
        assert_eq!(
            digest(&[])?,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            digest(&[b"", b"a", b"", b"bc", b""])?,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            digest(&[b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"])?,
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        let block = [b'a'; 1000];
        let mut hash = Sha256::new()?;
        for _ in 0..1000 {
            hash.update(&block)?;
        }
        assert_eq!(
            super::super::hex(&hash.finish()?),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
        Ok(())
    }
}
