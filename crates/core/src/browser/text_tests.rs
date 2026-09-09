//! Truncation that reports the total. Included by `text.rs`.

use super::*;

#[test]
fn text_under_the_limit_is_returned_whole_and_not_marked_truncated() {
    let page = truncate_page_text("hello", 100);
    assert_eq!(page.text, "hello");
    assert_eq!(page.total_chars, 5);
    assert_eq!(page.returned_chars, 5);
    assert!(!page.truncated);
}

#[test]
fn text_exactly_at_the_limit_is_not_truncated() {
    // The off-by-one that would mark every full-but-complete read as partial.
    let page = truncate_page_text("abcde", 5);
    assert_eq!(page.text, "abcde");
    assert!(!page.truncated);
    assert_eq!(page.total_chars, page.returned_chars);
}

#[test]
fn truncation_reports_the_pages_total_not_the_slices() {
    // The governing rule. An agent told the total is 5 concludes it has read
    // the page and that what it is looking for is not on it.
    let page = truncate_page_text("abcdefghij", 5);
    assert_eq!(page.text, "abcde");
    assert_eq!(page.returned_chars, 5);
    assert_eq!(
        page.total_chars, 10,
        "the total is the page's, never the slice's"
    );
    assert!(page.truncated);
}

#[test]
fn total_chars_is_the_only_field_that_survives_the_cap_unchanged() {
    // Scaled up: a realistic page against a realistic cap.
    let text = "x".repeat(400_000);
    let page = truncate_page_text(&text, DEFAULT_TEXT_LIMIT);
    assert_eq!(page.returned_chars, DEFAULT_TEXT_LIMIT);
    assert_eq!(page.total_chars, 400_000);
    assert!(page.truncated);
}

#[test]
fn a_zero_limit_still_says_it_truncated() {
    // An empty answer that does not admit it was cut is indistinguishable from
    // an empty page — which is the exact confusion the flag exists to prevent.
    let page = truncate_page_text("something", 0);
    assert_eq!(page.text, "");
    assert_eq!(page.returned_chars, 0);
    assert_eq!(page.total_chars, 9);
    assert!(page.truncated);
}

#[test]
fn an_empty_page_is_not_truncated() {
    let page = truncate_page_text("", 0);
    assert!(!page.truncated);
    assert_eq!(page.total_chars, 0);
}

#[test]
fn counting_and_cutting_are_in_chars_so_a_multi_byte_character_is_never_split() {
    // A page in Japanese cut at a byte boundary would not be valid UTF-8, and
    // in Rust the naive version does not merely corrupt — it panics.
    let text = "日本語のテキスト";
    assert_eq!(text.chars().count(), 8);
    assert!(
        text.len() > 8,
        "the byte length differs, which is the point"
    );

    let page = truncate_page_text(text, 3);
    assert_eq!(page.text, "日本語");
    assert_eq!(page.returned_chars, 3);
    assert_eq!(page.total_chars, 8);
    assert!(page.truncated);
}

#[test]
fn a_limit_landing_inside_a_multi_byte_run_still_produces_valid_text() {
    // Every prefix length must be a valid string, so walk them all.
    let text = "aé日🙂b";
    for limit in 0..=text.chars().count() + 2 {
        let page = truncate_page_text(text, limit);
        assert!(page.text.chars().count() <= text.chars().count());
        assert_eq!(page.total_chars, 5);
        assert_eq!(page.returned_chars, page.text.chars().count());
    }
}

#[test]
fn whitespace_and_newlines_are_left_exactly_as_the_page_had_them() {
    // Normalising here would make a returned dump not match the page, and the
    // caller's own script is where any tidying belongs.
    let text = "a\n\n  b\tc\r\n";
    let page = truncate_page_text(text, 100);
    assert_eq!(page.text, text);
}

#[test]
fn returned_chars_always_matches_the_returned_text() {
    // The alarm for the two fields drifting apart, which is how a "complete"
    // read would come to be believed.
    for (text, limit) in [
        ("", 0usize),
        ("abc", 10),
        ("abc", 2),
        ("日本語", 1),
        ("x", 0),
    ] {
        let page = truncate_page_text(text, limit);
        assert_eq!(
            page.returned_chars,
            page.text.chars().count(),
            "{text:?}/{limit}"
        );
        assert_eq!(page.truncated, page.returned_chars < page.total_chars);
    }
}
