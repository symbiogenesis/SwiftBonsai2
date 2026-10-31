//! The only caller-controlled adapter value is a bounded scale for pinned slot 0.

use super::{PendingChat, Rejection, RequestPolicy};
use http::Request;
use serde_json::{Value, json};

const CHAT: &[u8] = br#"{"messages":[{"role":"user","content":"Write a scene."}]}"#;

fn prepare(body: &[u8], enabled: bool) -> Result<PendingChat, Rejection> {
    let key = "a".repeat(64);
    let mut policy = RequestPolicy::new(8080, &key)?;
    // Production can set this only inside the owned launcher's verified service.
    policy.pinned_abliteration = enabled;
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("host", "127.0.0.1:8080")
        .header("authorization", format!("Bearer {key}"))
        .header("content-type", "application/json")
        .header("content-length", body.len())
        .body(())
        .map_err(|_| Rejection::Configuration)?;
    policy.check_head(&request.into_parts().0)?.prepare(body)
}

fn body_with(scale: &str) -> String {
    format!(
        r#"{{"messages":[{{"role":"user","content":"Write a scene."}}],"abliteration_scale":{scale}}}"#
    )
}

fn canonical(body: &[u8], enabled: bool) -> Result<Value, Rejection> {
    let admitted = prepare(body, enabled)?.admit(|_| Ok(1))?;
    serde_json::from_slice(admitted.backend_body()).map_err(|_| Rejection::ChatSchema)
}

#[test]
fn pinned_abliteration_defaults_to_1_3_without_changing_other_policy() -> Result<(), Rejection> {
    let baseline = canonical(CHAT, false)?;
    assert!(baseline.get("lora").is_none());
    let mut enabled = canonical(CHAT, true)?;
    let lora = enabled.as_object_mut().and_then(|body| body.remove("lora"));
    assert_eq!(lora, Some(json!([{ "id": 0, "scale": 1.3 }])));
    assert_eq!(enabled, baseline);
    Ok(())
}

#[test]
fn pinned_abliteration_scale_is_identical_for_counting_and_generation() -> Result<(), Rejection> {
    for (scale, expected) in [("0", json!(0.0)), ("1.3", json!(1.3)), ("2", json!(2.0))] {
        let pending = prepare(body_with(scale).as_bytes(), true)?;
        let counted = pending.body.clone();
        let decoded: Value = serde_json::from_slice(&counted).map_err(|_| Rejection::ChatSchema)?;
        assert_eq!(decoded["lora"], json!([{ "id": 0, "scale": expected }]));
        assert!(decoded.get("abliteration_scale").is_none());
        let admitted = pending.admit(|body| {
            assert_eq!(body, counted);
            Ok(1)
        })?;
        assert_eq!(admitted.backend_body(), counted);
    }
    Ok(())
}

