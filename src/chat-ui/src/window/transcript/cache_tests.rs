//! Work-count regressions guard streaming and search responsiveness without timing a benchmark.
use super::*;
use egui_kittest::Harness;

fn harness(transcript: Transcript) -> Harness<'static, Transcript> {
    Harness::builder()
        .with_theme(egui::Theme::Dark)
        .with_context_setup(|context| {
            context.add_font_provider(Arc::new(
                egui_system_font_provider::SystemFontProvider::new(),
            ));
        })
        .build_ui_state(
            |ui, transcript: &mut Transcript| {
                egui::ScrollArea::vertical().show(ui, |ui| transcript.show(ui, 600.0, None, true));
            },
            transcript,
        )
}

fn layout_counts(transcript: &Transcript) -> Vec<usize> {
    transcript
        .messages
        .iter()
        .map(|message| message.body_layouts)
        .collect()
}

#[test]
fn streamed_reasoning_and_search_only_relayout_affected_message_bodies() {
    let mut transcript = Transcript::default();
    transcript.push(Role::User, "Hit in the first message".to_owned());
    transcript.push(Role::Assistant, "An unrelated answer".to_owned());
    transcript.push(Role::User, "A second hit".to_owned());
    transcript.push(Role::Assistant, "A streamed hit".to_owned());
    transcript.search.query = "hit".to_owned();
    let mut chat = harness(transcript);
    chat.run();
    assert_eq!(layout_counts(chat.state()), vec![1, 1, 1, 1]);

    chat.state_mut().append(" Private reasoning", true);
    chat.run();
    assert_eq!(layout_counts(chat.state()), vec![1, 1, 1, 1]);
    chat.state_mut().messages[3].show_reasoning = true;
    chat.run();
    assert_eq!(layout_counts(chat.state()), vec![1, 1, 1, 1]);
    assert!(
        chat.state().messages[3]
            .cache
            .as_ref()
            .is_some_and(|cache| cache.reasoning.is_some())
    );

    chat.state_mut().append(" and another hit", false);
    chat.run();
    assert_eq!(layout_counts(chat.state()), vec![1, 1, 1, 2]);
    assert_eq!(chat.state().search.matches.len(), 4);

    chat.state_mut().search.active = 1;
    chat.state_mut().search.revision += 1;
    chat.run();
    assert_eq!(layout_counts(chat.state()), vec![2, 1, 2, 2]);
    chat.run();
    assert_eq!(layout_counts(chat.state()), vec![2, 1, 2, 2]);
}

#[test]
fn transcript_highlights_follow_selection_colors_without_reflowing_plain_messages() {
    let mut transcript = Transcript::default();
    transcript.push(Role::User, "highlight".to_owned());
    transcript.push(Role::Assistant, "plain".to_owned());
    transcript.search.query = "highlight".to_owned();
    let mut chat = harness(transcript);
    chat.run();
    assert_eq!(layout_counts(chat.state()), vec![1, 1]);
    let mut visuals = chat.ctx.style_of(egui::Theme::Dark).visuals.clone();
    visuals.selection.bg_fill = Color32::RED;
    chat.ctx.set_visuals(visuals);
    chat.run();
    assert_eq!(layout_counts(chat.state()), vec![2, 1]);
}
