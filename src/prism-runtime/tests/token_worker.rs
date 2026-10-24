#![forbid(unsafe_code)]
//! Focused protocol and process checks of the persistent tokenization worker.

#[cfg(feature = "cli")]
mod worker {
    use std::fs;
    use std::io::{self, BufRead, BufReader, Cursor, Read, Write};
    use std::path::PathBuf;
    use std::process::{Command, Output, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use bonsai_prism_runtime::Result;
    use bonsai_prism_runtime::special_input::SpecialInputPolicy;
    use bonsai_prism_runtime::token_worker::{MAX_WORKER_FRAME_BYTES, WorkerConfig, serve};
    use bonsai_prism_runtime::tokenization::{
        SpecialTokenInsertion, TextTokenizationPolicy, TokenBudget, TokenizationSession,
    };
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};

    const HEADER_BUDGET: usize = 128 * 1024;
    static NEXT_ARTIFACT: AtomicU64 = AtomicU64::new(0);

    struct Artifact {
        root: PathBuf,
        path: PathBuf,
        bytes: Vec<u8>,
    }

    impl Artifact {
        fn new(vocab_size: u64) -> Result<Self> {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let sequence = NEXT_ARTIFACT.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "SwiftBonsai-TokenizeDriver-{}-{nonce}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&root)?;
            let path = root.join("model.gguf");
            let mut bytes = Vec::new();
            u32_bytes(&mut bytes, 0x4655_4747);
            u32_bytes(&mut bytes, 3);
            u64_bytes(&mut bytes, 1);
            u64_bytes(&mut bytes, 10);
            string_metadata(
                &mut bytes,
                "general.architecture",
                "unsupported-for-generation",
            )?;
            string_metadata(&mut bytes, "tokenizer.ggml.model", "gpt2")?;
            string_metadata(&mut bytes, "tokenizer.ggml.pre", "qwen35")?;
            text(&mut bytes, "tokenizer.ggml.tokens")?;
            u32_bytes(&mut bytes, 9);
            u32_bytes(&mut bytes, 8);
            u64_bytes(&mut bytes, 260);
            let mut next = 256_u32;
            for byte in u8::MIN..=u8::MAX {
                let scalar = if (b'!'..=b'~').contains(&byte)
                    || (0xa1..=0xac).contains(&byte)
                    || (0xae..=0xff).contains(&byte)
                {
                    u32::from(byte)
                } else {
                    let scalar = next;
                    next += 1;
                    scalar
                };
                let character = char::from_u32(scalar).ok_or("invalid fixture byte alphabet")?;
                text(&mut bytes, &character.to_string())?;
            }
            for token in ["ab", "a!", "<stop>", "<tag>"] {
                text(&mut bytes, token)?;
            }
            text(&mut bytes, "tokenizer.ggml.token_type")?;
            u32_bytes(&mut bytes, 9);
            u32_bytes(&mut bytes, 5);
            u64_bytes(&mut bytes, 260);
            for _ in 0..258 {
                bytes.extend(1_i32.to_le_bytes());
            }
            for kind in [3_i32, 4] {
                bytes.extend(kind.to_le_bytes());
            }
            text(&mut bytes, "tokenizer.ggml.merges")?;
            u32_bytes(&mut bytes, 9);
            u32_bytes(&mut bytes, 8);
            u64_bytes(&mut bytes, 2);
            text(&mut bytes, "a b")?;
            text(&mut bytes, "a !")?;
            for (key, token) in [
                ("tokenizer.ggml.eos_token_id", 258),
                ("tokenizer.ggml.bos_token_id", 259),
            ] {
                text(&mut bytes, key)?;
                u32_bytes(&mut bytes, 4);
                u32_bytes(&mut bytes, token);
            }
            for key in [
                "tokenizer.ggml.add_bos_token",
                "tokenizer.ggml.add_eos_token",
            ] {
                text(&mut bytes, key)?;
                u32_bytes(&mut bytes, 7);
                bytes.push(1);
            }
            text(&mut bytes, "token_embd.weight")?;
            u32_bytes(&mut bytes, 2);
            u64_bytes(&mut bytes, 1024);
            u64_bytes(&mut bytes, vocab_size);
            u32_bytes(&mut bytes, 0);
            u64_bytes(&mut bytes, 0);
            let data_offset = bytes.len().div_ceil(32) * 32;
            let payload_bytes = usize::try_from(vocab_size)? * 1024 * 4;
            bytes.resize(data_offset + payload_bytes, 0);
            bytes[data_offset..data_offset + 4].copy_from_slice(&f32::NAN.to_le_bytes());
            fs::write(&path, &bytes)?;
            Ok(Self { root, path, bytes })
        }

