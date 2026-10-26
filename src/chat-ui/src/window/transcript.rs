//! Conversation-only messages with cached layout and viewport culling.
//! Application diagnostics never enter this model.
use eframe::egui::{self, Color32, FontId, RichText, Vec2};
use std::sync::Arc;

const MAX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum Role {
    User,
    Assistant,
}

struct LayoutCache {
    width: f32,
    pixels_per_point: f32,
    color: Color32,
    reasoning_color: Color32,
    show_reasoning: bool,
    body: Arc<egui::Galley>,
    reasoning: Option<Arc<egui::Galley>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Message {
    #[serde(skip)]
    id: u64,
    pub(super) role: Role,
    pub(super) body: String,
    reasoning: String,
    #[serde(skip)]
    pending: bool,
    #[serde(skip)]
    show_reasoning: bool,
    #[serde(skip)]
    cache: Option<LayoutCache>,
}

#[derive(Default, serde::Serialize)]
pub(super) struct Transcript {
    pub(super) messages: Vec<Message>,
    #[serde(skip)]
    next_id: u64,
}

impl<'de> serde::Deserialize<'de> for Transcript {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct StoredTranscript {
            messages: Vec<Message>,
        }
        let stored = StoredTranscript::deserialize(deserializer)?;
        let mut transcript = Self::default();
        for mut message in stored.messages {
            transcript.next_id = transcript.next_id.wrapping_add(1);
            message.id = transcript.next_id;
            transcript.messages.push(message);
        }
        if !transcript.valid_saved() {
            return Err(serde::de::Error::custom(
                "Saved transcript exceeds its text or message limit.",
            ));
        }
        Ok(transcript)
    }
}

impl Transcript {
    pub(super) fn valid_saved(&self) -> bool {
        self.messages.len() <= 512
            && self.messages.iter().fold(0_usize, |total, message| {
                total
                    .saturating_add(message.body.len())
                    .saturating_add(message.reasoning.len())
            }) <= MAX_BYTES
    }


    pub(super) fn clear(&mut self) {
        self.messages.clear();
    }

    pub(super) fn remove_empty_response(&mut self) {
        if let Some(message) = self.messages.last_mut() {
            message.pending = false;
        }
        if self.messages.last().is_some_and(|message| {
            message.role == Role::Assistant
                && message.body.is_empty()
                && message.reasoning.is_empty()
        }) {
            self.messages.pop();
        }
    }

    pub(super) fn push(&mut self, role: Role, body: String) {
        self.next_id = self.next_id.wrapping_add(1);
        self.messages.push(Message {
            id: self.next_id,
            role,
            body,
            reasoning: String::new(),
            pending: role == Role::Assistant,
            show_reasoning: false,
            cache: None,
        });
        self.trim();
    }

    pub(super) fn append(&mut self, text: &str, reasoning: bool) {
        if let Some(message) = self.messages.last_mut()
            && message.role == Role::Assistant
        {
            if reasoning {
                message.reasoning.push_str(text);
            } else {
                message.body.push_str(text);
            }
            message.cache = None;
        }
        self.trim();
    }

    pub(super) fn stage(&mut self) {
        if let Some(message) = self.messages.last_mut() {
            message.pending = false;
        }
        if self.messages.last().is_some_and(|message| {
            message.role == Role::Assistant
                && (!message.body.is_empty() || !message.reasoning.is_empty())
        }) {
            self.push(Role::Assistant, String::new());
        }
    }

    fn trim(&mut self) {
        let mut bytes: usize = self
            .messages
            .iter()
            .map(|message| message.body.len().saturating_add(message.reasoning.len()))
            .sum();
        while bytes > MAX_BYTES && self.messages.len() > 2 {
            let removed = self.messages.remove(0);
            bytes = bytes.saturating_sub(removed.body.len() + removed.reasoning.len());
        }
    }

    pub(super) fn plain_text(&self) -> String {
        let messages = self
            .messages
            .iter()
            .filter(|message| !message.body.is_empty())
            .map(|message| {
                (
                    if message.role == Role::User {
                        "You"
                    } else {
                        "Swift Bonsai"
                    },
                    message.body.as_str(),
                )
            });
        let capacity = messages
            .clone()
            .map(|(label, body)| label.len() + body.len() + 3)
            .sum::<usize>()
            .saturating_sub(2);
        let mut text = String::with_capacity(capacity);
        for (label, body) in messages {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(label);
            text.push('\n');
            text.push_str(body);
        }
        text
    }