#[test]
fn pinned_abliteration_rejects_invalid_scales_and_arbitrary_adapters() {
    for scale in [
        "null",
        "true",
        "false",
        r#""1.3""#,
        "[]",
        "{}",
        "-0.1",
        "2.00000001",
        "3",
        "1e309",
        "NaN",
        "Infinity",
    ] {
        assert!(matches!(
            prepare(body_with(scale).as_bytes(), true),
            Err(Rejection::ChatSchema)
        ));
    }
    for extra in [
        r#""abliteration_scale":1.3,"abliteration_scale":1.3"#,
        r#""lora":[{"id":0,"scale":1.3}]"#,
        r#""abliteration_scale":1.3,"lora":[]"#,
    ] {
        let body = format!(r#"{{"messages":[{{"role":"user","content":"text"}}],{extra}}}"#);
        assert!(matches!(
            prepare(body.as_bytes(), true),
            Err(Rejection::ChatSchema)
        ));
    }
}

#[test]
fn pinned_abliteration_scale_is_unavailable_without_verified_launch() {
    for scale in ["0", "1.3", "2"] {
        assert!(matches!(
            prepare(body_with(scale).as_bytes(), false),
            Err(Rejection::ChatSchema)
        ));
    }
}

#[cfg(windows)]
mod transport {
    #![expect(clippy::unwrap_used, reason = "Transport fixtures must fail loudly.")]

    use crate::{
        Rejection,
        reception::ReceiveError,
        service::{LocalChatService, ServiceError},
        supervisor::ServeError,
    };
    use serde_json::{Value, json};
    use std::{sync::Arc, time::Duration};
    use tokio::{
        io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
        net::{TcpListener, TcpStream},
        task::{LocalSet, spawn_local},
        time::timeout,
    };

    const FRONT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const BACK: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const EMPTY_RESPONSE: [u8; 0] = [];

    async fn request(port: u16, scale: f64) -> TcpStream {
        let body = serde_json::to_vec(&json!({
            "messages":[{"role":"user","content":"Name a tree."}],
            "stream":false, "max_completion_tokens":2, "abliteration_scale":scale,
        }))
        .unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let head = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {FRONT}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        client.write_all(head.as_bytes()).await.unwrap();
        client.write_all(&body).await.unwrap();
        client
    }

    async fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
        let mut reader = BufReader::new(stream);
        let mut head = String::new();
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).await.unwrap() > 0);
            head.push_str(&line);
            assert!(head.len() <= crate::MAX_HEADER_BYTES);
            if line == "\r\n" {
                break;
            }
        }
        let length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        assert!(length <= crate::MAX_BODY_BYTES);
        let mut body = vec![0; length];
        reader.read_exact(&mut body).await.unwrap();
        (head, body)
    }

    async fn respond(stream: &mut TcpStream, body: &[u8]) {
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).await.unwrap();
        stream.write_all(body).await.unwrap();
        stream.shutdown().await.unwrap();
    }

    #[test]
    fn invalid_scale_closes_schema_connection_before_backend_and_preserves_service() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(LocalSet::new().run_until(async {
            timeout(Duration::from_secs(2), async {
                let backend = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let mut service =
                    LocalChatService::bind(0, FRONT, backend.local_addr().unwrap().port(), BACK, 0)
                        .await
                        .unwrap();
                service.enable_pinned_abliteration();
                let service = Arc::new(service);
                let worker = Arc::clone(&service);
                let rejected =
                    spawn_local(async move { worker.accept().await.unwrap().serve().await });
                // Do not half-close the client: schema rejection itself must
                // close this complete request without emitting HTTP bytes.
                let mut client = request(service.port(), 2.000_000_01).await;
                let mut bytes = Vec::new();
                client.read_to_end(&mut bytes).await.unwrap();
                assert_eq!(bytes, EMPTY_RESPONSE);
                assert_eq!(
                    rejected.await.unwrap(),
                    Err(ServiceError::Request(ServeError::Receive(
                        ReceiveError::Policy(Rejection::ChatSchema)
                    )))
                );
                assert!(
                    timeout(Duration::from_millis(5), backend.accept())
                        .await
                        .is_err()
                );
                let worker = Arc::clone(&service);
                let serving =
                    spawn_local(async move { worker.accept().await.unwrap().serve().await });
                let backend_work = spawn_local(async move {
                    let (mut count, _) = backend.accept().await.unwrap();
                    let counted = read_request(&mut count).await;
                    respond(
                        &mut count,
                        br#"{"input_tokens":1,"object":"response.input_tokens"}"#,
                    )
                    .await;
                    drop(count);
                    let (mut generation, _) = backend.accept().await.unwrap();
                    let generated = read_request(&mut generation).await;
                    respond(&mut generation, br#"{"choices":[]}"#).await;
                    (counted, generated)
                });
                let mut client = request(service.port(), 1.3).await;
                let mut bytes = Vec::new();
                client.read_to_end(&mut bytes).await.unwrap();
                assert_eq!(serving.await.unwrap(), Ok(()));
                let ((count_head, counted), (generation_head, generated)) =
                    backend_work.await.unwrap();
                assert!(
                    count_head.starts_with("POST /v1/chat/completions/input_tokens HTTP/1.1\r\n")
                );
                assert!(generation_head.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
                assert_eq!(counted, generated);
                let canonical: Value = serde_json::from_slice(&counted).unwrap();
                assert_eq!(canonical["lora"], json!([{"id":0,"scale":1.3}]));
                assert!(bytes.starts_with(b"HTTP/1.1 200 OK\r\n"));
                assert!(bytes.ends_with(br#"{"choices":[]}"#));
            })
            .await
            .unwrap();
        }));
    }
}
