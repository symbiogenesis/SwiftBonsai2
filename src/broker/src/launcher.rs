//! Explicit Windows development launch of a compiled, pinned CPU backend.
//!
//! This owns lifetime, credentials, artifact handles and resource monitoring.
//! It does not establish a restricted token, filesystem boundary or denied
//! egress. Same-user processes and administrators remain in the trust boundary.

use crate::{
    artifacts::VerifiedArtifacts,
    launch_policy::{LaunchPlan, LaunchProfile},
    runtime::{RunExit, RunReport, run},
    scheduler::MAX_WAITING,
    service::{LocalChatService, bound_loopback_socket},
};
use bonsai_windows_credentials::{PrivateCredentials, validate_external_path};
use bonsai_windows_process::{
    OwnedProcess, ProcessSpec, available_memory_bytes, listener_process_id,
};
use std::{
    cell::Cell,
    ffi::OsString,
    future::poll_fn,
    io,
    net::{Ipv4Addr, SocketAddrV4},
    path::PathBuf,
    pin::pin,
    task::Poll,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{interval, sleep, timeout},
};

/// Explicit acknowledgement is required even though the backend is owned.
pub const LAUNCH_USAGE: &str = concat!(
    "Windows CPU development launcher (daily isolation unqualified).\n",
    "bonsai-broker --launch-development --allow-unisolated-backend\n",
    "  --data-directory <existing-external-directory>\n",
    "  --evidence-parent <existing-external-directory>\n",
    "  --frontend-port <0-or-1024..65535> --backend-port <1024..65535>\n",
    "  [--profile <reference-cpu|swift-pq2-assessment>]\n",
    "  [--native-engine] (requires the pinned Prism inference release and swift-pq2-assessment)\n",
    "  [--use-abliterate-lora] (pinned adapter; request scale 0..2, default 1.3; Swift Prism CPU server only)\n",
    "  [--max-waiting <0..8>] [--check-only]\n",
    "Default: reference-cpu. Only compiled model/runtime pins and fixed CPU policies are accepted.\n",
    "Swift selects original PQ2 weights; the demo requests its pinned LoRA by default. Daily use remains unqualified.\n",
    "Creates fresh protected credentials; prints paths, never key values.\n",
);

const PHYSICAL_RESERVE: u64 = 4 * 1024 * 1024 * 1024;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(600);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const HEALTH_LIMIT: usize = 20 * 1024;

/// Value-free launcher failure, suitable for technical status without secrets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchError {
    /// Missing acknowledgement, repeated or unknown option, or invalid port.
    Arguments,
    /// Storage is not an existing external local directory without redirection.
    Path,
    /// The fixed launch policy or inherited environment is invalid.
    Policy,
    /// Locked model/runtime verification failed.
    Artifacts,
    /// The pinned abliteration adapter is missing, altered or unsafe to open.
    Adapter,
    /// Protected credential provisioning failed.
    Credentials,
    /// Physical memory admission or a running resource guard failed.
    Memory,
    /// The requested backend port is occupied by another process.
    Port,
    /// Atomic Job-owned process creation failed.
    Process,
    /// The owned backend exited, failed readiness, or lost its listener identity.
    Backend,
    /// Console cancellation occurred before readiness.
    Cancelled,
    /// Frontend policy validation or listener binding failed.
    Service,
    /// Termination did not confirm an empty owned Job within the cleanup limit.
    Stop,
}

/// Parsed explicit development launch; paths and credentials have no Debug output.
pub struct LaunchConfig {
    data: PathBuf,
    evidence_parent: PathBuf,
    frontend_port: u16,
    backend_port: u16,
    max_waiting: usize,
    check_only: bool,
    profile: LaunchProfile,
    native_engine: bool,
    use_abliterate_lora: bool,
}

/// Offline verification result; checking never creates credentials or a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchCheck {
    /// Whether the current physical-memory observation meets launch admission.
    pub memory_admitted: bool,
}

