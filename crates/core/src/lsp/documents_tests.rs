//! Every ordering rule this module claims to obey, one test each.
//!
//! No process and no async anywhere: that is the reason the state machine was
//! separated from the sending in the first place.

use std::path::{Path, PathBuf};

use super::*;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const CSHARP: ServerId = "roslyn";
const TS: ServerId = "typescript-language-server";
const RUST: ServerId = "rust-analyzer";

fn path(name: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!(r"C:\repo\{name}"))
    } else {
        PathBuf::from(format!("/repo/{name}"))
    }
}

/// The shape of an action, flattened to something a test can assert on in one
/// line. `None` for the text of a close, which carries none.
fn summarise(action: &SyncAction) -> (&'static str, ServerId, String, Option<String>, i32) {
    match action {
        SyncAction::DidOpen {
            server,
            path,
            text,
            version,
        } => ("open", *server, name_of(path), Some(text.clone()), *version),
        SyncAction::DidChange {
            server,
            path,
            text,
            version,
        } => (
            "change",
            *server,
            name_of(path),
            Some(text.clone()),
            *version,
        ),
        SyncAction::DidClose { server, path } => ("close", *server, name_of(path), None, 0),
    }
}

fn name_of(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

fn summarise_all(
    actions: &[SyncAction],
) -> Vec<(&'static str, ServerId, String, Option<String>, i32)> {
    actions.iter().map(summarise).collect()
}

/// Just the kind and the server, for the tests that only care who was told.
fn kinds(actions: &[SyncAction]) -> Vec<(&'static str, ServerId)> {
    actions
        .iter()
        .map(|action| {
            let (kind, server, ..) = summarise(action);
            (kind, server)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Opening
// ---------------------------------------------------------------------------

#[test]
fn opening_a_document_tells_every_server_named() {
    let mut documents = Documents::new();
    let file = path("Order.cs");

    let actions = documents.open(&file, "class Order {}", &[CSHARP, TS]);

    assert_eq!(
        summarise_all(&actions),
        vec![
            (
                "open",
                CSHARP,
                "Order.cs".to_string(),
                Some("class Order {}".to_string()),
                1
            ),
            (
                "open",
                TS,
                "Order.cs".to_string(),
                Some("class Order {}".to_string()),
                1
            ),
        ],
        "both servers must be opened at the same version: the document has one \
         state and two servers must not be told different numbers for it"
    );
}

#[test]
fn opening_a_document_on_no_servers_yields_no_actions_but_still_tracks_it() {
    let mut documents = Documents::new();
    let file = path("notes.txt");

    let actions = documents.open(&file, "hello", &[]);

    assert!(
        actions.is_empty(),
        "no server serves this extension, so there is nobody to tell"
    );
    assert!(
        documents.is_open(&file),
        "the buffer is still the editor's buffer, and a server starting later \
         must be replayed it"
    );
    assert_eq!(documents.text(&file), Some("hello"));
}

#[test]
fn did_open_is_idempotent_per_document_and_server() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    documents.open(&file, "one", &[CSHARP]);

    let actions = documents.open(&file, "two", &[CSHARP]);

    assert_eq!(
        summarise_all(&actions),
        vec![(
            "change",
            CSHARP,
            "Order.cs".to_string(),
            Some("two".to_string()),
            2
        )],
        "a second didOpen for a document a server already has open is a protocol \
         violation that some servers answer by discarding their state for it"
    );
}

#[test]
fn a_server_added_to_an_already_open_document_is_opened_while_the_others_are_changed() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    documents.open(&file, "one", &[CSHARP]);

    let actions = documents.open(&file, "two", &[CSHARP, TS]);

    assert_eq!(
        summarise_all(&actions),
        vec![
            (
                "change",
                CSHARP,
                "Order.cs".to_string(),
                Some("two".to_string()),
                2
            ),
            (
                "open",
                TS,
                "Order.cs".to_string(),
                Some("two".to_string()),
                2
            ),
        ],
        "the same version for both, and the kind decided per server"
    );
}

#[test]
fn a_server_left_out_of_a_later_open_still_hears_about_the_new_text() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    documents.open(&file, "one", &[CSHARP, TS]);

    let actions = documents.open(&file, "two", &[CSHARP]);

    assert_eq!(
        kinds(&actions),
        vec![("change", CSHARP), ("change", TS)],
        "a shrinking server list is not a close; leaving TS on the old text is \
         the desynchronisation this module exists to prevent"
    );
    assert_eq!(documents.servers_for(&file), vec![CSHARP, TS]);
}

// ---------------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------------

#[test]
fn versions_increase_monotonically_per_document() {
    let mut documents = Documents::new();
    let file = path("Order.cs");

    documents.open(&file, "one", &[CSHARP]);
    documents.change(&file, "two");
    documents.change(&file, "three");
    let last = documents.change(&file, "four");

    assert_eq!(documents.version(&file), Some(4));
    assert_eq!(
        summarise_all(&last),
        vec![(
            "change",
            CSHARP,
            "Order.cs".to_string(),
            Some("four".to_string()),
            4
        )]
    );
}

#[test]
fn versions_are_per_document_and_not_global() {
    let mut documents = Documents::new();
    let one = path("One.cs");
    let two = path("Two.cs");

    documents.open(&one, "a", &[CSHARP]);
    documents.change(&one, "aa");
    documents.open(&two, "b", &[CSHARP]);

    assert_eq!(documents.version(&one), Some(2));
    assert_eq!(
        documents.version(&two),
        Some(1),
        "a shared counter would make the numbers meaningless per document"
    );
}

#[test]
fn versions_do_not_restart_across_a_close_and_reopen() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    documents.open(&file, "one", &[CSHARP]);
    documents.change(&file, "two");
    documents.close(&file);

    let actions = documents.open(&file, "three", &[CSHARP]);

    assert_eq!(
        summarise_all(&actions),
        vec![(
            "open",
            CSHARP,
            "Order.cs".to_string(),
            Some("three".to_string()),
            3
        )],
        "a version going backwards makes a server drop the change silently, and \
         nothing anywhere reports the desynchronisation that follows"
    );
    assert_eq!(documents.version(&file), Some(3));
}

