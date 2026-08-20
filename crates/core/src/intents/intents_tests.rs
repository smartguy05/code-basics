//! Tests for recorded agent intent. Included by `mod.rs` under `#[cfg(test)]`.

use super::*;

fn workspace() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn record(seq: u64, path: &str, old: &[&str], new: &[&str]) -> IntentRecord {
    IntentRecord {
        provider: ProviderId::ClaudeCode,
        turn_id: "turn-1".into(),
        tool_use_id: format!("tool-{seq}"),
        seq,
        path: path.into(),
        edit: IntentEdit {
            old_lines: old.iter().map(|s| s.to_string()).collect(),
            new_lines: new.iter().map(|s| s.to_string()).collect(),
            whole_file: false,
        },
        branch: None,
    }
}

#[test]
fn a_workspace_with_no_recording_has_no_intent() {
    let dir = workspace();

    let intents = load(dir.path(), &LoadOptions::default()).unwrap();

    assert!(intents.is_empty());
    assert!(intents.labels.is_empty());
}

#[test]
fn records_round_trip_through_the_file() {
    let dir = workspace();
    append_edit(dir.path(), &record(0, "src/a.rs", &["old"], &["new"])).unwrap();

    let intents = load(dir.path(), &LoadOptions::default()).unwrap();

    assert_eq!(intents.records.len(), 1);
    assert_eq!(intents.records[0].path, "src/a.rs");
    assert_eq!(intents.records[0].edit.new_lines, vec!["new"]);
}

#[test]
fn recording_creates_the_gitignore_entry() {
    let dir = workspace();
    append_edit(dir.path(), &record(0, "a.rs", &[], &["x"])).unwrap();

    let ignore =
        std::fs::read_to_string(crate::config::config_dir(dir.path()).join(".gitignore")).unwrap();

    assert!(
        ignore.lines().any(|l| l.trim() == "intents/"),
        "got: {ignore}"
    );
}

#[test]
fn records_are_returned_in_sequence_order() {
    let dir = workspace();
    append_edit(dir.path(), &record(2, "a.rs", &[], &["third"])).unwrap();
    append_edit(dir.path(), &record(0, "a.rs", &[], &["first"])).unwrap();
    append_edit(dir.path(), &record(1, "a.rs", &[], &["second"])).unwrap();

    let intents = load(dir.path(), &LoadOptions::default()).unwrap();

    let seqs: Vec<u64> = intents.records.iter().map(|r| r.seq).collect();
    assert_eq!(seqs, vec![0, 1, 2]);
}

fn prompt(turn: &str, text: &str) -> IntentPrompt {
    IntentPrompt {
        provider: ProviderId::ClaudeCode,
        turn_id: turn.into(),
        prompt: text.into(),
    }
}

#[test]
fn prompts_round_trip_and_join_by_turn() {
    let dir = workspace();
    append_prompt(
        dir.path(),
        &prompt("turn-7", "add exponential backoff, cap at 5"),
    )
    .unwrap();
    append_prompt(dir.path(), &prompt("turn-8", "unrelated request")).unwrap();

    let prompts = load_prompts(dir.path()).unwrap();

    assert_eq!(prompts.len(), 2);
    assert_eq!(
        prompt_for(&prompts, "turn-7"),
        Some("add exponential backoff, cap at 5")
    );
    // A turn with no recorded prompt joins to nothing.
    assert_eq!(prompt_for(&prompts, "turn-missing"), None);
}

#[test]
fn an_intent_prompt_serialises_with_the_keys_the_note_reads() {
    let value = serde_json::to_value(prompt("t", "why")).unwrap();
    let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["prompt", "provider", "turnId"]);
}

#[test]
fn clear_also_removes_the_prompts_file() {
    let dir = workspace();
    append_prompt(dir.path(), &prompt("t", "p")).unwrap();
    assert!(prompts_path(dir.path()).exists());

    clear(dir.path()).unwrap();

    assert!(!prompts_path(dir.path()).exists());
}