impl LaunchConfig {
    /// Parse the launcher arguments, including both mandatory acknowledgements.
    ///
    /// # Errors
    /// Rejects unknown/repeated options, nondecimal ports and missing opt-ins.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, LaunchError> {
        let mut args = args.into_iter();
        let (mut launch, mut unisolated) = (false, false);
        let mut check_only = false;
        let mut native_engine = false;
        let mut use_abliterate_lora = false;
        let mut profile = None;
        let (mut data, mut evidence_parent) = (None, None);
        let (mut frontend_port, mut backend_port, mut max_waiting) = (None, None, None);
        while let Some(argument) = args.next() {
            match argument.to_str() {
                Some("--launch-development") if !launch => launch = true,
                Some("--allow-unisolated-backend") if !unisolated => unisolated = true,
                Some("--check-only") if !check_only => check_only = true,
                Some("--native-engine") if !native_engine => {
                    native_engine = true;
                }
                Some("--use-abliterate-lora") if !use_abliterate_lora => {
                    use_abliterate_lora = true;
                }
                Some("--profile") if profile.is_none() => {
                    profile = Some(
                        args.next()
                            .and_then(|value| value.to_str().and_then(|name| name.parse().ok()))
                            .ok_or(LaunchError::Arguments)?,
                    );
                }
                Some("--data-directory") if data.is_none() => {
                    data = Some(PathBuf::from(args.next().ok_or(LaunchError::Arguments)?));
                }
                Some("--evidence-parent") if evidence_parent.is_none() => {
                    evidence_parent =
                        Some(PathBuf::from(args.next().ok_or(LaunchError::Arguments)?));
                }
                Some("--frontend-port") if frontend_port.is_none() => {
                    frontend_port = Some(number(args.next())?);
                }
                Some("--backend-port") if backend_port.is_none() => {
                    backend_port = Some(number(args.next())?);
                }
                Some("--max-waiting") if max_waiting.is_none() => {
                    max_waiting = Some(usize::from(number(args.next())?));
                }
                _ => return Err(LaunchError::Arguments),
            }
        }
        let frontend_port = frontend_port.ok_or(LaunchError::Arguments)?;
        let backend_port = backend_port.ok_or(LaunchError::Arguments)?;
        let max_waiting = max_waiting.unwrap_or(2);
        let profile = profile.unwrap_or_default();
        if !launch
            || !unisolated
            || (frontend_port != 0 && frontend_port < 1024)
            || backend_port < 1024
            || backend_port == frontend_port
            || max_waiting > MAX_WAITING
            || (native_engine && profile != LaunchProfile::SwiftPq2Assessment)
            || (use_abliterate_lora
                && (native_engine || profile != LaunchProfile::SwiftPq2Assessment))
        {
            return Err(LaunchError::Arguments);
        }
        Ok(Self {
            data: data.ok_or(LaunchError::Arguments)?,
            evidence_parent: evidence_parent.ok_or(LaunchError::Arguments)?,
            frontend_port,
            backend_port,
            max_waiting,
            check_only,
            profile,
            native_engine,
            use_abliterate_lora,
        })
    }

    /// Whether the command requests offline checks without credential creation or launch.
    #[must_use]
    pub const fn is_check_only(&self) -> bool {
        self.check_only
    }

    /// Compiled profile selected by the exact CLI name, defaulting to the reference.
    #[must_use]
    pub const fn profile(&self) -> LaunchProfile {
        self.profile
    }

    /// Whether the explicit native Swift child replaces the pinned Prism server.
    #[must_use]
    pub const fn uses_native_engine(&self) -> bool {
        self.native_engine
    }

    /// Verify the fixed policy and complete artifacts without creating state.
    ///
    /// # Errors
    /// Returns path, policy or artifact failures; low available memory is reported
    /// in the result rather than preventing this read-only verification.
    pub fn check(&self) -> Result<LaunchCheck, LaunchError> {
        self.validate_paths()?;
        let plan = LaunchPlan::for_profile_with_lora(
            &self.data,
            self.backend_port,
            &self.evidence_parent.join("not-created.key"),
            self.profile,
            self.use_abliterate_lora,
        )
        .map_err(|_| LaunchError::Policy)?;
        let _artifacts = self.verify_artifacts(&plan)?;
        let _adapter = Self::adapter_guard(&plan)?;
        let _native_executable = self.native_executable_guard()?;
        Ok(LaunchCheck {
            memory_admitted: available_memory_bytes().map_err(|_| LaunchError::Memory)?
                >= plan.required_available_bytes,
        })
    }

    fn adapter_guard(
        plan: &LaunchPlan,
    ) -> Result<Option<bonsai_artifacts::VerifiedFile>, LaunchError> {
        plan.adapter
            .as_ref()
            .map(|adapter| {
                bonsai_artifacts::VerifiedFile::verify(
                    &adapter.path,
                    adapter.bytes,
                    &adapter.sha256,
                )
                .map_err(|_| LaunchError::Adapter)
            })
            .transpose()
    }

    fn verify_artifacts(&self, plan: &LaunchPlan) -> Result<VerifiedArtifacts, LaunchError> {
        if self.native_engine {
            let artifacts = VerifiedArtifacts::verify_model_for_profile(&self.data, self.profile)
                .map_err(|_| LaunchError::Artifacts)?;
            if artifacts.model() != plan.model_path {
                return Err(LaunchError::Artifacts);
            }
            return Ok(artifacts);
        }
        let artifacts = VerifiedArtifacts::verify_for_profile(&self.data, self.profile)
            .map_err(|_| LaunchError::Artifacts)?;
        if artifacts.executable() != plan.executable
            || artifacts.model() != plan.model_path
            || artifacts.runtime_directory() != plan.runtime_directory
            || artifacts.archive_path() != plan.archive_path
        {
            return Err(LaunchError::Artifacts);
        }
        Ok(artifacts)
    }

    fn validate_paths(&self) -> Result<(), LaunchError> {
        for path in [&self.data, &self.evidence_parent] {
            validate_external_path(path).map_err(|_| LaunchError::Path)?;
            if !path.is_dir() {
                return Err(LaunchError::Path);
            }
        }
        Ok(())
    }

    fn native_executable_guard(
        &self,
    ) -> Result<Option<(PathBuf, bonsai_artifacts::VerifiedFile)>, LaunchError> {
        if self.native_engine {
            crate::inference::verify(&self.data).map(Some)
        } else {
            Ok(None)
        }
    }

    fn specification(
        &self,
        plan: LaunchPlan,
        credentials: &PrivateCredentials,
    ) -> Result<(ProcessSpec, Option<bonsai_artifacts::VerifiedFile>), LaunchError> {
        let native = self.native_executable_guard()?;
        let (executable, arguments, guard) = if let Some((executable, guard)) = native {
            let arguments = vec![
                OsString::from("--native-engine-child"),
                OsString::from("--model"),
                plan.model_path.into_os_string(),
                OsString::from("--port"),
                OsString::from(self.backend_port.to_string()),
                OsString::from("--key-file"),
                credentials.backend_path().as_os_str().to_owned(),
                OsString::from("--seed"),
                OsString::from("42"),
            ];
            (executable, arguments, Some(guard))
        } else {
            (plan.executable, plan.arguments, None)
        };
        Ok((
            ProcessSpec {
                executable,
                arguments,
                working_directory: credentials.directory().to_path_buf(),
                stdout: credentials.directory().join("backend.stdout"),
                stderr: credentials.directory().join("backend.stderr"),
                memory_limit_bytes: plan.memory_limit_bytes,
                active_process_limit: 1,
            },
            guard,
        ))
    }

    /// Verify pinned artifacts, create credentials, launch and await authenticated readiness.
    ///
    /// The caller supplies cancellation from before startup through serving. No
    /// frontend listener is created until readiness and PID checks pass. On any
    /// startup error or future drop the owned Job is killed. The old daily
    /// profile remains launch-disabled; this is a distinct development opt-in.
    ///
    /// # Errors
    /// Returns a value-free error; technical stdout/stderr remains in the new
    /// protected evidence directory. Failed evidence is retained for inspection.
    pub async fn start(
        self,
        shutdown: impl Future<Output = io::Result<()>>,
    ) -> Result<LaunchedService, LaunchError> {
        // Register cancellation before synchronous policy/artifact work. The CLI
        // retains this same future across startup and serving, so a signal in
        // the readiness-to-serving handoff remains observable.
        let mut shutdown = pin!(shutdown);
        if poll_fn(|cx| Poll::Ready(shutdown.as_mut().poll(cx).is_ready())).await {
            return Err(LaunchError::Cancelled);
        }
        if self.check_only {
            return Err(LaunchError::Arguments);
        }
        self.validate_paths()?;
        // Resolve policy before creating secrets or loading any native code.
        let pending_key = self.evidence_parent.join("not-created.key");
        let preliminary = LaunchPlan::for_profile_with_lora(
            &self.data,
            self.backend_port,
            &pending_key,
            self.profile,
            self.use_abliterate_lora,
        )
        .map_err(|_| LaunchError::Policy)?;
        admit(preliminary.required_available_bytes)?;
        let artifacts = self.verify_artifacts(&preliminary)?;
        let adapter = Self::adapter_guard(&preliminary)?;
        if poll_fn(|cx| Poll::Ready(shutdown.as_mut().poll(cx).is_ready())).await {
            return Err(LaunchError::Cancelled);
        }
        let credentials = PrivateCredentials::create(&self.evidence_parent)
            .map_err(|_| LaunchError::Credentials)?;
        let plan = LaunchPlan::for_profile_with_lora(
            &self.data,
            self.backend_port,
            credentials.backend_path(),
            self.profile,
            self.use_abliterate_lora,
        )
        .map_err(|_| LaunchError::Policy)?;
        // Reserve only as a collision check; PID checks close the release/bind
        // race without ever trusting a pre-existing server on the selected port.
        let reservation =
            bound_loopback_socket(self.backend_port).map_err(|_| LaunchError::Port)?;
        admit(plan.required_available_bytes)?;
        let (specification, native_executable) = self.specification(plan, &credentials)?;
        validate_external_path(&specification.executable).map_err(|_| LaunchError::Path)?;
        drop(reservation);
        let process = OwnedProcess::start(&specification).map_err(|_| LaunchError::Process)?;
        let startup = wait_ready(
            &process,
            self.backend_port,
            credentials.backend_key(),
            specification.memory_limit_bytes,
            shutdown,
        )
        .await;
        if let Err(error) = startup {
            process.stop(STOP_TIMEOUT).map_err(|_| LaunchError::Stop)?;
            return Err(error);
        }
        if self.native_engine
            && !native_identity(self.backend_port, credentials.backend_key(), process.id()).await
        {
            process.stop(STOP_TIMEOUT).map_err(|_| LaunchError::Stop)?;
            return Err(LaunchError::Backend);
        }
        let mut service = LocalChatService::bind(
            self.frontend_port,
            credentials.frontend_key(),
            self.backend_port,
            credentials.backend_key(),
            self.max_waiting,
        )
        .await
        .map_err(|_| LaunchError::Service)?;
        service.require_backend_process(process.id());
        if adapter.is_some() {
            service.enable_pinned_abliteration();
        }
        Ok(LaunchedService {
            service,
            process,
            credentials,
            _artifacts: artifacts,
            _adapter: adapter,
            _native_executable: native_executable,
            memory_limit_bytes: specification.memory_limit_bytes,
            backend_port: self.backend_port,
        })
    }
}

