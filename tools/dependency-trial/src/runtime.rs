//! Bounded, reused-connection loopback comparisons for runtime candidates.
//!
//! These trials isolate framed TCP I/O. They do not model the broker's complete
//! cancellation, admission, HTTP, or inference behavior. Every candidate uses
//! the same standard-library echo peers and verifies every returned byte.

use std::future::poll_fn;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::pin::Pin;
use std::sync::mpsc;
use std::task::Poll;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const SAMPLE_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(2);

/// An implementation under comparison, including the existing runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Engine {
    /// The broker's current single-threaded Tokio executor.
    Tokio,
    /// Smol's small executor and readiness-based I/O implementation.
    Smol,
    /// Compio's completion-based executor, using Windows IOCP on our target.
    Compio,
    /// A bounded standard-library thread for each active connection.
    Bespoke,
}

impl Engine {
    /// All candidates, with the existing implementation first.
    pub(super) const ALL: [Self; 4] = [Self::Tokio, Self::Smol, Self::Compio, Self::Bespoke];

    /// Stable identifier recorded in trial results.
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Tokio => "tokio",
            Self::Smol => "smol",
            Self::Compio => "compio",
            Self::Bespoke => "bespoke-std",
        }
    }
}

/// A bounded request/response workload using already-connected TCP sockets.
#[derive(Clone, Copy, Debug)]
pub(super) struct Workload {
    /// Concurrent connections; must lie in the broker-relevant range 1..=9.
    pub(super) connections: usize,
    /// Bytes in each request and its equal-size response: 256 or 4096.
    pub(super) frame_bytes: usize,
    /// Exchanges per reused connection.
    pub(super) rounds: usize,
}

impl Workload {
    fn validate(self) -> Result<(), String> {
        if !(1..=9).contains(&self.connections)
            || ![256, 4096].contains(&self.frame_bytes)
            || !(1..=1_000_000).contains(&self.rounds)
        {
            return Err("runtime workload exceeds the documented bounded range".to_owned());
        }
        Ok(())
    }
}

/// One verified timing observation. Socket, runtime and thread setup is excluded.
#[derive(Clone, Copy, Debug)]
pub(super) struct Sample {
    /// Elapsed time of the exchange loop, in nanoseconds.
    pub(super) elapsed_ns: u128,
    /// Completed request/response exchanges.
    pub(super) operations: u64,
    /// Total bytes sent and received by the client.
    pub(super) bytes: u64,
    /// Deterministic checksum of all verified responses.
    pub(super) checksum: u64,
}

struct Connection<S> {
    stream: S,
    request: Vec<u8>,
    response: Vec<u8>,
}

impl<S> Connection<S> {
    fn map_stream<T>(
        self,
        convert: impl FnOnce(S) -> std::io::Result<T>,
    ) -> Result<Connection<T>, String> {
        Ok(Connection {
            stream: convert(self.stream).map_err(io_error)?,
            request: self.request,
            response: self.response,
        })
    }
}

struct Peers {
    handles: Vec<JoinHandle<Result<u64, String>>>,
}

impl Peers {
    fn finish(mut self) -> Result<u64, String> {
        let mut checksum = 0_u64;
        let mut failure = None;
        for handle in self.handles.drain(..) {
            match handle.join() {
                Ok(Ok(value)) => checksum = checksum.wrapping_add(value),
                Ok(Err(error)) => failure = failure.or(Some(error)),
                Err(_) => {
                    failure = failure.or_else(|| Some("runtime echo peer panicked".to_owned()));
                }
            }
        }
        failure.map_or(Ok(checksum), Err)
    }
}

impl Drop for Peers {
    fn drop(&mut self) {
        for handle in self.handles.drain(..) {
            let _result = handle.join();
        }
    }
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "map_err consumes its error argument"
)]
fn io_error(error: std::io::Error) -> String {
    format!("runtime trial I/O: {error}")
}

fn frame(connection: usize, bytes: usize) -> Vec<u8> {
    (0..bytes)
        .map(|index| {
            // The bounded workload makes the conversion infallible.
            u8::try_from((index + connection * 31) % 251).unwrap_or_default()
        })
        .collect()
}

fn mark_round(request: &mut [u8], round: usize) -> Result<(), String> {
    let round = u64::try_from(round).map_err(|error| error.to_string())?;
    request[..8].copy_from_slice(&round.to_le_bytes());
    Ok(())
}

