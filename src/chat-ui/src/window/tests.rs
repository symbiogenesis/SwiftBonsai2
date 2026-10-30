//! Headless presentation regressions. No model, permission, capture or playback.
use super::*;

fn state() -> (State, Receiver<Command>, client::EventSender) {
    let (commands, incoming) = mpsc::channel();
    let (events, incoming_events) = client::event_channel();
    let mut state = State::create(commands, incoming_events, None, None);
    state.preview = false;
    state.model = ModelLifecycle::Preparing;
    (state, incoming, events)
}

#[test]
fn model_notifications_have_a_finite_per_frame_work_budget() {
    let (mut state, _commands, events) = state();
    for _ in 0..80 {
        assert!(events.send(Event::Stage("Queued stage")).is_ok());
    }
    state.drain_events();
    assert_eq!(state.events.try_iter().count(), 16);
}

#[test]
fn application_messages_never_enter_the_conversation() {
    let (mut state, _commands, _events) = state();
    state.model_event(Event::Starting);
    state.model_event(Event::Failed(
        "Startup failed. Protected credentials and technical evidence: C:\\private".to_owned(),
    ));
    state.model = ModelLifecycle::Generating;
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
    assert!(state.model.ready());
    assert!(!state.model.waiting_model());
}

#[test]
fn stopped_empty_response_leaves_only_conversation_content() {
    let (mut state, commands, _events) = state();
    state.model = ModelLifecycle::Ready;
    state.input = "Keep my question".to_owned();
    state.submit();
    assert!(matches!(commands.try_recv(), Ok(Command::Send { .. })));
    state.stop_generation();
    assert!(matches!(commands.try_recv(), Ok(Command::Stop)));
    assert!(state.model.waiting_model());
    assert_eq!(state.transcript.messages.len(), 1);
    assert_eq!(state.transcript.plain_text(), "You\nKeep my question");
    state.model_event(Event::Stopping("Keep my question".to_owned()));
    assert_eq!(state.input, "Keep my question");
    assert!(!state.warning);
    assert!(!state.model.retryable());
    state.model_event(Event::Stopped);
    assert!(!state.model.waiting_model());
    assert!(state.model.retryable());
    assert!(!state.warning);
    assert_eq!(state.transcript.messages.len(), 1);
}

#[test]
fn stop_freezes_queued_output_and_preserves_a_new_composer_draft() {
    let (mut state, commands, events) = state();
    state.model = ModelLifecycle::Ready;
    state.input = "Original question".to_owned();
    state.submit();
    assert!(matches!(commands.try_recv(), Ok(Command::Send { .. })));
    state.model_event(Event::Text("Partial answer".to_owned()));
    state.stop_generation();
    state.stop_generation();
    assert!(matches!(commands.try_recv(), Ok(Command::Stop)));
    assert!(commands.try_recv().is_err());
    let transcript = state.transcript.plain_text();
    let status = state.status.clone();
    for event in [
        Event::Text(" late text".to_owned()),
        Event::Reasoning("late reasoning".to_owned()),
        Event::Stage("Late stage"),
        Event::Draft("Late draft".to_owned()),
        Event::RetainedAnswer("Late restored draft".to_owned()),
        Event::Complete(Completion::Finished),
    ] {
        assert!(events.send(event).is_ok());
    }
    state.drain_events();
    assert_eq!(state.transcript.plain_text(), transcript);
    assert_eq!(state.status, status);
    assert!(state.model.stopping() && state.model.waiting_model());
    assert!(!state.model.ready() && !state.model.retryable() && !state.warning);
    state.input = "My next question".to_owned();
    state.model_event(Event::Stopping("Original question".to_owned()));
    assert_eq!(state.input, "My next question");
    state.model_event(Event::Stopped);
    assert!(!state.warning);
    state.retry();
    assert!(matches!(commands.try_recv(), Ok(Command::Retry)));
    assert!(!state.model.ready());
}

