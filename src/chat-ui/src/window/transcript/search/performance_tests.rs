//! Deterministic work-count and bounded-result regressions for streamed find.
use super::*;
use crate::window::transcript::{Role, Transcript};

#[test]
fn full_find_refresh_reuses_entries_and_prunes_unvisited_saturated_suffix() {
    let mut transcript = Transcript::default();
    transcript.push(Role::User, "hit".to_owned());
    transcript.push(Role::Assistant, "a retained hit".to_owned());
    transcript.search.query = "hit".to_owned();
    transcript.search.refresh(&transcript.messages);
    let rebuilt = transcript.search.rebuilt_cache_entries;
    let scanned = transcript.search.scanned_bytes;
    for _ in 0..3 {
        transcript.search.invalidate();
        transcript.search.refresh(&transcript.messages);
    }
    assert_eq!(transcript.search.rebuilt_cache_entries, rebuilt);
    assert_eq!(transcript.search.scanned_bytes, scanned);
    assert_eq!(transcript.search.cached_messages.len(), 2);

    // A new query populates both entries before the first message is replaced
    // with enough results to saturate the prefix and skip the unchanged tail.
    transcript.search.query = "a".to_owned();
    transcript.search.invalidate();
    transcript.search.refresh(&transcript.messages);
    transcript.messages[0].body = "a ".repeat(MAX_MATCHES + 1);
    transcript.search.replaced(transcript.messages[0].id);
    transcript.search.refresh(&transcript.messages);
    assert!(transcript.search.limited);
    assert_eq!(transcript.search.cached_messages.len(), 1);
    let tail = transcript.messages[1].id;
    assert!(!transcript.search.cached_messages.contains_key(&tail));

    // Eviction must recreate the formerly unvisited tail and its continuation.
    transcript.messages.remove(0);
    transcript.search.invalidate();
    transcript.search.refresh(&transcript.messages);
    assert!(!transcript.search.limited);
    assert_eq!(transcript.search.matches.len(), 2);
    assert!(
        transcript.search.cached_messages[&tail]
            .continuation
            .is_some()
    );
}

#[test]
fn find_query_and_streamed_text_use_the_same_scalar_unicode_lowercase() {
    let mut transcript = Transcript::default();
    transcript.push(Role::User, "\u{39f}\u{3a3}".to_owned());
    transcript.push(Role::Assistant, "\u{39f}".to_owned());
    transcript.search.query = "\u{39f}\u{3a3}".to_owned();
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.matches.len(), 1);
    assert_eq!(transcript.search.matches[0].range, 0..4);
    let query_revision = transcript.search.query_revision;

    // Uppercase final sigma must match both an existing body and the same
    // word arriving in separate deltas. Contextual string lowercasing would
    // turn only the query into a final-sigma scalar, losing both matches.
    transcript.append("\u{3a3}", false);
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.matches.len(), 2);
    assert_eq!(transcript.search.matches[1].range, 0..4);
    transcript.search.query = "\u{3bf}\u{3c3}".to_owned();
    transcript.search.invalidate();
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.matches.len(), 2);
    assert_eq!(transcript.search.query_revision, query_revision);
}

#[test]
fn find_refresh_retains_current_scroll_targets_and_discards_evicted_or_closed_results() {
    let mut transcript = Transcript::default();
    for _ in 0..super::super::MAX_MESSAGES {
        transcript.push(Role::User, "hit".to_owned());
    }
    transcript.search.query = "hit".to_owned();
    transcript.search.refresh(&transcript.messages);
    transcript.search.active = 10;
    transcript.search.navigate(false);
    let target = transcript.search.target.clone();
    // Evicting an earlier message preserves both the active result and queued
    // scroll target, even though their index changes after the full refresh.
    transcript.push(Role::Assistant, "new hit".to_owned());
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.active, 10);
    assert!(transcript.search.target == target);
    assert!(transcript.search.matches.get(transcript.search.active) == target.as_ref());

    transcript.search.active = transcript.search.matches.len() - 1;
    transcript.search.navigate(false);
    assert_eq!(transcript.search.active, 0);
    transcript.push(Role::User, "another hit".to_owned());
    transcript.search.refresh(&transcript.messages);
    assert!(transcript.search.target.is_none());

    transcript.search.navigate(false);
    assert!(transcript.search.target.is_some());
    transcript.search.query.clear();
    transcript.search.invalidate();
    transcript.search.refresh(&transcript.messages);
    assert!(transcript.search.target.is_none());
}

