//! Versioned, opt-in workspace recovery. Never restores an in-flight request.
use super::{
    Conversation, Duration, INPUT_LIMIT, Instant, MAX_CONVERSATIONS, MAX_DIAGNOSTIC_CHARS,
    OUTPUT_TOKENS, RichText, State, Transcript, VOICE_LIMIT, client, egui, io,
    workspace::ConversationSnapshot,
};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedWorkspace {
    version: u32,
    conversations: Vec<Conversation>,
    active_conversation: u64,
    next_conversation: u64,
    active: ConversationSnapshot,
    voice_profile: String,
}

#[derive(serde::Serialize)]
struct WorkspaceView<'a> {
    version: u32,
    conversations: &'a [Conversation],
    active_conversation: u64,
    next_conversation: u64,
    active: ActiveView<'a>,
    voice_profile: &'a str,
}

#[derive(serde::Serialize)]
struct ActiveView<'a> {
    transcript: &'a Transcript,
    committed: &'a [client::Message],
    input: &'a str,
    last_answer: &'a str,
    retained_draft: &'a str,
    answer_complete: bool,
    extra_effort: bool,
    output_budget: usize,
    status: &'a str,
    warning: bool,
}

impl State {
    pub(super) fn attach_recovery(&mut self, opened: io::Result<crate::recovery::Opened>) {
        match opened {
            Ok(opened) => {
                self.recovery = Some(opened.store);
                if let Some(bytes) = opened.saved {
                    if self.restore_workspace(&bytes).is_ok() {
                        self.save_conversations = true;
                        "Conversations restored. Completed turns are ready to continue."
                            .clone_into(&mut self.recovery_notice);
                    } else {
                        "Could not restore saved conversations. The existing file is preserved. Enable saving to replace it with this workspace.".clone_into(&mut self.recovery_notice);
                    }
                } else if opened.load_failed {
                    "Could not unlock saved conversations. The existing file is preserved. Enable saving to replace it with this workspace.".clone_into(&mut self.recovery_notice);
                }
            }
            Err(_) => {
                "Could not open conversation saving. Another window may be using this data folder, or the folder is not writable.".clone_into(&mut self.recovery_notice);
            }
        }
    }

