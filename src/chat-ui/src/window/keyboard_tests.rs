//! Window-level keyboard routing with inert model and speech channels.
use super::*;
use egui_kittest::{Harness, kittest::Queryable as _};

struct Fixture {
    app: State,
    commands: Receiver<Command>,
    _events: Sender<Event>,
}

fn harness() -> Harness<'static, Fixture> {
    let (commands, incoming) = mpsc::channel();
    let (events, incoming_events) = mpsc::channel();
    let mut app = State::create(commands, incoming_events, None, None);
    app.preview = false;
    app.ready = true;
    app.input = "Preserve this draft".to_owned();
    app.transcript
        .push(Role::User, "Find this message".to_owned());
    Harness::builder()
        .with_size([1180.0, 860.0])
        .with_context_setup(|context| {
            configure(context);
            context.add_font_provider(std::sync::Arc::new(
                egui_system_font_provider::SystemFontProvider::new(),
            ));
        })
        .build_ui_state(
            |ui, fixture: &mut Fixture| {
                fixture.app.handle_escape(ui.ctx());
                fixture.app.draw(ui);
            },
            Fixture {
                app,
                commands: incoming,
                _events: events,
            },
        )
}

fn open_find(chat: &mut Harness<'_, Fixture>) {
    chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F);
    chat.run();
    chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
        .type_text("Find");
    chat.run();
}

#[test]
fn find_escape_preserves_generation_and_draft_before_explicit_stop() {
    let mut chat = harness();
    chat.state_mut().app.busy = true;
    open_find(&mut chat);
    chat.key_press(egui::Key::Enter);
    chat.run();
    assert!(chat.state().app.busy);
    assert!(matches!(
        chat.state().commands.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(chat.query_by_label("Find in conversation").is_none());
    assert!(chat.state().app.busy);
    assert_eq!(chat.state().app.input, "Preserve this draft");
    assert!(matches!(
        chat.state().commands.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(!chat.state().app.busy);
    assert!(matches!(
        chat.state().commands.try_recv(),
        Ok(Command::Stop)
    ));
    assert_eq!(chat.state().app.input, "Preserve this draft");
}

#[test]
fn find_escape_preserves_speech_then_restores_composer_focus() {
    let mut chat = harness();
    chat.state_mut().app.speaking = true;
    open_find(&mut chat);
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(chat.state().app.speaking);
    let composer_id = chat.state().app.composer_id;
    assert_eq!(chat.ctx.memory(egui::Memory::focused), composer_id);
    assert_eq!(chat.state().app.input, "Preserve this draft");

    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(!chat.state().app.speaking);
    assert!(matches!(
        chat.state().commands.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}
