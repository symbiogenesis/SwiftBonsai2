//! Plain-text find state. Only visible message bodies participate, never reasoning.
use super::{Message, egui};
use std::ops::Range;

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
}

impl Search {
    pub(super) const fn invalidate(&mut self) {
        self.dirty = true;
    }

    pub(super) fn refresh(&mut self, messages: &[Message]) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let old_matches = std::mem::take(&mut self.matches);
        let previous = old_matches.get(self.active).cloned();
        self.limited = false;
        if !self.query.is_empty() {
            let query = self.query.to_lowercase();
            for message in messages {
                for range in find_ranges(&message.body, &query) {
                    if self.matches.len() == MAX_MATCHES {
                        self.limited = true;
                        break;
                    }
                    self.matches.push(Match {
                        message: message.id,
                        range,
                    });
                }
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

    pub(super) fn bar(&mut self, ui: &mut egui::Ui, messages: &[Message]) {
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::F)) {
            self.visibility = Visibility::Focus;
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Find")
                .on_hover_text("Find in this conversation · Ctrl+F")
                .clicked()
            {
                self.visibility = if self.visibility == Visibility::Closed {
                    Visibility::Focus
                } else {
                    Visibility::Closed
                };
                if self.visibility == Visibility::Closed {
                    self.query.clear();
                    self.invalidate();
                }
            }
            if self.visibility != Visibility::Closed {
                let input = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Find in conversation")
                        .desired_width(190.0)
                        .char_limit(256),
                );
                input.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Find in conversation")
                });
                if self.visibility == Visibility::Focus {
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
                    || ui
                        .input_mut(|input| input.consume_key(egui::Modifiers::SHIFT, egui::Key::F3))
                {
                    self.navigate(true);
                } else if next.clicked()
                    || ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F3))
                {
                    self.navigate(false);
                }
            }
            if ui
                .button("Latest")
                .on_hover_text("Jump to the latest message")
                .clicked()
            {
                self.latest = true;
                self.target = None;
            }
        });
    }
}

fn find_ranges(body: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    // Lowercasing may expand a Unicode scalar (for example İ). Map folded byte
    // boundaries back to complete original scalars so highlighting stays UTF-8 safe.
    let mut folded = String::new();
    let mut offsets = Vec::new();
    for (start, character) in body.char_indices() {
        let end = start + character.len_utf8();
        for lower in character.to_lowercase() {
            for _ in 0..lower.len_utf8() {
                offsets.push((start, end));
            }
            folded.push(lower);
        }
    }
    let mut ranges = Vec::new();
    for (start, found) in folded.match_indices(query) {
        let Some(&(original_start, _)) = offsets.get(start) else {
            continue;
        };
        let Some(&(_, original_end)) = offsets.get(start + found.len() - 1) else {
            continue;
        };
        let range = original_start..original_end;
        if ranges
            .last()
            .is_none_or(|previous: &Range<usize>| previous.end <= range.start)
        {
            ranges.push(range);
            if ranges.len() > MAX_MATCHES {
                break;
            }
        }
    }
    ranges
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
