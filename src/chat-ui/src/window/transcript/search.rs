//! Plain-text find state. Only visible message bodies participate, never reasoning.
use super::{Message, egui};
use std::{collections::BTreeMap, ops::Range};
#[cfg(test)]
mod performance_tests;
mod shortcuts;

const MAX_MATCHES: usize = 10_000;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Match {
    pub(super) message: u64,
    pub(super) range: Range<usize>,
}

#[derive(Default, PartialEq, Eq)]
enum Visibility {
    #[default]
    Closed,
    Open,
    Focus,
}

struct CachedMatches {
    // Streaming appends preserve an entry; replacement explicitly removes it.
    // Restored transcripts start with a fresh Search.
    body_bytes: usize,
    ranges: Vec<Range<usize>>,
    // Only the changing tail retains query-sized continuation scratch.
    continuation: Option<FoldedMatcher>,
}

impl CachedMatches {
    fn scan(body: &str, query: &str, tail: bool) -> (Self, usize) {
        let mut matcher = FoldedMatcher::new(query.as_bytes());
        let mut ranges = Vec::new();
        let scanned = matcher.extend(body, 0, &mut ranges);
        (
            Self {
                body_bytes: body.len(),
                ranges,
                continuation: tail.then_some(matcher),
            },
            scanned,
        )
    }
}

#[derive(Default)]
pub(super) struct Search {
    visibility: Visibility,
    pub(super) query: String,
    pub(super) matches: Vec<Match>,
    pub(super) active: usize,
    pub(super) revision: u64,
    pub(super) target: Option<Match>,
    pub(super) latest: bool,
    dirty: bool,
    limited: bool,
    cached_query: String,
    cached_messages: BTreeMap<u64, CachedMatches>,
    #[cfg(test)]
    scanned_messages: usize,
    #[cfg(test)]
    scanned_bytes: usize,
}

impl Search {
    pub(super) const fn invalidate(&mut self) {
        self.dirty = true;
    }

    pub(super) fn replaced(&mut self, message: u64) {
        self.cached_messages.remove(&message);
        self.invalidate();
    }

