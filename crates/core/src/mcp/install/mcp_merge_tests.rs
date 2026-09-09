use super::*;
use serde_json::json;

const NAME: &str = "code-basics-sql";
const POINTER: &[&str] = &["mcpServers"];

/// A workspace key of the shape `~/.claude.json`'s `projects` map really uses.
const PROJECT_KEY: &str = r"C:\work\repo";

fn entry() -> Value {
    json!({ "command": r"C:\apps\cb-app.exe", "args": ["mcp-sql"] })
}

#[test]
fn a_fresh_file_gets_only_the_server_map() {
    let out = merged_into_text("", POINTER, NAME, &entry()).expect("merges");
    let value: Value = serde_json::from_str(&out).expect("valid JSON");

    assert_eq!(value["mcpServers"][NAME], entry());
    assert_eq!(
        value.as_object().expect("object").len(),
        1,
        "nothing else is invented"
    );
}

/// The deliberate asymmetry with the wire, pinned from both sides: a config
/// file is pretty-printed and newline-terminated because a human reads and
/// diffs it, while the MCP transport is compact because the spec forbids
/// embedded newlines. Neither may drift into the other.
#[test]
fn config_output_is_pretty_printed_and_newline_terminated_unlike_the_compact_wire() {
    let out = merged_into_text("", POINTER, NAME, &entry()).expect("merges");

    assert!(out.ends_with("}\n"), "one trailing newline: {out:?}");
    assert!(!out.ends_with("}\n\n"), "exactly one: {out:?}");
    assert!(out.contains("\n  \"mcpServers\""), "pretty-printed: {out}");

    let wire = serde_json::to_string(&entry()).expect("wire");
    assert!(!wire.contains('\n'), "the wire carries no newline: {wire}");
}

/// `~/.claude.json` really is a 122 KB file whose `projects` map holds the
/// user's per-repository trust decisions. Dropping any of it would destroy
/// them, so the fixture is deliberately wide.
#[test]
fn every_other_top_level_key_survives_a_round_trip() {
    let mut existing = serde_json::Map::new();
    for i in 0..32 {
        existing.insert(
            format!("key{i}"),
            json!({ "n": i, "deep": { "a": [1, 2, 3] } }),
        );
    }
    let mut projects = serde_json::Map::new();
    projects.insert(
        PROJECT_KEY.to_string(),
        json!({
            "enabledMcpjsonServers": ["something"],
            "disabledMcpjsonServers": [],
            "history": [{ "display": "hi" }]
        }),
    );
    projects.insert(
        r"C:\work\other".to_string(),
        json!({ "allowedTools": ["Bash"] }),
    );
    existing.insert("projects".to_string(), Value::Object(projects));
    existing.insert(
        "mcpServers".to_string(),
        json!({ "other": { "command": "x" } }),
    );
    let before = Value::Object(existing);

    let out = merged_into_text(
        &serde_json::to_string(&before).expect("json"),
        POINTER,
        NAME,
        &entry(),
    )
    .expect("merges");
    let after: Value = serde_json::from_str(&out).expect("valid JSON");

    for (key, value) in before.as_object().expect("object") {
        if key == "mcpServers" {
            continue;
        }
        assert_eq!(&after[key], value, "{key} changed");
    }
    assert_eq!(after["mcpServers"]["other"], json!({ "command": "x" }));
    assert_eq!(after["mcpServers"][NAME], entry());
    assert_eq!(
        after["projects"][PROJECT_KEY]["enabledMcpjsonServers"],
        json!(["something"])
    );
}

#[test]
fn reinstalling_is_a_no_op_and_an_exe_move_refreshes_in_place() {
    let once = merged_into_text("", POINTER, NAME, &entry()).expect("merges");
    let twice = merged_into_text(&once, POINTER, NAME, &entry()).expect("merges");
    assert_eq!(once, twice, "a second identical install changes nothing");

    let moved = json!({ "command": r"D:\moved\cb-app.exe", "args": ["mcp-sql"] });
    let after = merged_into_text(&once, POINTER, NAME, &moved).expect("merges");
    let value: Value = serde_json::from_str(&after).expect("valid JSON");

    assert_eq!(value["mcpServers"][NAME], moved);
    assert_eq!(
        value["mcpServers"].as_object().expect("object").len(),
        1,
        "refreshed in place, not duplicated"
    );
}

#[test]
fn a_nested_pointer_reaches_a_per_project_server_map() {
    let pointer: &[&str] = &["projects", PROJECT_KEY, "mcpServers"];
    let mut projects = serde_json::Map::new();
    projects.insert(PROJECT_KEY.to_string(), json!({ "allowedTools": [] }));
    let mut root = serde_json::Map::new();
    root.insert("projects".to_string(), Value::Object(projects));
    let existing = serde_json::to_string(&Value::Object(root)).expect("json");

    let out = merged_into_text(&existing, pointer, NAME, &entry()).expect("merges");
    let value: Value = serde_json::from_str(&out).expect("valid JSON");

    assert_eq!(value["projects"][PROJECT_KEY]["mcpServers"][NAME], entry());
    assert_eq!(
        value["projects"][PROJECT_KEY]["allowedTools"],
        json!([]),
        "siblings under the pointer survive"
    );
}

#[test]
fn content_that_is_not_a_json_object_is_never_written() {
    for existing in ["[1, 2, 3]", "\"a string\"", "not json at all"] {
        assert!(
            merged_into_text(existing, POINTER, NAME, &entry()).is_err(),
            "{existing} must bail"
        );
    }
}

