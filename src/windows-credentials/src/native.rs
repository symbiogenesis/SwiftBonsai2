//! Audited native boundary: owned handles/LocalFree allocations, bounded token
//! and descriptor buffers, atomic ACL creation, CSPRNG, and volatile scrubbing.

use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::{self, NonNull};
use std::sync::atomic::{Ordering, compiler_fence};
use windows_sys::Win32::Foundation::{
    GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
};
use windows_sys::Win32::Security::{
    ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation, DACL_SECURITY_INFORMATION, EqualSid,
    GetAce, GetAclInformation, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
    GetSecurityDescriptorOwner, GetTokenInformation, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, GetDriveTypeW,
    OPEN_EXISTING, READ_CONTROL,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

pub(super) const REPARSE_ATTRIBUTE: u32 = FILE_ATTRIBUTE_REPARSE_POINT;

struct Allocation(NonNull<c_void>);

impl Allocation {
    fn from_raw(pointer: *mut c_void) -> io::Result<Self> {
        NonNull::new(pointer)
            .map(Self)
            .ok_or_else(io::Error::last_os_error)
    }

    const fn pointer(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

impl Drop for Allocation {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns the live LocalAlloc-compatible
        // allocation returned by the security conversion/query APIs.
        unsafe { LocalFree(self.pointer()) };
    }
}

pub(super) struct Security(Allocation);

impl Security {
    pub(super) fn new(directory: bool) -> io::Result<Self> {
        let owner = current_sid()?;
        let inheritance = if directory { "OICI" } else { "" };
        let system = if owner == "S-1-5-18" {
            String::new()
        } else {
            format!("(A;{inheritance};FA;;;SY)")
        };
        let sddl = format!("O:{owner}D:P(A;{inheritance};FA;;;{owner}){system}");
        let wide: Vec<_> = sddl.encode_utf16().chain([0]).collect();
        let mut descriptor = ptr::null_mut();
        // SAFETY: wide is NUL-terminated and lives for the call; descriptor is an
        // output slot. The returned LocalAlloc allocation is owned immediately.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &raw mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(Allocation::from_raw(descriptor)?))
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or_default(),
            lpSecurityDescriptor: self.0.pointer(),
            bInheritHandle: 0,
        }
    }

    pub(super) fn verify(&self, file: &File) -> io::Result<()> {
        let mut descriptor = ptr::null_mut();
        // SAFETY: file owns a live readable security handle; all optional outputs
        // are null and descriptor receives a separately allocated descriptor.
        let result = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &raw mut descriptor,
            )
        };
        if result != 0 {
            return Err(io::Error::from_raw_os_error(
                i32::try_from(result).unwrap_or(i32::MAX),
            ));
        }
        let actual = Allocation::from_raw(descriptor)?;
        verify_descriptors(self.0.pointer(), actual.pointer())
    }
}

fn current_sid() -> io::Result<String> {
    let mut raw_token = ptr::null_mut();
    // SAFETY: This API has no preconditions and returns a borrowed pseudo handle.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: the pseudo process handle is valid; raw_token is an output slot.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut raw_token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: OpenProcessToken succeeded and transfers ownership of this handle.
    let token = unsafe { OwnedHandle::from_raw_handle(raw_token) };
    let mut bytes = 0;
    // SAFETY: the null/zero buffer requests its required length; token is live.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &raw mut bytes,
        )
    };
    if bytes == 0 {
        return Err(io::Error::last_os_error());
    }
    let word_count = usize::try_from(bytes)
        .map_err(io::Error::other)?
        .div_ceil(size_of::<usize>());
    let mut buffer = vec![0_usize; word_count];
    // SAFETY: usize storage provides TOKEN_USER alignment and at least bytes
    // initialized bytes. The OS writes a token structure whose SID stays in it.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            bytes,
            &raw mut bytes,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if usize::try_from(bytes).map_err(io::Error::other)? < size_of::<TOKEN_USER>() {
        return Err(io::Error::other("Windows returned a truncated token user."));
    }
    // SAFETY: successful TokenUser query initialized an aligned TOKEN_USER.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut sid = ptr::null_mut();
    // SAFETY: the SID pointer refers into the still-live token information buffer.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &raw mut sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let allocation = Allocation::from_raw(sid.cast())?;
    let mut length = 0;
    // SAFETY: ConvertSidToStringSidW returns a NUL-terminated allocation; SID
    // strings are bounded by Windows' finite SID structure (well below 256).
    while length < 256 && unsafe { *sid.wrapping_add(length) } != 0 {
        length += 1;
    }
    if length == 256 {
        return Err(io::Error::other("Windows returned an invalid SID string."));
    }
    // SAFETY: the scanned prefix is inside the live NUL-terminated allocation.
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(sid, length) })
        .map_err(io::Error::other);
    drop(allocation);
    result
}

