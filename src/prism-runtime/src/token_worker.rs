//! Persistent, metadata-only token counting over a bounded JSONL transport.
//!
//! [`serve`] takes an already verified [`TokenizationSession`], emits and flushes
//! one readiness record, then processes requests serially without reopening the
//! artifact. The session retains no weights, graph or artifact handle. Each
//! request has exactly `id` (a JSON u64), `op` (`count` or `encode`) and `text`
//! (a UTF-8 string). Policies and context accounting are fixed at startup.
//!
//! Duplicate and unknown fields, invalid UTF-8/JSON, blank lines and invalid
//! request shapes produce `invalid_request` with a null ID. Tokenizer or context
//! rejection produces `rejected_input` with the parsed ID. Neither error includes
//! input text or underlying diagnostics. Both errors leave the session reusable.
//! IDs are opaque correlation values, not a retained global uniqueness registry.
//! Count responses omit token IDs; encode responses include their exact order.
//!
//! Frames count every byte before LF, including a CR in CRLF. An unterminated
//! final frame is rejected as `invalid_request`, then EOF ends cleanly. A frame
//! beyond the configured bound, frame allocation failure or any transport I/O error
//! terminates serving with an error; no attempt is made to drain or resynchronize
//! an oversized frame. Writes can be partial on I/O failure. Clients must discard
//! an incomplete response and restart the process. Every complete response is
//! flushed, allowing clients to wait for readiness before sending any request.

use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};

use crate::special_input::SpecialInputPolicy;
use crate::tokenization::{
    SpecialTokenInsertion, TextTokenizationPolicy, TokenBudget, TokenizationSession,
};
use crate::{Result, invalid};

/// Maximum admitted JSONL frame length, excluding its LF terminator (1 MiB).
pub const MAX_WORKER_FRAME_BYTES: usize = 1024 * 1024;

/// Immutable transport and complete-text policy for a worker session.
#[derive(Clone, Copy, Debug)]
pub struct WorkerConfig {
    /// Explicit tokenizer parsing, boundary insertion and context budget.
    pub text_policy: TextTokenizationPolicy,
    /// Maximum bytes before LF, from 1 through [`MAX_WORKER_FRAME_BYTES`].
    pub max_frame_bytes: usize,
}

impl WorkerConfig {
    /// Validate all resource bounds without reading or encoding input.
    ///
    /// # Errors
    /// Rejects invalid context/output accounting or frame byte bounds.
    pub fn validate(self) -> Result<()> {
        self.text_policy.budget.prompt_token_limit()?;
        if !(1..=MAX_WORKER_FRAME_BYTES).contains(&self.max_frame_bytes) {
            return Err(invalid("invalid token worker frame bound"));
        }
        Ok(())
    }
}

// Intentionally no Debug: requests own caller-provided prompt text.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: u64,
    op: Operation,
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Operation {
    Count,
    Encode,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Response<'a> {
    Ready {
        protocol: &'static str,
        model_sha256: &'a str,
        vocab_size: usize,
        context: usize,
        reserved_output_tokens: usize,
        max_frame_bytes: usize,
        special_input: &'static str,
        insert_special: &'static str,
    },
    Count {
        id: u64,
        prompt_token_count: usize,
    },
    Encoded {
        id: u64,
        prompt_token_count: usize,
        token_ids: &'a [u32],
    },
    Error {
        id: Option<u64>,
        code: &'static str,
    },
}

/// Serve strict JSONL requests using one immutable, artifact-verified tokenizer.
///
/// Readiness reports `bonsai-token-worker-v1`, the verified lowercase SHA-256,
/// vocabulary size and every fixed transport/text/context option. Input buffering
/// retains at most `max_frame_bytes` bytes plus the caller's bounded reader buffer.
/// Decoded text is bounded by its frame and token output by the prompt allowance.
/// The underlying tokenizer's additional piece/work bounds still apply; these
/// are resource contracts, not a process resident-memory or CPU-time guarantee.
/// The worker never emits prompts, tokenizer diagnostics or generated text.
///
/// # Errors
/// Rejects invalid configuration before readiness; allocation, oversized frames
/// and transport I/O errors terminate serving. Request errors emit safe records
/// and continue. An empty stream cleanly returns after its readiness record.
pub fn serve(
    session: &TokenizationSession,
    config: WorkerConfig,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<()> {
    config.validate()?;
    let TokenBudget {
        context,
        reserved_output_tokens,
    } = config.text_policy.budget;
    let ready = Response::Ready {
        protocol: "bonsai-token-worker-v1",
        model_sha256: session.identity(),
        vocab_size: session.vocab_size(),
        context,
        reserved_output_tokens,
        max_frame_bytes: config.max_frame_bytes,
        special_input: match config.text_policy.special_input {
            SpecialInputPolicy::Literal => "literal",
            SpecialInputPolicy::Parse => "parse",
        },
        insert_special: match config.text_policy.insert_special {
            SpecialTokenInsertion::None => "none",
            SpecialTokenInsertion::Model => "model",
        },
    };
    let mut frame = Vec::new();
    frame.try_reserve_exact(config.max_frame_bytes)?;
    write_record(output, &ready)?;
    loop {
        match read_frame(input, &mut frame, config.max_frame_bytes)? {
            Frame::Eof => return Ok(()),
            Frame::Unterminated => {
                write_invalid_request(output)?;
                return Ok(());
            }
            Frame::Complete => (),
        }
        if frame.trim_ascii_start().first() != Some(&b'{') {
            write_invalid_request(output)?;
            continue;
        }
        let Ok(request) = serde_json::from_slice::<Request>(&frame) else {
            write_invalid_request(output)?;
            continue;
        };
        let Ok(result) = session.encode(&request.text, config.text_policy) else {
            write_record(
                output,
                &Response::Error {
                    id: Some(request.id),
                    code: "rejected_input",
                },
            )?;
            continue;
        };
        let response = match request.op {
            Operation::Count => Response::Count {
                id: request.id,
                prompt_token_count: result.prompt_token_count(),
            },
            Operation::Encode => Response::Encoded {
                id: request.id,
                prompt_token_count: result.prompt_token_count(),
                token_ids: &result.token_ids,
            },
        };
        write_record(output, &response)?;
    }
}

enum Frame {
    Eof,
    Complete,
    Unterminated,
}

fn read_frame(input: &mut impl BufRead, frame: &mut Vec<u8>, limit: usize) -> Result<Frame> {
    frame.clear();
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return Ok(if frame.is_empty() {
                Frame::Eof
            } else {
                Frame::Unterminated
            });
        }
        let newline = available.iter().position(|&byte| byte == b'\n');
        let count = newline.unwrap_or(available.len());
        if count > limit - frame.len() {
            return Err(invalid("token worker frame exceeds byte limit"));
        }
        frame.extend_from_slice(&available[..count]);
        input.consume(count + usize::from(newline.is_some()));
        if newline.is_some() {
            return Ok(Frame::Complete);
        }
    }
}

fn write_invalid_request(output: &mut impl Write) -> Result<()> {
    write_record(
        output,
        &Response::Error {
            id: None,
            code: "invalid_request",
        },
    )
}

fn write_record(output: &mut impl Write, response: &Response<'_>) -> Result<()> {
    serde_json::to_writer(&mut *output, response)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
