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

/// A user-scope hook no longer names a workspace, so the payload's `cwd` is
/// what decides where a record lands.
#[test]
fn resolve_root_falls_back_to_the_payload_cwd_when_no_workspace_is_named() {
    let dir = workspace();
    let elsewhere = tempfile::tempdir().unwrap();

    assert_eq!(
        resolve_enabled_root(None, Some(dir.path())).unwrap(),
        dir.path()
    );
    assert_eq!(resolve_enabled_root(None, Some(elsewhere.path())), None);
}

/// An agent's `cwd` is wherever it was started, which is routinely a
/// subdirectory of the repository that enabled capture.
#[test]
fn resolve_root_ascends_from_a_subdirectory_to_the_enabled_workspace() {
    let dir = workspace();
    let deep = dir.path().join("src").join("deep");
    std::fs::create_dir_all(&deep).unwrap();

    assert_eq!(resolve_enabled_root(None, Some(&deep)).unwrap(), dir.path());
}

/// A project-scope hook still names its workspace, and that answer is taken
/// as given — no ascent, and the enabled check stays with the caller.
#[test]
fn a_named_workspace_is_used_verbatim_without_ascending() {
    let dir = workspace();
    let deep = dir.path().join("src");
    std::fs::create_dir_all(&deep).unwrap();

    assert_eq!(
        resolve_enabled_root(Some(&deep), Some(dir.path())).unwrap(),
        deep
    );
}

#[test]
fn an_unknown_event_name_is_not_accepted() {
    assert!(HookEvent::parse("PreToolUse").is_none());
    assert_eq!(HookEvent::parse("Stop"), Some(HookEvent::Stop));
}

// -- payload shapes the ingest has to survive -------------------------------

/// The hook runs unattended after every edit, so an event carrying nothing it
/// recognises must be a quiet no-op rather than a failure.
#[test]
fn a_payload_that_is_not_an_object_records_nothing() {
    let dir = workspace();

    assert_eq!(
        ingest(
            dir.path(),
            ProviderId::Codex,
            HookEvent::PostToolUse,
            &json!("hello")
        )
        .unwrap(),
        0
    );
    assert_eq!(
        ingest(
            dir.path(),
            ProviderId::Codex,
            HookEvent::Stop,
            &json!([1, 2, 3])
        )
        .unwrap(),
        0
    );
}

/// A tool input naming no file has nothing to attribute to.
#[test]
fn a_tool_input_with_no_file_path_records_nothing() {
    let dir = workspace();

    let written = ingest_edit_payload(
        dir.path(),
        ProviderId::ClaudeCode,
        json!({
            "prompt_id": "turn-1",
            "tool_name": "Edit",
            "tool_input": { "old_string": "a", "new_string": "b" },
        }),
    );

    assert_eq!(written, 0);
}

/// A `Write` whose content is missing is not a whole-file change anyone can
/// match against, and guessing an empty file would claim every deletion.
#[test]
fn a_write_with_no_content_records_nothing() {
    let dir = workspace();
    let file = dir.path().join("a.rs");

    let written = ingest_edit_payload(
        dir.path(),
        ProviderId::ClaudeCode,
        json!({
            "prompt_id": "turn-1",
            "tool_name": "Write",
            "tool_input": { "file_path": file.to_string_lossy() },
        }),
    );

    assert_eq!(written, 0);
}

/// Codex spells the written text `file_text` where Claude Code says `content`;
/// both have to reach the same record, or one agent silently records nothing.
#[test]
fn a_write_is_recognised_under_either_agents_spelling_for_its_content() {
    let dir = workspace();
    let file = dir.path().join("a.rs");

    ingest_edit_payload(
        dir.path(),
        ProviderId::Codex,
        json!({
            "turn_id": "turn-1",
            "tool_use_id": "call-1",
            "tool_name": "write",
            "tool_input": { "file_path": file.to_string_lossy(), "file_text": "one\ntwo" },
        }),
    );

    let records = load(dir.path(), &LoadOptions::default()).unwrap().records;
    assert_eq!(records.len(), 1);
    assert!(records[0].edit.whole_file);
    assert_eq!(records[0].edit.new_lines, vec!["one", "two"]);
}

/// An edits array where every entry is empty carries no evidence at all.
#[test]
fn an_edits_array_of_empty_pairs_records_nothing() {
    let dir = workspace();
    let file = dir.path().join("a.rs");

    let written = ingest_edit_payload(
        dir.path(),
        ProviderId::ClaudeCode,
        json!({
            "prompt_id": "turn-1",
            "tool_name": "Edit",
            "tool_input": {
                "file_path": file.to_string_lossy(),
                "edits": [{ "old_text": "", "new_text": "" }],
            },
        }),
    );

    assert_eq!(written, 0);
}

/// One unusable entry in an array must cost that entry, not the ones beside it.
#[test]
fn an_unusable_entry_in_an_edits_array_costs_only_itself() {
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
                    { "old_text": "", "new_text": "" },
                    { "old_text": "one", "new_text": "uno" },
                ],
            },
        }),
    );

    assert_eq!(written, 1);
}