    pub(super) fn workspace_bytes(&self) -> io::Result<Vec<u8>> {
        let bytes = serde_json::to_vec(&WorkspaceView {
            version: 1,
            conversations: &self.conversations,
            active_conversation: self.active_conversation,
            next_conversation: self.next_conversation,
            active: ActiveView {
                transcript: &self.transcript,
                committed: &self.committed,
                input: &self.input,
                last_answer: &self.last_answer,
                retained_draft: &self.retained_draft,
                answer_complete: self.answer_complete,
                extra_effort: self.extra_effort,
                output_budget: self.output_budget,
                status: &self.status,
                warning: self.warning,
            },
            voice_profile: &self.voice_profile,
        })
        .map_err(io::Error::other)?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(io::Error::other(
                "Conversation recovery reached its 8 MiB limit. Delete older conversations or disable saving.",
            ));
        }
        Ok(bytes)
    }

    fn restore_workspace(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(io::Error::other("Saved workspace is too large."));
        }
        let mut saved: SavedWorkspace = serde_json::from_slice(bytes).map_err(io::Error::other)?;
        if !saved.valid() {
            return Err(io::Error::other("Invalid saved workspace."));
        }
        // Readiness belongs to the fresh owned broker, never to disk. Retained
        // partial text remains visible but only committed turns reach history.
        "Recovered conversation. Preparing your local model…".clone_into(&mut saved.active.status);
        saved.active.warning = false;
        self.conversations = saved.conversations;
        self.active_conversation = saved.active_conversation;
        self.next_conversation = saved.next_conversation;
        self.voice_profile = saved.voice_profile;
        self.load_conversation(saved.active);
        self.restore_history = true;
        Ok(())
    }

    pub(super) fn recovery_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Conversations");
        if ui
            .add_enabled(
                self.recovery.is_some(),
                egui::Checkbox::new(
                    &mut self.save_conversations,
                    "Save conversations on this device",
                ),
            )
            .changed()
        {
            self.recovery_dirty = self.save_conversations;
            self.recovery_erase_pending = !self.save_conversations;
            self.last_recovery_save = Instant::now()
                .checked_sub(Duration::from_secs(3))
                .unwrap_or_else(Instant::now);
            if self.save_conversations {
                "Saving drafts and conversations with Windows user encryption…"
            } else {
                "Removing the saved workspace. Open conversations stay available in this window."
            }
            .clone_into(&mut self.recovery_notice);
        }
        ui.label(RichText::new("Off by default. Uses Windows user encryption in your model data folder. Restores drafts and completed history on the next launch. Turning this off removes the saved copy.").small().weak());
        if !self.recovery_notice.is_empty() {
            ui.label(RichText::new(&self.recovery_notice).small());
        }
        ui.add_space(16.0);
    }

    pub(super) fn poll_recovery(&mut self) {
        if let Some(recovery) = &self.recovery {
            while let Ok(event) = recovery.events.try_recv() {
                if !recovery.is_current(&event) {
                    continue;
                }
                match event {
                    crate::recovery::Event::Saved(_) if self.save_conversations => {
                        "Saved on this device with Windows user encryption."
                            .clone_into(&mut self.recovery_notice);
                    }
                    crate::recovery::Event::Erased(_) if !self.save_conversations => {
                        self.recovery_erase_pending = false;
                        "Saving is off. The saved workspace has been removed."
                            .clone_into(&mut self.recovery_notice);
                    }
                    crate::recovery::Event::Failed(_) => {
                        "Could not update saved conversations. Check available disk space and folder access; unsaved work remains in this window.".clone_into(&mut self.recovery_notice);
                        self.recovery_dirty = self.save_conversations;
                    }
                    crate::recovery::Event::Saved(_) | crate::recovery::Event::Erased(_) => {}
                }
            }
        }
        if self.last_recovery_save.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_recovery_save = Instant::now();
        if self.recovery_erase_pending {
            if let Some(recovery) = &self.recovery {
                let _queued = recovery.erase();
            }
        } else if self.save_conversations && self.recovery_dirty {
            match self.workspace_bytes() {
                Ok(bytes) => {
                    if let Some(recovery) = &self.recovery {
                        self.recovery_dirty = !recovery.save(bytes);
                    }
                }
                Err(error) => {
                    self.recovery_notice = format!("Could not save conversations: {error}");
                }
            }
        }
    }
}

impl SavedWorkspace {
    fn valid(&self) -> bool {
        let mut ids = std::collections::HashSet::new();
        self.version == 1
            && !self.conversations.is_empty()
            && self.conversations.len() <= MAX_CONVERSATIONS
            && self.voice_profile.chars().count() <= VOICE_LIMIT
            && self.next_conversation < u64::MAX
            && self.conversations.iter().all(|conversation| {
                conversation.id < self.next_conversation
                    && ids.insert(conversation.id)
                    && conversation.title.chars().count() <= 64
                    && if conversation.id == self.active_conversation {
                        conversation.saved.is_none()
                    } else {
                        conversation.saved.as_ref().is_some_and(valid_snapshot)
                    }
            })
            && ids.contains(&self.active_conversation)
            && valid_snapshot(&self.active)
    }
}

