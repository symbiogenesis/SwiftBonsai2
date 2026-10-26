//! Native egui chat presentation. Backend work and audio remain off the UI loop.
#![allow(
    clippy::redundant_pub_crate,
    reason = "Keep presentation internals parent-only."
)]

use crate::{
    client::{self, Command, Completion, Config, Event},
    speech,
};
use eframe::egui::{self, Color32, RichText};
use std::{
    collections::VecDeque,
    io,
    sync::mpsc::{self, Receiver, Sender},
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[cfg(test)]
mod interaction_tests;
#[cfg(test)]
mod keyboard_tests;
#[allow(
    unsafe_code,
    reason = "Audited UI-thread WinRT microphone initialization and teardown."
)]
mod microphone;
mod persistence;
#[allow(
    unsafe_code,
    reason = "Audited Windows high-contrast and fatal-dialog boundary."
)]
mod platform;
mod read_aloud;
#[cfg(test)]
mod tests;
mod transcript;
mod workspace;

use transcript::{Role, Transcript};
use workspace::Conversation;

const OUTPUT_TOKENS: [u32; 4] = [512, 1024, 2048, 4096];
const DEFAULT_VOICE: &str = "A warm, clear, natural adult voice with a calm conversational pace.";
const INPUT_LIMIT: usize = 32_768;
const VOICE_LIMIT: usize = 1_024;
const MAX_DIAGNOSTICS: usize = 128;
const MAX_DIAGNOSTIC_CHARS: usize = 4_096;
const MAX_CONVERSATIONS: usize = 16;

#[allow(
    clippy::struct_excessive_bools,
    reason = "Independent broker, audio and presentation state is explicit."
)]
struct State {
    commands: Sender<Command>,
    events: Receiver<Event>,
    worker: Option<JoinHandle<()>>,
    ready: bool,
    busy: bool,
    stopping: bool,
    retryable: bool,
    waiting_model: bool,
    preview: bool,
    input: String,
    transcript: Transcript,
    committed: Vec<client::Message>,
    conversations: Vec<Conversation>,
    active_conversation: u64,
    next_conversation: u64,
    rename_conversation: Option<(u64, String)>,
    delete_conversation: Option<u64>,
    recovery: Option<crate::recovery::Recovery>,
    save_conversations: bool,
    recovery_dirty: bool,
    recovery_erase_pending: bool,
    recovery_notice: String,
    last_recovery_save: Instant,
    restore_history: bool,
    status: String,
    warning: bool,
    diagnostics: VecDeque<String>,
    show_diagnostics: bool,
    show_settings: bool,
    sidebar_open: bool,
    help: Option<(&'static str, &'static str)>,
    extra_effort: bool,
    enable_thinking: bool,
    output_budget: usize,
    abliteration_scale: f32,
    abliteration_enabled: bool,
    focus_composer: bool,
    composing: bool,
    composer_id: Option<egui::Id>,
    composer_cursor: Option<egui::text::CCursorRange>,
    pending_cursor: Option<egui::text::CCursorRange>,
    speech: Option<speech::Session>,
    speech_checked: bool,
    speech_input: bool,
    speech_output: bool,
    canary_configured: bool,
    tts_configured: bool,
    speech_status: String,
    speech_notice: bool,
    voice_profile: String,
    listening: bool,
    dictation_requested: bool,
    microphone: Option<microphone::Microphone>,
    microphone_generation: Option<u64>,
    speaking: bool,
    speaking_message: Option<u64>,
    streaming_read_aloud: bool,
    streaming_active: bool,
    streaming_suppressed: bool,
    answer_complete: bool,
    last_answer: String,
    retained_draft: String,
    stage: Option<&'static str>,
    last_theme_check: Instant,
}

impl State {
    fn create(
        commands: Sender<Command>,
        events: Receiver<Event>,
        config: Option<&Config>,
        microphone: Option<microphone::Microphone>,
    ) -> Self {
        let speech = config
            .and_then(|config| speech::start(config.canary.clone(), config.qwen_tts.clone()).ok());
        let speech_checked = speech.is_none();
        Self {
            commands,
            events,
            worker: None,
            ready: false,
            busy: false,
            stopping: false,
            retryable: false,
            waiting_model: config.is_some(),
            preview: config.is_none(),
            input: String::new(),
            transcript: Transcript::default(),
            committed: Vec::new(),
            conversations: vec![Conversation::new(1)],
            active_conversation: 1,
            next_conversation: 2,
            rename_conversation: None,
            delete_conversation: None,
            recovery: None,
            save_conversations: false,
            recovery_dirty: false,
            recovery_erase_pending: false,
            recovery_notice: String::new(),
            last_recovery_save: Instant::now(),
            restore_history: false,
            status: "Preparing your local model…".to_owned(),
            warning: false,
            diagnostics: VecDeque::new(),
            show_diagnostics: false,
            show_settings: false,
            sidebar_open: false,
            help: None,
            extra_effort: false,
            enable_thinking: false,
            output_budget: 3,
            abliteration_scale: client::DEFAULT_ABLITERATION_SCALE,
            abliteration_enabled: config.is_none_or(|config| config.use_abliterate_lora),
            focus_composer: false,
            composing: false,
            composer_id: None,
            composer_cursor: None,
            pending_cursor: None,
            speech,
            speech_checked,
            speech_input: false,
            speech_output: false,
            canary_configured: config.is_some_and(|config| config.canary.is_some()),
            tts_configured: config.is_some_and(|config| config.qwen_tts.is_some()),
            speech_status: "Select Microphone or a response's speaker button for setup.".to_owned(),
            speech_notice: false,
            voice_profile: DEFAULT_VOICE.to_owned(),
            listening: false,
            dictation_requested: false,
            microphone,
            microphone_generation: None,
            speaking: false,
            speaking_message: None,
            streaming_read_aloud: false,
            streaming_active: false,
            streaming_suppressed: false,
            answer_complete: false,
            last_answer: String::new(),
            retained_draft: String::new(),
            stage: None,
            last_theme_check: Instant::now()
                .checked_sub(Duration::from_secs(2))
                .unwrap_or_else(Instant::now),
        }
    }

