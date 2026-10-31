//! Windows 10+ process ownership using an unnamed, noninheritable Job object.
//!
//! A suspended root must enter its Job before its first instruction; dropping its
//! owner kills the complete tree. This enforces lifecycle and committed-memory
//! limits. Explicit sandboxed launches additionally require native Windows PSEC
//! filesystem, network and desktop containment; no weaker fallback is used.
#![cfg(windows)]

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::AsSocket;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

// All Win32 calls and their pointer/handle contracts are confined here.
#[allow(
    unsafe_code,
    reason = "Keep audited Win32 calls in this native boundary."
)]
#[expect(
    clippy::redundant_pub_crate,
    reason = "Explicit parent-only visibility keeps the audited boundary private."
)]
mod native;

/// Bounded failure context retained inside a Vulkan query's `io::Error`.
///
/// Downcast `io::Error::get_ref()` to this type to distinguish failed discovery
/// stages without parsing driver messages or inferring unsupported hardware.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VulkanQueryFailure {
    /// Fixed query-stage label; never a driver-provided string or file path.
    pub stage: &'static str,
    /// Raw `VkResult` when a Vulkan entry point returned one.
    pub vulkan_result: Option<i32>,
    /// Loader API version already observed before failure, if available.
    pub loader_api_version: Option<u32>,
    /// Requested application API version, if instance creation was attempted.
    pub application_api_version: Option<u32>,
    /// Optional current-process module observations around instance creation.
    pub instance_modules: Option<VulkanInstanceModules>,
}

impl std::fmt::Display for VulkanQueryFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Vulkan inventory failed at {}", self.stage)?;
        if let Some(result) = self.vulkan_result {
            write!(formatter, " (VkResult {result})")?;
        }
        Ok(())
    }
}

impl std::error::Error for VulkanQueryFailure {}

/// Model-free observations from the native system Vulkan loader.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VulkanInventory {
    /// Loader-reported packed Vulkan API version, without vendor decoding.
    pub loader_api_version: u32,
    /// Declared application/instance API version, capped at Vulkan 1.3.
    pub application_api_version: u32,
    /// Current-process module observations immediately around instance creation.
    pub instance_modules: VulkanInstanceModules,
    /// Enumerated adapters, including explicitly identified software devices.
    pub adapters: Vec<VulkanAdapter>,
}

/// Bounded module-name snapshots around the single Vulkan instance-creation call.
///
/// These are basenames only, without paths, hashes or signature claims. A missing
/// snapshot is unknown; a snapshot cannot identify attempted or unloaded DLLs,
/// distinguish files with equal basenames, or establish driver dependencies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VulkanInstanceModules {
    /// Sorted unique basenames before instance creation, or unknown on failure.
    pub before: Option<Vec<String>>,
    /// Sorted unique basenames after instance creation returns, or unknown.
    pub after: Option<Vec<String>>,
}

/// One Vulkan physical device; observations do not qualify an inference backend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VulkanAdapter {
    /// Driver-reported device name, not an independently established `SoC` SKU.
    pub name: String,
    /// Numeric Vulkan vendor identifier.
    pub vendor_id: u32,
    /// Numeric Vulkan device identifier.
    pub device_id: u32,
    /// Vendor-specific raw driver version; never decoded as a Vulkan version.
    pub driver_version: u32,
    /// Device-supported packed Vulkan API version.
    pub api_version: u32,
    /// Stable Vulkan device-type label.
    pub device_type: &'static str,
    /// Whether the API identifies an integrated or discrete hardware GPU.
    pub hardware_gpu: bool,
    /// Compute-capable queue families and their queue counts.
    pub compute_queues: Vec<(u32, u32)>,
    /// Advertised device extension names, sorted for stable inspection.
    pub extensions: Vec<String>,
    /// Heap sizes and device-local flags; these are capacities, not free budgets.
    pub memory_heaps: Vec<(u64, bool)>,
    /// Supported half-precision shader arithmetic, unknown before Vulkan 1.1 queries.
    pub shader_float16: Option<bool>,
    /// Supported integer-dot shader feature, unknown before Vulkan 1.1 queries.
    pub shader_integer_dot_product: Option<bool>,
    /// Subgroup width when Vulkan 1.1 properties are available.
    pub subgroup_size: Option<u32>,
    /// Raw subgroup operation flags when Vulkan 1.1 properties are available.
    pub subgroup_operations: Option<u32>,
    /// Raw shader-stage flags restricting the reported subgroup operations.
    pub subgroup_stages: Option<u32>,
}

/// Query the native system Vulkan loader without creating a logical device.
///
/// Requires a native ARM64 process and rejects Vulkan environment overrides.
/// Enumeration storage is bounded. Driver calls themselves have no timeout;
/// callers must isolate this operation in a deadline-owned child process, as
/// the maintenance `host-gpu` command does. No kernels, surfaces or windows are
/// created, and no device memory is allocated.
///
/// # Errors
/// Returns unsupported target, missing loader, malformed/oversized native data
/// or Vulkan failures. Installed loader/ICD trust is not established by a query.
/// Known stages carry a downcastable [`VulkanQueryFailure`]; a failed query does
/// not establish that the physical device lacks Vulkan support.
pub fn vulkan_inventory() -> io::Result<VulkanInventory> {
    native::vulkan_inventory()
}

