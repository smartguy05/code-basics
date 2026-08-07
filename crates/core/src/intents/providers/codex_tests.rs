//! Tests for reading Codex's rollout files and reporting its configuration.
//! Included by `providers/codex.rs` under `#[cfg(test)]`.
//!
//! The transform layer — JSON line in, [`IntentRecord`] out — is pure and is
//! tested directly. Everything that has to find a file goes through the
//! `*_in(home, ..)` seam so a test can point the whole provider at a tempdir
//! instead of at whatever Codex happens to be installed on this machine.

use super::*;

// -- helpers ----------------------------------------------------------------

fn workspace() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\code\demo")
    } else {
        PathBuf::from("/code/demo")
    }
}

/// One rollout line, in the envelope Codex writes.
fn line(kind: &str, payload: Value) -> String {
    serde_json::to_string(&json!({ "type": kind, "payload": payload })).unwrap()
}

fn session_meta(cwd: &Path, branch: Option<&str>) -> String {
    let mut payload = json!({ "cwd": cwd.to_string_lossy() });
    if let Some(branch) = branch {
        payload["git"] = json!({ "branch": branch });
    }
    line("session_meta", payload)
}

fn custom_tool_call(call_id: &str, input: &str) -> Value {
    json!({
        "type": "custom_tool_call",
        "name": "apply_patch",
        "call_id": call_id,
        "input": input,
    })
}

fn envelope(path: &str) -> String {
    format!("*** Begin Patch\n*** Update File: {path}\n@@\n-old\n+new\n*** End Patch\n")
}

fn write_rollout(home: &Path, name: &str, lines: &[String]) -> PathBuf {
    let dir = home.join("sessions").join("2026").join("08").join("07");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();
    path
}

// -- characterisation: the trait still resolves the real Codex home ---------

/// Pinned before the `*_in` seam was introduced, so the refactor could be
/// shown not to have changed what the trait method answers on this machine.
#[test]
fn detected_agrees_with_the_resolved_codex_home_directory() {
    let expected = codex_home().is_some_and(|h| h.is_dir());

    assert_eq!(Codex::new().detected(), expected);
}

/// A directory Codex has never seen has no sessions and no capture, whether or
/// not Codex is installed here.
#[test]
fn status_for_an_unknown_workspace_reports_nothing_configured() {
    let dir = workspace();

    let status = Codex::new().status(dir.path());

    assert_eq!(status.provider, ProviderId::Codex);
    assert_eq!(status.capture, None);
    assert_eq!(status.sessions, 0);
    assert_eq!(status.detected, codex_home().is_some_and(|h| h.is_dir()));
}

// -- split_lines ------------------------------------------------------------

#[test]
fn splitting_lines_strips_the_carriage_return_of_a_windows_file() {
    assert_eq!(split_lines("one\r\ntwo\r\n"), vec!["one", "two"]);
}

#[test]
fn splitting_empty_text_yields_no_lines() {
    assert!(split_lines("").is_empty());
}

#[test]
fn a_trailing_newline_does_not_produce_an_extra_empty_line() {
    assert_eq!(split_lines("only\n"), vec!["only"]);
}

#[test]
fn blank_lines_inside_the_text_are_kept() {
    assert_eq!(split_lines("a\n\nb"), vec!["a", "", "b"]);
}

// -- file_change_to_edit ----------------------------------------------------

#[test]
fn an_added_file_becomes_a_whole_file_insertion() {
    let edit = file_change_to_edit(&json!({ "type": "add", "content": "one\ntwo" })).unwrap();

    assert!(edit.old_lines.is_empty());
    assert_eq!(edit.new_lines, vec!["one", "two"]);
    assert!(edit.whole_file);
}

#[test]
fn a_deleted_file_becomes_a_whole_file_removal() {
    let edit = file_change_to_edit(&json!({ "type": "delete", "content": "gone\n" })).unwrap();

    assert_eq!(edit.old_lines, vec!["gone"]);
    assert!(edit.new_lines.is_empty());
    assert!(edit.whole_file);
}

#[test]
fn an_updated_file_is_read_from_its_unified_diff_and_is_not_whole_file() {
    let edit = file_change_to_edit(&json!({
        "type": "update",
        "unified_diff": "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-before\n+after\n",
    }))
    .unwrap();

    assert_eq!(edit.old_lines, vec!["before"]);
    assert_eq!(edit.new_lines, vec!["after"]);
    assert!(
        !edit.whole_file,
        "an update is a region, not a rewrite: {edit:?}"
    );
}

