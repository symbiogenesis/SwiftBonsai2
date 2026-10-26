//! Queue accounting and cancellation fixtures; no inference or timing claims.

use super::*;
use crate::client::{Completion, Message};
use std::{sync::mpsc, thread, time::Duration};

fn fill(sender: &EventSender) -> Result<()> {
    for index in 0..STREAM_EVENTS {
        let mut event = Some(if index % 2 == 0 {
            Event::Text("x".into())
        } else {
            Event::Reasoning("y".into())
        });
        assert!(sender.try_stream(&mut event)?);
        assert!(event.is_none());
    }
    Ok(())
}

#[test]
fn stream_entries_and_payload_are_bounded_and_lifecycle_order_is_retained() -> Result<()> {
    let (sender, receiver) = channel();
    fill(&sender)?;
    let mut pending = Some(Event::Text("pending".into()));
    assert!(!sender.try_stream(&mut pending)?);
    assert!(pending.is_some());
    sender
        .send(Event::Stage("Review"))
        .map_err(|()| Failure::Shutdown)?;
    sender
        .send(Event::Draft("Retained draft".into()))
        .map_err(|()| Failure::Shutdown)?;
    sender
        .send(Event::Committed(vec![Message {
            role: "assistant",
            content: "Committed".into(),
            reasoning_content: None,
        }]))
        .map_err(|()| Failure::Shutdown)?;
    sender
        .send(Event::Complete(Completion::Finished))
        .map_err(|()| Failure::Shutdown)?;
    assert_eq!(
        receiver.try_iter().take(STREAM_EVENTS).count(),
        STREAM_EVENTS
    );
    assert!(matches!(receiver.try_recv(), Ok(Event::Stage("Review"))));
    assert!(matches!(receiver.try_recv(), Ok(Event::Draft(text)) if text == "Retained draft"));
    assert!(
        matches!(receiver.try_recv(), Ok(Event::Committed(history)) if history[0].content == "Committed")
    );
    assert!(matches!(
        receiver.try_recv(),
        Ok(Event::Complete(Completion::Finished))
    ));
    assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));

    let mut maximum = Some(Event::Text("x".repeat(MAX_EVENT)));
    assert!(sender.try_stream(&mut maximum)?);
    let mut surplus = Some(Event::Reasoning("y".into()));
    assert!(!sender.try_stream(&mut surplus)?);
    assert!(receiver.try_recv().is_ok());
    assert!(sender.try_stream(&mut surplus)?);
    Ok(())
}

#[test]
fn tiny_deltas_coalesce_without_crossing_kind_or_lifecycle_boundaries() -> Result<()> {
    let (sender, receiver) = channel();
    for _ in 0..=(COALESCE_BYTES * 2) {
        assert!(sender.try_stream(&mut Some(Event::Text("x".into())))?);
    }
    assert!(sender.try_stream(&mut Some(Event::Reasoning("why".into())))?);
    sender
        .send(Event::Stage("New stage"))
        .map_err(|()| Failure::Shutdown)?;
    assert!(sender.try_stream(&mut Some(Event::Text("new".into())))?);
    let events: Vec<_> = receiver.try_iter().collect();
    assert_eq!(events.len(), 6);
    for (event, expected) in events[..3].iter().zip([COALESCE_BYTES, COALESCE_BYTES, 1]) {
        assert!(matches!(event, Event::Text(text) if text.len() == expected));
    }
    assert!(matches!(&events[3], Event::Reasoning(text) if text == "why"));
    assert!(matches!(events[4], Event::Stage("New stage")));
    assert!(matches!(&events[5], Event::Text(text) if text == "new"));
    Ok(())
}

#[test]
fn generic_send_keeps_stream_bounds_and_retained_answers_have_a_separate_body_limit() -> Result<()>
{
    let (sender, receiver) = channel();
    sender
        .send(Event::Text("x".repeat(MAX_EVENT)))
        .map_err(|()| Failure::Shutdown)?;
    assert_eq!(sender.send(Event::Text("surplus".into())), Err(()));
    sender
        .send(Event::RetainedAnswer("r".repeat(MAX_BODY)))
        .map_err(|()| Failure::Shutdown)?;
    sender
        .send(Event::Complete(Completion::TokenLimit))
        .map_err(|()| Failure::Shutdown)?;
    assert!(matches!(receiver.try_recv(), Ok(Event::Text(text)) if text.len() == MAX_EVENT));
    assert!(
        matches!(receiver.try_recv(), Ok(Event::RetainedAnswer(text)) if text.len() == MAX_BODY)
    );
    assert!(matches!(
        receiver.try_recv(),
        Ok(Event::Complete(Completion::TokenLimit))
    ));
    assert!(sender.send(Event::Text("x".repeat(MAX_EVENT + 1))).is_err());
    assert!(
        sender
            .send(Event::RetainedAnswer("x".repeat(MAX_BODY + 1)))
            .is_err()
    );
    assert!(sender.send(Event::Draft("x".repeat(MAX_BODY + 1))).is_err());
    assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    Ok(())
}

