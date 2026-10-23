//! Development runner regressions use synthetic loopback backends only.
#![expect(
    clippy::unwrap_used,
    reason = "test setup and assertions must fail loudly"
)]

use bonsai_broker::{
    runtime::{ConfigurationError, DevelopmentConfig, RunExit, run},
    scheduler::GateError,
    service::{LocalChatService, ServiceError},
};
use std::{
    ffi::OsString,
    fs,
    future::{Future, pending},
    io::ErrorKind,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::{LocalSet, spawn_local},
    time::timeout,
};

const FRONT_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const BACK_KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn check(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(LocalSet::new().run_until(async {
            timeout(Duration::from_secs(10), test).await.unwrap();
        }));
}

async fn setup(waiting: usize) -> (TcpListener, Arc<LocalChatService>) {
    let backend = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let service = LocalChatService::bind(
        0,
        FRONT_KEY,
        backend.local_addr().unwrap().port(),
        BACK_KEY,
        waiting,
    )
    .await
    .unwrap();
    (backend, Arc::new(service))
}

async fn request(port: u16) -> TcpStream {
    let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let body = br#"{"messages":[{"role":"user","content":"A scene."}],"stream":false}"#;
    let head = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {FRONT_KEY}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    client.write_all(head.as_bytes()).await.unwrap();
    client.write_all(body).await.unwrap();
    client
}

