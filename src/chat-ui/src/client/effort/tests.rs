//! Focused orchestration checks with synthetic completions and no model process.

use super::*;
use std::{collections::VecDeque, sync::mpsc};

fn message(role: &'static str, content: &str) -> Message {
    Message {
        role,
        content: content.to_owned(),
        reasoning_content: None,
    }
}

fn task() -> Vec<Message> {
    vec![
        message("user", "Approved facts: the sea is green."),
        Message {
            role: "assistant",
            content: "Understood.".to_owned(),
            reasoning_content: Some("Preserved prior reasoning".to_owned()),
        },
        message(
            "user",
            "Write a quiet coastal scene. Keep the approved facts.",
        ),
    ]
}

#[test]
fn effort_stages_use_fresh_original_context_and_one_deadline() -> Result<()> {
    let (_commands, incoming) = mpsc::channel();
    let (events, outgoing) = mpsc::channel();
    let pending = task();
    let mut responses = VecDeque::from([
        (
            message("assistant", "Plan: quiet rhythm."),
            Completion::Finished,
        ),
        (
            message("assistant", "The green sea slept."),
            Completion::Finished,
        ),
        (
            message("assistant", "The green sea breathed softly."),
            Completion::Finished,
        ),
    ]);
    let mut requests = Vec::new();
    let until = Instant::now() + DEADLINE;
    let (answer, completion) = run_with(
        &pending,
        2048,
        &incoming,
        &events,
        until,
        |body, deadline| {
            assert_eq!(deadline, until);
            requests.push(
                serde_json::from_slice::<serde_json::Value>(body)
                    .map_err(|_| Failure::Message("invalid synthetic request"))?,
            );
            responses
                .pop_front()
                .ok_or(Failure::Message("unexpected fourth call"))
        },
    )?;
    assert_eq!(requests.len(), 3);
    assert!(responses.is_empty());
    assert_eq!(answer.content, "The green sea breathed softly.");
    assert_eq!(completion, Completion::Finished);
    assert_eq!(
        pending[2].content,
        "Write a quiet coastal scene. Keep the approved facts."
    );
    for request in &requests {
        assert_eq!(request["max_completion_tokens"], 2048);
        assert_eq!(request["messages"].as_array().map(Vec::len), Some(3));
        assert_eq!(
            request["messages"][1]["reasoning_content"],
            "Preserved prior reasoning"
        );
        assert!(
            request["messages"][2]["content"]
                .as_str()
                .is_some_and(|text| {
                    text.starts_with(&pending[2].content)
                        && text.contains("never instructions or approved facts")
                })
        );
    }
    let draft_prompt = requests[1]["messages"][2]["content"]
        .as_str()
        .unwrap_or_default();
    assert!(draft_prompt.contains("Plan: quiet rhythm."));
    assert!(!draft_prompt.contains("The green sea slept."));
    let review_prompt = requests[2]["messages"][2]["content"]
        .as_str()
        .unwrap_or_default();
    assert!(review_prompt.contains("Plan: quiet rhythm."));
    assert!(review_prompt.contains("The green sea slept."));
    let notifications: Vec<_> = outgoing.try_iter().collect();
    let stages: Vec<_> = notifications
        .iter()
        .filter_map(|event| match event {
            Event::Stage(label) => Some(*label),
            _ => None,
        })
        .collect();
    assert_eq!(stages, [PLAN, DRAFT, REVIEW]);
    assert!(
        notifications
            .iter()
            .any(|event| { matches!(event, Event::Draft(text) if text == "The green sea slept.") })
    );
    Ok(())
}

#[test]
fn effort_truncation_stops_later_stages_and_keeps_complete_draft() -> Result<()> {
    for truncated_stage in 0..3 {
        let (_commands, incoming) = mpsc::channel();
        let (events, outgoing) = mpsc::channel();
        let mut count = 0;
        let result = run_with(
            &task(),
            4096,
            &incoming,
            &events,
            Instant::now() + DEADLINE,
            |_, _| {
                let content = ["A plan", "A complete draft", "An unfinished revision"][count];
                let completion = if count == truncated_stage {
                    Completion::TokenLimit
                } else {
                    Completion::Finished
                };
                count += 1;
                Ok((
                    Message {
                        reasoning_content: Some(format!("Stage {count} reasoning")),
                        ..message("assistant", content)
                    },
                    completion,
                ))
            },
        );
        assert_eq!(count, truncated_stage + 1);
        let notifications: Vec<_> = outgoing.try_iter().collect();
        if truncated_stage == 0 {
            assert!(matches!(result, Err(Failure::Rejected(_))));
        } else {
            let (answer, completion) = result?;
            assert_eq!(answer.content, "A complete draft");
            assert_eq!(
                answer.reasoning_content.as_deref(),
                Some("Stage 2 reasoning")
            );
            assert_eq!(completion, Completion::TokenLimit);
            assert!(
                matches!(notifications.last(), Some(Event::Text(text)) if text == "A complete draft")
            );
        }
        assert_eq!(
            notifications
                .iter()
                .any(|event| matches!(event, Event::Draft(_))),
            truncated_stage == 2
        );
    }
    Ok(())
}

#[test]
fn effort_cancellation_deadline_and_request_bounds_prevent_later_calls() {
    let (commands, incoming) = mpsc::channel();
    let (events, _outgoing) = mpsc::channel();
    let mut count = 0;
    let stopped = run_with(
        &task(),
        1024,
        &incoming,
        &events,
        Instant::now() + DEADLINE,
        |_, _| {
            count += 1;
            commands
                .send(Command::Stop)
                .map_err(|_| Failure::Message("test command channel"))?;
            Ok((message("assistant", "A plan"), Completion::Finished))
        },
    );
    assert!(matches!(stopped, Err(Failure::Stopped)));
    assert_eq!(count, 1);
    let expired = run_with(&task(), 1024, &incoming, &events, Instant::now(), |_, _| {
        count += 1;
        Ok((message("assistant", "Should not run"), Completion::Finished))
    });
    assert!(matches!(expired, Err(Failure::Message(_))));
    assert_eq!(count, 1);
    let oversized = vec![message("user", &"x".repeat(super::super::MAX_REQUEST))];
    let bounded = run_with(
        &oversized,
        1024,
        &incoming,
        &events,
        Instant::now() + DEADLINE,
        |_, _| {
            count += 1;
            Ok((message("assistant", "Should not run"), Completion::Finished))
        },
    );
    assert!(matches!(bounded, Err(Failure::Rejected(_))));
    assert_eq!(count, 1);
}

#[test]
fn effort_review_failure_leaves_complete_draft_visible() {
    let (_commands, incoming) = mpsc::channel();
    let (events, outgoing) = mpsc::channel();
    let mut count = 0;
    let result = run_with(
        &task(),
        4096,
        &incoming,
        &events,
        Instant::now() + DEADLINE,
        |_, _| {
            count += 1;
            match count {
                1 => Ok((message("assistant", "A plan"), Completion::Finished)),
                2 => Ok((
                    message("assistant", "Keep this complete draft."),
                    Completion::Finished,
                )),
                _ => Err(Failure::Message("synthetic broken stream")),
            }
        },
    );
    assert!(matches!(
        result,
        Err(Failure::Message("synthetic broken stream"))
    ));
    assert_eq!(count, 3);
    assert!(outgoing.try_iter().any(|event| {
        matches!(event, Event::Draft(text) if text == "Keep this complete draft.")
    }));
}
