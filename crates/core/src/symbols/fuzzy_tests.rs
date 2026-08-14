//! Tests for scoring a query against a candidate name.
//! Included by `fuzzy.rs` under `#[cfg(test)]`.

use std::time::{Duration, Instant};

use super::{rank, score, Match};

/// Reconstructs the matched text from a match's positions, treating them as
/// char indices. Every positional assertion goes through this, because reading
/// the characters back out is the only check that distinguishes a correct char
/// index from a byte offset that happens to look plausible.
fn matched_text(candidate: &str, m: &Match) -> String {
    let chars: Vec<char> = candidate.chars().collect();
    m.positions.iter().map(|&i| chars[i]).collect()
}

fn s(query: &str, candidate: &str) -> i32 {
    score(query, candidate)
        .unwrap_or_else(|| panic!("expected {query:?} to match {candidate:?}"))
        .score
}

#[test]
fn an_acronym_matches_camel_humps() {
    let tight = score("GDS", "GitDiffService").expect("GDS should match GitDiffService");
    let loose = score("GDS", "GetDataSourceFactoryHelper")
        .expect("GDS should match GetDataSourceFactoryHelper");

    assert_eq!(matched_text("GitDiffService", &tight), "GDS");
    assert_eq!(matched_text("GetDataSourceFactoryHelper", &loose), "GDS");

    // Both land entirely on humps; the shorter, tighter name has to win.
    assert!(
        tight.score > loose.score,
        "GitDiffService ({}) should outrank GetDataSourceFactoryHelper ({})",
        tight.score,
        loose.score
    );
}

#[test]
fn exact_beats_prefix_beats_word_boundary_beats_scattered() {
    let exact = s("run", "run");
    let prefix = s("run", "runTests");
    let boundary = s("run", "task_run");
    let scattered = s("run", "arbitraryunknown");

    assert!(
        exact > prefix && prefix > boundary && boundary > scattered,
        "expected exact({exact}) > prefix({prefix}) > boundary({boundary}) > scattered({scattered})"
    );
}

#[test]
fn consecutive_beats_scattered_at_equal_candidate_length() {
    let consecutive = s("abc", "abcdefgh");
    let scattered = s("abc", "axbxcxdx");
    assert_eq!("abcdefgh".len(), "axbxcxdx".len());
    assert!(
        consecutive > scattered,
        "expected consecutive({consecutive}) > scattered({scattered})"
    );
}

#[test]
fn a_lowercase_query_matches_a_mixed_case_candidate() {
    let m = score("gds", "GitDiffService").expect("a lowercase query is case-insensitive");
    assert_eq!(matched_text("GitDiffService", &m), "GDS");
}

#[test]
fn an_uppercase_query_does_not_match_a_lowercase_candidate() {
    assert!(score("GDS", "gitdiffservice").is_none());
    // ...and the same query still matches when the case is really there.
    assert!(score("GDS", "GitDiffService").is_some());
}

#[test]
fn an_empty_query_matches_everything_with_score_zero() {
    let m = score("", "anything at all").expect("an empty query matches");
    assert_eq!(m.score, 0);
    assert!(m.positions.is_empty());

    let m = score("", "").expect("an empty query matches an empty candidate");
    assert_eq!(m.score, 0);
}

#[test]
fn a_non_subsequence_does_not_match() {
    assert!(score("zzz", "GitDiffService").is_none());
    // Right letters, wrong order.
    assert!(score("sdg", "GitDiffService").is_none());
    // Query longer than the candidate.
    assert!(score("handler", "hand").is_none());
    assert!(score("x", "").is_none());
}

#[test]
fn positions_are_strictly_increasing_and_index_the_matched_chars() {
    let candidate = "src/git/attribution.rs";
    let m = score("gitattr", candidate).expect("gitattr should match");

    assert!(
        m.positions.windows(2).all(|w| w[0] < w[1]),
        "positions must be strictly increasing: {:?}",
        m.positions
    );
    assert_eq!(m.positions.len(), "gitattr".chars().count());
    assert_eq!(matched_text(candidate, &m).to_lowercase(), "gitattr");
}

