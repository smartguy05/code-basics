use super::*;

fn edit(path: &str, old: &[&str], new: &[&str]) -> UserEdit {
    UserEdit {
        path: path.to_string(),
        old_lines: old.iter().map(|s| s.to_string()).collect(),
        new_lines: new.iter().map(|s| s.to_string()).collect(),
    }
}

fn note(id: &str, seq: u64, label: &str, edits: Vec<UserEdit>) -> UserIntent {
    UserIntent {
        id: id.to_string(),
        seq,
        label: label.to_string(),
        edits,
    }
}

#[test]
fn turn_id_is_recognisable_and_colon_free_id() {
    let n = note("u3", 3, "why", vec![]);
    assert_eq!(n.turn_id(), "usernote:u3");
    // The grouping key built from this must begin `intent:usernote:` so a card
    // can be recognised as user-authored.
    let key = format!("intent:{}:{}", n.turn_id(), n.label);
    assert!(key.starts_with("intent:usernote:"), "{key}");
}

#[test]
fn overlap_is_by_shared_line_in_the_same_file() {
    let a = note("a", 0, "A", vec![edit("f.rs", &[], &["let x = 1;"])]);
    let same = note("b", 1, "B", vec![edit("f.rs", &[], &["let x = 1;"])]);
    let other_file = note("c", 2, "C", vec![edit("g.rs", &[], &["let x = 1;"])]);
    let other_line = note("d", 3, "D", vec![edit("f.rs", &[], &["let y = 2;"])]);

    assert!(a.overlaps(&same), "same file + shared line overlaps");
    assert!(!a.overlaps(&other_file), "different file does not overlap");
    assert!(!a.overlaps(&other_line), "different line does not overlap");
}

#[test]
fn upsert_replaces_an_overlapping_note() {
    let mut list = vec![note("a", 0, "old", vec![edit("f.rs", &[], &["line A"])])];
    upsert(
        &mut list,
        note("b", 1, "new", vec![edit("f.rs", &[], &["line A"])]),
    );
    assert_eq!(
        list.len(),
        1,
        "the overlapping note was replaced, not stacked"
    );
    assert_eq!(list[0].label, "new");
}

#[test]
fn upsert_keeps_a_disjoint_note() {
    let mut list = vec![note("a", 0, "keep", vec![edit("f.rs", &[], &["line A"])])];
    upsert(
        &mut list,
        note("b", 1, "add", vec![edit("f.rs", &[], &["line B"])]),
    );
    assert_eq!(list.len(), 2);
}

#[test]
fn remove_overlapping_drops_matches_only() {
    let mut list = vec![
        note("a", 0, "x", vec![edit("f.rs", &[], &["line A"])]),
        note("b", 1, "y", vec![edit("f.rs", &[], &["line B"])]),
    ];
    let geom = note("g", 9, "", vec![edit("f.rs", &[], &["line A"])]);
    assert!(remove_overlapping(&mut list, &geom));
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "b");
}

#[test]
fn next_id_is_colon_free_and_monotonic() {
    let list = vec![note("u0", 0, "a", vec![]), note("u4", 4, "b", vec![])];
    let (id, seq) = next_id(&list);
    assert_eq!(seq, 5);
    assert_eq!(id, "u5");
    assert!(!id.contains(':'));
}

#[test]
fn to_intents_yields_a_declared_label_scoped_to_its_files() {
    let list = vec![note(
        "a",
        0,
        "handle the retry",
        vec![
            edit("a.rs", &["was"], &["now"]),
            edit("b.rs", &[], &["added"]),
        ],
    )];
    let (records, labels) = to_intents(&list);

    assert_eq!(records.len(), 2, "one record per file edit");
    assert!(records.iter().all(|r| r.provider == ProviderId::User));
    assert!(records.iter().all(|r| r.turn_id == "usernote:a"));

    assert_eq!(labels.len(), 1);
    let label = &labels[0];
    assert_eq!(
        label.source,
        LabelSource::Declared,
        "user notes are declared"
    );
    assert_eq!(label.label, "handle the retry");
    assert_eq!(label.paths, vec!["a.rs".to_string(), "b.rs".to_string()]);
}

#[test]
fn merge_into_rebases_user_records_above_every_agent_seq() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    save(
        root,
        &[note("a", 0, "why", vec![edit("f.rs", &[], &["new line"])])],
    )
    .unwrap();

    // Two agent records at seqs 5 and 9.
    let mut intents = Intents {
        records: vec![
            IntentRecord {
                provider: ProviderId::ClaudeCode,
                turn_id: "t".into(),
                tool_use_id: "1".into(),
                seq: 5,
                path: "f.rs".into(),
                edit: IntentEdit {
                    old_lines: vec![],
                    new_lines: vec!["x".into()],
                    whole_file: false,
                },
                branch: None,
            },
            IntentRecord {
                provider: ProviderId::ClaudeCode,
                turn_id: "t".into(),
                tool_use_id: "2".into(),
                seq: 9,
                path: "f.rs".into(),
                edit: IntentEdit {
                    old_lines: vec![],
                    new_lines: vec!["y".into()],
                    whole_file: false,
                },
                branch: None,
            },
        ],
        labels: vec![],
    };

    merge_into(root, &mut intents).unwrap();

    let user_record = intents
        .records
        .iter()
        .find(|r| r.provider == ProviderId::User)
        .expect("user record merged in");
    assert!(
        user_record.seq > 9,
        "user record must outrank every agent seq (got {}), so it wins a contested line",
        user_record.seq
    );
    assert_eq!(intents.labels.len(), 1, "user label merged in");
}

#[test]
fn save_then_load_round_trips() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    assert!(load(root).unwrap().is_empty(), "missing file is empty");

    let notes = vec![note("a", 0, "why", vec![edit("f.rs", &["old"], &["new"])])];
    save(root, &notes).unwrap();
    assert_eq!(load(root).unwrap(), notes);
}

#[test]
fn a_corrupt_file_loads_as_empty_rather_than_failing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(intents_dir(root)).unwrap();
    std::fs::write(user_intents_path(root), "{ not json").unwrap();
    assert!(load(root).unwrap().is_empty());
}
