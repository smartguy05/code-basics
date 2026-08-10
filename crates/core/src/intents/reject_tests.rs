//! Tests for rejecting a change — the marker comment left where code was.
//! Included by `reject.rs` under `#[cfg(test)]`.

use std::time::{Duration, SystemTime};

use super::*;
use crate::git::patch::{DiffLine, FileDiff, Hunk, LineOrigin};

const DATE: &str = "2026-08-10";

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

// ---------------------------------------------------------------------------
// Comment syntax
// ---------------------------------------------------------------------------

#[test]
fn the_languages_this_repository_is_written_in_can_all_be_marked() {
    for path in [
        "crates/core/src/lib.rs",
        "src/App.tsx",
        "src/ipc/api.ts",
        "scripts/generate-index.mjs",
        "sidecar/inspector/Program.cs",
    ] {
        assert_eq!(comment_prefix(path), Some("//"), "{path}");
    }
}

#[test]
fn hash_and_dash_comment_families_are_recognised() {
    for path in [
        "setup.py",
        "run.sh",
        "config.yml",
        "Cargo.toml",
        "deploy.ps1",
    ] {
        assert_eq!(comment_prefix(path), Some("#"), "{path}");
    }
    for path in ["migrations/001.sql", "plugin.lua"] {
        assert_eq!(comment_prefix(path), Some("--"), "{path}");
    }
}

/// A block comment could be inserted into these, but a stray `/*` that fails
/// to close silently swallows the rest of the file. Abstaining costs a marker;
/// guessing costs the file.
#[test]
fn a_file_with_no_line_comment_syntax_is_not_marked() {
    for path in [
        "package.json",
        "src/styles.css",
        "README.md",
        "icon.png",
        "LICENSE",
    ] {
        assert_eq!(comment_prefix(path), None, "{path}");
    }
}

/// Windows reports the case the filesystem stored, not the case anyone typed.
#[test]
fn extensions_match_whatever_case_the_path_arrived_in() {
    assert_eq!(comment_prefix("SRC/MAIN.RS"), Some("//"));
    assert_eq!(comment_prefix("Setup.PY"), Some("#"));
}

#[test]
fn files_named_by_convention_rather_than_extension_are_recognised() {
    assert_eq!(comment_prefix("Dockerfile"), Some("#"));
    assert_eq!(comment_prefix("docker/Dockerfile"), Some("#"));
    assert_eq!(comment_prefix("Makefile"), Some("#"));
    assert_eq!(comment_prefix(".gitignore"), Some("#"));
}

// ---------------------------------------------------------------------------
// The reason text
// ---------------------------------------------------------------------------

/// The reason arrives from a text box, so it can contain newlines. Every line
/// of the marker has to start with the comment prefix, and a raw newline would
/// put live code in the middle of the block.
#[test]
fn a_multi_line_reason_is_flattened_to_one_line() {
    let cleaned = sanitise_reason("first line\r\nsecond\tline\n\n  third  ");

    assert!(!cleaned.contains('\n'));
    assert!(!cleaned.contains('\r'));
    assert!(!cleaned.contains('\t'));
    assert_eq!(cleaned, "first line second line third");
}

#[test]
fn a_runaway_reason_is_truncated_rather_than_pasted_whole() {
    let cleaned = sanitise_reason(&"word ".repeat(500));

    assert!(cleaned.len() <= MAX_REASON, "got {} chars", cleaned.len());
    assert!(cleaned.ends_with('…'), "truncation should be visible");
}

#[test]
fn a_reason_of_only_whitespace_comes_back_empty() {
    assert_eq!(sanitise_reason("   \n\t  "), "");
}

// ---------------------------------------------------------------------------
// The marker block
// ---------------------------------------------------------------------------

#[test]
fn the_block_states_the_token_the_reason_and_what_to_do_next() {
    let block = marker_block("//", "", "regex mis-detects a column named limit", DATE);
    let text = block.join("\n");

    assert!(text.contains(MARKER));
    assert!(text.contains(DATE));
    assert!(text.contains("regex mis-detects a column named limit"));
    // The agent has to learn both halves of the contract: fix it, and remove
    // the marker afterwards. A marker nobody deletes blocks every commit.
    assert!(text.contains("delete"));
}