/// The same edit can be seen by a hook and again by a history sweep.
#[test]
fn the_same_edit_seen_twice_is_deduplicated_on_tool_use_id() {
    let dir = workspace();
    let first = record(0, "a.rs", &[], &["x"]);
    let mut second = first.clone();
    second.seq = 5;

    append_edit(dir.path(), &first).unwrap();
    append_edit(dir.path(), &second).unwrap();

    let intents = load(dir.path(), &LoadOptions::default()).unwrap();

    assert_eq!(intents.records.len(), 1);
}

#[test]
fn an_edit_that_changes_nothing_is_dropped() {
    let dir = workspace();
    append_edit(dir.path(), &record(0, "a.rs", &[], &[])).unwrap();

    assert!(load(dir.path(), &LoadOptions::default())
        .unwrap()
        .is_empty());
}

/// A half-written final line after a crash must not cost the rest of the log.
#[test]
fn a_malformed_line_is_skipped_rather_than_failing_the_load() {
    let dir = workspace();
    append_edit(dir.path(), &record(0, "a.rs", &[], &["good"])).unwrap();
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(edits_path(dir.path()))
            .unwrap();
        writeln!(f, "{{\"provider\": truncated").unwrap();
    }

    let intents = load(dir.path(), &LoadOptions::default()).unwrap();

    assert_eq!(intents.records.len(), 1);
}

#[test]
fn records_from_another_branch_are_discarded() {
    let dir = workspace();
    let mut mine = record(0, "a.rs", &[], &["mine"]);
    mine.branch = Some("feature".into());
    let mut theirs = record(1, "a.rs", &[], &["theirs"]);
    theirs.branch = Some("main".into());

    append_edit(dir.path(), &mine).unwrap();
    append_edit(dir.path(), &theirs).unwrap();

    let options = LoadOptions {
        branch: Some("feature".into()),
    };
    let intents = load(dir.path(), &options).unwrap();

    assert_eq!(intents.records.len(), 1);
    assert_eq!(intents.records[0].edit.new_lines, vec!["mine"]);
}

/// A record that never recorded a branch is kept: it is more useful than
/// nothing, and dropping it would lose all history written before branches
/// were captured.
#[test]
fn a_record_with_no_branch_survives_branch_filtering() {
    let dir = workspace();
    append_edit(dir.path(), &record(0, "a.rs", &[], &["x"])).unwrap();

    let options = LoadOptions {
        branch: Some("main".into()),
    };

    assert_eq!(load(dir.path(), &options).unwrap().records.len(), 1);
}

#[test]
fn an_absolute_path_is_made_relative_to_the_workspace() {
    let dir = workspace();
    let absolute = dir.path().join("src").join("a.rs");
    append_edit(
        dir.path(),
        &record(0, &absolute.to_string_lossy(), &[], &["x"]),
    )
    .unwrap();

    let intents = load(dir.path(), &LoadOptions::default()).unwrap();

    assert_eq!(intents.records[0].path, "src/a.rs");
}

#[test]
fn an_edit_outside_the_workspace_is_discarded() {
    let dir = workspace();
    let outside = if cfg!(windows) {
        "C:/elsewhere/a.rs"
    } else {
        "/elsewhere/a.rs"
    };
    append_edit(dir.path(), &record(0, outside, &[], &["x"])).unwrap();

    assert!(load(dir.path(), &LoadOptions::default())
        .unwrap()
        .is_empty());
}

#[test]
fn backslash_paths_are_normalised_to_forward_slashes() {
    let dir = workspace();
    append_edit(dir.path(), &record(0, "src\\nested\\a.rs", &[], &["x"])).unwrap();

    let intents = load(dir.path(), &LoadOptions::default()).unwrap();

    assert_eq!(intents.records[0].path, "src/nested/a.rs");
}

#[test]
fn next_seq_continues_after_the_highest_recorded() {
    let dir = workspace();
    assert_eq!(next_seq(dir.path()), 0);

    append_edit(dir.path(), &record(0, "a.rs", &[], &["x"])).unwrap();
    append_edit(dir.path(), &record(7, "a.rs", &[], &["y"])).unwrap();

    assert_eq!(next_seq(dir.path()), 8);
}

