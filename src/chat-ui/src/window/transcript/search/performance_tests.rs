//! Deterministic work-count and bounded-result regressions for streamed find.
use super::*;
use crate::window::transcript::{Role, Transcript};

#[test]
fn streamed_find_reuses_completed_messages_and_preserves_unicode_matches() {
    let mut transcript = Transcript::default();
    transcript.push(Role::User, "Élan İ éLAN".to_owned());
    transcript.push(Role::Assistant, "Élan".to_owned());
    let mut search = Search {
        query: "ÉLAN".to_owned(),
        ..Search::default()
    };
    search.invalidate();
    search.refresh(&transcript.messages);
    assert_eq!(search.scanned_messages, 2);
    assert_eq!(search.matches.len(), 3);
    search.navigate(true);
    assert_eq!(search.active, 2);

    transcript.append(" éLAN", false);
    search.invalidate();
    search.refresh(&transcript.messages);
    assert_eq!(search.scanned_messages, 3);
    assert_eq!(search.matches.len(), 4);
    assert_eq!(search.active, 2);
    for found in &search.matches {
        let message = &transcript.messages[usize::from(found.message != transcript.messages[0].id)];
        assert_eq!(message.body[found.range.clone()].to_lowercase(), "élan");
    }

    search.query = "i".to_owned();
    search.invalidate();
    search.refresh(&transcript.messages);
    assert_eq!(search.scanned_messages, 5);
    assert_eq!(search.matches.len(), 1);
    assert_eq!(
        &transcript.messages[0].body[search.matches[0].range.clone()],
        "İ"
    );
    transcript.messages.remove(0);
    search.invalidate();
    search.refresh(&transcript.messages);
    assert!(search.matches.is_empty());
    assert_eq!(search.cached_messages.len(), 1);
    assert_eq!(search.scanned_messages, 5);
}

#[test]
fn cached_find_keeps_the_global_limit_and_discards_closed_query_data() {
    let mut transcript = Transcript::default();
    transcript.push(Role::User, "a ".repeat(MAX_MATCHES - 1));
    transcript.push(Role::Assistant, "a a a".to_owned());
    let mut search = Search {
        query: "a".to_owned(),
        ..Search::default()
    };
    search.invalidate();
    search.refresh(&transcript.messages);
    assert_eq!(search.matches.len(), MAX_MATCHES);
    assert!(search.limited);
    transcript.append(" a", false);
    search.invalidate();
    search.refresh(&transcript.messages);
    assert_eq!(search.matches.len(), MAX_MATCHES);
    assert!(search.limited);
    assert_eq!(search.scanned_messages, 3);
    search.query.clear();
    search.invalidate();
    search.refresh(&transcript.messages);
    assert!(search.matches.is_empty());
    assert!(search.cached_messages.is_empty());
    assert!(!search.limited);
    assert_eq!(find_ranges("ASCII ascii", "ascii"), vec![0..5, 6..11]);
}