    pub(super) fn refresh(&mut self, messages: &[Message]) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let old_matches = std::mem::take(&mut self.matches);
        let previous = old_matches.get(self.active).cloned();
        self.limited = false;
        if self.query.is_empty() {
            self.cached_query.clear();
            self.cached_messages.clear();
        } else {
            let query = self.query.to_lowercase();
            if self.cached_query != query {
                self.cached_messages.clear();
                query.clone_into(&mut self.cached_query);
            }
            let mut previous_cache = std::mem::take(&mut self.cached_messages);
            for message in messages {
                let tail = messages.last().is_some_and(|last| last.id == message.id);
                let mut cached = previous_cache.remove(&message.id).unwrap_or_else(|| {
                    let (cached, scanned) = CachedMatches::scan(&message.body, &query, tail);
                    #[cfg(test)]
                    {
                        self.scanned_messages += 1;
                        self.scanned_bytes += scanned;
                    }
                    let _ = scanned;
                    cached
                });
                if cached.body_bytes != message.body.len() {
                    let scanned = if tail
                        && cached.body_bytes < message.body.len()
                        && let Some(matcher) = &mut cached.continuation
                    {
                        matcher.extend(
                            &message.body[cached.body_bytes..],
                            cached.body_bytes,
                            &mut cached.ranges,
                        )
                    } else {
                        let (fresh, scanned) = CachedMatches::scan(&message.body, &query, tail);
                        cached = fresh;
                        scanned
                    };
                    cached.body_bytes = message.body.len();
                    #[cfg(test)]
                    {
                        self.scanned_messages += 1;
                        self.scanned_bytes += scanned;
                    }
                    let _ = scanned;
                }
                if !tail {
                    cached.continuation = None;
                }
                for range in &cached.ranges {
                    if self.matches.len() == MAX_MATCHES {
                        self.limited = true;
                        break;
                    }
                    self.matches.push(Match {
                        message: message.id,
                        range: range.clone(),
                    });
                }
                self.cached_messages.insert(message.id, cached);
                if self.limited {
                    break;
                }
            }
        }
        self.active = previous
            .as_ref()
            .and_then(|previous| self.matches.iter().position(|found| found == previous))
            .unwrap_or_else(|| self.active.min(self.matches.len().saturating_sub(1)));
        if old_matches != self.matches {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn navigate(&mut self, backwards: bool) {
        if self.matches.is_empty() {
            return;
        }
        self.active = if backwards {
            self.active.checked_sub(1).unwrap_or(self.matches.len() - 1)
        } else {
            (self.active + 1) % self.matches.len()
        };
        self.target = self.matches.get(self.active).cloned();
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn button(&mut self, ui: &mut egui::Ui) {
        let response = ui
            .add(
                egui::Button::new("")
                    .min_size(egui::Vec2::splat(32.0))
                    .selected(self.visibility != Visibility::Closed),
            )
            .accessible_name("Find")
            .on_hover_text("Find in this conversation · Ctrl+F");
        let center = response.rect.center();
        let stroke = ui.style().interact(&response).fg_stroke;
        let painter = ui.painter_at(response.rect);
        painter.circle_stroke(center - egui::vec2(2.0, 2.0), 5.0, stroke);
        painter.line_segment(
            [center + egui::vec2(2.0, 2.0), center + egui::vec2(7.0, 7.0)],
            stroke,
        );
        if response.clicked() {
            self.visibility = if self.visibility == Visibility::Closed {
                Visibility::Focus
            } else {
                Visibility::Closed
            };
            if self.visibility == Visibility::Closed {
                self.query.clear();
                self.target = None;
                self.invalidate();
            }
        }
    }

    pub(super) fn bar(&mut self, ui: &mut egui::Ui, messages: &[Message]) {
        let accepts_input = ui.memory(|memory| memory.allows_interaction(ui.layer_id()));
        if accepts_input
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::F))
        {
            self.visibility = Visibility::Focus;
        }
        if self.visibility == Visibility::Closed {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            let field_id = ui.make_persistent_id("find-query");
            if accepts_input {
                self.navigate_on_enter(ui.ctx(), field_id);
            }
            let input = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .id(field_id)
                    .return_key(None)
                    .hint_text("Find in conversation")
                    .desired_width(190.0)
                    .char_limit(256),
            );
            input.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Find in conversation")
            });
            if accepts_input && self.visibility == Visibility::Focus {
                input.request_focus();
                self.visibility = Visibility::Open;
            }
            if input.changed() {
                self.active = 0;
                self.matches.clear();
                self.invalidate();
                self.revision = self.revision.wrapping_add(1);
                self.refresh(messages);
                self.target = self.matches.first().cloned();
            }
            self.refresh(messages);
            let count = self.matches.len();
            let label = if count == 0 {
                if self.query.is_empty() {
                    "Message text only".to_owned()
                } else {
                    "No matches".to_owned()
                }
            } else {
                format!(
                    "{} of {count}{}",
                    self.active + 1,
                    if self.limited { "+" } else { "" }
                )
            };
            ui.label(label);
            let previous = ui.add_enabled(count > 0, egui::Button::new("Previous match"));
            let next = ui.add_enabled(count > 0, egui::Button::new("Next match"));
            if previous.clicked()
                || (accepts_input
                    && ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::SHIFT, egui::Key::F3)
                    }))
            {
                self.navigate(true);
            } else if next.clicked()
                || (accepts_input
                    && ui
                        .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F3)))
            {
                self.navigate(false);
            }
        });
    }
}

#[cfg(test)]
fn find_ranges(body: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut matcher = FoldedMatcher::new(query.as_bytes());
    let mut ranges = Vec::new();
    matcher.extend(body, 0, &mut ranges);
    ranges
}