async fn backend_request(stream: &mut TcpStream) {
    let mut bytes = Vec::new();
    loop {
        let mut buffer = [0; 4096];
        let size = stream.read(&mut buffer).await.unwrap();
        assert_ne!(size, 0);
        bytes.extend_from_slice(&buffer[..size]);
        assert!(bytes.len() < 16 * 1024);
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let head = std::str::from_utf8(&bytes[..end]).unwrap();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .unwrap()
                .parse()
                .unwrap();
            if bytes.len() >= end + 4 + length {
                return;
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

#[test]
fn shutdown_drains_active_and_waiting_requests_at_the_exact_capacity() {
    check(async {
        let (backend, service) = setup(2).await;
        let mut clients = Vec::new();
        for _ in 0..4 {
            clients.push(request(service.port()).await);
        }
        let (stop, stopped) = oneshot::channel();
        let worker = Arc::clone(&service);
        let running = spawn_local(async move {
            run(&worker, async {
                stopped.await.unwrap();
                Ok(())
            })
            .await
        });
        let (mut counting, _) = backend.accept().await.unwrap();
        backend_request(&mut counting).await;
        // All four clients were ready before the runner's first poll. It can
        // accept exactly three while the first backend response stays pending.
        assert_eq!(
            clients[3].try_read(&mut [0]).unwrap_err().kind(),
            ErrorKind::WouldBlock
        );
        stop.send(()).unwrap();
        let report = running.await.unwrap();
        assert_eq!(report.exit, RunExit::Shutdown);
        assert_eq!(report.peak_in_flight, 3);
        assert_eq!(report.completed, 0);
        assert_eq!(report.failed, 3);
        assert_eq!(counting.read(&mut [0]).await.unwrap(), 0);
        for client in clients.iter_mut().take(3) {
            assert_eq!(client.read(&mut [0]).await.unwrap(), 0);
        }
        assert!(matches!(
            service.accept().await,
            Err(ServiceError::Gate(GateError::Closed))
        ));
    });
}

#[test]
fn generation_failure_exits_the_runner_with_a_distinct_admission_latch() {
    check(async {
        let (backend, service) = setup(1).await;
        let client = request(service.port()).await;
        let worker = Arc::clone(&service);
        let running = spawn_local(async move { run(&worker, pending()).await });
        let (mut counting, _) = backend.accept().await.unwrap();
        backend_request(&mut counting).await;
        respond(
            &mut counting,
            br#"{"input_tokens":10,"object":"response.input_tokens"}"#,
        )
        .await;
        let (mut generation, _) = backend.accept().await.unwrap();
        backend_request(&mut generation).await;
        drop(client);
        let report = running.await.unwrap();
        assert_eq!(report.exit, RunExit::AdmissionClosed);
        assert_eq!(report.failed, 1);
        assert_eq!(generation.read(&mut [0]).await.unwrap(), 0);
    });
}

#[test]
fn successful_delivery_and_notification_failure_both_drain_without_detached_work() {
    check(async {
        let (backend, service) = setup(0).await;
        let mut client = request(service.port()).await;
        let (stop, stopped) = oneshot::channel();
        let worker = Arc::clone(&service);
        let running = spawn_local(async move {
            run(&worker, async {
                stopped.await.unwrap();
                Err(std::io::Error::from(ErrorKind::Other))
            })
            .await
        });
        let (mut counting, _) = backend.accept().await.unwrap();
        backend_request(&mut counting).await;
        respond(
            &mut counting,
            br#"{"input_tokens":10,"object":"response.input_tokens"}"#,
        )
        .await;
        let (mut generation, _) = backend.accept().await.unwrap();
        backend_request(&mut generation).await;
        respond(&mut generation, br#"{"choices":[]}"#).await;
        let mut response = Vec::new();
        client.read_to_end(&mut response).await.unwrap();
        assert!(response.ends_with(br#"{"choices":[]}"#));
        stop.send(()).unwrap();
        let report = running.await.unwrap();
        assert_eq!(report.exit, RunExit::ShutdownFailure);
        assert_eq!(report.completed, 1);
        assert_eq!(report.failed, 0);
        assert_eq!(report.peak_in_flight, 1);
    });
}

struct KeyFiles(PathBuf);

impl KeyFiles {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "SwiftBonsai-RunnerKeys-{}-{nonce}",
            std::process::id()
        ));
        assert!(path.is_absolute());
        fs::create_dir(&path).unwrap();
        fs::write(path.join("frontend.key"), FRONT_KEY).unwrap();
        fs::write(path.join("backend.key"), BACK_KEY).unwrap();
        Self(path)
    }

    fn arguments(&self, backend_port: u16) -> Vec<OsString> {
        vec![
            "--development".into(),
            "--allow-unverified-key-files".into(),
            "--frontend-port".into(),
            "0".into(),
            "--backend-port".into(),
            backend_port.to_string().into(),
            "--frontend-key-file".into(),
            self.0.join("frontend.key").into(),
            "--backend-key-file".into(),
            self.0.join("backend.key").into(),
        ]
    }
}

impl Drop for KeyFiles {
    fn drop(&mut self) {
        // Only individually named files from this fixture; no recursive cleanup.
        fs::remove_file(self.0.join("frontend.key")).unwrap();
        fs::remove_file(self.0.join("backend.key")).unwrap();
        fs::remove_dir(&self.0).unwrap();
    }
}

#[test]
fn development_configuration_requires_opt_in_and_bounded_distinct_file_keys() {
    check(async {
        let files = KeyFiles::new();
        let backend = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = backend.local_addr().unwrap().port();
        let mut args = files.arguments(port);
        args.remove(0);
        assert!(matches!(
            DevelopmentConfig::parse(args),
            Err(ConfigurationError::DevelopmentOptIn)
        ));
        let mut args = files.arguments(port);
        args.extend([OsString::from("--max-waiting"), OsString::from("9")]);
        assert!(matches!(
            DevelopmentConfig::parse(args),
            Err(ConfigurationError::Arguments)
        ));
        let mut args = files.arguments(port);
        args.push("--development".into());
        assert!(matches!(
            DevelopmentConfig::parse(args),
            Err(ConfigurationError::Arguments)
        ));
        for invalid in ["a".repeat(65), "A".repeat(64), format!("{FRONT_KEY}\r\n")] {
            fs::write(files.0.join("frontend.key"), invalid).unwrap();
            assert!(matches!(
                DevelopmentConfig::parse(files.arguments(port))
                    .unwrap()
                    .bind()
                    .await,
                Err(ConfigurationError::KeyFormat)
            ));
        }
        fs::write(files.0.join("frontend.key"), BACK_KEY).unwrap();
        assert!(matches!(
            DevelopmentConfig::parse(files.arguments(port))
                .unwrap()
                .bind()
                .await,
            Err(ConfigurationError::Service(ServiceError::Configuration))
        ));
        fs::write(files.0.join("frontend.key"), FRONT_KEY).unwrap();
        let owner = DevelopmentConfig::parse(files.arguments(port))
            .unwrap()
            .bind()
            .await
            .unwrap();
        assert!(owner.service().port() >= 1024);
        #[cfg(windows)]
        assert!(fs::write(files.0.join("frontend.key"), BACK_KEY).is_err());
        drop(owner);
    });
}
