//! Tests for the Claude Code transcript reader.
//! Included by `providers/claude_code.rs` under `#[cfg(test)]`.

use super::*;
use serde_json::json;

// -- summarise --------------------------------------------------------------

#[test]
fn a_summary_is_the_first_sentence_of_the_first_prose_line() {
    let summary =
        summarise("Rename the adapter so the dispatch reads clearly. Then run the tests.");

    assert_eq!(
        summary.as_deref(),
        Some("Rename the adapter so the dispatch reads clearly")
    );
}

#[test]
fn a_summary_skips_blank_lines_headings_and_fences() {
    let text = "\n\n# Heading that is long enough to pass\n```rust\n\nMove the parser into its own module\n";

    assert_eq!(
        summarise(text).as_deref(),
        Some("Move the parser into its own module")
    );
}

#[test]
fn a_trailing_colon_is_dropped_from_a_summary() {
    // The sample used to be "Let me read the failing test:", which the
    // narration gate now refuses as an announcement. The mechanic under test is
    // the colon, so the sample changed rather than the rule.
    assert_eq!(
        summarise("The failing test reads the manifest twice:").as_deref(),
        Some("The failing test reads the manifest twice")
    );
}

/// A label shorter than a few words is noise, and a wrong label is worse than
/// none — so the short case abstains.
#[test]
fn a_summary_too_short_to_mean_anything_is_refused() {
    assert_eq!(summarise("Done."), None);
    assert_eq!(summarise("Okay"), None);
}

/// A paragraph makes the card unreadable while looking authoritative.
#[test]
fn a_summary_too_long_for_a_card_is_refused() {
    let long = format!("{} and more", "x".repeat(120));

    assert_eq!(summarise(&long), None);
}

#[test]
fn prose_with_nothing_but_blank_lines_has_no_summary() {
    assert_eq!(summarise("\n   \n\t\n"), None);
}

#[test]
fn a_sentence_ending_in_a_question_or_bang_is_also_terminated() {
    // "Should the adapter own this?" used to stand in for the `?` terminator
    // and is now refused as a question, so the `?` case uses a sentence that
    // survives the gate.
    assert_eq!(
        summarise("The adapter owns this now? Apparently so.").as_deref(),
        Some("The adapter owns this now")
    );
    assert_eq!(
        summarise("That was the bug all along! Fixing it now.").as_deref(),
        Some("That was the bug all along")
    );
}

// -- The narration gate -----------------------------------------------------
//
// Mined history is prose written for a chat, so it carries the same failure
// mode as the Stop hook's fallback and shares its gate (`hook::looks_like_narration`).

#[test]
fn mined_prose_about_the_tooling_is_refused() {
    assert_eq!(summarise("The workflow is running with Opus"), None);
    assert_eq!(
        summarise("The subagent came back with three findings"),
        None
    );
}

#[test]
fn mined_prose_that_only_announces_the_next_step_is_refused() {
    assert_eq!(summarise("Let me read the failing test"), None);
    assert_eq!(summarise("I'll wire the command up next"), None);
}

#[test]
fn mined_prose_that_describes_the_change_is_still_kept() {
    assert_eq!(
        summarise("Move the parser into its own module").as_deref(),
        Some("Move the parser into its own module")
    );
}

// -- lines_of ---------------------------------------------------------------

#[test]
fn empty_text_produces_no_lines() {
    assert!(lines_of("").is_empty());
}

#[test]
fn carriage_returns_are_stripped_from_every_line() {
    assert_eq!(lines_of("one\r\ntwo\r\n"), vec!["one", "two"]);
}

#[test]
fn a_single_line_without_a_terminator_is_still_a_line() {
    assert_eq!(lines_of("only"), vec!["only"]);
}

// -- pair_to_edit / single_or_many -------------------------------------------

#[test]
fn an_edit_pair_reads_the_string_spelling() {
    let edit = pair_to_edit(&json!({ "old_string": "a\nb", "new_string": "c" })).unwrap();

    assert_eq!(edit.old_lines, vec!["a", "b"]);
    assert_eq!(edit.new_lines, vec!["c"]);
    assert!(!edit.whole_file);
}

/// Both spellings ship depending on the agent version; both are accepted.
#[test]
fn an_edit_pair_also_reads_the_text_spelling() {
    let edit = pair_to_edit(&json!({ "old_text": "a", "new_text": "b" })).unwrap();

    assert_eq!(edit.old_lines, vec!["a"]);
    assert_eq!(edit.new_lines, vec!["b"]);
}