        fn hash(&self) -> String {
            Sha256::digest(&self.bytes)
                .into_iter()
                .flat_map(|byte| [byte >> 4, byte & 15])
                .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
                .collect()
        }
    }

    impl Drop for Artifact {
        fn drop(&mut self) {
            // Only the exact file and directory created by this fixture are removed.
            let _ = fs::remove_file(&self.path);
            let _ = fs::remove_file(self.root.join("prompt-input.bin"));
            let _ = fs::remove_dir(&self.root);
        }
    }

    fn u32_bytes(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend(value.to_le_bytes());
    }

    fn u64_bytes(bytes: &mut Vec<u8>, value: u64) {
        bytes.extend(value.to_le_bytes());
    }

    fn text(bytes: &mut Vec<u8>, value: &str) -> Result<()> {
        u64_bytes(bytes, u64::try_from(value.len())?);
        bytes.extend(value.as_bytes());
        Ok(())
    }

    fn string_metadata(bytes: &mut Vec<u8>, key: &str, value: &str) -> Result<()> {
        text(bytes, key)?;
        u32_bytes(bytes, 8);
        text(bytes, value)
    }

    const fn config() -> WorkerConfig {
        WorkerConfig {
            text_policy: TextTokenizationPolicy {
                budget: TokenBudget {
                    context: 4,
                    reserved_output_tokens: 1,
                },
                special_input: SpecialInputPolicy::Parse,
                insert_special: SpecialTokenInsertion::None,
            },
            max_frame_bytes: 1024,
        }
    }

    fn load(fixture: &Artifact) -> Result<TokenizationSession> {
        TokenizationSession::load(
            &fixture.path,
            u64::try_from(fixture.bytes.len())?,
            &fixture.hash(),
            HEADER_BUDGET,
        )
    }

    fn records(output: &[u8]) -> Result<Vec<Value>> {
        assert!(output.ends_with(b"\n"));
        std::str::from_utf8(output)?
            .lines()
            .map(|line| serde_json::from_str(line).map_err(Into::into))
            .collect()
    }

    fn run(
        session: &TokenizationSession,
        options: WorkerConfig,
        input: &[u8],
    ) -> Result<Vec<Value>> {
        let mut output = Vec::new();
        // One-byte reads exercise frame boundaries independently of chunk sizes.
        serve(
            session,
            options,
            &mut BufReader::with_capacity(1, input),
            &mut output,
        )?;
        records(&output)
    }

    #[test]
    fn persistent_counts_and_exact_ids_survive_artifact_removal() -> Result<()> {
        let fixture = Artifact::new(260)?;
        let session = load(&fixture)?;
        fs::remove_file(&fixture.path)?;
        let output = run(
            &session,
            config(),
            b"{\"id\":7,\"op\":\"count\",\"text\":\"ab\"}\n{\"id\":8,\"op\":\"encode\",\"text\":\"ab<stop>\"}\n{\"id\":9,\"op\":\"encode\",\"text\":\"\"}\n",
        )?;
        assert_eq!(
            output[0],
            json!({
                "type": "ready", "protocol": "bonsai-token-worker-v1",
                "model_sha256": fixture.hash(), "vocab_size": 260,
                "context": 4, "reserved_output_tokens": 1,
                "max_frame_bytes": 1024, "special_input": "parse", "insert_special": "none"
            })
        );
        assert_eq!(
            output[1],
            json!({"type": "count", "id": 7, "prompt_token_count": 1})
        );
        assert_eq!(
            output[2],
            json!({"type": "encoded", "id": 8, "prompt_token_count": 2, "token_ids": [256, 258]})
        );
        assert_eq!(
            output[3],
            json!({"type": "encoded", "id": 9, "prompt_token_count": 0, "token_ids": []})
        );
        assert_eq!(output.len(), 4);
        Ok(())
    }