/// Model-free observations from the absolute Windows system `OpenCL` loader.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenClInventory {
    /// Selected loader path; this is not a signature or dependency trust verdict.
    pub loader_path: PathBuf,
    /// Enumerated `OpenCL` platforms, each containing only GPU observations.
    pub platforms: Vec<OpenClPlatform>,
}

/// One system `OpenCL` platform, without a context, queue or executable program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenClPlatform {
    /// Driver-reported platform name.
    pub name: String,
    /// Driver-reported vendor name.
    pub vendor: String,
    /// `OpenCL` platform version string; not parsed as a kernel qualification.
    pub version: String,
    /// Enumerated GPU devices; CPU devices are not requested.
    pub devices: Vec<OpenClDevice>,
}

/// Read-only capabilities of one `OpenCL` GPU; no inference backend is qualified.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenClDevice {
    /// Driver-reported GPU name, not an independently verified silicon SKU.
    pub name: String,
    /// Driver-reported vendor name.
    pub vendor: String,
    /// Raw Khronos/PCI vendor identifier.
    pub vendor_id: u32,
    /// Driver-reported `OpenCL` device version.
    pub version: String,
    /// Driver-reported driver version.
    pub driver_version: String,
    /// `OpenCL` C language version reported by the compiler.
    pub opencl_c_version: String,
    /// Whether the driver reports the device currently available.
    pub available: bool,
    /// Whether the driver reports an online `OpenCL` C compiler.
    pub compiler_available: bool,
    /// Device address width in bits.
    pub address_bits: u32,
    /// Reported global memory capacity, not a live free-memory budget.
    pub global_memory_bytes: u64,
    /// Maximum single memory-object allocation, not guaranteed allocatable bytes.
    pub maximum_allocation_bytes: u64,
    /// Reported local memory capacity per compute unit.
    pub local_memory_bytes: u64,
    /// Advertised extension names in sorted order.
    pub extensions: Vec<String>,
}

/// Enumerate native ARM64 GPU capabilities using the absolute system `OpenCL` loader.
///
/// Queries create no context, queue, program, kernel or device allocation. Storage
/// is bounded to eight platforms, sixteen total GPUs and 16 KiB per string.
/// Driver calls need an externally owned deadline, as in `host-opencl`.
///
/// # Errors
/// Rejects emulated/non-ARM64 queries, `OpenCL` environment overrides, missing
/// loaders, failed calls and malformed/oversized driver data. Loading the system
/// implementation does not establish its signature or dependency provenance.
pub fn opencl_inventory() -> io::Result<OpenClInventory> {
    native::opencl_inventory()
}

/// One observed Windows CPU Set; processor indices are relative to `group`.
///
/// Efficiency and scheduling classes are raw operating-system observations,
/// not Snapdragon core names or evidence that a particular thread count wins.
/// Parking and allocation are changing state rather than compatibility identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Independent operating-system CPU Set flags are retained without inventing a policy state machine."
)]
pub struct CpuSet {
    /// Windows CPU Set identifier, valid for the current system observation.
    pub id: u32,
    /// Processor group containing this CPU Set's home processor.
    pub group: u16,
    /// Home logical processor within its group.
    pub logical_processor_index: u8,
    /// Group-relative core index; shared by hardware threads on one core.
    pub core_index: u8,
    /// Group-relative shared last-level cache index.
    pub last_level_cache_index: u8,
    /// Group-relative NUMA node index.
    pub numa_node_index: u8,
    /// Windows efficiency class; larger values identify faster, less efficient cores.
    pub efficiency_class: u8,
    /// Raw Windows scheduling class, without an inferred performance policy.
    pub scheduling_class: u8,
    /// Whether Windows currently reports the CPU Set as parked.
    pub parked: bool,
    /// Whether Windows reports the CPU Set as allocated.
    pub allocated: bool,
    /// Whether the CPU Set is allocated to this querying process.
    pub allocated_to_target_process: bool,
    /// Whether Windows reports the CPU Set as real-time.
    pub real_time: bool,
}

/// Reads bounded, current-process Windows CPU Set topology without changing affinity.
///
/// The returned records are sorted by processor group and logical processor.
/// Unknown future record types are skipped. Observations do not establish ISA,
/// runtime eligibility, process affinity restrictions or a qualified CPU profile.
/// See [the Windows API contract](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getsystemcpusetinformation).
///
/// # Errors
/// Returns native query failures, `Unsupported` when no known CPU Set records
/// exist, or `InvalidData` for malformed, duplicate or oversized records. A
/// changing required buffer size is retried at most three times.
pub fn cpu_sets() -> io::Result<Vec<CpuSet>> {
    native::cpu_sets()
}