#[test]
fn an_insertion_with_no_old_side_is_still_an_edit() {
    let edit = pair_to_edit(&json!({ "old_string": "", "new_string": "added" })).unwrap();

    assert!(edit.old_lines.is_empty());
    assert_eq!(edit.new_lines, vec!["added"]);
}

#[test]
fn a_pair_with_neither_side_is_not_an_edit() {
    assert!(pair_to_edit(&json!({ "old_string": "", "new_string": "" })).is_none());
    assert!(pair_to_edit(&json!({ "file_path": "a.rs" })).is_none());
}

#[test]
fn a_multi_edit_input_yields_one_edit_per_pair() {
    let input = json!({
        "edits": [
            { "old_string": "a", "new_string": "b" },
            { "old_string": "c", "new_string": "d" },
        ]
    });

    assert_eq!(single_or_many(&input).len(), 2);
}

#[test]
fn empty_pairs_inside_a_multi_edit_are_dropped() {
    let input = json!({
        "edits": [
            { "old_string": "", "new_string": "" },
            { "old_string": "c", "new_string": "d" },
        ]
    });

    let edits = single_or_many(&input);

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_lines, vec!["d"]);
}

#[test]
fn an_input_without_an_edits_array_is_read_as_one_pair() {
    let edits = single_or_many(&json!({ "old_string": "a", "new_string": "b" }));

    assert_eq!(edits.len(), 1);
}

// -- read_tool_use ----------------------------------------------------------

fn root() -> PathBuf {
    PathBuf::from(if cfg!(windows) {
        r"C:\work\repo"
    } else {
        "/work/repo"
    })
}

fn in_root(relative: &str) -> String {
    let mut path = root().to_string_lossy().into_owned();
    path.push(std::path::MAIN_SEPARATOR);
    path.push_str(relative);
    path
}

fn collect(block: serde_json::Value) -> Vec<IntentRecord> {
    let mut seq = 0;
    let mut out = Vec::new();
    read_tool_use(
        &block,
        &root(),
        &Some("main".to_string()),
        "turn-1",
        &mut seq,
        &mut out,
    );
    out
}

#[test]
fn a_write_call_records_the_whole_file() {
    let records = collect(json!({
        "type": "tool_use",
        "id": "toolu_1",
        "name": "Write",
        "input": { "file_path": in_root("src/main.rs"), "content": "fn main() {}\n" },
    }));

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].path, "src/main.rs");
    assert_eq!(records[0].tool_use_id, "toolu_1:0");
    assert_eq!(records[0].seq, 1);
    assert_eq!(records[0].branch.as_deref(), Some("main"));
    assert_eq!(records[0].provider, ProviderId::ClaudeCode);
    assert!(records[0].edit.whole_file);
    assert_eq!(records[0].edit.new_lines, vec!["fn main() {}"]);
    assert!(records[0].edit.old_lines.is_empty());
}

#[test]
fn a_write_call_also_accepts_the_file_text_spelling() {
    let records = collect(json!({
        "id": "toolu_2",
        "name": "Write",
        "input": { "file_path": in_root("a.rs"), "file_text": "one\ntwo" },
    }));

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].edit.new_lines, vec!["one", "two"]);
}

#[test]
fn a_multi_edit_call_records_one_entry_per_pair_with_distinct_ids() {
    let records = collect(json!({
        "id": "toolu_3",
        "name": "MultiEdit",
        "input": {
            "file_path": in_root("src/lib.rs"),
            "edits": [
                { "old_string": "a", "new_string": "b" },
                { "old_string": "c", "new_string": "d" },
            ],
        },
    }));

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].tool_use_id, "toolu_3:0");
    assert_eq!(records[1].tool_use_id, "toolu_3:1");
    assert_eq!(records[0].seq, 1);
    assert_eq!(records[1].seq, 2);
    assert!(!records[0].edit.whole_file);
}

/// A read is not an edit; only the edit tools produce history.
#[test]
fn a_non_editing_tool_call_records_nothing() {
    let records = collect(json!({
        "id": "toolu_4",
        "name": "Read",
        "input": { "file_path": in_root("src/lib.rs") },
    }));

    assert!(records.is_empty());
}

/// An agent editing someone else's repository is not this workspace's history.
#[test]
fn an_edit_outside_the_workspace_is_dropped() {
    let records = collect(json!({
        "id": "toolu_5",
        "name": "Write",
        "input": { "file_path": "/somewhere/else/x.rs", "content": "hi" },
    }));

    assert!(records.is_empty());
}

#[test]
fn a_tool_call_with_no_input_or_no_path_records_nothing() {
    assert!(collect(json!({ "id": "a", "name": "Write" })).is_empty());
    assert!(collect(json!({ "id": "a", "name": "Write", "input": { "content": "x" } })).is_empty());
}

