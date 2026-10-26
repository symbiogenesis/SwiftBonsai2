//! Bounded, nonblocking input to a potentially stalled native speech worker.

use super::{AtomicU64, Command, Envelope, MAX_TEXT, Ordering};
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex, mpsc},
    time::Duration,
};

const MAX_COMMANDS: usize = 64;
// Neither finishing a stream nor cancelling input may need a vacant data slot.
const STREAM_COMMANDS: usize = MAX_COMMANDS - 2;
const MAX_BYTES: usize = 1024 * 1024;
const COALESCE_BYTES: usize = 16 * 1024;
pub(super) const OVERFLOW_MESSAGE: &str = "Read aloud stopped because its queued text or speech input exceeded the local limit. Text chat remains available.";

#[derive(Default)]
enum StreamState {
    #[default]
    Open,
    Finished,
    Failed,
}

#[derive(Default)]
struct Queue {
    commands: VecDeque<Envelope>,
    bytes: usize,
    stream_units: usize,
    stream: StreamState,
    sender_closed: bool,
    receiver_closed: bool,
}

type Shared = Arc<(Mutex<Queue>, Condvar)>;

pub(super) struct Sender {
    shared: Shared,
    #[cfg(test)]
    observed: Option<mpsc::Sender<Envelope>>,
}
pub(super) struct Receiver(Shared);

pub(super) fn channel() -> (Sender, Receiver) {
    let shared = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
    (
        Sender {
            shared: Arc::clone(&shared),
            #[cfg(test)]
            observed: None,
        },
        Receiver(shared),
    )
}

#[cfg(test)]
pub(super) fn fixture_channel() -> (Sender, mpsc::Receiver<Envelope>) {
    let (mut sender, _receiver) = channel();
    let (observed, incoming) = mpsc::channel();
    // UI interaction fixtures observe every issued command. Queue admission and
    // supersession have their own tests against channel(), without this hook.
    sender.observed = Some(observed);
    (sender, incoming)
}

const fn bytes(command: &Command) -> usize {
    match command {
        Command::Transcribe { wav, .. } => wav.len(),
        Command::Speak {
            text,
            voice_description,
        } => text.len() + voice_description.len(),
        Command::BeginStream { voice_description } => voice_description.len(),
        Command::AppendStream { text } => text.len(),
        _ => 0,
    }
}

const fn is_input(command: &Command) -> bool {
    matches!(command, Command::Transcribe { .. } | Command::StopListening)
}

impl Queue {
    fn retain(&mut self, keep: impl Fn(&Command) -> bool) {
        self.commands.retain(|envelope| keep(&envelope.command));
        self.bytes = self
            .commands
            .iter()
            .map(|envelope| bytes(&envelope.command))
            .sum();
    }

    fn overflow(&mut self, current: &AtomicU64) {
        // Cancel native startup/synthesis immediately, before the worker drains
        // its queue. Preserve pending input; this failure concerns read aloud.
        let generation = current.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        self.retain(is_input);
        self.stream = StreamState::Failed;
        self.commands.push_back(Envelope {
            command: Command::OutputOverflow,
            output_generation: generation,
        });
    }
}

