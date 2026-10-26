//! Conversation-only messages with cached layout and viewport culling.
//! Application diagnostics never enter this model.
use eframe::egui::{self, Color32, FontId, RichText, Vec2};
use std::sync::Arc;

const MAX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Role {
    User,
    Assistant,
}

struct LayoutCache {
    width: f32,
    pixels_per_point: f32,
    color: Color32,
    reasoning_color: Color32,
    body: Arc<egui::Galley>,
    reasoning: Option<Arc<egui::Galley>>,
}

pub(super) struct Message {
    id: u64,
    pub(super) role: Role,
    pub(super) body: String,
    reasoning: String,
    pending: bool,
    show_reasoning: bool,
    cache: Option<LayoutCache>,
}

#[derive(Default)]
pub(super) struct Transcript {
    pub(super) messages: Vec<Message>,
    next_id: u64,
}

impl Transcript {
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
        }) {
            return;
        }
        let body = ui.fonts_mut(|fonts| {
            fonts.layout(self.body.clone(), FontId::proportional(16.0), color, width)
        });
        let reasoning = (!self.reasoning.is_empty()).then(|| {
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
            body,
            reasoning,
        });
    }

    fn height(&self) -> f32 {
        let Some(cache) = &self.cache else {
            return 100.0;
        };
        let mut height = 40.0 + 24.0 + 12.0 + cache.body.size().y.max(20.0);
        if let Some(reasoning) = &cache.reasoning {
            height += 32.0;
            if self.show_reasoning {
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
            if let Some(reasoning) = &cache.reasoning {
                child.add_space(8.0);
                let label = if self.show_reasoning {
                    "Hide thinking"
                } else {
                    "Show thinking"
                };
                if child.small_button(label).clicked() {
                    self.show_reasoning = !self.show_reasoning;
                }
                if self.show_reasoning {
                    child.add_space(4.0);
                    child.add(egui::Label::new(Arc::clone(reasoning)).selectable(true));
                }
            }
        }
    }
}
