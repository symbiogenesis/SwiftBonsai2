//! Explicit development-only listener integration. Nothing starts automatically.
//!
//! This module is not a launcher or qualified daily inference boundary. Backend
//! process identity, bypass prevention, OS memory/filesystem/egress restrictions,
//! and real-model behavior must be qualified before exposing daily chat.

use crate::{
    RequestPolicy,
    backend::{BackendError, BackendPolicy},
    scheduler::{GateError, RequestGate, Reservation},
    supervisor::{BackendConnector, REQUEST_TIMEOUT, ServeError, serve_chat},
    transport::LoopbackBackend,
};
use std::{
    future::{Future, poll_fn},
    net::{Ipv4Addr, SocketAddrV4},
    pin::pin,
    sync::atomic::{AtomicBool, Ordering},
    task::Poll,
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::{
    net::{TcpListener, TcpStream},
    time::{Instant, timeout_at},
};

/// Maximum time an accepted socket may wait for the single active request slot.
pub const QUEUE_TIMEOUT: Duration = Duration::from_secs(30);

/// Value-free service configuration, admission, transport or request error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceError {
    /// Ports or credentials are invalid, shared, or unsuitable for loopback use.
    Configuration,
    /// Binding or accepting the numeric loopback listener failed.
    Io,
    /// No reservation is available, the queue expired, or shutdown was requested.
    Gate(GateError),
    /// The admitted request failed; the connection may contain a partial response.
    Request(ServeError),
}

/// Numeric-loopback listener, immutable backend and bounded admission gate.
///
/// Bind is explicit and requires a Tokio runtime with I/O and time enabled.
/// No task, process, key file, UI, retry or restart is created. The application
/// must poll accepted requests concurrently to use the bounded waiting capacity.
pub struct LocalChatService {
    listener: TcpListener,
    policy: RequestPolicy,
    backend: LoopbackBackend,
    gate: RequestGate,
    port: u16,
    capacity: usize,
}

impl LocalChatService {
    /// Bind only `127.0.0.1`, using distinct frontend/backend ports and keys.
    ///
    /// Port zero requests an OS-assigned frontend port; other frontend ports
    /// and the backend port must be unprivileged. Keys are never generated or
    /// logged here. The backend must already be owned by the future launcher.
    ///
    /// # Errors
    /// Returns a value-free [`ServiceError`] for invalid policy or bind failure.
    pub async fn bind(
        port: u16,
        frontend_key: &str,
        backend_port: u16,
        backend_key: &str,
        max_waiting: usize,
    ) -> Result<Self, ServiceError> {
        if (port != 0 && port < 1024)
            || port == backend_port
            || bool::from(frontend_key.as_bytes().ct_eq(backend_key.as_bytes()))
        {
            return Err(ServiceError::Configuration);
        }
        // Validate every non-I/O input before binding a socket.
        RequestPolicy::new(1024, frontend_key).map_err(|_| ServiceError::Configuration)?;
        let backend = LoopbackBackend::new(backend_port, backend_key)
            .map_err(|_| ServiceError::Configuration)?;
        let gate = RequestGate::new(max_waiting).map_err(ServiceError::Gate)?;
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
            .await
            .map_err(|_| ServiceError::Io)?;
        let port = listener.local_addr().map_err(|_| ServiceError::Io)?.port();
        if port < 1024 || port == backend_port {
            return Err(ServiceError::Configuration);
        }
        Ok(Self {
            listener,
            policy: RequestPolicy::new(port, frontend_key)
                .map_err(|_| ServiceError::Configuration)?,
            backend,
            gate,
            port,
            capacity: max_waiting + 1,
        })
    }

    /// Numeric frontend port, including an OS-assigned port after binding zero.
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Immutable total reservation bound, including the one active request.
    ///
    /// Pending accepts and accepted handles share this bound. This is not a
    /// count of currently available slots and does not change after shutdown.
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Stop admission and wake queued, accepting and active operations.
    ///
    /// Polling operations observe closure and drop their owned streams. No
    /// backend-process cancellation acknowledgement or termination is implied.
    /// The listener remains bound until this service is dropped.
    pub fn close(&self) {
        self.gate.close();
    }

