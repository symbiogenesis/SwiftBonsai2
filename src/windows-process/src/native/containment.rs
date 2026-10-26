//! Exact-version MXC PSEC composition with the runner's atomic Windows Job.
//!
//! MXC 1.0.1's supported spawn facade cannot accept our Job or resource limits.
//! These explicitly pinned doc-hidden compatibility exports are intentionally
//! confined here. Review this boundary and its fixed `FlatBuffer` policy whenever
//! either dependency changes; never use SDK dispatch, audit or fallback tiers.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::windows::fs::MetadataExt as _;
use std::path::{Component, Path, PathBuf, Prefix};

use mxc_sdk::process_container_common::{
    ProcessSecurityEnvironment, ProcessSecurityEnvironmentApi,
    base_container_runner::BaseContainerRunner,
};
use mxc_sdk::process_security_environment_spec::process_security_environment_layout::{
    EndpointPolicyT, FilterAction, IngressPolicyT, NetworkPolicyT, ProcessSecurityEnvironmentT,
    SchemaVersionT, finish_process_security_environment_buffer,
};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW;

use crate::{ProcessSpec, SandboxPolicy, invalid, validate_path};

/// Owns only a native PSEC handle; creation and policy remain immutable thereafter.
pub(super) struct Environment(ProcessSecurityEnvironment);

#[allow(
    clippy::non_send_fields_in_send_ty,
    reason = "The upstream owner wraps a process-wide opaque Windows handle; the safety contract is documented here."
)]
// SAFETY: PSEC handles are process-wide native handles, not thread-affine COM
// objects. Moving the unique owner does not transfer or duplicate ownership.
// The upstream close function and resident system DLL remain live process-wide.
unsafe impl Send for Environment {}
// SAFETY: Shared access exposes only a borrowed, immutable handle for process
// creation. Native close needs exclusive ownership through Drop; it cannot race
// any safe borrow, and this wrapper never mutates the environment after creation.
unsafe impl Sync for Environment {}

impl Environment {
    pub(super) fn raw(&self) -> HANDLE {
        self.0.raw().0
    }

    pub(super) fn create(spec: &ProcessSpec, policy: &SandboxPolicy) -> io::Result<Self> {
        let readonly = validate_policy(spec, policy)?;
        let api = available_api()?;
        let bytes = policy_bytes(readonly, Some(path_text(&policy.writable_directory)?));
        api.create(&bytes, 0).map(Self).map_err(native_error)
    }
}

pub(super) fn available() -> io::Result<()> {
    let api = available_api()?;
    // API exports alone do not prove an enabled, usable native implementation.
    // Creating the same network/UI posture exercises actual PSEC policy admission
    // without a child, credentials, filesystem grants or host ACL changes.
    let environment = api
        .create(&policy_bytes(Vec::new(), None), 0)
        .map_err(native_error)?;
    drop(environment);
    Ok(())
}

fn available_api() -> io::Result<ProcessSecurityEnvironmentApi> {
    let api = ProcessSecurityEnvironmentApi::load().map_err(native_error)?;
    if !api.supports_version(1, 1).map_err(native_error)?
        || !BaseContainerRunner::supports_ingress_host_loopback_allow()
    {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "native sandbox requires PSEC 1.1 and host-loopback ingress support",
        ));
    }
    Ok(api)
}

fn native_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("native PSEC containment: {error}"),
    )
}

pub(super) fn child_environment(policy: &SandboxPolicy) -> io::Result<Vec<u16>> {
    let scratch = path_text(&policy.writable_directory)?;
    super::environment::build(
        std::env::vars_os(),
        &BTreeMap::from([
            ("TEMP".to_owned(), scratch.clone()),
            ("TMP".to_owned(), scratch),
        ]),
    )
}

fn validate_policy(spec: &ProcessSpec, policy: &SandboxPolicy) -> io::Result<Vec<String>> {
    if policy.readonly_paths.is_empty() || policy.readonly_paths.len() > 64 {
        return Err(invalid(
            "native sandbox requires 1..64 explicit read-only grants",
        ));
    }
    validate_local_path(&policy.writable_directory)?;
    if !policy.writable_directory.is_dir()
        || !same_path(&spec.working_directory, &policy.writable_directory)?
    {
        return Err(invalid(
            "sandbox working directory must be its existing private scratch",
        ));
    }
    for output in [&spec.stdout, &spec.stderr] {
        if within(output, &policy.writable_directory)? {
            return Err(invalid(
                "sandbox evidence must remain outside worker scratch",
            ));
        }
    }
    let mut readonly = runtime_paths()?;
    for path in &policy.readonly_paths {
        validate_local_path(path)?;
        if !path.is_file() {
            return Err(invalid(
                "sandbox read-only grants must name exact existing files",
            ));
        }
        if within(path, &policy.writable_directory)? || within(&policy.writable_directory, path)? {
            return Err(invalid(
                "sandbox read-only and writable grants must not overlap",
            ));
        }
        if let Some(profile) = std::env::var_os("USERPROFILE")
            && within(Path::new(&profile), path)?
        {
            return Err(invalid(
                "sandbox cannot grant an entire user profile or its ancestors",
            ));
        }
        readonly.push(path_text(path)?);
    }
    let mut executable_granted = false;
    for path in &policy.readonly_paths {
        if same_path(&spec.executable, path)? {
            executable_granted = true;
        }
    }
    if !executable_granted {
        return Err(invalid(
            "sandbox executable requires an explicit read-only grant",
        ));
    }
    for path in &readonly {
        if within(&policy.writable_directory, Path::new(path))? {
            return Err(invalid(
                "sandbox scratch cannot overlap an OS runtime grant",
            ));
        }
    }
    readonly.sort_unstable();
    readonly.dedup();
    Ok(readonly)
}