    fn set_status(&mut self, message: &str, warning: bool) {
        self.record(message);
        message
            .split("Protected credentials and technical evidence:")
            .next()
            .unwrap_or(message)
            .trim()
            .clone_into(&mut self.status);
        self.warning = warning;
    }

    fn record(&mut self, message: &str) {
        if self
            .diagnostics
            .back()
            .is_none_or(|previous| previous != message)
        {
            self.diagnostics
                .push_back(message.chars().take(MAX_DIAGNOSTIC_CHARS).collect());
            while self.diagnostics.len() > MAX_DIAGNOSTICS {
                self.diagnostics.pop_front();
            }
        }
    }

    fn submit(&mut self) {
        if self.preview {
            self.set_status(
                "Preview only. Launch the app normally to chat with your local model.",
                false,
            );
            return;
        }
        if !self.ready || self.busy || self.input.trim().is_empty() {
            return;
        }
        let text = self.input.trim().to_owned();
        self.stop_speech();
        self.last_answer.clear();
        self.retained_draft.clear();
        self.stage = None;
        self.answer_complete = false;
        let max_completion_tokens = OUTPUT_TOKENS
            .get(self.output_budget)
            .copied()
            .unwrap_or(4096);
        if self
            .commands
            .send(Command::Send {
                text: text.clone(),
                max_completion_tokens,
                extra_effort: self.extra_effort,
                enable_thinking: self.enable_thinking,
                abliteration_scale: self.abliteration_scale,
            })
            .is_err()
        {
            self.fail("The model worker stopped. Close and restart the app.");
            return;
        }
        if let Some(conversation) = self
            .conversations
            .iter_mut()
            .find(|conversation| conversation.id == self.active_conversation)
            && !conversation.renamed
            && self.transcript.messages.is_empty()
        {
            conversation.title = text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(48)
                .collect();
        }
        self.transcript.push(Role::User, text);
        self.transcript.push(Role::Assistant, String::new());
        self.recovery_dirty = true;
        self.input.clear();
        self.place_composer_cursor(0);
        self.busy = true;
        self.streaming_suppressed = false;
        self.set_status("Responding…", false);
    }

    fn new_chat(&mut self) {
        if !self.ready || self.busy {
            return;
        }
        if self.conversations.len() >= MAX_CONVERSATIONS {
            self.set_status(
                "You have 16 conversations open. Delete one before starting another.",
                true,
            );
            return;
        }
        if !self.preview && self.commands.send(Command::NewChat).is_err() {
            self.fail("The model worker stopped. Close and restart the app.");
            return;
        }
        self.stop_speech();
        self.save_conversation();
        let id = self.next_conversation;
        self.next_conversation = self.next_conversation.saturating_add(1);
        self.conversations.push(Conversation::new(id));
        self.active_conversation = id;
        self.clear_conversation();
        self.recovery_dirty = true;
        self.set_status("Ready for a new conversation.", false);
    }

    fn retry(&mut self) {
        if !self.retryable {
            return;
        }
        if self.commands.send(Command::Retry).is_err() {
            self.fail("The model worker stopped. Close and restart the app.");
            return;
        }
        self.stop_speech();
        self.retryable = false;
        self.ready = false;
        self.busy = false;
        self.waiting_model = true;
        self.set_status("Restarting the local model…", false);
    }

    fn stop_generation(&mut self) {
        if !self.busy {
            return;
        }
        if self.commands.send(Command::Stop).is_err() {
            self.fail("The model worker stopped. Close and restart the app.");
            return;
        }
        self.stop_speech();
        self.ready = false;
        self.busy = false;
        self.stopping = true;
        self.retryable = false;
        self.waiting_model = true;
        self.transcript.remove_empty_response();
        self.set_status("Stopping the response…", false);
    }