#[test]
fn a_change_of_an_unknown_type_is_not_guessed_at() {
    assert!(file_change_to_edit(&json!({ "type": "rename", "content": "x" })).is_none());
    assert!(file_change_to_edit(&json!({ "content": "x" })).is_none());
}

#[test]
fn a_change_missing_the_field_its_type_requires_is_dropped() {
    assert!(file_change_to_edit(&json!({ "type": "add" })).is_none());
    assert!(file_change_to_edit(&json!({ "type": "delete" })).is_none());
    assert!(file_change_to_edit(&json!({ "type": "update" })).is_none());
}

// -- push_record ------------------------------------------------------------

fn an_edit() -> IntentEdit {
    IntentEdit {
        old_lines: vec!["old".into()],
        new_lines: vec!["new".into()],
        whole_file: false,
    }
}

#[test]
fn a_pushed_record_carries_the_provider_turn_and_tool_identifiers() {
    let mut out = Vec::new();
    let mut seq = 0;

    push_record(
        &mut out,
        &root(),
        &None,
        &mut seq,
        "src/lib.rs",
        an_edit(),
        "call-1",
        "turn-9",
    );

    assert_eq!(out.len(), 1);
    assert_eq!(out[0].provider, ProviderId::Codex);
    assert_eq!(out[0].turn_id, "codex-history-turn-9");
    assert_eq!(out[0].tool_use_id, "call-1:src/lib.rs");
    assert_eq!(out[0].path, "src/lib.rs");
    assert_eq!(out[0].seq, 1);
}

#[test]
fn an_edit_that_changes_nothing_is_not_recorded() {
    let mut out = Vec::new();
    let mut seq = 0;

    push_record(
        &mut out,
        &root(),
        &None,
        &mut seq,
        "src/lib.rs",
        IntentEdit {
            old_lines: Vec::new(),
            new_lines: Vec::new(),
            whole_file: true,
        },
        "call-1",
        "turn-1",
    );

    assert!(out.is_empty());
    assert_eq!(
        seq, 0,
        "a dropped record must not consume a sequence number"
    );
}

/// An agent editing another repository is not this workspace's history.
#[test]
fn a_path_outside_the_workspace_is_dropped() {
    let mut out = Vec::new();
    let mut seq = 0;
    let outside = if cfg!(windows) {
        r"D:\elsewhere\other.rs"
    } else {
        "/elsewhere/other.rs"
    };

    push_record(
        &mut out,
        &root(),
        &None,
        &mut seq,
        outside,
        an_edit(),
        "call-1",
        "turn-1",
    );

    assert!(out.is_empty());
}

#[test]
fn an_absolute_path_inside_the_workspace_is_made_relative_with_forward_slashes() {
    let mut out = Vec::new();
    let mut seq = 0;
    let inside = root().join("src").join("lib.rs");

    push_record(
        &mut out,
        &root(),
        &None,
        &mut seq,
        &inside.to_string_lossy(),
        an_edit(),
        "call-1",
        "turn-1",
    );

    assert_eq!(out[0].path, "src/lib.rs");
}

#[test]
fn each_recorded_edit_takes_the_next_sequence_number() {
    let mut out = Vec::new();
    let mut seq = 7;

    for path in ["a.rs", "b.rs"] {
        push_record(
            &mut out,
            &root(),
            &None,
            &mut seq,
            path,
            an_edit(),
            "call-1",
            "turn-1",
        );
    }

    assert_eq!(
        out.iter().map(|r| r.seq).collect::<Vec<_>>(),
        vec![8, 9],
        "sequence continues from where the caller left it"
    );
}

#[test]
fn the_sessions_branch_is_copied_onto_every_record() {
    let mut out = Vec::new();
    let mut seq = 0;

    push_record(
        &mut out,
        &root(),
        &Some("feature/x".to_string()),
        &mut seq,
        "a.rs",
        an_edit(),
        "call-1",
        "turn-1",
    );

    assert_eq!(out[0].branch.as_deref(), Some("feature/x"));
}

// -- read_custom_tool_call --------------------------------------------------

