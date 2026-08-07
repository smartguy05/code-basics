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
