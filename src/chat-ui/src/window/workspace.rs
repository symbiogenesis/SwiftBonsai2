//! Bounded conversation workspace. Visible partial output and committed model
//! history remain separate, so switching never promotes an interrupted turn.
use super::{Command, RichText, State, Transcript, client, egui};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Independent response choices and transcript status are preserved for each conversation."
)]
pub(super) struct ConversationSnapshot {
    pub(super) transcript: Transcript,
    #[serde(deserialize_with = "super::persistence::deserialize_history")]
    pub(super) committed: Vec<client::Message>,
    pub(super) input: String,
    pub(super) last_answer: String,
    pub(super) retained_draft: String,
    pub(super) answer_complete: bool,
    pub(super) extra_effort: bool,
    #[serde(default)]
    pub(super) enable_thinking: bool,
    #[serde(default)]
    pub(super) streaming_read_aloud: bool,
    pub(super) output_budget: usize,
    #[serde(default = "client::default_abliteration_scale")]
    pub(super) abliteration_scale: f32,
    pub(super) status: String,
    pub(super) warning: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Conversation {
    pub(super) id: u64,
    pub(super) title: String,
    pub(super) renamed: bool,
    pub(super) saved: Option<ConversationSnapshot>,
}

impl Conversation {
    pub(super) fn new(id: u64) -> Self {
        Self {
            id,
            title: "New conversation".to_owned(),
            renamed: false,
            saved: None,
        }
    }
}

enum ConversationAction {
    Select(u64),
    Rename(u64, String),
    Delete(u64),
}

impl State {
    pub(super) fn take_conversation(&mut self) -> ConversationSnapshot {
        ConversationSnapshot {
            transcript: std::mem::take(&mut self.transcript),
            committed: std::mem::take(&mut self.committed),
            input: std::mem::take(&mut self.input),
            last_answer: std::mem::take(&mut self.last_answer),
            retained_draft: std::mem::take(&mut self.retained_draft),
            answer_complete: self.answer_complete,
            extra_effort: self.extra_effort,
            enable_thinking: self.enable_thinking,
            streaming_read_aloud: self.streaming_read_aloud,
            output_budget: self.output_budget,
            abliteration_scale: self.abliteration_scale,
            status: self.status.clone(),
            warning: self.warning,
        }
    }

    pub(super) fn save_conversation(&mut self) {
        let snapshot = self.take_conversation();
        if let Some(conversation) = self
            .conversations
            .iter_mut()
            .find(|conversation| conversation.id == self.active_conversation)
        {
            conversation.saved = Some(snapshot);
        }
    }

    pub(super) fn clear_conversation(&mut self) {
        self.transcript.clear();
        self.committed.clear();
        self.input.clear();
        self.last_answer.clear();
        self.retained_draft.clear();
        self.stage = None;
        self.answer_complete = false;
        self.extra_effort = false;
        self.enable_thinking = false;
        self.streaming_read_aloud = false;
        self.output_budget = 3;
        self.abliteration_scale = client::DEFAULT_ABLITERATION_SCALE;
        self.place_composer_cursor(0);
        self.focus_composer = true;
    }

    pub(super) fn load_conversation(&mut self, snapshot: ConversationSnapshot) {
        self.transcript = snapshot.transcript;
        self.committed = snapshot.committed;
        self.input = snapshot.input;
        self.last_answer = snapshot.last_answer;
        self.retained_draft = snapshot.retained_draft;
        self.answer_complete = snapshot.answer_complete;
        self.extra_effort = snapshot.extra_effort;
        self.enable_thinking = snapshot.enable_thinking;
        self.streaming_read_aloud = snapshot.streaming_read_aloud;
        self.output_budget = snapshot.output_budget;
        self.abliteration_scale = snapshot.abliteration_scale;
        self.status = snapshot.status;
        self.warning = snapshot.warning;
        self.stage = None;
        self.place_composer_cursor(self.input.chars().count());
        self.focus_composer = true;
    }