/// Every agent identifier is optional in practice. A payload with none of them
/// still records, under a turn id that says so rather than an empty string
/// that would join to every other unidentified turn by accident.
#[test]
fn a_payload_with_no_turn_identifier_records_under_a_named_unknown() {
    let dir = workspace();
    let file = dir.path().join("a.rs");

    ingest_edit_payload(
        dir.path(),
        ProviderId::Codex,
        json!({
            "tool_name": "Edit",
            "tool_input": {
                "file_path": file.to_string_lossy(),
                "old_string": "a",
                "new_string": "b",
            },
        }),
    );

    let records = load(dir.path(), &LoadOptions::default()).unwrap().records;
    assert_eq!(records[0].turn_id, "unknown-turn");
}

/// An empty identifier is no identifier: the next name down is used instead.
#[test]
fn an_empty_turn_identifier_falls_through_to_the_next_one() {
    let dir = workspace();
    let file = dir.path().join("a.rs");

    ingest_edit_payload(
        dir.path(),
        ProviderId::ClaudeCode,
        json!({
            "turn_id": "",
            "prompt_id": "",
            "session_id": "session-9",
            "tool_name": "Edit",
            "tool_input": {
                "file_path": file.to_string_lossy(),
                "old_string": "a",
                "new_string": "b",
            },
        }),
    );

    let records = load(dir.path(), &LoadOptions::default()).unwrap().records;
    assert_eq!(records[0].turn_id, "session-9");
}

/// Both agents' spellings of the pair have to work, since the same hook binary
/// serves both.
#[test]
fn an_edit_pair_is_recognised_under_either_agents_field_names() {
    let dir = workspace();
    let file = dir.path().join("a.rs");

    ingest_edit_payload(
        dir.path(),
        ProviderId::Codex,
        json!({
            "turn_id": "turn-1",
            "tool_use_id": "call-1",
            "tool_name": "edit",
            "tool_input": {
                "file_path": file.to_string_lossy(),
                "old_text": "let old = 1;",
                "new_text": "let new = 2;",
            },
        }),
    );

    let records = load(dir.path(), &LoadOptions::default()).unwrap().records;
    assert_eq!(records[0].edit.old_lines, vec!["let old = 1;"]);
    assert_eq!(records[0].edit.new_lines, vec!["let new = 2;"]);
    assert!(!records[0].edit.whole_file);
}

