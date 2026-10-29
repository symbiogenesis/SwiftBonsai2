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
    let scratch = (matcher.fallback.capacity(), matcher.starts.capacity());
    // A long repeated prefix exercises fallback without retaining source bytes
    // or allocating an offset pair for every folded byte.
    for start in 0..super::super::MAX_BYTES {
        assert!(!matcher.push(b'a', start, start + 1));
    }
    assert_eq!(matcher.ranges, Vec::<Range<usize>>::new());
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
