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

// ---------------------------------------------------------------------------
// Moving changes between cards
// ---------------------------------------------------------------------------

fn request(
    existing: Option<&str>,
    label: &str,
    destination: Vec<UserEdit>,
    moving: Vec<UserEdit>,
) -> MoveRequest {
    MoveRequest {
        existing_id: existing.map(str::to_string),
        label: label.to_string(),
        destination_edits: destination,
        moving,
    }
}

#[test]
fn a_note_id_is_recovered_from_the_grouping_key_it_ends_up_in() {
    let n = note("u3", 3, "why this changed", vec![]);
    let key = format!("intent:{}:{}", n.turn_id(), n.label);

    assert_eq!(note_id_of_group(&key), Some("u3"));
}

#[test]
fn a_label_with_colons_in_it_does_not_confuse_the_id() {
    // Ids are colon-free; labels are whatever the user typed.
    assert_eq!(
        note_id_of_group("intent:usernote:u7:fix: the thing: properly"),
        Some("u7")
    );
}

#[test]
fn a_card_that_is_not_a_user_note_yields_no_id() {
    assert_eq!(note_id_of_group("intent:abc123:some agent reason"), None);
    assert_eq!(note_id_of_group("symbol:function:handle_request"), None);
    assert_eq!(note_id_of_group("other:src/main.rs"), None);
    assert_eq!(note_id_of_group("intent:usernote::no id"), None);
}

#[test]
fn moving_into_a_new_card_creates_one_note_holding_the_moved_lines() {
    let mut list = Vec::new();

    move_edits(
        &mut list,
        request(
            None,
            "extracted",
            vec![],
            vec![edit("a.rs", &["old"], &["new"])],
        ),
    )
    .unwrap();

    assert_eq!(list.len(), 1);
    assert_eq!(list[0].label, "extracted");
    assert_eq!(list[0].edits, vec![edit("a.rs", &["old"], &["new"])]);
}

#[test]
fn moving_into_an_existing_note_keeps_its_own_lines_as_well() {
    let mut list = vec![note(
        "u0",
        0,
        "the destination",
        vec![edit("a.rs", &[], &["destination line"])],
    )];

    move_edits(
        &mut list,
        request(
            Some("u0"),
            "the destination",
            vec![edit("a.rs", &[], &["destination line"])],
            vec![edit("b.rs", &[], &["moved line"])],
        ),
    )
    .unwrap();

    assert_eq!(list.len(), 1, "a move must not leave a second card behind");
    let paths: Vec<&str> = list[0].edits.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, vec!["a.rs", "b.rs"]);
}

#[test]
fn moving_out_of_a_note_leaves_the_lines_that_stayed_behind_annotated() {
    // The bug `upsert` alone would cause: it drops an overlapping note whole, so
    // moving one line out would take the other line's note with it.
    let mut list = vec![note(
        "u0",
        0,
        "the source",
        vec![edit("a.rs", &[], &["stays", "goes"])],
    )];

    move_edits(
        &mut list,
        request(
            None,
            "elsewhere",
            vec![],
            vec![edit("a.rs", &[], &["goes"])],
        ),
    )
    .unwrap();

    let source = list.iter().find(|u| u.id == "u0").expect("source survives");
    assert_eq!(source.edits, vec![edit("a.rs", &[], &["stays"])]);

    let moved = list.iter().find(|u| u.label == "elsewhere").unwrap();
    assert_eq!(moved.edits, vec![edit("a.rs", &[], &["goes"])]);
}

#[test]
fn moving_the_last_line_out_of_a_note_removes_the_empty_note() {
    let mut list = vec![note(
        "u0",
        0,
        "the source",
        vec![edit("a.rs", &[], &["only"])],
    )];

    move_edits(
        &mut list,
        request(
            None,
            "elsewhere",
            vec![],
            vec![edit("a.rs", &[], &["only"])],
        ),
    )
    .unwrap();

    assert_eq!(list.len(), 1);
    assert_eq!(list[0].label, "elsewhere");
}

