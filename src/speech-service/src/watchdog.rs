//! Process-local inference cancellation; never applies to the caller's UI Job.

use std::{
    io,
    net::TcpStream,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(super) struct Watchdog {
    stopped: Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl Watchdog {
    pub(super) fn start(peer: &TcpStream, limit: Duration) -> io::Result<Self> {
        let peer = peer.try_clone()?;
        peer.set_read_timeout(Some(Duration::from_millis(25)))?;
        let until = Instant::now() + limit;
        let (stopped, incoming) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("bonsai-speech-deadline".to_owned())
            .spawn(move || {
                let mut byte = [0];
                if monitor(&incoming, until, || peer.peek(&mut byte)) {
                    abort_companion();
                }
            })?;
        Ok(Self {
            stopped,
            worker: Some(worker),
        })
    }
}

fn stopped(incoming: &Receiver<()>) -> bool {
    matches!(
        incoming.try_recv(),
        Ok(()) | Err(TryRecvError::Disconnected)
    )
}

fn monitor(
    incoming: &Receiver<()>,
    until: Instant,
    mut peek: impl FnMut() -> io::Result<usize>,
) -> bool {
    loop {
        if stopped(incoming) {
            return false;
        }
        if Instant::now() >= until {
            return true;
        }
        let event = peek();
        // Inference may complete while the socket wait is pending. Disarming
        // owns that completed call even if the peer closes at the same time.
        if stopped(incoming) {
            return false;
        }
        match event {
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Ok(_) | Err(_) => return true,
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        let _sent = self.stopped.send(());
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}

#[allow(
    clippy::exit,
    reason = "This library runs only inside an independently Job-owned speech companion; blocking native inference cannot otherwise be cancelled safely. Exit closes only this companion and never the UI or Swift owner."
)]
fn abort_companion() -> ! {
    std::process::exit(70)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_inference_stop_wins_over_eof_from_a_pending_peek() -> io::Result<()> {
        let (stop, incoming) = mpsc::channel();
        let (entered, pending) = mpsc::sync_channel(0);
        let (release, blocked) = mpsc::channel();
        let worker = thread::spawn(move || {
            monitor(&incoming, Instant::now() + Duration::from_secs(10), || {
                entered.send(()).map_err(io::Error::other)?;
                blocked.recv().map_err(io::Error::other)?;
                Ok(0)
            })
        });
        pending
            .recv_timeout(Duration::from_secs(3))
            .map_err(io::Error::other)?;
        stop.send(()).map_err(io::Error::other)?;
        release.send(()).map_err(io::Error::other)?;
        assert!(
            !worker
                .join()
                .map_err(|_| io::Error::other("Watchdog fixture failed."))?
        );
        Ok(())
    }
}