// -- joining labels to edits ------------------------------------------------

fn label(turn: &str, text: &str, paths: &[&str]) -> IntentLabel {
    IntentLabel {
        provider: ProviderId::ClaudeCode,
        turn_id: turn.into(),
        label: text.into(),
        paths: paths.iter().map(|s| s.to_string()).collect(),
        anchor: None,
        source: LabelSource::Declared,
    }
}

#[test]
fn a_label_naming_the_file_is_preferred_over_a_turn_wide_one() {
    let intents = Intents {
        records: vec![record(0, "src/a.rs", &[], &["x"])],
        labels: vec![
            label("turn-1", "whole turn", &[]),
            label("turn-1", "just this file", &["src/a.rs"]),
        ],
    };

    let found = intents.label_for(&intents.records[0]).unwrap();

    assert_eq!(found.label, "just this file");
}

#[test]
fn scoped_labels_for_path_returns_all_covering_declared_labels() {
    let intents = Intents {
        records: Vec::new(),
        labels: vec![
            label("a", "reason A", &["dir"]),         // covers by directory
            label("b", "reason B", &["dir/file.rs"]), // covers by exact path
            label("c", "elsewhere", &["other"]),      // does not cover
            IntentLabel {
                source: LabelSource::Inferred,
                ..label("d", "inferred", &["dir"])
            }, // inferred: never claims
        ],
    };

    let got: Vec<&str> = intents
        .scoped_labels_for_path("dir/file.rs")
        .iter()
        .map(|l| l.label.as_str())
        .collect();

    assert_eq!(got, vec!["reason A", "reason B"]);
    // Unique cross-turn binding abstains when more than one covers.
    assert!(intents
        .effective_scoped_label(&record(0, "dir/file.rs", &[], &["x"]), "dir/file.rs")
        .is_none());
}

#[test]
fn scoped_labels_for_path_dedupes_same_turn_and_text() {
    let intents = Intents {
        records: Vec::new(),
        labels: vec![
            label("t", "same", &["dir"]),
            label("t", "same", &["dir/f.rs"]),
        ],
    };
    assert_eq!(intents.scoped_labels_for_path("dir/f.rs").len(), 1);
}

/// An agent that described its turn once still explains every edit in it.
#[test]
fn a_turn_wide_label_covers_a_file_it_does_not_name() {
    let intents = Intents {
        records: vec![record(0, "src/a.rs", &[], &["x"])],
        labels: vec![label("turn-1", "whole turn", &[])],
    };

    assert_eq!(
        intents.label_for(&intents.records[0]).unwrap().label,
        "whole turn"
    );
}

#[test]
fn a_label_from_another_turn_never_applies() {
    let intents = Intents {
        records: vec![record(0, "src/a.rs", &[], &["x"])],
        labels: vec![label("turn-2", "different turn", &["src/a.rs"])],
    };

    assert!(intents.label_for(&intents.records[0]).is_none());
}

#[test]
fn a_label_path_written_with_backslashes_still_matches() {
    let intents = Intents {
        records: vec![record(0, "src/a.rs", &[], &["x"])],
        labels: vec![label("turn-1", "matched", &["src\\a.rs"])],
    };

    assert_eq!(
        intents.label_for(&intents.records[0]).unwrap().label,
        "matched"
    );
}

/// A scope naming a directory covers every file edited beneath it — the shape
/// an agent uses to group a cohesive set of edits under one project folder.
#[test]
fn a_directory_label_covers_a_file_beneath_it() {
    let intents = Intents {
        records: vec![record(
            0,
            "ONEflight.Client.OPS135.Components/Pages/Reports/Trips/FlightsReportPage.razor",
            &[],
            &["x"],
        )],
        labels: vec![label(
            "turn-1",
            "cancel superseded table reads",
            &["ONEflight.Client.OPS135.Components"],
        )],
    };

    assert_eq!(
        intents.label_for(&intents.records[0]).unwrap().label,
        "cancel superseded table reads"
    );
}