/// One ready development backend with retained artifact and credential handles.
pub struct LaunchedService {
    service: LocalChatService,
    process: OwnedProcess,
    credentials: PrivateCredentials,
    _artifacts: VerifiedArtifacts,
    _adapter: Option<bonsai_artifacts::VerifiedFile>,
    _native_executable: Option<bonsai_artifacts::VerifiedFile>,
    memory_limit_bytes: usize,
    backend_port: u16,
}

impl LaunchedService {
    /// Bound frontend port, available only after the owned backend is ready.
    pub const fn port(&self) -> u16 {
        self.service.port()
    }

    /// Protected evidence directory; contains credentials, so share only selected logs.
    pub fn evidence_directory(&self) -> &std::path::Path {
        self.credentials.directory()
    }

    /// Serve while observing process exit, listener PID and physical/process memory.
    ///
    /// Every exit closes admission, drains owned sockets, terminates the complete
    /// Job and confirms it is empty. Interrupted generation ends this instance;
    /// no automatic restart or admission reset is supplied.
    ///
    /// # Errors
    /// Returns resource/backend/cleanup failure after shutting down the service.
    #[allow(
        clippy::future_not_send,
        reason = "the bounded runner and shutdown may be local; no task is spawned"
    )]
    pub async fn run(
        self,
        shutdown: impl Future<Output = io::Result<()>>,
    ) -> Result<RunReport, LaunchError> {
        let failure = Cell::new(None);
        let mut shutdown = pin!(shutdown);
        let monitoring = async {
            let mut tick = interval(Duration::from_millis(250));
            loop {
                tick.tick().await;
                let status = guard(&self.process, self.memory_limit_bytes)
                    .and_then(|()| owner(&self.process, self.backend_port));
                if let Err(error) = status {
                    failure.set(Some(error));
                    return Err(io::Error::other("Owned backend guard failed."));
                }
            }
        };
        let mut monitoring = pin!(monitoring);
        let monitor = poll_fn(|cx| {
            if let Poll::Ready(result) = shutdown.as_mut().poll(cx) {
                return Poll::Ready(result);
            }
            monitoring.as_mut().poll(cx)
        });
        let report = run(&self.service, monitor).await;
        self.process
            .stop(STOP_TIMEOUT)
            .map_err(|_| LaunchError::Stop)?;
        if let Some(error) = failure.get() {
            return Err(error);
        }
        if report.exit == RunExit::ShutdownFailure {
            return Err(LaunchError::Cancelled);
        }
        Ok(report)
    }
}