    fn handle_escape(&mut self, ctx: &egui::Context) {
        if self.transcript.close_find_on_escape(ctx) {
            self.focus_composer = true;
        } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            if self.busy {
                self.stop_generation();
            } else if self.speaking || self.dictation_requested {
                self.stop_speech();
            }
        }
    }

    fn retain_draft(&mut self) {
        if !self.retained_draft.is_empty() {
            self.last_answer.clone_from(&self.retained_draft);
            self.answer_complete = true;
        }
    }

    fn fail(&mut self, message: &str) {
        self.stop_speech();
        self.ready = false;
        self.busy = false;
        self.stopping = false;
        self.retryable = false;
        self.waiting_model = false;
        self.transcript.remove_empty_response();
        self.retain_draft();
        self.set_status(message, true);
    }

    fn reject(&mut self, message: &str, text: &str, retry_required: bool) {
        self.stop_speech();
        self.busy = false;
        self.ready = !retry_required;
        self.retryable = false;
        self.waiting_model = retry_required;
        self.transcript.remove_empty_response();
        self.input = text.chars().take(INPUT_LIMIT).collect();
        self.place_composer_cursor(self.input.chars().count());
        self.retain_draft();
        self.set_status(message, true);
        self.focus_composer = !retry_required;
    }

    fn drain_events(&mut self) {
        for _ in 0..256 {
            match self.events.try_recv() {
                Ok(event) => self.model_event(event),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if !self.preview && (self.ready || self.busy || self.waiting_model) {
                        self.fail("The model worker stopped. Close and restart the app.");
                    }
                    break;
                }
            }
        }
    }

    fn model_event(&mut self, event: Event) {
        if self.stopping
            && matches!(
                event,
                Event::Text(_)
                    | Event::Reasoning(_)
                    | Event::Stage(_)
                    | Event::Draft(_)
                    | Event::Complete(_)
            )
        {
            // Events already queued at the click cannot restart the response
            // or replace the stop/cleanup status.
            return;
        }
        self.recovery_dirty = true;
        match event {
            Event::Starting => {
                self.stopping = false;
                self.waiting_model = true;
                self.ready = false;
                self.busy = false;
                self.retryable = false;
                self.set_status("Preparing your local model…", false);
            }
            Event::Retryable(message) => {
                self.fail(&message);
                self.retryable = true;
            }
            Event::Stopping(text) => self.stopping_response(&text),
            Event::Stopped => self.stopped_response(),
            Event::Ready => {
                if self.restore_history {
                    self.restore_history = false;
                    if self
                        .commands
                        .send(Command::RestoreChat(self.committed.clone()))
                        .is_err()
                    {
                        self.fail("The model worker stopped. Close and restart the app.");
                        return;
                    }
                }
                self.waiting_model = false;
                self.ready = true;
                self.retryable = false;
                self.set_status("Ready when you are.", false);
            }
            Event::Rejected {
                message,
                text,
                retry_required,
            } => self.reject(&message, &text, retry_required),
            Event::Stage(label) => {
                self.replace_streaming_stage();
                self.stage = Some(label);
                self.last_answer.clear();
                self.answer_complete = false;
                self.transcript.stage();
                self.set_status(label, false);
            }
            Event::Draft(text) => self.retained_draft = text,
            Event::Text(text) => {
                self.last_answer.push_str(&text);
                self.transcript.append(&text, false);
                self.stream_answer(&text);
                self.stage
                    .unwrap_or("Writing…")
                    .clone_into(&mut self.status);
            }
            Event::Reasoning(text) => self.transcript.append(&text, true),
            Event::Committed(history) => self.commit_history(history),
            Event::Complete(completion) => {
                if self.streaming_active {
                    self.speech_command(speech::Command::FinishStream);
                }
                self.transcript.remove_empty_response();
                self.busy = false;
                self.answer_complete = true;
                let message = match completion {
                    Completion::Finished => "Ready when you are.",
                    Completion::TokenLimit
                        if !self.retained_draft.is_empty()
                            && self.last_answer == self.retained_draft =>
                    {
                        "Review did not finish. Your complete draft is retained."
                    }
                    Completion::TokenLimit => {
                        "Response reached the selected output cap, including thinking. You can ask Swift to continue."
                    }
                };
                self.set_status(message, completion == Completion::TokenLimit);
            }
            Event::Failed(message) => self.fail(&message),
        }
    }

    fn commit_history(&mut self, history: Vec<client::Message>) {
        if self.stopping
            && let Some(answer) = history.last().filter(|answer| answer.role == "assistant")
        {
            // A fully completed turn can win just before the stop click. Keep
            // its visible answer consistent with the authoritative history,
            // without reopening Send or overwriting the cleanup status.
            self.transcript
                .replace_response(&answer.content, answer.reasoning_content.as_deref());
            self.last_answer.clone_from(&answer.content);
            self.answer_complete = true;
        }
        self.committed = history;
    }

    fn stopping_response(&mut self, text: &str) {
        self.stop_speech();
        self.busy = false;
        self.ready = false;
        self.stopping = true;
        self.waiting_model = true;
        self.retryable = false;
        if self.input.is_empty() {
            self.input = text.chars().take(INPUT_LIMIT).collect();
            self.place_composer_cursor(self.input.chars().count());
        }
        self.transcript.remove_empty_response();
        self.retain_draft();
        self.set_status("Response stopped. Releasing the local model…", false);
    }

    fn stopped_response(&mut self) {
        self.stopping = false;
        self.busy = false;
        self.ready = false;
        self.waiting_model = false;
        self.retryable = true;
        self.set_status(client::STOPPED_MESSAGE, false);
        self.focus_composer = true;
    }

    fn speech_command(&mut self, command: speech::Command) -> bool {
        if self
            .speech
            .as_ref()
            .is_some_and(|session| session.commands.send(command).is_ok())
        {
            return true;
        }
        self.speech_input = false;
        self.speech_output = false;
        self.cancel_microphone();
        self.speaking = false;
        self.speaking_message = None;
        self.streaming_active = false;
        self.streaming_suppressed = true;
        "Speech worker stopped. Restart the app to retry.".clone_into(&mut self.speech_status);
        false
    }

    fn stop_speech(&mut self) {
        let was_active = self.speaking || self.dictation_requested || self.listening;
        self.cancel_microphone();
        if let Some(session) = &self.speech {
            let _result = session.commands.send(speech::Command::StopSpeaking);
        }
        self.listening = false;
        self.dictation_requested = false;
        self.speaking = false;
        self.speaking_message = None;
        self.streaming_active = false;
        self.streaming_suppressed = true;
        if was_active {
            "Speech stopped. Microphone off.".clone_into(&mut self.speech_status);
            self.speech_notice = true;
        }
    }

    fn cancel_microphone(&mut self) {
        if let Some(session) = &self.speech {
            session.set_input_generation(None);
            let _result = session.commands.send(speech::Command::StopListening);
        }
        if let Some(microphone) = &mut self.microphone {
            microphone.stop();
        }
        self.microphone_generation = None;
        self.dictation_requested = false;
        self.listening = false;
    }

    fn toggle_dictation(&mut self) {
        self.speech_notice = true;
        if self.dictation_requested {
            self.cancel_microphone();
            "Microphone off. Review your message before sending."
                .clone_into(&mut self.speech_status);
        } else if !self.speech_input {
            let message = self.microphone_help();
            message.clone_into(&mut self.speech_status);
            self.help = Some(("Microphone setup", message));
        } else if !self.ready {
            "Enable the microphone after model startup completes."
                .clone_into(&mut self.speech_status);
        } else if !self.busy && !self.speaking {
            let Some(microphone) = &mut self.microphone else {
                return;
            };
            match microphone.start() {
                Ok(generation) => {
                    if let Some(session) = &self.speech {
                        session.set_input_generation(Some(generation));
                    }
                    self.microphone_generation = Some(generation);
                    self.dictation_requested = true;
                    "Requesting microphone permission…".clone_into(&mut self.speech_status);
                    self.focus_composer = true;
                }
                Err(_) => {
                    "Microphone permission is unavailable. Check Windows privacy settings."
                        .clone_into(&mut self.speech_status);
                }
            }
        }
        self.record(&self.speech_status.clone());
    }

    fn toggle_read_aloud(&mut self, message_id: u64) {
        if self.speaking && self.speaking_message == Some(message_id) {
            self.stop_speech();
            return;
        }
        if self.transcript.speech_text(message_id).is_none() {
            return;
        }
        self.speech_notice = true;
        if self.speech_output {
            let voice_description = self.voice_profile.trim().to_owned();
            if voice_description.is_empty() {
                "Describe the voice in Settings before reading aloud."
                    .clone_into(&mut self.speech_status);
                self.show_settings = true;
                self.record(&self.speech_status.clone());
                return;
            }
            let Some(text) = self.transcript.speech_text(message_id).map(str::to_owned) else {
                return;
            };
            self.stop_speech();
            if self.speech_command(speech::Command::Speak {
                text,
                voice_description,
            }) {
                self.speaking = true;
                self.speaking_message = Some(message_id);
                "Reading the selected response aloud.".clone_into(&mut self.speech_status);
            }
        } else {
            let message = self.read_aloud_help();
            message.clone_into(&mut self.speech_status);
            self.help = Some(("Read aloud setup", message));
        }
        self.record(&self.speech_status.clone());
    }

    const fn microphone_help(&self) -> &'static str {
        if !self.canary_configured {
            "Launch scripts/Start-Chat.ps1 with its default speech settings to prepare native Canary Qwen 2.5B automatically. The model loads after you enable the microphone and permit Windows microphone access. See tools/canary-asr/README.md. Nothing is recording now."
        } else if self.microphone.is_none() {
            "Windows microphone access could not initialize. Check Windows microphone privacy settings and restart the app. Nothing is recording."
        } else {
            "Canary Qwen 2.5B is not ready. Start its service on the configured port, check its protected key file, then restart the app. See tools/canary-asr/README.md. Nothing is recording."
        }
    }

    const fn read_aloud_help(&self) -> &'static str {
        if self.tts_configured {
            "Qwen3-TTS VoiceDesign is not ready. Start its service on the configured port, check its protected key file, then restart the app. See tools/qwen-tts/README.md. Your voice description will be used when the service is ready."
        } else {
            "Launch the app with scripts/Start-Chat.ps1 to prepare Qwen3-TTS 1.7B VoiceDesign automatically. Use its default settings without -NoTts or -NoSpeechModels. Selecting a response's speaker button starts the local service. Describe your desired voice in Voice profile; no preset or reference recording is required. See tools/qwen-tts/README.md."
        }
    }

    fn drain_speech_events(&mut self) {
        for _ in 0..32 {
            let Some(session) = &self.speech else { break };
            match session.events.try_recv() {
                Ok(event) => self.speech_event(event),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.speech_checked = true;
                    self.speech_input = false;
                    self.speech_output = false;
                    self.cancel_microphone();
                    self.speaking = false;
                    self.speaking_message = None;
                    self.streaming_active = false;
                    self.streaming_suppressed = true;
                    "Speech unavailable. Text chat remains available."
                        .clone_into(&mut self.speech_status);
                    break;
                }
            }
        }
    }

    fn speech_event(&mut self, event: speech::Event) {
        let previous = self.speech_status.clone();
        match event {
            speech::Event::Capabilities { input, output } => {
                self.speech_checked = true;
                self.speech_input = input && self.microphone.is_some();
                self.speech_output = output;
                match (self.speech_input, output) {
                    (true, true) => "Microphone and read aloud are ready.",
                    (true, false) => "Microphone ready. Select a response's speaker button for setup.",
                    (false, true) => "Response speakers ready. Select Microphone for setup.",
                    (false, false) => {
                        "Speech setup required. Select Microphone or a response's speaker button for instructions."
                    }
                }
                .clone_into(&mut self.speech_status);
            }
            speech::Event::Listening(listening) => {
                self.listening = listening && self.dictation_requested;
                if self.listening {
                    "Recording. Review transcribed text before sending."
                } else if self.dictation_requested {
                    "Microphone enabled."
                } else {
                    "Microphone off."
                }
                .clone_into(&mut self.speech_status);
            }
            speech::Event::Recognized { generation, text } => {
                self.accept_transcript(generation, &text);
            }
            speech::Event::Speaking {
                generation,
                speaking,
            } => self.output_status(
                generation,
                speaking,
                if speaking {
                    if self.streaming_active {
                        "Reading the response as it arrives."
                    } else {
                        "Reading the selected response aloud."
                    }
                } else {
                    "Read aloud stopped."
                },
            ),
            speech::Event::Failed {
                generation,
                message,
            } => self.output_status(generation, false, message),
            speech::Event::OutputStatus {
                generation,
                message,
            } => self.output_status(generation, true, message),
            speech::Event::InputFailed {
                generation,
                message,
            } => {
                if self.dictation_requested && self.microphone_generation == Some(generation) {
                    self.cancel_microphone();
                    message.clone_into(&mut self.speech_status);
                    self.speech_notice = true;
                }
            }
            speech::Event::Status(message) if self.dictation_requested => {
                message.clone_into(&mut self.speech_status);
            }
            speech::Event::Status(_) => {}
        }
        if self.speech_status != previous {
            self.record(&self.speech_status.clone());
        }
    }

    fn output_status(&mut self, generation: u64, speaking: bool, message: &str) {
        if self
            .speech
            .as_ref()
            .is_some_and(|session| session.is_current_output(generation))
        {
            self.speaking = speaking;
            if !speaking {
                self.speaking_message = None;
                self.streaming_active = false;
                self.streaming_suppressed = true;
            }
            message.clone_into(&mut self.speech_status);
        }
    }

    fn accept_transcript(&mut self, generation: u64, text: &str) {
        if !self.dictation_requested
            || self.microphone_generation != Some(generation)
            || !self.ready
            || self.busy
        {
            return;
        }
        if !text.trim().is_empty() {
            self.insert_dictation(text.trim());
        }
        if self
            .microphone
            .as_mut()
            .is_some_and(|microphone| microphone.resume(generation).is_err())
        {
            self.cancel_microphone();
            "Microphone capture became unavailable. Enable it again to retry."
                .clone_into(&mut self.speech_status);
        } else {
            "Transcript ready to review. Resuming microphone…".clone_into(&mut self.speech_status);
        }
    }

    fn place_composer_cursor(&mut self, index: usize) {
        self.recovery_dirty = true;
        let cursor = egui::text::CCursorRange::one(egui::text::CCursor::new(index));
        self.composer_cursor = Some(cursor);
        self.pending_cursor = Some(cursor);
    }

    fn insert_dictation(&mut self, text: &str) {
        let length = self.input.chars().count();
        let range = self.composer_cursor.map_or(length..length, |cursor| {
            let range = cursor.as_sorted_char_range();
            usize::from(range.start).min(length)..usize::from(range.end).min(length)
        });
        let available = INPUT_LIMIT.saturating_sub(length - (range.end - range.start));
        let phrase: String = format!("{text} ").chars().take(available).collect();
        let byte_index = |index| {
            self.input
                .char_indices()
                .nth(index)
                .map_or(self.input.len(), |(byte, _)| byte)
        };
        let bytes = byte_index(range.start)..byte_index(range.end);
        self.input.replace_range(bytes, &phrase);
        self.place_composer_cursor(range.start + phrase.chars().count());
    }

    fn poll_microphone(&mut self) {
        let previous = self.speech_status.clone();
        let Some(microphone) = &mut self.microphone else {
            return;
        };
        for event in microphone.poll() {
            match event {
                microphone::Event::Status(message) if self.dictation_requested => {
                    message.clone_into(&mut self.speech_status);
                }
                microphone::Event::Status(_) => {}
                microphone::Event::Failed(message) => {
                    self.cancel_microphone();
                    message.clone_into(&mut self.speech_status);
                }
                microphone::Event::Clip { generation, wav } => {
                    if self.dictation_requested && self.microphone_generation == Some(generation) {
                        self.listening = false;
                        if self.speech_command(speech::Command::Transcribe { generation, wav }) {
                            "Transcribing locally. Microphone capture is paused."
                                .clone_into(&mut self.speech_status);
                        }
                    }
                }
            }
        }
        if self.speech_status != previous {
            self.record(&self.speech_status.clone());
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(recovery) = &self.recovery {
            if self.recovery_erase_pending {
                recovery.finish_erase();
            } else if self.save_conversations {
                recovery.finish(self.workspace_bytes().ok());
            }
        }
        self.stop_speech();
        self.microphone.take();
        self.speech.take();
        let _sent = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}

fn configure(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style
        .text_styles
        .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 9.0);
    style.spacing.interact_size.y = 34.0;
    style.visuals = forest_visuals();
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}

