//! Find owns its keyboard commands before the window handles cancellation.
use super::{Search, Visibility, egui};

impl Search {
    pub(in crate::window::transcript) fn close_on_escape(&mut self, ctx: &egui::Context) -> bool {
        if self.visibility == Visibility::Closed
            || !ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            return false;
        }
        self.visibility = Visibility::Closed;
        self.query.clear();
        self.target = None;
        self.invalidate();
        true
    }

    pub(super) fn navigate_on_enter(&mut self, ctx: &egui::Context, field_id: egui::Id) {
        let composing_id = field_id.with("find-ime-composing");
        if self.visibility == Visibility::Closed || !ctx.memory(|memory| memory.has_focus(field_id))
        {
            ctx.data_mut(|data| data.remove::<bool>(composing_id));
            return;
        }
        let mut composing = ctx.data(|data| data.get_temp::<bool>(composing_id).unwrap_or(false));
        let ime_event = ctx.input(|input| {
            let mut ime_event = false;
            for event in &input.events {
                match event {
                    egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                        composing = !text.is_empty();
                        ime_event = true;
                    }
                    egui::Event::Ime(egui::ImeEvent::Commit(_)) => {
                        composing = false;
                        ime_event = true;
                    }
                    egui::Event::Ime(_) => ime_event = true,
                    _ => {}
                }
            }
            ime_event
        });
        ctx.data_mut(|data| data.insert_temp(composing_id, composing));
        if composing || ime_event {
            return;
        }
        let backwards = ctx.input_mut(|input| {
            let modifiers = input.events.iter().find_map(|event| match event {
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    modifiers,
                    ..
                } if *modifiers == egui::Modifiers::NONE
                    || *modifiers == egui::Modifiers::SHIFT =>
                {
                    Some(*modifiers)
                }
                _ => None,
            })?;
            input.consume_key(modifiers, egui::Key::Enter);
            Some(modifiers.shift)
        });
        if let Some(backwards) = backwards {
            self.navigate(backwards);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::transcript::{Role, Transcript};
    use egui_kittest::{Harness, kittest::Queryable as _};

    struct Fixture {
        transcript: Transcript,
        escaped: bool,
        global_escape: bool,
        other_input: String,
    }

    fn harness() -> Harness<'static, Fixture> {
        Harness::builder().with_size([700.0, 300.0]).build_ui_state(
            |ui, fixture: &mut Fixture| {
                fixture.escaped |= fixture.transcript.search.close_on_escape(ui.ctx());
                fixture.global_escape |= ui
                    .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                fixture.transcript.search_bar(ui);
                ui.add(egui::TextEdit::singleline(&mut fixture.other_input))
                    .widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Other field")
                    });
            },
            {
                let mut transcript = Transcript::default();
                transcript.push(Role::User, "Needle then needle then NEEDLE".to_owned());
                Fixture {
                    transcript,
                    escaped: false,
                    global_escape: false,
                    other_input: String::new(),
                }
            },
        )
    }

    fn open_find(chat: &mut Harness<'_, Fixture>) {
        chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F);
        chat.run();
        chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
            .type_text("needle");
        chat.run();
        assert_eq!(chat.state().transcript.search.matches.len(), 3);
    }

    #[test]
    fn find_enter_moves_both_directions_and_keeps_query_focused() {
        let mut chat = harness();
        open_find(&mut chat);
        chat.key_press(egui::Key::Enter);
        chat.run();
        assert_eq!(chat.state().transcript.search.active, 1);
        assert!(chat.get_by_label("2 of 3").rect().is_positive());
        chat.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Enter);
        chat.run();
        assert_eq!(chat.state().transcript.search.active, 0);
        chat.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Enter);
        chat.run();
        assert_eq!(chat.state().transcript.search.active, 2);
        assert!(
            chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
                .is_focused()
        );
        chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
            .type_text("!");
        chat.run();
        assert_eq!(chat.state().transcript.search.query, "needle!");
        assert!(!chat.state().transcript.search.query.contains('\n'));
    }

    #[test]
    fn find_enter_does_not_handle_other_fields_or_modified_enter() {
        let mut chat = harness();
        open_find(&mut chat);
        chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Enter);
        chat.run();
        assert_eq!(chat.state().transcript.search.active, 0);
        chat.get_by_label("Other field").focus();
        chat.run();
        chat.key_press(egui::Key::Enter);
        chat.run();
        assert_eq!(chat.state().transcript.search.active, 0);
    }

    #[test]
    fn find_enter_waits_for_ime_composition_and_commit() {
        let mut chat = harness();
        open_find(&mut chat);
        chat.event(egui::Event::Ime(egui::ImeEvent::Preedit {
            text: "x".to_owned(),
            active_range_chars: None,
        }));
        chat.run();
        chat.key_press(egui::Key::Enter);
        chat.run();
        assert_eq!(chat.state().transcript.search.active, 0);
        chat.input_mut().events.extend([
            egui::Event::Ime(egui::ImeEvent::Commit(String::new())),
            egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        chat.run();
        assert_eq!(chat.state().transcript.search.active, 0);
        chat.key_press(egui::Key::Enter);
        chat.run();
        assert_eq!(chat.state().transcript.search.active, 1);
    }

    #[test]
    fn escape_dismisses_find_before_global_cancel_and_clears_highlights() {
        let mut chat = harness();
        open_find(&mut chat);
        chat.key_press(egui::Key::Enter);
        chat.run();
        assert!(chat.state().transcript.search.target.is_some());
        chat.key_press(egui::Key::Escape);
        chat.run();
        assert!(chat.state().escaped);
        assert!(!chat.state().global_escape);
        assert!(chat.state().transcript.search.query.is_empty());
        assert!(chat.state().transcript.search.target.is_none());
        chat.state_mut().transcript.search.refresh(&[]);
        assert!(chat.state().transcript.search.matches.is_empty());
        assert!(chat.query_by_label("Find in conversation").is_none());
        chat.key_press(egui::Key::Escape);
        chat.run();
        assert!(chat.state().global_escape);
    }
}
