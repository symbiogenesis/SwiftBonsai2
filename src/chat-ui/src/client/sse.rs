//! Borrowed SSE framing and JSON deltas, separate from completed reply storage.

use super::{Completion, Failure, MAX_EVENT, Result};
use serde::{Deserialize, de::IgnoredAny};
use std::borrow::Cow;

#[derive(Default)]
pub(super) struct Sse {
    framer: Framer,
    reply: Reply,
}

#[derive(Default)]
struct Reply {
    text: String,
    reasoning: String,
    completion: Option<Completion>,
    done: bool,
}

#[derive(Default)]
struct Framer {
    bom_bytes: u8,
    started: bool,
    after_cr: bool,
    line: Vec<u8>,
    data: Vec<u8>,
    event: Vec<u8>,
    dispatched: bool,
}

struct Frame<'a> {
    data: &'a [u8],
    event: &'a [u8],
}

// The transparent wrapper makes Serde's borrowed-Cow string visitor apply
// inside Option as well: unescaped strings borrow; JSON escapes own decoding.
#[derive(Deserialize)]
#[serde(transparent)]
struct Text<'a>(#[serde(borrow)] Cow<'a, str>);

#[derive(Deserialize)]
struct Chunk<'a> {
    #[serde(default, borrow)]
    choices: Vec<Choice<'a>>,
    error: Option<IgnoredAny>,
}

#[derive(Deserialize)]
struct Choice<'a> {
    #[serde(default, borrow)]
    delta: Delta<'a>,
    index: Option<u32>,
    #[serde(borrow)]
    finish_reason: Option<Text<'a>>,
}

#[derive(Default, Deserialize)]
struct Delta<'a> {
    #[serde(borrow)]
    content: Option<Text<'a>>,
    #[serde(borrow)]
    reasoning_content: Option<Text<'a>>,
    #[serde(borrow)]
    reasoning: Option<Text<'a>>,
}

impl Sse {
    pub(super) fn push(
        &mut self,
        mut bytes: &[u8],
        delta: &mut impl FnMut(Cow<'_, str>, Cow<'_, str>) -> Result<()>,
    ) -> Result<()> {
        while let Some(frame) = self.framer.next_event(&mut bytes)? {
            self.reply.dispatch(frame, delta)?;
        }
        Ok(())
    }

    pub(super) fn finish(&self) -> Result<Completion> {
        if !self.reply.done {
            return Err(Failure::Message(
                "The local model ended its reply without confirming that generation finished.",
            ));
        }
        if self.reply.text.is_empty() && self.reply.reasoning.is_empty() {
            return Err(Failure::Message("The local model returned no text."));
        }
        // SSE discards any final event lacking a blank-line terminator. The
        // explicit terminal event and [DONE] must both have dispatched.
        self.reply.completion.ok_or(Failure::Message(
            "The response omitted its terminal completion reason.",
        ))
    }

    pub(super) fn into_answer(self) -> (String, String) {
        (self.reply.text, self.reply.reasoning)
    }
}

impl Framer {
    // A frame borrows only framing storage. Consumers can validate, accumulate
    // or enqueue its text before the next call recycles that storage.
    fn next_event(&mut self, bytes: &mut &[u8]) -> Result<Option<Frame<'_>>> {
        if self.dispatched {
            self.data.clear();
            self.event.clear();
            self.dispatched = false;
        }
        while let Some((&byte, rest)) = bytes.split_first() {
            *bytes = rest;
            if !self.started {
                match (self.bom_bytes, byte) {
                    (0, 0xef) => {
                        self.bom_bytes = 1;
                        continue;
                    }
                    (1, 0xbb) => {
                        self.bom_bytes = 2;
                        continue;
                    }
                    (2, 0xbf) => {
                        self.started = true;
                        continue;
                    }
                    _ => {
                        self.started = true;
                        // A failed BOM prefix is ordinary line data. Its two
                        // possible prefix bytes cannot contain a line ending.
                        if self.bom_bytes > 0 {
                            self.line.push(0xef);
                        }
                        if self.bom_bytes > 1 {
                            self.line.push(0xbb);
                        }
                    }
                }
            }
            if self.push_byte(byte)? {
                self.dispatched = true;
                return Ok(Some(Frame {
                    data: &self.data,
                    event: &self.event,
                }));
            }
        }
        Ok(None)
    }

    fn push_byte(&mut self, byte: u8) -> Result<bool> {
        if self.after_cr {
            self.after_cr = false;
            if byte == b'\n' {
                return Ok(false);
            }
        }
        if matches!(byte, b'\r' | b'\n') {
            self.after_cr = byte == b'\r';
            if self.line.is_empty() {
                if self.data.is_empty() {
                    self.event.clear();
                } else {
                    self.data.pop();
                    return Ok(true);
                }
            } else {
                self.process_line()?;
                self.line.clear();
            }
        } else {
            if self.line.len() == MAX_EVENT {
                return Err(Failure::Message("A streaming line exceeded its limit."));
            }
            self.line.push(byte);
        }
        Ok(false)
    }

    fn process_line(&mut self) -> Result<()> {
        if self.line.first() == Some(&b':') {
            return Ok(());
        }
        let (field, value) = self.line.iter().position(|&byte| byte == b':').map_or(
            (self.line.as_slice(), b"".as_slice()),
            |colon| {
                let value = &self.line[colon + 1..];
                (
                    &self.line[..colon],
                    value.strip_prefix(b" ").unwrap_or(value),
                )
            },
        );
        match field {
            b"data" => {
                if self.data.len() + value.len() + 1 > MAX_EVENT {
                    return Err(Failure::Message("A streaming event exceeded its limit."));
                }
                self.data.extend_from_slice(value);
                self.data.push(b'\n');
            }
            b"event" => {
                self.event.clear();
                self.event.extend_from_slice(value);
            }
            // id/retry affect reconnection only. Requests are never replayed:
            // interruption cancels the owned broker generation.
            _ => {}
        }
        Ok(())
    }
}

