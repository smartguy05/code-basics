//! Tests for ingesting hook payloads. Included by `hook.rs` under `#[cfg(test)]`.

use super::*;
use crate::intents::{load, LoadOptions};
use serde_json::json;

fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(crate::intents::intents_dir(dir.path())).unwrap();
    dir
}

fn ingest_edit_payload(root: &Path, provider: ProviderId, payload: serde_json::Value) -> usize {
    ingest(root, provider, HookEvent::PostToolUse, &payload).unwrap()
}

// -- Claude Code edit payloads ----------------------------------------------

#[test]
fn a_claude_code_edit_becomes_one_record() {
    let dir = workspace();
    let file = dir.path().join("src").join("a.rs");

    let written = ingest_edit_payload(
        dir.path(),
        ProviderId::ClaudeCode,
        json!({
            "prompt_id": "turn-1",
            "tool_use_id": "call-1",
            "tool_name": "Edit",
            "tool_input": {
                "file_path": file.to_string_lossy(),
                "old_string": "let old = 1;",
                "new_string": "let new = 2;",
            },
        }),
    );

    assert_eq!(written, 1);
    let intents = load(dir.path(), &LoadOptions::default()).unwrap();
    assert_eq!(intents.records[0].path, "src/a.rs");
    assert_eq!(intents.records[0].edit.old_lines, vec!["let old = 1;"]);
    assert_eq!(intents.records[0].edit.new_lines, vec!["let new = 2;"]);
    assert_eq!(intents.records[0].turn_id, "turn-1");
}

/// The documented hook payload uses an array of edits rather than one pair.
#[test]
fn a_claude_code_edits_array_becomes_one_record_per_edit() {
    let dir = workspace();
    let file = dir.path().join("a.rs");

    let written = ingest_edit_payload(
        dir.path(),
        ProviderId::ClaudeCode,
        json!({
            "prompt_id": "turn-1",
            "tool_use_id": "call-1",
            "tool_name": "Edit",
            "tool_input": {
                "file_path": file.to_string_lossy(),
                "edits": [
                    { "old_text": "one", "new_text": "uno" },
                    { "old_text": "two", "new_text": "dos" },
                ],
            },
        }),
    );

    assert_eq!(written, 2);
}

#[test]
fn a_claude_code_write_is_recorded_as_a_whole_file_change() {
    let dir = workspace();
    let file = dir.path().join("new.rs");

    ingest_edit_payload(
        dir.path(),
        ProviderId::ClaudeCode,
        json!({
            "prompt_id": "turn-1",
            "tool_use_id": "call-1",
            "tool_name": "Write",
            "tool_input": {
                "file_path": file.to_string_lossy(),
                "content": "fn one() {}\nfn two() {}",
            },
        }),
    );

    let intents = load(dir.path(), &LoadOptions::default()).unwrap();
    assert!(intents.records[0].edit.whole_file);
    assert_eq!(intents.records[0].edit.new_lines.len(), 2);
}

// -- Codex edit payloads ----------------------------------------------------

#[test]
fn a_codex_apply_patch_becomes_records_for_each_file_it_names() {
    let dir = workspace();
    let a = dir.path().join("a.rs");
    let b = dir.path().join("b.rs");
    let patch = format!(
        "*** Begin Patch\n\
         *** Update File: {}\n\
         -old alpha\n\
         +new alpha\n\
         *** Update File: {}\n\
         +new beta\n\
         *** End Patch",
        a.display(),
        b.display()
    );

    let written = ingest_edit_payload(
        dir.path(),
        ProviderId::Codex,
        json!({
            "turn_id": "turn-9",
            "tool_use_id": "call-9",
            "tool_name": "apply_patch",
            "tool_input": { "command": patch },
        }),
    );

    assert_eq!(written, 2);
    let intents = load(dir.path(), &LoadOptions::default()).unwrap();
    let paths: Vec<&str> = intents.records.iter().map(|r| r.path.as_str()).collect();
    assert!(paths.contains(&"a.rs"));
    assert!(paths.contains(&"b.rs"));
    assert_eq!(intents.records[0].turn_id, "turn-9");
}