/// Windows-reported availability of native ARM64 packed-kernel instructions.
///
/// A positive flag is an OS observation, not kernel correctness or performance
/// qualification. A negative flag means unsupported **or not detected** by the
/// Windows HAL; it does not prove that the silicon lacks the instruction set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Arm64Isa {
    /// `PF_ARM_NEON_INSTRUCTIONS_AVAILABLE` (19).
    pub advsimd: bool,
    /// `PF_ARM_V82_DP_INSTRUCTIONS_AVAILABLE` (43), including SDOT/UDOT.
    pub dotprod: bool,
    /// `PF_ARM_V82_I8MM_INSTRUCTIONS_AVAILABLE` (66), NEON integer matrix multiply.
    pub i8mm: bool,
}

/// Queries native ARM64 instruction availability without executing optional ISA.
///
/// Uses [the Windows processor-feature API](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-isprocessorfeaturepresent).
/// I8MM detection requires Windows 11 24H2 / Server 2025 or later; earlier HALs
/// can return false even on capable hardware. This does not change dispatch,
/// infer support from a processor name, or inspect an emulated process's ISA.
///
/// # Errors
/// Returns `Unsupported` outside an ARM64 build; emulated x86/x64 callers cannot
/// produce a native ARM64 eligibility observation.
pub fn arm64_isa() -> io::Result<Arm64Isa> {
    native::arm64_isa()
}

/// Stable observed host facts for partitioning host-native compiler outputs.
///
/// This is build-cache identity, not a runtime compatibility or ISA qualification
/// verdict. It deliberately excludes changing CPU availability and power state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeBuildHost {
    /// Bounded CPU description from the native hardware registry.
    pub processor_name: String,
    /// CPU architecture, family, model and revision reported by Windows.
    pub processor_identifier: String,
    /// CPU vendor reported by the native hardware registry.
    pub vendor_identifier: String,
    /// Actual running kernel major, minor and build numbers from `RtlGetVersion`.
    pub windows_version: [u32; 3],
    /// Sorted available Windows processor-feature IDs in the bounded range 0..256.
    ///
    /// Includes optional ARM features queried by the pinned native GGML build;
    /// negative observations mean unsupported or not detected by the Windows HAL.
    pub processor_features: Vec<u32>,
}

/// Observes native Windows ARM64 compiler-host facts without executing optional ISA.
///
/// Reads fixed hardware-registry values, the actual kernel version and bounded
/// `IsProcessorFeaturePresent` flags. No affinity, power or compiler policy changes.
///
/// # Errors
/// Returns `Unsupported` outside an ARM64 process, native query failures, or
/// `InvalidData` when mandatory identity values are absent, malformed or oversized.
pub fn native_build_host() -> io::Result<NativeBuildHost> {
    native::native_build_host()
}

/// Current Windows power-source and battery-saver snapshot.
///
/// These changing status values are neither energy telemetry nor immutable
/// hardware identity. Equal snapshots do not rule out intervening changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PowerStatus {
    /// Whether AC power is connected; `None` preserves Windows' unknown sentinel.
    pub ac_online: Option<bool>,
    /// Whether Windows currently reports battery saver as enabled.
    pub battery_saver: bool,
    /// Reported remaining battery percentage; `None` preserves unknown status.
    pub battery_percent: Option<u8>,
}

/// Reads a power-status snapshot without changing power policy or device settings.
///
/// See [the Windows power-status contract](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-system_power_status).
/// No energy consumption, thermal condition or effective performance mode is inferred.
///
/// # Errors
/// Returns native query failures or `InvalidData` for undocumented field values.
/// Documented unknown AC and battery-percentage sentinels remain `None`.
pub fn power_status() -> io::Result<PowerStatus> {
    native::power_status()
}

/// Checked Windows effective power mode, using notification contract version two.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectivePowerMode {
    /// Battery saver, also named energy saver high savings in newer SDKs.
    BatterySaver,
    /// Better battery, also named energy saver standard in newer SDKs.
    BetterBattery,
    /// Balanced performance.
    Balanced,
    /// High performance under the legacy high-performance overlay.
    HighPerformance,
    /// Maximum performance.
    MaxPerformance,
    /// Game-mode power policy.
    GameMode,
    /// Windows mixed-reality power policy.
    MixedReality,
}

impl EffectivePowerMode {
    /// Stable observation label; no energy or performance outcome is inferred.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BatterySaver => "battery-saver",
            Self::BetterBattery => "better-battery",
            Self::Balanced => "balanced",
            Self::HighPerformance => "high-performance",
            Self::MaxPerformance => "max-performance",
            Self::GameMode => "game-mode",
            Self::MixedReality => "mixed-reality",
        }
    }
}