#[test]
fn a_custom_tool_call_records_every_file_its_patch_named() {
    let patch = "*** Begin Patch\n*** Update File: a.rs\n-old\n+new\n*** Add File: b.rs\n+fresh\n\
                 *** End Patch\n";
    let mut out = Vec::new();
    let mut seq = 0;

    read_custom_tool_call(
        &custom_tool_call("call-1", patch),
        &root(),
        &None,
        &mut seq,
        &mut out,
    );

    assert_eq!(
        out.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        vec!["a.rs", "b.rs"]
    );
}

/// One patch is one intent, so every file it touched shares a turn.
#[test]
fn every_file_in_one_patch_shares_the_call_id_as_its_turn() {
    let patch = "*** Begin Patch\n*** Update File: a.rs\n-old\n+new\n*** Update File: b.rs\n\
                 -old\n+new\n*** End Patch\n";
    let mut out = Vec::new();
    let mut seq = 0;

    read_custom_tool_call(
        &custom_tool_call("call-1", patch),
        &root(),
        &None,
        &mut seq,
        &mut out,
    );

    assert_eq!(out[0].turn_id, "codex-history-call-1");
    assert_eq!(out[1].turn_id, out[0].turn_id);
    assert_ne!(
        out[0].tool_use_id, out[1].tool_use_id,
        "the per-file tool id still distinguishes them"
    );
}

#[test]
fn a_tool_call_for_another_tool_records_nothing() {
    let mut out = Vec::new();
    let mut seq = 0;
    let mut payload = custom_tool_call("call-1", &envelope("a.rs"));
    payload["name"] = json!("shell");

    read_custom_tool_call(&payload, &root(), &None, &mut seq, &mut out);

    assert!(out.is_empty());
}

#[test]
fn a_payload_that_is_not_a_custom_tool_call_records_nothing() {
    let mut out = Vec::new();
    let mut seq = 0;
    let mut payload = custom_tool_call("call-1", &envelope("a.rs"));
    payload["type"] = json!("message");

    read_custom_tool_call(&payload, &root(), &None, &mut seq, &mut out);

    assert!(out.is_empty());
}

#[test]
fn a_custom_tool_call_whose_input_is_not_a_string_records_nothing() {
    let mut out = Vec::new();
    let mut seq = 0;
    let payload = json!({
        "type": "custom_tool_call",
        "name": "apply_patch",
        "call_id": "call-1",
        "input": { "patch": envelope("a.rs") },
    });

    read_custom_tool_call(&payload, &root(), &None, &mut seq, &mut out);

    assert!(out.is_empty());
}

// -- read_patch_apply_end ---------------------------------------------------

fn patch_apply_end(success: Option<bool>, turn: Option<&str>, changes: Value) -> Value {
    let mut payload = json!({
        "type": "patch_apply_end",
        "call_id": "call-1",
        "changes": changes,
    });
    if let Some(success) = success {
        payload["success"] = json!(success);
    }
    if let Some(turn) = turn {
        payload["turn_id"] = json!(turn);
    }
    payload
}

#[test]
fn a_successful_patch_apply_records_each_change_in_its_map() {
    let mut out = Vec::new();
    let mut seq = 0;

    read_patch_apply_end(
        &patch_apply_end(
            Some(true),
            Some("turn-3"),
            json!({
                "a.rs": { "type": "add", "content": "fresh" },
                "b.rs": { "type": "update", "unified_diff": "@@\n-old\n+new\n" },
            }),
        ),
        &root(),
        &None,
        &mut seq,
        &mut out,
    );

    let mut paths: Vec<&str> = out.iter().map(|r| r.path.as_str()).collect();
    paths.sort();
    assert_eq!(paths, vec!["a.rs", "b.rs"]);
    assert!(out.iter().all(|r| r.turn_id == "codex-history-turn-3"));
}

/// A declined or failed patch never reached the working tree, so it cannot
/// explain anything in the diff.
#[test]
fn a_failed_patch_apply_is_not_recorded() {
    let mut out = Vec::new();
    let mut seq = 0;

    read_patch_apply_end(
        &patch_apply_end(
            Some(false),
            Some("turn-3"),
            json!({ "a.rs": { "type": "add", "content": "fresh" } }),
        ),
        &root(),
        &None,
        &mut seq,
        &mut out,
    );

    assert!(out.is_empty());
}

