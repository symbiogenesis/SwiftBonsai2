//! Real loopback transport regressions; no model or external network is used.
#![expect(
    clippy::unwrap_used,
    reason = "test setup and assertions must fail loudly"
)]

use bonsai_broker::{
    backend::BackendError,
    scheduler::GateError,
    service::{LocalChatService, ServiceError},
    supervisor::{BackendConnector, ServeError},
    transport::LoopbackBackend,
};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::{LocalSet, spawn_local},
    time::{Instant, timeout},
};

const FRONT_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const BACK_KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn run(test: impl Future<Output = ()>) {
    run_with_timeout(test, Duration::from_secs(10));
}

fn run_with_timeout(test: impl Future<Output = ()>, limit: Duration) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(LocalSet::new().run_until(async {
            timeout(limit, test).await.unwrap();
        }));
}

async fn listener() -> TcpListener {
    TcpListener::bind("127.0.0.1:0").await.unwrap()
}

async fn service(backend: &TcpListener, waiting: usize) -> Arc<LocalChatService> {
    Arc::new(
        LocalChatService::bind(
            0,
            FRONT_KEY,
            backend.local_addr().unwrap().port(),
            BACK_KEY,
            waiting,
        )
        .await
        .unwrap(),
    )
}

#[test]
#[cfg(windows)]
fn frontend_retains_exclusive_loopback_address_ownership() {
    run(async {
        let backend = listener().await;
        let service = service(&backend, 0).await;
        let competing = tokio::net::TcpSocket::new_v4().unwrap();
        competing.set_reuseaddr(true).unwrap();
        let address = std::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, service.port());
        assert!(competing.bind(address.into()).is_err());
        // Exclusivity does not change numeric-loopback reachability.
        let client = TcpStream::connect(address).await.unwrap();
        let accepted = service.accept().await.unwrap();
        drop(accepted);
        drop(client);
    });
}

async fn send_request(client: &mut TcpStream, port: u16) {
    let body = br#"{"messages":[{"role":"user","content":"A rainy scene."}],"max_completion_tokens":128,"stream":false}"#;
    let head = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nAuthorization: Bearer {FRONT_KEY}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    client.write_all(head.as_bytes()).await.unwrap();
    client.write_all(body).await.unwrap();
}

async fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let mut raw = Vec::new();
    loop {
        assert!(raw.len() <= 2 * 1024 * 1024);
        let mut buffer = [0; 4096];
        let count = stream.read(&mut buffer).await.unwrap();
        assert_ne!(count, 0);
        raw.extend_from_slice(&buffer[..count]);
        if let Some(end) = raw.windows(4).position(|part| part == b"\r\n\r\n") {
            let head = String::from_utf8(raw[..end + 4].to_vec()).unwrap();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .unwrap()
                .parse()
                .unwrap();
            if raw.len() >= end + 4 + length {
                return (head, raw[end + 4..end + 4 + length].to_vec());
            }
        }
    }
}

async fn respond(stream: &mut TcpStream, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(body).await.unwrap();
}

async fn count(listener: &TcpListener) -> (String, Vec<u8>) {
    let (mut stream, _) = listener.accept().await.unwrap();
    let request = read_request(&mut stream).await;
    respond(
        &mut stream,
        br#"{"input_tokens":100,"object":"response.input_tokens"}"#,
    )
    .await;
    request
}