#[test]
fn an_edit_whose_both_sides_are_empty_records_nothing() {
    let records = collect(json!({
        "id": "toolu_6",
        "name": "Edit",
        "input": { "file_path": in_root("a.rs"), "old_string": "", "new_string": "" },
    }));

    assert!(records.is_empty());
}

// -- transcript_cwd ---------------------------------------------------------

fn write_lines(path: &Path, lines: &[String]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, format!("{}\n", lines.join("\n"))).unwrap();
}

#[test]
fn a_transcripts_cwd_is_read_and_separator_normalised() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    write_lines(
        &path,
        &[json!({ "type": "user", "cwd": r"C:\work\repo" }).to_string()],
    );

    assert_eq!(transcript_cwd(&path).as_deref(), Some("C:/work/repo"));
}

#[test]
fn a_cwd_past_the_first_forty_lines_is_not_looked_for() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut lines: Vec<String> = (0..40)
        .map(|_| json!({ "type": "user" }).to_string())
        .collect();
    lines.push(json!({ "cwd": "/late" }).to_string());
    write_lines(&path, &lines);

    assert_eq!(transcript_cwd(&path), None);
}

#[test]
fn malformed_lines_do_not_stop_the_cwd_search() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    write_lines(
        &path,
        &[
            "not json at all".to_string(),
            json!({ "cwd": "/work/repo" }).to_string(),
        ],
    );

    assert_eq!(transcript_cwd(&path).as_deref(), Some("/work/repo"));
}

#[test]
fn a_missing_transcript_has_no_cwd() {
    let dir = tempfile::tempdir().unwrap();

    assert_eq!(transcript_cwd(&dir.path().join("nothing.jsonl")), None);
}

// -- read_transcript --------------------------------------------------------

fn assistant(content: serde_json::Value) -> String {
    json!({ "type": "assistant", "gitBranch": "main", "message": { "content": content } })
        .to_string()
}

fn text_block(text: &str) -> serde_json::Value {
    json!({ "type": "text", "text": text })
}

/// A human prompt line, the shape `read_transcript` mines into an `IntentPrompt`.
fn user(text: &str) -> String {
    json!({
        "type": "user",
        "message": { "role": "user", "content": [ { "type": "text", "text": text } ] },
    })
    .to_string()
}

fn edit_block(id: &str, relative: &str) -> serde_json::Value {
    json!({
        "type": "tool_use",
        "id": id,
        "name": "Edit",
        "input": { "file_path": in_root(relative), "old_string": "a", "new_string": "b" },
    })
}

fn transcript(dir: &Path, name: &str, lines: &[String]) -> PathBuf {
    let path = dir.join(name);
    write_lines(&path, lines);
    path
}

fn read(lines: &[String]) -> (Vec<IntentRecord>, Vec<IntentLabel>) {
    let mined = read_mined(lines);
    (mined.records, mined.labels)
}

fn read_mined(lines: &[String]) -> HistoryMined {
    let dir = tempfile::tempdir().unwrap();
    let path = transcript(dir.path(), "sess.jsonl", lines);
    let mut seq = 0;
    let mut mined = HistoryMined::default();
    read_transcript(&path, &root(), &mut seq, &mut mined);
    mined
}

#[test]
fn the_nearest_preceding_prose_labels_the_edits_that_follow() {
    let (records, labels) = read(&[assistant(json!([
        text_block("Move the parser into its own module"),
        edit_block("t1", "a.rs"),
        edit_block("t2", "b.rs"),
    ]))]);

    assert_eq!(records.len(), 2);
    assert_eq!(labels.len(), 1, "one label per block of prose: {labels:?}");
    assert_eq!(labels[0].label, "Move the parser into its own module");
    assert_eq!(labels[0].provider, ProviderId::ClaudeCode);
    assert_eq!(labels[0].turn_id, records[0].turn_id);
    assert_eq!(records[0].turn_id, records[1].turn_id);
    assert!(labels[0].paths.is_empty());
    assert!(labels[0].anchor.is_none());
}

#[test]
fn new_prose_starts_a_new_intent_group() {
    let (records, labels) = read(&[assistant(json!([
        text_block("Move the parser into its own module"),
        edit_block("t1", "a.rs"),
        text_block("Now update the callers to match"),
        edit_block("t2", "b.rs"),
    ]))]);

    assert_eq!(records.len(), 2);
    assert_eq!(labels.len(), 2);
    assert_ne!(records[0].turn_id, records[1].turn_id);
    assert_eq!(labels[1].label, "Now update the callers to match");
}