#[test]
fn a_patch_apply_end_without_a_success_flag_is_treated_as_applied() {
    let mut out = Vec::new();
    let mut seq = 0;

    read_patch_apply_end(
        &patch_apply_end(
            None,
            Some("turn-3"),
            json!({ "a.rs": { "type": "add", "content": "fresh" } }),
        ),
        &root(),
        &None,
        &mut seq,
        &mut out,
    );

    assert_eq!(out.len(), 1);
}

/// Without a turn the card grouping would split one turn's work into a card
/// per call, so the call id stands in.
#[test]
fn an_empty_or_absent_turn_id_falls_back_to_the_call_id() {
    for turn in [None, Some("")] {
        let mut out = Vec::new();
        let mut seq = 0;

        read_patch_apply_end(
            &patch_apply_end(
                Some(true),
                turn,
                json!({ "a.rs": { "type": "add", "content": "fresh" } }),
            ),
            &root(),
            &None,
            &mut seq,
            &mut out,
        );

        assert_eq!(out[0].turn_id, "codex-history-call-1", "turn was {turn:?}");
    }
}

#[test]
fn a_patch_apply_end_with_no_changes_map_records_nothing() {
    let mut out = Vec::new();
    let mut seq = 0;
    let payload = json!({ "type": "patch_apply_end", "call_id": "call-1", "success": true });

    read_patch_apply_end(&payload, &root(), &None, &mut seq, &mut out);

    assert!(out.is_empty());
}

#[test]
fn a_change_the_reader_cannot_understand_does_not_stop_the_others() {
    let mut out = Vec::new();
    let mut seq = 0;

    read_patch_apply_end(
        &patch_apply_end(
            Some(true),
            Some("turn-3"),
            json!({
                "a.rs": { "type": "rename", "to": "c.rs" },
                "b.rs": { "type": "add", "content": "fresh" },
            }),
        ),
        &root(),
        &None,
        &mut seq,
        &mut out,
    );

    assert_eq!(out.len(), 1);
    assert_eq!(out[0].path, "b.rs");
}

// -- session_cwd and session_branch -----------------------------------------

#[test]
fn the_workspace_is_read_from_the_first_line_with_separators_normalised() {
    let dir = workspace();
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(
        &path,
        format!(
            "{}\n{}\n",
            session_meta(&root(), None),
            line("event_msg", json!({ "type": "noise" }))
        ),
    )
    .unwrap();

    let cwd = session_cwd(&path).unwrap();

    assert_eq!(cwd, normalise_path(&root().to_string_lossy()));
    assert!(!cwd.contains('\\'));
}

#[test]
fn a_rollout_whose_first_line_is_not_session_meta_names_no_workspace() {
    let dir = workspace();
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(&path, format!("{}\n", line("event_msg", json!({})))).unwrap();

    assert!(session_cwd(&path).is_none());
}

#[test]
fn an_unreadable_or_unparseable_rollout_names_no_workspace() {
    let dir = workspace();
    let missing = dir.path().join("nothing.jsonl");
    let garbage = dir.path().join("garbage.jsonl");
    std::fs::write(&garbage, "not json at all\n").unwrap();

    assert!(session_cwd(&missing).is_none());
    assert!(session_cwd(&garbage).is_none());
}

#[test]
fn the_branch_is_read_from_the_git_block_of_the_meta_line() {
    let dir = workspace();
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(&path, format!("{}\n", session_meta(&root(), Some("main")))).unwrap();

    assert_eq!(session_branch(&path).as_deref(), Some("main"));
}

#[test]
fn a_session_recorded_outside_a_repository_has_no_branch() {
    let dir = workspace();
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(&path, format!("{}\n", session_meta(&root(), None))).unwrap();

    assert!(session_branch(&path).is_none());
}

// -- read_rollout -----------------------------------------------------------

#[test]
fn a_rollout_is_read_in_both_of_the_shapes_codex_writes() {
    let dir = workspace();
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(
        &path,
        format!(
            "{}\n{}\n{}\n",
            session_meta(&root(), Some("main")),
            line(
                "response_item",
                custom_tool_call("call-1", &envelope("a.rs"))
            ),
            line(
                "event_msg",
                patch_apply_end(
                    Some(true),
                    Some("turn-3"),
                    json!({ "b.rs": { "type": "add", "content": "fresh" } })
                )
            ),
        ),
    )
    .unwrap();

    let mut out = Vec::new();
    let mut seq = 0;
    read_rollout(&path, &root(), &mut seq, &mut out);

    assert_eq!(
        out.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        vec!["a.rs", "b.rs"]
    );
    assert!(
        out.iter().all(|r| r.branch.as_deref() == Some("main")),
        "the session's branch is stamped on both shapes"
    );
}