fn verify_response(request: &[u8], response: &[u8]) -> Result<u64, String> {
    if response != request || response.len() < 8 {
        return Err("runtime trial returned a corrupt or stale response".to_owned());
    }
    let mut round = [0_u8; 8];
    round.copy_from_slice(&response[..8]);
    Ok(u64::from_le_bytes(round).wrapping_add(u64::from(response[response.len() - 1])))
}

fn check_deadline(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        return Err("runtime trial exceeded its finite deadline".to_owned());
    }
    Ok(())
}

fn prepare(workload: Workload) -> Result<(Vec<Connection<TcpStream>>, Peers), String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(io_error)?;
    let address = listener.local_addr().map_err(io_error)?;
    let mut clients = Vec::with_capacity(workload.connections);
    let mut peers = Peers {
        handles: Vec::with_capacity(workload.connections),
    };
    for connection in 0..workload.connections {
        let client = TcpStream::connect_timeout(&address, IO_TIMEOUT).map_err(io_error)?;
        let (mut peer, _) = listener.accept().map_err(io_error)?;
        for socket in [&client, &peer] {
            socket.set_nodelay(true).map_err(io_error)?;
            socket
                .set_read_timeout(Some(IO_TIMEOUT))
                .map_err(io_error)?;
            socket
                .set_write_timeout(Some(IO_TIMEOUT))
                .map_err(io_error)?;
        }
        let mut request = frame(connection, workload.frame_bytes);
        mark_round(&mut request, 0)?;
        let mut expected = request.clone();
        let mut received = vec![0; workload.frame_bytes];
        peers.handles.push(thread::spawn(move || {
            let deadline = Instant::now() + SAMPLE_TIMEOUT;
            let mut checksum = 0_u64;
            for round in 0..=workload.rounds {
                check_deadline(deadline)?;
                mark_round(&mut expected, round)?;
                peer.read_exact(&mut received).map_err(io_error)?;
                let observed = verify_response(&expected, &received)?;
                if round != 0 {
                    checksum = checksum.wrapping_add(observed);
                }
                peer.write_all(&received).map_err(io_error)?;
            }
            Ok(checksum)
        }));
        clients.push(Connection {
            stream: client,
            request,
            response: vec![0; workload.frame_bytes],
        });
    }
    Ok((clients, peers))
}

/// Measure one candidate after the caller has enforced the CI-only benchmark rule.
///
/// Every response is checked; invalid data and timeouts fail the observation.
pub(super) fn measure(engine: Engine, workload: Workload) -> Result<Sample, String> {
    workload.validate()?;
    let (clients, peers) = prepare(workload)?;
    let outcome = match engine {
        Engine::Tokio => tokio(clients, workload),
        Engine::Smol => smol(clients, workload),
        Engine::Compio => compio(clients, workload),
        Engine::Bespoke => bespoke(clients, workload),
    };
    // Always join the shared peers, including when the client side fails.
    let peer_checksum = peers.finish();
    let (elapsed_ns, checksum) = outcome?;
    if peer_checksum? != checksum {
        return Err("runtime client/peer checksum mismatch".to_owned());
    }
    let operations =
        u64::try_from(workload.connections * workload.rounds).map_err(|error| error.to_string())?;
    let frame_bytes = u64::try_from(workload.frame_bytes).map_err(|error| error.to_string())?;
    Ok(Sample {
        elapsed_ns,
        operations,
        bytes: operations * frame_bytes * 2,
        checksum,
    })
}

// A single shared join strategy prevents differences in task-spawning policy
// from being mistaken for differences in socket operations. It polls at most
// nine futures and never detaches a task.
#[allow(
    clippy::future_not_send,
    reason = "all candidates run local borrowed futures without spawning tasks"
)]
fn join_connections<F>(futures: Vec<F>) -> impl Future<Output = Result<u64, String>>
where
    F: Future<Output = Result<u64, String>>,
{
    let mut pending: Vec<Option<Pin<Box<F>>>> = futures
        .into_iter()
        .map(|future| Some(Box::pin(future)))
        .collect();
    let mut checksum = 0_u64;
    poll_fn(move |cx| {
        for future in &mut pending {
            if let Some(active) = future {
                match active.as_mut().poll(cx) {
                    Poll::Ready(Ok(value)) => {
                        checksum = checksum.wrapping_add(value);
                        *future = None;
                    }
                    Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                    Poll::Pending => {}
                }
            }
        }
        if pending.iter().all(Option::is_none) {
            Poll::Ready(Ok(checksum))
        } else {
            Poll::Pending
        }
    })
}