#[test]
fn streamed_find_appends_only_new_results_without_rebuilding_completed_caches() {
    let mut transcript = Transcript::default();
    for _ in 0..200 {
        transcript.push(Role::User, "Élan ".repeat(20));
    }
    transcript.push(Role::Assistant, "Él".to_owned());
    transcript.search.query = "ÉLAN".to_owned();
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.matches.len(), 4000);
    transcript.search.active = 19;
    transcript.search.navigate(false);
    let selected = transcript.search.matches[transcript.search.active].clone();
    let target = transcript.search.target.clone();
    let rebuilt = transcript.search.rebuilt_cache_entries;
    let copied = transcript.search.copied_matches;
    let scanned = transcript.search.scanned_bytes;
    let revision = transcript.search.revision;
    let prefix = transcript.search.matches.clone();

    for _ in 0..128 {
        transcript.append("AN 🌱 Él", false);
        transcript.search.refresh(&transcript.messages);
    }
    assert_eq!(transcript.search.rebuilt_cache_entries, rebuilt);
    assert_eq!(transcript.search.copied_matches - copied, 128);
    assert_eq!(
        transcript.search.scanned_bytes - scanned,
        128 * "AN 🌱 Él".len()
    );
    assert_eq!(transcript.search.revision - revision, 128);
    assert!(transcript.search.matches[..prefix.len()] == prefix);
    assert!(transcript.search.matches[transcript.search.active] == selected);
    assert!(transcript.search.target == target);
    assert_eq!(transcript.search.cached_messages.len(), 201);
    let tail = &transcript.messages[200];
    for found in &transcript.search.matches[prefix.len()..] {
        assert_eq!(found.message, tail.id);
        assert_eq!(&tail.body[found.range.clone()], "ÉlAN");
    }

    // Replacement must rebuild even when byte length happens to be unchanged;
    // subsequent appends start a fresh continuation and retain the new prefix.
    let replacement = "x".repeat(tail.body.len());
    transcript.replace_response(&replacement, None);
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.matches.len(), 4000);
    assert!(transcript.search.rebuilt_cache_entries > rebuilt);
    let rebuilt = transcript.search.rebuilt_cache_entries;
    let copied = transcript.search.copied_matches;
    transcript.append(" élan", false);
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.matches.len(), 4001);
    assert_eq!(transcript.search.rebuilt_cache_entries, rebuilt);
    assert_eq!(transcript.search.copied_matches - copied, 1);

    transcript.messages.remove(0);
    transcript.search.invalidate();
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.matches.len(), 3981);
    assert_eq!(transcript.search.cached_messages.len(), 200);
    assert!(transcript.search.matches[transcript.search.active] == selected);
}

#[test]
fn streamed_find_exact_limit_preserves_results_when_lookahead_becomes_limited() {
    let mut transcript = Transcript::default();
    transcript.push(Role::User, "a ".repeat(MAX_MATCHES - 1));
    transcript.push(Role::Assistant, "a".to_owned());
    transcript.search.query = "a".to_owned();
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.matches.len(), MAX_MATCHES);
    assert!(!transcript.search.limited);
    transcript.search.navigate(true);
    let target = transcript.search.target.clone();
    let rebuilt = transcript.search.rebuilt_cache_entries;
    let copied = transcript.search.copied_matches;
    let scanned = transcript.search.scanned_bytes;
    let revision = transcript.search.revision;

    transcript.append(" A", false);
    transcript.search.refresh(&transcript.messages);
    assert!(transcript.search.limited);
    assert_eq!(transcript.search.matches.len(), MAX_MATCHES);
    assert_eq!(transcript.search.active, MAX_MATCHES - 1);
    assert!(transcript.search.target == target);
    assert_eq!(transcript.search.revision, revision);
    assert_eq!(transcript.search.rebuilt_cache_entries, rebuilt);
    assert_eq!(transcript.search.copied_matches, copied);
    assert_eq!(transcript.search.scanned_bytes - scanned, 2);

    transcript.append(" a", false);
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.scanned_bytes - scanned, 2);
    assert_eq!(transcript.search.rebuilt_cache_entries, rebuilt);
    assert_eq!(transcript.search.copied_matches, copied);
    assert!(transcript.search.target == target);
    transcript.messages.remove(0);
    transcript.search.invalidate();
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.matches.len(), 3);
    assert!(!transcript.search.limited);
}