/// Sequential observations of Windows power scheme and effective power mode.
///
/// These are changing policy conditions, not an immutable compatibility identity.
/// A scheme identifier does not capture edits to individual power settings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PowerPolicy {
    /// Active scheme GUID in lowercase canonical hyphenated notation.
    pub active_scheme: String,
    /// Latest observed mode from a temporary version-two notification registration.
    pub effective_mode: EffectivePowerMode,
}

/// Observes active power scheme and effective mode without changing settings.
///
/// Uses [PowerGetActiveScheme](https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powergetactivescheme)
/// and the initial current-value notification from
/// [PowerRegisterForEffectivePowerModeNotifications](https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powerregisterforeffectivepowermodenotifications).
/// The registration is removed before returning a successful snapshot. No polling,
/// user power-mode names, undocumented overlay API or energy estimate is used.
/// Waiting for the initial asynchronous notification is bounded to one second;
/// native cleanup then waits for the short callback to finish.
///
/// # Errors
/// Returns native failures, a missing initial notification, an unknown mode or
/// a malformed scheme. A failed notification cleanup disables future mode queries
/// in this process rather than accumulating registrations or reusing stale data.
pub fn power_policy() -> io::Result<PowerPolicy> {
    native::power_policy()
}

/// Explicit launch inputs; no shell or executable search path is used.
#[derive(Debug)]
pub struct ProcessSpec {
    /// Expanded absolute path of the executable.
    pub executable: PathBuf,
    /// Arguments encoded with the Windows C runtime argv convention.
    pub arguments: Vec<OsString>,
    /// Expanded absolute native working directory.
    pub working_directory: PathBuf,
    /// Expanded absolute path of a new output evidence file.
    pub stdout: PathBuf,
    /// Expanded absolute path of a new error evidence file.
    pub stderr: PathBuf,
    /// Maximum job-wide committed memory, including descendants.
    pub memory_limit_bytes: usize,
    /// Maximum simultaneously active processes, including the root.
    pub active_process_limit: u32,
}

/// Least-authority native worker filesystem grants.
///
/// Networking denies external ingress and egress, with a per-container host
/// loopback exception for the authenticated local HTTP service. Loopback access
/// is not limited to the broker. Required Windows runtime directories are added
/// by the native boundary; caller profile and data directories are not.
#[derive(Debug)]
pub struct SandboxPolicy {
    /// Exact existing model, credential, executable and runtime library files.
    pub readonly_paths: Vec<PathBuf>,
    /// Existing private worker scratch, also used as its working directory.
    /// PSEC may substitute its own private per-container TEMP/TMP directory.
    pub writable_directory: PathBuf,
}

/// Requires native PSEC 1.1, host-loopback ingress and security-environment creation.
///
/// This never changes host ACLs, firewall settings or installed Windows features.
/// It does not launch a process or establish actual-model compatibility.
/// Recognized ambient credential variable names reject with a value-free error;
/// other inherited entries remain intact, so unknown secrets remain a residual risk.
///
/// # Errors
/// Returns `Unsupported` when the native contract is unavailable, or a native
/// failure when the host cannot create the required restricted environment, or
/// `InvalidInput` for recognized ambient credential variables.
pub fn native_sandbox_available() -> io::Result<()> {
    validate_sandbox_environment()?;
    native::sandbox_available()
}

/// Startup compiler/tool inputs with a child-only inherited-environment overlay.
///
/// The unnamed Job owns the complete tree without applying model-specific
/// memory or process-count limits. Descendants cannot escape this ownership.
#[derive(Debug)]
pub struct ToolProcessSpec {
    /// Expanded absolute executable path; no executable search is performed.
    pub executable: PathBuf,
    /// Windows CRT arguments, or the explicit `cmd.exe /d /c` startup grammar.
    pub arguments: Vec<OsString>,
    /// Expanded absolute native working directory.
    pub working_directory: PathBuf,
    /// Expanded absolute path of a new standard-output evidence file.
    pub stdout: PathBuf,
    /// Expanded absolute path of a new standard-error evidence file.
    pub stderr: PathBuf,
    /// Child-only overrides; Windows system/profile variables cannot be replaced.
    pub environment: BTreeMap<String, String>,
}

/// Current root-process counters, distinct from the job-wide commit limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessMemory {
    /// Root-process private committed bytes.
    pub private_bytes: usize,
    /// Root-process resident working-set bytes.
    pub working_set_bytes: usize,
    /// Operating-system lifetime peak working set, including mapped residency.
    pub peak_working_set_bytes: usize,
}

/// Owns a complete native process tree; dropping it kills remaining processes.
pub struct OwnedProcess(native::NativeProcess);

/// Retained identity of the caller whose lifetime bounds a supervised operation.
pub struct ParentProcess(native::ParentProcess);

impl ParentProcess {
    /// Opens a noninheritable synchronization handle and checks creation identity.
    ///
    /// `creation_file_time` is the caller's Windows UTC creation FILETIME, in
    /// 100-nanosecond ticks since 1601. Checking it prevents accepting a reused PID.
    ///
    /// # Errors
    /// Rejects inaccessible, exited or mismatched processes, including PID zero.
    pub fn open(id: u32, creation_file_time: u64) -> io::Result<Self> {
        native::ParentProcess::open(id, creation_file_time).map(Self)
    }