/// A subagent's work is retroactively mined too, keyed to its own lineage so
/// it never collides with the main session. This is the inverse of the old
/// skip: a sidechain edit with resolvable lineage now produces a record and a
/// label under a `-sub-<root>-` turn id.
#[test]
fn a_sidechain_edit_is_mined_as_a_subagent_turn() {
    let line = json!({
        "type": "assistant",
        "isSidechain": true,
        "uuid": "s1",
        "parentUuid": null,
        "gitBranch": "main",
        "message": { "content": [ text_block("Move the parser into its own module"), edit_block("t1", "a.rs") ] },
    })
    .to_string();

    let (records, labels) = read(&[line]);

    assert_eq!(records.len(), 1);
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].turn_id, records[0].turn_id);
    assert_eq!(labels[0].source, LabelSource::Inferred);
    assert!(
        records[0].turn_id.contains("-sub-s1-"),
        "got: {}",
        records[0].turn_id
    );
}

/// Parallel subagents interleave their lines in one transcript, so contiguity
/// cannot separate them — the lineage root does. Two subagents whose lines
/// alternate must land in two distinct turns, each carrying its own prompt.
#[test]
fn interleaved_subagents_are_grouped_by_lineage_not_order() {
    let a_prompt = json!({
        "type": "user",
        "isSidechain": true,
        "uuid": "a1",
        "parentUuid": null,
        "message": { "role": "user", "content": [ { "type": "text", "text": "Refactor the parser in module A" } ] },
    })
    .to_string();
    let b_prompt = json!({
        "type": "user",
        "isSidechain": true,
        "uuid": "b1",
        "parentUuid": null,
        "message": { "role": "user", "content": [ { "type": "text", "text": "Rename the callers in module B" } ] },
    })
    .to_string();
    let a_edit = json!({
        "type": "assistant",
        "isSidechain": true,
        "uuid": "a2",
        "parentUuid": "a1",
        "gitBranch": "main",
        "message": { "content": [ text_block("Move the parser into its own module"), edit_block("ta", "a.rs") ] },
    })
    .to_string();
    let b_edit = json!({
        "type": "assistant",
        "isSidechain": true,
        "uuid": "b2",
        "parentUuid": "b1",
        "gitBranch": "main",
        "message": { "content": [ text_block("Now update the callers to match"), edit_block("tb", "b.rs") ] },
    })
    .to_string();

    // Interleaved line-by-line: A prompt, B prompt, A edit, B edit.
    let mined = read_mined(&[a_prompt, b_prompt, a_edit, b_edit]);

    assert_eq!(mined.records.len(), 2);
    let a = mined.records.iter().find(|r| r.path == "a.rs").unwrap();
    let b = mined.records.iter().find(|r| r.path == "b.rs").unwrap();
    assert_ne!(
        a.turn_id, b.turn_id,
        "interleaved subagents must not share a turn"
    );
    assert!(a.turn_id.contains("-sub-a1-"), "got: {}", a.turn_id);
    assert!(b.turn_id.contains("-sub-b1-"), "got: {}", b.turn_id);

    // Each subagent's prompt joins its own edits, not the other's.
    let a_prompt_rec = mined
        .prompts
        .iter()
        .find(|p| p.turn_id == a.turn_id)
        .unwrap();
    assert!(
        a_prompt_rec.prompt.contains("module A"),
        "got: {}",
        a_prompt_rec.prompt
    );
    let b_prompt_rec = mined
        .prompts
        .iter()
        .find(|p| p.turn_id == b.turn_id)
        .unwrap();
    assert!(
        b_prompt_rec.prompt.contains("module B"),
        "got: {}",
        b_prompt_rec.prompt
    );
    assert_eq!(mined.labels.len(), 2);
}

/// A wrong grouping is worse than none: a sidechain entry whose parent lineage
/// cannot be resolved (the parent uuid is not in the file) is skipped, never
/// misattributed to some other subagent.
#[test]
fn a_sidechain_entry_with_an_unresolvable_parent_abstains() {
    let line = json!({
        "type": "assistant",
        "isSidechain": true,
        "uuid": "x1",
        "parentUuid": "ghost",
        "gitBranch": "main",
        "message": { "content": [ text_block("Move the parser into its own module"), edit_block("t1", "a.rs") ] },
    })
    .to_string();

    let (records, labels) = read(&[line]);

    assert!(records.is_empty(), "got: {records:?}");
    assert!(labels.is_empty());
}