fn number(value: Option<OsString>) -> Result<u16, LaunchError> {
    crate::configuration::decimal_u16(value).ok_or(LaunchError::Arguments)
}

fn admit(required: u64) -> Result<(), LaunchError> {
    if available_memory_bytes().map_err(|_| LaunchError::Memory)? < required {
        return Err(LaunchError::Memory);
    }
    Ok(())
}

fn guard(process: &OwnedProcess, limit: usize) -> Result<(), LaunchError> {
    if process
        .try_wait()
        .map_err(|_| LaunchError::Backend)?
        .is_some()
    {
        return Err(LaunchError::Backend);
    }
    let memory = process.memory_usage().map_err(|_| LaunchError::Memory)?;
    if memory.private_bytes > limit || memory.working_set_bytes > limit {
        return Err(LaunchError::Memory);
    }
    admit(PHYSICAL_RESERVE)
}

fn owner(process: &OwnedProcess, port: u16) -> Result<(), LaunchError> {
    if listener_process_id(port).map_err(|_| LaunchError::Backend)? != Some(process.id()) {
        return Err(LaunchError::Backend);
    }
    Ok(())
}

async fn wait_ready(
    process: &OwnedProcess,
    port: u16,
    key: &str,
    limit: usize,
    shutdown: impl Future<Output = io::Result<()>>,
) -> Result<(), LaunchError> {
    let startup = async {
        loop {
            guard(process, limit)?;
            match listener_process_id(port).map_err(|_| LaunchError::Backend)? {
                Some(pid) if pid != process.id() => return Err(LaunchError::Backend),
                Some(_)
                    if healthy(port, key, process.id()).await
                        && authenticated(port, key, process.id()).await =>
                {
                    owner(process, port)?;
                    guard(process, limit)?;
                    return Ok(());
                }
                _ => {}
            }
            sleep(Duration::from_millis(100)).await;
        }
    };
    let mut shutdown = pin!(shutdown);
    let mut startup = pin!(timeout(STARTUP_TIMEOUT, startup));
    poll_fn(|cx| {
        if shutdown.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(LaunchError::Cancelled));
        }
        startup
            .as_mut()
            .poll(cx)
            .map(|result| result.map_err(|_| LaunchError::Backend)?)
    })
    .await
}