    pub(super) fn switch_conversation(&mut self, id: u64) {
        if !self.ready || self.busy || id == self.active_conversation {
            return;
        }
        let Some(index) = self
            .conversations
            .iter()
            .position(|conversation| conversation.id == id)
        else {
            return;
        };
        let Some(saved) = self.conversations[index].saved.as_ref() else {
            return;
        };
        if !self.preview
            && self
                .commands
                .send(Command::RestoreChat(saved.committed.clone()))
                .is_err()
        {
            self.fail("The model worker stopped. Close and restart the app.");
            return;
        }
        self.stop_speech();
        self.save_conversation();
        self.conversation_notice = None;
        if let Some(snapshot) = self.conversations[index].saved.take() {
            self.active_conversation = id;
            self.load_conversation(snapshot);
            self.recovery_dirty = true;
        }
    }

    pub(super) fn remove_conversation(&mut self, id: u64) {
        if !self.ready || self.busy {
            return;
        }
        let Some(index) = self
            .conversations
            .iter()
            .position(|conversation| conversation.id == id)
        else {
            return;
        };
        if id == self.active_conversation {
            if let Some(next) = self
                .conversations
                .iter()
                .find(|conversation| conversation.id != id)
                .map(|conversation| conversation.id)
            {
                self.switch_conversation(next);
                if self.active_conversation == id {
                    return;
                }
            } else {
                if !self.preview && self.commands.send(Command::NewChat).is_err() {
                    self.fail("The model worker stopped. Close and restart the app.");
                    return;
                }
                self.stop_speech();
                self.clear_conversation();
                self.active_conversation = self.next_conversation;
                self.next_conversation = self.next_conversation.saturating_add(1);
                self.conversations
                    .push(Conversation::new(self.active_conversation));
                self.set_status("Ready for a new conversation.", false);
            }
        }
        self.conversations.remove(index);
        self.recovery_dirty = true;
        self.conversation_notice = Some("Conversation deleted.");
    }