#[test]
fn saturated_delivery_remains_cancellable_and_terminal_notifications_fit() -> Result<()> {
    for shutdown in [false, true] {
        let (sender, receiver) = channel();
        fill(&sender)?;
        let (commands, incoming) = mpsc::channel();
        let (finished, result) = mpsc::channel();
        let (started, waiting) = mpsc::channel();
        sender.0.lock().map_err(|_| Failure::Shutdown)?.wait_started = Some(started);
        let worker = thread::spawn(move || {
            let outcome = sender.stream_until(
                Event::Text("surplus".into()),
                &incoming,
                Instant::now() + Duration::from_secs(10),
            );
            if !shutdown {
                assert!(
                    sender
                        .send(Event::Stopping("Original prompt".into()))
                        .is_ok()
                );
                assert!(sender.send(Event::Stopped).is_ok());
            }
            assert!(finished.send(outcome).is_ok());
        });
        assert!(waiting.recv_timeout(Duration::from_secs(3)).is_ok());
        assert!(
            commands
                .send(if shutdown {
                    Command::Shutdown
                } else {
                    Command::Stop
                })
                .is_ok()
        );
        assert_eq!(
            result.recv_timeout(Duration::from_secs(3)),
            Ok(Err(if shutdown {
                Failure::Shutdown
            } else {
                Failure::Stopped
            }))
        );
        assert!(worker.join().is_ok());
        assert_eq!(
            receiver.try_iter().take(STREAM_EVENTS).count(),
            STREAM_EVENTS
        );
        if !shutdown {
            assert!(
                matches!(receiver.try_recv(), Ok(Event::Stopping(text)) if text == "Original prompt")
            );
            assert!(matches!(receiver.try_recv(), Ok(Event::Stopped)));
        }
        assert!(matches!(
            receiver.try_recv(),
            Err(TryRecvError::Disconnected)
        ));
    }
    Ok(())
}

#[test]
fn backpressured_delivery_resumes_with_exact_fifo_bytes_after_consumption() -> Result<()> {
    let (sender, receiver) = channel();
    fill(&sender)?;
    let (_commands, incoming) = mpsc::channel();
    let (started, waiting) = mpsc::channel();
    sender.0.lock().map_err(|_| Failure::Shutdown)?.wait_started = Some(started);
    let (finished, outcome) = mpsc::channel();
    let worker = thread::spawn(move || {
        let result = sender.stream_until(
            Event::Text("resumed".into()),
            &incoming,
            Instant::now() + Duration::from_secs(10),
        );
        assert!(finished.send(result).is_ok());
    });
    assert!(waiting.recv_timeout(Duration::from_secs(3)).is_ok());
    assert!(matches!(receiver.try_recv(), Ok(Event::Text(text)) if text == "x"));
    assert_eq!(outcome.recv_timeout(Duration::from_secs(3)), Ok(Ok(())));
    assert!(worker.join().is_ok());
    let events: Vec<_> = receiver.try_iter().collect();
    assert_eq!(events.len(), STREAM_EVENTS);
    for (index, event) in events[..STREAM_EVENTS - 1].iter().enumerate() {
        if index % 2 == 0 {
            assert!(matches!(event, Event::Reasoning(text) if text == "y"));
        } else {
            assert!(matches!(event, Event::Text(text) if text == "x"));
        }
    }
    assert!(matches!(&events[STREAM_EVENTS - 1], Event::Text(text) if text == "resumed"));
    Ok(())
}

#[test]
fn saturation_deadline_closed_consumer_and_control_overflow_fail_without_waiting() -> Result<()> {
    let (sender, receiver) = channel();
    fill(&sender)?;
    let (_commands, incoming) = mpsc::channel();
    assert!(matches!(
        sender.stream_until(Event::Text("surplus".into()), &incoming, Instant::now()),
        Err(Failure::Message(_))
    ));
    for _ in 0..CONTROL_RESERVE {
        sender
            .send(Event::Stage("Reserved"))
            .map_err(|()| Failure::Shutdown)?;
    }
    assert_eq!(sender.send(Event::Stopped), Err(()));
    drop(receiver);
    assert_eq!(
        sender.stream_until(
            Event::Text("surplus".into()),
            &incoming,
            Instant::now() + Duration::from_secs(10)
        ),
        Err(Failure::Shutdown)
    );
    Ok(())
}

#[test]
fn saturated_delivery_expires_at_the_request_deadline() -> Result<()> {
    let (sender, receiver) = channel();
    fill(&sender)?;
    let (_commands, incoming) = mpsc::channel();
    assert!(matches!(
        sender.stream_until(
            Event::Text("surplus".into()),
            &incoming,
            Instant::now() + Duration::from_millis(1)
        ),
        Err(Failure::Message(_))
    ));
    assert_eq!(receiver.try_iter().count(), STREAM_EVENTS);
    Ok(())
}