/// A directory scope stops at its own boundary: a file outside it is not
/// covered, and because the scope is non-empty it cannot serve as the turn-wide
/// fallback either, so the file goes unlabelled rather than mislabelled.
#[test]
fn a_directory_label_does_not_cover_a_file_outside_it() {
    let intents = Intents {
        records: vec![record(
            0,
            ".memories/work-items/19778/completed.md",
            &[],
            &["x"],
        )],
        labels: vec![label(
            "turn-1",
            "cancel superseded table reads",
            &["ONEflight.Client.OPS135.Components"],
        )],
    };

    assert!(intents.label_for(&intents.records[0]).is_none());
}

/// The prefix match is at a path segment, so `foo` never covers `foobar/x`.
#[test]
fn a_directory_label_does_not_cover_a_sibling_prefix() {
    let intents = Intents {
        records: vec![record(0, "foobar/x.rs", &[], &["x"])],
        labels: vec![label("turn-1", "in foo", &["foo"])],
    };

    assert!(intents.label_for(&intents.records[0]).is_none());
}

#[test]
fn a_directory_label_written_with_backslashes_still_matches() {
    let intents = Intents {
        records: vec![record(0, "dir/sub/f.rs", &[], &["x"])],
        labels: vec![label("turn-1", "matched", &["dir\\sub"])],
    };

    assert_eq!(
        intents.label_for(&intents.records[0]).unwrap().label,
        "matched"
    );
}

// -- effective (cross-turn) label resolution --------------------------------

/// A record whose own turn owns a covering label resolves to it, exactly as
/// `label_for` does — same-turn keeps top priority, so nothing existing changes.
#[test]
fn effective_label_prefers_the_same_turn_label() {
    let mut r = record(0, "src/a.rs", &[], &["x"]);
    r.turn_id = "turn-1".into();
    let intents = Intents {
        records: vec![r],
        labels: vec![
            label("turn-1", "same turn", &["src/a.rs"]),
            label("turn-2", "other turn", &["src/a.rs"]),
        ],
    };

    assert_eq!(
        intents
            .effective_scoped_label(&intents.records[0], "src/a.rs")
            .unwrap()
            .label,
        "same turn"
    );
}

/// An orphan record (no same-turn label) binds to a declared, path-scoped label
/// from another turn when exactly one such label covers the file.
#[test]
fn effective_label_binds_a_path_scoped_label_from_another_turn() {
    let mut r = record(0, "src/a.rs", &[], &["x"]);
    r.turn_id = "turnA".into();
    let intents = Intents {
        records: vec![r],
        labels: vec![label("turnB", "declared reason", &["src/a.rs"])],
    };

    assert_eq!(
        intents
            .effective_scoped_label(&intents.records[0], "src/a.rs")
            .unwrap()
            .label,
        "declared reason"
    );
}

/// A directory-scoped declared label from another turn covers a file beneath it.
#[test]
fn effective_label_binds_a_directory_scope_from_another_turn() {
    let mut r = record(0, "src/components/x.ts", &[], &["x"]);
    r.turn_id = "turnA".into();
    let intents = Intents {
        records: vec![r],
        labels: vec![label("turnB", "panel work", &["src/components"])],
    };

    assert_eq!(
        intents
            .effective_scoped_label(&intents.records[0], "src/components/x.ts")
            .unwrap()
            .label,
        "panel work"
    );
}

/// Two declared labels from different turns both cover the file: ambiguous, so
/// it abstains rather than guess.
#[test]
fn effective_label_abstains_when_two_cross_turn_labels_cover_the_file() {
    let mut r = record(0, "src/a.rs", &[], &["x"]);
    r.turn_id = "turnA".into();
    let intents = Intents {
        records: vec![r],
        labels: vec![
            label("turnB", "first", &["src/a.rs"]),
            label("turnC", "second", &["src/a.rs"]),
        ],
    };

    assert!(intents
        .effective_scoped_label(&intents.records[0], "src/a.rs")
        .is_none());
}

