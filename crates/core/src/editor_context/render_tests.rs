use super::*;

use crate::model::{
    EditorContext, EditorCursor, EditorOpenFile, EditorRecentFile, EditorSelection, EditorViewport,
};

fn open_file(path: &str, active: bool, dirty: bool, pinned: bool) -> EditorOpenFile {
    EditorOpenFile {
        path: path.to_string(),
        active,
        dirty,
        pinned,
    }
}

/// A full, populated context: an active file with cursor, viewport, a selection,
/// a few open tabs and a recent list.
fn full() -> EditorContext {
    EditorContext {
        active_file: Some("src/App.cs".to_string()),
        cursor: Some(EditorCursor {
            line: 12,
            character: 4,
        }),
        viewport: Some(EditorViewport {
            first_visible_line: 5,
            last_visible_line: 40,
        }),
        selection: Some(EditorSelection {
            start_line: 12,
            start_character: 4,
            end_line: 14,
            end_character: 8,
            text: "var w = 1;".to_string(),
        }),
        open_files: vec![
            open_file("src/App.cs", true, true, false),
            open_file("src/Main.cs", false, false, false),
            open_file("src/Util.cs", false, false, true),
        ],
        recent_files: vec![
            EditorRecentFile {
                path: "src/App.cs".to_string(),
            },
            EditorRecentFile {
                path: "src/Main.cs".to_string(),
            },
        ],
    }
}

/// An empty context: nothing open, nothing active, nothing selected.
fn empty() -> EditorContext {
    EditorContext {
        active_file: None,
        cursor: None,
        viewport: None,
        selection: None,
        open_files: Vec::new(),
        recent_files: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// get_active_file
// ---------------------------------------------------------------------------

#[test]
fn active_file_reports_path_cursor_viewport_and_flags() {
    let answer = get_active_file(&full());
    assert!(answer.ok);
    assert!(
        answer.text.contains("Active file: src/App.cs"),
        "{}",
        answer.text
    );
    assert!(
        answer.text.contains("Cursor: line 12, character 4"),
        "{}",
        answer.text
    );
    assert!(
        answer.text.contains("Visible lines: 5-40"),
        "{}",
        answer.text
    );
    assert!(
        answer.text.contains("Unsaved changes: yes"),
        "{}",
        answer.text
    );
    assert!(answer.text.contains("Pinned: no"), "{}", answer.text);
    assert!(
        answer.text.contains("1-based line, 0-based UTF-16"),
        "{}",
        answer.text
    );
}

#[test]
fn active_file_with_no_active_file_is_a_refusal_not_a_fabricated_path() {
    let answer = get_active_file(&empty());
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("noActiveFile"));
    // No path, no line number invented.
    assert!(!answer.text.contains(".cs"), "{}", answer.text);
}

#[test]
fn active_file_states_when_cursor_or_viewport_were_not_reported_rather_than_inventing() {
    let mut context = full();
    context.cursor = None;
    context.viewport = None;
    let answer = get_active_file(&context);
    assert!(answer.ok);
    assert!(
        answer.text.contains("Cursor: not reported"),
        "{}",
        answer.text
    );
    assert!(
        answer.text.contains("Visible lines: not reported"),
        "{}",
        answer.text
    );
}

#[test]
fn active_file_not_among_open_tabs_abstains_on_its_flags() {
    let mut context = full();
    // Active file whose entry is not in open_files.
    context.active_file = Some("src/Ghost.cs".to_string());
    let answer = get_active_file(&context);
    assert!(answer.ok);
    assert!(
        answer.text.contains("not among the reported open tabs"),
        "{}",
        answer.text
    );
    assert!(
        !answer.text.contains("Unsaved changes: "),
        "{}",
        answer.text
    );
}

// ---------------------------------------------------------------------------
// get_selection
// ---------------------------------------------------------------------------

#[test]
fn selection_reports_range_text_and_length() {
    let answer = get_selection(&full());
    assert!(answer.ok);
    assert!(
        answer
            .text
            .contains("Selection in src/App.cs: from line 12, character 4 to line 14, character 8"),
        "{}",
        answer.text
    );
    assert!(
        answer.text.contains("10 character(s) selected"),
        "{}",
        answer.text
    );
    assert!(answer.text.contains("var w = 1;"), "{}", answer.text);
    assert!(
        answer.text.contains("1-based line, 0-based UTF-16"),
        "{}",
        answer.text
    );
}

#[test]
fn selection_with_an_active_file_but_no_selection_is_a_genuine_empty_not_a_refusal() {
    let mut context = full();
    context.selection = None;
    let answer = get_selection(&context);
    assert!(answer.ok, "an empty selection is data, not a refusal");
    assert!(
        answer.text.contains("No text is selected in src/App.cs"),
        "{}",
        answer.text
    );
    assert!(answer.text.contains("complete answer"), "{}", answer.text);
}

#[test]
fn selection_with_no_active_file_is_the_no_active_file_refusal() {
    let answer = get_selection(&empty());
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("noActiveFile"));
}

// ---------------------------------------------------------------------------
// get_open_files
// ---------------------------------------------------------------------------

#[test]
fn open_files_are_listed_with_their_flags() {
    let answer = get_open_files(&full());
    assert!(answer.ok);
    assert!(answer.text.contains("3 open file(s):"), "{}", answer.text);
    assert!(
        answer.text.contains("src/App.cs  (active, unsaved)"),
        "{}",
        answer.text
    );
    assert!(
        answer.text.contains("src/Util.cs  (pinned)"),
        "{}",
        answer.text
    );
    // A tab with no flags has no parenthetical.
    assert!(
        answer.text.contains("\n  src/Main.cs\n") || answer.text.ends_with("  src/Main.cs"),
        "{}",
        answer.text
    );
}

#[test]
fn no_open_files_is_a_genuine_empty_answer() {
    let answer = get_open_files(&empty());
    assert!(answer.ok);
    assert!(answer.text.contains("No files are open"), "{}", answer.text);
    assert!(answer.text.contains("complete answer"), "{}", answer.text);
}

// ---------------------------------------------------------------------------
// get_recent_files
// ---------------------------------------------------------------------------

#[test]
fn recent_files_are_listed_most_recent_first() {
    let answer = get_recent_files(&full());
    assert!(answer.ok);
    assert!(
        answer
            .text
            .contains("2 recently edited file(s), most recent first:"),
        "{}",
        answer.text
    );
    let app = answer.text.find("src/App.cs").unwrap();
    let main = answer.text.find("src/Main.cs").unwrap();
    assert!(app < main, "order must be preserved: {}", answer.text);
}

#[test]
fn no_recent_files_is_a_genuine_empty_answer() {
    let answer = get_recent_files(&empty());
    assert!(answer.ok);
    assert!(
        answer.text.contains("No files have been edited recently"),
        "{}",
        answer.text
    );
}

// ---------------------------------------------------------------------------
// Nothing fabricates a path or position
// ---------------------------------------------------------------------------

#[test]
fn every_tool_on_an_empty_context_fabricates_no_path_or_position() {
    let context = empty();
    for answer in [
        get_active_file(&context),
        get_selection(&context),
        get_open_files(&context),
        get_recent_files(&context),
    ] {
        assert!(!answer.text.contains(".cs"), "{}", answer.text);
        assert!(!answer.text.contains("line 1"), "{}", answer.text);
    }
}