    #[test]
    fn fixed_policy_and_reservation_reject_overflow_then_recover() -> Result<()> {
        let fixture = Artifact::new(260)?;
        let session = load(&fixture)?;
        let mut options = config();
        options.text_policy.insert_special = SpecialTokenInsertion::Model;
        let output = run(
            &session,
            options,
            b"{\"id\":0,\"op\":\"encode\",\"text\":\"ab<stop>\"}\n{\"id\":1,\"op\":\"encode\",\"text\":\"ab\"}\n{\"id\":2,\"op\":\"encode\",\"text\":\"\"}\n",
        )?;
        assert_eq!(
            output[1],
            json!({"type": "error", "id": 0, "code": "rejected_input"})
        );
        assert_eq!(
            output[2],
            json!({"type": "encoded", "id": 1, "prompt_token_count": 3, "token_ids": [259, 256, 258]})
        );
        assert_eq!(output[3]["token_ids"], json!([259, 258]));
        options.text_policy.special_input = SpecialInputPolicy::Literal;
        options.text_policy.insert_special = SpecialTokenInsertion::None;
        options.text_policy.budget.context = 16;
        let output = run(
            &session,
            options,
            b"{\"id\":0,\"op\":\"encode\",\"text\":\"<stop>\"}\n",
        )?;
        assert_eq!(output[0]["special_input"], "literal");
        assert_eq!(output[1]["token_ids"], json!([60, 115, 116, 111, 112, 62]));
        Ok(())
    }

    #[test]
    fn malformed_requests_are_private_and_do_not_poison_later_frames() -> Result<()> {
        let fixture = Artifact::new(260)?;
        let session = load(&fixture)?;
        let invalid: &[&[u8]] = &[
            b"",
            b"\xff",
            b"null",
            b"[]",
            br#"[1,"count","ab"]"#,
            b"{",
            b"{}",
            br#"{"id":1,"op":"count","text":"private-prompt","text":"a"}"#,
            br#"{"id":1,"id":2,"op":"count","text":"private-prompt"}"#,
            br#"{"id":1,"op":"count","op":"encode","text":"private-prompt"}"#,
            br#"{"id":1,"op":"count","text":"private-prompt","context":99}"#,
            br#"{"id":-1,"op":"count","text":"private-prompt"}"#,
            br#"{"id":1.5,"op":"count","text":"private-prompt"}"#,
            br#"{"id":"1","op":"count","text":"private-prompt"}"#,
            br#"{"id":18446744073709551616,"op":"count","text":"private-prompt"}"#,
            br#"{"id":1,"op":"count","text":null}"#,
            br#"{"id":1,"op":"count"}"#,
            br#"{"id":1,"op":"generate","text":"private-prompt"}"#,
            br#"{"id":1,"op":"count","text":"private-prompt"} {}"#,
        ];
        let mut input = Vec::new();
        for frame in invalid {
            input.extend_from_slice(frame);
            input.push(b'\n');
        }
        input.extend_from_slice(
            b"{\"id\":18446744073709551615,\"op\":\"count\",\"text\":\"ab\"}\r\n",
        );
        let mut output = Vec::new();
        serve(&session, config(), &mut Cursor::new(&input), &mut output)?;
        assert!(!std::str::from_utf8(&output)?.contains("private-prompt"));
        let output = records(&output)?;
        assert_eq!(output.len(), invalid.len() + 2);
        for record in &output[1..=invalid.len()] {
            assert_eq!(
                record,
                &json!({"type": "error", "id": null, "code": "invalid_request"})
            );
        }
        assert_eq!(
            output[invalid.len() + 1],
            json!({"type": "count", "id": u64::MAX, "prompt_token_count": 1})
        );
        Ok(())
    }