/// A pointer segment holding something other than an object belongs to a shape
/// we do not understand. Replacing it would destroy it.
#[test]
fn a_pointer_segment_holding_a_non_object_bails_rather_than_replacing_it() {
    assert!(merged_into_text(r#"{"mcpServers": ["a"]}"#, POINTER, NAME, &entry()).is_err());
    assert!(merged_into_text(
        r#"{"projects": 7}"#,
        &["projects", "x", "mcpServers"],
        NAME,
        &entry()
    )
    .is_err());
}

#[test]
fn removal_takes_only_our_entry_and_reports_nothing_when_absent() {
    let with_ours = merged_into_text(
        r#"{"mcpServers":{"other":{"command":"x"}}}"#,
        POINTER,
        NAME,
        &entry(),
    )
    .expect("merges");

    let removed = removal_text(&with_ours, POINTER, NAME)
        .expect("removes")
        .expect("something to remove");
    let value: Value = serde_json::from_str(&removed).expect("valid JSON");
    assert_eq!(value["mcpServers"]["other"], json!({ "command": "x" }));
    assert!(value["mcpServers"].get(NAME).is_none());

    assert!(
        removal_text(&removed, POINTER, NAME).expect("ok").is_none(),
        "a file holding no entry of ours yields no write"
    );
}

/// Asking whether something is installed must not create the parent objects it
/// looked for — a removal on a file with no `mcpServers` key leaves the file
/// alone rather than adding an empty map to it.
#[test]
fn a_removal_that_finds_nothing_invents_no_empty_parents() {
    let existing = r#"{"other": 1}"#;
    assert!(removal_text(existing, POINTER, NAME).expect("ok").is_none());
    assert!(
        removal_text(existing, &["projects", "x", "mcpServers"], NAME)
            .expect("ok")
            .is_none()
    );
}

#[test]
fn is_installed_in_reads_back_what_the_merge_wrote() {
    assert!(!is_installed_in("", POINTER, NAME));
    let out = merged_into_text("", POINTER, NAME, &entry()).expect("merges");
    assert!(is_installed_in(&out, POINTER, NAME));
    assert!(!is_installed_in(&out, POINTER, "someone-else"));
    assert!(!is_installed_in("broken {", POINTER, NAME));
}

// ---------------------------------------------------------------------------
// An unreadable existing file is NOT an absent one.
//
// `read_to_string(path).ok()` collapsed the two, and the consequences compound:
// the plan carried the contents of a *brand-new* config, `merges_existing` came
// back `false`, and `apply_writes_atomically` gates its `.bak` copy on that same
// flag — so the backup was skipped and a 122 KB `~/.claude.json` would have been
// renamed over with four lines. `~/.claude.json` is exactly the file most likely
// to be briefly unreadable, because a running Claude Code rewrites it.
// ---------------------------------------------------------------------------

/// A directory stands in for "a path that exists and cannot be read as text".
///
/// Reading a directory fails with a real `io::Error` on every platform, which is
/// the property under test — the point is that the path EXISTS, so treating the
/// failure as absence is the bug. A permissions-denied file would test the same
/// rule but cannot be created portably.
#[test]
fn an_existing_path_that_cannot_be_read_is_an_error_and_never_a_fresh_file() {
    let dir = tempfile::tempdir().expect("temp dir");
    let unreadable = dir.path().join("claude.json");
    std::fs::create_dir(&unreadable).expect("a directory where a file is expected");

    let error = merged_text(&unreadable, POINTER, NAME, &entry())
        .expect_err("an unreadable existing path must not be planned as a new file");

    let sentence = format!("{error:#}");
    assert!(
        sentence.contains("left untouched") || sentence.contains("could not be read"),
        "the refusal must say the file was not touched: {sentence}"
    );
}

#[test]
fn a_genuinely_absent_file_is_still_planned_as_a_fresh_one() {
    // The other half of the distinction: absence is an ordinary, supported case
    // and must keep working, or the fix above would break every first install.
    let dir = tempfile::tempdir().expect("temp dir");
    let absent = dir.path().join("does-not-exist.json");

    let (text, merges_existing) =
        merged_text(&absent, POINTER, NAME, &entry()).expect("a missing file is fine");

    assert!(!merges_existing, "there is nothing to merge into");
    let value: Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(value["mcpServers"][NAME], entry());
}

#[test]
fn merging_into_a_real_existing_file_reports_that_it_merges() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("claude.json");
    std::fs::write(&path, r#"{"numStartups": 7}"#).expect("write");

    let (text, merges_existing) = merged_text(&path, POINTER, NAME, &entry()).expect("merges");

    assert!(
        merges_existing,
        "a file that is there must be reported as merged into, or apply_writes_atomically skips the .bak"
    );
    let value: Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(value["numStartups"], json!(7), "the sibling key survives");
    assert_eq!(value["mcpServers"][NAME], entry());
}

#[test]
fn an_unreadable_file_is_not_reported_as_not_installed() {
    // `is_installed` swallowed the same error via `.unwrap_or(false)`, so the UI
    // said "not installed" and offered the install — which is what turned an
    // unreadable file into a destructive write in one click.
    let dir = tempfile::tempdir().expect("temp dir");
    let unreadable = dir.path().join("claude.json");
    std::fs::create_dir(&unreadable).expect("a directory where a file is expected");

    assert_eq!(
        None,
        installed_state(&unreadable, POINTER, NAME),
        "an unreadable file has no answer about what is installed in it"
    );
    assert_eq!(
        Some(false),
        installed_state(&dir.path().join("absent.json"), POINTER, NAME),
        "an absent file definitely does not have our server in it"
    );
}