/// The files are appended to while Codex runs, so a half-written final line
/// must not cost the rest of the session.
#[test]
fn unparseable_and_payloadless_lines_in_a_rollout_are_skipped() {
    let dir = workspace();
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(
        &path,
        format!(
            "{}\n{}\n{}\n{}\n{{\"type\":\"response_item\",\"pay",
            session_meta(&root(), None),
            "this is not json",
            serde_json::to_string(&json!({ "type": "response_item" })).unwrap(),
            line(
                "response_item",
                custom_tool_call("call-1", &envelope("a.rs"))
            ),
        ),
    )
    .unwrap();

    let mut out = Vec::new();
    let mut seq = 0;
    read_rollout(&path, &root(), &mut seq, &mut out);

    assert_eq!(out.len(), 1);
    assert_eq!(out[0].path, "a.rs");
}

#[test]
fn a_rollout_that_cannot_be_read_records_nothing() {
    let dir = workspace();
    let mut out = Vec::new();
    let mut seq = 0;

    read_rollout(&dir.path().join("gone.jsonl"), &root(), &mut seq, &mut out);

    assert!(out.is_empty());
}

// -- detection, against a Codex home under our control ----------------------

#[test]
fn codex_is_not_detected_when_its_home_directory_is_absent() {
    let dir = workspace();

    assert!(!Codex::new().detected_in(Some(&dir.path().join("never-created"))));
    assert!(
        !Codex::new().detected_in(None),
        "no home resolves to no Codex"
    );
}

#[test]
fn codex_is_detected_when_its_home_directory_exists() {
    let dir = workspace();

    assert!(Codex::new().detected_in(Some(dir.path())));
}

#[test]
fn an_undetected_codex_reports_the_absent_status() {
    let dir = workspace();

    let status = Codex::new().status_in(None, dir.path());

    assert_eq!(status, ProviderStatus::absent(ProviderId::Codex));
}

// -- status -----------------------------------------------------------------

/// A Codex home that exists but has nothing configured in it.
fn codex_home_dir() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join("sessions")).unwrap();
    home
}

