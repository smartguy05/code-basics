//! The session's pure decisions, headlessly.
//!
//! Everything that needs a process lives in `crates/core/tests/lsp_session.rs`,
//! because `env!("CARGO_BIN_EXE_cb-fake-lsp")` only exists in an integration
//! target. What is left here is small and is exactly the part that turns a
//! failure into a *sentence*: which `Availability` a failure is, what a dead
//! server's detail says, and which text a snippet is read from. Each of those is
//! a place where being plausibly wrong is invisible.

use std::time::Duration;

use super::*;
use crate::lsp::transport::{DeathReason, RequestFailure};

fn death(reason: DeathReason, tail: &[&str]) -> Death {
    Death {
        code: Some(3),
        reason,
        stderr_tail: tail.iter().map(|line| (*line).to_string()).collect(),
    }
}

// ---------------------------------------------------------------------------
// Which outcome a failure is
// ---------------------------------------------------------------------------

#[test]
fn every_request_failure_maps_to_a_distinct_reason_and_none_of_them_to_ready() {
    // The governing rule of the whole subsystem, at its narrowest: not one of
    // these may become an answer, and two of them may not become each other. An
    // unadvertised capability rendered as `Failed` sends the user looking for a
    // broken server, and a still-loading one rendered as `Failed` tells them to
    // restart something that is working.
    let cases = [
        (
            RequestError::Unsupported {
                method: "textDocument/implementation",
                capability: "implementationProvider",
            },
            Availability::Unsupported,
        ),
        (
            RequestError::StillLoading {
                method: "textDocument/references",
                waited: Duration::from_secs(1),
            },
            Availability::Loading,
        ),
        (
            RequestError::Failed {
                method: "textDocument/references",
                failure: RequestFailure::TimedOut {
                    after: Duration::from_secs(1),
                },
            },
            Availability::Failed,
        ),
        (
            RequestError::UnusableUri {
                path: PathBuf::from("relative.cs"),
            },
            Availability::Failed,
        ),
        (
            RequestError::NotOpen {
                path: PathBuf::from("C:/x/y.cs"),
            },
            Availability::Failed,
        ),
    ];

    for (error, expected) in cases {
        let outcome = availability_for(&error);
        assert_eq!(outcome, expected, "for {error:?}");
        assert_ne!(
            outcome,
            Availability::Ready,
            "a failure that reads as `Ready` becomes a count: {error:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// What a dead server's row says
// ---------------------------------------------------------------------------

#[test]
fn a_failure_detail_carries_the_servers_last_words() {
    // By the time a user is told a language feature is off, the server's own
    // final output is usually the only thing that explains why.
    let detail = failure_detail(
        &death(DeathReason::Exited, &["loading", "boom: no SDK found"]),
        &["loading".to_string(), "boom: no SDK found".to_string()],
    );

    assert!(detail.contains("exited"), "{detail}");
    assert!(
        detail.contains("boom: no SDK found"),
        "the reason has to survive into the row: {detail}"
    );
}

#[test]
fn a_failure_detail_keeps_only_the_last_few_lines_of_a_flood() {
    // The tail holds two hundred lines and a status row is one field. Keeping all
    // of it would push the useful part off whatever renders it.
    let flood: Vec<String> = (0..50).map(|n| format!("line {n}")).collect();
    let detail = failure_detail(&death(DeathReason::Exited, &[]), &flood);

    assert!(detail.contains("line 49"), "{detail}");
    assert!(
        !detail.contains("line 44"),
        "only the last five lines belong in a row: {detail}"
    );
}

#[test]
fn a_single_stderr_line_is_not_repeated_under_a_heading() {
    // `Death`'s own `Display` already ends with the last stderr line, so a
    // one-line tail printed again under "Last output" would say it twice.
    let detail = failure_detail(
        &death(DeathReason::Exited, &["only line"]),
        &["only line".to_string()],
    );
    assert_eq!(detail.matches("only line").count(), 1, "{detail}");
}

// ---------------------------------------------------------------------------
// Groups, and the note that keeps an empty one honest
// ---------------------------------------------------------------------------

#[test]
fn a_refused_goto_group_is_empty_and_says_which_group_it_was() {
    // An empty `implementations` list that says nothing is read as a claim about
    // the user's code. It is a claim about our conversation with a server.
    let mut notes = Vec::new();
    let mut refusals = Vec::new();

    let locations = group(
        Err(RequestError::Unsupported {
            method: "textDocument/implementation",
            capability: "implementationProvider",
        }),
        "implementations",
        &mut notes,
        &mut refusals,
    );

    assert!(locations.is_empty());
    assert_eq!(refusals, vec![Availability::Unsupported]);
    assert_eq!(notes.len(), 1);
    assert!(
        notes[0].contains("implementations") && notes[0].contains("implementationProvider"),
        "{:?}",
        notes[0]
    );
}

#[test]
fn a_group_that_answered_nothing_adds_no_note_at_all() {
    // A genuine zero is a real answer about the code, and annotating it would
    // qualify something that needs no qualification.
    let mut notes = Vec::new();
    let mut refusals = Vec::new();

    let locations = group(Ok(Vec::new()), "declarations", &mut notes, &mut refusals);

    assert!(locations.is_empty());
    assert!(notes.is_empty(), "{notes:?}");
    assert!(refusals.is_empty());
}

#[test]
fn a_wholly_refused_goto_answer_takes_the_most_severe_reason_and_never_unsupported() {
    // `Unsupported` is the one outcome that tells the user retrying is pointless
    // ("the server is alive and healthy and does not offer this"), so it is the
    // wrong one to inherit from a corpse. The refusals arrive in a fixed group
    // order — declarations, implementations, type definitions — so taking the
    // first would report a dead server as a healthy one lacking a capability
    // whenever it was the *declarations* provider that was unadvertised.
    assert_eq!(
        worst(&[
            Availability::Unsupported,
            Availability::Failed,
            Availability::Failed
        ]),
        Availability::Failed
    );
    assert_eq!(
        worst(&[Availability::Unsupported, Availability::Loading]),
        Availability::Loading,
        "still loading outranks a capability that will never appear"
    );
    // All three refused for the same reason still says that reason.
    assert_eq!(
        worst(&[Availability::Unsupported; 3]),
        Availability::Unsupported
    );
    // And nothing may promote a refusal into an answer.
    assert_ne!(
        worst(&[Availability::Unsupported, Availability::Failed]),
        Availability::Ready
    );
}

// ---------------------------------------------------------------------------
// Where a request is aimed
// ---------------------------------------------------------------------------

#[test]
fn a_request_is_aimed_at_the_lsp_line_and_the_column_it_was_given() {
    // The request direction of the IPC convention, which is otherwise pinned
    // nowhere: `positions_tests` pins `to_lsp_line`, and every scripted reply in
    // `crates/core/tests/lsp_session.rs` is independent of the position asked
    // about — so a call site that forgot to convert aims every Find Usages one
    // line below the caret, and a server that resolves no symbol there answers
    // `[]`, which arrives as a confident "0 usages".
    assert_eq!(
        request_position(1, 4),
        Position {
            line: 0,
            character: 4
        },
        "the gutter's line 1 is the server's line 0, and the column is untouched"
    );
    assert_eq!(
        request_position(42, 0),
        Position {
            line: 41,
            character: 0
        }
    );
    // Line 0 cannot come from a gutter, and saturating is what `to_lsp_line`
    // does rather than wrapping to four billion.
    assert_eq!(
        request_position(0, 7),
        Position {
            line: 0,
            character: 7
        }
    );
}

// ---------------------------------------------------------------------------
// A count taken from a server that was only promoted at the ceiling
// ---------------------------------------------------------------------------

#[test]
fn a_count_from_a_server_promoted_at_the_ceiling_is_qualified_rather_than_left_bare() {
    // `ReadyState::is_ready()` is deliberately true for `ReadyWithCaveat`, so a
    // request proceeds — and `client::ask`'s own docs make qualifying the answer
    // the caller's obligation, because a count taken in that state is
    // byte-identical to one from a primed server. This is that caller.
    let note = caveat_note(&ReadyState::ReadyWithCaveat {
        detail: "the project load never finished".to_string(),
    })
    .expect("a promoted server must qualify its count");
    assert!(
        note.contains("the project load never finished"),
        "the server's own reason has to survive: {note}"
    );
    assert!(
        note.contains("low"),
        "the qualification has to say which way the number is wrong: {note}"
    );

    // A primed server qualifies nothing: annotating a good answer is how a
    // caveat stops meaning anything.
    assert_eq!(caveat_note(&ReadyState::Ready), None);
    assert_eq!(caveat_note(&ReadyState::Loading), None);
}

#[test]
fn a_promoted_server_carries_its_caveat_in_its_own_field_and_not_as_its_detail() {
    // The status surface's only rule is "say nothing about a ready server", so a
    // caveat delivered as `detail` is dropped whole and a ceiling-promoted server
    // is drawn exactly like a primed one — silence in the titlebar while every
    // count in the file says it may be low. `detail` cannot carry it: for a plain
    // `Ready` that field is the program path, so the two states are
    // indistinguishable there without reading the English.
    let promoted = running_row(
        &ReadyState::ReadyWithCaveat {
            detail: "the project load never finished".to_string(),
        },
        "roslyn.exe",
    );
    assert_eq!(promoted.state, Availability::Ready, "usable, with a caveat");
    assert_eq!(
        promoted.caveat.as_deref(),
        Some("the project load never finished"),
        "the caveat needs a field the frontend can branch on"
    );
    assert_eq!(
        promoted.detail.as_deref(),
        Some("roslyn.exe"),
        "and `detail` goes on meaning what it means for every other ready server"
    );

    let primed = running_row(&ReadyState::Ready, "roslyn.exe");
    assert_eq!(primed.state, Availability::Ready);
    assert_eq!(primed.caveat, None, "a good answer is not qualified");
    assert_eq!(primed.detail.as_deref(), Some("roslyn.exe"));

    let loading = running_row(&ReadyState::Loading, "roslyn.exe");
    assert_eq!(loading.state, Availability::Loading);
    assert_eq!(loading.caveat, None, "nothing has been promoted yet");
    assert!(loading
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("roslyn.exe")));
}

#[test]
fn a_caveat_is_added_to_whatever_the_answer_already_had_to_say() {
    // A goto answer can already carry a note about a refused group, and the two
    // are about different things: one group could not be asked, and the whole
    // answer came from a server that never finished priming.
    let merged = with_caveat(
        Some("No implementations: ...".to_string()),
        Some("qualified"),
    )
    .expect("both must survive");
    assert!(merged.contains("No implementations"), "{merged}");
    assert!(merged.contains("qualified"), "{merged}");

    assert_eq!(
        with_caveat(None, Some("qualified")).as_deref(),
        Some("qualified")
    );
    assert_eq!(
        with_caveat(Some("only".to_string()), None).as_deref(),
        Some("only")
    );
    assert_eq!(with_caveat(None, None), None);
}

// ---------------------------------------------------------------------------
// The shapes an unavailable answer takes
// ---------------------------------------------------------------------------

#[test]
fn no_unready_answer_ever_carries_a_count() {
    // One place decides `total` is `None`, and this is the only route to it.
    let unready = Unready {
        outcome: Availability::Starting,
        message: "starting".to_string(),
        server: Some("csharp".to_string()),
    };
    let usages = unready.usages();

    assert_eq!(usages.outcome, Availability::Starting);
    assert_eq!(usages.total, None);
    assert!(usages.usages.is_empty());
    assert!(!usages.truncated);
    assert_eq!(usages.server.as_deref(), Some("csharp"));
    assert_eq!(usages.message.as_deref(), Some("starting"));
}

#[test]
fn an_unready_answer_with_no_server_names_none() {
    // An absent server names nothing: `Some("typescript")` here would put a
    // server that does not exist in a status line.
    let usages = Unready::not_configured("nothing installed".to_string()).usages();
    assert_eq!(usages.outcome, Availability::NotConfigured);
    assert_eq!(usages.server, None);
}

#[test]
fn the_other_two_shapes_are_empty_with_a_reason() {
    let definitions = Unready {
        outcome: Availability::Loading,
        message: "loading".to_string(),
        server: None,
    }
    .definition();
    assert_eq!(definitions.outcome, Availability::Loading);
    assert!(definitions.declarations.is_empty());
    assert!(definitions.implementations.is_empty());
    assert!(definitions.type_definitions.is_empty());
    assert_eq!(definitions.message.as_deref(), Some("loading"));

    let anchors = Unready {
        outcome: Availability::Failed,
        message: "dead".to_string(),
        server: None,
    }
    .anchors();
    assert_eq!(anchors.outcome, Availability::Failed);
    assert!(anchors.anchors.is_empty());
    assert_eq!(anchors.message.as_deref(), Some("dead"));
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

#[test]
fn a_path_is_routed_by_its_extension_and_an_unclaimed_one_gets_nothing() {
    assert_eq!(
        language_of(Path::new("C:/x/Program.cs")),
        Some(Language::CSharp)
    );
    assert_eq!(
        language_of(Path::new("C:/x/app.tsx")),
        Some(Language::TypeScript)
    );
    assert_eq!(language_of(Path::new("C:/x/main.rs")), Some(Language::Rust));
    assert_eq!(
        language_of(Path::new("C:/x/run.py")),
        Some(Language::Python)
    );
    // Not a default server: asking a C# server about a `.md` file and rendering
    // its answer would be worse than showing nothing.
    assert_eq!(language_of(Path::new("C:/x/README.md")), None);
    assert_eq!(language_of(Path::new("C:/x/Makefile")), None);
}

#[test]
fn every_language_has_a_name_a_human_would_recognise() {
    // These are rendered in a status row beside "not installed", so they have to
    // be the names the user would search for, not the config ids.
    assert_eq!(name(Language::CSharp), "C#");
    for language in LANGUAGES {
        assert!(!name(language).is_empty());
    }
}

// ---------------------------------------------------------------------------
// Where a snippet's text comes from
// ---------------------------------------------------------------------------

#[test]
fn an_open_buffer_beats_the_file_on_disk() {
    // The server answered about the buffer. A snippet read from disk would show a
    // line the answer was not about — silently, and only for unsaved edits, which
    // is the normal state of a file somebody is asking questions about.
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("a.ts");
    std::fs::write(&path, "on disk\nsecond\n").expect("write");

    let mut buffers = BTreeMap::new();
    buffers.insert(path.clone(), "in the editor\nsecond\n".to_string());
    let mut text = WorkspaceText::new(buffers);

    assert_eq!(text.line(&path, 0).as_deref(), Some("in the editor"));
    assert_eq!(text.line(&path, 1).as_deref(), Some("second"));
}

#[test]
fn a_file_nobody_has_open_is_read_from_disk() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("a.ts");
    std::fs::write(&path, "first\nsecond\n").expect("write");

    let mut text = WorkspaceText::new(BTreeMap::new());
    assert_eq!(text.line(&path, 1).as_deref(), Some("second"));
}

#[test]
fn a_line_that_cannot_be_read_is_none_rather_than_an_empty_line() {
    // `results` keeps the row either way — the count must stay right — so this
    // only decides whether there is a snippet, and an empty string would look
    // like a blank line in the file.
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("a.ts");
    std::fs::write(&path, "only one line\n").expect("write");

    let mut text = WorkspaceText::new(BTreeMap::new());
    assert_eq!(text.line(&path, 9), None, "past the end of the file");
    assert_eq!(
        text.line(&dir.path().join("gone.ts"), 0),
        None,
        "a file that is not there"
    );
}

#[test]
fn a_file_too_large_to_hand_a_server_is_not_read() {
    // Mirrors the symbol index's own 1 MiB rule. A request may name any file in
    // the workspace, and an unbounded read on the actor's own thread is not a
    // thing to leave to chance.
    let dir = tempfile::tempdir().expect("a temp dir");
    let small = dir.path().join("small.ts");
    std::fs::write(&small, "x").expect("write");
    assert_eq!(readable(&small).as_deref(), Some("x"));

    let large = dir.path().join("large.ts");
    std::fs::write(&large, "x".repeat(AUTO_OPEN_MAX_BYTES as usize + 1)).expect("write");
    assert_eq!(readable(&large), None);
}