/// No uuid means no lineage to group on, so the entry abstains rather than
/// guess a turn.
#[test]
fn a_sidechain_entry_with_no_uuid_abstains() {
    let line = json!({
        "type": "assistant",
        "isSidechain": true,
        "gitBranch": "main",
        "message": { "content": [ text_block("Move the parser into its own module"), edit_block("t1", "a.rs") ] },
    })
    .to_string();

    let (records, labels) = read(&[line]);

    assert!(records.is_empty(), "got: {records:?}");
    assert!(labels.is_empty());
}

/// Regression: the main-session turn id is unchanged even when a sidechain
/// shares the transcript. The main path keeps its `claude-history-<session>-<block>`
/// shape and never grows a `-sub-` segment.
#[test]
fn the_main_session_turn_id_is_unchanged_when_a_sidechain_is_present() {
    let dir = tempfile::tempdir().unwrap();
    let main = json!({
        "type": "assistant",
        "uuid": "m1",
        "parentUuid": null,
        "gitBranch": "main",
        "message": { "content": [ text_block("Move the parser into its own module"), edit_block("t1", "a.rs") ] },
    })
    .to_string();
    let sub = json!({
        "type": "assistant",
        "isSidechain": true,
        "uuid": "s1",
        "parentUuid": null,
        "gitBranch": "main",
        "message": { "content": [ text_block("Now update the callers to match"), edit_block("t2", "b.rs") ] },
    })
    .to_string();
    let path = transcript(dir.path(), "abc-123.jsonl", &[main, sub]);

    let mut seq = 0;
    let mut mined = HistoryMined::default();
    read_transcript(&path, &root(), &mut seq, &mut mined);

    let main_rec = mined.records.iter().find(|r| r.path == "a.rs").unwrap();
    let sub_rec = mined.records.iter().find(|r| r.path == "b.rs").unwrap();
    assert_eq!(main_rec.turn_id, "claude-history-abc-123-1");
    assert!(
        sub_rec.turn_id.contains("-sub-s1-"),
        "got: {}",
        sub_rec.turn_id
    );
}

#[test]
fn user_entries_and_malformed_lines_are_skipped() {
    let user = json!({
        "type": "user",
        "message": { "content": [ edit_block("t1", "a.rs") ] },
    })
    .to_string();

    let (records, _) = read(&["{ broken".to_string(), user]);

    assert!(records.is_empty());
}

/// The user's prompt is mined and keyed to the **same** turn id as the edits of
/// the block it opened — the invariant the whole feature rests on.
#[test]
fn the_user_prompt_is_mined_and_keyed_to_the_edits_turn() {
    let mined = read_mined(&[
        user("Add exponential backoff to the token refresh, cap at 5 retries"),
        assistant(json!([
            text_block("Add retry with backoff to the refresher"),
            edit_block("t1", "a.rs"),
        ])),
    ]);

    assert_eq!(mined.records.len(), 1);
    assert_eq!(mined.prompts.len(), 1);
    assert_eq!(mined.prompts[0].turn_id, mined.records[0].turn_id);
    assert!(mined.prompts[0].prompt.contains("exponential backoff"));
    assert_eq!(mined.prompts[0].provider, ProviderId::ClaudeCode);
}

/// A `user` line carrying a tool_result — not a human prompt — must never be
/// mined as one.
#[test]
fn a_tool_result_user_line_is_not_mined_as_a_prompt() {
    let tool_result = json!({
        "type": "user",
        "message": { "content": [ { "type": "tool_result", "content": "ok" } ] },
    })
    .to_string();

    let mined = read_mined(&[
        tool_result,
        assistant(json!([
            text_block("doing the thing"),
            edit_block("t1", "a.rs"),
        ])),
    ]);

    assert_eq!(mined.records.len(), 1);
    assert!(mined.prompts.is_empty(), "got: {:?}", mined.prompts);
}

/// A block of prose with no edits produces no prompt — the prompt only lands
/// where records did, so it can join.
#[test]
fn a_prompt_with_no_edits_in_its_block_is_not_recorded() {
    let mined = read_mined(&[
        user("just asking a question"),
        assistant(json!([text_block("Here is the answer to your question")])),
    ]);

    assert!(mined.records.is_empty());
    assert!(mined.prompts.is_empty());
}

#[test]
fn the_branch_recorded_on_the_entry_is_carried_onto_the_records() {
    let (records, _) = read(&[assistant(json!([edit_block("t1", "a.rs")]))]);

    assert_eq!(records[0].branch.as_deref(), Some("main"));
}

#[test]
fn an_empty_branch_is_recorded_as_no_branch() {
    let line = json!({
        "type": "assistant",
        "gitBranch": "",
        "message": { "content": [ edit_block("t1", "a.rs") ] },
    })
    .to_string();

    let (records, _) = read(&[line]);

    assert_eq!(records[0].branch, None);
}