    /// Confirms that the retained caller process has not exited.
    ///
    /// # Errors
    /// Returns a failed native wait or `BrokenPipe` after caller exit.
    pub fn ensure_alive(&self) -> io::Result<()> {
        self.0.ensure_alive()
    }
}

impl OwnedProcess {
    /// Starts a natively contained worker with verified Job and mitigation attributes.
    ///
    /// The exact worker retains the existing PID, memory, process-count and
    /// lifetime contracts. Writable application storage is private scratch;
    /// Windows also supplies its own per-container private temporary directory.
    /// OS/profile environment entries are inherited; TEMP/TMP request scratch
    /// but PSEC can replace them with that OS-managed temporary directory.
    /// All policy and capability failures reject without an uncontained retry.
    /// Native launches require a verified one-process Job before the first
    /// instruction. PSEC may omit ordinary creation attributes, so suspended
    /// Job assignment is repaired explicitly and native terminate-on-close is
    /// also required. There is no retry without containment or resource limits.
    ///
    /// # Errors
    /// Returns invalid launch/grant paths, reparse paths, overlapping read/write
    /// grants, unavailable native PSEC support or failed native creation.
    pub fn start_sandboxed(spec: &ProcessSpec, policy: &SandboxPolicy) -> io::Result<Self> {
        validate_process(spec)?;
        if spec.active_process_limit != 1 {
            return Err(invalid(
                "native sandbox requires a single-process worker policy",
            ));
        }
        validate_sandbox_environment()?;
        let command = command_line(&spec.executable, &spec.arguments)?;
        native::NativeProcess::start_sandboxed(spec, command, policy).map(Self)
    }

    /// Atomically owns a startup compiler/tool tree and then resumes its root.
    ///
    /// The complete Unicode environment is inherited and merged case-insensitively
    /// with the child-only overlay. OS/profile entries remain unchanged. Only the
    /// three evidence streams are inherited as handles, and the window is hidden.
    /// Closing the owner kills descendants even if the root already exited.
    ///
    /// # Errors
    /// Rejects invalid paths, malformed arguments or environment entries, protected
    /// system/profile overrides, existing evidence files, or native launch failures.
    pub fn start_tool(spec: &ToolProcessSpec) -> io::Result<Self> {
        for path in [
            &spec.executable,
            &spec.working_directory,
            &spec.stdout,
            &spec.stderr,
        ] {
            validate_path(path)?;
        }
        if !spec.executable.is_file() || !spec.working_directory.is_dir() {
            return Err(invalid("executable or working directory does not exist"));
        }
        let command = tool_command_line(&spec.executable, &spec.arguments)?;
        let environment = native::tool_environment(&spec.environment)?;
        native::NativeProcess::start_tool(spec, command, environment.as_deref()).map(Self)
    }

    /// Creates evidence files, atomically assigns a suspended root, then resumes.
    ///
    /// The full OS/profile environment is inherited without modification.
    /// Model/backend override variables are rejected before creating any files.
    /// Headless CPU workers deny Win32k calls, legacy extension points and
    /// remote/low-integrity executable images, with heap termination enabled.
    /// These creation-time mitigations preserve nested Jobs but do not restrict
    /// ordinary filesystem access, sockets or the caller's security identity.
    ///
    /// # Errors
    /// Rejects ambiguous paths, invalid limits/arguments, inherited backend
    /// overrides, existing evidence files, or any failed Windows operation,
    /// including unsupported required process-creation mitigations.
    pub fn start(spec: &ProcessSpec) -> io::Result<Self> {
        validate_process(spec)?;
        let command = command_line(&spec.executable, &spec.arguments)?;
        native::NativeProcess::start(spec, command).map(Self)
    }

    /// Starts a bounded, model-free `--version` or `--help` inspection.
    ///
    /// The caller verifies and retains the inspected executable and dependencies.
    /// Exactly one fixed inspection argument and a one-process limit are required.
    /// Memory, Job ownership, child-process denial, evidence and inherited-environment
    /// validation match worker launches. Heap termination, extension-point denial
    /// and image-load restrictions remain enabled. Win32k calls remain available
    /// so legacy runtime inspection can initialize imported Windows shell DLLs.
    /// This is an explicit inspection policy, never a fallback for model workers.
    ///
    /// # Errors
    /// Rejects arguments other than a single `--version` or `--help`, process
    /// limits other than one, and every invalid worker launch input or native failure.
    pub fn start_inspection(spec: &ProcessSpec) -> io::Result<Self> {
        validate_process(spec)?;
        if spec.active_process_limit != 1
            || !matches!(spec.arguments.as_slice(), [argument] if argument == "--version" || argument == "--help")
        {
            return Err(invalid(
                "inspection requires one --version or --help argument and a single-process limit",
            ));
        }
        let command = command_line(&spec.executable, &spec.arguments)?;
        native::NativeProcess::start_inspection(spec, command).map(Self)
    }