#[test]
fn real_loopback_count_and_generation_preserve_body_and_private_headers() {
    run(async {
        let backend = listener().await;
        let service = service(&backend, 1).await;
        let worker = Arc::clone(&service);
        let serving = spawn_local(async move { worker.accept().await.unwrap().serve().await });
        let backend_work = spawn_local(async move {
            let counted = count(&backend).await;
            let (mut stream, _) = backend.accept().await.unwrap();
            let generated = read_request(&mut stream).await;
            respond(&mut stream, br#"{"choices":[]}"#).await;
            (counted, generated)
        });
        let mut client = TcpStream::connect(("127.0.0.1", service.port()))
            .await
            .unwrap();
        send_request(&mut client, service.port()).await;
        let mut response = Vec::new();
        client.read_to_end(&mut response).await.unwrap();
        assert_eq!(serving.await.unwrap(), Ok(()));
        let ((count_head, count_body), (generation_head, generation_body)) =
            backend_work.await.unwrap();
        assert_eq!(count_body, generation_body);
        assert!(count_head.starts_with("POST /v1/chat/completions/input_tokens HTTP/1.1\r\n"));
        assert!(generation_head.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
        for head in [&count_head, &generation_head] {
            assert!(head.contains(&format!("Authorization: Bearer {BACK_KEY}\r\n")));
            assert!(!head.contains(FRONT_KEY));
        }
        assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert!(response.ends_with(br#"{"choices":[]}"#));
    });
}

#[test]
fn accepted_capacity_is_reserved_before_a_second_socket_or_body() {
    run(async {
        let backend = listener().await;
        let service = service(&backend, 0).await;
        assert_eq!(service.capacity(), 1);
        let _client = TcpStream::connect(("127.0.0.1", service.port()))
            .await
            .unwrap();
        let accepted = service.accept().await.unwrap();
        assert!(matches!(
            service.accept().await,
            Err(ServiceError::Gate(GateError::Capacity))
        ));
        drop(accepted);
        service.close();
        assert_eq!(service.capacity(), 1);
        assert!(matches!(
            service.accept().await,
            Err(ServiceError::Gate(GateError::Closed))
        ));
    });
}

#[test]
fn queued_socket_expires_without_reading_a_request_or_connecting_backend() {
    run_with_timeout(
        async {
            let backend = listener().await;
            let service = service(&backend, 1).await;
            assert_eq!(service.capacity(), 2);
            let worker = Arc::clone(&service);
            let active = spawn_local(async move { worker.accept().await.unwrap().serve().await });
            let mut first_client = TcpStream::connect(("127.0.0.1", service.port()))
                .await
                .unwrap();
            send_request(&mut first_client, service.port()).await;
            count(&backend).await;
            let (mut generation, _) = backend.accept().await.unwrap();
            read_request(&mut generation).await;

            let mut queued_client = TcpStream::connect(("127.0.0.1", service.port()))
                .await
                .unwrap();
            // No headers or body are supplied: queue expiry must independently
            // close an accepted socket while a prior generation owns the slot.
            let worker = Arc::clone(&service);
            let (accepted_tx, accepted_rx) = oneshot::channel();
            let queued = spawn_local(async move {
                let accepted = worker.accept().await.unwrap();
                accepted_tx.send(()).unwrap();
                accepted.serve().await
            });
            accepted_rx.await.unwrap();
            tokio::task::yield_now().await;
            tokio::time::pause();
            tokio::time::advance(bonsai_broker::service::QUEUE_TIMEOUT).await;
            assert_eq!(
                queued.await.unwrap(),
                Err(ServiceError::Gate(GateError::Deadline))
            );
            tokio::time::resume();
            let mut byte = [0];
            assert_eq!(queued_client.read(&mut byte).await.unwrap(), 0);
            assert!(
                timeout(Duration::from_millis(20), backend.accept())
                    .await
                    .is_err()
            );
            service.close();
            assert_eq!(
                active.await.unwrap(),
                Err(ServiceError::Gate(GateError::Closed))
            );
            assert_eq!(generation.read(&mut byte).await.unwrap(), 0);
        },
        bonsai_broker::service::QUEUE_TIMEOUT + Duration::from_secs(10),
    );
}

#[test]
fn disconnect_during_generation_closes_backend_and_latches_service() {
    run(async {
        let backend = listener().await;
        let service = service(&backend, 1).await;
        let worker = Arc::clone(&service);
        let serving = spawn_local(async move { worker.accept().await.unwrap().serve().await });
        let (started_tx, started_rx) = oneshot::channel();
        let backend_work = spawn_local(async move {
            count(&backend).await;
            let (mut stream, _) = backend.accept().await.unwrap();
            read_request(&mut stream).await;
            started_tx.send(()).unwrap();
            let mut byte = [0];
            assert_eq!(stream.read(&mut byte).await.unwrap(), 0);
        });
        let mut client = TcpStream::connect(("127.0.0.1", service.port()))
            .await
            .unwrap();
        send_request(&mut client, service.port()).await;
        started_rx.await.unwrap();
        drop(client);
        assert_eq!(
            serving.await.unwrap(),
            Err(ServiceError::Request(ServeError::ClientAbandoned))
        );
        backend_work.await.unwrap();
        assert!(matches!(
            service.accept().await,
            Err(ServiceError::Gate(GateError::Closed))
        ));
    });
}

#[test]
fn caller_cancellation_during_generation_latches_service() {
    run(async {
        let backend = listener().await;
        let service = service(&backend, 1).await;
        let worker = Arc::clone(&service);
        let serving = spawn_local(async move { worker.accept().await.unwrap().serve().await });
        let (started_tx, started_rx) = oneshot::channel();
        let backend_work = spawn_local(async move {
            count(&backend).await;
            let (mut stream, _) = backend.accept().await.unwrap();
            read_request(&mut stream).await;
            started_tx.send(()).unwrap();
            let mut byte = [0];
            assert_eq!(stream.read(&mut byte).await.unwrap(), 0);
        });
        let mut client = TcpStream::connect(("127.0.0.1", service.port()))
            .await
            .unwrap();
        send_request(&mut client, service.port()).await;
        started_rx.await.unwrap();
        serving.abort();
        assert!(serving.await.unwrap_err().is_cancelled());
        backend_work.await.unwrap();
        assert!(matches!(
            service.accept().await,
            Err(ServiceError::Gate(GateError::Closed))
        ));
    });
}

#[test]
fn shutdown_wakes_pending_accept() {
    run(async {
        let backend = listener().await;
        let service = service(&backend, 1).await;
        let worker = Arc::clone(&service);
        let accepting = spawn_local(async move { worker.accept().await.map(|_| ()) });
        tokio::task::yield_now().await;
        service.close();
        assert_eq!(
            accepting.await.unwrap(),
            Err(ServiceError::Gate(GateError::Closed))
        );
    });
}

#[test]
fn shutdown_during_generation_drops_both_sockets() {
    run(async {
        let backend = listener().await;
        let service = service(&backend, 1).await;
        let worker = Arc::clone(&service);
        let serving = spawn_local(async move { worker.accept().await.unwrap().serve().await });
        let (started_tx, started_rx) = oneshot::channel();
        let backend_work = spawn_local(async move {
            count(&backend).await;
            let (mut stream, _) = backend.accept().await.unwrap();
            read_request(&mut stream).await;
            started_tx.send(()).unwrap();
            let mut byte = [0];
            assert_eq!(stream.read(&mut byte).await.unwrap(), 0);
        });
        let mut client = TcpStream::connect(("127.0.0.1", service.port()))
            .await
            .unwrap();
        send_request(&mut client, service.port()).await;
        started_rx.await.unwrap();
        service.close();
        assert_eq!(
            serving.await.unwrap(),
            Err(ServiceError::Gate(GateError::Closed))
        );
        backend_work.await.unwrap();
        let mut byte = [0];
        assert_eq!(client.read(&mut byte).await.unwrap(), 0);
    });
}

#[test]
fn invalid_configuration_does_not_open_a_listener() {
    run(async {
        for (port, front, back_port, back, waiting) in [
            (0, FRONT_KEY, 8081, FRONT_KEY, 0),
            (8081, FRONT_KEY, 8081, BACK_KEY, 0),
            (80, FRONT_KEY, 8081, BACK_KEY, 0),
            (0, "invalid", 8081, BACK_KEY, 0),
            (0, FRONT_KEY, 80, BACK_KEY, 0),
            (0, FRONT_KEY, 8081, BACK_KEY, 9),
        ] {
            assert!(
                LocalChatService::bind(port, front, back_port, back, waiting)
                    .await
                    .is_err()
            );
        }
    });
}

#[test]
fn expired_connector_deadline_never_connects() {
    run(async {
        let backend = listener().await;
        let connector =
            LoopbackBackend::new(backend.local_addr().unwrap().port(), BACK_KEY).unwrap();
        assert!(matches!(
            connector.connect(Instant::now()).await,
            Err(BackendError::Deadline)
        ));
        assert!(
            timeout(Duration::from_millis(20), backend.accept())
                .await
                .is_err()
        );
    });
}