/// Prose too long or too short to summarise leaves the edits unlabelled rather
/// than attaching something misleading.
#[test]
fn edits_with_no_usable_prose_are_recorded_without_a_label() {
    let (records, labels) = read(&[assistant(json!([
        text_block("Done."),
        edit_block("t1", "a.rs"),
    ]))]);

    assert_eq!(records.len(), 1);
    assert!(labels.is_empty(), "got: {labels:?}");
}

#[test]
fn an_entry_whose_message_has_no_content_array_is_skipped() {
    let line =
        json!({ "type": "assistant", "message": { "content": "just a string" } }).to_string();

    let (records, _) = read(&[line]);

    assert!(records.is_empty());
}

#[test]
fn an_unreadable_transcript_is_ignored_rather_than_failing() {
    let dir = tempfile::tempdir().unwrap();
    let mut seq = 0;
    let mut mined = HistoryMined::default();

    read_transcript(
        &dir.path().join("missing.jsonl"),
        &root(),
        &mut seq,
        &mut mined,
    );

    assert!(mined.records.is_empty());
    assert!(mined.labels.is_empty());
}

#[test]
fn the_turn_id_names_the_session_file_and_the_prose_block() {
    let dir = tempfile::tempdir().unwrap();
    let path = transcript(
        dir.path(),
        "abc-123.jsonl",
        &[assistant(json!([
            text_block("Move the parser into its own module"),
            edit_block("t1", "a.rs"),
        ]))],
    );

    let mut seq = 0;
    let mut mined = HistoryMined::default();
    read_transcript(&path, &root(), &mut seq, &mut mined);

    assert_eq!(mined.records[0].turn_id, "claude-history-abc-123-1");
}

// -- the Provider, before the seam ------------------------------------------

/// Characterisation: whatever the machine's own `~/.claude` holds, a workspace
/// that no session ever ran in has no history and no sessions, and an absent
/// agent reports the absent status verbatim.
#[test]
fn a_workspace_no_session_ever_ran_in_has_no_history() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ClaudeCode::new();

    let status = provider.status(dir.path());
    let HistoryMined {
        records, labels, ..
    } = provider.history(dir.path()).unwrap();

    assert_eq!(status.provider, ProviderId::ClaudeCode);
    assert_eq!(status.detected, provider.detected());
    assert_eq!(status.sessions, 0);
    assert!(records.is_empty());
    assert!(labels.is_empty());
    if !provider.detected() {
        assert_eq!(status, ProviderStatus::absent(ProviderId::ClaudeCode));
    }
}

// -- the Provider, against a fixture home -----------------------------------

/// A `~/.claude` fixture tree: `projects/<encoded workspace>/<session>.jsonl`.
struct Fixture {
    home: tempfile::TempDir,
    workspace: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            home: tempfile::tempdir().unwrap(),
            workspace: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(fixture.home.path().join("projects")).unwrap();
        fixture
    }

    fn root(&self) -> &Path {
        self.workspace.path()
    }

    fn provider(&self) -> ClaudeCode {
        ClaudeCode::with_home(self.home.path())
    }

    fn cwd_line(&self) -> String {
        json!({ "type": "user", "cwd": self.root().to_string_lossy() }).to_string()
    }

    /// A session under an arbitrary project directory name, recording `cwd`.
    fn session_in(&self, project_dir: &str, name: &str, mut lines: Vec<String>) {
        let mut all = vec![self.cwd_line()];
        all.append(&mut lines);
        write_lines(
            &self
                .home
                .path()
                .join("projects")
                .join(project_dir)
                .join(name),
            &all,
        );
    }

    fn session(&self, name: &str, lines: Vec<String>) {
        self.session_in(&encode_project_dir(self.root()), name, lines);
    }

    /// The blocks a real edit turn carries, with paths inside this workspace.
    fn edit_turn(&self, prose: &str, id: &str, relative: &str) -> String {
        let path = self.root().join(relative);
        json!({
            "type": "assistant",
            "gitBranch": "main",
            "message": { "content": [
                { "type": "text", "text": prose },
                {
                    "type": "tool_use",
                    "id": id,
                    "name": "Edit",
                    "input": {
                        "file_path": path.to_string_lossy(),
                        "old_string": "a",
                        "new_string": "b",
                    },
                },
            ] },
        })
        .to_string()
    }

    /// A subagent transcript, written where this Claude Code version keeps them:
    /// `projects/<enc>/<session>/subagents/<agent>.jsonl`, not the flat project
    /// dir. Every line is a sidechain; the root line carries `parentUuid: null`
    /// (the spawning Task lives in the main file, which this standalone file does
    /// not repeat) so its lineage resolves.
    fn subagent_session(&self, session: &str, agent: &str, relative: &str) {
        let path = self.root().join(relative);
        let root_line = json!({
            "type": "user",
            "cwd": self.root().to_string_lossy(),
            "uuid": "sub-root",
            "parentUuid": null,
            "isSidechain": true,
            "message": { "role": "user", "content": "go do the thing" },
        })
        .to_string();
        let edit_line = json!({
            "type": "assistant",
            "uuid": "sub-edit",
            "parentUuid": "sub-root",
            "isSidechain": true,
            "gitBranch": "main",
            "message": { "content": [
                { "type": "text", "text": "editing the file now" },
                {
                    "type": "tool_use",
                    "id": "se1",
                    "name": "Edit",
                    "input": {
                        "file_path": path.to_string_lossy(),
                        "old_string": "a",
                        "new_string": "b",
                    },
                },
            ] },
        })
        .to_string();
        let dir = self
            .home
            .path()
            .join("projects")
            .join(encode_project_dir(self.root()))
            .join(session)
            .join("subagents");
        write_lines(&dir.join(format!("{agent}.jsonl")), &[root_line, edit_line]);
    }
}