fn forest_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(20, 25, 24);
    visuals.window_fill = Color32::from_rgb(27, 33, 31);
    visuals.extreme_bg_color = Color32::from_rgb(24, 30, 28);
    visuals.faint_bg_color = Color32::from_rgb(35, 43, 39);
    visuals.override_text_color = Some(Color32::from_rgb(226, 232, 226));
    visuals.selection.bg_fill = Color32::from_rgb(154, 182, 150);
    visuals.selection.stroke = egui::Stroke::new(1.0, Color32::from_rgb(17, 31, 21));
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(42, 51, 46);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(58, 71, 62);
    visuals.widgets.active.bg_fill = Color32::from_rgb(66, 84, 70);
    visuals
}

impl State {
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(16.0);
        ui.label(RichText::new("BONSAI").strong().size(20.0));
        ui.label(RichText::new("A little space to think.").small().weak());
        ui.add_space(24.0);
        if ui
            .add_enabled(
                self.ready && !self.busy,
                egui::Button::new("+ New conversation")
                    .min_size(egui::vec2(ui.available_width(), 42.0)),
            )
            .on_hover_text("Start fresh · Ctrl+N")
            .clicked()
        {
            self.new_chat();
        }
        ui.add_space(24.0);
        self.conversation_list(ui);
        ui.add_space(8.0);
        if !self.transcript.messages.is_empty() && ui.button("Copy conversation").clicked() {
            ui.ctx().copy_text(self.transcript.plain_text());
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Rename"))
                .clicked()
            {
                self.rename_conversation = self
                    .conversations
                    .iter()
                    .find(|conversation| conversation.id == self.active_conversation)
                    .map(|conversation| (conversation.id, conversation.title.clone()));
            }
            if ui
                .add_enabled(self.ready && !self.busy, egui::Button::new("Delete"))
                .clicked()
            {
                self.delete_conversation = Some(self.active_conversation);
            }
        });
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(12.0);
            if ui.button("Settings").clicked() {
                self.show_settings = !self.show_settings;
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new(if self.save_conversations {
                    "Conversation saving is on"
                } else {
                    "Conversations stay in this window"
                })
                .small()
                .weak(),
            );
            ui.label(RichText::new("Private · On your device").small().weak());
            ui.label(
                RichText::new(if self.preview {
                    "Interface preview"
                } else if self.busy {
                    "●  Responding"
                } else if self.ready {
                    "●  Model ready"
                } else if self.retryable {
                    "●  Model paused"
                } else if self.waiting_model {
                    "◌  Preparing model"
                } else {
                    "●  Model unavailable"
                })
                .small(),
            );
        });
    }

    fn abliteration_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Abliteration");
        ui.add_enabled_ui(!self.busy && self.abliteration_enabled, |ui| {
            ui.add(
                egui::Slider::new(&mut self.abliteration_scale, 0.0..=2.0)
                    .step_by(0.1)
                    .fixed_decimals(1)
                    .text("Abliteration scale"),
            );
        });
        ui.label(
            RichText::new(if self.abliteration_enabled {
                "0 turns it off. Applies to your next response."
            } else {
                "Disabled at launch. Restart without -NoAbliterateLora to enable it."
            })
            .small()
            .weak(),
        );
        ui.add_space(16.0);
    }

    fn thinking_controls(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.enable_thinking, "Thinking");
        ui.label(
            RichText::new(
                "Off by default. Applies to your next response and every Extra effort pass.",
            )
            .small()
            .weak(),
        );
        ui.add_space(8.0);
    }

    fn settings(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        egui::Window::new("Settings")
            .open(&mut open)
            .resizable(false)
            .default_width(400.0)
            .default_height(620.0)
            .vscroll(true)
            .max_height((ctx.content_rect().height() - 48.0).max(180.0))
            .show(ctx, |ui| {
                let before = (
                    self.extra_effort,
                    self.enable_thinking,
                    self.output_budget,
                    self.abliteration_scale,
                    self.voice_profile.clone(),
                );
                self.recovery_controls(ui);
                ui.heading("Response");
                ui.add_enabled_ui(!self.busy, |ui| {
                    self.thinking_controls(ui);
                    ui.checkbox(&mut self.extra_effort, "Extra effort (experimental)");
                    ui.label(
                        RichText::new(
                            "Uses up to three passes. Takes longer; quality gains are unproven.",
                        )
                        .small()
                        .weak(),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let label = ui.label("Output cap");
                        egui::ComboBox::from_id_salt("output-cap")
                            .selected_text(format!(
                                "{} tokens",
                                OUTPUT_TOKENS
                                    .get(self.output_budget)
                                    .copied()
                                    .unwrap_or(4096)
                            ))
                            .show_ui(ui, |ui| {
                                for (index, tokens) in OUTPUT_TOKENS.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut self.output_budget,
                                        index,
                                        format!("{tokens} tokens"),
                                    );
                                }
                            })
                            .response
                            .labelled_by(label.id);
                    });
                });
                ui.label(
                    RichText::new("8,192-token context. The output cap includes thinking.")
                        .small()
                        .weak(),
                );
                ui.add_space(16.0);
                self.abliteration_controls(ui);
                ui.heading("Voice");
                let label = ui.label("Voice profile");
                ui.add_enabled(
                    !self.speaking,
                    egui::TextEdit::multiline(&mut self.voice_profile)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .char_limit(VOICE_LIMIT),
                )
                .labelled_by(label.id);
                ui.label(
                    RichText::new(
                        "Describe the voice you want to hear. Applies to your next read aloud.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(16.0);
                if ui.button("Open diagnostics").clicked() {
                    self.show_diagnostics = true;
                }
                ui.label(
                    RichText::new("Technical details stay outside your conversation.")
                        .small()
                        .weak(),
                );
                self.recovery_dirty |= before
                    != (
                        self.extra_effort,
                        self.enable_thinking,
                        self.output_budget,
                        self.abliteration_scale,
                        self.voice_profile.clone(),
                    );
            });
        self.show_settings = open;
    }

    fn diagnostics(&mut self, ctx: &egui::Context) {
        if !self.show_diagnostics {
            return;
        }
        let mut open = true;
        egui::Window::new("Diagnostics")
            .open(&mut open)
            .default_width(640.0)
            .show(ctx, |ui| {
                ui.label("Recent application messages");
                ui.label(
                    RichText::new("Kept in memory for this session. Review paths before sharing.")
                        .small()
                        .weak(),
                );
                if ui.button("Copy diagnostics").clicked() {
                    ui.ctx().copy_text(
                        self.diagnostics
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n\n"),
                    );
                }
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| {
                        for message in &self.diagnostics {
                            ui.add(egui::Label::new(message).selectable(true).wrap());
                            ui.separator();
                        }
                    });
            });
        self.show_diagnostics = open;
    }

    fn header(&mut self, ui: &mut egui::Ui, narrow: bool) {
        ui.horizontal(|ui| {
            if narrow
                && ui
                    .button("Menu")
                    .accessible_name("Conversation menu")
                    .on_hover_text("Conversation menu")
                    .clicked()
            {
                self.sidebar_open = !self.sidebar_open;
            }
            ui.vertical(|ui| {
                ui.label(RichText::new("Swift Bonsai").strong().size(19.0));
                ui.label(
                    RichText::new(if self.preview {
                        "Interface preview · no model or audio services"
                    } else if self.busy {
                        "Making room for your next idea…"
                    } else {
                        "Your private, local AI companion"
                    })
                    .small()
                    .weak(),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Settings").clicked() {
                    self.show_settings = !self.show_settings;
                }
            });
        });
        ui.add_space(8.0);
        ui.separator();
        if self.warning || self.retryable {
            ui.horizontal_wrapped(|ui| {
                let color = if self.warning {
                    ui.visuals().warn_fg_color
                } else {
                    ui.visuals().text_color()
                };
                ui.label(RichText::new(&self.status).color(color));
                if self.retryable && ui.button("Retry model").clicked() {
                    self.retry();
                }
                if ui.small_button("Details").clicked() {
                    self.show_diagnostics = true;
                }
            });
        }
        if self.preview {
            ui.label(
                RichText::new("PREVIEW  ·  Launch normally to use your local model.")
                    .small()
                    .weak(),
            );
        }
        if self.recovery_notice.starts_with("Could not") {
            ui.label(RichText::new(&self.recovery_notice).color(ui.visuals().warn_fg_color));
        }
    }

    fn empty_state(&mut self, ui: &mut egui::Ui) {
        ui.add_space((ui.available_height() * 0.18).max(20.0));
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("B")
                    .size(44.0)
                    .color(ui.visuals().selection.bg_fill),
            );
            ui.add_space(12.0);
            ui.label(RichText::new("What’s on your mind?").strong().size(30.0));
            ui.add_space(8.0);
            ui.label(RichText::new("Write, explore, or work through an idea together.").weak());
            ui.add_space(28.0);
            for (title, prompt) in [
                (
                    "Find the right words",
                    "Help me write a clear, thoughtful message about ",
                ),
                ("Explore an idea", "Help me think through an idea: "),
                ("Make a plan", "Help me create a practical plan for "),
            ] {
                if ui
                    .add(egui::Button::new(title).min_size(egui::vec2(280.0, 42.0)))
                    .clicked()
                {
                    prompt.clone_into(&mut self.input);
                    self.place_composer_cursor(self.input.chars().count());
                    self.focus_composer = true;
                }
            }
        });
    }

    #[allow(
        clippy::too_many_lines,
        reason = "Keep composer focus, IME and action dispatch together."
    )]
    fn composer(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width().min(800.0);
        let margin = ((ui.available_width() - width) * 0.5).max(0.0);
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_width(width);
                egui::Frame::new()
                    .fill(ui.visuals().faint_bg_color)
                    .corner_radius(14)
                    .inner_margin(14)
                    .show(ui, |ui| {
                        let label = ui.label(RichText::new("Message").small().weak());
                        // A pasted manuscript must not push the transcript and
                        // Send/Stop controls outside the window. TextEdit keeps
                        // the caret visible within this native scroll area.
                        let editor = egui::ScrollArea::vertical()
                            .id_salt(("composer-scroll", self.active_conversation))
                            .max_height(160.0)
                            .show(ui, |ui| {
                                let output = ui
                                    .add_enabled_ui(!self.busy, |ui| {
                                        egui::TextEdit::multiline(&mut self.input)
                                            .id_salt("composer")
                                            .hint_text(if self.ready {
                                                "Ask anything…"
                                            } else if self.retryable {
                                                "Retry the model to continue…"
                                            } else if self.waiting_model {
                                                "Write while your local model prepares…"
                                            } else {
                                                "Model unavailable"
                                            })
                                            .desired_rows(3)
                                            .desired_width(f32::INFINITY)
                                            .frame(egui::Frame::NONE)
                                            .char_limit(INPUT_LIMIT)
                                            .return_key(Some(egui::KeyboardShortcut::new(
                                                egui::Modifiers::SHIFT,
                                                egui::Key::Enter,
                                            )))
                                            .show(ui)
                                    })
                                    .inner;
                                let response = output.response.response.labelled_by(label.id);
                                self.recovery_dirty |= response.changed();
                                self.composer_id = Some(response.id);
                                let pending_cursor = self.pending_cursor.take();
                                let cursor = pending_cursor.or(output.cursor_range);
                                if let Some(mut state) =
                                    egui::TextEdit::load_state(ui.ctx(), response.id)
                                {
                                    if let Some(cursor) = pending_cursor {
                                        state.cursor.set_char_range(Some(cursor));
                                        state.store(ui.ctx(), response.id);
                                        ui.ctx().request_repaint();
                                    } else {
                                        self.composer_cursor = state.cursor.char_range();
                                    }
                                }
                                let focus_requested =
                                    self.focus_composer && self.ready && !self.busy;
                                if focus_requested {
                                    response.request_focus();
                                    self.focus_composer = false;
                                }
                                let caret = cursor.map(|cursor| {
                                    output
                                        .galley
                                        .pos_from_cursor(cursor.primary)
                                        .translate(output.galley_pos.to_vec2())
                                });
                                // Focusing a recovered draft can leave the cursor
                                // already at the end, so egui sees no cursor change
                                // and does not request its usual automatic scroll.
                                if (response.has_focus() || focus_requested)
                                    && (response.gained_focus()
                                        || focus_requested
                                        || pending_cursor.is_some()
                                        || ui.input(|input| {
                                            input.key_pressed(egui::Key::End)
                                                || input.key_pressed(egui::Key::Home)
                                        }))
                                    && let Some(caret) = caret
                                {
                                    ui.scroll_to_rect(caret, None);
                                }
                                (response, caret)
                            });
                        #[cfg(test)]
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(
                                egui::Id::unique("composer-viewport"),
                                (
                                    editor.inner.1,
                                    editor.inner_rect,
                                    editor.state.clamped_offset(),
                                ),
                            );
                        });
                        let response = editor.inner.0;
                        let ime_event = ui.input(|input| {
                            input
                                .events
                                .iter()
                                .any(|event| matches!(event, egui::Event::Ime(_)))
                        });
                        if response.has_focus()
                            && !self.composing
                            && !ime_event
                            && ui.input_mut(|input| {
                                let plain_enter = input.events.iter().any(|event| {
                                    matches!(event, egui::Event::Key {
                                        key: egui::Key::Enter,
                                        pressed: true,
                                        modifiers,
                                        ..
                                    } if *modifiers == egui::Modifiers::NONE)
                                });
                                plain_enter
                                    && input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                            })
                        {
                            self.submit();
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_enabled(
                                    self.dictation_requested || (!self.busy && !self.speaking),
                                    egui::Button::new(if self.dictation_requested {
                                        "● Microphone on"
                                    } else {
                                        "Microphone"
                                    }),
                                )
                                .on_hover_text(
                                    "Dictate into your message · never sends automatically",
                                )
                                .clicked()
                            {
                                self.toggle_dictation();
                            }
                            self.read_aloud_controls(ui);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if self.busy {
                                        if ui.button("Stop").clicked() {
                                            self.stop_generation();
                                        }
                                    } else if ui
                                        .add_enabled(
                                            self.ready && !self.input.trim().is_empty(),
                                            egui::Button::new(
                                                RichText::new("Send  ↑")
                                                    .color(ui.visuals().selection.stroke.color),
                                            )
                                            .fill(ui.visuals().selection.bg_fill),
                                        )
                                        .on_hover_text("Send message · Enter")
                                        .clicked()
                                    {
                                        self.submit();
                                    }
                                },
                            );
                        });
                    });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(if self.busy || !self.ready {
                            &self.status
                        } else {
                            "Enter to send · Shift+Enter for a new line"
                        })
                        .small()
                        .weak(),
                    );
                    if self.help.is_none()
                        && (self.speech_notice || self.dictation_requested || self.speaking)
                    {
                        ui.label(RichText::new(&self.speech_status).small());
                    }
                });
            });
        });
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        let narrow = ui.available_width() < 900.0;
        if !narrow {
            egui::Panel::left("navigation")
                .exact_size(238.0)
                .frame(
                    egui::Frame::new()
                        .fill(ui.visuals().window_fill)
                        .inner_margin(18),
                )
                .show(ui, |ui| self.sidebar(ui));
        } else if self.sidebar_open {
            let mut open = self.sidebar_open;
            egui::Window::new("Conversation")
                .open(&mut open)
                .default_width(260.0)
                .show(ui.ctx(), |ui| self.sidebar(ui));
            self.sidebar_open = open;
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(24, 16)))
            .show(ui, |ui| {
                self.header(ui, narrow);
                egui::Panel::bottom("composer-panel")
                    .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 12)))
                    .show(ui, |ui| self.composer(ui));
                let empty = self.transcript.messages.is_empty();
                if !empty {
                    ui.push_id(("conversation-find", self.active_conversation), |ui| {
                        self.transcript.search_bar(ui);
                    });
                }
                egui::ScrollArea::vertical()
                    .id_salt(("conversation-scroll", self.active_conversation))
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if empty {
                            self.empty_state(ui);
                        } else {
                            ui.add_space(20.0);
                            let width = ui.available_width().min(800.0);
                            let margin = ((ui.available_width() - width) * 0.5).max(0.0);
                            ui.horizontal(|ui| {
                                ui.add_space(margin);
                                ui.vertical(|ui| {
                                    ui.set_width(width);
                                    ui.push_id(
                                        ("conversation-text", self.active_conversation),
                                        |ui| {
                                            if let Some(message_id) = self.transcript.show(
                                                ui,
                                                width,
                                                self.speaking_message,
                                                true,
                                            ) {
                                                self.toggle_read_aloud(message_id);
                                            }
                                        },
                                    );
                                });
                            });
                        }
                    });
            });
        let ctx = ui.ctx().clone();
        self.settings(&ctx);
        self.diagnostics(&ctx);
        self.conversation_dialogs(&ctx);
        if let Some((title, message)) = self.help {
            let mut open = true;
            egui::Window::new(title)
                .open(&mut open)
                .default_width(460.0)
                .show(&ctx, |ui| {
                    ui.label(message);
                    if ui.button("Close").clicked() {
                        self.help = None;
                        self.speech_notice = false;
                    }
                });
            if !open {
                self.help = None;
                self.speech_notice = false;
            }
        }
    }
}