#[test]
fn streamed_find_saturated_prefix_skips_tail_cache_and_result_work() {
    let mut transcript = Transcript::default();
    transcript.push(Role::User, "a ".repeat(MAX_MATCHES + 1));
    transcript.push(Role::Assistant, "no matches yet".to_owned());
    transcript.search.query = "a".to_owned();
    transcript.search.refresh(&transcript.messages);
    assert!(transcript.search.limited);
    assert_eq!(transcript.search.matches.len(), MAX_MATCHES);
    let scanned = transcript.search.scanned_bytes;
    let rebuilt = transcript.search.rebuilt_cache_entries;
    let copied = transcript.search.copied_matches;
    let revision = transcript.search.revision;
    for _ in 0..128 {
        transcript.append(" a", false);
        transcript.search.refresh(&transcript.messages);
    }
    assert_eq!(transcript.search.scanned_bytes, scanned);
    assert_eq!(transcript.search.rebuilt_cache_entries, rebuilt);
    assert_eq!(transcript.search.copied_matches, copied);
    assert_eq!(transcript.search.revision, revision);
    assert!(transcript.search.limited);

    // Eviction exposes the appended tail, which was deliberately not scanned
    // while the unchanged prefix already supplied the result lookahead.
    transcript.messages.remove(0);
    transcript.search.invalidate();
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.matches.len(), 129);
    assert!(!transcript.search.limited);
    assert_eq!(transcript.search.cached_messages.len(), 1);
    let rebuilt = transcript.search.rebuilt_cache_entries;
    let copied = transcript.search.copied_matches;
    transcript.append(" A", false);
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.matches.len(), 130);
    assert_eq!(transcript.search.rebuilt_cache_entries, rebuilt);
    assert_eq!(transcript.search.copied_matches - copied, 1);
}

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
fn streamed_find_scans_only_appended_bytes_and_keeps_one_continuation() {
    let mut transcript = Transcript::default();
    for _ in 0..100 {
        transcript.push(Role::User, "Completed answer 🌱".to_owned());
    }
    transcript.push(Role::Assistant, "a".repeat(16 * 1024));
    let mut search = Search {
        query: "aaaaaz".to_owned(),
        ..Search::default()
    };
    search.invalidate();
    search.refresh(&transcript.messages);
    let initial_bytes = search.scanned_bytes;
    for _ in 0..4096 {
        transcript.append("a", false);
        search.invalidate();
        search.refresh(&transcript.messages);
    }
    transcript.append("z🌱", false);
    search.invalidate();
    search.refresh(&transcript.messages);
    assert_eq!(search.scanned_bytes - initial_bytes, 4096 + "z🌱".len());
    assert_eq!(search.matches.len(), 1);
    let tail = &transcript.messages[100];
    assert_eq!(&tail.body[search.matches[0].range.clone()], "aaaaaz");
    assert_eq!(
        search
            .cached_messages
            .values()
            .filter(|cached| cached.continuation.is_some())
            .count(),
        1
    );
    let continuation = search.cached_messages[&tail.id].continuation.as_ref();
    assert!(continuation.is_some_and(|matcher| {
        matcher.starts.len() == search.query.len() && matcher.fallback.len() == search.query.len()
    }));
}

#[test]
fn streamed_find_replacements_rebuild_equal_shorter_and_longer_bodies() {
    let mut transcript = Transcript::default();
    transcript.push(Role::User, "hit".to_owned());
    transcript.push(Role::Assistant, "miss".to_owned());
    transcript.search.query = "hit".to_owned();
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.matches.len(), 1);
    for (body, count) in [("HIT!", 2), ("no", 1), ("HIT! HIT!", 3)] {
        transcript.replace_response(body, None);
        transcript.search.refresh(&transcript.messages);
        assert_eq!(transcript.search.matches.len(), count);
    }
    let scanned = transcript.search.scanned_bytes;
    transcript.append(" HIT!", false);
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.matches.len(), 4);
    assert_eq!(transcript.search.scanned_bytes - scanned, " HIT!".len());
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

// Retain the previous whole-buffer implementation as a small-input oracle for
// exact folded-domain nonoverlap and original UTF-8 highlight boundaries.
fn reference_ranges(body: &str, query: &str) -> Vec<Range<usize>> {
    let mut folded = String::new();
    let mut offsets = Vec::new();
    for (start, character) in body.char_indices() {
        let end = start + character.len_utf8();
        for lower in character.to_lowercase() {
            offsets.extend(std::iter::repeat_n((start, end), lower.len_utf8()));
            folded.push(lower);
        }
    }
    let mut ranges = Vec::new();
    for (start, found) in folded.match_indices(query) {
        let range = offsets[start].0..offsets[start + found.len() - 1].1;
        if ranges
            .last()
            .is_none_or(|previous: &Range<usize>| previous.end <= range.start)
        {
            ranges.push(range);
            if ranges.len() > MAX_MATCHES {
                break;
            }
        }
    }
    ranges
}

