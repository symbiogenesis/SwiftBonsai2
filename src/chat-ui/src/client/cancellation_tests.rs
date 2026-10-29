//! Cancellation regressions use only inert loopback peers, never a model.
use super::*;
use std::{net::TcpListener, sync::mpsc};

#[test]
fn stop_cancels_stalled_headers_and_body_and_closes_the_connection() -> io::Result<()> {
    for response in [
        &b""[..],
        &b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 4096\r\n\r\ndata: "[..],
    ] {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let (accepted, ready) = mpsc::channel();
        let (closed, observed_close) = mpsc::channel();
        let server = thread::spawn(move || -> io::Result<()> {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(Duration::from_secs(3)))?;
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte)?;
                head.push(byte[0]);
            }
            stream.write_all(response)?;
            let _sent = accepted.send(());
            // Consume any request body still queued, then require closure.
            // Windows may reset an abandoned response instead of sending FIN.
            let mut remaining = Vec::new();
            match stream.read_to_end(&mut remaining) {
                Ok(_) => {}
                Err(error) if matches!(error.kind(), io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted) => {}
                Err(error) => return Err(error),
            }
            let _sent = closed.send(());
            Ok(())
        });
        let (commands, incoming) = mpsc::channel();
        let (events, _notifications) = event_channel();
        let (finished, result) = mpsc::channel();
        let worker = thread::spawn(move || {
            let outcome = generate(port, "inert-test-key", b"{}".to_vec(), &incoming, &events);
            let _sent = finished.send(outcome.err());
        });
        assert!(ready.recv_timeout(Duration::from_secs(3)).is_ok());
        assert!(commands.send(Command::Stop).is_ok());
        assert_eq!(result.recv_timeout(Duration::from_secs(3)), Ok(Some(Failure::Stopped)));
        assert!(observed_close.recv_timeout(Duration::from_secs(3)).is_ok());
        assert!(worker.join().is_ok());
        server.join().map_err(|_| io::Error::other("Loopback fixture panicked"))??;
    }
    Ok(())
}

#[test]
fn stop_wins_over_buffered_completion_and_connection_failure() {
    let answer = Message {
        role: "assistant",
        content: "Completed in a buffer".into(),
        reasoning_content: None,
    };
    for outcome in [
        Ok((answer, Completion::Finished)),
        Err(Failure::Message(LOCAL_CONNECTION_CLOSED)),
    ] {
        let (commands, incoming) = mpsc::channel();
        let (events, notifications) = event_channel();
        assert!(commands.send(Command::Stop).is_ok());
        let mut history = Vec::new();
        assert_eq!(
            finish_turn(
                &mut history,
                Vec::new(),
                "Original prompt",
                outcome,
                &events,
                &incoming
            ),
            Err(Failure::Stopped)
        );
        assert!(history.is_empty());
        assert!(
            matches!(notifications.try_recv(), Ok(Event::Stopping(text)) if text == "Original prompt")
        );
        assert!(notifications.try_recv().is_err());
    }
}

#[test]
fn stopped_retry_notice_waits_for_explicit_retry_without_failure_events() {
    let (commands, incoming) = mpsc::channel();
    let (events, notifications) = event_channel();
    assert!(commands.send(Command::Stop).is_ok());
    assert!(commands.send(Command::Retry).is_ok());
    assert!(wait_for_retry(
        &incoming,
        &events,
        &mut Vec::new(),
        &Event::Stopped
    ));
    assert!(matches!(notifications.try_recv(), Ok(Event::Stopped)));
    assert!(notifications.try_recv().is_err());
}