    /// Returns the root Windows process identifier.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.0.id()
    }

    /// Polls root completion without waiting; exit code 259 is handled correctly.
    ///
    /// # Errors
    /// Returns failed Windows wait/query operations.
    pub fn try_wait(&self) -> io::Result<Option<u32>> {
        self.0.try_wait()
    }

    /// Waits up to a bounded interval for root completion, waking immediately on exit.
    ///
    /// # Errors
    /// Rejects intervals at or above the native infinite-wait sentinel, or a failed wait.
    pub fn wait_for_exit(&self, timeout: Duration) -> io::Result<Option<u32>> {
        self.0.wait_for_exit(timeout)
    }

    /// Terminates the entire tree and waits within a positive timeout.
    ///
    /// # Errors
    /// Returns invalid zero timeouts, Windows failures, or `TimedOut` if active
    /// processes remain after the deadline. Dropping still closes the Job.
    pub fn stop(&self, timeout: Duration) -> io::Result<()> {
        self.0.stop(timeout)
    }

    /// Returns the observed peak job-wide committed bytes.
    /// Windows may include denied allocation requests in this diagnostic counter;
    /// it is not a measurement of successfully resident or accepted allocations.
    ///
    /// # Errors
    /// Returns failed Windows job-accounting operations.
    pub fn peak_job_memory_bytes(&self) -> io::Result<usize> {
        self.0.peak_job_memory_bytes()
    }

    /// Returns current root private commitment, working set and lifetime peak residency.
    ///
    /// # Errors
    /// Returns failed Windows process-memory queries.
    pub fn memory_usage(&self) -> io::Result<ProcessMemory> {
        self.0.memory_usage()
    }
}

fn validate_process(spec: &ProcessSpec) -> io::Result<()> {
    for path in [
        &spec.executable,
        &spec.working_directory,
        &spec.stdout,
        &spec.stderr,
    ] {
        validate_path(path)?;
    }
    if !spec.executable.is_file() || !spec.working_directory.is_dir() {
        return Err(invalid("executable or working directory does not exist"));
    }
    if spec.memory_limit_bytes == 0 || spec.active_process_limit == 0 {
        return Err(invalid(
            "job memory and active-process limits must be positive",
        ));
    }
    for (name, _) in std::env::vars_os() {
        if forbidden_environment_name(&name) {
            return Err(invalid("inherited environment contains a backend override"));
        }
    }
    Ok(())
}

/// Returns currently available physical memory; this is an observation only.
///
/// # Errors
/// Returns a failed Windows memory-status query.
pub fn available_memory_bytes() -> io::Result<u64> {
    native::available_memory_bytes()
}

/// Finds the process listening specifically on IPv4 `127.0.0.1:port`.
///
/// Wildcard and nonloopback listeners are not accepted. An ambiguous duplicate
/// listener is an error. This observation does not reserve or authenticate a port.
///
/// # Errors
/// Returns invalid port zero, malformed/oversized tables, ambiguity, or Windows
/// TCP-table query failures.
pub fn listener_process_id(port: u16) -> io::Result<Option<u32>> {
    if port == 0 {
        return Err(invalid("listener port must be nonzero"));
    }
    native::listener_process_id(port)
}