async fn healthy(port: u16, key: &str, process_id: u32) -> bool {
    timeout(
        Duration::from_secs(1),
        health_exchange(port, key, process_id),
    )
    .await
    .is_ok_and(|result| result.is_ok())
}

async fn native_identity(port: u16, key: &str, process_id: u32) -> bool {
    timeout(Duration::from_secs(2), async {
        validate_native_identity(&exchange(port, "/v1/models", Some(key), process_id).await?)
    })
    .await
    .is_ok_and(|result| result.is_ok())
}

fn validate_native_identity(bytes: &[u8]) -> io::Result<()> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Models {
        data: Vec<crate::Object<NativeModel>>,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct NativeModel {
        id: String,
        object: String,
        model_sha256: String,
        template_sha256: String,
        runtime: String,
        context_tokens: usize,
        seed: u32,
    }
    let body = validate_response(bytes, 200)?;
    let crate::Object(models): crate::Object<Models> =
        serde_json::from_slice(body).map_err(io::Error::other)?;
    let [crate::Object(model)] = models.data.as_slice() else {
        return Err(io::Error::other("native model inventory mismatch"));
    };
    if model.id != "Swift-Bonsai-2-PQ2_0"
        || model.object != "model"
        || model.model_sha256 != crate::inference::MODEL_SHA256
        || model.template_sha256 != crate::inference::TEMPLATE_SHA256
        || model.runtime != "prism-inference"
        || model.context_tokens != crate::CONTEXT_TOKENS
        || model.seed != 42
    {
        return Err(io::Error::other("native model readiness identity mismatch"));
    }
    Ok(())
}