/// A cross-turn label mined from prose is a guess; it never binds across turns.
#[test]
fn effective_label_never_crosses_a_turn_for_an_inferred_label() {
    let mut r = record(0, "src/a.rs", &[], &["x"]);
    r.turn_id = "turnA".into();
    let intents = Intents {
        records: vec![r],
        labels: vec![IntentLabel {
            provider: ProviderId::ClaudeCode,
            turn_id: "turnB".into(),
            label: "a mined sentence".into(),
            paths: vec!["src/a.rs".into()],
            anchor: None,
            source: LabelSource::Inferred,
        }],
    };

    assert!(intents
        .effective_scoped_label(&intents.records[0], "src/a.rs")
        .is_none());
}

/// A turn-wide (empty-`paths`) label from another turn never binds here — a
/// bare reason bridges only through the diff-level single-orphan pass.
#[test]
fn effective_label_does_not_bind_an_empty_paths_label_across_turns() {
    let mut r = record(0, "src/a.rs", &[], &["x"]);
    r.turn_id = "turnA".into();
    let intents = Intents {
        records: vec![r],
        labels: vec![label("turnB", "turn-wide reason", &[])],
    };

    assert!(intents
        .effective_scoped_label(&intents.records[0], "src/a.rs")
        .is_none());
}

#[test]
fn records_can_be_looked_up_by_path_in_sequence_order() {
    let intents = Intents {
        records: vec![
            record(1, "b.rs", &[], &["b"]),
            record(2, "a.rs", &[], &["second"]),
            record(0, "a.rs", &[], &["first"]),
        ],
        labels: Vec::new(),
    };

    let found = intents.for_path("a.rs");

    assert_eq!(found.len(), 2);
    assert_eq!(found[0].seq, 0);
    assert_eq!(found[1].seq, 2);
}

#[test]
fn clearing_removes_everything_recorded() {
    let dir = workspace();
    append_edit(dir.path(), &record(0, "a.rs", &[], &["x"])).unwrap();
    append_label(dir.path(), &label("turn-1", "why", &[])).unwrap();

    clear(dir.path()).unwrap();

    assert!(load(dir.path(), &LoadOptions::default())
        .unwrap()
        .is_empty());
}

#[test]
fn clearing_a_workspace_that_recorded_nothing_is_not_an_error() {
    let dir = workspace();

    assert!(clear(dir.path()).is_ok());
}

// -- path helpers, called directly -------------------------------------------

#[test]
fn normalising_a_path_only_touches_separators() {
    assert_eq!(normalise_path("src\\a\\b.rs"), "src/a/b.rs");
    assert_eq!(normalise_path("src/a/b.rs"), "src/a/b.rs");
    // Case, spaces and dots are content, not separators.
    assert_eq!(normalise_path("Src/My File.rs"), "Src/My File.rs");
    assert_eq!(normalise_path(""), "");
}

#[test]
fn a_path_already_relative_is_accepted_unchanged() {
    let root = Path::new(if cfg!(windows) { "C:/repo" } else { "/repo" });

    assert_eq!(relative_to(root, "src/a.rs"), Some("src/a.rs".to_string()));
}

#[test]
fn a_leading_dot_slash_is_stripped_from_a_relative_path() {
    let root = Path::new(if cfg!(windows) { "C:/repo" } else { "/repo" });

    assert_eq!(
        relative_to(root, "./src/a.rs"),
        Some("src/a.rs".to_string())
    );
}

/// A relative path that climbs out names a file this workspace does not own.
#[test]
fn a_relative_path_that_climbs_out_of_the_workspace_is_rejected() {
    let root = Path::new(if cfg!(windows) { "C:/repo" } else { "/repo" });

    assert_eq!(relative_to(root, "../other/a.rs"), None);
}