#[test]
fn a_subagent_transcript_is_discovered_and_its_edit_is_mined() {
    let fixture = Fixture::new();
    // A normal flat main-session transcript must still be found.
    fixture.session(
        "main.jsonl",
        vec![fixture.edit_turn("do a thing", "t1", "main.rs")],
    );
    // A file edited only by a subagent, whose geometry lives one level deeper.
    fixture.subagent_session("session-1", "agent-abc", "sub.rs");

    let HistoryMined { records, .. } = fixture.provider().history(fixture.root()).unwrap();
    let paths: Vec<&str> = records.iter().map(|r| r.path.as_str()).collect();

    assert!(
        paths.iter().any(|p| p.ends_with("main.rs")),
        "the flat main session is still mined: {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p.ends_with("sub.rs")),
        "the subagent edit is now mined: {paths:?}"
    );
}

#[test]
fn a_home_directory_that_exists_is_reported_as_detected() {
    let fixture = Fixture::new();

    assert!(fixture.provider().detected());
    assert!(!ClaudeCode::with_home(fixture.home.path().join("nope")).detected());
}

#[test]
fn an_absent_agent_reports_the_absent_status() {
    let fixture = Fixture::new();
    let provider = ClaudeCode::with_home(fixture.home.path().join("nope"));

    assert_eq!(
        provider.status(fixture.root()),
        ProviderStatus::absent(ProviderId::ClaudeCode)
    );
}

#[test]
fn status_counts_the_sessions_recorded_for_this_workspace() {
    let fixture = Fixture::new();
    fixture.session("one.jsonl", vec![]);
    fixture.session("two.jsonl", vec![]);

    let status = fixture.provider().status(fixture.root());

    assert!(status.detected);
    assert_eq!(status.sessions, 2);
    assert_eq!(status.capture, None);
    assert!(status.caveats.is_empty());
}

/// Files that are not transcripts share the directory; only `.jsonl` counts.
#[test]
fn non_transcript_files_are_not_counted_as_sessions() {
    let fixture = Fixture::new();
    fixture.session("one.jsonl", vec![]);
    write_lines(
        &fixture
            .home
            .path()
            .join("projects")
            .join(encode_project_dir(fixture.root()))
            .join("notes.md"),
        &[fixture.cwd_line()],
    );

    assert_eq!(fixture.provider().status(fixture.root()).sessions, 1);
}

/// The encoded directory name is a hint, not a contract. When it does not
/// match, every candidate directory is read and `cwd` still finds the session.
#[test]
fn a_session_under_an_unexpected_directory_name_is_still_found() {
    let fixture = Fixture::new();
    fixture.session_in("some-other-name", "one.jsonl", vec![]);

    assert_eq!(fixture.provider().status(fixture.root()).sessions, 1);
}

/// A session recorded in a different workspace is not this workspace's.
#[test]
fn a_session_for_another_workspace_is_not_counted() {
    let fixture = Fixture::new();
    write_lines(
        &fixture
            .home
            .path()
            .join("projects")
            .join("C--elsewhere")
            .join("one.jsonl"),
        &[json!({ "cwd": "C:/elsewhere" }).to_string()],
    );

    assert_eq!(fixture.provider().status(fixture.root()).sessions, 0);
}

#[test]
fn a_home_with_no_projects_directory_reports_no_sessions() {
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();

    let status = ClaudeCode::with_home(home.path()).status(workspace.path());

    assert!(status.detected);
    assert_eq!(status.sessions, 0);
}