fn tokio(clients: Vec<Connection<TcpStream>>, workload: Workload) -> Result<(u128, u64), String> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(io_error)?;
    runtime.block_on(async move {
        let mut connections = Vec::with_capacity(clients.len());
        for client in clients {
            client.stream.set_nonblocking(true).map_err(io_error)?;
            let mut connection = client.map_stream(tokio::net::TcpStream::from_std)?;
            tokio::time::timeout(IO_TIMEOUT, async {
                connection
                    .stream
                    .write_all(&connection.request)
                    .await
                    .map_err(io_error)?;
                connection
                    .stream
                    .read_exact(&mut connection.response)
                    .await
                    .map_err(io_error)?;
                verify_response(&connection.request, &connection.response)
            })
            .await
            .map_err(|error| format!("Tokio warmup timeout: {error}"))??;
            connections.push(connection);
        }
        let futures = connections
            .into_iter()
            .map(|mut connection| async move {
                let mut checksum = 0_u64;
                for round in 1..=workload.rounds {
                    mark_round(&mut connection.request, round)?;
                    connection
                        .stream
                        .write_all(&connection.request)
                        .await
                        .map_err(io_error)?;
                    connection
                        .stream
                        .read_exact(&mut connection.response)
                        .await
                        .map_err(io_error)?;
                    checksum = checksum
                        .wrapping_add(verify_response(&connection.request, &connection.response)?);
                }
                Ok(checksum)
            })
            .collect();
        let exchanges = join_connections(futures);
        let start = Instant::now();
        let checksum = tokio::time::timeout(SAMPLE_TIMEOUT, exchanges)
            .await
            .map_err(|error| format!("Tokio trial timeout: {error}"))??;
        Ok((start.elapsed().as_nanos(), checksum))
    })
}

fn smol(clients: Vec<Connection<TcpStream>>, workload: Workload) -> Result<(u128, u64), String> {
    use smol::io::{AsyncReadExt as _, AsyncWriteExt as _};

    smol::block_on(async move {
        let mut connections = Vec::with_capacity(clients.len());
        for client in clients {
            let mut connection = client.map_stream(smol::Async::new)?;
            let warmup = async {
                connection
                    .stream
                    .write_all(&connection.request)
                    .await
                    .map_err(io_error)?;
                connection
                    .stream
                    .read_exact(&mut connection.response)
                    .await
                    .map_err(io_error)?;
                verify_response(&connection.request, &connection.response)
            };
            let timeout = async {
                smol::Timer::after(IO_TIMEOUT).await;
                Err("Smol warmup exceeded its finite deadline".to_owned())
            };
            smol::future::race(warmup, timeout).await?;
            connections.push(connection);
        }
        let futures = connections
            .into_iter()
            .map(|mut connection| async move {
                let mut checksum = 0_u64;
                for round in 1..=workload.rounds {
                    mark_round(&mut connection.request, round)?;
                    connection
                        .stream
                        .write_all(&connection.request)
                        .await
                        .map_err(io_error)?;
                    connection
                        .stream
                        .read_exact(&mut connection.response)
                        .await
                        .map_err(io_error)?;
                    checksum = checksum
                        .wrapping_add(verify_response(&connection.request, &connection.response)?);
                }
                Ok(checksum)
            })
            .collect();
        let timeout = async {
            smol::Timer::after(SAMPLE_TIMEOUT).await;
            Err("Smol trial exceeded its finite deadline".to_owned())
        };
        let exchanges = join_connections(futures);
        let start = Instant::now();
        let checksum = smol::future::race(exchanges, timeout).await?;
        Ok((start.elapsed().as_nanos(), checksum))
    })
}

fn compio(clients: Vec<Connection<TcpStream>>, workload: Workload) -> Result<(u128, u64), String> {
    use compio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let runtime = compio::runtime::Runtime::new().map_err(io_error)?;
    runtime.block_on(async move {
        let mut connections = Vec::with_capacity(clients.len());
        for client in clients {
            let mut connection = client.map_stream(compio::net::TcpStream::from_std)?;
            connection = compio::time::timeout(IO_TIMEOUT, async move {
                let compio::BufResult(result, request) =
                    connection.stream.write_all(connection.request).await;
                result.map_err(io_error)?;
                connection.request = request;
                let compio::BufResult(result, response) =
                    connection.stream.read_exact(connection.response).await;
                result.map_err(io_error)?;
                connection.response = response;
                verify_response(&connection.request, &connection.response)?;
                Ok::<_, String>(connection)
            })
            .await
            .map_err(|error| format!("Compio warmup timeout: {error}"))??;
            connections.push(connection);
        }
        let futures = connections
            .into_iter()
            .map(|mut connection| async move {
                let mut checksum = 0_u64;
                for round in 1..=workload.rounds {
                    mark_round(&mut connection.request, round)?;
                    let compio::BufResult(result, request) =
                        connection.stream.write_all(connection.request).await;
                    result.map_err(io_error)?;
                    connection.request = request;
                    let compio::BufResult(result, response) =
                        connection.stream.read_exact(connection.response).await;
                    result.map_err(io_error)?;
                    connection.response = response;
                    checksum = checksum
                        .wrapping_add(verify_response(&connection.request, &connection.response)?);
                }
                Ok(checksum)
            })
            .collect();
        let exchanges = join_connections(futures);
        let start = Instant::now();
        let checksum = compio::time::timeout(SAMPLE_TIMEOUT, exchanges)
            .await
            .map_err(|error| format!("Compio trial timeout: {error}"))??;
        Ok((start.elapsed().as_nanos(), checksum))
    })
}

