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
    ] {
        let mut chat = harness(size, seeded);
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