#[test]
fn a_replay_does_not_advance_the_version() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    documents.open(&file, "one", &[CSHARP]);
    documents.change(&file, "two");

    let actions = documents.replay_for(TS);

    assert_eq!(
        summarise_all(&actions),
        vec![(
            "open",
            TS,
            "Order.cs".to_string(),
            Some("two".to_string()),
            2
        )],
        "the document did not change; a server that missed it needs the current \
         state, not a new one"
    );
    assert_eq!(documents.version(&file), Some(2));
}

// ---------------------------------------------------------------------------
// Replay — the rule that breaks in the app and not in a test
// ---------------------------------------------------------------------------

#[test]
fn a_server_that_starts_late_gets_the_whole_open_set() {
    let mut documents = Documents::new();
    let first = path("A.cs");
    let second = path("B.cs");
    let third = path("C.cs");
    documents.open(&first, "a", &[]);
    documents.open(&second, "b", &[]);
    documents.open(&third, "c", &[]);

    let actions = documents.replay_for(CSHARP);

    assert_eq!(
        summarise_all(&actions),
        vec![
            ("open", CSHARP, "A.cs".to_string(), Some("a".to_string()), 1),
            ("open", CSHARP, "B.cs".to_string(), Some("b".to_string()), 1),
            ("open", CSHARP, "C.cs".to_string(), Some("c".to_string()), 1),
        ],
        "Roslyn takes tens of seconds to come up, and a file opened while it was \
         starting is invisible to it forever unless it is replayed — invisible \
         being an empty answer rather than an error"
    );
}

#[test]
fn a_replayed_server_then_hears_about_every_later_change() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    documents.open(&file, "one", &[]);
    documents.replay_for(CSHARP);

    let actions = documents.change(&file, "two");

    assert_eq!(
        kinds(&actions),
        vec![("change", CSHARP)],
        "a replay that is not recorded serves the late server once and then \
         leaves it on stale text, answering confidently, forever"
    );
}

#[test]
fn a_restarted_server_is_replayed_the_whole_open_set() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    documents.open(&file, "one", &[CSHARP]);
    documents.change(&file, "two");

    let actions = documents.replay_for(CSHARP);

    assert_eq!(
        summarise_all(&actions),
        vec![(
            "open",
            CSHARP,
            "Order.cs".to_string(),
            Some("two".to_string()),
            2
        )],
        "a restarted process remembers nothing, so skipping documents already \
         recorded as open on it would leave the one case replay exists for unserved"
    );
}

#[test]
fn a_document_edited_while_no_server_was_up_is_replayed_the_edited_text() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    documents.open(&file, "on disk", &[]);
    let during_startup = documents.change(&file, "unsaved edit");

    let actions = documents.replay_for(CSHARP);

    assert!(
        during_startup.is_empty(),
        "there was nobody to tell at the time, which is the whole reason the \
         mirror has to keep the text"
    );
    assert_eq!(
        summarise_all(&actions),
        vec![(
            "open",
            CSHARP,
            "Order.cs".to_string(),
            Some("unsaved edit".to_string()),
            2
        )],
        "a change nobody could be told about must still move the mirror, or the \
         late server is caught up to the text the file had before the user typed"
    );
}

#[test]
fn a_replay_with_nothing_open_yields_no_actions() {
    let mut documents = Documents::new();
    assert!(documents.replay_for(CSHARP).is_empty());
}

#[test]
fn a_replay_does_not_disturb_the_other_servers() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    documents.open(&file, "one", &[TS]);

    let actions = documents.replay_for(CSHARP);

    assert_eq!(kinds(&actions), vec![("open", CSHARP)]);
    assert_eq!(documents.servers_for(&file), vec![CSHARP, TS]);
}

// ---------------------------------------------------------------------------
// Closing
// ---------------------------------------------------------------------------