/// Codex advertises `apply_patch` as a freeform tool for some models, so the
/// payload is raw text rather than a structured object.
#[test]
fn a_freeform_codex_payload_is_still_recorded() {
    let dir = workspace();
    let file = dir.path().join("a.rs");
    let patch = format!(
        "*** Begin Patch\n*** Update File: {}\n+added line\n*** End Patch",
        file.display()
    );

    let written = ingest_edit_payload(
        dir.path(),
        ProviderId::Codex,
        json!({
            "turn_id": "turn-1",
            "tool_use_id": "call-1",
            "tool_name": "apply_patch",
            "tool_input": patch,
        }),
    );

    assert_eq!(written, 1);
}

#[test]
fn an_apply_patch_run_through_the_shell_is_still_recorded() {
    let dir = workspace();
    let file = dir.path().join("a.rs");
    let patch = format!(
        "*** Begin Patch\n*** Update File: {}\n+added line\n*** End Patch",
        file.display()
    );

    let written = ingest_edit_payload(
        dir.path(),
        ProviderId::Codex,
        json!({
            "turn_id": "turn-1",
            "tool_use_id": "call-1",
            "tool_name": "shell",
            "tool_input": { "command": ["bash", "-lc", format!("apply_patch '{patch}'")] },
        }),
    );

    assert_eq!(written, 1);
}

// -- events that are not edits ----------------------------------------------

#[test]
fn a_tool_call_that_is_not_an_edit_records_nothing() {
    let dir = workspace();

    let written = ingest_edit_payload(
        dir.path(),
        ProviderId::Codex,
        json!({
            "turn_id": "turn-1",
            "tool_name": "shell",
            "tool_input": { "command": ["ls", "-la"] },
        }),
    );

    assert_eq!(written, 0);
}

#[test]
fn an_edit_outside_the_workspace_records_nothing() {
    let dir = workspace();
    let outside = if cfg!(windows) {
        "C:/elsewhere/a.rs"
    } else {
        "/elsewhere/a.rs"
    };

    let written = ingest_edit_payload(
        dir.path(),
        ProviderId::ClaudeCode,
        json!({
            "prompt_id": "turn-1",
            "tool_name": "Edit",
            "tool_input": { "file_path": outside, "old_string": "a", "new_string": "b" },
        }),
    );

    assert_eq!(written, 0);
}

#[test]
fn a_payload_with_no_tool_input_records_nothing() {
    let dir = workspace();

    assert_eq!(
        ingest_edit_payload(dir.path(), ProviderId::Codex, json!({ "turn_id": "t" })),
        0
    );
}

// -- labels from the closing message ----------------------------------------

#[test]
fn a_declared_intent_line_becomes_a_label() {
    let labels = parse_labels("Done.\n\nIntent: add retry to token refresh");

    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].1, "add retry to token refresh");
    assert!(labels[0].0.is_empty());
}

#[test]
fn a_declared_intent_can_name_the_files_it_covers() {
    let labels = parse_labels("Intent(src/a.rs, src/b.rs): cache the refreshed token");

    assert_eq!(labels[0].0, vec!["src/a.rs", "src/b.rs"]);
    assert_eq!(labels[0].1, "cache the refreshed token");
}

#[test]
fn several_declared_intents_are_all_captured() {
    let labels = parse_labels(
        "Intent(a.rs): add retry logic\n\
         Intent(b.rs): update the tests",
    );

    assert_eq!(labels.len(), 2);
}

#[test]
fn a_declared_intent_is_recognised_in_a_bulleted_list() {
    let labels = parse_labels("- Intent: rename Quote to Estimate");

    assert_eq!(labels[0].1, "rename Quote to Estimate");
}