fn validate_local_path(path: &Path) -> io::Result<()> {
    validate_path(path)?;
    if !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)))
        || path.file_name().is_none()
        || path_text(path)?
            .get(2..)
            .is_some_and(|tail| tail.contains(':'))
    {
        return Err(invalid(
            "sandbox grants require non-root local paths without streams",
        ));
    }
    for ancestor in path.ancestors() {
        let metadata = std::fs::symlink_metadata(ancestor)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(invalid("sandbox paths must not contain reparse points"));
        }
    }
    Ok(())
}

fn path_text(path: &Path) -> io::Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("sandbox paths must be Unicode"))
}

fn same_path(left: &Path, right: &Path) -> io::Result<bool> {
    let left: Vec<u16> = left.as_os_str().encode_wide().collect();
    let right: Vec<u16> = right.as_os_str().encode_wide().collect();
    let left_len = i32::try_from(left.len()).map_err(|_| invalid("sandbox path is too long"))?;
    let right_len = i32::try_from(right.len()).map_err(|_| invalid("sandbox path is too long"))?;
    // SAFETY: Both terminated-independent slices are readable for their checked
    // UTF-16 lengths. The ordinal comparison retains no pointers or state.
    let compared =
        unsafe { CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) };
    if compared == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(compared == CSTR_EQUAL)
    }
}

fn within(path: &Path, root: &Path) -> io::Result<bool> {
    for ancestor in path.ancestors() {
        if same_path(ancestor, root)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn runtime_paths() -> io::Result<Vec<String>> {
    let mut buffer = vec![0_u16; 32_768];
    // SAFETY: The writable UTF-16 allocation has exactly the supplied capacity.
    let length = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), 32_768) };
    let length =
        usize::try_from(length).map_err(|_| invalid("Windows directory length overflow"))?;
    if length == 0 || length >= buffer.len() {
        return Err(io::Error::last_os_error());
    }
    let windows = PathBuf::from(OsString::from_wide(&buffer[..length]));
    validate_local_path(&windows)?;
    let mut paths = Vec::new();
    for name in ["System32", "WinSxS", "SysWOW64"] {
        let path = windows.join(name);
        if path.is_dir() {
            validate_local_path(&path)?;
            paths.push(path_text(&path)?);
        }
    }
    if paths.is_empty() {
        return Err(invalid("Windows runtime directories are unavailable"));
    }
    Ok(paths)
}

fn policy_bytes(readonly: Vec<String>, scratch: Option<String>) -> Vec<u8> {
    let mut egress = EndpointPolicyT::default();
    egress.default_action = FilterAction::deny;
    let mut ingress = IngressPolicyT::default();
    ingress.default_action = FilterAction::deny;
    ingress.host_loopback = FilterAction::allow;
    let mut network = NetworkPolicyT::default();
    network.egress = Some(Box::new(egress));
    network.ingress = Some(Box::new(ingress));
    // Host loopback is explicitly bidirectional and includes other host-owned
    // service addresses. Native tests show that neither removing the capability/
    // peer nor explicit all-address egress deny rules narrow that local scope.
    // No internet/private-network capability or external allow rule is granted.
    network.allowed_appcontainer_peer = Some("MXC-Loopback".to_owned());
    let mut spec = ProcessSecurityEnvironmentT::default();
    spec.version = SchemaVersionT { major: 1, minor: 1 };
    spec.capabilities = Some("networkLoopback".to_owned());
    spec.disallow_win32k_system_calls = true;
    // PSEC enforces these desktop restrictions without nesting a Job UI limit.
    spec.ui_restrictions = 0x03ff;
    spec.fs_read_only = (!readonly.is_empty()).then_some(readonly);
    spec.fs_read_write = scratch.map(|path| vec![path]);
    spec.network_policy = Some(Box::new(network));
    let mut builder = flatbuffers::FlatBufferBuilder::with_capacity(1024);
    let policy = spec.pack(&mut builder);
    finish_process_security_environment_buffer(&mut builder, policy);
    builder.finished_data().to_vec()
}