/// Requires exclusive Windows ownership of a socket's bound address.
///
/// Call this before binding a server socket or reserving a backend port. Without
/// exclusivity, some conflicting `SO_REUSEADDR` binds can share an
/// address and make delivery indeterminate. The socket stays borrowed;
/// this function does not bind it or take ownership of its handle.
///
/// # Errors
/// Returns the Winsock error if `SO_EXCLUSIVEADDRUSE` cannot be enabled.
pub fn require_exclusive_address(socket: &impl AsSocket) -> io::Result<()> {
    native::require_exclusive_address(socket)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn forbidden_environment_name(name: &OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_uppercase();
    ["LLAMA_", "LLAMACPP_", "GGML_", "AIP_", "MTMD_"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || ["HF_TOKEN", "HF_ENDPOINT", "MODEL_ENDPOINT"].contains(&name.as_str())
}

fn validate_sandbox_environment() -> io::Result<()> {
    if std::env::vars_os().any(|(name, _)| credential_environment_name(&name)) {
        return Err(invalid(
            "sandbox launch rejects inherited credential environment variables",
        ));
    }
    Ok(())
}

fn credential_environment_name(name: &OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_uppercase();
    [
        "_TOKEN",
        "_SECRET",
        "_PASSWORD",
        "_API_KEY",
        "_PRIVATE_KEY",
        "_ACCESS_KEY",
        "_CONNECTION_STRING",
        "_CONNECTIONSTRING",
        "_DATABASE_URL",
        "_REDIS_URL",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
        || [
            "TOKEN",
            "SECRET",
            "PASSWORD",
            "API_KEY",
            "PRIVATE_KEY",
            "ACCESS_KEY",
            "AWS_ACCESS_KEY_ID",
            "AZURE_OPENAI_KEY",
            "AZURE_STORAGE_KEY",
            "AZURE_STORAGE_CONNECTION_STRING",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "CONNECTION_STRING",
            "CONNECTIONSTRING",
            "DATABASE_URL",
            "REDIS_URL",
        ]
        .contains(&name.as_str())
        || [
            "CONNECTIONSTRINGS__",
            "SQLCONNSTR_",
            "SQLAZURECONNSTR_",
            "MYSQLCONNSTR_",
            "CUSTOMCONNSTR_",
            "POSTGRESQLCONNSTR_",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn validate_path(path: &Path) -> io::Result<()> {
    // Inspect raw segments as Path::components normalizes embedded `.` segments.
    let text: Vec<u16> = path.as_os_str().encode_wide().collect();
    let ambiguous = text.split(|unit| *unit == 92 || *unit == 47).any(|part| {
        part == [46]
            || part == [46, 46]
            || part.last().is_some_and(|last| *last == 46 || *last == 32)
    });
    if !path.is_absolute() || text.contains(&0) || text.contains(&37) || ambiguous {
        return Err(invalid(
            "native paths must be expanded absolute paths without ambiguous segments",
        ));
    }
    // Device/verbatim paths can bypass ordinary Win32 file-name semantics.
    if !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::UNC(_, _)))
    {
        return Err(invalid("native paths must use a drive or UNC prefix"));
    }
    for component in path.components() {
        if let Component::Normal(name) = component {
            // CreateFileW also opens DOS devices and NTFS alternate streams.
            // Evidence paths must name ordinary files, even when fully qualified.
            if name
                .encode_wide()
                .any(|unit| unit < 32 || matches!(unit, 34 | 42 | 58 | 60 | 62 | 63 | 124))
                || reserved_device(name)
            {
                return Err(invalid(
                    "native paths must name ordinary files and directories",
                ));
            }
        }
    }
    Ok(())
}

fn reserved_device(name: &OsStr) -> bool {
    let text = name.to_string_lossy();
    let base = text
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ');
    let base = base.to_ascii_uppercase();
    matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || (base.starts_with("COM") || base.starts_with("LPT"))
        && matches!(
            base.get(3..),
            Some(
                "1" | "2"
                    | "3"
                    | "4"
                    | "5"
                    | "6"
                    | "7"
                    | "8"
                    | "9"
                    | "\u{b9}"
                    | "\u{b2}"
                    | "\u{b3}"
            )
        )
}

fn command_line(executable: &Path, arguments: &[OsString]) -> io::Result<Vec<u16>> {
    let mut output = Vec::new();
    for value in
        std::iter::once(executable.as_os_str()).chain(arguments.iter().map(OsString::as_os_str))
    {
        if !output.is_empty() {
            output.push(32);
        }
        quote(value, &mut output)?;
        if output.len() >= 32_767 {
            return Err(invalid("Windows command line exceeds 32767 UTF-16 units"));
        }
    }
    output.push(0);
    Ok(output)
}

fn tool_command_line(executable: &Path, arguments: &[OsString]) -> io::Result<Vec<u16>> {
    if !executable
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("cmd.exe"))
    {
        return command_line(executable, arguments);
    }
    let [disable_auto_run, execute, script] = arguments else {
        return Err(invalid("unsupported command interpreter arguments"));
    };
    if disable_auto_run != "/d" || execute != "/c" {
        return Err(invalid("command interpreter startup must disable AutoRun"));
    }
    let script = script
        .to_str()
        .ok_or_else(|| invalid("command interpreter input must be Unicode"))?;
    if script.contains(['\0', '\r', '\n']) {
        return Err(invalid("unsupported command interpreter input"));
    }
    let command = if Path::new(script).is_absolute() {
        validate_path(Path::new(script))?;
        if script.contains('"') {
            return Err(invalid("unsupported environment batch path"));
        }
        format!(" /d /v:off /s /c \"\"{}\"\"", script.replace('/', "\\"))
    } else {
        format!(" /d /v:off /c {script}")
    };
    let mut output = Vec::new();
    quote(executable.as_os_str(), &mut output)?;
    output.extend(command.encode_utf16());
    if output.len() >= 32_767 {
        return Err(invalid("Windows command line exceeds 32767 UTF-16 units"));
    }
    output.push(0);
    Ok(output)
}

fn quote(value: &OsStr, output: &mut Vec<u16>) -> io::Result<()> {
    output.push(34);
    let mut slashes = 0;
    for unit in value.encode_wide() {
        match unit {
            0 => return Err(invalid("arguments cannot contain NUL")),
            92 => {
                slashes += 1;
                continue;
            }
            34 => output.extend(std::iter::repeat_n(92, slashes * 2 + 1)),
            _ => output.extend(std::iter::repeat_n(92, slashes)),
        }
        output.push(unit);
        slashes = 0;
    }
    output.extend(std::iter::repeat_n(92, slashes * 2));
    output.push(34);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_argument_quoting() -> io::Result<()> {
        for (input, expected) in [
            ("", "\"\""),
            ("a b", "\"a b\""),
            ("a\\", "\"a\\\\\""),
            ("a\"b", "\"a\\\"b\""),
        ] {
            let mut output = Vec::new();
            quote(OsStr::new(input), &mut output)?;
            assert_eq!(String::from_utf16_lossy(&output), expected);
        }
        assert!(
            command_line(
                Path::new("C:\\probe.exe"),
                &[OsString::from("x".repeat(32_767))]
            )
            .is_err()
        );
        assert!(command_line(Path::new("C:\\probe.exe"), &[OsString::from("a\0b")]).is_err());
        Ok(())
    }

    #[test]
    fn path_and_environment_policy() {
        for path in [
            "relative",
            "C:relative",
            "C:\\a\\..\\b",
            "C:\\a\\.\\b",
            "C:\\a.\\b",
            "C:\\%TEMP%\\b",
            "\\\\?\\C:\\a",
        ] {
            assert!(validate_path(Path::new(path)).is_err(), "{path}");
        }
        assert!(validate_path(Path::new("C:\\native cwd\\probe.exe")).is_ok());
        for name in ["llama_arg", "GGML_CUDA", "HF_TOKEN", "model_endpoint"] {
            assert!(forbidden_environment_name(OsStr::new(name)));
        }
        for name in [
            "SystemDrive",
            "SystemRoot",
            "windir",
            "ProgramData",
            "LOCALAPPDATA",
            "APPDATA",
            "PATH",
            "HF_HOME",
        ] {
            assert!(!forbidden_environment_name(OsStr::new(name)));
        }
    }

    #[test]
    fn native_paths_reject_devices_streams_and_reserved_characters() {
        for name in [
            "NUL",
            "nul.txt",
            "NUL .txt",
            "Con",
            "PRN.log",
            "AUX",
            "CONIN$",
            "CONOUT$",
            "COM1",
            "com9.txt",
            "LPT1",
            "lpt9.log",
            "COM\u{b9}",
            "com\u{b2}.txt",
            "COM\u{b3}",
            "LPT\u{b9}",
            "lpt\u{b2}.txt",
            "LPT\u{b3}",
            "file.log:stream",
            "file.log::$DATA",
            "wild?card",
            "wild*card",
            "quoted\"name",
            "pipe|name",
            "less<name",
            "greater>name",
            "control\u{1f}name",
        ] {
            assert!(
                validate_path(&Path::new("C:\\evidence").join(name)).is_err(),
                "{name}"
            );
            assert!(
                validate_path(&Path::new("C:\\evidence").join(name).join("output.log")).is_err(),
                "{name}"
            );
        }
        for name in [
            "console.log",
            "nulled.txt",
            "COM10",
            "LPT10",
            ".config",
            "ordinary name",
        ] {
            assert!(
                validate_path(&Path::new("C:\\evidence").join(name)).is_ok(),
                "{name}"
            );
        }
    }
}
#[cfg(test)]
#[test]
fn sandbox_environment_classifies_credential_names_without_changing_environment() {
    for name in [
        "OPENAI_API_KEY",
        "openai_api_key",
        "ANTHROPIC_API_KEY",
        "AZURE_OPENAI_KEY",
        "AZURE_STORAGE_CONNECTION_STRING",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "HF_TOKEN",
        "service_TOKEN",
        "service_SECRET",
        "service_PASSWORD",
        "service_API_KEY",
        "service_PRIVATE_KEY",
        "service_ACCESS_KEY",
        "TOKEN",
        "PASSWORD",
        "GOOGLE_APPLICATION_CREDENTIALS",
        "DATABASE_URL",
        "REDIS_URL",
        "APP_DATABASE_URL",
        "APP_REDIS_URL",
        "APP_CONNECTION_STRING",
        "APP_CONNECTIONSTRING",
        "ConnectionStrings__DefaultConnection",
        "SQLCONNSTR_Default",
        "SQLAZURECONNSTR_Default",
        "MYSQLCONNSTR_Default",
        "CUSTOMCONNSTR_Default",
        "POSTGRESQLCONNSTR_Default",
    ] {
        assert!(credential_environment_name(OsStr::new(name)), "{name}");
    }
    for name in [
        "SystemDrive",
        "SystemRoot",
        "windir",
        "ProgramData",
        "LOCALAPPDATA",
        "APPDATA",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "PATH",
        "TEMP",
        "TMP",
        "ComSpec",
        "ProgramFiles",
        "HF_HOME",
        "TOKENIZERS_PARALLELISM",
        "PASSWORD_POLICY",
    ] {
        assert!(!credential_environment_name(OsStr::new(name)), "{name}");
    }
}