#[test]
fn the_destination_ends_up_with_the_highest_sequence_number() {
    // Otherwise a line moved out of one note could still be won by the note it
    // just left, since attribution hands a contested line to the later record.
    let mut list = vec![
        note("u0", 0, "first", vec![edit("a.rs", &[], &["a"])]),
        note("u1", 9, "second", vec![edit("b.rs", &[], &["b"])]),
    ];

    move_edits(
        &mut list,
        request(None, "third", vec![], vec![edit("c.rs", &[], &["c"])]),
    )
    .unwrap();

    let moved = list.iter().find(|u| u.label == "third").unwrap();
    assert!(
        list.iter().all(|u| u.label == "third" || u.seq < moved.seq),
        "the destination must outrank every other note"
    );
}

#[test]
fn moving_something_already_in_the_destination_changes_nothing_and_repeats_nothing() {
    let mut list = vec![note("u0", 0, "dest", vec![edit("a.rs", &[], &["line"])])];

    move_edits(
        &mut list,
        request(
            Some("u0"),
            "dest",
            vec![edit("a.rs", &[], &["line"])],
            vec![edit("a.rs", &[], &["line"])],
        ),
    )
    .unwrap();

    assert_eq!(list.len(), 1);
    assert_eq!(list[0].edits, vec![edit("a.rs", &[], &["line"])]);
}

#[test]
fn moving_into_an_existing_note_can_rename_it() {
    let mut list = vec![note("u0", 0, "old name", vec![edit("a.rs", &[], &["a"])])];

    move_edits(
        &mut list,
        request(
            Some("u0"),
            "new name",
            vec![edit("a.rs", &[], &["a"])],
            vec![edit("b.rs", &[], &["b"])],
        ),
    )
    .unwrap();

    assert_eq!(list[0].label, "new name");
}

#[test]
fn a_moved_file_merges_into_the_destinations_entry_for_the_same_file() {
    let mut list = Vec::new();

    move_edits(
        &mut list,
        request(
            None,
            "one card",
            vec![edit("a.rs", &["old one"], &["new one"])],
            vec![edit("a.rs", &["old two"], &["new two"])],
        ),
    )
    .unwrap();

    assert_eq!(list[0].edits.len(), 1, "one entry per file");
    assert_eq!(
        list[0].edits[0],
        edit("a.rs", &["old one", "old two"], &["new one", "new two"])
    );
}

#[test]
fn a_file_named_with_the_other_separator_is_the_same_file() {
    let mut list = vec![note("u0", 0, "dest", vec![edit("src/a.rs", &[], &["a"])])];

    move_edits(
        &mut list,
        request(
            Some("u0"),
            "dest",
            vec![edit("src/a.rs", &[], &["a"])],
            vec![edit("src\\a.rs", &[], &["b"])],
        ),
    )
    .unwrap();

    assert_eq!(list[0].edits.len(), 1, "one entry, not two spellings");
}

#[test]
fn a_move_with_nothing_in_it_is_refused_rather_than_stored() {
    let mut list = Vec::new();

    assert!(move_edits(&mut list, request(None, "name", vec![], vec![])).is_err());
    assert!(move_edits(
        &mut list,
        request(None, "name", vec![], vec![edit("a.rs", &[], &[])])
    )
    .is_err());
    assert!(list.is_empty(), "a refused move stores nothing");
}

#[test]
fn a_new_card_needs_a_name() {
    let mut list = Vec::new();

    assert!(move_edits(
        &mut list,
        request(None, "   ", vec![], vec![edit("a.rs", &[], &["a"])])
    )
    .is_err());
    assert!(list.is_empty());
}

#[test]
fn moving_into_a_card_that_has_gone_is_refused_and_changes_nothing() {
    let mut list = vec![note("u0", 0, "still here", vec![edit("a.rs", &[], &["a"])])];

    let error = move_edits(
        &mut list,
        request(Some("u9"), "gone", vec![], vec![edit("b.rs", &[], &["b"])]),
    )
    .unwrap_err();

    assert!(error.to_string().contains("no longer"));
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].edits, vec![edit("a.rs", &[], &["a"])]);
}

#[test]
fn a_moved_note_still_converts_into_records_the_pipeline_consumes() {
    // The whole point of storing a move as a note: it goes through the ordinary
    // record path and is attributed by content like everything else.
    let mut list = Vec::new();
    move_edits(
        &mut list,
        request(
            None,
            "moved here",
            vec![],
            vec![edit("a.rs", &["old"], &["new"])],
        ),
    )
    .unwrap();

    let (records, labels) = to_intents(&list);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].provider, ProviderId::User);
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].label, "moved here");
    assert_eq!(labels[0].paths, vec!["a.rs".to_string()]);
}