/// Several edits in one tool call must not collide on the call's own id, or
/// deduplication on load would keep only the first.
#[test]
fn each_edit_in_one_call_gets_a_distinct_tool_use_id() {
    let dir = workspace();
    let file = dir.path().join("a.rs");

    ingest_edit_payload(
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

    let records = load(dir.path(), &LoadOptions::default()).unwrap().records;
    assert_eq!(records.len(), 2, "both edits must survive deduplication");
    assert_ne!(records[0].tool_use_id, records[1].tool_use_id);
    assert_ne!(records[0].seq, records[1].seq);
}

/// A `Stop` whose payload has no closing message has no reason in it.
#[test]
fn a_stop_payload_with_no_closing_message_writes_no_label() {
    let dir = workspace();

    let written = ingest(
        dir.path(),
        ProviderId::ClaudeCode,
        HookEvent::Stop,
        &json!({ "prompt_id": "turn-1" }),
    )
    .unwrap();

    assert_eq!(written, 0);
    assert!(load(dir.path(), &LoadOptions::default())
        .unwrap()
        .labels
        .is_empty());
}

/// A closing message too short to mean anything yields no label at all — the
/// abstain rule: a useless label is worse than none.
#[test]
fn a_stop_payload_with_an_unusable_message_writes_no_label() {
    let dir = workspace();

    let written = ingest(
        dir.path(),
        ProviderId::ClaudeCode,
        HookEvent::Stop,
        &json!({ "prompt_id": "turn-1", "last_assistant_message": "ok" }),
    )
    .unwrap();

    assert_eq!(written, 0);
}

// -- the recorder command line ----------------------------------------------

fn args(raw: &[&str]) -> Vec<String> {
    raw.iter().map(|s| s.to_string()).collect()
}

/// The hooks we write name the subcommand *and* the marker; either alone has
/// to be enough, because a user may have edited one of them out.
#[test]
fn either_recorder_token_identifies_a_record_invocation() {
    assert!(is_record_invocation(&args(&[
        "cb-app.exe",
        "record-intent"
    ])));
    assert!(is_record_invocation(&args(&[
        "cb-app.exe",
        "--code-basics-intent"
    ])));
}

/// Anything else is the application starting normally, and must open a window.
#[test]
fn an_ordinary_launch_is_not_a_record_invocation() {
    assert!(!is_record_invocation(&args(&["cb-app.exe"])));
    assert!(!is_record_invocation(&args(&[
        "cb-app.exe",
        "C:/work/repo"
    ])));
    assert!(!is_record_invocation(&args(&["cb-app.exe", "record"])));
}

#[test]
fn a_flag_yields_the_value_that_follows_it() {
    assert_eq!(
        flag(&args(&["exe", "--event", "Stop"]), "--event").as_deref(),
        Some("Stop")
    );
}

#[test]
fn a_flag_that_was_never_passed_yields_nothing() {
    assert_eq!(flag(&args(&["exe", "--event", "Stop"]), "--provider"), None);
}

/// A trailing flag has no value after it. Reading past the end would panic,
/// and treating the flag's own name as its value would be worse.
#[test]
fn a_flag_with_no_value_after_it_yields_nothing() {
    assert_eq!(flag(&args(&["exe", "--event"]), "--event"), None);
}

/// First wins, so a hook line appended to one that already had the flag does
/// not silently change what the earlier entry meant.
#[test]
fn a_repeated_flag_yields_the_first_value() {
    assert_eq!(
        flag(
            &args(&["exe", "--event", "Stop", "--event", "PostToolUse"]),
            "--event"
        )
        .as_deref(),
        Some("Stop")
    );
}

#[test]
fn a_recorder_command_line_is_parsed_into_its_parts() {
    let parsed = parse_recorder_args(&args(&[
        "cb-app.exe",
        "record-intent",
        "--code-basics-intent",
        "--provider",
        "codex",
        "--event",
        "PostToolUse",
        "--workspace",
        "C:/work/repo",
    ]))
    .unwrap();

    assert_eq!(parsed.provider, ProviderId::Codex);
    assert_eq!(parsed.event, HookEvent::PostToolUse);
    assert_eq!(parsed.workspace.as_deref(), Some("C:/work/repo"));
}

/// Claude Code is the default so an older hook line written without the flag
/// keeps recording rather than silently mislabelling itself.
#[test]
fn an_unnamed_or_unknown_provider_is_treated_as_claude_code() {
    for line in [
        vec!["exe", "record-intent", "--event", "Stop"],
        vec!["exe", "record-intent", "--event", "Stop", "--provider", ""],
        vec![
            "exe",
            "record-intent",
            "--event",
            "Stop",
            "--provider",
            "something-else",
        ],
    ] {
        let parsed = parse_recorder_args(&args(&line)).unwrap();
        assert_eq!(parsed.provider, ProviderId::ClaudeCode, "for {line:?}");
    }
}

/// Some other hook fired. Nothing to record, and not a problem.
#[test]
fn a_command_line_with_no_recognised_event_parses_to_nothing() {
    assert!(parse_recorder_args(&args(&["exe", "record-intent"])).is_none());
    assert!(parse_recorder_args(&args(&["exe", "record-intent", "--event"])).is_none());
    assert!(
        parse_recorder_args(&args(&["exe", "record-intent", "--event", "PreToolUse"])).is_none()
    );
}

/// A launch that never asked to record must not be parsed as one, even if
/// stray arguments happen to look like the flags.
#[test]
fn a_non_record_invocation_parses_to_nothing() {
    assert!(parse_recorder_args(&args(&["exe", "--event", "Stop"])).is_none());
}

/// No `--workspace` is normal: the payload's `cwd` stands in for it.
#[test]
fn an_absent_workspace_flag_leaves_the_root_to_the_payload() {
    let parsed = parse_recorder_args(&args(&["exe", "record-intent", "--event", "Stop"])).unwrap();

    assert_eq!(parsed.workspace, None);
    assert_eq!(
        resolve_root(
            parsed.workspace.as_deref(),
            &json!({ "cwd": "/from/payload" })
        )
        .unwrap(),
        PathBuf::from("/from/payload")
    );
}

// -- which lifecycle event fired --------------------------------------------

/// Only the two events this feature installs are accepted. Anything else is a
/// hook someone else configured, and acting on it would record edits the user
/// never asked to have recorded.
#[test]
fn only_the_two_installed_events_are_recognised() {
    assert_eq!(
        HookEvent::parse("PostToolUse"),
        Some(HookEvent::PostToolUse)
    );
    assert_eq!(HookEvent::parse("Stop"), Some(HookEvent::Stop));

    for unknown in [
        "PreToolUse",
        "SessionStart",
        "SubagentStop",
        "Notification",
        "UserPromptSubmit",
        "",
    ] {
        assert!(HookEvent::parse(unknown).is_none(), "accepted {unknown:?}");
    }
}

/// The names are the agents' own, matched exactly: a case-insensitive match
/// would accept a similarly-named event from some other tool.
#[test]
fn event_names_are_matched_exactly_rather_than_loosely() {
    assert!(HookEvent::parse("posttooluse").is_none());
    assert!(HookEvent::parse("STOP").is_none());
    assert!(HookEvent::parse(" Stop").is_none());
    assert!(HookEvent::parse("Stop ").is_none());
    assert!(HookEvent::parse("Stopped").is_none());
}