#[test]
fn streamed_find_matches_folded_reference_at_every_unicode_append_boundary() {
    let alphabet = ["a", "A", "İ", "\u{307}", "Σ", "ς", "🌱"];
    let queries = alphabet
        .iter()
        .flat_map(|first| {
            alphabet
                .iter()
                .map(move |second| format!("{first}{second}").to_lowercase())
        })
        .chain(alphabet.iter().map(|query| query.to_lowercase()))
        .collect::<Vec<_>>();
    let bodies = alphabet
        .into_iter()
        .flat_map(move |first| {
            alphabet.into_iter().flat_map(move |second| {
                alphabet
                    .into_iter()
                    .map(move |third| format!("{first}{second}{third}"))
            })
        })
        .chain(["aaaabaaaabaaaaa".to_owned(), "ababaabababa".to_owned()]);
    for body in bodies {
        for query in &queries {
            let expected = reference_ranges(&body, query);
            for split in body
                .char_indices()
                .map(|(index, _)| index)
                .chain([body.len()])
            {
                let (mut cached, _) = CachedMatches::scan(&body[..split], query, true);
                if let Some(matcher) = &mut cached.continuation {
                    matcher.extend(&body[split..], split, &mut cached.ranges);
                }
                assert_eq!(
                    cached.ranges, expected,
                    "body={body:?}, query={query:?}, split={split}"
                );
            }
        }
    }
}

#[test]
fn streamed_find_stops_scanning_after_result_lookahead_saturates() {
    let mut transcript = Transcript::default();
    transcript.push(Role::Assistant, "İ ".repeat(MAX_MATCHES + 1));
    transcript.search.query = "i".to_owned();
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.matches.len(), MAX_MATCHES);
    assert!(transcript.search.limited);
    let scanned = transcript.search.scanned_bytes;
    transcript.append(&"İ ".repeat(4096), false);
    transcript.search.refresh(&transcript.messages);
    assert_eq!(transcript.search.scanned_bytes, scanned);
    assert_eq!(transcript.search.matches.len(), MAX_MATCHES);
    assert!(transcript.search.limited);
}

#[test]
fn bounded_find_matches_folded_reference_across_expansion_and_prefix_boundaries() {
    let alphabet = ["a", "A", "İ", "\u{307}", "Σ", "ς", "🌱"];
    let queries = alphabet
        .iter()
        .flat_map(|first| {
            alphabet
                .iter()
                .map(move |second| format!("{first}{second}").to_lowercase())
        })
        .chain(alphabet.iter().map(|query| query.to_lowercase()))
        .collect::<Vec<_>>();
    for first in alphabet {
        for second in alphabet {
            for third in alphabet {
                let body = format!("{first}{second}{third}");
                for query in &queries {
                    assert_eq!(
                        find_ranges(&body, query),
                        reference_ranges(&body, query),
                        "body={body:?}, query={query:?}"
                    );
                }
            }
        }
    }
    for body in [
        "aaaabaaaabaaaaa",
        "ababaabababa",
        "ASCII ascii",
        "ΣΟΣ σος σοσ",
        "Élan İ 界 🌱 éLAN",
    ] {
        for query in ["a", "aa", "aab", "ababa", "ASCII", "ΣΟΣ", "ÉLAN", "🌱"] {
            let query = query.to_lowercase();
            assert_eq!(find_ranges(body, &query), reference_ranges(body, &query));
        }
    }
    assert_eq!(find_ranges("İ", ""), Vec::<Range<usize>>::new());
    assert_eq!(find_ranges("İİ", "i\u{307}"), vec![0..2, 2..4]);
    assert_eq!(find_ranges("aaa", "aa"), vec![0..2]);
}

#[test]
fn bounded_find_scratch_depends_on_query_bytes_and_retains_the_result_lookahead() {
    let query = "aaaaaaaaz";
    let mut matcher = FoldedMatcher::new(query.as_bytes());
    let mut ranges = Vec::new();
    let scratch = (matcher.fallback.capacity(), matcher.starts.capacity());
    // A long repeated prefix exercises fallback without retaining source bytes
    // or allocating an offset pair for every folded byte.
    for start in 0..super::super::MAX_BYTES {
        assert!(!matcher.push(b'a', start, start + 1, &mut ranges));
    }
    assert_eq!(ranges, Vec::<Range<usize>>::new());
    assert_eq!(
        (matcher.fallback.len(), matcher.starts.len()),
        (query.len(), query.len())
    );
    assert_eq!(
        (matcher.fallback.capacity(), matcher.starts.capacity()),
        scratch
    );

    let body = "İ ".repeat(MAX_MATCHES + 10);
    let ranges = find_ranges(&body, "i");
    assert_eq!(ranges.len(), MAX_MATCHES + 1);
    assert_eq!(ranges.last(), Some(&(MAX_MATCHES * 3..MAX_MATCHES * 3 + 2)));
    assert!(ranges.iter().all(|range| &body[range.clone()] == "İ"));
}