#[test]
fn the_workspace_root_itself_is_not_a_file_inside_it() {
    let root = Path::new(if cfg!(windows) { "C:/repo" } else { "/repo" });

    // Stripping must require a separator after the root, or `/repository/a.rs`
    // would be read as a file in `/repo`.
    assert_eq!(
        relative_to(root, if cfg!(windows) { "C:/repo" } else { "/repo" }),
        None
    );
    assert_eq!(
        relative_to(
            root,
            if cfg!(windows) {
                "C:/repository/a.rs"
            } else {
                "/repository/a.rs"
            }
        ),
        None
    );
}

#[test]
fn a_trailing_separator_on_the_root_does_not_change_the_result() {
    let with = Path::new(if cfg!(windows) { "C:/repo/" } else { "/repo/" });
    let without = Path::new(if cfg!(windows) { "C:/repo" } else { "/repo" });
    let file = if cfg!(windows) {
        "C:/repo/src/a.rs"
    } else {
        "/repo/src/a.rs"
    };

    assert_eq!(relative_to(with, file), Some("src/a.rs".to_string()));
    assert_eq!(relative_to(with, file), relative_to(without, file));
}

/// On Windows the agent and the opened workspace legitimately disagree on case.
#[cfg(windows)]
#[test]
fn a_windows_root_matches_case_insensitively() {
    assert_eq!(
        relative_to(Path::new("C:/Repo"), "c:\\repo\\src\\a.rs"),
        Some("src/a.rs".to_string())
    );
}

#[test]
fn recorded_intent_lives_under_the_config_directory() {
    let root = Path::new("/repo");

    assert_eq!(
        intents_dir(root),
        crate::config::config_dir(root).join("intents")
    );
    assert_eq!(edits_path(root), intents_dir(root).join("edits.jsonl"));
    assert_eq!(labels_path(root), intents_dir(root).join("labels.jsonl"));

    // Both logs share one directory, which is what makes `clear` a directory
    // listing rather than a list of names kept in step by hand.
    assert_eq!(edits_path(root).parent(), labels_path(root).parent());
}

