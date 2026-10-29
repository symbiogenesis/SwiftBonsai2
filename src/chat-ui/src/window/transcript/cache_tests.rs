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

#[test]
fn highlighted_layout_cache_keeps_constant_size_keys_during_streaming_and_navigation() {
    let mut transcript = Transcript::default();
    let highlights = 10_000;
    transcript.push(Role::User, "hit ".repeat(highlights));
    transcript.push(Role::Assistant, "A streamed answer".to_owned());
    transcript.search.query = "hit".to_owned();
    let mut chat = harness(transcript);
    chat.run();
    assert_eq!(chat.state().search.matches.len(), highlights);
    let visits = chat.state().messages[0].highlight_range_visits.get();
    assert_eq!(visits, highlights);

    for _ in 0..8 {
        chat.state_mut().append(" plain continuation", false);
        chat.run();
    }
    assert_eq!(chat.state().messages[0].body_layouts, 1);
    assert_eq!(
        chat.state().messages[0].highlight_range_visits.get(),
        visits
    );
    assert!(
        chat.state().messages[0]
            .cache
            .as_ref()
            .is_some_and(|cache| {
                cache.highlights.count == highlights && cache.selected.as_ref() == Some(&(0..3))
            })
    );

    // Selection changes redraw the affected highlight job once, then later
    // frames reuse its fixed-size identity without retaining a range vector.
    chat.state_mut().search.active = highlights - 1;
    chat.state_mut().search.revision += 1;
    chat.run();
    assert_eq!(
        chat.state().messages[0].highlight_range_visits.get(),
        visits * 2
    );
    chat.run();
    assert_eq!(
        chat.state().messages[0].highlight_range_visits.get(),
        visits * 2
    );
}