fn descriptor_parts(descriptor: PSECURITY_DESCRIPTOR) -> io::Result<(*mut c_void, *mut ACL)> {
    let mut owner = ptr::null_mut();
    let mut defaulted = 0;
    let mut present = 0;
    let mut dacl = ptr::null_mut();
    // SAFETY: only valid security descriptors returned by Windows enter this
    // function, and all output pointers reference live local slots.
    if unsafe { GetSecurityDescriptorOwner(descriptor, &raw mut owner, &raw mut defaulted) } == 0
        // SAFETY: same descriptor and valid local output slots as above.
        || unsafe { GetSecurityDescriptorDacl(descriptor, &raw mut present, &raw mut dacl, &raw mut defaulted) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if owner.is_null() || present == 0 || dacl.is_null() {
        return Err(io::Error::other(
            "Credential security lacks an explicit owner or DACL.",
        ));
    }
    Ok((owner, dacl))
}

fn ace_count(acl: *mut ACL) -> io::Result<u32> {
    let mut information = ACL_SIZE_INFORMATION::default();
    // SAFETY: acl came from a valid descriptor; information is correctly sized.
    if unsafe {
        GetAclInformation(
            acl,
            (&raw mut information).cast(),
            u32::try_from(size_of::<ACL_SIZE_INFORMATION>()).unwrap_or_default(),
            AclSizeInformation,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(information.AceCount)
}

fn ace_bytes(acl: *mut ACL, index: u32) -> io::Result<Vec<u8>> {
    let mut ace = ptr::null_mut();
    // SAFETY: index is bounded by the ACL's returned ACE count; acl is live.
    if unsafe { GetAce(acl, index, &raw mut ace) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: GetAce returns a complete ACE beginning with an aligned ACE_HEADER.
    let header = unsafe { &*ace.cast::<ACE_HEADER>() };
    // SAFETY: the valid ACE's header gives the complete in-ACL byte length.
    Ok(
        unsafe { std::slice::from_raw_parts(ace.cast::<u8>(), usize::from(header.AceSize)) }
            .to_vec(),
    )
}

fn verify_descriptors(
    expected: PSECURITY_DESCRIPTOR,
    actual: PSECURITY_DESCRIPTOR,
) -> io::Result<()> {
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: actual is a live OS-returned descriptor; output slots are valid.
    if unsafe { GetSecurityDescriptorControl(actual, &raw mut control, &raw mut revision) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let (expected_owner, expected_dacl) = descriptor_parts(expected)?;
    let (actual_owner, actual_dacl) = descriptor_parts(actual)?;
    // SAFETY: both owner SID pointers come from live valid descriptors.
    if control & SE_DACL_PROTECTED == 0 || unsafe { EqualSid(expected_owner, actual_owner) } == 0 {
        return Err(io::Error::other(
            "Credential owner or protected DACL differs from the requested security.",
        ));
    }
    let count = ace_count(expected_dacl)?;
    if count != ace_count(actual_dacl)? {
        return Err(io::Error::other(
            "Credential DACL contains unexpected access rules.",
        ));
    }
    for index in 0..count {
        if ace_bytes(expected_dacl, index)? != ace_bytes(actual_dacl, index)? {
            return Err(io::Error::other(
                "Credential DACL differs from the requested access rules.",
            ));
        }
    }
    Ok(())
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

pub(super) fn validate_local_drive(root: &Path) -> io::Result<()> {
    let root = wide(root);
    // SAFETY: root is a live NUL-terminated absolute drive root.
    // DRIVE_REMOVABLE=2, DRIVE_FIXED=3, DRIVE_RAMDISK=6 (WinBase.h).
    if matches!(unsafe { GetDriveTypeW(root.as_ptr()) }, 2 | 3 | 6) {
        Ok(())
    } else {
        Err(super::invalid(
            "Credential storage requires a recognized local disk.",
        ))
    }
}

pub(super) fn create_directory(path: &Path, security: &Security) -> io::Result<()> {
    let path = wide(path);
    let attributes = security.attributes();
    // SAFETY: both the NUL-terminated path and descriptor-backed attributes live
    // through the synchronous call. CreateDirectoryW fails if the name exists.
    if unsafe { CreateDirectoryW(path.as_ptr(), &raw const attributes) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn create_file(path: &Path, security: &Security) -> io::Result<File> {
    let path = wide(path);
    let attributes = security.attributes();
    // SAFETY: path and security descriptor live for this call; CREATE_NEW never
    // overwrites an object. The handle is not inherited and disallows deletion.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE | READ_CONTROL,
            FILE_SHARE_READ,
            &raw const attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    file_from_handle(handle)
}

pub(super) fn open_reader(path: &Path, directory: bool) -> io::Result<File> {
    let path = wide(path);
    let flags = FILE_FLAG_OPEN_REPARSE_POINT
        | if directory {
            FILE_FLAG_BACKUP_SEMANTICS
        } else {
            FILE_ATTRIBUTE_NORMAL
        };
    // SAFETY: path is live and NUL-terminated. No inheritable attributes are
    // supplied. Share-read denies subsequent write/delete opens while held.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | READ_CONTROL,
            FILE_SHARE_READ,
            ptr::null(),
            OPEN_EXISTING,
            flags,
            ptr::null_mut(),
        )
    };
    let file = file_from_handle(handle)?;
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(super::invalid("Credential object became a reparse point."));
    }
    Ok(file)
}

use std::os::windows::fs::MetadataExt;

fn file_from_handle(handle: *mut c_void) -> io::Result<File> {
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: only successful CreateFileW handles enter here, and ownership is
    // transferred exactly once to File, whose destructor closes it.
    Ok(unsafe { File::from_raw_handle(handle) })
}

pub(super) fn random(buffer: &mut [u8]) -> io::Result<()> {
    let length = u32::try_from(buffer.len()).map_err(io::Error::other)?;
    // SAFETY: buffer is live writable storage of exactly length bytes. A null
    // algorithm handle is required for the system-preferred CSPRNG flag.
    if unsafe {
        BCryptGenRandom(
            ptr::null_mut(),
            buffer.as_mut_ptr(),
            length,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    } < 0
    {
        return Err(io::Error::other(
            "Windows cryptographic random generation failed.",
        ));
    }
    Ok(())
}

pub(super) fn scrub(buffer: &mut [u8]) {
    for byte in buffer {
        // SAFETY: byte uniquely references valid writable storage. Volatile
        // stores plus the compiler fence prevent dead-store elimination.
        unsafe { ptr::write_volatile(byte, 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

#[cfg(test)]
pub(super) fn noninheritable(file: &File) -> io::Result<bool> {
    use windows_sys::Win32::Foundation::{GetHandleInformation, HANDLE_FLAG_INHERIT};
    let mut flags = 0;
    // SAFETY: file owns a live handle; flags is a valid output slot.
    if unsafe { GetHandleInformation(file.as_raw_handle(), &raw mut flags) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(flags & HANDLE_FLAG_INHERIT == 0)
}