#[test]
fn labels_are_kept_separately_from_edits() {
    let dir = workspace();
    append_edit(dir.path(), &record(0, "a.rs", &[], &["x"])).unwrap();
    append_label(dir.path(), &label("turn-1", "why", &[])).unwrap();

    // The two hooks fire at different times and append independently; sharing
    // one file would interleave them.
    assert_ne!(edits_path(dir.path()), labels_path(dir.path()));
    assert!(labels_path(dir.path()).exists());
    assert_eq!(
        std::fs::read_to_string(labels_path(dir.path()))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

/// The gate at the point of *recording* only helps future turns. A workspace
/// that has been capturing for weeks already holds narration that predates it,
/// and those are exactly the labels titling cards today — so the gate has to
/// run on the way out as well.
#[test]
fn an_inferred_label_that_reads_as_narration_is_dropped_on_load() {
    let dir = workspace();
    append_edit(dir.path(), &record(0, "a.rs", &[], &["x"])).unwrap();
    append_label(
        dir.path(),
        &IntentLabel {
            provider: ProviderId::ClaudeCode,
            turn_id: "turn-1".into(),
            label: "Workflow running in the background".into(),
            paths: Vec::new(),
            anchor: None,
            source: LabelSource::Inferred,
        },
    )
    .unwrap();

    let intents = load(dir.path(), &LoadOptions::default()).unwrap();

    assert!(intents.labels.is_empty());
}

/// A declared label is the agent's own words and is never second-guessed, on
/// the way in or the way out.
#[test]
fn a_declared_label_survives_load_whatever_it_says() {
    let dir = workspace();
    append_edit(dir.path(), &record(0, "a.rs", &[], &["x"])).unwrap();
    append_label(
        dir.path(),
        &IntentLabel {
            provider: ProviderId::ClaudeCode,
            turn_id: "turn-1".into(),
            label: "let me run the workflow with Opus".into(),
            paths: Vec::new(),
            anchor: None,
            source: LabelSource::Declared,
        },
    )
    .unwrap();

    let intents = load(dir.path(), &LoadOptions::default()).unwrap();

    assert_eq!(intents.labels.len(), 1);
}

/// A handful of words is not a reason. "Running" was titling a card.
#[test]
fn an_inferred_label_too_short_to_say_anything_is_dropped_on_load() {
    let dir = workspace();
    append_edit(dir.path(), &record(0, "a.rs", &[], &["x"])).unwrap();
    append_label(
        dir.path(),
        &IntentLabel {
            provider: ProviderId::ClaudeCode,
            turn_id: "turn-1".into(),
            label: "Running".into(),
            paths: Vec::new(),
            anchor: None,
            source: LabelSource::Inferred,
        },
    )
    .unwrap();

    assert!(load(dir.path(), &LoadOptions::default())
        .unwrap()
        .labels
        .is_empty());
}

// --- importing history ------------------------------------------------------

#[test]
fn rebasing_nothing_leaves_the_base_alone() {
    let mut records: Vec<IntentRecord> = Vec::new();

    // An import that found no history must not advance the workspace's
    // numbering, or the next real edit would leave a gap for no reason.
    assert_eq!(rebase_seqs(&mut records, 7), 7);
    assert!(records.is_empty());
}

#[test]
fn rebasing_preserves_the_order_records_were_recorded_in() {
    let mut records = vec![
        record(1, "a.rs", &[], &["x"]),
        record(2, "b.rs", &[], &["y"]),
        record(3, "c.rs", &[], &["z"]),
    ];

    rebase_seqs(&mut records, 10);

    // Absolute values are an implementation detail; the ordering is not —
    // attribution resolves a contested line in favour of the later edit.
    let seqs: Vec<u64> = records.iter().map(|r| r.seq).collect();
    assert!(
        seqs.windows(2).all(|w| w[0] < w[1]),
        "order was not preserved: {seqs:?}"
    );
    assert!(seqs[0] > 10, "imported records must sit above the base");
    assert_eq!(records[0].path, "a.rs");
    assert_eq!(records[2].path, "c.rs");
}

#[test]
fn the_base_advances_to_the_highest_seq_not_the_last() {
    let mut records = vec![
        record(7, "a.rs", &[], &["x"]),
        record(0, "b.rs", &[], &["y"]),
    ];

    // A provider can hand back a record whose own seq is lower than the one
    // before it. The base still only ever moves forwards.
    let next = rebase_seqs(&mut records, 0);

    let highest = records.iter().map(|r| r.seq).max().unwrap();
    assert_eq!(next, highest);
    assert!(next >= records.last().unwrap().seq);
}

#[test]
fn a_second_import_cannot_collide_with_the_first() {
    let dir = workspace();

    let mut first = vec![
        record(0, "a.rs", &[], &["x"]),
        record(1, "b.rs", &[], &["y"]),
    ];
    rebase_seqs(&mut first, next_seq(dir.path()));
    for r in &first {
        append_edit(dir.path(), r).unwrap();
    }

    // The same history, imported again: deduplication is by tool id, but the
    // numbering must not overlap even so.
    let mut second = vec![
        record(0, "c.rs", &[], &["x"]),
        record(1, "d.rs", &[], &["y"]),
    ];
    rebase_seqs(&mut second, next_seq(dir.path()));

    let earlier: Vec<u64> = first.iter().map(|r| r.seq).collect();
    for r in &second {
        assert!(
            !earlier.contains(&r.seq),
            "seq {} collides with {earlier:?}",
            r.seq
        );
        assert!(r.seq > *earlier.iter().max().unwrap());
    }
}

#[test]
fn rebasing_from_zero_leaves_the_first_record_alone() {
    let mut records = vec![
        record(0, "a.rs", &[], &["x"]),
        record(1, "b.rs", &[], &["y"]),
        record(2, "c.rs", &[], &["z"]),
    ];

    rebase_seqs(&mut records, 0);

    // Characterising, not endorsing: the base is carried forward from each
    // record as it is rebased, so seqs accumulate rather than being shifted by
    // a constant. Only the first record is genuinely untouched at base zero.
    // Order — the only thing seq is read for — survives either way.
    assert_eq!(
        records.iter().map(|r| r.seq).collect::<Vec<_>>(),
        vec![0, 1, 3]
    );
}