async fn health_exchange(port: u16, key: &str, process_id: u32) -> io::Result<()> {
    validate_health(&exchange(port, "/health", Some(key), process_id).await?)
}

async fn authenticated(port: u16, key: &str, process_id: u32) -> bool {
    timeout(Duration::from_secs(2), async {
        // /health is public in the pinned engine. A protected endpoint must
        // reject missing credentials and accept this newly generated key.
        let denied = exchange(port, "/v1/models", None, process_id).await?;
        validate_response(&denied, 401)?;
        let allowed = exchange(port, "/v1/models", Some(key), process_id).await?;
        let body = validate_response(&allowed, 200)?;
        let models: serde_json::Value = serde_json::from_slice(body).map_err(io::Error::other)?;
        if !models.get("data").is_some_and(serde_json::Value::is_array) {
            return Err(io::Error::other("Backend authentication probe failed."));
        }
        Ok::<(), io::Error>(())
    })
    .await
    .is_ok_and(|result| result.is_ok())
}

async fn exchange(
    port: u16,
    path: &str,
    key: Option<&str>,
    process_id: u32,
) -> io::Result<Vec<u8>> {
    let check_owner = || {
        if listener_process_id(port)? != Some(process_id) {
            return Err(io::Error::other("Readiness listener owner changed."));
        }
        Ok(())
    };
    check_owner()?;
    let mut stream = TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).await?;
    check_owner()?;
    let head = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n");
    stream.write_all(head.as_bytes()).await?;
    if let Some(key) = key {
        stream.write_all(b"Authorization: Bearer ").await?;
        stream.write_all(key.as_bytes()).await?;
        stream.write_all(b"\r\n").await?;
    }
    stream.write_all(b"Connection: close\r\n\r\n").await?;
    let mut bytes = Vec::with_capacity(1024);
    stream
        .take(u64::try_from(HEALTH_LIMIT + 1).map_err(io::Error::other)?)
        .read_to_end(&mut bytes)
        .await?;
    Ok(bytes)
}

fn validate_health(bytes: &[u8]) -> io::Result<()> {
    #[derive(serde::Deserialize)]
    struct Health {
        status: String,
    }
    let body = validate_response(bytes, 200)?;
    let value: Health = serde_json::from_slice(body).map_err(io::Error::other)?;
    if value.status != "ok" {
        return Err(io::Error::other("Backend is not ready."));
    }
    Ok(())
}

fn validate_response(bytes: &[u8], status: u16) -> io::Result<&[u8]> {
    let invalid = || io::Error::other("Backend is not ready.");
    if bytes.len() > HEALTH_LIMIT {
        return Err(invalid());
    }
    let mut headers = [httparse::EMPTY_HEADER; crate::MAX_HEADERS];
    let mut response = httparse::Response::new(&mut headers);
    let httparse::Status::Complete(head) = response.parse(bytes).map_err(|_| invalid())? else {
        return Err(invalid());
    };
    if head > crate::MAX_HEADER_BYTES
        || response.code != Some(status)
        || response.version != Some(1)
    {
        return Err(invalid());
    }
    let mut length = None;
    let mut content_type = None;
    for header in response.headers {
        if header.name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(invalid());
        }
        if header.name.eq_ignore_ascii_case("content-length") {
            if length.is_some()
                || header.value.is_empty()
                || !header.value.iter().all(u8::is_ascii_digit)
            {
                return Err(invalid());
            }
            length = Some(
                std::str::from_utf8(header.value)
                    .map_err(|_| invalid())?
                    .parse::<usize>()
                    .map_err(|_| invalid())?,
            );
        }
        if header.name.eq_ignore_ascii_case("content-type") {
            if content_type.is_some() {
                return Err(invalid());
            }
            content_type = Some(header.value);
        }
    }
    let body = &bytes[head..];
    if body.len() > 4096
        || length != Some(body.len())
        || !content_type.is_some_and(|value| {
            value == b"application/json" || value == b"application/json; charset=utf-8"
        })
    {
        return Err(invalid());
    }
    Ok(body)
}

#[cfg(test)]
#[path = "launcher_tests.rs"]
mod lifecycle_tests;

#[cfg(test)]
mod tests {
    use super::{LaunchConfig, LaunchError, LaunchProfile, validate_health};
    use std::ffi::OsString;

