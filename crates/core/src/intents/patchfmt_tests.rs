//! Tests for Codex patch parsing. Included by `patchfmt.rs` under `#[cfg(test)]`.

use super::*;

fn envelope(body: &str) -> String {
    format!("*** Begin Patch\n{body}\n*** End Patch\n")
}

#[test]
fn text_without_an_envelope_yields_nothing() {
    assert!(parse_envelope("just some output").is_empty());
    assert!(parse_envelope("").is_empty());
}

#[test]
fn an_update_section_separates_added_from_removed_lines() {
    let patch = envelope(
        "*** Update File: src/lib.rs\n\
         @@ fn main\n\
          	unchanged\n\
         -let old = 1;\n\
         +let new = 2;",
    );

    let files = parse_envelope(&patch);

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "src/lib.rs");
    assert_eq!(files[0].edit.old_lines, vec!["let old = 1;"]);
    assert_eq!(files[0].edit.new_lines, vec!["let new = 2;"]);
    assert!(!files[0].edit.whole_file);
}

#[test]
fn one_envelope_can_describe_several_files() {
    let patch = envelope(
        "*** Update File: a.rs\n\
         +first\n\
         *** Update File: b.rs\n\
         +second",
    );

    let files = parse_envelope(&patch);

    assert_eq!(files.len(), 2);
    assert_eq!(files[0].path, "a.rs");
    assert_eq!(files[0].edit.new_lines, vec!["first"]);
    assert_eq!(files[1].path, "b.rs");
    assert_eq!(files[1].edit.new_lines, vec!["second"]);
}

#[test]
fn an_added_file_is_recorded_as_a_whole_file_write() {
    let patch = envelope("*** Add File: src/new.rs\n+fn one() {}\n+fn two() {}");

    let files = parse_envelope(&patch);

    assert_eq!(files.len(), 1);
    assert!(files[0].edit.whole_file);
    assert_eq!(files[0].edit.new_lines, vec!["fn one() {}", "fn two() {}"]);
    assert!(files[0].edit.old_lines.is_empty());
}

#[test]
fn a_deleted_file_records_its_removed_lines_as_a_whole_file_change() {
    let patch = envelope("*** Delete File: gone.rs\n-fn gone() {}");

    let files = parse_envelope(&patch);

    assert_eq!(files.len(), 1);
    assert!(files[0].edit.whole_file);
    assert_eq!(files[0].edit.old_lines, vec!["fn gone() {}"]);
}

#[test]
fn a_move_target_is_recorded_against_the_file_being_moved() {
    let patch = envelope(
        "*** Update File: old/path.rs\n\
         *** Move to: new/path.rs\n\
         +changed",
    );

    let files = parse_envelope(&patch);

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "old/path.rs");
    assert_eq!(files[0].moved_to.as_deref(), Some("new/path.rs"));
}

/// A blank context line is written as an empty line rather than a single
/// space. Treating it as anything but context shifts every following line.
#[test]
fn an_empty_context_line_is_not_mistaken_for_content() {
    let patch = envelope(
        "*** Update File: a.rs\n\
         \n\
         +added",
    );

    let files = parse_envelope(&patch);

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].edit.new_lines, vec!["added"]);
    assert!(files[0].edit.old_lines.is_empty());
}

#[test]
fn hunk_markers_are_not_treated_as_content() {
    let patch = envelope("*** Update File: a.rs\n@@ impl Thing\n+added");

    let files = parse_envelope(&patch);

    assert_eq!(files[0].edit.new_lines, vec!["added"]);
}

#[test]
fn a_section_that_changes_nothing_is_dropped() {
    let patch = envelope("*** Update File: untouched.rs\n context only");

    assert!(parse_envelope(&patch).is_empty());
}

/// The payload can be truncated. What was named before the cut is still
/// usable evidence.
#[test]
fn a_patch_missing_its_end_marker_is_still_read() {
    let files = parse_envelope("*** Begin Patch\n*** Update File: a.rs\n+added\n");

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].edit.new_lines, vec!["added"]);
}

#[test]
fn carriage_returns_are_stripped_from_patch_lines() {
    let patch = "*** Begin Patch\r\n*** Update File: a.rs\r\n+added\r\n*** End Patch\r\n";

    let files = parse_envelope(patch);

    assert_eq!(files[0].path, "a.rs");
    assert_eq!(files[0].edit.new_lines, vec!["added"]);
}

#[test]
fn content_before_any_section_header_is_ignored() {
    let patch = envelope("+orphan\n*** Update File: a.rs\n+real");

    let files = parse_envelope(&patch);

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].edit.new_lines, vec!["real"]);
}

// -- plain unified diffs, the other shape Codex records ---------------------

#[test]
fn a_unified_diff_separates_added_from_removed_lines() {
    let edit = parse_unified_diff("@@ -1,3 +1,3 @@\n context\n-removed\n+added\n");

    assert_eq!(edit.old_lines, vec!["removed"]);
    assert_eq!(edit.new_lines, vec!["added"]);
}

/// `---` and `+++` headers start with the same characters as content lines.
#[test]
fn unified_diff_file_headers_are_not_mistaken_for_content() {
    let edit = parse_unified_diff(
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n",
    );

    assert_eq!(edit.old_lines, vec!["old"]);
    assert_eq!(edit.new_lines, vec!["new"]);
}

#[test]
fn the_no_newline_marker_is_ignored_in_a_unified_diff() {
    let edit = parse_unified_diff("@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n");

    assert_eq!(edit.old_lines, vec!["old"]);
    assert_eq!(edit.new_lines, vec!["new"]);
}

#[test]
fn an_empty_unified_diff_yields_an_empty_edit() {
    assert!(parse_unified_diff("").is_empty());
}

// -- finding the envelope inside a tool_input of unknown shape --------------

#[test]
fn a_freeform_payload_is_read_from_a_bare_string() {
    let value = serde_json::json!(envelope("*** Update File: a.rs\n+added"));

    let found = envelope_from_value(&value).expect("an envelope");

    assert_eq!(parse_envelope(&found).len(), 1);
}

#[test]
fn a_structured_payload_is_read_from_its_input_field() {
    let value = serde_json::json!({
        "input": envelope("*** Update File: a.rs\n+added"),
    });

    assert!(envelope_from_value(&value).is_some());
}

/// `apply_patch` is also invoked through the shell, in which case the envelope
/// arrives inside an argv array rather than a field of its own.
#[test]
fn an_apply_patch_nested_in_a_shell_call_is_still_found() {
    let value = serde_json::json!({
        "command": ["bash", "-lc", format!("apply_patch '{}'", envelope("*** Update File: a.rs\n+added"))],
    });

    let found = envelope_from_value(&value).expect("an envelope");

    assert_eq!(parse_envelope(&found)[0].path, "a.rs");
}

#[test]
fn a_payload_with_no_patch_anywhere_yields_nothing() {
    let value = serde_json::json!({ "command": ["ls", "-la"], "timeout": 5 });

    assert!(envelope_from_value(&value).is_none());
}
