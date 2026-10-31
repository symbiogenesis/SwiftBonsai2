//! Conversation-only messages with cached layout and viewport culling.
//! Application diagnostics never enter this model.
use eframe::egui::{self, Color32, FontId, RichText, Vec2};
use std::{ops::Range, sync::Arc};

#[cfg(test)]
mod cache_tests;
mod search;

const MAX_BYTES: usize = 4 * 1024 * 1024;
pub(super) const MAX_MESSAGES: usize = 512;
const SPEAKER_SIZE: f32 = 36.0;
const SPEAKER_FOOTER_HEIGHT: f32 = 64.0;

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
    body_revision: u64,
    // Search owns the ranges. For an unchanged body and effective query, the
    // visible ranges are always the same prefix; eviction may change its count.
    highlights: HighlightKey,
    selected: Option<Range<usize>>,
    // Only the active result owns underline geometry. Navigation reuses the
    // highlighted galley instead of rebuilding every match's layout section.
    underline: Vec<[egui::Pos2; 2]>,
    highlight_fill: Color32,
    highlight_color: Color32,
    reasoning_bytes: usize,
    body: Arc<egui::Galley>,
    reasoning: Option<Arc<egui::Galley>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct HighlightKey {
    query_revision: u64,
    count: usize,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Message {
    #[serde(skip)]
    pub(super) id: u64,
    pub(super) role: Role,
    pub(super) body: String,
    #[serde(skip)]
    body_revision: u64,
    reasoning: String,
    #[serde(skip)]
    pending: bool,
    #[serde(skip)]
    show_reasoning: bool,
    #[serde(skip)]
    cache: Option<LayoutCache>,
    #[cfg(test)]
    #[serde(skip)]
    body_layouts: usize,
    #[cfg(test)]
    #[serde(skip)]
    underline_layouts: usize,
    #[cfg(test)]
    #[serde(skip)]
    highlight_range_visits: std::cell::Cell<usize>,
}

#[derive(Default, serde::Serialize)]
pub(super) struct Transcript {
    pub(super) messages: Vec<Message>,
    // All production text mutations go through this type. Keep the running
    // total out of recovery data and rebuild it when messages are restored.
    #[serde(skip)]
    text_bytes: usize,
    #[serde(skip)]
    next_id: u64,
    #[serde(skip)]
    search: search::Search,
    #[cfg(test)]
    #[serde(skip)]
    trim_message_visits: usize,
    #[cfg(test)]
    #[serde(skip)]
    trim_batches: usize,
}

impl<'de> serde::Deserialize<'de> for Transcript {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct StoredTranscript {
            #[serde(deserialize_with = "super::persistence::deserialize_transcript_messages")]
            messages: Vec<Message>,
        }
        let stored = StoredTranscript::deserialize(deserializer)?;
        let mut transcript = Self::default();
        for mut message in stored.messages {
            transcript.next_id = transcript.next_id.wrapping_add(1);
            message.id = transcript.next_id;
            transcript.text_bytes = transcript.text_bytes.saturating_add(message.text_bytes());
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
        self.messages.len() <= MAX_MESSAGES
            && self.messages.iter().fold(0_usize, |total, message| {
                total
                    .saturating_add(message.body.len())
                    .saturating_add(message.reasoning.len())
            }) <= MAX_BYTES
    }

    pub(super) fn clear(&mut self) {
        self.messages.clear();
        self.text_bytes = 0;
        self.search = search::Search::default();
    }

    pub(super) fn remove_empty_response(&mut self) {
        self.search.invalidate();
        self.messages.pop_if(|message| {
            message.pending = false;
            message.role == Role::Assistant
                && message.body.is_empty()
                && message.reasoning.is_empty()
        });
    }

    pub(super) fn push(&mut self, role: Role, body: String) {
        self.search.invalidate();
        self.next_id = self.next_id.wrapping_add(1);
        self.text_bytes = self.text_bytes.saturating_add(body.len());
        self.messages.push(Message {
            id: self.next_id,
            role,
            body,
            body_revision: 0,
            reasoning: String::new(),
            pending: role == Role::Assistant,
            show_reasoning: false,
            cache: None,
            #[cfg(test)]
            body_layouts: 0,
            #[cfg(test)]
            underline_layouts: 0,
            #[cfg(test)]
            highlight_range_visits: std::cell::Cell::new(0),
        });
        self.trim();
    }

    pub(super) fn replace_response(&mut self, text: &str, reasoning: Option<&str>) {
        if self
            .messages
            .last()
            .is_none_or(|message| message.role != Role::Assistant)
        {
            self.push(Role::Assistant, String::new());
        }
        if let Some(message) = self.messages.last_mut() {
            self.search.replaced(message.id);
            self.text_bytes = self.text_bytes.saturating_sub(message.text_bytes());
            text.clone_into(&mut message.body);
            message.body_revision = message.body_revision.wrapping_add(1);
            reasoning
                .unwrap_or_default()
                .clone_into(&mut message.reasoning);
            self.text_bytes = self.text_bytes.saturating_add(message.text_bytes());
            message.cache = None;
        }
        self.trim();
        self.remove_empty_response();
    }

    pub(super) fn append(&mut self, text: &str, reasoning: bool) {
        if !reasoning {
            self.search.appended();
        }
        if let Some(message) = self.messages.last_mut()
            && message.role == Role::Assistant
        {
            if reasoning {
                message.reasoning.push_str(text);
            } else {
                message.body.push_str(text);
                if !text.is_empty() {
                    message.body_revision = message.body_revision.wrapping_add(1);
                }
            }
            self.text_bytes = self.text_bytes.saturating_add(text.len());
            // Layout keys track answer revisions and reasoning lengths independently.
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
        if self.text_bytes <= MAX_BYTES && self.messages.len() <= MAX_MESSAGES {
            return;
        }
        let mut removed = 0;
        // Bound message ownership and per-frame traversal even when messages
        // contain little or no text. Preserve the final two messages under the
        // byte policy; inspect only the evicted prefix and move the suffix once.
        for message in self
            .messages
            .iter()
            .take(self.messages.len().saturating_sub(2))
        {
            if self.text_bytes <= MAX_BYTES && self.messages.len() - removed <= MAX_MESSAGES {
                break;
            }
            #[cfg(test)]
            {
                self.trim_message_visits += 1;
            }
            self.text_bytes = self.text_bytes.saturating_sub(message.text_bytes());
            removed += 1;
        }
        if removed > 0 {
            self.search.invalidate();
            drop(self.messages.drain(..removed));
            #[cfg(test)]
            {
                self.trim_batches += 1;
            }
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

    pub(super) fn search_bar(&mut self, ui: &mut egui::Ui) {
        self.search.bar(ui, &self.messages);
    }

    pub(super) fn find_button(&mut self, ui: &mut egui::Ui) {
        self.search.button(ui);
    }

    pub(super) fn latest_button(&mut self, ui: &mut egui::Ui, viewport: egui::Rect) {
        let rect = egui::Rect::from_center_size(
            egui::pos2(viewport.center().x, viewport.bottom() - 30.0),
            Vec2::splat(36.0),
        );
        let response = ui
            .place(rect, egui::Button::new("").corner_radius(18))
            .accessible_name("Latest")
            .on_hover_text("Jump to the latest message");
        let center = response.rect.center();
        let stroke = ui.style().interact(&response).fg_stroke;
        let painter = ui.painter_at(response.rect);
        painter.line_segment(
            [center - egui::vec2(0.0, 7.0), center + egui::vec2(0.0, 7.0)],
            stroke,
        );
        painter.add(egui::Shape::line(
            vec![
                center + egui::vec2(-6.0, 1.0),
                center + egui::vec2(0.0, 7.0),
                center + egui::vec2(6.0, 1.0),
            ],
            stroke,
        ));
        if response.clicked() {
            self.search.latest = true;
            self.search.target = None;
            ui.ctx().request_repaint();
        }
    }

    pub(super) fn close_find_on_escape(&mut self, ctx: &egui::Context) -> bool {
        self.search.close_on_escape(ctx)
    }

    pub(super) fn speech_text(&self, id: u64) -> Option<&str> {
        self.messages
            .iter()
            .find(|message| message.id == id && message.can_speak())
            .map(|message| message.body.as_str())
    }

    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        speaking_message: Option<u64>,
        speech_enabled: bool,
    ) -> Option<u64> {
        let mut clicked_speaker = None;
        self.search.refresh(&self.messages);
        let width = width.max(120.0);
        let mut remaining = self.search.matches.as_slice();
        for message in &mut self.messages {
            let count = remaining.partition_point(|found| found.message == message.id);
            let ranges = &remaining[..count];
            remaining = &remaining[count..];
            let selected = self
                .search
                .matches
                .get(self.search.active)
                .filter(|found| found.message == message.id)
                .map(|found| &found.range);
            let highlights = HighlightKey {
                query_revision: if ranges.is_empty() {
                    0
                } else {
                    self.search.query_revision
                },
                count,
            };
            message.layout(ui, width - 40.0, ranges, highlights, selected);
            let height = message.height();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
            let target = self
                .search
                .target
                .as_ref()
                .filter(|found| found.message == message.id);
            if ui.is_rect_visible(rect) || target.is_some() {
                let found_rect = message.show(ui, rect, target.map(|found| found.range.start));
                if message.role == Role::Assistant
                    && message.speaker(ui, rect, speaking_message, speech_enabled)
                {
                    clicked_speaker = Some(message.id);
                }
                if let Some(found_rect) = found_rect {
                    ui.scroll_to_rect(found_rect, Some(egui::Align::Center));
                    self.search.target = None;
                }
            }
            ui.add_space(16.0);
        }
        if self.search.latest {
            ui.scroll_to_cursor(Some(egui::Align::Max));
            self.search.latest = false;
        }
        clicked_speaker
    }
}

impl Message {
    const fn text_bytes(&self) -> usize {
        self.body.len().saturating_add(self.reasoning.len())
    }

    fn can_speak(&self) -> bool {
        self.role == Role::Assistant && !self.pending && !self.body.trim().is_empty()
    }

    fn speaker(
        &self,
        ui: &mut egui::Ui,
        bubble: egui::Rect,
        speaking_message: Option<u64>,
        speech_enabled: bool,
    ) -> bool {
        let playing = speaking_message == Some(self.id);
        let rect = egui::Rect::from_min_size(
            bubble.right_bottom() - Vec2::splat(SPEAKER_SIZE + 20.0),
            Vec2::splat(SPEAKER_SIZE),
        );
        let mut footer = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("message-speaker", self.id))
                .max_rect(rect),
        );
        let label = if playing {
            "Stop reading this response"
        } else {
            "Read this response aloud"
        };
        let response = footer.add_enabled(
            playing || (speech_enabled && self.can_speak()),
            egui::Button::new("")
                .min_size(rect.size())
                .selected(playing),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, response.enabled(), label)
        });
        let response = response.on_hover_text(if self.pending && !playing {
            "Available when this response finishes"
        } else {
            label
        });
        let painter = footer.painter_at(response.rect);
        let color = footer.style().interact(&response).fg_stroke.color;
        let center = response.rect.center();
        if playing {
            painter.rect_filled(
                egui::Rect::from_center_size(center, Vec2::splat(10.0)),
                1.0,
                color,
            );
        } else {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    center + egui::vec2(-8.0, -3.0),
                    center + egui::vec2(-4.0, 3.0),
                ),
                0.0,
                color,
            );
            let points = [(-4.0, -3.0), (1.0, -7.0), (1.0, 7.0), (-4.0, 3.0)]
                .map(|(x, y)| center + egui::vec2(x, y));
            painter.add(egui::Shape::convex_polygon(
                points.to_vec(),
                color,
                egui::Stroke::NONE,
            ));
            for radius in [5.0_f32, 9.0] {
                let points = [-0.8_f32, -0.4, 0.0, 0.4, 0.8].map(|angle| {
                    center + egui::vec2(radius.mul_add(angle.cos(), 1.0), radius * angle.sin())
                });
                painter.add(egui::Shape::line(
                    points.to_vec(),
                    egui::Stroke::new(1.5, color),
                ));
            }
        }
        response.clicked()
    }

    fn layout(
        &mut self,
        ui: &egui::Ui,
        width: f32,
        matches: &[search::Match],
        highlights: HighlightKey,
        selected: Option<&Range<usize>>,
    ) {
        let color = ui.visuals().text_color();
        let reasoning_color = ui.visuals().weak_text_color();
        let pixels_per_point = ui.ctx().pixels_per_point();
        let highlight_fill = ui.visuals().selection.bg_fill;
        let highlight_color = ui.visuals().selection.stroke.color;
        let same_geometry = self.cache.as_ref().is_some_and(|cache| {
            cache.width.to_bits() == width.to_bits()
                && cache.pixels_per_point.to_bits() == pixels_per_point.to_bits()
        });
        let body_unchanged = same_geometry
            && self.cache.as_ref().is_some_and(|cache| {
                cache.body_revision == self.body_revision
                    && cache.color == color
                    && cache.highlights == highlights
                    && (matches.is_empty()
                        || (cache.highlight_fill == highlight_fill
                            && cache.highlight_color == highlight_color))
            });
        let reasoning_unchanged = same_geometry
            && self.cache.as_ref().is_some_and(|cache| {
                cache.reasoning_bytes == self.reasoning.len()
                    && cache.reasoning_color == reasoning_color
            });
        let previous = self.cache.take();
        let body = if body_unchanged && let Some(cache) = &previous {
            Arc::clone(&cache.body)
        } else {
            #[cfg(test)]
            {
                self.body_layouts += 1;
            }
            self.layout_body(ui, width, matches)
        };
        let underline = if let Some(selected) = selected {
            if body_unchanged
                && let Some(cache) = &previous
                && cache.selected.as_ref() == Some(selected)
            {
                // Move the geometry below, after the other cache fields are read.
                None
            } else {
                #[cfg(test)]
                {
                    self.underline_layouts += 1;
                }
                Some(active_underline(
                    &self.body,
                    &body,
                    selected,
                    pixels_per_point,
                ))
            }
        } else {
            Some(Vec::new())
        };
        let reasoning = if !self.show_reasoning || self.reasoning.is_empty() {
            None
        } else if reasoning_unchanged
            && let Some(cache) = &previous
            && let Some(reasoning) = &cache.reasoning
        {
            Some(Arc::clone(reasoning))
        } else {
            Some(ui.fonts_mut(|fonts| {
                fonts.layout(
                    self.reasoning.clone(),
                    FontId::proportional(14.0),
                    reasoning_color,
                    width,
                )
            }))
        };
        self.cache = Some(LayoutCache {
            width,
            pixels_per_point,
            color,
            reasoning_color,
            body_revision: self.body_revision,
            highlights,
            selected: selected.cloned(),
            underline: underline
                .unwrap_or_else(|| previous.map_or_else(Vec::new, |cache| cache.underline)),
            highlight_fill,
            highlight_color,
            reasoning_bytes: self.reasoning.len(),
            body,
            reasoning,
        });
    }

    fn layout_body(
        &self,
        ui: &egui::Ui,
        width: f32,
        matches: &[search::Match],
    ) -> Arc<egui::Galley> {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = width;
        let normal =
            egui::TextFormat::simple(FontId::proportional(16.0), ui.visuals().text_color());
        let mut previous = 0;
        for found in matches {
            #[cfg(test)]
            {
                self.highlight_range_visits
                    .set(self.highlight_range_visits.get() + 1);
            }
            let range = &found.range;
            job.append(&self.body[previous..range.start], 0.0, normal.clone());
            let mut highlight = normal.clone();
            highlight.background = ui.visuals().selection.bg_fill;
            highlight.color = ui.visuals().selection.stroke.color;
            job.append(&self.body[range.clone()], 0.0, highlight);
            previous = range.end;
        }
        job.append(&self.body[previous..], 0.0, normal);
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    }
    fn height(&self) -> f32 {
        let Some(cache) = &self.cache else {
            return 100.0;
        };
        let mut height = 40.0 + 24.0 + 12.0 + cache.body.size().y.max(20.0);
        if self.role == Role::Assistant {
            height += SPEAKER_FOOTER_HEIGHT;
        }
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

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        target: Option<usize>,
    ) -> Option<egui::Rect> {
        let mut found_rect = None;
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
                child.label(RichText::new("Responding…").weak().italics());
            } else if !self.body.is_empty() {
                let response =
                    child.add(egui::Label::new(Arc::clone(&cache.body)).selectable(true));
                for line in &cache.underline {
                    child.painter().line_segment(
                        line.map(|point| point + response.rect.min.to_vec2()),
                        egui::Stroke::new(2.0, cache.highlight_color),
                    );
                }
                if let Some(target) = target {
                    let characters = self.body[..target].chars().count();
                    found_rect = Some(
                        cache
                            .body
                            .pos_from_cursor(egui::text::CCursor::new(characters))
                            .translate(response.rect.min.to_vec2()),
                    );
                }
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
        found_rect
    }
}