impl Reply {
    fn dispatch(
        &mut self,
        frame: Frame<'_>,
        delta: &mut impl FnMut(Cow<'_, str>, Cow<'_, str>) -> Result<()>,
    ) -> Result<()> {
        if frame.event == b"error" {
            return Err(Failure::Message("The model reported a streaming error."));
        }
        if !frame.event.is_empty() && frame.event != b"message" {
            return Err(Failure::Message(
                "The model returned an unsupported event type.",
            ));
        }
        if frame.data == b"[DONE]" {
            if self.done || self.completion.is_none() {
                return Err(Failure::Message(
                    "The response had an invalid terminal completion marker.",
                ));
            }
            self.done = true;
            return Ok(());
        }
        if self.done {
            return Err(Failure::Message(
                "The response continued after its completion marker.",
            ));
        }
        let chunk: Chunk<'_> = serde_json::from_slice(frame.data)
            .map_err(|_| Failure::Message("The model returned an invalid streaming event."))?;
        if chunk.error.is_some() || chunk.choices.len() > 1 {
            return Err(Failure::Message(
                "The model could not complete this response.",
            ));
        }
        if let Some(choice) = chunk.choices.into_iter().next() {
            if self.completion.is_some() || choice.index.is_some_and(|index| index != 0) {
                return Err(Failure::Message(
                    "The model returned an unexpected completion choice.",
                ));
            }
            let text = choice
                .delta
                .content
                .map_or(Cow::Borrowed(""), |text| text.0);
            let reasoning = choice
                .delta
                .reasoning_content
                .or(choice.delta.reasoning)
                .map_or(Cow::Borrowed(""), |text| text.0);
            self.text.push_str(&text);
            self.reasoning.push_str(&reasoning);
            delta(text, reasoning)?;
            if let Some(reason) = choice.finish_reason {
                self.completion = Some(match reason.0.as_ref() {
                    "stop" => Completion::Finished,
                    "length" => Completion::TokenLimit,
                    "content_filter" => {
                        return Err(Failure::Message("The model filtered this response."));
                    }
                    _ => {
                        return Err(Failure::Message(
                            "The model ended with an unsupported completion reason.",
                        ));
                    }
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_callbacks_borrow_unescaped_text_and_own_escaped_text() -> Result<()> {
        let mut sse = Sse::default();
        let mut received = Vec::new();
        sse.push(
            concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"Rain 雨\",\"reasoning\":\"Consider\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"\\n\\u96ea\",\"reasoning_content\":\"\\tthought\"},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n",
            ).as_bytes(),
            &mut |text, reasoning| {
                if received.is_empty() {
                    assert!(matches!(text, Cow::Borrowed(_)));
                    assert!(matches!(reasoning, Cow::Borrowed(_)));
                } else {
                    assert!(matches!(text, Cow::Owned(_)));
                    assert!(matches!(reasoning, Cow::Owned(_)));
                }
                received.push((text.into_owned(), reasoning.into_owned()));
                Ok(())
            },
        )?;
        assert_eq!(sse.finish()?, Completion::Finished);
        assert_eq!(
            received,
            [
                ("Rain 雨".to_owned(), "Consider".to_owned()),
                ("\n雪".to_owned(), "\tthought".to_owned()),
            ]
        );
        assert_eq!(
            sse.into_answer(),
            ("Rain 雨\n雪".into(), "Consider\tthought".into())
        );
        Ok(())
    }

    #[test]
    fn framer_lends_one_event_and_retains_the_unconsumed_input() -> Result<()> {
        let mut framer = Framer::default();
        let mut bytes = b"event: message\r\ndata: first\r\n\r\ndata: second\n\n".as_slice();
        let frame = framer
            .next_event(&mut bytes)?
            .ok_or(Failure::Message("Missing first frame"))?;
        assert_eq!(frame.data, b"first");
        assert_eq!(frame.event, b"message");
        assert_eq!(bytes, b"\ndata: second\n\n");
        let frame = framer
            .next_event(&mut bytes)?
            .ok_or(Failure::Message("Missing second frame"))?;
        assert_eq!(frame.data, b"second");
        assert_eq!(frame.event, b"");
        assert!(framer.next_event(&mut bytes)?.is_none());
        Ok(())
    }

    #[test]
    fn failed_bom_prefix_stays_in_its_original_line() -> Result<()> {
        for prefix in [b"\xef".as_slice(), b"\xef\xbb"] {
            let mut framer = Framer::default();
            let wire = [prefix, b"data: ignored\n\ndata: accepted\n\n"].concat();
            let mut bytes = wire.as_slice();
            let frame = framer
                .next_event(&mut bytes)?
                .ok_or(Failure::Message("Missing post-prefix frame"))?;
            assert_eq!(frame.data, b"accepted");
            assert!(framer.next_event(&mut bytes)?.is_none());
        }
        Ok(())
    }
}