impl eframe::App for State {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let was_ready = self.ready;
        let was_busy = self.busy;
        self.drain_events();
        if ((!was_ready && self.ready) || (was_busy && !self.busy && self.ready))
            && ctx.memory(|memory| {
                memory
                    .focused()
                    .is_none_or(|id| Some(id) == self.composer_id)
            })
            && !ctx
                .plugin::<egui::text_selection::LabelSelectionState>()
                .lock()
                .has_selection()
            && !ctx.input(|input| input.pointer.any_down())
        {
            self.focus_composer = true;
        }
        self.poll_microphone();
        self.drain_speech_events();
        self.poll_recovery();
        ctx.input(|input| {
            for event in &input.events {
                match event {
                    egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                        self.composing = !text.is_empty();
                    }
                    egui::Event::Ime(egui::ImeEvent::Commit(_)) => self.composing = false,
                    _ => {}
                }
            }
        });
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::N)) {
            self.new_chat();
        }
        self.handle_escape(ctx);
        if self.last_theme_check.elapsed() >= Duration::from_secs(1) {
            ctx.set_visuals(platform::high_contrast_visuals().unwrap_or_else(forest_visuals));
            self.last_theme_check = Instant::now();
        }
        // No frame-rate polling: wait between events when idle. Microphone's
        // WinRT state machine is polled only during an explicit capture session.
        let waiting = !self.preview
            && (self.waiting_model
                || self.busy
                || self.speaking
                || self.dictation_requested
                || !self.speech_checked);
        ctx.request_repaint_after(if waiting {
            Duration::from_millis(50)
        } else {
            Duration::from_secs(1)
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 860.0])
            .with_min_inner_size([620.0, 640.0]),
        renderer: eframe::Renderer::Wgpu,
        ..eframe::NativeOptions::default()
    }
}