impl Sender {
    pub(super) fn send(
        &self,
        envelope: Envelope,
        current: &AtomicU64,
    ) -> Result<(), mpsc::SendError<Envelope>> {
        #[cfg(test)]
        if let Some(observed) = &self.observed {
            return observed.send(envelope);
        }
        let Ok(mut queue) = self.shared.0.lock() else {
            return Err(mpsc::SendError(envelope));
        };
        if queue.receiver_closed || queue.sender_closed {
            return Err(mpsc::SendError(envelope));
        }
        match &envelope.command {
            Command::Shutdown => queue.retain(|_| false),
            Command::BeginStream { .. } | Command::Speak { .. } => {
                // Both commands invalidate input and all preceding output.
                queue.retain(|command| matches!(command, Command::Shutdown));
                queue.stream_units = 0;
                queue.stream = StreamState::Open;
            }
            Command::Transcribe { .. } => {
                queue.retain(|command| {
                    matches!(command, Command::StopListening | Command::Shutdown)
                });
            }
            Command::StopSpeaking => queue.retain(is_input),
            Command::StopListening => {
                queue.retain(|command| !is_input(command));
            }
            Command::AppendStream { text } => {
                if matches!(queue.stream, StreamState::Failed) {
                    return Ok(());
                }
                let units = text.encode_utf16().take(MAX_TEXT + 1).count();
                if matches!(queue.stream, StreamState::Finished)
                    || text.contains('\0')
                    || units > MAX_TEXT.saturating_sub(queue.stream_units)
                    || text.len() > MAX_BYTES - queue.bytes
                {
                    queue.overflow(current);
                    self.shared.1.notify_one();
                    return Ok(());
                }
                queue.stream_units += units;
                if let Some(Envelope {
                    command: Command::AppendStream { text: tail },
                    output_generation,
                }) = queue.commands.back_mut()
                    && *output_generation == envelope.output_generation
                    && tail.len() + text.len() <= COALESCE_BYTES
                {
                    tail.push_str(text);
                    queue.bytes += text.len();
                    return Ok(());
                }
            }
            Command::FinishStream => {
                if !matches!(queue.stream, StreamState::Open) {
                    return Ok(());
                }
                queue.stream = StreamState::Finished;
            }
            Command::OutputOverflow => {}
        }
        let payload_bytes = bytes(&envelope.command);
        if payload_bytes > MAX_BYTES {
            return Err(mpsc::SendError(envelope));
        }
        let command_limit = if matches!(envelope.command, Command::AppendStream { .. }) {
            STREAM_COMMANDS
        } else {
            MAX_COMMANDS
        };
        if queue.commands.len() >= command_limit || payload_bytes > MAX_BYTES - queue.bytes {
            if matches!(
                envelope.command,
                Command::AppendStream { .. } | Command::FinishStream
            ) {
                queue.overflow(current);
            } else {
                return Err(mpsc::SendError(envelope));
            }
        } else {
            if matches!(envelope.command, Command::Shutdown) {
                queue.sender_closed = true;
            }
            queue.bytes += payload_bytes;
            queue.commands.push_back(envelope);
        }
        drop(queue);
        self.shared.1.notify_one();
        Ok(())
    }
}

impl Receiver {
    pub(super) fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<Envelope, mpsc::RecvTimeoutError> {
        let (lock, wake) = &*self.0;
        let (mut queue, _) = wake
            .wait_timeout_while(
                lock.lock()
                    .map_err(|_| mpsc::RecvTimeoutError::Disconnected)?,
                timeout,
                |queue| queue.commands.is_empty() && !queue.sender_closed,
            )
            .map_err(|_| mpsc::RecvTimeoutError::Disconnected)?;
        let outcome = if let Some(envelope) = queue.commands.pop_front() {
            queue.bytes -= bytes(&envelope.command);
            Ok(envelope)
        } else if queue.sender_closed {
            Err(mpsc::RecvTimeoutError::Disconnected)
        } else {
            Err(mpsc::RecvTimeoutError::Timeout)
        };
        drop(queue);
        outcome
    }