#[test]
fn close_tells_every_server_that_had_it_open_and_only_those() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    let other = path("Other.cs");
    documents.open(&file, "one", &[CSHARP, TS]);
    documents.open(&other, "two", &[RUST]);

    let actions = documents.close(&file);

    assert_eq!(
        summarise_all(&actions),
        vec![
            ("close", CSHARP, "Order.cs".to_string(), None, 0),
            ("close", TS, "Order.cs".to_string(), None, 0),
        ],
        "rust-analyzer was never told about this document and telling it to \
         close one it does not have is a protocol violation"
    );
    assert!(!documents.is_open(&file));
    assert!(documents.is_open(&other));
}

#[test]
fn close_for_a_document_nobody_opened_yields_no_actions() {
    let mut documents = Documents::new();

    let actions = documents.close(&path("Order.cs"));

    assert!(
        actions.is_empty(),
        "nobody has it open, so there is no server that would accept a didClose"
    );
}

#[test]
fn closing_twice_tells_the_servers_once() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    documents.open(&file, "one", &[CSHARP]);
    documents.close(&file);

    assert!(documents.close(&file).is_empty());
}

#[test]
fn a_closed_document_is_not_replayed() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    documents.open(&file, "one", &[CSHARP]);
    documents.close(&file);

    assert!(
        documents.replay_for(TS).is_empty(),
        "the servers have gone back to disk for it; replaying would re-open a \
         buffer the editor no longer has"
    );
}

// ---------------------------------------------------------------------------
// Changing
// ---------------------------------------------------------------------------

#[test]
fn change_for_a_document_nobody_opened_yields_no_actions() {
    let mut documents = Documents::new();
    let file = path("Order.cs");

    let actions = documents.change(&file, "text");

    assert!(
        actions.is_empty(),
        "the caller has a bug, and inventing an open would send a server text \
         under a version nobody agreed on and hide the bug behind an answer that \
         happens to look right"
    );
    assert!(!documents.is_open(&file), "and nothing was recorded either");
    assert_eq!(documents.version(&file), None);
}

#[test]
fn change_tells_every_server_that_has_it_open() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    documents.open(&file, "one", &[CSHARP, TS]);

    let actions = documents.change(&file, "two");

    assert_eq!(kinds(&actions), vec![("change", CSHARP), ("change", TS)]);
}

// ---------------------------------------------------------------------------
// The buffer this holds
// ---------------------------------------------------------------------------

#[test]
fn text_is_the_editors_buffer_and_the_latest_one() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    documents.open(&file, "on disk", &[CSHARP]);
    documents.change(&file, "unsaved edit");

    assert_eq!(
        documents.text(&file),
        Some("unsaved edit"),
        "a request aimed at a buffer the server thinks is older resolves the \
         caret to a different symbol and answers confidently about the wrong one"
    );
}

#[test]
fn text_and_version_are_gone_once_the_document_is_closed() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    documents.open(&file, "one", &[CSHARP]);
    documents.close(&file);

    assert_eq!(documents.text(&file), None);
    assert_eq!(
        documents.version(&file),
        None,
        "there is no buffer; the servers have gone back to disk"
    );
}

#[test]
fn text_and_version_are_none_for_a_document_never_opened() {
    let documents = Documents::new();
    let file = path("Order.cs");

    assert_eq!(documents.text(&file), None);
    assert_eq!(documents.version(&file), None);
    assert!(documents.servers_for(&file).is_empty());
    assert!(documents.open_paths().is_empty());
}

#[test]
fn open_paths_are_in_path_order() {
    let mut documents = Documents::new();
    documents.open(&path("B.cs"), "b", &[]);
    documents.open(&path("A.cs"), "a", &[]);
    documents.open(&path("C.cs"), "c", &[]);

    let names: Vec<String> = documents.open_paths().iter().map(|p| name_of(p)).collect();
    assert_eq!(names, vec!["A.cs", "B.cs", "C.cs"]);
}

// ---------------------------------------------------------------------------
// Accessors on the action itself
// ---------------------------------------------------------------------------

#[test]
fn an_action_names_its_server_and_its_document() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    let actions = documents.open(&file, "one", &[CSHARP]);

    assert_eq!(actions[0].server(), CSHARP);
    assert_eq!(actions[0].path(), file.as_path());
}

// ---------------------------------------------------------------------------
// The version ceiling
// ---------------------------------------------------------------------------

#[test]
fn a_version_at_the_ceiling_stops_rather_than_wrapping_negative() {
    let mut documents = Documents::new();
    let file = path("Order.cs");
    documents.open(&file, "one", &[CSHARP]);
    documents.force_version(&file, i32::MAX);

    let actions = documents.change(&file, "two");

    assert_eq!(
        documents.version(&file),
        Some(i32::MAX),
        "wrapping would send version -2147483648, which is a version going \
         backwards — the one thing a server drops silently"
    );
    assert_eq!(
        summarise_all(&actions),
        vec![(
            "change",
            CSHARP,
            "Order.cs".to_string(),
            Some("two".to_string()),
            i32::MAX
        )]
    );
}
