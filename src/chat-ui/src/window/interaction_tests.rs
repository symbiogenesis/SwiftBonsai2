//! Real egui input/accessibility regressions with inert backend channels.
//! GPU rendering is opt-in and writes captures only to validated external storage.
use super::*;
use egui_kittest::{Harness, kittest::Queryable as _};
use std::{fs, path::PathBuf};

const QUESTION: &str = "I have a few ideas for a short story, but I am not sure where to start.";
const ANSWER: &str = "Start with the moment something changes.\n\nA quiet bookshop receives a letter addressed to someone who has never existed. Its owner recognizes the handwriting as their own.\n\nWhat draws you in more: the mystery of the letter, or the person who finds it?";

struct Fixture {
    app: State,
    commands: Receiver<Command>,
    _events: Sender<Event>,
}

impl Fixture {
    fn new(seeded: bool) -> Self {
        let (commands, incoming) = mpsc::channel();
        let (events, incoming_events) = mpsc::channel();
        let mut app = State::create(commands, incoming_events, None, None);
        app.preview = false;
        app.ready = true;
        app.waiting_model = false;
        app.status = "Ready when you are.".to_owned();
        if seeded {
            app.transcript.push(Role::User, QUESTION.to_owned());
            app.transcript.push(Role::Assistant, ANSWER.to_owned());
            app.transcript.remove_empty_response();
            app.last_answer = ANSWER.to_owned();
            app.answer_complete = true;
        }
        Self {
            app,
            commands: incoming,
            _events: events,
        }
    }
}

fn harness(size: [f32; 2], seeded: bool) -> Harness<'static, Fixture> {
    Harness::builder()
        .with_size(size)
        .with_theme(egui::Theme::Dark)
        .with_context_setup(|context| {
            configure(context);
            context.add_font_provider(std::sync::Arc::new(
                egui_system_font_provider::SystemFontProvider::new(),
            ));
        })
        .build_ui_state(
            |ui, fixture: &mut Fixture| fixture.app.draw(ui),
            Fixture::new(seeded),
        )
}