// KMP keeps matching work linear even for repeated prefixes. Both scratch
// vectors depend on the query length, never the retained message length.
struct FoldedMatcher {
    query: Vec<u8>,
    fallback: Vec<usize>,
    starts: Vec<usize>,
    next: usize,
    matched: usize,
}

impl FoldedMatcher {
    fn new(query: &[u8]) -> Self {
        let mut fallback = vec![0; query.len()];
        let mut matched = 0;
        for index in 1..query.len() {
            while matched > 0 && query[index] != query[matched] {
                matched = fallback[matched - 1];
            }
            if query[index] == query[matched] {
                matched += 1;
            }
            fallback[index] = matched;
        }
        Self {
            query: query.to_vec(),
            fallback,
            starts: vec![0; query.len()],
            next: 0,
            matched: 0,
        }
    }

    fn extend(&mut self, body: &str, offset: usize, ranges: &mut Vec<Range<usize>>) -> usize {
        if ranges.len() > MAX_MATCHES {
            return 0;
        }
        if body.is_ascii() {
            for (start, byte) in body.bytes().enumerate() {
                if self.push(
                    byte.to_ascii_lowercase(),
                    offset + start,
                    offset + start + 1,
                    ranges,
                ) {
                    return start + 1;
                }
            }
        } else {
            // Preserve scalar lowercasing and original UTF-8 highlight bounds,
            // including matches that cross successive streamed append calls.
            for (start, character) in body.char_indices() {
                let end = start + character.len_utf8();
                for lower in character.to_lowercase() {
                    let mut encoded = [0; 4];
                    for &byte in lower.encode_utf8(&mut encoded).as_bytes() {
                        if self.push(byte, offset + start, offset + end, ranges) {
                            return end;
                        }
                    }
                }
            }
        }
        body.len()
    }

    // True means the global result limit's one-match lookahead is complete.
    fn push(&mut self, byte: u8, start: usize, end: usize, ranges: &mut Vec<Range<usize>>) -> bool {
        self.starts[self.next] = start;
        self.next += 1;
        if self.next == self.query.len() {
            self.next = 0;
        }
        while self.matched > 0 && byte != self.query[self.matched] {
            self.matched = self.fallback[self.matched - 1];
        }
        if byte == self.query[self.matched] {
            self.matched += 1;
        }
        if self.matched == self.query.len() {
            let range = self.starts[self.next]..end;
            if ranges
                .last()
                .is_none_or(|previous| previous.end <= range.start)
            {
                ranges.push(range);
            }
            // match_indices consumes each folded match before searching again,
            // even when two matches map to the same expanded source scalar.
            self.matched = 0;
        }
        ranges.len() > MAX_MATCHES
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::transcript::{Role, Transcript};

    #[test]
    fn unicode_find_preserves_original_boundaries_and_excludes_reasoning() {
        let mut transcript = Transcript::default();
        transcript.push(Role::User, "Élan İ 界 🌱 éLAN".to_owned());
        transcript.push(Role::Assistant, "Visible answer".to_owned());
        transcript.append("secret Élan", true);
        let mut search = Search {
            query: "ÉLAN".to_owned(),
            ..Search::default()
        };
        search.invalidate();
        search.refresh(&transcript.messages);
        assert_eq!(search.matches.len(), 2);
        assert_eq!(
            &transcript.messages[0].body[search.matches[0].range.clone()],
            "Élan"
        );
        assert_eq!(
            &transcript.messages[0].body[search.matches[1].range.clone()],
            "éLAN"
        );
        assert_eq!(find_ranges("İ", "i"), vec![0..2]);
        assert_eq!(find_ranges("界🌱界", "界"), vec![0..3, 7..10]);
        search.navigate(true);
        assert_eq!(search.active, 1);
        search.navigate(false);
        assert_eq!(search.active, 0);
        transcript.append(" ÉLAN", false);
        search.invalidate();
        search.refresh(&transcript.messages);
        assert_eq!(search.matches.len(), 3);
        assert_eq!(search.active, 0);
    }
}