// Match byte offsets map to Unicode scalar indices in the galley. Retain only
// the selected glyph runs, splitting wrapped rows and font fallback baselines
// in the same way as epaint's TextFormat underline. Newlines own no glyph.
fn active_underline(
    body: &str,
    galley: &egui::Galley,
    selected: &Range<usize>,
    pixels_per_point: f32,
) -> Vec<[egui::Pos2; 2]> {
    let start = body[..selected.start].chars().count();
    let end = start + body[selected.clone()].chars().count();
    let mut offset = 0;
    let mut lines: Vec<[egui::Pos2; 2]> = Vec::new();
    let stroke = egui::Stroke::new(2.0, Color32::PLACEHOLDER);
    for row in &galley.rows {
        if offset >= end {
            break;
        }
        let first = start.saturating_sub(offset).min(row.glyphs.len());
        let last = end.saturating_sub(offset).min(row.glyphs.len());
        let mut run: Option<[egui::Pos2; 2]> = None;
        for glyph in &row.glyphs[first..last] {
            let mut y = glyph.logical_rect().bottom();
            stroke.round_center_to_pixel(pixels_per_point, &mut y);
            let right = egui::pos2(glyph.max_x(), y) + row.pos.to_vec2();
            if let Some(line) = &mut run
                && line[0].y.to_bits() == right.y.to_bits()
            {
                line[1] = right;
            } else {
                if let Some(line) = run.take() {
                    lines.push(line);
                }
                run = Some([egui::pos2(glyph.pos.x, y) + row.pos.to_vec2(), right]);
            }
        }
        if let Some(line) = run {
            lines.push(line);
        }
        offset += row.char_count_including_newline().0;
    }
    lines
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