fn valid_snapshot(saved: &ConversationSnapshot) -> bool {
    saved.transcript.valid_saved()
        && saved.input.chars().count() <= INPUT_LIMIT
        && saved.output_budget < OUTPUT_TOKENS.len()
        && saved.status.len() <= MAX_DIAGNOSTIC_CHARS * 4
        && saved.last_answer.len() <= 4 * 1024 * 1024
        && saved.retained_draft.len() <= 4 * 1024 * 1024
        && saved.committed.len() <= 128
        && saved.committed.len().is_multiple_of(2)
        && saved.committed.iter().enumerate().all(|(index, message)| {
            message.role
                == if index.is_multiple_of(2) {
                    "user"
                } else {
                    "assistant"
                }
                && message.content.len() <= 1024 * 1024
                && message
                    .reasoning_content
                    .as_ref()
                    .is_none_or(|text| text.len() <= 1024 * 1024)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::{Role, mpsc};

    fn state() -> State {
        let (commands, _incoming) = mpsc::channel();
        let (_events, incoming) = mpsc::channel();
        State::create(commands, incoming, None, None)
    }

    #[test]
    fn recovery_preserves_drafts_without_promoting_partial_turns() -> io::Result<()> {
        let mut original = state();
        original.input = "Unsent draft 🌱".to_owned();
        original
            .transcript
            .push(Role::User, "Interrupted request".to_owned());
        original.transcript.push(Role::Assistant, String::new());
        original.transcript.append("Partial answer", false);
        let bytes = original.workspace_bytes()?;
        let mut restored = state();
        restored.restore_workspace(&bytes)?;
        assert_eq!(restored.input, original.input);
        assert_eq!(
            restored.transcript.plain_text(),
            original.transcript.plain_text()
        );
        assert!(restored.committed.is_empty());
        assert!(restored.restore_history);
        assert!(!restored.busy);
        assert!(!restored.speech_input);
        assert!(!restored.speaking);
        let mut invalid: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        invalid["active"]["output_budget"] = serde_json::json!(99);
        assert!(
            restored
                .restore_workspace(&serde_json::to_vec(&invalid).map_err(io::Error::other)?)
                .is_err()
        );
        invalid["active"]["output_budget"] = serde_json::json!(3);
        invalid["version"] = serde_json::json!(2);
        assert!(
            restored
                .restore_workspace(&serde_json::to_vec(&invalid).map_err(io::Error::other)?)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn recovery_rapid_toggles_ignore_stale_erase_and_save_acknowledgments() -> io::Result<()> {
        let mut state = state();
        let (store, controlled) = crate::recovery::Recovery::controlled();
        state.recovery = Some(store);
        state.recovery_erase_pending = true;
        state.last_recovery_save = Instant::now()
            .checked_sub(Duration::from_secs(3))
            .unwrap_or_else(Instant::now);
        state.poll_recovery();
        let first_erase = controlled.next_completion()?;
        assert!(matches!(first_erase, crate::recovery::Event::Erased(_)));

        // Enable saving while the first deletion's acknowledgment is delayed.
        state.save_conversations = true;
        state.recovery_erase_pending = false;
        state.recovery_dirty = true;
        state.last_recovery_save = Instant::now()
            .checked_sub(Duration::from_secs(3))
            .unwrap_or_else(Instant::now);
        state.poll_recovery();
        let later_save = controlled.next_completion()?;
        assert!(matches!(later_save, crate::recovery::Event::Saved(_)));

        // Disable again before that queued save completes. The old erase must
        // not clear the new deletion intention, even during window shutdown.
        state.save_conversations = false;
        state.recovery_erase_pending = true;
        controlled.complete(first_erase)?;
        state.poll_recovery();
        assert!(state.recovery_erase_pending);
        state.last_recovery_save = Instant::now()
            .checked_sub(Duration::from_secs(3))
            .unwrap_or_else(Instant::now);
        state.poll_recovery();
        let final_erase = controlled.next_completion()?;
        assert!(matches!(final_erase, crate::recovery::Event::Erased(_)));
        controlled.complete(later_save)?;
        state.poll_recovery();
        assert!(state.recovery_erase_pending);
        controlled.complete(final_erase)?;
        state.poll_recovery();
        assert!(!state.recovery_erase_pending);
        assert!(!state.save_conversations);
        assert!(state.recovery_notice.contains("removed"));
        drop(controlled);
        Ok(())
    }
}