pub(super) fn run(config: Config) -> io::Result<()> {
    let recovery = crate::recovery::Recovery::open(&config.data);
    eframe::run_native(
        "Swift Bonsai",
        native_options(),
        Box::new(move |cc| {
            configure(&cc.egui_ctx);
            let (commands, incoming) = mpsc::channel();
            let (events, incoming_events) = mpsc::channel();
            let microphone = microphone::Microphone::new().ok();
            let mut state = State::create(commands, incoming_events, Some(&config), microphone);
            state.attach_recovery(recovery);
            state.worker = Some(
                std::thread::Builder::new()
                    .name("swift-chat-client".to_owned())
                    .spawn(move || client::run(config, incoming, events))?,
            );
            Ok(Box::new(state))
        }),
    )
    .map_err(|error| io::Error::other(error.to_string()))
}

/// Opens only a seeded interface; no backend, credential or audio operation runs.
pub(super) fn run_preview() -> io::Result<()> {
    eframe::run_native("Swift Bonsai · Preview", native_options(), Box::new(|cc| {
        configure(&cc.egui_ctx);
        let (commands, _incoming) = mpsc::channel();
        let (_events, incoming_events) = mpsc::channel();
        let mut state = State::create(commands, incoming_events, None, None);
        state.ready = true;
        "Interface preview".clone_into(&mut state.status);
        state.transcript.push(Role::User, "I have a few ideas for a short story, but I’m not sure where to start.".to_owned());
        state.transcript.push(Role::Assistant, "Start with the moment something changes.\n\nA quiet bookshop receives a letter addressed to someone who has never existed. Its owner recognizes the handwriting as their own.\n\nWhat draws you in more: the mystery of the letter, or the person who finds it?".to_owned());
        state.last_answer = state.transcript.messages.last().map_or(String::new(), |message| message.body.clone());
        state.answer_complete = true;
        Ok(Box::new(state))
    })).map_err(|error| io::Error::other(error.to_string()))
}

pub(super) fn show_error(message: &str) {
    platform::show_error(message);
}