    fn arguments() -> Vec<OsString> {
        [
            "--launch-development",
            "--allow-unisolated-backend",
            "--data-directory",
            "C:\\ExternalData",
            "--evidence-parent",
            "C:\\ExternalEvidence",
            "--frontend-port",
            "0",
            "--backend-port",
            "8081",
        ]
        .into_iter()
        .map(OsString::from)
        .collect()
    }

    #[test]
    fn lora_launch_is_explicit_swift_prism_only() {
        let default = LaunchConfig::parse(arguments()).ok();
        assert!(default.is_some_and(|config| !config.use_abliterate_lora));
        let mut enabled = arguments();
        enabled.extend(
            ["--profile", "swift-pq2-assessment", "--use-abliterate-lora"].map(OsString::from),
        );
        let config = LaunchConfig::parse(enabled.clone()).ok();
        assert!(config.is_some_and(|config| config.use_abliterate_lora));
        for extra in ["--use-abliterate-lora", "--native-engine"] {
            let mut rejected = enabled.clone();
            rejected.push(extra.into());
            assert_eq!(
                LaunchConfig::parse(rejected).err(),
                Some(LaunchError::Arguments)
            );
        }
        let mut reference = arguments();
        reference.push("--use-abliterate-lora".into());
        assert_eq!(
            LaunchConfig::parse(reference).err(),
            Some(LaunchError::Arguments)
        );
    }