#[test]
fn highlight_identity_tracks_same_length_queries_replacement_and_result_eviction() {
    let mut transcript = Transcript::default();
    transcript.push(Role::User, "hit cat".to_owned());
    transcript.push(Role::Assistant, "hit cat".to_owned());
    transcript.search.query = "hit".to_owned();
    let mut chat = harness(transcript);
    chat.run();
    let query_revision = chat.state().search.query_revision;
    assert_eq!(layout_counts(chat.state()), vec![1, 1]);

    // Counts and byte lengths stay unchanged while every highlight moves.
    chat.state_mut().search.query = "cat".to_owned();
    chat.state_mut().search.invalidate();
    chat.run();
    assert_ne!(chat.state().search.query_revision, query_revision);
    assert_eq!(layout_counts(chat.state()), vec![2, 2]);
    assert!(
        chat.state()
            .search
            .matches
            .iter()
            .all(|found| found.range == (4..7))
    );
    assert!(
        chat.state().messages[0]
            .cache
            .as_ref()
            .is_some_and(|cache| { cache.selected.as_ref() == Some(&(4..7)) })
    );

    chat.state_mut().search.active = 1;
    chat.state_mut().search.revision += 1;
    chat.run();
    assert_eq!(layout_counts(chat.state()), vec![3, 3]);
    assert!(
        chat.state().messages[1]
            .cache
            .as_ref()
            .is_some_and(|cache| { cache.selected.as_ref() == Some(&(4..7)) })
    );
    chat.state_mut().replace_response("cat hit", None);
    chat.run();
    assert_eq!(layout_counts(chat.state()), vec![3, 4]);
    assert_eq!(chat.state().search.matches[1].range, 0..3);
    assert!(
        chat.state().messages[1]
            .cache
            .as_ref()
            .is_some_and(|cache| {
                cache.body_revision == chat.state().messages[1].body_revision
                    && cache.selected.as_ref() == Some(&(0..3))
            })
    );

    // Closing Find clears its query. Reopening the same query must acquire a
    // new identity even though the message bodies never changed.
    chat.state_mut().search.query.clear();
    chat.state_mut().search.invalidate();
    chat.run();
    assert_eq!(layout_counts(chat.state()), vec![4, 5]);
    assert!(chat.state().messages.iter().all(|message| {
        message
            .cache
            .as_ref()
            .is_some_and(|cache| cache.highlights.count == 0)
    }));
    chat.state_mut().search.query = "cat".to_owned();
    chat.state_mut().search.invalidate();
    chat.run();
    assert_eq!(layout_counts(chat.state()), vec![5, 6]);
    assert!(chat.state().messages.iter().all(|message| {
        message
            .cache
            .as_ref()
            .is_some_and(|cache| cache.highlights.count == 1)
    }));

    // Removing a saturated prefix exposes a previously hidden match without
    // changing that later message's body or the effective query.
    let mut transcript = Transcript::default();
    transcript.push(Role::User, "hit ".repeat(10_001));
    transcript.push(Role::Assistant, "hit".to_owned());
    transcript.search.query = "hit".to_owned();
    let mut chat = harness(transcript);
    chat.run();
    assert_eq!(chat.state().messages[1].highlight_range_visits.get(), 0);
    chat.state_mut().messages.remove(0);
    chat.state_mut().search.invalidate();
    chat.run();
    assert_eq!(chat.state().search.matches.len(), 1);
    assert_eq!(chat.state().messages[0].body_layouts, 2);
    assert_eq!(chat.state().messages[0].highlight_range_visits.get(), 1);
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
fn transcript_accounting_caps_empty_messages_before_bytes_require_eviction() {
    let mut transcript = Transcript::default();
    for _ in 0..MAX_MESSAGES {
        transcript.push(Role::User, String::new());
    }
    let retained = transcript.messages[1].id;
    transcript.push(Role::Assistant, String::new());
    assert_eq!(transcript.messages.len(), MAX_MESSAGES);
    assert_eq!(transcript.messages[0].id, retained);
    assert_eq!(transcript.trim_message_visits, 1);
    assert_eq!(transcript.trim_batches, 1);
    assert!(transcript.valid_saved());
    assert!(
        transcript
            .messages
            .last()
            .is_some_and(|message| message.pending)
    );
    for _ in 0..4096 {
        transcript.append("x", false);
    }
    assert_byte_accounting(&transcript);
    assert_eq!(transcript.trim_message_visits, 1);
    assert_eq!(transcript.trim_batches, 1);
}

#[test]
fn transcript_accounting_count_eviction_preserves_find_and_recovery()
-> Result<(), serde_json::Error> {
    let mut transcript = Transcript::default();
    for _ in 0..MAX_MESSAGES {
        transcript.push(Role::User, "hit".to_owned());
    }
    transcript.search.query = "hit".to_owned();
    transcript.search.refresh(&transcript.messages);
    transcript.search.active = 10;
    let selected = transcript.search.matches[10].clone();
    transcript.push(Role::Assistant, "new hit".to_owned());
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.messages.len(), MAX_MESSAGES);
    assert_eq!(transcript.search.matches.len(), MAX_MESSAGES);
    assert_eq!(transcript.search.active, 9);
    assert!(transcript.search.matches[transcript.search.active] == selected);
    assert!(transcript.search.matches.iter().all(|found| {
        transcript
            .messages
            .iter()
            .any(|message| message.id == found.message)
    }));
    assert_byte_accounting(&transcript);
    let restored: Transcript = serde_json::from_slice(&serde_json::to_vec(&transcript)?)?;
    assert_eq!(restored.messages.len(), MAX_MESSAGES);
    assert_eq!(restored.messages[0].body, "hit");
    assert_eq!(restored.messages[MAX_MESSAGES - 1].body, "new hit");
    assert_byte_accounting(&restored);
    Ok(())
}

#[test]
fn transcript_accounting_count_and_byte_eviction_remove_one_combined_prefix() {
    let mut transcript = Transcript::default();
    transcript.push(Role::User, "x".repeat(MAX_BYTES / 2));
    transcript.push(Role::Assistant, "y".repeat(MAX_BYTES / 2));
    for _ in 2..MAX_MESSAGES {
        transcript.push(Role::User, String::new());
    }
    let retained = transcript.messages[2].id;
    // The count bound requires one eviction, but freeing that message alone
    // leaves the byte budget exceeded. Drain the combined prefix only once.
    transcript.push(Role::Assistant, "z".repeat(MAX_BYTES / 2 + 1));
    assert_eq!(transcript.messages.len(), MAX_MESSAGES - 1);
    assert_eq!(transcript.messages[0].id, retained);
    assert_eq!(transcript.trim_message_visits, 2);
    assert_eq!(transcript.trim_batches, 1);
    assert!(transcript.valid_saved());
    assert_byte_accounting(&transcript);
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
    for _ in 0..MAX_MESSAGES {
        message_limit.push(Role::User, String::new());
    }
    assert!(serde_json::from_slice::<Transcript>(&serde_json::to_vec(&message_limit)?).is_ok());
    let mut saved = serde_json::to_value(&message_limit)?;
    if let Some(messages) = saved["messages"].as_array_mut() {
        messages.push(serde_json::json!({"role": "User", "body": "", "reasoning": ""}));
    }
    assert!(serde_json::from_value::<Transcript>(saved).is_err());
    Ok(())
}
