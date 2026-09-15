//! Tests for retiring intents a commit has absorbed. Included by `retire.rs`
//! under `#[cfg(test)]`.

use super::*;
use crate::intents::{IntentEdit, IntentLabel, IntentRecord, LabelSource, ProviderId};

fn rec(tool: &str, path: &str, new_lines: &[&str], old_lines: &[&str]) -> IntentRecord {
    IntentRecord {
        provider: ProviderId::ClaudeCode,
        turn_id: "turn-1".into(),
        tool_use_id: tool.into(),
        seq: 1,
        path: path.into(),
        edit: IntentEdit {
            old_lines: old_lines.iter().map(|s| s.to_string()).collect(),
            new_lines: new_lines.iter().map(|s| s.to_string()).collect(),
            whole_file: false,
        },
        branch: None,
    }
}

fn snap(path: &str, head: Option<&str>, added: &[&str], removed: &[&str]) -> FileSnapshot {
    FileSnapshot {
        path: path.into(),
        head_blob: head.map(|s| s.to_string()),
        working_added: added.iter().map(|s| s.to_string()).collect(),
        working_removed: removed.iter().map(|s| s.to_string()).collect(),
        in_working_diff: !added.is_empty() || !removed.is_empty(),
        readable: true,
    }
}

const A: &str = "    let retry_budget = compute_retry_budget(config);";
const B: &str = "    let backoff = ExponentialBackoff::new(retry_budget);";

#[test]
fn a_record_whose_lines_are_all_in_head_and_none_in_the_working_diff_is_retired() {
    let head = format!("fn main() {{\n{A}\n{B}\n}}\n");
    let s = snap("a.rs", Some(&head), &[], &[]);
    assert_eq!(
        verdict(&rec("t:0", "a.rs", &[A, B], &[]), &s),
        Verdict::Retire
    );
}

#[test]
fn a_record_with_any_line_still_in_the_working_diff_is_kept() {
    // Half the card was committed; the other half is still uncommitted. Losing
    // the label for the remaining hunk is the outcome this must never allow.
    let head = format!("fn main() {{\n{A}\n}}\n");
    let s = snap("a.rs", Some(&head), &[B], &[]);
    assert_eq!(
        verdict(&rec("t:0", "a.rs", &[A, B], &[]), &s),
        Verdict::Keep(KeepReason::StillInWorkingTree)
    );
}

#[test]
fn a_record_absent_from_the_working_diff_but_not_in_head_is_kept() {
    // The branch-switch guard: this code is not in *this* HEAD, so nothing here
    // absorbed it and it must not be retired.
    let s = snap("a.rs", Some("fn main() {}\n"), &[], &[]);
    assert_eq!(
        verdict(&rec("t:0", "a.rs", &[A, B], &[]), &s),
        Verdict::Keep(KeepReason::NotInHead)
    );
}

#[test]
fn a_deletion_only_record_is_retired_when_its_old_lines_are_gone_from_head() {
    let s = snap("a.rs", Some("fn main() {}\n"), &[], &[]);
    assert_eq!(
        verdict(&rec("t:0", "a.rs", &[], &[A, B]), &s),
        Verdict::Retire
    );
}

#[test]
fn a_deletion_only_record_is_kept_while_its_old_lines_survive_in_head() {
    let head = format!("fn main() {{\n{A}\n{B}\n}}\n");
    let s = snap("a.rs", Some(&head), &[], &[]);
    assert_eq!(
        verdict(&rec("t:0", "a.rs", &[], &[A, B]), &s),
        Verdict::Keep(KeepReason::NotInHead)
    );
}

#[test]
fn a_record_whose_only_lines_are_unanchorable_is_never_retired() {
    // Nothing but braces and blanks carries no decidable evidence either way.
    let s = snap("a.rs", Some("fn main() {}\n"), &[], &[]);
    assert_eq!(
        verdict(&rec("t:0", "a.rs", &["}", "", "  "], &[]), &s),
        Verdict::Keep(KeepReason::NoEvidence)
    );
}

#[test]
fn a_whole_file_write_is_never_retired_on_content_alone() {
    // A whole-file write "matches everything", so finding its lines in HEAD
    // proves nothing. It retires only when its path is out of the diff entirely
    // *and* the file still exists at HEAD.
    let head = format!("fn main() {{\n{A}\n{B}\n}}\n");
    let mut r = rec("t:0", "a.rs", &[A, B], &[]);
    r.edit.whole_file = true;

    let touched = snap("a.rs", Some(&head), &[A], &[]);
    assert_eq!(
        verdict(&r, &touched),
        Verdict::Keep(KeepReason::WholeFileWrite)
    );

    let quiet = snap("a.rs", Some(&head), &[], &[]);
    assert_eq!(verdict(&r, &quiet), Verdict::Retire);
}