#[test]
fn queued_rejections_after_stop_preserve_teardown_and_composer_drafts() {
    for retry_required in [false, true] {
        for newer_draft in ["", "My next question"] {
            let (mut state, commands, events) = state();
            state.model = ModelLifecycle::Ready;
            state.input = "Original question".to_owned();
            state.submit();
            assert!(matches!(commands.try_recv(), Ok(Command::Send { .. })));
            state.model_event(Event::Text("Partial answer".to_owned()));
            state.stop_generation();
            assert!(matches!(commands.try_recv(), Ok(Command::Stop)));
            newer_draft.clone_into(&mut state.input);
            let transcript = state.transcript.plain_text();
            let status = state.status.clone();
            assert!(
                events
                    .send(Event::Rejected {
                        message: "Queued rejection".to_owned(),
                        text: "Original question".to_owned(),
                        retry_required,
                    })
                    .is_ok()
            );

            state.drain_events();
            assert!(state.model.stopping() && state.model.waiting_model());
            assert!(
                !state.model.ready()
                    && !state.model.busy()
                    && !state.model.retryable()
                    && !state.warning
            );
            assert_eq!(state.status, status);
            assert_eq!(state.transcript.plain_text(), transcript);
            assert_eq!(
                state.input,
                if newer_draft.is_empty() {
                    "Original question"
                } else {
                    newer_draft
                }
            );
            state.submit();
            assert!(commands.try_recv().is_err());

            state.model_event(Event::Stopped);
            assert!(
                state.model.retryable() && !state.model.ready() && !state.model.waiting_model()
            );
            state.retry();
            assert!(matches!(commands.try_recv(), Ok(Command::Retry)));
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn queued_rejection_after_stop_retains_completed_extra_effort_draft() {
    for retry_required in [false, true] {
        let (mut state, commands, events) = state();
        state.model = ModelLifecycle::Ready;
        state.input = "Original question".to_owned();
        state.submit();
        assert!(matches!(commands.try_recv(), Ok(Command::Send { .. })));
        state.model_event(Event::Text("A complete draft.".to_owned()));
        state.model_event(Event::Draft("A complete draft.".to_owned()));
        state.model_event(Event::Stage("Review"));
        state.model_event(Event::Text("An unfinished revision".to_owned()));
        state.stop_generation();
        assert!(matches!(commands.try_recv(), Ok(Command::Stop)));
        let transcript = state.transcript.plain_text();
        let status = state.status.clone();
        assert!(
            events
                .send(Event::Rejected {
                    message: "Review failed".to_owned(),
                    text: "Original question".to_owned(),
                    retry_required,
                })
                .is_ok()
        );

        state.drain_events();
        assert_eq!(state.status, status);
        state.model_event(Event::Stopped);
        assert_eq!(state.last_answer, "A complete draft.");
        assert!(state.answer_complete);
        assert_eq!(state.transcript.plain_text(), transcript);
        assert_eq!(state.input, "Original question");
        assert!(state.committed.is_empty());
        assert!(state.model.retryable() && !state.model.ready());
        assert_eq!(state.status, client::STOPPED_MESSAGE);
    }
}

#[test]
fn completed_turn_winning_stop_race_keeps_visible_and_committed_answers_consistent() {
    let (mut state, _commands, _events) = state();
    state.model = ModelLifecycle::Ready;
    state.input = "A question".to_owned();
    state.submit();
    state.model_event(Event::Text("Partial".to_owned()));
    state.stop_generation();
    let status = state.status.clone();
    state.model_event(Event::Committed(vec![
        client::Message {
            role: "user",
            content: "A question".into(),
            reasoning_content: None,
        },
        client::Message {
            role: "assistant",
            content: "Complete answer".into(),
            reasoning_content: Some("Complete reasoning".into()),
        },
    ]));
    state.model_event(Event::Complete(Completion::Finished));
    assert_eq!(state.transcript.messages.len(), 2);
    assert!(state.transcript.plain_text().ends_with("Complete answer"));
    assert_eq!(state.committed[1].content.as_ref(), state.last_answer);
    assert!(state.answer_complete);
    assert_eq!(state.status, status);
    assert!(!state.model.ready() && !state.model.retryable() && !state.warning);
    state.model_event(Event::Stopped);
    assert!(state.model.retryable());
    assert!(!state.warning);
}

#[test]
fn response_controls_preserve_limits_recover_composer_and_retain_complete_draft() {
    let (mut state, commands, _events) = state();
    state.model = ModelLifecycle::Ready;
    state.input = "A story about a garden".to_owned();
    state.extra_effort = true;
    state.output_budget = 1;
    state.abliteration_scale = 1.7;
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
    assert!(state.model.busy());
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
    assert!(!state.model.ready());
    assert!(!state.model.busy());
    assert!(state.answer_complete);
    assert_eq!(state.last_answer, "A complete draft.");
    assert_eq!(state.transcript.messages.len(), count);
    assert_eq!(state.input, "A story about a garden");
    state.model_event(Event::Retryable("Retry the local model.".to_owned()));
    state.retry();
    assert!(matches!(commands.try_recv(), Ok(Command::Retry)));
    assert!(!state.model.retryable());
    assert_eq!(state.transcript.messages.len(), count);
}

#[test]
fn speech_controls_stay_visible_and_explain_unconfigured_services_without_listening() {
    let (mut state, _commands, _events) = state();
    assert!(!state.dictation_requested);
    assert!(state.microphone.is_none());
    assert!(state.microphone_help().contains("Start-Chat.ps1"));
    assert!(state.read_aloud_help().contains("Start-Chat.ps1"));
    assert!(state.read_aloud_help().contains("-NoTts"));
    state.toggle_streaming_read_aloud();
    assert!(state.help.is_some());
    assert!(!state.streaming_read_aloud);
    state.help = None;
    state.toggle_dictation();
    assert!(state.help.is_some());
    assert!(!state.dictation_requested);
    assert!(state.microphone_generation.is_none());
    state.help = None;
    state
        .transcript
        .push(Role::Assistant, "A completed answer".to_owned());
    state.transcript.remove_empty_response();
    let message_id = state.transcript.messages[0].id;
    state.toggle_read_aloud(message_id);
    assert!(state.help.is_some());
    assert!(!state.speaking);
    assert_eq!(state.transcript.messages.len(), 1);
}

#[test]
fn response_speakers_select_individual_answers_and_ignore_replaced_playback_events()
-> io::Result<()> {
    let (mut state, _commands, _events) = state();
    let (session, speech_commands) = speech::Session::fixture()?;
    state.speech = Some(session);
    state.speech_output = true;
    state.model = ModelLifecycle::Ready;
    state
        .transcript
        .push(Role::User, "Do not speak my prompt".to_owned());
    state
        .transcript
        .push(Role::Assistant, "The older answer".to_owned());
    state.transcript.append("Private reasoning", true);
    state.transcript.remove_empty_response();
    state
        .transcript
        .push(Role::Assistant, "The latest answer".to_owned());
    state.transcript.remove_empty_response();
    let user = state.transcript.messages[0].id;
    let older = state.transcript.messages[1].id;
    let latest = state.transcript.messages[2].id;
    state.last_answer = "Unrelated last-answer state".to_owned();
    state.answer_complete = false;
    state.toggle_read_aloud(user);
    assert!(!state.speaking);
    state.toggle_read_aloud(older);
    assert_eq!(state.speaking_message, Some(older));
    assert!(matches!(
        speech_commands.recv_timeout(Duration::from_secs(2)),
        Ok(speech::Command::StopListening)
    ));
    assert!(matches!(
        speech_commands.recv_timeout(Duration::from_secs(2)),
        Ok(speech::Command::StopSpeaking)
    ));
    assert!(
        matches!(speech_commands.recv_timeout(Duration::from_secs(2)), Ok(speech::Command::Speak { text, voice_description })
        if text == "The older answer" && voice_description == DEFAULT_VOICE)
    );
    state.toggle_read_aloud(latest);
    assert_eq!(state.speaking_message, Some(latest));
    assert!(matches!(
        speech_commands.recv_timeout(Duration::from_secs(2)),
        Ok(speech::Command::StopListening)
    ));
    assert!(matches!(
        speech_commands.recv_timeout(Duration::from_secs(2)),
        Ok(speech::Command::StopSpeaking)
    ));
    assert!(
        matches!(speech_commands.recv_timeout(Duration::from_secs(2)), Ok(speech::Command::Speak { text, .. }) if text == "The latest answer")
    );
    state.speech_event(speech::Event::Speaking {
        generation: 2,
        speaking: false,
    });
    assert!(state.speaking);
    assert_eq!(state.speaking_message, Some(latest));
    state.speech_event(speech::Event::Failed {
        generation: 4,
        message: "Speech failed",
    });
    assert!(!state.speaking);
    assert_eq!(state.speaking_message, None);
    state.toggle_read_aloud(older);
    state.toggle_read_aloud(older);
    assert!(!state.speaking);
    assert_eq!(state.speaking_message, None);
    state
        .transcript
        .push(Role::Assistant, "Still streaming".to_owned());
    state.toggle_read_aloud(state.transcript.messages[3].id);
    assert!(!state.speaking);
    state.model = ModelLifecycle::Generating;
    state.toggle_read_aloud(older);
    assert!(state.speaking);
    assert_eq!(state.speaking_message, Some(older));
    state.model = ModelLifecycle::Ready;
    state.toggle_read_aloud(latest);
    state.new_chat();
    assert!(!state.speaking);
    assert_eq!(state.speaking_message, None);
    Ok(())
}

#[test]
fn speech_memory_failure_keeps_text_chat_and_completed_answer_available() -> io::Result<()> {
    let (mut state, commands, _events) = state();
    state.model = ModelLifecycle::Ready;
    state.input = "A short answer".to_owned();
    state.submit();
    assert!(matches!(commands.try_recv(), Ok(Command::Send { .. })));
    state.model_event(Event::Text("The completed answer.".to_owned()));
    state.model_event(Event::Complete(Completion::Finished));
    let transcript = state.transcript.plain_text();
    state.speech = Some(speech::start(None, None)?);
    state.speech_output = true;
    state.speaking = true;
    state.speech_event(speech::Event::Failed {
        generation: 0,
        message: "Read aloud stopped to preserve text chat memory.",
    });
    assert!(state.model.ready());
    assert!(state.answer_complete);
    assert!(!state.speaking);
    assert!(!state.model.retryable());
    assert_eq!(state.last_answer, "The completed answer.");
    assert_eq!(state.transcript.plain_text(), transcript);
    assert!(state.speech_status.contains("preserve text chat"));
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    Ok(())
}

#[test]
fn cancelled_dictation_cannot_insert_a_late_transcript() {
    let (mut state, _commands, _events) = state();
    state.model = ModelLifecycle::Ready;
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
    state.model = ModelLifecycle::Ready;
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
    state.model = ModelLifecycle::Ready;
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
    let mut output = context.run_ui(input, |ui| state.draw(ui));
    assert_ne!(output.shapes.len(), 0);
    output.textures_delta.clear();
    assert!(state.worker.is_none());
    assert!(state.microphone.is_none());
    assert!(state.speech.is_none());
}

#[test]
fn conversation_workspace_preserves_drafts_settings_and_exact_committed_turns() {
    let (mut state, commands, _events) = state();
    state.model = ModelLifecycle::Ready;
    state.input = "A garden story".to_owned();
    state.submit();
    assert!(matches!(commands.try_recv(), Ok(Command::Send { .. })));
    state.model_event(Event::Text("Complete story".to_owned()));
    state.model_event(Event::Committed(vec![
        client::Message {
            role: "user",
            content: "A garden story".into(),
            reasoning_content: None,
        },
        client::Message {
            role: "assistant",
            content: "Complete story".into(),
            reasoning_content: Some("Private reasoning".into()),
        },
    ]));
    state.model_event(Event::Complete(Completion::Finished));
    state.input = "An unsent revision 🌱".to_owned();
    state.extra_effort = true;
    state.enable_thinking = true;
    state.output_budget = 1;
    state.abliteration_scale = 1.7;
    let original = state.active_conversation;
    let visible = state.transcript.plain_text();
    state.new_chat();
    assert!(matches!(commands.try_recv(), Ok(Command::NewChat)));
    assert_eq!(state.conversations.len(), 2);
    assert_eq!(state.conversations[0].title, "A garden story");
    assert!(state.transcript.messages.is_empty());
    assert!(state.committed.is_empty());
    assert_eq!(state.input, "");
    assert!(!state.extra_effort);
    assert!(!state.enable_thinking);
    state.input = "Second conversation draft".to_owned();
    let second = state.active_conversation;
    state.switch_conversation(original);
    let restored = commands.try_recv();
    assert!(
        matches!(&restored, Ok(Command::RestoreChat(history)) if history.len() == 2 && history[1].reasoning_content.as_deref() == Some("Private reasoning"))
    );
    assert_eq!(state.transcript.plain_text(), visible);
    assert_eq!(state.input, "An unsent revision 🌱");
    assert!(state.extra_effort);
    assert!(state.enable_thinking);
    assert_eq!(state.output_budget, 1);
    assert!((state.abliteration_scale - 1.7).abs() < f32::EPSILON);
    state.switch_conversation(second);
    assert!(matches!(commands.try_recv(), Ok(Command::RestoreChat(history)) if history.is_empty()));
    assert_eq!(state.input, "Second conversation draft");
    assert!(!state.enable_thinking);
}

#[test]
fn conversation_workspace_never_promotes_interrupted_visible_output() {
    let (mut state, commands, _events) = state();
    state.model = ModelLifecycle::Ready;
    state.input = "Unfinished request".to_owned();
    state.submit();
    assert!(matches!(commands.try_recv(), Ok(Command::Send { .. })));
    state.model_event(Event::Text("Partial answer".to_owned()));
    state.model_event(Event::Rejected {
        message: "Interrupted".to_owned(),
        text: "Unfinished request".to_owned(),
        retry_required: true,
    });
    state.model_event(Event::Retryable("Retry required".to_owned()));
    state.retry();
    assert!(matches!(commands.try_recv(), Ok(Command::Retry)));
    state.model_event(Event::Starting);
    state.model_event(Event::Ready);
    let original = state.active_conversation;
    state.new_chat();
    assert!(matches!(commands.try_recv(), Ok(Command::NewChat)));
    state.switch_conversation(original);
    assert!(matches!(commands.try_recv(), Ok(Command::RestoreChat(history)) if history.is_empty()));
    assert!(state.transcript.plain_text().contains("Partial answer"));
    assert_eq!(state.input, "Unfinished request");
    assert!(state.committed.is_empty());
}

#[test]
fn conversation_workspace_bounds_count_blocks_busy_switches_and_deletes_explicitly() {
    let (mut state, commands, _events) = state();
    state.model = ModelLifecycle::Ready;
    let original = state.active_conversation;
    state.new_chat();
    assert!(matches!(commands.try_recv(), Ok(Command::NewChat)));
    let second = state.active_conversation;
    state.model = ModelLifecycle::Generating;
    state.switch_conversation(original);
    state.remove_conversation(second);
    state.new_chat();
    assert_eq!(state.active_conversation, second);
    assert_eq!(state.conversations.len(), 2);
    assert!(commands.try_recv().is_err());
    state.model = ModelLifecycle::Ready;
    state.remove_conversation(second);
    assert_eq!(state.active_conversation, original);
    assert_eq!(state.conversations.len(), 1);
    assert!(matches!(commands.try_recv(), Ok(Command::RestoreChat(_))));
    state.remove_conversation(original);
    assert_ne!(state.active_conversation, original);
    assert_eq!(state.conversations.len(), 1);
    assert!(matches!(commands.try_recv(), Ok(Command::NewChat)));
    for _ in 1..MAX_CONVERSATIONS {
        state.new_chat();
        assert!(matches!(commands.try_recv(), Ok(Command::NewChat)));
    }
    state.new_chat();
    assert_eq!(state.conversations.len(), MAX_CONVERSATIONS);
    assert!(state.warning);
    assert!(commands.try_recv().is_err());
}

#[test]
fn model_lifecycle_requires_teardown_and_explicit_retry_before_verified_readiness() {
    let (mut state, commands, _events) = state();
    state.model_event(Event::Ready);
    state.input = "Original question".into();
    state.submit();
    assert!(matches!(commands.try_recv(), Ok(Command::Send { .. })));
    assert_eq!(state.model, ModelLifecycle::Generating);
    for event in [Event::Ready, Event::Starting, Event::Stopped] {
        state.model_event(event);
        assert_eq!(state.model, ModelLifecycle::Generating);
    }
    state.input = "A newer draft".into();
    state.model_event(Event::Rejected {
        message: "Connection failed".into(),
        text: "Original question".into(),
        retry_required: true,
    });
    assert_eq!(state.model, ModelLifecycle::AwaitingTeardown);
    assert_eq!(state.input, "A newer draft");
    let status = state.status.clone();
    for event in [
        Event::Ready,
        Event::Starting,
        Event::Complete(Completion::Finished),
    ] {
        state.model_event(event);
        assert_eq!(state.model, ModelLifecycle::AwaitingTeardown);
        assert_eq!(state.status, status);
    }
    state.submit();
    state.retry();
    assert!(commands.try_recv().is_err());
    state.model_event(Event::Retryable("Teardown finished".into()));
    assert_eq!(state.model, ModelLifecycle::Retryable);
    state.model_event(Event::Ready);
    assert_eq!(state.model, ModelLifecycle::Retryable);
    state.retry();
    assert!(matches!(commands.try_recv(), Ok(Command::Retry)));
    assert_eq!(state.model, ModelLifecycle::Restarting);
    for event in [
        Event::Retryable("Old teardown notice".into()),
        Event::Ready,
        Event::Stopped,
        Event::Stopping("Old prompt".into()),
        Event::Complete(Completion::Finished),
    ] {
        state.model_event(event);
        assert_eq!(state.model, ModelLifecycle::Restarting);
    }
    state.submit();
    assert!(commands.try_recv().is_err());
    state.model_event(Event::Starting);
    assert_eq!(state.model, ModelLifecycle::Preparing);
    state.model_event(Event::Ready);
    assert_eq!(state.model, ModelLifecycle::Ready);
    assert_eq!(state.input, "A newer draft");
}

#[test]
fn model_lifecycle_stop_and_terminal_failure_never_reopen_send_from_late_events() {
    let (mut state, commands, _events) = state();
    state.model_event(Event::Ready);
    state.input = "Original question".into();
    state.submit();
    assert!(matches!(commands.try_recv(), Ok(Command::Send { .. })));
    state.stop_generation();
    assert!(matches!(commands.try_recv(), Ok(Command::Stop)));
    for event in [
        Event::Ready,
        Event::Starting,
        Event::Complete(Completion::Finished),
    ] {
        state.model_event(event);
        assert_eq!(state.model, ModelLifecycle::Stopping);
    }
    state.input = "A newer draft".into();
    state.model_event(Event::Failed("Owned teardown could not be verified".into()));
    assert_eq!(state.model, ModelLifecycle::Unavailable);
    let status = state.status.clone();
    for event in [
        Event::Ready,
        Event::Starting,
        Event::Stopped,
        Event::Retryable("Old notice".into()),
        Event::Rejected {
            message: "Old rejection".into(),
            text: "Original question".into(),
            retry_required: false,
        },
    ] {
        state.model_event(event);
        assert_eq!(state.model, ModelLifecycle::Unavailable);
        assert_eq!(state.status, status);
        assert_eq!(state.input, "A newer draft");
    }
    state.submit();
    state.retry();
    assert!(commands.try_recv().is_err());
}

#[test]
fn model_lifecycle_recoverable_rejection_preserves_a_newer_composer_draft() {
    let (mut state, commands, _events) = state();
    state.model_event(Event::Ready);
    state.input = "Original question".into();
    state.submit();
    assert!(matches!(commands.try_recv(), Ok(Command::Send { .. })));
    state.input = "A newer draft".into();
    state.model_event(Event::Rejected {
        message: "Prompt rejected".into(),
        text: "Original question".into(),
        retry_required: false,
    });
    assert_eq!(state.model, ModelLifecycle::Ready);
    assert_eq!(state.input, "A newer draft");
    state.model_event(Event::Complete(Completion::Finished));
    assert_eq!(state.status, "Prompt rejected");
    assert!(!state.answer_complete);
}

#[test]
fn model_lifecycle_queued_rejection_cannot_cancel_an_explicit_restart() {
    for newer_draft in ["", "A newer draft"] {
        let (mut state, commands, _events) = state();
        state.model_event(Event::Retryable("Teardown finished".into()));
        newer_draft.clone_into(&mut state.input);
        state.retry();
        assert!(matches!(commands.try_recv(), Ok(Command::Retry)));
        let status = state.status.clone();
        state.model_event(Event::Rejected {
            message: "Queued behind teardown".into(),
            text: "Original question".into(),
            retry_required: true,
        });
        assert_eq!(state.model, ModelLifecycle::Restarting);
        assert_eq!(state.status, status);
        assert_eq!(
            state.input,
            if newer_draft.is_empty() {
                "Original question"
            } else {
                newer_draft
            }
        );
        for event in [
            Event::Retryable("Duplicate teardown notice".into()),
            Event::Stopped,
            Event::Ready,
        ] {
            state.model_event(event);
            assert_eq!(state.model, ModelLifecycle::Restarting);
            assert_eq!(state.status, status);
            state.retry();
            state.submit();
            assert!(commands.try_recv().is_err());
        }
        state.model_event(Event::Starting);
        assert_eq!(state.model, ModelLifecycle::Preparing);
        state.model_event(Event::Ready);
        assert_eq!(state.model, ModelLifecycle::Ready);
    }
}