#[test]
fn every_line_carries_the_prefix_and_the_anchors_indentation() {
    let block = marker_block(
        "#",
        "    ",
        "a reason long enough to need wrapping across lines",
        DATE,
    );

    assert!(block.len() >= 2);
    for line in &block {
        assert!(line.starts_with("    # "), "got {line:?}");
    }
}

#[test]
fn a_long_reason_wraps_instead_of_running_off_the_screen() {
    let reason = "the generated SQL used a regular expression to find an existing \
                  LIMIT clause, which matches a column that happens to be called limit";
    let block = marker_block("//", "  ", reason, DATE);

    for line in &block {
        assert!(
            line.chars().count() <= 2 + 3 + WRAP,
            "line too long ({}): {line:?}",
            line.chars().count()
        );
    }

    // Wrapping must not lose any of it.
    let joined = block.join(" ");
    for word in reason.split_whitespace() {
        assert!(joined.contains(word), "lost {word:?}");
    }
}

/// A word longer than the wrap budget cannot be broken without corrupting it —
/// an over-long line is the lesser evil.
#[test]
fn an_unbreakable_word_is_left_intact_on_its_own_line() {
    let long = "a".repeat(WRAP * 2);
    let block = marker_block("//", "", &long, DATE);

    assert!(block.iter().any(|l| l.contains(&long)));
}

#[test]
fn a_marker_is_recognisable_once_written() {
    let block = marker_block("//", "\t", "why", DATE);
    assert!(block.iter().any(|l| is_marker_line(l)));
    assert!(!is_marker_line("    // an ordinary comment"));
}

// ---------------------------------------------------------------------------
// Anchors
// ---------------------------------------------------------------------------

fn line(index: u32, origin: LineOrigin, content: &str, old: Option<u32>) -> DiffLine {
    DiffLine {
        index,
        origin,
        content: content.to_string(),
        old_lineno: old,
        new_lineno: None,
        no_newline: false,
    }
}

/// Two hunks: two lines added after baseline line 10, and baseline line 50
/// replaced. Enough to tell "where in the baseline" from "where in the file
/// once only some of this is reverted".
fn two_hunks() -> FileDiff {
    FileDiff {
        path: "src/lib.rs".into(),
        old_path: None,
        is_binary: false,
        hunks: vec![
            Hunk {
                old_start: 10,
                old_lines: 0,
                new_start: 11,
                new_lines: 2,
                header: String::new(),
                lines: vec![
                    line(0, LineOrigin::Addition, "    let extra = 1;", None),
                    line(1, LineOrigin::Addition, "    let more = 2;", None),
                ],
            },
            Hunk {
                old_start: 50,
                old_lines: 1,
                new_start: 52,
                new_lines: 1,
                header: String::new(),
                lines: vec![
                    line(2, LineOrigin::Deletion, "    original();", Some(50)),
                    line(3, LineOrigin::Addition, "    replacement();", None),
                ],
            },
        ],
    }
}

#[test]
fn an_addition_only_hunk_anchors_just_after_the_line_it_followed() {
    // Nothing was removed, so the marker belongs where the additions were:
    // immediately after baseline line 10.
    assert_eq!(anchors(&two_hunks(), &[0]), vec![11]);
}

#[test]
fn a_replacement_hunk_anchors_at_the_line_it_replaced() {
    assert_eq!(anchors(&two_hunks(), &[1]), vec![52]);
}

/// The anchor has to be a line number in the file that will exist *after* the
/// revert, not in the baseline. An earlier hunk that is being kept still has
/// its added lines in the file, pushing everything below it down.
#[test]
fn a_kept_earlier_hunk_shifts_the_anchor_by_its_net_line_change() {
    let diff = two_hunks();

    // Hunk 0 kept (+2 lines), hunk 1 rejected: baseline 50 sits at 52.
    assert_eq!(anchors(&diff, &[1]), vec![52]);

    // Both rejected: the file returns to the baseline, so no shift at all.
    assert_eq!(anchors(&diff, &[0, 1]), vec![11, 50]);
}

#[test]
fn an_out_of_range_hunk_index_is_ignored_rather_than_panicking() {
    assert_eq!(anchors(&two_hunks(), &[99]), Vec::<u32>::new());
}