fn bespoke(clients: Vec<Connection<TcpStream>>, workload: Workload) -> Result<(u128, u64), String> {
    let (ready_sender, ready_receiver) = mpsc::channel();
    let mut start_senders = Vec::with_capacity(clients.len());
    let handles: Vec<_> = clients
        .into_iter()
        .map(|mut connection| {
            let (start_sender, start_receiver) = mpsc::channel();
            start_senders.push(start_sender);
            let ready_sender = ready_sender.clone();
            thread::spawn(move || {
                connection
                    .stream
                    .write_all(&connection.request)
                    .map_err(io_error)?;
                connection
                    .stream
                    .read_exact(&mut connection.response)
                    .map_err(io_error)?;
                verify_response(&connection.request, &connection.response)?;
                ready_sender.send(()).map_err(|error| error.to_string())?;
                start_receiver
                    .recv_timeout(IO_TIMEOUT)
                    .map_err(|error| error.to_string())?;
                let deadline = Instant::now() + SAMPLE_TIMEOUT;
                let mut checksum = 0_u64;
                for round in 1..=workload.rounds {
                    check_deadline(deadline)?;
                    mark_round(&mut connection.request, round)?;
                    connection
                        .stream
                        .write_all(&connection.request)
                        .map_err(io_error)?;
                    connection
                        .stream
                        .read_exact(&mut connection.response)
                        .map_err(io_error)?;
                    checksum = checksum
                        .wrapping_add(verify_response(&connection.request, &connection.response)?);
                }
                Ok(checksum)
            })
        })
        .collect();
    let mut workers = Peers { handles };
    drop(ready_sender);
    for _ in 0..workload.connections {
        ready_receiver
            .recv_timeout(IO_TIMEOUT)
            .map_err(|error| error.to_string())?;
    }
    let start = Instant::now();
    for start_sender in start_senders {
        start_sender.send(()).map_err(|error| error.to_string())?;
    }
    let mut checksum = 0_u64;
    let mut failure = None;
    for handle in workers.handles.drain(..) {
        match handle.join() {
            Ok(Ok(value)) => checksum = checksum.wrapping_add(value),
            Ok(Err(error)) => failure = failure.or(Some(error)),
            Err(_) => {
                failure = failure.or_else(|| Some("bespoke trial client panicked".to_owned()));
            }
        }
    }
    failure.map_or_else(|| Ok((start.elapsed().as_nanos(), checksum)), Err)
}

#[cfg(test)]
mod tests {
    use super::{Workload, frame, mark_round, verify_response};

    #[test]
    fn protocol_verifies_full_payload_and_round_identity() -> Result<(), String> {
        let mut request = frame(8, 4096);
        mark_round(&mut request, 42)?;
        assert_eq!(
            verify_response(&request, &request)?,
            42 + u64::from(request[4095])
        );
        let mut stale = request.clone();
        mark_round(&mut stale, 41)?;
        assert!(verify_response(&request, &stale).is_err());
        let mut corrupt = request.clone();
        corrupt[2048] ^= 1;
        assert!(verify_response(&request, &corrupt).is_err());
        Ok(())
    }

    #[test]
    fn workload_limits_match_the_small_broker() {
        assert!(
            Workload {
                connections: 9,
                frame_bytes: 4096,
                rounds: 1
            }
            .validate()
            .is_ok()
        );
        assert!(
            Workload {
                connections: 10,
                frame_bytes: 256,
                rounds: 1
            }
            .validate()
            .is_err()
        );
        assert!(
            Workload {
                connections: 1,
                frame_bytes: 8,
                rounds: 1
            }
            .validate()
            .is_err()
        );
        assert!(
            Workload {
                connections: 1,
                frame_bytes: 256,
                rounds: 0
            }
            .validate()
            .is_err()
        );
    }
}