    #[test]
    fn lora_guard_rejects_missing_and_altered_adapter_and_opt_out_needs_no_file()
    -> std::io::Result<()> {
        use std::{fs, io};
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-lora-guard-{stamp}"));
        fs::create_dir(&directory)?;
        let key = directory.join("not-created.key");
        let base = super::LaunchPlan::for_profile(
            &directory,
            49152,
            &key,
            LaunchProfile::SwiftPq2Assessment,
        )
        .map_err(io::Error::other)?;
        assert!(
            LaunchConfig::adapter_guard(&base)
                .map_err(|_| io::Error::other("Unexpected base guard"))?
                .is_none()
        );
        let plan = super::LaunchPlan::for_profile_with_lora(
            &directory,
            49152,
            &key,
            LaunchProfile::SwiftPq2Assessment,
            true,
        )
        .map_err(io::Error::other)?;
        assert_eq!(
            LaunchConfig::adapter_guard(&plan).err(),
            Some(LaunchError::Adapter)
        );
        let path = &plan
            .adapter
            .as_ref()
            .ok_or_else(|| io::Error::other("Missing adapter plan"))?
            .path;
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("Missing parent"))?;
        fs::create_dir_all(parent)?;
        fs::write(path, b"altered adapter")?;
        assert_eq!(
            LaunchConfig::adapter_guard(&plan).err(),
            Some(LaunchError::Adapter)
        );
        assert_eq!(directory.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn launch_requires_explicit_opt_ins_and_rejects_overrides() {
        assert!(LaunchConfig::parse(arguments()).is_ok());
        let mut missing = arguments();
        missing.remove(1);
        assert_eq!(
            LaunchConfig::parse(missing).err(),
            Some(LaunchError::Arguments)
        );
        for extra in [
            vec!["--model", "C:\\OtherModel"],
            vec!["--frontend-port", "5173"],
            vec!["--max-waiting", "9"],
            vec!["--allow-unverified-key-files"],
        ] {
            let mut args = arguments();
            args.extend(extra.into_iter().map(OsString::from));
            assert_eq!(
                LaunchConfig::parse(args).err(),
                Some(LaunchError::Arguments)
            );
        }
        let mut collision = arguments();
        collision[7] = OsString::from("8081");
        assert_eq!(
            LaunchConfig::parse(collision).err(),
            Some(LaunchError::Arguments)
        );
    }

    #[test]
    fn native_engine_is_explicit_and_swift_only() {
        let mut args = arguments();
        args.push(OsString::from("--native-engine"));
        assert_eq!(
            LaunchConfig::parse(args).err(),
            Some(LaunchError::Arguments)
        );
        let mut args = arguments();
        args.extend(["--profile", "swift-pq2-assessment", "--native-engine"].map(OsString::from));
        let config = LaunchConfig::parse(args).ok();
        assert!(config.is_some_and(|value| value.uses_native_engine()));
        let mut args = arguments();
        args.extend(
            [
                "--profile",
                "swift-pq2-assessment",
                "--native-engine",
                "--native-engine",
            ]
            .map(OsString::from),
        );
        assert_eq!(
            LaunchConfig::parse(args).err(),
            Some(LaunchError::Arguments)
        );
    }

    #[test]
    fn native_readiness_requires_exact_model_template_runtime_context_and_seed()
    -> std::io::Result<()> {
        use serde_json::json;
        let model = json!({"id":"Swift-Bonsai-2-PQ2_0","object":"model",
            "model_sha256":crate::inference::MODEL_SHA256,
            "template_sha256":crate::inference::TEMPLATE_SHA256,
            "runtime":"prism-inference","context_tokens":8192,"seed":42});
        let body = serde_json::to_string(&json!({"data":[model]}))?;
        assert!(super::validate_native_identity(&response(&body)).is_ok());
        for (field, changed) in [
            ("id", json!("foreign")),
            ("object", json!("list")),
            ("model_sha256", json!("0".repeat(64))),
            ("template_sha256", json!("0".repeat(64))),
            ("runtime", json!("Prism")),
            ("context_tokens", json!(8193)),
            ("seed", json!(43)),
        ] {
            let mut changed_model = model.clone();
            changed_model[field] = changed;
            let body = serde_json::to_string(&json!({"data":[changed_model]}))?;
            assert!(
                super::validate_native_identity(&response(&body)).is_err(),
                "{field}"
            );
        }
        for data in [
            json!([]),
            json!([model.clone(), model]),
            json!([["positional"]]),
        ] {
            let body = serde_json::to_string(&json!({"data":data}))?;
            assert!(super::validate_native_identity(&response(&body)).is_err());
        }
        let duplicate = body.replace("\"seed\":42", "\"seed\":42,\"seed\":42");
        assert!(super::validate_native_identity(&response(&duplicate)).is_err());
        Ok(())
    }

    #[test]
    fn profiles_are_explicit_exact_unique_and_default_to_reference() {
        let default = LaunchConfig::parse(arguments());
        assert_eq!(
            default.map(|config| config.profile()),
            Ok(LaunchProfile::ReferenceCpu)
        );
        for profile in [
            LaunchProfile::ReferenceCpu,
            LaunchProfile::SwiftPq2Assessment,
        ] {
            let mut args = arguments();
            args.extend([OsString::from("--profile"), OsString::from(profile.name())]);
            assert_eq!(
                LaunchConfig::parse(args).map(|config| config.profile()),
                Ok(profile)
            );
        }
        for extra in [
            vec!["--profile"],
            vec!["--profile", ""],
            vec!["--profile", "Swift-pq2-assessment"],
            vec!["--profile", "swift-ptq1-assessment"],
            vec!["--profile", "C:\\ExternalProfile.json"],
            vec![
                "--profile",
                "swift-pq2-assessment",
                "--profile",
                "reference-cpu",
            ],
            vec![
                "--profile",
                "swift-pq2-assessment",
                "--lora",
                "C:\\Adapter.gguf",
            ],
            vec!["--profile", "swift-pq2-assessment", "--use-repacked-ptq1"],
            vec![
                "--profile",
                "swift-pq2-assessment",
                "--model",
                "C:\\Other.gguf",
            ],
            vec![
                "--profile",
                "swift-pq2-assessment",
                "--runtime",
                "C:\\Other.exe",
            ],
            vec!["--profile", "swift-pq2-assessment", "--batch-size", "256"],
        ] {
            let mut args = arguments();
            args.extend(extra.into_iter().map(OsString::from));
            assert_eq!(
                LaunchConfig::parse(args).err(),
                Some(LaunchError::Arguments)
            );
        }
    }

    fn response(body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[test]
    fn readiness_requires_bounded_complete_unambiguous_json() {
        assert!(validate_health(&response(r#"{"status":"ok"}"#)).is_ok());
        assert!(validate_health(&response(r#"{"status":"loading model"}"#)).is_err());
        assert!(validate_health(&response(r#"{"status":"ok"}garbage"#)).is_err());
        for message in [
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\nContent-Length: 15\r\n\r\n{\"status\":\"ok\"}",
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 16\r\n\r\n{\"status\":\"ok\"}",
            "HTTP/1.1 503 Busy\r\nContent-Type: application/json\r\nContent-Length: 15\r\n\r\n{\"status\":\"ok\"}",
        ] {
            assert!(validate_health(message.as_bytes()).is_err());
        }
        assert!(validate_health(&vec![b' '; 20 * 1024 + 1]).is_err());
    }
}