#[test]
fn positions_are_char_indices_not_byte_indices() {
    let candidate = "café_handler";
    let m = score("céh", candidate).expect("céh should match café_handler");

    // c=0, a=1, f=2, é=3, _=4, h=5. As byte offsets 'h' would be 6, because
    // é occupies two bytes.
    assert_eq!(m.positions, vec![0, 3, 5]);
    assert_eq!(matched_text(candidate, &m), "céh");

    // A candidate with no ASCII at all must behave the same way.
    let cyrillic = "МойОбработчик";
    let m = score("МО", cyrillic).expect("МО should match МойОбработчик");
    assert_eq!(matched_text(cyrillic, &m), "МО");
    let chars = cyrillic.chars().count();
    assert!(m.positions.iter().all(|&p| p < chars));
}

#[test]
fn underscore_dot_slash_and_backslash_all_start_a_word() {
    let boundary: Vec<i32> = ["a_handler", "a.handler", "a/handler", "a\\handler"]
        .iter()
        .map(|c| s("h", c))
        .collect();
    let plain = s("h", "aahandler");

    assert!(
        boundary.iter().all(|&b| b == boundary[0]),
        "every separator should be worth the same: {boundary:?}"
    );
    assert!(
        boundary[0] > plain,
        "a word-boundary hit ({}) should beat a mid-word one ({plain})",
        boundary[0]
    );
}

#[test]
fn a_digit_to_letter_change_starts_a_word() {
    assert!(s("h", "a1handler") > s("h", "aahandler"));
}

#[test]
fn a_shorter_candidate_wins_an_otherwise_equal_tie() {
    let short = s("foo", "foobar");
    let long = s("foo", "foobarbazqux");
    assert!(short > long, "expected short({short}) > long({long})");
}

#[test]
fn rank_honours_the_limit_and_orders_best_first() {
    let items = vec![
        "arbitraryunknown",
        "task_run",
        "runTests",
        "run",
        "unrelated",
    ];
    let out = rank(items.into_iter(), |s| *s, "run", 3);

    assert_eq!(out.len(), 3);
    let names: Vec<&str> = out.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, vec!["run", "runTests", "task_run"]);
    assert!(out[0].1.score > out[1].1.score);
}

#[test]
fn rank_with_a_zero_limit_returns_nothing() {
    let out = rank(["run", "runTests"].into_iter(), |s| *s, "run", 0);
    assert!(out.is_empty());
}

#[test]
fn ranking_is_deterministic_across_repeated_runs() {
    // Deliberately full of ties: same length, same shape, same score.
    let items = vec![
        "run_alpha",
        "run_gamma",
        "run_beta",
        "run_delta",
        "runAlpha",
        "runGamma",
        "task_run",
        "trun_run",
    ];

    let first = rank(items.clone().into_iter(), |s| *s, "run", 5);
    let first_names: Vec<&str> = first.iter().map(|(n, _)| *n).collect();

    for pass in 0..10 {
        let again = rank(items.clone().into_iter(), |s| *s, "run", 5);
        let names: Vec<&str> = again.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names, first_names,
            "pass {pass} disagreed with the first run"
        );
    }
}

#[test]
fn an_absurdly_long_candidate_does_not_panic_or_hang() {
    let mut candidate = "x".repeat(100_000);
    candidate.push_str("Handler");

    let started = Instant::now();
    let m = score("handler", &candidate).expect("the tail still matches");
    let miss = score("handlerz", &candidate);
    let elapsed = started.elapsed();

    assert_eq!(matched_text(&candidate, &m).to_lowercase(), "handler");
    assert!(miss.is_none());
    assert!(
        elapsed < Duration::from_secs(2),
        "a 100k-char candidate took {elapsed:?}"
    );
}