#[test]
fn accessible_chat_controls_handle_enter_and_shift_enter() {
    let mut menu = harness([760.0, 800.0], false);
    menu.get_by_role_and_label(egui::Role::Button, "Conversation menu")
        .click();
    menu.run();
    assert!(menu.state().app.sidebar_open);
    assert!(
        menu.get_by_role_and_label(egui::Role::Button, "+ New conversation")
            .rect()
            .is_positive()
    );
    drop(menu);

    let mut chat = harness([1180.0, 860.0], false);
    chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
        .focus();
    chat.run();
    chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
        .type_text("First line");
    chat.run();
    assert!(
        chat.get_by_role_and_label(egui::Role::Button, "Send  ↑")
            .rect()
            .is_positive()
    );

    chat.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Enter);
    chat.run();
    assert_eq!(chat.state().app.input, "First line\n");
    assert!(chat.state().app.transcript.messages.is_empty());
    assert!(matches!(
        chat.state().commands.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
        .type_text("Second line");
    chat.run();
    chat.key_press(egui::Key::Enter);
    chat.run();
    assert!(
        matches!(chat.state().commands.try_recv(), Ok(Command::Send { text, .. })
        if text == "First line\nSecond line")
    );
    assert!(chat.state().app.busy);
    assert_eq!(chat.state().app.transcript.messages.len(), 2);
    assert_eq!(chat.state().app.input, "");
    assert!(
        chat.get_by_role_and_label(egui::Role::Button, "Stop")
            .rect()
            .is_positive()
    );
    assert!(chat.state().app.worker.is_none());
    assert!(chat.state().app.speech.is_none());
    assert!(chat.state().app.microphone.is_none());
}

#[test]
fn composer_keeps_a_draft_while_the_model_prepares() {
    let mut chat = harness([620.0, 760.0], false);
    chat.state_mut().app.ready = false;
    chat.state_mut().app.waiting_model = true;
    chat.run();
    chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
        .focus();
    chat.run();
    chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
        .type_text("Keep this draft while loading 🌱");
    chat.run();
    chat.key_press(egui::Key::Enter);
    chat.run();
    assert_eq!(chat.state().app.input, "Keep this draft while loading 🌱");
    assert!(chat.state().app.transcript.messages.is_empty());
    assert!(matches!(
        chat.state().commands.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn transcript_export_preserves_roles_spacing_and_visible_unicode_only() {
    let mut transcript = Transcript::default();
    assert_eq!(transcript.plain_text(), "");
    transcript.push(Role::User, "First line\n\n界".to_owned());
    transcript.push(Role::Assistant, String::new());
    transcript.append("Private thinking", true);
    assert_eq!(transcript.plain_text(), "You\nFirst line\n\n界");
    transcript.stage();
    transcript.append("Answer 🌱\n", false);
    transcript.append("More private thinking", true);
    transcript.push(Role::User, String::new());
    assert_eq!(
        transcript.plain_text(),
        "You\nFirst line\n\n界\n\nSwift Bonsai\nAnswer 🌱\n"
    );
    transcript.clear();
    assert_eq!(transcript.plain_text(), "");
}

#[test]
fn thinking_toggle_reveals_current_streamed_reasoning_after_each_expansion() {
    let mut chat = harness([1180.0, 860.0], true);
    chat.state_mut()
        .app
        .model_event(Event::Reasoning("First thought.".to_owned()));
    chat.run();
    assert!(chat.query_by_label("First thought.").is_none());
    chat.get_by_role_and_label(egui::Role::Button, "Show thinking")
        .click();
    chat.run();
    assert!(chat.get_by_label("First thought.").rect().is_positive());

    chat.state_mut()
        .app
        .model_event(Event::Reasoning(" A second thought.".to_owned()));
    chat.run();
    assert!(
        chat.get_by_label("First thought. A second thought.")
            .rect()
            .is_positive()
    );
    chat.get_by_role_and_label(egui::Role::Button, "Hide thinking")
        .click();
    chat.run();
    assert!(
        chat.query_by_label("First thought. A second thought.")
            .is_none()
    );

    chat.state_mut()
        .app
        .model_event(Event::Reasoning(" Latest thought.".to_owned()));
    chat.run();
    assert!(
        chat.query_by_label("First thought. A second thought. Latest thought.")
            .is_none()
    );
    chat.get_by_role_and_label(egui::Role::Button, "Show thinking")
        .click();
    chat.run();
    assert!(
        chat.get_by_label("First thought. A second thought. Latest thought.")
            .rect()
            .is_positive()
    );
    assert!(chat.get_by_label(ANSWER).rect().is_positive());
    assert!(!chat.state().app.transcript.plain_text().contains("thought"));
    assert!(chat.state().app.worker.is_none());
    assert!(chat.state().app.speech.is_none());
    assert!(chat.state().app.microphone.is_none());
}

#[test]
fn streaming_append_preserves_a_real_transcript_text_selection() {
    let mut chat = harness([1180.0, 860.0], true);
    chat.run();
    let answer = chat.get_by_label(ANSWER).rect();
    let start = answer.left_top() + egui::vec2(3.0, 8.0);
    let end = answer.left_top() + egui::vec2(130.0, 8.0);
    chat.hover_at(start);
    chat.drag_at(start);
    chat.run_steps(1);
    chat.hover_at(end);
    chat.run_steps(1);
    chat.drop_at(end);
    chat.run();
    assert!(
        chat.ctx
            .plugin::<egui::text_selection::LabelSelectionState>()
            .lock()
            .has_selection()
    );

    chat.state_mut()
        .app
        .model_event(Event::Text("\n\nWe can build from there.".to_owned()));
    chat.run();
    assert!(
        chat.ctx
            .plugin::<egui::text_selection::LabelSelectionState>()
            .lock()
            .has_selection()
    );
    assert!(
        chat.state()
            .app
            .transcript
            .plain_text()
            .ends_with("We can build from there.")
    );
    assert!(chat.state().app.worker.is_none());
    assert!(chat.state().app.speech.is_none());
    assert!(chat.state().app.microphone.is_none());
}

fn capture_directory(value: std::ffi::OsString) -> io::Result<PathBuf> {
    let directory = PathBuf::from(value);
    // This existing boundary rejects relative/unexpanded paths, reparse points,
    // and every Git ancestor before any directory or image can be written.
    bonsai_windows_credentials::validate_external_path(&directory)?;
    Ok(directory)
}

#[test]
fn capture_destination_rejects_relative_and_unexpanded_paths() {
    assert!(capture_directory("captures".into()).is_err());
    assert!(capture_directory("%TEMP%\\Bonsai-ui".into()).is_err());
    assert!(capture_directory(env!("CARGO_MANIFEST_DIR").into()).is_err());
}

#[test]
fn capture_chat_layouts_when_requested() -> io::Result<()> {
    let Some(value) = std::env::var_os("BONSAI_UI_CAPTURE_DIRECTORY") else {
        // Ordinary local/hosted regressions do not initialize a GPU.
        return Ok(());
    };
    let directory = capture_directory(value)?;
    fs::create_dir_all(&directory)?;
    for (name, size, seeded) in [
        ("conversation-wide.png", [1180.0, 860.0], true),
        ("conversation-narrow.png", [620.0, 760.0], true),
        ("empty-state.png", [1180.0, 860.0], false),
        ("settings-narrow.png", [620.0, 640.0], true),
    ] {
        let mut chat = harness(size, seeded);
        if name == "settings-narrow.png" {
            chat.state_mut().app.show_settings = true;
        }
        chat.run();
        chat.render()
            .map_err(io::Error::other)?
            .save(directory.join(name))
            .map_err(io::Error::other)?;
        assert!(chat.state().app.worker.is_none());
        assert!(chat.state().app.speech.is_none());
        assert!(chat.state().app.microphone.is_none());
    }
    Ok(())
}

#[test]
fn find_controls_navigate_unicode_matches_and_refresh_streamed_text() {
    let mut chat = Harness::builder()
        .with_size([900.0, 760.0])
        .with_context_setup(|context| {
            configure(context);
            context.add_font_provider(std::sync::Arc::new(
                egui_system_font_provider::SystemFontProvider::new(),
            ));
        })
        .build_ui_state(
            |ui, transcript: &mut Transcript| {
                transcript.search_bar(ui);
                egui::ScrollArea::vertical().show(ui, |ui| transcript.show(ui, 700.0));
            },
            {
                let mut transcript = Transcript::default();
                transcript.push(Role::User, "Élan and an idea".to_owned());
                transcript.push(Role::Assistant, "An éLAN, then another Élan.".to_owned());
                transcript.append("Reasoning with Élan stays private.", true);
                transcript
            },
        );
    chat.get_by_role_and_label(egui::Role::Button, "Find")
        .click();
    chat.run();
    chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
        .type_text("ÉLAN");
    chat.run();
    assert!(chat.get_by_label("1 of 3").rect().is_positive());
    chat.get_by_role_and_label(egui::Role::Button, "Next match")
        .click();
    chat.run();
    assert!(chat.get_by_label("2 of 3").rect().is_positive());
    chat.get_by_role_and_label(egui::Role::Button, "Previous match")
        .click();
    chat.run();
    assert!(chat.get_by_label("1 of 3").rect().is_positive());
    chat.state_mut().append(" Élan", false);
    chat.run();
    assert!(chat.get_by_label("1 of 4").rect().is_positive());
    assert!(!chat.state().plain_text().contains("Reasoning"));
    chat.get_by_role_and_label(egui::Role::Button, "Find")
        .click();
    chat.run();
    assert!(chat.query_by_label("Find in conversation").is_none());
}

#[test]
fn long_conversation_find_scrolls_to_offscreen_matches_and_keeps_latest_reachable() {
    const FIRST: &str = "Needle at the beginning of the conversation.";
    const LAST: &str = "Needle at the end of the conversation.";
    for size in [[1180.0, 860.0], [620.0, 760.0]] {
        let mut chat = harness(size, false);
        chat.state_mut()
            .app
            .transcript
            .push(Role::User, FIRST.to_owned());
        for index in 0..30 {
            chat.state_mut().app.transcript.push(
                Role::Assistant,
                format!("Middle passage {index}.\nA detail about a character.\nAnother detail about the setting.\nA final detail about the scene."),
            );
        }
        chat.state_mut()
            .app
            .transcript
            .push(Role::Assistant, LAST.to_owned());
        chat.state_mut().app.transcript.remove_empty_response();
        chat.run();
        assert!(chat.query_by_label(FIRST).is_none());
        assert!(chat.get_by_label(LAST).rect().is_positive());
        chat.get_by_role_and_label(egui::Role::Button, "Find")
            .click();
        chat.run();
        chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
            .type_text("Needle");
        chat.run();
        let controls = chat
            .get_by_role_and_label(egui::Role::Button, "Find")
            .rect();
        let first = chat.get_by_label(FIRST).rect();
        let composer = chat
            .get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
            .rect();
        assert!(first.top() >= controls.bottom());
        assert!(first.bottom() < composer.top());
        assert!(chat.query_by_label(LAST).is_none());
        assert!(chat.get_by_label("1 of 2").rect().is_positive());

        chat.get_by_role_and_label(egui::Role::Button, "Next match")
            .click();
        chat.run();
        let last = chat.get_by_label(LAST).rect();
        assert!(last.top() >= controls.bottom());
        assert!(last.bottom() < composer.top());
        assert!(chat.query_by_label(FIRST).is_none());
        assert!(chat.get_by_label("2 of 2").rect().is_positive());
        assert!(
            (chat
                .get_by_role_and_label(egui::Role::Button, "Find")
                .rect()
                .top()
                - controls.top())
            .abs()
                < 0.5
        );

        chat.get_by_role_and_label(egui::Role::Button, "Previous match")
            .click();
        chat.run();
        assert!(chat.get_by_label(FIRST).rect().is_positive());
        assert!(chat.query_by_label(LAST).is_none());
        chat.get_by_role_and_label(egui::Role::Button, "Latest")
            .click();
        chat.run();
        assert!(chat.get_by_label(LAST).rect().is_positive());
        assert!(chat.query_by_label(FIRST).is_none());
        assert!(
            (chat
                .get_by_role_and_label(egui::Role::Button, "Find")
                .rect()
                .top()
                - controls.top())
            .abs()
                < 0.5
        );
    }
}

#[test]
fn real_text_selection_does_not_leak_when_conversation_message_ids_repeat() {
    let mut chat = harness([1180.0, 860.0], true);
    chat.run();
    let original = chat.state().app.active_conversation;
    let answer = chat.get_by_label(ANSWER).rect();
    let start = answer.left_top() + egui::vec2(3.0, 8.0);
    let end = answer.left_top() + egui::vec2(130.0, 8.0);
    chat.hover_at(start);
    chat.drag_at(start);
    chat.run_steps(1);
    chat.hover_at(end);
    chat.run_steps(1);
    chat.drop_at(end);
    chat.run();
    assert!(
        chat.ctx
            .plugin::<egui::text_selection::LabelSelectionState>()
            .lock()
            .has_selection()
    );

    // Switch without a pointer click, which would independently clear selection.
    // Both transcripts assign their first two messages ids 1 and 2.
    chat.state_mut().app.new_chat();
    chat.state_mut()
        .app
        .transcript
        .push(Role::User, "A different question.".to_owned());
    chat.state_mut().app.transcript.push(
        Role::Assistant,
        "An unrelated answer that must not inherit another conversation's text selection."
            .to_owned(),
    );
    chat.state_mut().app.transcript.remove_empty_response();
    chat.run();
    assert!(
        !chat
            .ctx
            .plugin::<egui::text_selection::LabelSelectionState>()
            .lock()
            .has_selection()
    );
    chat.state_mut().app.switch_conversation(original);
    chat.run();
    assert!(chat.get_by_label(ANSWER).rect().is_positive());
    assert!(
        !chat
            .ctx
            .plugin::<egui::text_selection::LabelSelectionState>()
            .lock()
            .has_selection()
    );
}