#[test]
fn the_intent_keyword_is_matched_regardless_of_case() {
    let labels = parse_labels("intent: lowercase still works");

    assert_eq!(labels[0].1, "lowercase still works");
}

/// Better a coarse label than an unexplained change.
#[test]
fn a_message_with_no_declared_intent_falls_back_to_its_first_sentence() {
    let labels = parse_labels("I added retry handling to the token refresh path. Then tests.");

    assert_eq!(labels.len(), 1);
    assert_eq!(
        labels[0].1,
        "I added retry handling to the token refresh path"
    );
}

#[test]
fn a_heading_is_skipped_when_falling_back() {
    let labels = parse_labels("## Summary\nSwapped the cost calculation over to the new API");

    assert_eq!(
        labels[0].1,
        "Swapped the cost calculation over to the new API"
    );
}

#[test]
fn a_message_too_short_to_mean_anything_produces_no_label() {
    assert!(parse_labels("ok").is_empty());
    assert!(parse_labels("").is_empty());
}

#[test]
fn a_stop_payload_writes_a_label_joined_to_its_turn() {
    let dir = workspace();

    let written = ingest(
        dir.path(),
        ProviderId::Codex,
        HookEvent::Stop,
        &json!({
            "turn_id": "turn-7",
            "last_assistant_message": "Intent: add retry to token refresh",
        }),
    )
    .unwrap();

    assert_eq!(written, 1);
    let intents = load(dir.path(), &LoadOptions::default()).unwrap();
    assert_eq!(intents.labels[0].turn_id, "turn-7");
    assert_eq!(intents.labels[0].label, "add retry to token refresh");
}

/// The whole point of the two-event design: geometry and reason arrive
/// separately and are joined afterwards.
#[test]
fn an_edit_and_a_later_label_join_on_the_turn_identifier() {
    let dir = workspace();
    let file = dir.path().join("a.rs");

    ingest_edit_payload(
        dir.path(),
        ProviderId::ClaudeCode,
        json!({
            "prompt_id": "turn-42",
            "tool_use_id": "call-1",
            "tool_name": "Edit",
            "tool_input": {
                "file_path": file.to_string_lossy(),
                "old_string": "old",
                "new_string": "new",
            },
        }),
    );
    ingest(
        dir.path(),
        ProviderId::ClaudeCode,
        HookEvent::Stop,
        &json!({
            "prompt_id": "turn-42",
            "last_assistant_message": "Intent: swap the implementation",
        }),
    )
    .unwrap();

    let intents = load(dir.path(), &LoadOptions::default()).unwrap();
    let label = intents.label_for(&intents.records[0]).unwrap();

    assert_eq!(label.label, "swap the implementation");
}

// -- guards -----------------------------------------------------------------

/// A user-level hook fires for every repository on the machine.
#[test]
fn a_workspace_that_never_enabled_capture_is_not_recorded_into() {
    let dir = tempfile::tempdir().unwrap();

    assert!(!is_enabled(dir.path()));
}

#[test]
fn a_workspace_that_enabled_capture_is_recorded_into() {
    let dir = workspace();

    assert!(is_enabled(dir.path()));
}

#[test]
fn the_workspace_named_on_the_command_line_wins_over_the_payload() {
    let payload = json!({ "cwd": "/from/payload" });

    let resolved = resolve_root(Some("/explicit"), &payload).unwrap();

    assert_eq!(resolved, PathBuf::from("/explicit"));
}

#[test]
fn the_payload_cwd_is_used_when_no_workspace_was_named() {
    let payload = json!({ "cwd": "/from/payload" });

    assert_eq!(
        resolve_root(None, &payload).unwrap(),
        PathBuf::from("/from/payload")
    );
}

#[test]
fn an_unknown_event_name_is_not_accepted() {
    assert!(HookEvent::parse("PreToolUse").is_none());
    assert_eq!(HookEvent::parse("Stop"), Some(HookEvent::Stop));
}