    #[cfg(test)]
    pub(super) fn recv(&self) -> Result<Envelope, mpsc::RecvTimeoutError> {
        loop {
            match self.recv_timeout(Duration::from_secs(1)) {
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                outcome => return outcome,
            }
        }
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        if let Ok(mut queue) = self.shared.0.lock() {
            queue.sender_closed = true;
        }
        self.shared.1.notify_one();
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        if let Ok(mut queue) = self.0.0.lock() {
            queue.receiver_closed = true;
            queue.commands.clear();
            queue.bytes = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::speech::{Commands, NO_INPUT};
    use std::io;

    fn fixture() -> (Commands, Receiver) {
        let (sender, receiver) = channel();
        (
            Commands {
                sender,
                current_output: Arc::new(AtomicU64::new(0)),
                current_input: Arc::new(AtomicU64::new(NO_INPUT)),
            },
            receiver,
        )
    }

    fn begin(commands: &Commands) -> io::Result<()> {
        commands
            .send(Command::BeginStream {
                voice_description: "Warm".to_owned(),
            })
            .map_err(io::Error::other)
    }

    #[test]
    fn stalled_speech_worker_coalesces_and_preserves_accepted_text() -> io::Result<()> {
        let (commands, receiver) = fixture();
        begin(&commands)?;
        for _ in 0..MAX_TEXT {
            commands
                .send(Command::AppendStream {
                    text: "x".to_owned(),
                })
                .map_err(io::Error::other)?;
        }
        commands
            .send(Command::FinishStream)
            .map_err(io::Error::other)?;
        {
            let queue = receiver
                .0
                .0
                .lock()
                .map_err(|_| io::Error::other("Queue poisoned"))?;
            assert_eq!(queue.commands.len(), 4);
            assert_eq!(queue.bytes, MAX_TEXT + 4);
            assert!(queue.commands.len() <= MAX_COMMANDS && queue.bytes <= MAX_BYTES);
            drop(queue);
        }
        assert!(matches!(
            receiver.recv().map_err(io::Error::other)?.command,
            Command::BeginStream { .. }
        ));
        let mut text = String::new();
        for _ in 0..2 {
            let Command::AppendStream { text: part } =
                receiver.recv().map_err(io::Error::other)?.command
            else {
                return Err(io::Error::other("Expected accepted text"));
            };
            text.push_str(&part);
        }
        assert_eq!(text, "x".repeat(MAX_TEXT));
        assert!(matches!(
            receiver.recv().map_err(io::Error::other)?.command,
            Command::FinishStream
        ));
        Ok(())
    }

    #[test]
    fn speech_overflow_cancels_inflight_generation_and_reports_once_then_recovers() -> io::Result<()>
    {
        let (commands, receiver) = fixture();
        begin(&commands)?;
        let previous = receiver.recv().map_err(io::Error::other)?.output_generation;
        commands
            .send(Command::AppendStream {
                text: "🙂".repeat(MAX_TEXT / 2),
            })
            .map_err(io::Error::other)?;
        commands
            .send(Command::AppendStream {
                text: "x".to_owned(),
            })
            .map_err(io::Error::other)?;
        let failed = receiver.recv().map_err(io::Error::other)?;
        assert!(matches!(failed.command, Command::OutputOverflow));
        assert_ne!(failed.output_generation, previous);
        assert_eq!(
            failed.output_generation,
            commands.current_output.load(Ordering::Acquire)
        );
        commands
            .send(Command::AppendStream {
                text: "late".to_owned(),
            })
            .map_err(io::Error::other)?;
        commands
            .send(Command::FinishStream)
            .map_err(io::Error::other)?;
        assert!(matches!(
            receiver.recv_timeout(Duration::ZERO),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        begin(&commands)?;
        commands
            .send(Command::AppendStream {
                text: "Recovered".to_owned(),
            })
            .map_err(io::Error::other)?;
        assert!(matches!(
            receiver.recv().map_err(io::Error::other)?.command,
            Command::BeginStream { .. }
        ));
        assert!(
            matches!(receiver.recv().map_err(io::Error::other)?.command, Command::AppendStream { text } if text == "Recovered")
        );
        Ok(())
    }

    #[test]
    fn speech_stop_and_shutdown_release_saturated_input_without_dropping_newer_work()
    -> io::Result<()> {
        let (commands, receiver) = fixture();
        begin(&commands)?;
        for _ in 0..MAX_COMMANDS {
            commands
                .send(Command::AppendStream {
                    text: "x".to_owned(),
                })
                .map_err(io::Error::other)?;
            commands
                .send(Command::StopListening)
                .map_err(io::Error::other)?;
        }
        {
            let queue = receiver
                .0
                .0
                .lock()
                .map_err(|_| io::Error::other("Queue poisoned"))?;
            assert!(queue.commands.len() <= MAX_COMMANDS);
            assert!(
                queue
                    .commands
                    .iter()
                    .any(|envelope| matches!(envelope.command, Command::OutputOverflow))
            );
            drop(queue);
        }
        commands
            .send(Command::StopSpeaking)
            .map_err(io::Error::other)?;
        begin(&commands)?;
        commands
            .send(Command::AppendStream {
                text: "Newer".to_owned(),
            })
            .map_err(io::Error::other)?;
        assert!(matches!(
            receiver.recv().map_err(io::Error::other)?.command,
            Command::BeginStream { .. }
        ));
        assert!(
            matches!(receiver.recv().map_err(io::Error::other)?.command, Command::AppendStream { text } if text == "Newer")
        );
        commands.send(Command::Shutdown).map_err(io::Error::other)?;
        assert!(matches!(
            receiver.recv().map_err(io::Error::other)?.command,
            Command::Shutdown
        ));
        assert!(matches!(
            receiver.recv_timeout(Duration::ZERO),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        Ok(())
    }

    #[test]
    fn speech_finish_and_stop_listening_use_reserved_slots_without_cancelling_text()
    -> io::Result<()> {
        let (commands, receiver) = fixture();
        begin(&commands)?;
        for _ in 0..STREAM_COMMANDS - 2 {
            commands
                .send(Command::AppendStream {
                    text: "x".to_owned(),
                })
                .map_err(io::Error::other)?;
            commands
                .send(Command::StopListening)
                .map_err(io::Error::other)?;
        }
        {
            let queue = receiver
                .0
                .0
                .lock()
                .map_err(|_| io::Error::other("Queue poisoned"))?;
            assert_eq!(queue.commands.len(), STREAM_COMMANDS);
            drop(queue);
        }
        let generation = commands.current_output.load(Ordering::Acquire);
        commands
            .send(Command::FinishStream)
            .map_err(io::Error::other)?;
        commands
            .send(Command::StopListening)
            .map_err(io::Error::other)?;
        let queue = receiver
            .0
            .0
            .lock()
            .map_err(|_| io::Error::other("Queue poisoned"))?;
        assert!(queue.commands.len() <= MAX_COMMANDS);
        assert!(
            queue
                .commands
                .iter()
                .any(|envelope| matches!(envelope.command, Command::FinishStream))
        );
        assert!(
            !queue
                .commands
                .iter()
                .any(|envelope| matches!(envelope.command, Command::OutputOverflow))
        );
        assert_eq!(commands.current_output.load(Ordering::Acquire), generation);
        drop(queue);
        Ok(())
    }

    #[test]
    fn speech_payload_limit_rejects_oversized_audio_before_storage() {
        let (commands, receiver) = fixture();
        assert!(
            commands
                .send(Command::Transcribe {
                    generation: 7,
                    wav: vec![0; MAX_BYTES + 1]
                })
                .is_err()
        );
        assert!(matches!(
            receiver.recv_timeout(Duration::ZERO),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
    }

    #[test]
    fn speech_overload_reaches_the_window_as_a_current_generation_failure() -> io::Result<()> {
        let session = crate::speech::start(None, None)?;
        begin(&session.commands)?;
        session
            .commands
            .send(Command::AppendStream {
                text: "invalid\0text".to_owned(),
            })
            .map_err(io::Error::other)?;
        let mut reported = false;
        for _ in 0..8 {
            if let crate::speech::Event::Failed {
                generation,
                message,
            } = session
                .events
                .recv_timeout(Duration::from_secs(2))
                .map_err(io::Error::other)?
                && session.is_current_output(generation)
            {
                assert_eq!(message, OVERFLOW_MESSAGE);
                reported = true;
                break;
            }
        }
        assert!(reported);
        Ok(())
    }
}
