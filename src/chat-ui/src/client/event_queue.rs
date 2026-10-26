//! Bounded worker-to-window delivery with cancellation-aware stream pressure.

use super::{Command, Event, Failure, IO_POLL, MAX_BODY, MAX_EVENT, Result};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, RecvTimeoutError, TryRecvError},
    },
    time::Instant,
};

const STREAM_EVENTS: usize = 64;
const CONTROL_RESERVE: usize = 16;
const COALESCE_BYTES: usize = 16 * 1024;

#[derive(Default)]
struct Queue {
    events: VecDeque<Event>,
    stream_bytes: usize,
    sender_closed: bool,
    receiver_closed: bool,
    #[cfg(test)]
    wait_started: Option<std::sync::mpsc::Sender<()>>,
}

/// The single inference worker's bounded notification endpoint.
pub(crate) struct EventSender(Arc<Mutex<Queue>>);

/// The native window's nonblocking notification endpoint.
pub(crate) struct EventReceiver(Arc<Mutex<Queue>>);

/// Create one FIFO; lifecycle events retain room when streaming is saturated.
pub(crate) fn channel() -> (EventSender, EventReceiver) {
    let shared = Arc::new(Mutex::new(Queue::default()));
    (EventSender(Arc::clone(&shared)), EventReceiver(shared))
}

const fn stream_bytes(event: &Event) -> usize {
    match event {
        Event::Text(text) | Event::Reasoning(text) => text.len(),
        _ => 0,
    }
}

impl EventSender {
    // A request has fewer than CONTROL_RESERVE lifecycle notifications. Another
    // request requires the window to consume its preceding completion/retry.
    // Unexpected exhaustion fails closed instead of blocking process teardown.
    pub(crate) fn send(&self, event: Event) -> std::result::Result<(), ()> {
        if matches!(event, Event::Text(_) | Event::Reasoning(_)) {
            return if self.try_stream(&mut Some(event)).map_err(|_| ())? {
                Ok(())
            } else {
                Err(())
            };
        }
        if matches!(&event, Event::Draft(text) | Event::RetainedAnswer(text) if text.len() > MAX_BODY)
        {
            return Err(());
        }
        let mut queue = self.0.lock().map_err(|_| ())?;
        if queue.receiver_closed || queue.events.len() >= STREAM_EVENTS + CONTROL_RESERVE {
            return Err(());
        }
        queue.events.push_back(event);
        drop(queue);
        Ok(())
    }

    fn try_stream(&self, event: &mut Option<Event>) -> Result<bool> {
        let mut queue = self.0.lock().map_err(|_| Failure::Shutdown)?;
        if queue.receiver_closed {
            return Err(Failure::Shutdown);
        }
        let bytes = event.as_ref().map_or(0, stream_bytes);
        if bytes > MAX_EVENT {
            return Err(Failure::Message("A streaming update exceeded its limit."));
        }
        if queue.stream_bytes + bytes > MAX_EVENT {
            return Ok(false);
        }
        let pending = event.as_ref();
        let tail = queue.events.back_mut();
        let combined = match (tail, pending) {
            (Some(Event::Text(tail)), Some(Event::Text(text)))
            | (Some(Event::Reasoning(tail)), Some(Event::Reasoning(text)))
                if tail.len() + text.len() <= COALESCE_BYTES =>
            {
                tail.push_str(text);
                true
            }
            _ => false,
        };
        if combined {
            queue.stream_bytes += bytes;
            drop(queue);
            *event = None;
            return Ok(true);
        }
        if queue.events.len() >= STREAM_EVENTS {
            return Ok(false);
        }
        if let Some(event) = event.take() {
            queue.stream_bytes += bytes;
            queue.events.push_back(event);
        }
        drop(queue);
        Ok(true)
    }

    pub(super) fn stream_until(
        &self,
        event: Event,
        commands: &Receiver<Command>,
        until: Instant,
    ) -> Result<()> {
        let mut event = Some(event);
        loop {
            super::check_commands(commands)?;
            if Instant::now() >= until {
                return Err(Failure::Message(
                    "The local model took longer than the ten-minute request limit.",
                ));
            }
            if self.try_stream(&mut event)? {
                return Ok(());
            }
            // The native window is behind. Do not read another socket byte or
            // generate more notifications; Stop/Shutdown wake this wait directly.
            #[cfg(test)]
            if let Ok(mut queue) = self.0.lock()
                && let Some(started) = queue.wait_started.take()
            {
                let _sent = started.send(());
            }
            let wait = until.saturating_duration_since(Instant::now()).min(IO_POLL);
            match commands.recv_timeout(wait) {
                Ok(Command::Stop) => return Err(Failure::Stopped),
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                    return Err(Failure::Shutdown);
                }
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

impl Drop for EventSender {
    fn drop(&mut self) {
        if let Ok(mut queue) = self.0.lock() {
            queue.sender_closed = true;
        }
    }
}

impl EventReceiver {
    pub(crate) fn try_recv(&self) -> std::result::Result<Event, TryRecvError> {
        let mut queue = self.0.lock().map_err(|_| TryRecvError::Disconnected)?;
        if let Some(event) = queue.events.pop_front() {
            queue.stream_bytes -= stream_bytes(&event);
            Ok(event)
        } else if queue.sender_closed {
            Err(TryRecvError::Disconnected)
        } else {
            Err(TryRecvError::Empty)
        }
    }

    #[cfg(test)]
    pub(crate) fn try_iter(&self) -> impl Iterator<Item = Event> + '_ {
        std::iter::from_fn(|| self.try_recv().ok())
    }
}

impl Drop for EventReceiver {
    fn drop(&mut self) {
        if let Ok(mut queue) = self.0.lock() {
            queue.receiver_closed = true;
            queue.events.clear();
            queue.stream_bytes = 0;
        }
    }
}

#[cfg(test)]
mod tests;