    #[test]
    fn newline_bound_and_unterminated_eof_have_explicit_results() -> Result<()> {
        let fixture = Artifact::new(260)?;
        let session = load(&fixture)?;
        let frame = b"{\"id\":1,\"op\":\"count\",\"text\":\"ab\"}";
        let mut options = config();
        options.max_frame_bytes = frame.len();
        let mut input = frame.to_vec();
        input.push(b'\n');
        assert_eq!(run(&session, options, &input)?[1]["type"], "count");
        input.extend_from_slice(frame);
        let output = run(&session, options, &input)?;
        assert_eq!(output.len(), 3);
        assert_eq!(
            output[2],
            json!({"type": "error", "id": null, "code": "invalid_request"})
        );
        assert_eq!(run(&session, options, b"")?.len(), 1);
        // One extra byte is fatal, even if followed by a valid request.
        let mut oversized = frame.to_vec();
        oversized.extend_from_slice(b" \n");
        oversized.extend_from_slice(&input);
        let mut output = Vec::new();
        assert!(
            serve(
                &session,
                options,
                &mut BufReader::with_capacity(1, oversized.as_slice()),
                &mut output
            )
            .is_err()
        );
        assert_eq!(records(&output)?.len(), 1);
        Ok(())
    }

    #[test]
    fn invalid_configuration_never_emits_readiness() -> Result<()> {
        let fixture = Artifact::new(260)?;
        let session = load(&fixture)?;
        let mut options = config();
        for bytes in [0, MAX_WORKER_FRAME_BYTES + 1] {
            options.max_frame_bytes = bytes;
            let mut output = Vec::new();
            assert!(serve(&session, options, &mut Cursor::new([]), &mut output).is_err());
            assert_eq!(output, [0_u8; 0]);
        }
        options = config();
        options.text_policy.budget.reserved_output_tokens = options.text_policy.budget.context;
        let mut output = Vec::new();
        assert!(serve(&session, options, &mut Cursor::new([]), &mut output).is_err());
        assert_eq!(output, [0_u8; 0]);
        Ok(())
    }

    struct BrokenRead;

