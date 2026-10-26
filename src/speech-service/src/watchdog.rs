//! Process-local inference cancellation; never applies to the caller's UI Job.

use std::{
    io,
    net::TcpStream,
    sync::mpsc::{self, Sender, TryRecvError},
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
                loop {
                    match incoming.try_recv() {
                        Ok(()) | Err(TryRecvError::Disconnected) => return,
                        Err(TryRecvError::Empty) => {}
                    }
                    if Instant::now() >= until {
                        abort_companion();
                    }
                    match peer.peek(&mut byte) {
                        Err(error)
                            if matches!(
                                error.kind(),
                                io::ErrorKind::WouldBlock
                                    | io::ErrorKind::TimedOut
                                    | io::ErrorKind::Interrupted
                            ) => {}
                        Ok(_) | Err(_) => abort_companion(),
                    }
                }
            })?;
        Ok(Self {
            stopped,
            worker: Some(worker),
        })
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
