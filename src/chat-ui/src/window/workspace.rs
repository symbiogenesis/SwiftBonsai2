//! Bounded conversation workspace. Visible partial output and committed model
//! history remain separate, so switching never promotes an interrupted turn.
use super::{Command, RichText, State, Transcript, client, egui};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConversationSnapshot {
    pub(super) transcript: Transcript,
    pub(super) committed: Vec<client::Message>,
    pub(super) input: String,
    pub(super) last_answer: String,
    pub(super) retained_draft: String,
    pub(super) answer_complete: bool,
    pub(super) extra_effort: bool,
    pub(super) output_budget: usize,
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
            output_budget: self.output_budget,
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
        self.output_budget = 3;
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
        self.output_budget = snapshot.output_budget;
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
    }

    pub(super) fn conversation_list(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("CONVERSATIONS").small().weak());
        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt("conversation-list")
            .max_height(260.0)
            .show(ui, |ui| {
                for conversation in &self.conversations {
                    if ui
                        .add_enabled(
                            self.ready && !self.busy,
                            egui::Button::selectable(
                                conversation.id == self.active_conversation,
                                &conversation.title,
                            )
                            .wrap(),
                        )
                        .clicked()
                    {
                        selected = Some(conversation.id);
                    }
                }
            });
        if let Some(id) = selected {
            self.switch_conversation(id);
        }
    }

    pub(super) fn conversation_dialogs(&mut self, ctx: &egui::Context) {
        if let Some((id, mut title)) = self.rename_conversation.take() {
            let mut open = true;
            let mut done = false;
            egui::Window::new("Rename conversation")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    let label = ui.label("Conversation name");
                    ui.add(egui::TextEdit::singleline(&mut title).char_limit(64))
                        .labelled_by(label.id);
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
            if open && !done {
                self.rename_conversation = Some((id, title));
            }
        }
        if let Some(id) = self.delete_conversation {
            let mut open = true;
            let mut confirmed = false;
            let mut cancelled = false;
            egui::Window::new("Delete conversation?")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(
                        "This removes the conversation and its unsent draft from this workspace.",
                    );
                    ui.horizontal(|ui| {
                        confirmed = ui
                            .add_enabled(
                                self.ready && !self.busy,
                                egui::Button::new("Delete conversation"),
                            )
                            .clicked();
                        cancelled = ui.button("Keep conversation").clicked();
                    });
                });
            if confirmed {
                self.remove_conversation(id);
            }
            if !open || confirmed || cancelled {
                self.delete_conversation = None;
            }
        }
    }
}