    impl Read for BrokenRead {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("private transport detail"))
        }
    }

    struct BrokenFlush;

    impl Write for BrokenFlush {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("private transport detail"))
        }
    }

    #[test]
    fn transport_read_and_flush_failures_terminate() -> Result<()> {
        let fixture = Artifact::new(260)?;
        let session = load(&fixture)?;
        let mut output = Vec::new();
        assert!(
            serve(
                &session,
                config(),
                &mut BufReader::new(BrokenRead),
                &mut output
            )
            .is_err()
        );
        assert_eq!(records(&output)?.len(), 1);
        assert!(serve(&session, config(), &mut Cursor::new([]), &mut BrokenFlush).is_err());
        Ok(())
    }

    fn arguments(fixture: &Artifact) -> Vec<String> {
        vec![
            "--model".into(),
            fixture.path.to_string_lossy().into_owned(),
            "--bytes".into(),
            fixture.bytes.len().to_string(),
            "--sha256".into(),
            fixture.hash(),
            "--memory-bytes".into(),
            HEADER_BUDGET.to_string(),
            "--context".into(),
            "4".into(),
            "--reserved-output-tokens".into(),
            "1".into(),
            "--special-input".into(),
            "parse".into(),
            "--insert-special".into(),
            "none".into(),
            "--max-frame-bytes".into(),
            "1024".into(),
        ]
    }

    fn child(fixture: &Artifact, options: &[String], input: &[u8]) -> Result<Output> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_bonsai-token-worker"))
            .args(options)
            .current_dir(&fixture.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .ok_or("missing child stdin")?
            .write_all(input)?;
        Ok(child.wait_with_output()?)
    }

    #[test]
    fn binary_flushes_readiness_before_input_then_reuses_verified_session() -> Result<()> {
        let fixture = Artifact::new(260)?;
        let mut child = Command::new(env!("CARGO_BIN_EXE_bonsai-token-worker"))
            .args(arguments(&fixture))
            .current_dir(&fixture.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdout = BufReader::new(child.stdout.take().ok_or("missing child stdout")?);
        let mut ready = String::new();
        stdout.read_line(&mut ready)?;
        assert_eq!(
            serde_json::from_str::<Value>(&ready)?["model_sha256"],
            fixture.hash()
        );
        fs::remove_file(&fixture.path)?;
        child.stdin.take().ok_or("missing child stdin")?.write_all(b"{\"id\":1,\"op\":\"count\",\"text\":\"ab\"}\n{\"id\":2,\"op\":\"encode\",\"text\":\"ab\"}\n")?;
        let mut responses = Vec::new();
        stdout.read_to_end(&mut responses)?;
        let result = child.wait_with_output()?;
        assert!(result.status.success());
        assert_eq!(result.stderr, [0_u8; 0]);
        let responses = records(&responses)?;
        assert_eq!(responses.len(), 2);
        assert_eq!(
            responses[0],
            json!({"type": "count", "id": 1, "prompt_token_count": 1})
        );
        assert_eq!(responses[1]["token_ids"], json!([256]));
        Ok(())
    }

    #[test]
    fn binary_rejects_unsafe_startup_and_fatal_frames_with_static_diagnostics() -> Result<()> {
        let fixture = Artifact::new(260)?;
        let base = arguments(&fixture);
        for tail in [
            vec!["--context", "2"],
            vec!["--prompt", "private-prompt"],
            vec!["--unknown", "private-prompt"],
            vec!["--max-frame-bytes"],
            vec!["--help"],
        ] {
            let mut options = base.clone();
            options.extend(tail.into_iter().map(str::to_owned));
            let output = child(&fixture, &options, b"")?;
            assert!(!output.status.success());
            assert_eq!(output.stdout, [0_u8; 0]);
            assert_eq!(output.stderr, b"token worker startup failed\n");
        }
        let mut options = base.clone();
        let digest = options
            .iter()
            .position(|item| item == "--sha256")
            .ok_or("missing hash")?;
        options[digest + 1] = "0".repeat(64);
        let output = child(&fixture, &options, b"")?;
        assert!(!output.status.success());
        assert_eq!(output.stdout, [0_u8; 0]);
        assert_eq!(output.stderr, b"token worker startup failed\n");
        for (key, value) in [
            ("--model", "relative.gguf"),
            ("--bytes", "0"),
            ("--bytes", "+1"),
            ("--memory-bytes", "0"),
            ("--context", "0"),
            ("--context", "262145"),
            ("--reserved-output-tokens", "4"),
            ("--reserved-output-tokens", "-1"),
            ("--special-input", "auto"),
            ("--insert-special", "auto"),
            ("--max-frame-bytes", "0"),
            ("--max-frame-bytes", "1048577"),
        ] {
            let mut options = base.clone();
            let position = options
                .iter()
                .position(|option| option == key)
                .ok_or("missing option")?;
            options[position + 1] = value.to_owned();
            let output = child(&fixture, &options, b"")?;
            assert!(!output.status.success());
            assert_eq!(output.stdout, [0_u8; 0]);
            assert_eq!(output.stderr, b"token worker startup failed\n");
        }
        let help = child(&fixture, &["--help".into()], b"")?;
        assert!(help.status.success());
        assert_eq!(help.stderr, [0_u8; 0]);
        assert!(std::str::from_utf8(&help.stdout)?.starts_with("Usage: bonsai-token-worker"));
        let output = child(&fixture, &base, &vec![b'x'; 1025])?;
        assert!(!output.status.success());
        assert_eq!(records(&output.stdout)?.len(), 1);
        assert_eq!(output.stderr, b"token worker transport failed\n");
        Ok(())
    }
}