// ---------------------------------------------------------------------------
// Insertion
// ---------------------------------------------------------------------------

const FILE: &str = "fn main() {\n    one();\n    two();\n    three();\n}\n";

#[test]
fn a_marker_is_inserted_before_its_anchor_line() {
    let out = insert_markers(FILE, &[3], "//", "two was wrong", DATE);
    let lines: Vec<&str> = out.lines().collect();

    let marker = lines.iter().position(|l| is_marker_line(l)).unwrap();
    let two = lines.iter().position(|l| l.contains("two();")).unwrap();
    assert!(marker < two, "marker must precede the code it explains");
    assert!(out.contains("    one();\n"));
    assert!(out.contains("    three();\n"));
}

#[test]
fn the_marker_copies_the_indentation_of_the_line_it_precedes() {
    let out = insert_markers(FILE, &[3], "//", "why", DATE);

    assert!(
        out.lines().any(|l| l.starts_with("    // ")),
        "expected four-space indent, got:\n{out}"
    );
}

/// Inserting top-down would invalidate every later anchor as soon as the first
/// block was added.
#[test]
fn several_anchors_all_land_on_the_right_lines() {
    let out = insert_markers(FILE, &[2, 4], "//", "why", DATE);
    let lines: Vec<&str> = out.lines().collect();

    let markers: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| is_marker_line(l))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(markers.len(), 2);

    let one = lines.iter().position(|l| l.contains("one();")).unwrap();
    let three = lines.iter().position(|l| l.contains("three();")).unwrap();
    assert!(markers[0] < one, "first marker should precede one()");
    assert!(markers[1] < three, "second marker should precede three()");
    assert!(markers[0] < markers[1]);
}

/// Rejecting twice in the same place happens — the agent tries again and gets
/// it wrong again. The reason should update without the block stacking up.
#[test]
fn re_rejecting_the_same_place_replaces_the_marker_rather_than_stacking() {
    let once = insert_markers(FILE, &[3], "//", "first reason", DATE);
    let twice = insert_markers(&once, &[3], "//", "second reason", DATE);

    assert_eq!(
        twice.lines().filter(|l| is_marker_line(l)).count(),
        1,
        "got:\n{twice}"
    );
    assert!(twice.contains("second reason"));
    assert!(!twice.contains("first reason"));
    assert!(twice.contains("    two();"));
}

#[test]
fn an_anchor_past_the_end_of_the_file_appends_at_the_end() {
    let out = insert_markers(FILE, &[999], "//", "why", DATE);

    assert!(out.lines().any(is_marker_line));
    assert!(out.starts_with("fn main() {"));
}

#[test]
fn an_empty_file_still_takes_a_marker() {
    let out = insert_markers("", &[1], "//", "the whole file was wrong", DATE);
    assert!(out.lines().any(is_marker_line));
}

#[test]
fn a_file_without_a_trailing_newline_does_not_gain_one_silently() {
    let out = insert_markers("one\ntwo", &[1], "//", "why", DATE);
    assert!(!out.ends_with('\n'), "got {out:?}");
}

#[test]
fn no_anchors_leaves_the_file_byte_for_byte_unchanged() {
    assert_eq!(insert_markers(FILE, &[], "//", "why", DATE), FILE);
}

// ---------------------------------------------------------------------------
// The date
// ---------------------------------------------------------------------------

#[test]
fn dates_are_rendered_as_plain_iso_days() {
    assert_eq!(iso_date(at(0)), "1970-01-01");
    assert_eq!(iso_date(at(86_399)), "1970-01-01");
    assert_eq!(iso_date(at(86_400)), "1970-01-02");
}

/// The two cases a hand-rolled civil calendar gets wrong.
#[test]
fn leap_days_and_century_leap_years_are_handled() {
    // 1972-02-29 — the first leap day after the epoch.
    assert_eq!(iso_date(at(789 * 86_400)), "1972-02-29");
    assert_eq!(iso_date(at(790 * 86_400)), "1972-03-01");
    // 2000 was a leap year despite being a century.
    assert_eq!(iso_date(at(11_017 * 86_400)), "2000-03-01");
}