#[test]
fn a_record_for_a_file_that_no_longer_exists_is_retired() {
    // Absent from HEAD *and* absent from the working tree: there is nothing left
    // for it to ever label. Distinct from the branch-switch case, where the path
    // is present-but-different rather than gone.
    let s = snap("gone.rs", None, &[], &[]);
    assert_eq!(
        verdict(&rec("t:0", "gone.rs", &[A, B], &[]), &s),
        Verdict::Retire
    );
}

#[test]
fn an_unreadable_or_binary_file_yields_keep() {
    let mut s = snap("a.png", Some("binary"), &[], &[]);
    s.readable = false;
    assert_eq!(
        verdict(&rec("t:0", "a.png", &[A, B], &[]), &s),
        Verdict::Keep(KeepReason::Unreadable)
    );
}

// --- labels ----------------------------------------------------------------

fn label(turn: &str, paths: &[&str]) -> IntentLabel {
    IntentLabel {
        provider: ProviderId::ClaudeCode,
        turn_id: turn.into(),
        label: "add the retry budget".into(),
        paths: paths.iter().map(|s| s.to_string()).collect(),
        anchor: None,
        source: LabelSource::Declared,
        self_confidence: None,
    }
}

#[test]
fn a_label_is_retired_only_when_no_surviving_record_carries_its_turn() {
    // A declared, path-scoped label can title an orphan record on that path from
    // *any* turn, so a stale label is a second, independent way the bug shows.
    let head = format!("fn main() {{\n{A}\n{B}\n}}\n");
    let snapshots = vec![snap("a.rs", Some(&head), &[], &[])];
    let intents = Intents {
        records: vec![rec("t:0", "a.rs", &[A, B], &[])],
        labels: vec![label("turn-1", &["a.rs"])],
    };

    let outcome = plan(&intents, &snapshots);
    assert_eq!(outcome.records, vec![0]);
    assert_eq!(outcome.labels, vec![0]);
}

#[test]
fn a_label_survives_while_any_of_its_records_do() {
    let head = format!("fn main() {{\n{A}\n}}\n");
    let snapshots = vec![snap("a.rs", Some(&head), &[B], &[])];
    let intents = Intents {
        records: vec![rec("t:0", "a.rs", &[A, B], &[])],
        labels: vec![label("turn-1", &[])],
    };

    let outcome = plan(&intents, &snapshots);
    assert!(outcome.records.is_empty());
    assert!(outcome.labels.is_empty());
}

#[test]
fn a_record_with_no_snapshot_is_kept() {
    // Never decide about a path nobody looked at.
    let intents = Intents {
        records: vec![rec("t:0", "a.rs", &[A, B], &[])],
        labels: Vec::new(),
    };
    assert!(plan(&intents, &[]).records.is_empty());
}

// --- tombstones ------------------------------------------------------------

#[test]
fn a_tombstoned_record_is_rejected_on_re_import() {
    let r = rec("t:0", "a.rs", &[A, B], &[]);
    let tombs = tombstones_for(&[&r]);
    let mut mined = vec![r.clone()];
    assert_eq!(reject_tombstoned(&mut mined, &tombs), 1);
    assert!(mined.is_empty());
}

#[test]
fn a_new_edit_with_the_same_text_but_a_different_tool_use_id_survives_a_tombstone() {
    // The conjunction rule. Content alone would suppress a genuinely new edit
    // that repeats a change - losing a live label, the outcome to avoid.
    let old = rec("t:0", "a.rs", &[A, B], &[]);
    let tombs = tombstones_for(&[&old]);
    let mut mined = vec![rec("other:0", "a.rs", &[A, B], &[])];
    assert_eq!(reject_tombstoned(&mut mined, &tombs), 0);
    assert_eq!(mined.len(), 1);
}

#[test]
fn a_tombstone_matches_across_a_shifted_synthetic_turn_id() {
    // Mined turn ids are `claude-history-{session}-{block}` and the block index
    // shifts as a session is resumed or compacted, so identity cannot use it.
    let mut old = rec("t:0", "a.rs", &[A, B], &[]);
    old.turn_id = "claude-history-abc-1".into();
    let tombs = tombstones_for(&[&old]);

    let mut remined = rec("t:0", "a.rs", &[A, B], &[]);
    remined.turn_id = "claude-history-abc-4".into();
    let mut mined = vec![remined];
    assert_eq!(reject_tombstoned(&mut mined, &tombs), 1);
}

