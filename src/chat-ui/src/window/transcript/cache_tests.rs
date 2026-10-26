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

fn assert_byte_accounting(transcript: &Transcript) {
    assert_eq!(
        transcript.text_bytes,
        transcript
            .messages
            .iter()
            .map(Message::text_bytes)
            .sum::<usize>()
    );
}

#[test]
fn transcript_accounting_streaming_visits_no_completed_messages() {
    let mut transcript = Transcript::default();
    for _ in 0..400 {
        transcript.push(Role::User, "Completed 界 🌱".to_owned());
    }
    transcript.push(Role::Assistant, String::new());
    for _ in 0..4096 {
        transcript.append("答🌱", false);
        transcript.append("考", true);
    }
    assert_byte_accounting(&transcript);
    assert_eq!(transcript.messages.len(), 401);
    assert_eq!(transcript.trim_message_visits, 0);
    assert_eq!(transcript.trim_batches, 0);
}

#[test]
fn transcript_accounting_bulk_eviction_drains_only_the_required_prefix() {
    let mut transcript = Transcript::default();
    for _ in 0..8 {
        transcript.push(Role::User, "x".repeat(MAX_BYTES / 8));
    }
    transcript.push(Role::Assistant, String::new());
    let retained_id = transcript.messages[4].id;
    transcript.replace_response(&"y".repeat(MAX_BYTES / 2), None);
    assert_byte_accounting(&transcript);
    assert_eq!(transcript.text_bytes, MAX_BYTES);
    assert_eq!(transcript.messages.len(), 5);
    assert_eq!(transcript.messages[0].id, retained_id);
    assert_eq!(transcript.trim_message_visits, 4);
    assert_eq!(transcript.trim_batches, 1);

    // A streamed continuation crosses the bound again without revisiting any
    // message already evicted or shifting the suffix more than once.
    transcript.append("界", true);
    assert_byte_accounting(&transcript);
    assert_eq!(transcript.messages.len(), 4);
    assert_eq!(transcript.trim_message_visits, 5);
    assert_eq!(transcript.trim_batches, 2);
}

#[test]
fn transcript_accounting_preserves_replacement_stage_clear_and_last_two() {
    let mut transcript = Transcript::default();
    transcript.append("ignored without an assistant", false);
    assert_byte_accounting(&transcript);
    transcript.push(Role::User, "界".to_owned());
    transcript.append("ignored after a user", true);
    assert_eq!(transcript.text_bytes, "界".len());
    transcript.replace_response("🌱 answer", Some("考"));
    assert_byte_accounting(&transcript);
    transcript.stage();
    assert_eq!(transcript.messages.len(), 3);
    assert_byte_accounting(&transcript);
    transcript.replace_response("短", None);
    assert_byte_accounting(&transcript);
    transcript.replace_response("", None);
    assert_eq!(transcript.messages.len(), 2);
    assert_byte_accounting(&transcript);
    transcript.replace_response("replaced", Some("🌱"));
    assert_byte_accounting(&transcript);

    transcript.append(&"x".repeat(MAX_BYTES), false);
    assert_eq!(transcript.messages.len(), 2);
    assert!(transcript.text_bytes > MAX_BYTES);
    assert!(!transcript.valid_saved());
    assert_byte_accounting(&transcript);
    transcript.clear();
    assert_eq!(transcript.text_bytes, 0);
    transcript.push(Role::Assistant, String::new());
    transcript.remove_empty_response();
    assert!(transcript.messages.is_empty());
    assert_byte_accounting(&transcript);
}

#[test]
fn transcript_accounting_restore_rebuilds_total_without_changing_saved_schema()
-> Result<(), serde_json::Error> {
    let mut transcript = Transcript::default();
    transcript.push(Role::User, "界 🌱".to_owned());
    transcript.replace_response("答", Some("考"));
    let bytes = serde_json::to_vec(&transcript)?;
    let document: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(document.as_object().map(serde_json::Map::len), Some(1));
    assert!(document.get("messages").is_some());
    let mut restored: Transcript = serde_json::from_slice(&bytes)?;
    assert_byte_accounting(&restored);
    assert_eq!(restored.text_bytes, transcript.text_bytes);
    restored.append(" 🌱", false);
    restored.append("思", true);
    restored.replace_response("shorter", None);
    assert_byte_accounting(&restored);
    assert_eq!(restored.trim_message_visits, 0);

    let mut message_limit = Transcript::default();
    for _ in 0..512 {
        message_limit.push(Role::User, String::new());
    }
    assert!(serde_json::from_slice::<Transcript>(&serde_json::to_vec(&message_limit)?).is_ok());
    message_limit.push(Role::User, String::new());
    assert!(!message_limit.valid_saved());
    assert!(serde_json::from_slice::<Transcript>(&serde_json::to_vec(&message_limit)?).is_err());
    Ok(())
}