    /// Reserve capacity before accepting a socket or allocating request buffers.
    ///
    /// At most one active plus the configured waiting count may be reserved,
    /// including pending accept calls and returned but unpolled requests. Kernel
    /// TCP backlog is separate. A full gate returns immediately; the caller must
    /// avoid a busy retry loop. Dropping this future releases its reservation.
    ///
    /// # Errors
    /// Returns [`ServiceError`] for capacity, shutdown or socket failure.
    pub async fn accept(&self) -> Result<AcceptedChat<'_>, ServiceError> {
        let reservation = self.gate.reserve().map_err(ServiceError::Gate)?;
        let shutdown = pin!(self.gate.closed());
        let accepting = pin!(self.listener.accept());
        let mut shutdown = shutdown;
        let mut accepting = accepting;
        let (stream, peer) = poll_fn(|cx| {
            if shutdown.as_mut().poll(cx).is_ready() {
                return Poll::Ready(Err(ServiceError::Gate(GateError::Closed)));
            }
            accepting
                .as_mut()
                .poll(cx)
                .map(|result| result.map_err(|_| ServiceError::Io))
        })
        .await?;
        if !peer.ip().is_loopback() || !peer.is_ipv4() {
            return Err(ServiceError::Configuration);
        }
        stream.set_nodelay(true).map_err(|_| ServiceError::Io)?;
        let accepted = Instant::now();
        Ok(AcceptedChat {
            stream,
            reservation,
            service: self,
            accepted,
        })
    }
}

/// One reserved, owned socket. Drop closes it and releases capacity.
/// No Debug implementation: peer streams and policy are intentionally private.
pub struct AcceptedChat<'a> {
    stream: TcpStream,
    reservation: Reservation<'a>,
    service: &'a LocalChatService,
    accepted: Instant,
}

impl AcceptedChat<'_> {
    /// Wait fairly for the single active slot, then serve one bounded request.
    ///
    /// Queue time counts toward the ten-minute total. Queue expiry and shutdown
    /// drop this socket without reading a body; errors never append a second
    /// HTTP response. Client half-close is treated as abandonment by the
    /// supervisor, and disconnecting closes the backend transport. Once a
    /// generation connection is attempted, any failure or caller cancellation
    /// closes the entire admission gate: a new request must not follow an
    /// unacknowledged backend cancellation. Recreating this service requires
    /// the caller to establish a stopped or freshly owned backend first.
    ///
    /// # Errors
    /// Returns [`ServiceError`] for queue/deadline/shutdown or request failure.
    pub async fn serve(self) -> Result<(), ServiceError> {
        let deadline = self.accepted + REQUEST_TIMEOUT;
        let queue_deadline = deadline.min(self.accepted + QUEUE_TIMEOUT);
        if Instant::now() >= queue_deadline {
            return Err(ServiceError::Gate(GateError::Deadline));
        }
        let active = timeout_at(queue_deadline, self.reservation.enter(deadline))
            .await
            .map_err(|_| ServiceError::Gate(GateError::Deadline))?
            .map_err(ServiceError::Gate)?;
        if Instant::now() >= queue_deadline {
            return Err(ServiceError::Gate(GateError::Deadline));
        }
        active.check().map_err(ServiceError::Gate)?;
        let backend = GenerationAttempt {
            backend: &self.service.backend,
            gate: &self.service.gate,
            counted: AtomicBool::new(false),
            generation: AtomicBool::new(false),
            completed: AtomicBool::new(false),
        };
        let mut shutdown = pin!(self.service.gate.closed());
        let mut serving = pin!(serve_chat(
            self.stream,
            &self.service.policy,
            &backend,
            deadline
        ));
        let result = poll_fn(|cx| {
            if shutdown.as_mut().poll(cx).is_ready() {
                return Poll::Ready(Err(ServiceError::Gate(GateError::Closed)));
            }
            serving
                .as_mut()
                .poll(cx)
                .map(|result| result.map_err(ServiceError::Request))
        })
        .await;
        backend.completed.store(result.is_ok(), Ordering::Relaxed);
        active.check().map_err(ServiceError::Gate)?;
        result
    }
}

// The supervisor makes exactly one count connection, then one generation
// connection. Mark before the second attempt, including cancellation of connect.
// Drop runs even when the caller cancels AcceptedChat::serve externally.
struct GenerationAttempt<'a> {
    backend: &'a LoopbackBackend,
    gate: &'a RequestGate,
    counted: AtomicBool,
    generation: AtomicBool,
    completed: AtomicBool,
}

impl BackendConnector for GenerationAttempt<'_> {
    type Stream = TcpStream;

    fn policy(&self) -> &BackendPolicy {
        self.backend.policy()
    }

    async fn connect(&self, deadline: Instant) -> Result<TcpStream, BackendError> {
        if self.counted.swap(true, Ordering::Relaxed) {
            self.generation.store(true, Ordering::Relaxed);
        }
        self.backend.connect(deadline).await
    }
}

impl Drop for GenerationAttempt<'_> {
    fn drop(&mut self) {
        if self.generation.load(Ordering::Relaxed) && !self.completed.load(Ordering::Relaxed) {
            self.gate.close();
        }
    }
}