    pub(super) fn show(&mut self, ui: &mut egui::Ui, width: f32) {
        let width = width.max(120.0);
        for message in &mut self.messages {
            message.layout(ui, width - 40.0);
            let height = message.height();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
            if ui.is_rect_visible(rect) {
                message.show(ui, rect);
            }
            ui.add_space(16.0);
        }
    }
}

impl Message {
    fn layout(&mut self, ui: &egui::Ui, width: f32) {
        let color = ui.visuals().text_color();
        let reasoning_color = ui.visuals().weak_text_color();
        let pixels_per_point = ui.ctx().pixels_per_point();
        if self.cache.as_ref().is_some_and(|cache| {
            (cache.width - width).abs() < 0.5
                && cache.pixels_per_point.to_bits() == pixels_per_point.to_bits()
                && cache.color == color
                && cache.reasoning_color == reasoning_color
                && cache.show_reasoning == self.show_reasoning
        }) {
            return;
        }
        let body = ui.fonts_mut(|fonts| {
            fonts.layout(self.body.clone(), FontId::proportional(16.0), color, width)
        });
        let reasoning = (self.show_reasoning && !self.reasoning.is_empty()).then(|| {
            ui.fonts_mut(|fonts| {
                fonts.layout(
                    self.reasoning.clone(),
                    FontId::proportional(14.0),
                    reasoning_color,
                    width,
                )
            })
        });
        self.cache = Some(LayoutCache {
            width,
            pixels_per_point,
            color,
            reasoning_color,
            show_reasoning: self.show_reasoning,
            body,
            reasoning,
        });
    }

    fn height(&self) -> f32 {
        let Some(cache) = &self.cache else {
            return 100.0;
        };
        let mut height = 40.0 + 24.0 + 12.0 + cache.body.size().y.max(20.0);
        if !self.reasoning.is_empty() {
            height += 32.0;
            if self.show_reasoning
                && let Some(reasoning) = &cache.reasoning
            {
                height += reasoning.size().y + 8.0;
            }
        }
        height
    }

    fn show(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let fill = if self.role == Role::User {
            ui.visuals().faint_bg_color
        } else {
            ui.visuals().extreme_bg_color
        };
        ui.painter().rect_filled(rect, 12.0, fill);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("message", self.id))
                .max_rect(rect.shrink(20.0))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        child.horizontal(|ui| {
            let (avatar, name) = if self.role == Role::User {
                ("●", "You")
            } else {
                ("B", "Swift Bonsai")
            };
            ui.label(
                RichText::new(avatar)
                    .color(ui.visuals().selection.bg_fill)
                    .size(18.0),
            );
            ui.label(RichText::new(name).strong().size(14.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("Copy")
                    .on_hover_text("Copy this message")
                    .clicked()
                {
                    ui.ctx().copy_text(self.body.clone());
                }
            });
        });
        child.add_space(8.0);
        if let Some(cache) = &self.cache {
            if self.body.is_empty() && self.pending {
                child.label(RichText::new("Thinking…").weak().italics());
            } else if !self.body.is_empty() {
                child.add(egui::Label::new(Arc::clone(&cache.body)).selectable(true));
            }
            if !self.reasoning.is_empty() {
                child.add_space(8.0);
                let label = if self.show_reasoning {
                    "Hide thinking"
                } else {
                    "Show thinking"
                };
                if child.small_button(label).clicked() {
                    self.show_reasoning = !self.show_reasoning;
                    child.ctx().request_repaint();
                }
                if self.show_reasoning
                    && let Some(reasoning) = &cache.reasoning
                {
                    child.add_space(4.0);
                    child.add(egui::Label::new(Arc::clone(reasoning)).selectable(true));
                }
            }
        }
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;

    #[test]
    fn restored_transcript_retains_text_without_layout_or_pending_generation()
    -> Result<(), serde_json::Error> {
        let mut transcript = Transcript::default();
        transcript.push(Role::User, "界 🌱".to_owned());
        transcript.push(Role::Assistant, "Interrupted draft".to_owned());
        transcript.append("Retained reasoning", true);
        let bytes = serde_json::to_vec(&transcript)?;
        let mut restored: Transcript = serde_json::from_slice(&bytes)?;
        assert_eq!(restored.plain_text(), transcript.plain_text());
        assert_eq!(restored.messages[1].reasoning, "Retained reasoning");
        assert!(!restored.messages[1].pending);
        assert!(
            restored
                .messages
                .iter()
                .all(|message| message.cache.is_none())
        );
        assert_ne!(restored.messages[0].id, restored.messages[1].id);
        restored.push(Role::User, "New turn".to_owned());
        assert_ne!(restored.messages[1].id, restored.messages[2].id);
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("cache"));
        assert!(!text.contains("pending"));
        assert!(!text.contains("next_id"));
        Ok(())
    }
}