fn install_hooks(path: &Path, root: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let (content, _) = hooks_json::plan_merge(path, root).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn hooks_inside_the_workspace_are_reported_as_a_project_install() {
    let home = codex_home_dir();
    let dir = workspace();
    install_hooks(&dir.path().join(".codex").join("hooks.json"), dir.path());

    let status = Codex::new().status_in(Some(home.path()), dir.path());

    assert_eq!(status.capture, Some(InstallScope::Project));
}

#[test]
fn hooks_only_in_the_codex_home_are_reported_as_a_user_install() {
    let home = codex_home_dir();
    let dir = workspace();
    install_hooks(&home.path().join("hooks.json"), dir.path());

    let status = Codex::new().status_in(Some(home.path()), dir.path());

    assert_eq!(status.capture, Some(InstallScope::User));
}

#[test]
fn a_workspace_with_no_hooks_anywhere_reports_no_capture() {
    let home = codex_home_dir();
    let dir = workspace();

    let status = Codex::new().status_in(Some(home.path()), dir.path());

    assert!(status.detected);
    assert_eq!(status.capture, None);
    assert!(status.caveats.is_empty(), "got: {:?}", status.caveats);
}

/// The configuration looks right and does nothing — exactly the case that has
/// to be said out loud.
#[test]
fn a_project_install_in_an_untrusted_repository_is_flagged() {
    let home = codex_home_dir();
    let dir = workspace();
    install_hooks(&dir.path().join(".codex").join("hooks.json"), dir.path());

    let status = Codex::new().status_in(Some(home.path()), dir.path());

    assert!(
        status.caveats.iter().any(|c| c.contains("trusted")),
        "got: {:?}",
        status.caveats
    );
}

/// The shape Codex actually writes on Windows: a single-quoted TOML literal
/// holding the path lowercased, with its native separators unescaped.
fn trust(home: &Path, root: &Path) {
    std::fs::write(
        home.join("config.toml"),
        format!(
            "[projects.'{}']\ntrust_level = \"trusted\"\n",
            root.to_string_lossy().to_lowercase()
        ),
    )
    .unwrap();
}

#[test]
fn a_project_install_in_a_trusted_repository_is_not_flagged() {
    let home = codex_home_dir();
    let dir = workspace();
    install_hooks(&dir.path().join(".codex").join("hooks.json"), dir.path());
    trust(home.path(), dir.path());

    let status = Codex::new().status_in(Some(home.path()), dir.path());

    assert!(
        !status.caveats.iter().any(|c| c.contains("trusted")),
        "got: {:?}",
        status.caveats
    );
}

/// Codex will not run a command hook until the user has approved it once, so
/// any install says so.
#[test]
fn any_install_warns_that_codex_asks_the_user_to_review_the_hook() {
    let home = codex_home_dir();
    let dir = workspace();
    install_hooks(&home.path().join("hooks.json"), dir.path());

    let status = Codex::new().status_in(Some(home.path()), dir.path());

    assert!(
        status.caveats.iter().any(|c| c.contains("review")),
        "got: {:?}",
        status.caveats
    );
}

#[test]
fn compressed_sessions_are_counted_rather_than_decompressed() {
    let home = codex_home_dir();
    let dir = workspace();
    let day = home.path().join("sessions").join("2026").join("08");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(day.join("rollout-old.jsonl.zst"), b"\x28\xb5\x2f\xfd").unwrap();

    let status = Codex::new().status_in(Some(home.path()), dir.path());

    assert!(
        status.caveats.iter().any(|c| c.contains("1 older session")),
        "got: {:?}",
        status.caveats
    );
}

#[test]
fn status_counts_only_the_sessions_recorded_in_this_workspace() {
    let home = codex_home_dir();
    let dir = workspace();
    let other = workspace();
    write_rollout(
        home.path(),
        "rollout-mine.jsonl",
        &[session_meta(dir.path(), Some("main"))],
    );
    write_rollout(
        home.path(),
        "rollout-theirs.jsonl",
        &[session_meta(other.path(), None)],
    );

    let status = Codex::new().status_in(Some(home.path()), dir.path());

    assert_eq!(status.sessions, 1);
}

// -- is_trusted -------------------------------------------------------------

#[test]
fn a_missing_config_file_means_the_project_is_not_trusted() {
    let home = codex_home_dir();
    let dir = workspace();

    assert!(!is_trusted_in(Some(home.path()), dir.path()));
}

#[test]
fn trust_recorded_for_another_project_does_not_apply_to_this_one() {
    let home = codex_home_dir();
    let dir = workspace();
    let other = workspace();
    trust(home.path(), other.path());

    assert!(!is_trusted_in(Some(home.path()), dir.path()));
}

/// The key is a quoted path whose separators and case vary between what Codex
/// wrote and how the workspace was opened.
#[test]
fn a_trusted_path_matches_whatever_separators_and_case_it_was_written_with() {
    let home = codex_home_dir();
    let dir = workspace();
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            "[projects.\"{}\"]\ntrust_level  =  \"trusted\"\n",
            normalise_path(&dir.path().to_string_lossy()).to_uppercase()
        ),
    )
    .unwrap();

    assert!(is_trusted_in(Some(home.path()), dir.path()));
}

/// Characterises a limitation rather than endorsing it. Codex writes the key
/// as a single-quoted TOML literal, which is what the text-level match is
/// built for. A hand-edited config using a *basic* string escapes each
/// separator, and `\\` normalises to `//`, which no workspace path contains —
/// so the project reads as untrusted. Abstaining is the safe direction here:
/// the caveat is shown when it need not have been.
#[test]
fn a_backslash_escaped_trust_key_is_not_recognised() {
    let home = codex_home_dir();
    let dir = workspace();
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            "[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
            dir.path().to_string_lossy().replace('\\', "\\\\")
        ),
    )
    .unwrap();

    assert_eq!(is_trusted_in(Some(home.path()), dir.path()), !cfg!(windows));
}

/// A later section ends the project's, so its keys must not be read as if they
/// were still inside it.
#[test]
fn a_trust_level_after_the_projects_section_closed_is_not_read_as_this_ones() {
    let home = codex_home_dir();
    let dir = workspace();
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            "[projects.\"{}\"]\napproval_policy = \"on-request\"\n\n[tui]\ntrust_level = \
             \"trusted\"\n",
            normalise_path(&dir.path().to_string_lossy())
        ),
    )
    .unwrap();

    assert!(!is_trusted_in(Some(home.path()), dir.path()));
}

// -- install plans ----------------------------------------------------------