#[test]
fn a_tombstone_does_not_reject_the_same_call_editing_a_different_file() {
    let old = rec("t:0", "a.rs", &[A, B], &[]);
    let tombs = tombstones_for(&[&old]);
    let mut mined = vec![rec("t:0", "b.rs", &[A, B], &[])];
    assert_eq!(reject_tombstoned(&mut mined, &tombs), 0);
}

// --- bounding the store by branch ------------------------------------------

fn rec_on(tool: &str, path: &str, branch: Option<&str>, whole_file: bool) -> IntentRecord {
    IntentRecord {
        branch: branch.map(str::to_string),
        edit: IntentEdit {
            old_lines: Vec::new(),
            new_lines: vec![A.into()],
            whole_file,
        },
        ..rec(tool, path, &[A], &[])
    }
}

fn live(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn records_on_a_dead_branch_are_archived_recoverably() {
    let records = vec![
        rec_on("t:0", "a.rs", Some("feature-gone"), false),
        rec_on("t:1", "b.rs", Some("main"), false),
    ];
    let plan = plan_archive_stale(&records, &live(&["main"]), |_| false);
    assert_eq!(plan.branch_stale, vec![0]);
    assert!(plan.ignored_whole_file.is_empty());
}

#[test]
fn a_record_with_no_branch_is_never_archived() {
    // Origin unknown: a wrong removal is worse than a large store.
    let records = vec![rec_on("t:0", "a.rs", None, false)];
    let plan = plan_archive_stale(&records, &live(&["main"]), |_| false);
    assert!(plan.branch_stale.is_empty() && plan.ignored_whole_file.is_empty());
}

#[test]
fn a_whole_file_write_to_an_ignored_path_is_tombstoned_even_on_a_live_branch() {
    // The `.memories/*.html` case: never in HEAD, so never retired on content,
    // and re-mined every run — the permanent-removal path wins over the branch.
    let records = vec![rec_on("t:0", ".memories/r.html", Some("main"), true)];
    let plan = plan_archive_stale(&records, &live(&["main"]), |p| p.starts_with(".memories"));
    assert_eq!(plan.ignored_whole_file, vec![0]);
    assert!(plan.branch_stale.is_empty());
}

#[test]
fn a_tracked_whole_file_write_is_left_alone() {
    let records = vec![rec_on("t:0", "src/app.rs", Some("main"), true)];
    let plan = plan_archive_stale(&records, &live(&["main"]), |_| false);
    assert!(plan.branch_stale.is_empty() && plan.ignored_whole_file.is_empty());
}

// --- the HEAD-move gate ----------------------------------------------------

#[test]
fn the_content_check_is_skipped_when_head_has_not_moved() {
    // The Intent poll fires every couple of seconds; on an unchanged HEAD the
    // verdict can absorb nothing, so it must be a no-op rather than a whole-store
    // scan.
    assert!(!should_run_content_check(Some("abc123"), "abc123"));
}

#[test]
fn the_content_check_runs_when_head_moves_or_was_never_seen() {
    assert!(should_run_content_check(Some("abc123"), "def456"));
    // Never pruned: run once so an older version's backlog is cleaned.
    assert!(should_run_content_check(None, "abc123"));
}

// --- the per-path anchor set (hoisted out of the per-record loop) -----------

#[test]
fn many_records_on_one_path_are_judged_against_the_same_head_blob() {
    // `plan` builds each file's HEAD anchor set once and shares it across every
    // record on that path; the verdicts must match the per-record `verdict`.
    let head = format!("fn main() {{\n{A}\n{B}\n}}\n");
    let intents = Intents {
        records: vec![
            rec("t:0", "a.rs", &[A], &[]),            // absorbed
            rec("t:1", "a.rs", &[B], &[]),            // absorbed
            rec("t:2", "a.rs", &["let z = 1;"], &[]), // not in head
        ],
        labels: Vec::new(),
    };
    let snapshots = vec![snap("a.rs", Some(&head), &[], &[])];
    let outcome = plan(&intents, &snapshots);
    // First two retire, the third is kept.
    assert_eq!(outcome.records, vec![0, 1]);
    assert_eq!(outcome.kept, 1);
}

#[test]
fn serialisation_shape_pins_the_wire_keys() {
    // `RetireSummary` crosses IPC; these are the exact keys `types.ts` mirrors.
    let json = serde_json::to_value(RetireSummary {
        records_retired: 3,
        labels_retired: 1,
        kept_records: 7,
        head: Some("abc123".into()),
        pruned: true,
    })
    .unwrap();
    let object = json.as_object().unwrap();
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "head",
            "keptRecords",
            "labelsRetired",
            "pruned",
            "recordsRetired"
        ]
    );
}