#[test]
fn hooks_installed_in_the_workspace_are_reported_as_project_capture() {
    let fixture = Fixture::new();
    let path = project_settings_path(fixture.root());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let (content, _) = hooks_json::plan_merge(&path, Some(fixture.root())).unwrap();
    std::fs::write(&path, content).unwrap();

    assert_eq!(
        fixture.provider().status(fixture.root()).capture,
        Some(InstallScope::Project)
    );
}

#[test]
fn hooks_installed_in_the_home_directory_are_reported_as_user_capture() {
    let fixture = Fixture::new();
    let path = user_settings_path(fixture.home.path());
    let (content, _) = hooks_json::plan_merge(&path, Some(fixture.root())).unwrap();
    std::fs::write(&path, content).unwrap();

    assert_eq!(
        fixture.provider().status(fixture.root()).capture,
        Some(InstallScope::User)
    );
}

/// Both installed at once: the workspace's own settings are the more specific
/// answer, and are what the capture row should name.
#[test]
fn a_project_install_wins_over_a_user_install() {
    let fixture = Fixture::new();
    for path in [
        project_settings_path(fixture.root()),
        user_settings_path(fixture.home.path()),
    ] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let (content, _) = hooks_json::plan_merge(&path, Some(fixture.root())).unwrap();
        std::fs::write(&path, content).unwrap();
    }

    assert_eq!(
        fixture.provider().status(fixture.root()).capture,
        Some(InstallScope::Project)
    );
}

#[test]
fn a_user_install_plan_targets_settings_json_in_the_home_directory() {
    let fixture = Fixture::new();

    let plan = fixture
        .provider()
        .install_plan(fixture.root(), InstallScope::User)
        .unwrap();

    assert_eq!(plan.provider, ProviderId::ClaudeCode);
    assert_eq!(plan.scope, InstallScope::User);
    assert_eq!(
        plan.writes[0].path,
        fixture.home.path().join("settings.json")
    );
    assert!(!plan.writes[0].merges_existing);
    assert!(
        plan.caveats.iter().any(|c| c.contains("every repository")),
        "got: {:?}",
        plan.caveats
    );
}

#[test]
fn a_plan_that_merges_an_existing_file_says_a_backup_is_written() {
    let fixture = Fixture::new();
    let path = user_settings_path(fixture.home.path());
    std::fs::write(&path, r#"{ "model": "opus" }"#).unwrap();

    let plan = fixture
        .provider()
        .install_plan(fixture.root(), InstallScope::User)
        .unwrap();

    assert!(plan.writes[0].merges_existing);
    assert!(
        plan.caveats.iter().any(|c| c.contains("backup is written")),
        "got: {:?}",
        plan.caveats
    );
}

#[test]
fn history_reads_every_session_for_the_workspace_into_records() {
    let fixture = Fixture::new();
    fixture.session(
        "one.jsonl",
        vec![fixture.edit_turn("Move the parser into its own module", "t1", "src/a.rs")],
    );
    fixture.session(
        "two.jsonl",
        vec![fixture.edit_turn("Now update the callers to match", "t2", "src/b.rs")],
    );

    let HistoryMined {
        records, labels, ..
    } = fixture.provider().history(fixture.root()).unwrap();

    let mut paths: Vec<&str> = records.iter().map(|r| r.path.as_str()).collect();
    paths.sort();
    assert_eq!(paths, ["src/a.rs", "src/b.rs"]);
    assert_eq!(labels.len(), 2);
    assert!(records.iter().all(|r| r.provider == ProviderId::ClaudeCode));
}

/// One counter across the whole read, so ordering survives the merge in
/// `providers::history`.
#[test]
fn the_sequence_number_keeps_rising_across_sessions() {
    let fixture = Fixture::new();
    fixture.session(
        "one.jsonl",
        vec![fixture.edit_turn("Move the parser into its own module", "t1", "src/a.rs")],
    );
    fixture.session(
        "two.jsonl",
        vec![fixture.edit_turn("Now update the callers to match", "t2", "src/b.rs")],
    );

    let HistoryMined { records, .. } = fixture.provider().history(fixture.root()).unwrap();

    assert_eq!(
        records.iter().map(|r| r.seq).collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn history_from_a_home_that_does_not_exist_is_empty_rather_than_an_error() {
    let fixture = Fixture::new();
    let provider = ClaudeCode::with_home(fixture.home.path().join("nope"));

    let HistoryMined {
        records, labels, ..
    } = provider.history(fixture.root()).unwrap();

    assert!(records.is_empty());
    assert!(labels.is_empty());
}