    pub(super) fn conversation_list(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("CONVERSATIONS").small().strong().weak());
            ui.label(
                RichText::new(self.conversations.len().to_string())
                    .small()
                    .weak(),
            );
        });
        ui.add_space(8.0);
        let mut action = None;
        egui::ScrollArea::vertical()
            .id_salt("conversation-list")
            .auto_shrink([false, false])
            .min_scrolled_height(0.0)
            .show(ui, |ui| {
                let row_width = ui.available_width();
                for conversation in &self.conversations {
                    // Stable identities survive row removal and duplicate titles.
                    ui.push_id(conversation.id, |ui| {
                        if let Some(selected) = self.conversation_row(ui, conversation, row_width) {
                            action = Some(selected);
                        }
                    });
                    ui.add_space(4.0);
                }
            });
        match action {
            Some(ConversationAction::Select(id)) => self.switch_conversation(id),
            Some(ConversationAction::Rename(id, title)) => {
                self.rename_conversation = Some((id, title));
                ui.ctx().memory_mut(|memory| {
                    memory.request_focus(egui::Id::unique("conversation-name"));
                });
            }
            Some(ConversationAction::Delete(id)) => self.delete_conversation = Some(id),
            None => {}
        }
    }

    fn conversation_row(
        &self,
        ui: &mut egui::Ui,
        conversation: &Conversation,
        row_width: f32,
    ) -> Option<ConversationAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let width = (row_width - 38.0).max(40.0);
            let active = conversation.id == self.active_conversation;
            let title = RichText::new(&conversation.title).color(if active {
                ui.visuals().selection.stroke.color
            } else {
                ui.visuals().text_color()
            });
            let response = ui
                .add_enabled_ui(self.ready && !self.busy, |ui| {
                    ui.add_sized(
                        [width, 40.0],
                        egui::Button::selectable(active, title)
                            .right_text(())
                            .truncate()
                            .corner_radius(8),
                    )
                })
                .inner;
            // Only scroll when the composer can consume this focus request in
            // the same frame; pending focus during generation must not repaint forever.
            if active && self.focus_composer && self.ready && !self.busy {
                response.scroll_to_me(Some(egui::Align::Center));
            }
            if response.on_hover_text(&conversation.title).clicked() {
                action = Some(ConversationAction::Select(conversation.id));
            }
            let (response, _) = egui::containers::menu::MenuButton::from_button(
                egui::Button::new("⋯")
                    .min_size(egui::vec2(34.0, 40.0))
                    .frame_when_inactive(false),
            )
            .ui(ui, |ui| {
                action = self.conversation_menu(ui, conversation);
            });
            let label = format!("Actions for {}", conversation.title);
            response.accessible_name(&label).on_hover_text(label);
        });
        action
    }

    fn conversation_menu(
        &self,
        ui: &mut egui::Ui,
        conversation: &Conversation,
    ) -> Option<ConversationAction> {
        ui.set_min_width(156.0);
        if ui
            .add_enabled(!self.busy, egui::Button::new("Rename"))
            .clicked()
        {
            ui.close();
            return Some(ConversationAction::Rename(
                conversation.id,
                conversation.title.clone(),
            ));
        }
        let transcript = if conversation.id == self.active_conversation {
            Some(&self.transcript)
        } else {
            conversation.saved.as_ref().map(|saved| &saved.transcript)
        };
        if ui
            .add_enabled(
                transcript.is_some_and(|value| !value.messages.is_empty()),
                egui::Button::new("Copy conversation"),
            )
            .clicked()
        {
            if let Some(transcript) = transcript {
                ui.ctx().copy_text(transcript.plain_text());
            }
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(
                self.ready && !self.busy,
                egui::Button::new(RichText::new("Delete").color(ui.visuals().error_fg_color)),
            )
            .clicked()
        {
            ui.close();
            return Some(ConversationAction::Delete(conversation.id));
        }
        None
    }

    pub(super) fn conversation_dialogs(&mut self, ctx: &egui::Context) {
        self.rename_dialog(ctx);
        self.delete_dialog(ctx);
    }

    fn rename_dialog(&mut self, ctx: &egui::Context) {
        if let Some((id, mut title)) = self.rename_conversation.take() {
            let mut done = false;
            let modal = egui::Modal::new(egui::Id::unique("rename-conversation")).show(ctx, |ui| {
                ui.set_width(340.0);
                ui.heading("Rename conversation");
                ui.add_space(12.0);
                let label = ui.label("Conversation name");
                ui.add(
                    egui::TextEdit::singleline(&mut title)
                        .id(egui::Id::unique("conversation-name"))
                        .char_limit(64)
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(label.id);
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!title.trim().is_empty(), egui::Button::new("Save name"))
                        .clicked()
                    {
                        if let Some(conversation) = self
                            .conversations
                            .iter_mut()
                            .find(|conversation| conversation.id == id)
                        {
                            title.trim().clone_into(&mut conversation.title);
                            conversation.renamed = true;
                            self.recovery_dirty = true;
                        }
                        done = true;
                    }
                    if ui.button("Cancel").clicked() {
                        done = true;
                    }
                });
            });
            if !modal.should_close() && !done {
                self.rename_conversation = Some((id, title));
            }
        }
    }

    fn delete_dialog(&mut self, ctx: &egui::Context) {
        if let Some(id) = self.delete_conversation {
            let Some(conversation) = self.conversations.iter().find(|value| value.id == id) else {
                self.delete_conversation = None;
                return;
            };
            let title = conversation.title.clone();
            let mut confirmed = false;
            let mut cancelled = false;
            let modal = egui::Modal::new(egui::Id::unique("delete-conversation"))
                .show(ctx, |ui| {
                    ui.set_width(340.0);
                    ui.heading("Delete conversation?");
                    ui.add_space(12.0);
                    ui.label(RichText::new(&title).strong());
                    ui.label("This conversation and its unsent draft will be removed. This cannot be undone.");
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        cancelled = ui.button("Keep conversation").clicked();
                        confirmed = ui
                            .add_enabled(
                                self.ready && !self.busy,
                                egui::Button::new(RichText::new("Delete conversation").color(ui.visuals().error_fg_color)),
                            )
                            .clicked();
                    });
                });
            if confirmed {
                self.remove_conversation(id);
            }
            if modal.should_close() || confirmed || cancelled {
                self.delete_conversation = None;
            }
        }
    }
}
