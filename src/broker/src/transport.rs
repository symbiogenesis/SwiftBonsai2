//! Fixed numeric-loopback backend connections for development integration.
//!
//! The Windows launcher can require an owned listener PID for each connection.
//! Direct backend access and daily OS isolation remain separately unqualified.

use crate::{
    backend::{BackendError, BackendPolicy},
    supervisor::BackendConnector,
};
use std::{
    net::{Ipv4Addr, SocketAddrV4},
    time::Duration,
};
use tokio::{
    net::TcpStream,
    time::{Instant, timeout_at},
};

/// Maximum duration for establishing one private backend connection.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Immutable endpoint and private HTTP policy, with no diagnostic serialization.
///
/// Both token counting and generation use this same endpoint and credential.
/// This does not attest the process listening there or its model/template.
pub struct LoopbackBackend {
    address: SocketAddrV4,
    policy: BackendPolicy,
    #[cfg(windows)]
    process_id: Option<u32>,
}

impl LoopbackBackend {
    /// Configure one numeric IPv4 endpoint; no DNS or connection occurs here.
    ///
    /// The caller must supply a random owner-private key, distinct from the
    /// frontend credential, and prevent backend replacement or bypass.
    ///
    /// # Errors
    /// Returns [`BackendError::Configuration`] for an invalid port or credential.
    pub fn new(port: u16, key: &str) -> Result<Self, BackendError> {
        Ok(Self {
            address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, port),
            policy: BackendPolicy::new(port, key)?,
            #[cfg(windows)]
            process_id: None,
        })
    }

    #[cfg(windows)]
    pub(crate) const fn require_process(&mut self, process_id: u32) {
        self.process_id = Some(process_id);
    }

    fn check_owner(&self) -> Result<(), BackendError> {
        #[cfg(windows)]
        if let Some(process_id) = self.process_id
            && bonsai_windows_process::listener_process_id(self.address.port())
                .map_err(|_| BackendError::Io)?
                != Some(process_id)
        {
            return Err(BackendError::Configuration);
        }
        Ok(())
    }
}

impl BackendConnector for LoopbackBackend {
    type Stream = TcpStream;

    fn policy(&self) -> &BackendPolicy {
        &self.policy
    }

    async fn connect(&self, deadline: Instant) -> Result<TcpStream, BackendError> {
        self.check_owner()?;
        let deadline = deadline.min(Instant::now() + CONNECT_TIMEOUT);
        if Instant::now() >= deadline {
            return Err(BackendError::Deadline);
        }
        let stream = timeout_at(deadline, TcpStream::connect(self.address))
            .await
            .map_err(|_| BackendError::Deadline)?
            .map_err(|_| BackendError::Io)?;
        if Instant::now() >= deadline {
            return Err(BackendError::Deadline);
        }
        self.check_owner()?;
        stream.set_nodelay(true).map_err(|_| BackendError::Io)?;
        Ok(stream)
    }
}
