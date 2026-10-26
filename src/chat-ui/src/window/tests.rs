//! Headless presentation regressions. No model, permission, capture or playback.
use super::*;

fn state() -> (State, Receiver<Command>, Sender<Event>) {
    let (commands, incoming) = mpsc::channel();
    let (events, incoming_events) = mpsc::channel();
    let mut state = State::create(commands, incoming_events, None, None);
    state.preview = false;
    (state, incoming, events)
}

#[test]
fn application_messages_never_enter_the_conversation() {
    let (mut state, _commands, _events) = state();
    state.model_event(Event::Starting);
    state.model_event(Event::Failed(
        "Startup failed. Protected credentials and technical evidence: C:\\private".to_owned(),
    ));
    state.model_event(Event::Retryable("Free memory, then retry.".to_owned()));
    state.model_event(Event::Rejected {
        message: "Prompt exceeds the context limit.".to_owned(),
        text: "Keep this message".to_owned(),
        retry_required: false,
    });
    state.speech_event(speech::Event::Capabilities {
        input: false,
        output: false,
    });
    assert!(state.transcript.messages.is_empty());
    assert!(!state.diagnostics.is_empty());
    assert!(!state.status.contains("C:\\private"));
    assert_eq!(state.input, "Keep this message");
    assert!(state.ready);
    assert!(!state.waiting_model);
}

#[test]
fn stopped_empty_response_leaves_only_conversation_content() {
    let (mut state, commands, _events) = state();
    state.ready = true;
    state.input = "Keep my question".to_owned();
    state.submit();
    assert!(matches!(commands.try_recv(), Ok(Command::Send { .. })));
    state.stop_generation();
    assert!(matches!(commands.try_recv(), Ok(Command::Stop)));
    assert!(state.waiting_model);
    assert_eq!(state.transcript.messages.len(), 1);
    assert_eq!(state.transcript.plain_text(), "You\nKeep my question");
    state.model_event(Event::Retryable("Response cancelled.".to_owned()));
    assert!(!state.waiting_model);
    assert!(state.retryable);
    assert_eq!(state.transcript.messages.len(), 1);
}

#[test]
fn response_controls_preserve_limits_recover_composer_and_retain_complete_draft() {
    let (mut state, commands, _events) = state();
    state.ready = true;
    state.input = "A story about a garden".to_owned();
    state.extra_effort = true;
    state.output_budget = 1;
    state.submit();
    let command = commands.try_recv();
    assert!(matches!(
        command,
        Ok(Command::Send {
            max_completion_tokens: 1024,
            extra_effort: true,
            ..
        })
    ));
    assert!(state.busy);
    assert_eq!(state.input, "");
    assert_eq!(state.transcript.messages.len(), 2);
    state.model_event(Event::Stage("Draft"));
    state.model_event(Event::Text("A complete draft.".to_owned()));
    state.model_event(Event::Draft("A complete draft.".to_owned()));
    state.model_event(Event::Stage("Review"));
    state.model_event(Event::Text("Partial revision".to_owned()));
    let count = state.transcript.messages.len();
    state.model_event(Event::Rejected {
        message: "Review could not finish.".to_owned(),
        text: "A story about a garden".to_owned(),
        retry_required: true,
    });
    assert!(!state.ready);
    assert!(!state.busy);
    assert!(state.answer_complete);
    assert_eq!(state.last_answer, "A complete draft.");
    assert_eq!(state.transcript.messages.len(), count);
    assert_eq!(state.input, "A story about a garden");
    state.model_event(Event::Retryable("Retry the local model.".to_owned()));
    state.retry();
    assert!(matches!(commands.try_recv(), Ok(Command::Retry)));
    assert!(!state.retryable);
    assert_eq!(state.transcript.messages.len(), count);
}

#[test]
fn speech_controls_stay_visible_and_explain_unconfigured_services_without_listening() {
    let (mut state, _commands, _events) = state();
    assert!(!state.dictation_requested);
    assert!(state.microphone.is_none());
    assert!(state.microphone_help().contains("-CanaryKeyFile"));
    assert!(state.read_aloud_help().contains("Start-Demo.ps1"));
    assert!(state.read_aloud_help().contains("-NoTts"));
    state.toggle_dictation();
    assert!(state.help.is_some());
    assert!(!state.dictation_requested);
    assert!(state.microphone_generation.is_none());
    state.help = None;
    state.toggle_read_aloud();
    assert!(state.help.is_some());
    assert!(!state.speaking);
    assert!(state.transcript.messages.is_empty());
}

#[test]
fn cancelled_dictation_cannot_insert_a_late_transcript() {
    let (mut state, _commands, _events) = state();
    state.ready = true;
    state.input = "Existing text".to_owned();
    state.dictation_requested = true;
    state.microphone_generation = Some(7);
    state.cancel_microphone();
    state.accept_transcript(7, "late recognition");
    assert_eq!(state.input, "Existing text");
    assert!(!state.dictation_requested);
    assert!(state.microphone_generation.is_none());
}

#[test]
fn dictation_replaces_the_unicode_selection_and_advances_the_caret() {
    let (mut state, _commands, _events) = state();
    state.ready = true;
    state.dictation_requested = true;
    state.microphone_generation = Some(7);
    state.input = "café 日本語 🌿 end".to_owned();
    state.composer_cursor = Some(egui::text::CCursorRange::two(
        egui::text::CCursor::new(5),
        egui::text::CCursor::new(9),
    ));
    state.accept_transcript(7, "こんにちは");
    assert_eq!(state.input, "café こんにちは 🌿 end");
    assert!(
        state
            .pending_cursor
            .is_some_and(|cursor| usize::from(cursor.primary.index) == 11 && cursor.is_empty())
    );
    state.accept_transcript(7, "again");
    assert_eq!(state.input, "café こんにちは again 🌿 end");
    state.input = "界".repeat(INPUT_LIMIT);
    state.place_composer_cursor(INPUT_LIMIT);
    state.accept_transcript(7, "overflow");
    assert_eq!(state.input.chars().count(), INPUT_LIMIT);
    assert!(state.input.chars().all(|character| character == '界'));
}

#[test]
fn diagnostics_are_bounded_and_separate_from_copyable_chat() {
    let (mut state, _commands, _events) = state();
    state.transcript.push(Role::User, "Hello".to_owned());
    state.transcript.push(Role::Assistant, "Welcome".to_owned());
    for index in 0..200 {
        state.record(&format!("Technical event {index}"));
    }
    assert_eq!(state.diagnostics.len(), MAX_DIAGNOSTICS);
    assert_eq!(
        state.transcript.plain_text(),
        "You\nHello\n\nSwift Bonsai\nWelcome"
    );
}

#[test]
fn chat_renders_at_minimum_window_size_without_starting_services() {
    let (mut state, _commands, _events) = state();
    state.preview = true;
    state.ready = true;
    state.sidebar_open = false;
    state.transcript.push(
        Role::User,
        "Unicode stays intact: café 日本語 🌿".to_owned(),
    );
    state.transcript.push(
        Role::Assistant,
        "A readable answer.\nWith a second paragraph.".to_owned(),
    );
    let context = egui::Context::default();
    configure(&context);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(620.0, 640.0),
        )),
        ..egui::RawInput::default()
    };
    let output = context.run_ui(input, |ui| state.draw(ui));
    assert_ne!(output.shapes.len(), 0);
    assert!(state.worker.is_none());
    assert!(state.microphone.is_none());
    assert!(state.speech.is_none());
}
