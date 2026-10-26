//! Unicode-safe incremental speech segmentation with a bounded text lifetime.

use super::{Duration, Instant, MAX_SEGMENT, MAX_TEXT, VecDeque};

const LATENCY: Duration = Duration::from_millis(700);

#[derive(Default)]
pub(super) struct Text {
    pending: String,
    units: usize,
    since: Option<Instant>,
    pub(super) finished: bool,
    #[cfg(test)]
    compacted_bytes: usize,
}

impl Text {
    pub(super) fn append(&mut self, text: &str, now: Instant) -> Result<(), &'static str> {
        let units = text.encode_utf16().take(MAX_TEXT + 1).count();
        if self.finished || text.contains('\0') || units > MAX_TEXT.saturating_sub(self.units) {
            return Err(
                "Streaming read aloud exceeded its 32,768 UTF-16 character limit or received invalid text.",
            );
        }
        self.units += units;
        self.pending.push_str(text);
        // The fallback is an idle timeout, not a periodic word-fragment timer.
        // Keep collecting a continuous clause until punctuation or MAX_SEGMENT
        // releases it; otherwise each brief fragment restarts model synthesis.
        if !text.is_empty() && !self.pending.trim().is_empty() {
            self.since = Some(now);
        }
        Ok(())
    }

    pub(super) fn ready(&mut self, now: Instant) -> VecDeque<String> {
        let mut result = VecDeque::new();
        let expired = self
            .since
            .is_some_and(|since| now.duration_since(since) >= LATENCY);
        let mut consumed = 0;
        loop {
            let pending = self.pending[consumed..].trim_start();
            consumed = self.pending.len() - pending.len();
            let maximum = pending
                .char_indices()
                .nth(MAX_SEGMENT)
                .map(|(index, _)| index);
            let limit = maximum.unwrap_or(pending.len());
            let sentence = pending[..limit]
                .char_indices()
                .find_map(|(index, character)| {
                    let end = index + character.len_utf8();
                    (character == '\n'
                        || (matches!(character, '.' | '!' | '?' | '…')
                            && pending[end..]
                                .chars()
                                .next()
                                .is_some_and(char::is_whitespace)))
                    .then_some(end)
                });
            let boundary = sentence.or_else(|| {
                if maximum.is_some() {
                    Some(
                        pending[..limit]
                            .char_indices()
                            .rev()
                            .find(|(index, character)| {
                                *index >= limit / 2 && character.is_whitespace()
                            })
                            .map_or(limit, |(index, _)| index),
                    )
                } else if self.finished {
                    Some(pending.len())
                } else if expired {
                    pending
                        .char_indices()
                        .rev()
                        .find(|(_, character)| character.is_whitespace())
                        .map(|(index, _)| index)
                } else {
                    None
                }
            });
            let Some(boundary) = boundary.filter(|boundary| *boundary > 0) else {
                break;
            };
            let piece = pending[..boundary].trim();
            if !piece.is_empty() {
                result.push_back(piece.to_owned());
            }
            consumed += boundary;
        }
        if consumed == self.pending.len() {
            self.pending.clear();
            self.since = None;
        } else {
            if consumed > 0 {
                #[cfg(test)]
                {
                    self.compacted_bytes += self.pending.len() - consumed;
                }
                self.pending.drain(..consumed);
            }
            if !result.is_empty() {
                self.since = Some(now);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{Instant, LATENCY, MAX_SEGMENT, MAX_TEXT, Text};

    #[test]
    fn streaming_sentences_start_before_finish_and_final_tail_is_not_repeated()
    -> Result<(), &'static str> {
        let now = Instant::now();
        let mut text = Text::default();
        text.append("Hello 🙂. The next", now)?;
        assert_eq!(text.ready(now), ["Hello 🙂."]);
        text.append(" sentence! tail", now)?;
        assert_eq!(text.ready(now), ["The next sentence!"]);
        text.finished = true;
        assert_eq!(text.ready(now), ["tail"]);
        assert!(text.ready(now).is_empty());
        assert!(text.append("late", now).is_err());
        Ok(())
    }

    #[test]
    fn streaming_latency_flush_preserves_partial_word_and_unicode_limit() -> Result<(), &'static str>
    {
        let now = Instant::now();
        let mut text = Text::default();
        text.append("A clause without punctuation part", now)?;
        assert!(text.ready(now).is_empty());
        assert_eq!(text.ready(now + LATENCY), ["A clause without punctuation"]);
        text.append("ial 🙂", now + LATENCY)?;
        text.finished = true;
        assert_eq!(text.ready(now + LATENCY), ["partial 🙂"]);
        let mut text = Text::default();
        text.append(&"🙂".repeat(MAX_SEGMENT + 5), now)?;
        let ready = text.ready(now);
        assert_eq!(
            ready.front().map(|part| part.chars().count()),
            Some(MAX_SEGMENT)
        );
        text.finished = true;
        assert_eq!(
            text.ready(now).front().map(|part| part.chars().count()),
            Some(5)
        );
        Ok(())
    }

    #[test]
    fn streaming_latency_waits_for_idle_while_words_keep_arriving() -> Result<(), &'static str> {
        let now = Instant::now();
        let mut text = Text::default();
        text.append("A clause", now)?;
        let almost_latency = LATENCY
            .checked_sub(std::time::Duration::from_millis(1))
            .ok_or("latency must exceed one millisecond")?;
        let later = now + almost_latency;
        text.append(" continuing", later)?;
        assert!(text.ready(now + LATENCY).is_empty());
        assert!(text.ready(later + almost_latency).is_empty());
        assert_eq!(text.ready(later + LATENCY), ["A clause"]);
        text.append(" softly.", later + LATENCY)?;
        text.finished = true;
        assert_eq!(text.ready(later + LATENCY), ["continuing softly."]);
        assert!(text.ready(later + LATENCY).is_empty());
        Ok(())
    }

    #[test]
    fn streaming_empty_append_does_not_defer_an_idle_clause() -> Result<(), &'static str> {
        let now = Instant::now();
        let mut text = Text::default();
        text.append("A paused clause tail", now)?;
        text.append("", now + LATENCY)?;
        assert_eq!(text.ready(now + LATENCY), ["A paused clause"]);
        text.finished = true;
        assert_eq!(text.ready(now + LATENCY), ["tail"]);
        Ok(())
    }

    #[test]
    fn streaming_text_limit_counts_all_consumed_utf16_and_rejects_nul() -> Result<(), &'static str>
    {
        let now = Instant::now();
        let mut text = Text::default();
        text.append(&"🙂".repeat(MAX_TEXT / 2), now)?;
        assert!(!text.ready(now).is_empty());
        assert!(text.append("x", now).is_err());
        assert!(Text::default().append("bad\0text", now).is_err());
        Ok(())
    }

    #[test]
    fn streaming_many_sentences_compact_only_the_final_unconsumed_tail() -> Result<(), &'static str>
    {
        let now = Instant::now();
        let mut text = Text::default();
        text.append(&format!("{}tail", "Sentence 🙂. ".repeat(1000)), now)?;
        let ready = text.ready(now);
        assert_eq!(ready.len(), 1000);
        assert!(ready.iter().all(|part| part == "Sentence 🙂."));
        assert_eq!(text.pending, "tail");
        assert_eq!(text.compacted_bytes, "tail".len());
        text.finished = true;
        assert_eq!(text.ready(now), ["tail"]);
        assert_eq!(text.compacted_bytes, "tail".len());
        Ok(())
    }
}