#[test]
fn a_user_scope_install_targets_the_hooks_file_in_the_codex_home() {
    let home = codex_home_dir();
    let dir = workspace();

    let plan = Codex::new()
        .install_plan_in(Some(home.path()), dir.path(), InstallScope::User)
        .unwrap();

    assert_eq!(plan.writes[0].path, home.path().join("hooks.json"));
    assert!(
        plan.caveats.iter().any(|c| c.contains("every repository")),
        "got: {:?}",
        plan.caveats
    );
}

#[test]
fn a_user_scope_install_without_a_codex_home_is_an_error() {
    let dir = workspace();

    let error = Codex::new()
        .install_plan_in(None, dir.path(), InstallScope::User)
        .unwrap_err()
        .to_string();

    assert!(error.contains("Codex home"), "got: {error}");
}

#[test]
fn a_project_install_warns_about_trust_only_while_the_repository_is_untrusted() {
    let home = codex_home_dir();
    let dir = workspace();

    let untrusted = Codex::new()
        .install_plan_in(Some(home.path()), dir.path(), InstallScope::Project)
        .unwrap();
    trust(home.path(), dir.path());
    let trusted = Codex::new()
        .install_plan_in(Some(home.path()), dir.path(), InstallScope::Project)
        .unwrap();

    assert!(untrusted
        .caveats
        .iter()
        .any(|c| c.contains("not yet trusted")));
    assert!(!trusted
        .caveats
        .iter()
        .any(|c| c.contains("not yet trusted")));
}

#[test]
fn a_plan_that_merges_an_existing_file_says_a_backup_will_be_written() {
    let home = codex_home_dir();
    let dir = workspace();
    install_hooks(&home.path().join("hooks.json"), dir.path());

    let plan = Codex::new()
        .install_plan_in(Some(home.path()), dir.path(), InstallScope::User)
        .unwrap();

    assert!(plan.writes[0].merges_existing);
    assert!(
        plan.caveats.iter().any(|c| c.contains("backup")),
        "got: {:?}",
        plan.caveats
    );
}

// -- history ----------------------------------------------------------------

#[test]
fn history_reads_the_edits_of_every_session_recorded_in_this_workspace() {
    let home = codex_home_dir();
    let dir = workspace();
    write_rollout(
        home.path(),
        "rollout-1.jsonl",
        &[
            session_meta(dir.path(), Some("main")),
            line(
                "response_item",
                custom_tool_call("call-1", &envelope("a.rs")),
            ),
        ],
    );

    let (records, labels) = Codex::new()
        .history_in(Some(home.path()), dir.path())
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].path, "a.rs");
    assert_eq!(records[0].branch.as_deref(), Some("main"));
    assert!(
        labels.is_empty(),
        "a rollout carries no stated intent to label with"
    );
}

/// The session files of every project share one directory tree, so the wrong
/// workspace's edits are the failure mode to guard against.
#[test]
fn history_ignores_sessions_recorded_in_another_workspace() {
    let home = codex_home_dir();
    let dir = workspace();
    let other = workspace();
    write_rollout(
        home.path(),
        "rollout-theirs.jsonl",
        &[
            session_meta(other.path(), None),
            line(
                "response_item",
                custom_tool_call("call-1", &envelope("a.rs")),
            ),
        ],
    );

    let (records, _) = Codex::new()
        .history_in(Some(home.path()), dir.path())
        .unwrap();

    assert!(records.is_empty());
}

#[test]
fn history_numbers_records_across_sessions_without_repeating_a_sequence() {
    let home = codex_home_dir();
    let dir = workspace();
    for (name, call) in [("rollout-1.jsonl", "call-1"), ("rollout-2.jsonl", "call-2")] {
        write_rollout(
            home.path(),
            name,
            &[
                session_meta(dir.path(), None),
                line("response_item", custom_tool_call(call, &envelope("a.rs"))),
            ],
        );
    }

    let (records, _) = Codex::new()
        .history_in(Some(home.path()), dir.path())
        .unwrap();

    assert_eq!(records.len(), 2);
    assert_eq!(
        records.iter().map(|r| r.seq).collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn history_without_a_codex_home_is_empty_rather_than_an_error() {
    let dir = workspace();

    let (records, labels) = Codex::new().history_in(None, dir.path()).unwrap();

    assert!(records.is_empty());
    assert!(labels.is_empty());
}
