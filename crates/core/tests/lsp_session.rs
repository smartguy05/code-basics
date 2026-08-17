//! Every language server for one workspace, behind one actor, against `cb-fake-lsp`.
//!
//! These live here rather than beside `session.rs` for the same reason
//! `lsp_client.rs` does: the session's whole job is owning *processes*, and
//! `env!("CARGO_BIN_EXE_cb-fake-lsp")` only exists in an integration target.
//!
//! # What is being pinned
//!
//! Not "does a request work" — `lsp_client.rs` already proves that against the
//! real captured Roslyn handshake. What is proved here is the set of answers
//! that are **not** answers, because every one of them is a shape that a
//! plausible implementation flattens into an empty success: a language with no
//! server installed, a server still coming up, an extension nobody claims, a
//! capability the server does not advertise, a server that died, and a session
//! that has been torn down because the workspace was replaced. `Availability`
//! has six variants precisely so those stay apart, and a test suite that only
//! exercised the happy path would not notice them collapsing.
//!
//! **Every test is wrapped in a timeout.** An actor test that can hang reads as
//! a broken suite rather than a broken actor.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cb_core::lsp::model::{Availability, ServerStatus};
use cb_core::lsp::registry::Probe;
use cb_core::lsp::session::{self, LspHandle};
use cb_core::lsp::settings::{LspConfig, ServerOverride};
use cb_core::lsp::uri::{to_file_uri, UriStyle};
use serde_json::{json, Value};
use tempfile::TempDir;

const FAKE: &str = env!("CARGO_BIN_EXE_cb-fake-lsp");

/// The outer bound on any one test. Generous — these spawn real processes and
/// some of them spawn a second one after a restart — but finite.
const TEST_TIMEOUT: Duration = Duration::from_secs(45);

/// The fake's own self-destruct, so a test killed halfway through cannot leave a
/// stray process behind.
const FAKE_SELF_TIMEOUT_MS: &str = "40000";

macro_rules! bounded {
    ($body:expr) => {
        tokio::time::timeout(TEST_TIMEOUT, $body)
            .await
            .expect("the test timed out — the session hung, which is the bug")
    };
}

/// Wait for a condition rather than for a duration. The bounded outer timeout is
/// what turns a condition that never comes true into a failure.
async fn until(mut condition: impl FnMut() -> bool) {
    while !condition() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// [`until`], but give up after `deadline` and say so.
///
/// Used where a state genuinely might never arrive: letting the outer timeout
/// catch that costs forty-five seconds and reports "the session hung", which is
/// the wrong diagnosis for a state machine that simply went somewhere else.
async fn until_or(deadline: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let started = std::time::Instant::now();
    while !condition() {
        if started.elapsed() > deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    true
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// A started session over a temporary workspace.
///
/// `handle` is declared first so it drops first, and `Drop` requests teardown
/// before the temporary directory goes: the servers' cwd *is* that directory.
struct Harness {
    handle: LspHandle,
    dir: TempDir,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.handle.request_teardown();
    }
}

impl Harness {
    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// The TypeScript row of the status surface, which is the only server these
    /// tests configure.
    fn typescript(&self) -> Option<ServerStatus> {
        self.handle
            .status()
            .servers
            .into_iter()
            .find(|server| server.id == "typescript")
    }

    fn typescript_state(&self) -> Option<Availability> {
        self.typescript().map(|server| server.state)
    }
}

/// The full capability set the fake advertises, minus whatever a test removes.
///
/// Trimmed from the real Roslyn 2.140.9 handshake in `lsp_client.rs` to the keys
/// this layer gates on. `textDocumentSync: 2` is Incremental, which is what the
/// real server offers and what `protocol::document_range` is written against.
fn capabilities(without: &[&str]) -> Value {
    let mut capabilities = json!({
        "textDocumentSync": { "openClose": true, "change": 2, "save": {} },
        "definitionProvider": true,
        "typeDefinitionProvider": true,
        "implementationProvider": true,
        "referencesProvider": { "workDoneProgress": true },
        "documentSymbolProvider": true,
    });
    for key in without {
        capabilities.as_object_mut().unwrap().remove(*key);
    }
    capabilities
}

/// A workspace whose TypeScript server is the scripted fake, and whose other
/// three languages are switched off.
///
/// The other three are disabled so the status surface is the same on every
/// machine: this repository's own developer box really does have Roslyn
/// installed, and a test that asserted on the whole server list would pass here
/// and fail on a machine with a different set of extensions.
fn config(script: &Path, program: &str) -> LspConfig {
    let mut env = BTreeMap::new();
    env.insert(
        "CB_FAKE_LSP_SCRIPT".to_string(),
        script.display().to_string(),
    );
    env.insert(
        "CB_FAKE_LSP_TIMEOUT_MS".to_string(),
        FAKE_SELF_TIMEOUT_MS.to_string(),
    );

    let mut servers = BTreeMap::new();
    servers.insert(
        "typescript".to_string(),
        ServerOverride {
            program: Some(program.to_string()),
            // Deliberately empty rather than absent: the built-in list is
            // `--stdio`, which the fake does not understand.
            args: Some(Vec::new()),
            env,
            ..ServerOverride::default()
        },
    );
    for id in ["csharp", "rust", "python"] {
        servers.insert(
            id.to_string(),
            ServerOverride {
                enabled: Some(false),
                ..ServerOverride::default()
            },
        );
    }
    LspConfig { servers }
}

fn harness(script: Value) -> Harness {
    built(FAKE, |_| script)
}

fn harness_with_program(script: Value, program: &str) -> Harness {
    built(program, |_| script)
}

/// A harness whose script is built from the workspace root, for the tests whose
/// scripted replies have to name files inside it.
fn built(program: &str, build: impl FnOnce(&Path) -> Value) -> Harness {
    let dir = tempfile::tempdir().expect("a temp dir");
    let mut script = build(dir.path());
    announce_readiness(&mut script);
    let path = dir.path().join("script.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&script).expect("a script")).expect("write");

    let handle = session::start(dir.path().to_path_buf(), Some(config(&path, program)), 1);
    Harness { handle, dir }
}

/// Make the fake say what the server it is standing in for says.
///
/// These tests occupy the **typescript** slot, and the real
/// `typescript-language-server` announces that it is ready by ending a
/// `$/progress` whose `begin` was titled "Initializing JS/TS language features…"
/// under a per-request UUID token. Until that arrives the session is `Loading`,
/// which is correct and is the whole point of `Readiness::ProgressTitle`.
///
/// So the fake has to send it, or every test here would be measuring the
/// readiness ceiling rather than the behaviour it names. Added centrally rather
/// than in thirty scripts, and only the readiness pair is added — a test that
/// wants a server which never becomes ready sets its own `emitOnInitialized`.
fn announce_readiness(script: &mut Value) {
    let Some(object) = script.as_object_mut() else {
        return;
    };
    if object.contains_key("emitOnInitialized") {
        return;
    }
    let token = "9f2b7c1e-fake-4a2d-9e11-000000000001";
    object.insert(
        "emitOnInitialized".to_string(),
        json!([
            {
                "method": "$/progress",
                "params": {
                    "token": token,
                    "value": {
                        "kind": "begin",
                        "title": "Initializing JS/TS language features…"
                    }
                }
            },
            {
                "method": "$/progress",
                "params": { "token": token, "value": { "kind": "end" } }
            }
        ]),
    );
}

/// A machine with nothing at all installed, so "not found" is a fact about the
/// test rather than about whoever is running it.
struct NothingInstalled;

impl Probe for NothingInstalled {
    fn on_path(&self, _name: &str) -> Option<PathBuf> {
        None
    }
    fn is_file(&self, _path: &Path) -> bool {
        false
    }
    fn is_dir(&self, _path: &Path) -> bool {
        false
    }
    fn read_dir(&self, _path: &Path) -> Vec<PathBuf> {
        Vec::new()
    }
    fn home(&self) -> Option<PathBuf> {
        None
    }
    fn env(&self, _key: &str) -> Option<String> {
        None
    }
}

fn bare_workspace() -> Harness {
    let dir = tempfile::tempdir().expect("a temp dir");
    let handle = session::start_with_probe(
        dir.path().to_path_buf(),
        None,
        1,
        Arc::new(NothingInstalled),
    );
    Harness { handle, dir }
}

/// One location, at `line`/`character` (both 0-based, LSP's own numbering).
fn location(path: &Path, line: u32, start: u32, end: u32) -> Value {
    json!({
        "uri": to_file_uri(path, UriStyle::Encoded).expect("an absolute path"),
        "range": {
            "start": { "line": line, "character": start },
            "end": { "line": line, "character": end }
        }
    })
}

/// The real hierarchical `documentSymbol` answer, abridged from `lsp_client.rs`.
fn document_symbols() -> Value {
    json!([{
        "name": "CodeBasics.Inspector",
        "kind": 3,
        "range": { "start": { "line": 2, "character": 0 }, "end": { "line": 107, "character": 1 } },
        "selectionRange": { "start": { "line": 2, "character": 10 }, "end": { "line": 2, "character": 30 } },
        "children": [{
            "name": "Collections",
            "kind": 5,
            "range": { "start": { "line": 18, "character": 0 }, "end": { "line": 106, "character": 1 } },
            "selectionRange": { "start": { "line": 18, "character": 22 }, "end": { "line": 18, "character": 33 } },
            "children": [{
                "name": "TryGetElements(ClrObject, IReadOnlyList<ClrObject>, int) : bool",
                "kind": 6,
                "range": { "start": { "line": 25, "character": 4 }, "end": { "line": 47, "character": 5 } },
                "selectionRange": { "start": { "line": 25, "character": 23 }, "end": { "line": 25, "character": 37 } },
                "children": []
            }]
        }]
    }])
}

/// Bring the TypeScript server up and wait until it can be asked something.
///
/// The first request is what starts it — lazily, by design — and that request is
/// answered `Starting` rather than blocked, so a caller that wants an answer
/// asks twice. That is the behaviour, not a wrinkle of the harness.
async fn started(harness: &Harness) {
    let answer = harness
        .handle
        .find_usages(&harness.file("main.ts"), 1, 0)
        .await;
    assert_eq!(
        answer.outcome,
        Availability::Starting,
        "the first request starts the server and says so"
    );
    until(|| harness.typescript_state() == Some(Availability::Ready)).await;
}

// ---------------------------------------------------------------------------
// Nothing installed, and nothing claimed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_language_with_no_server_installed_is_not_configured_and_says_how_to_get_one() {
    // The single most important behaviour in this file. An empty `Ready` here
    // would render as "0 usages" for a symbol with forty, about a language this
    // machine cannot answer questions about at all.
    bounded!(async {
        let harness = bare_workspace();
        let answer = harness
            .handle
            .find_usages(&harness.file("app.ts"), 1, 0)
            .await;

        assert_eq!(answer.outcome, Availability::NotConfigured);
        assert_eq!(
            answer.total, None,
            "no server answered, so there is no count — `Some(0)` would be a claim"
        );
        assert!(answer.usages.is_empty());
        let message = answer.message.expect("a reason");
        assert!(
            message.contains("typescript-language-server"),
            "the reason must name what to install: {message}"
        );
        assert_ne!(answer.outcome, Availability::Ready);
    });
}

#[tokio::test]
async fn a_file_no_server_claims_names_the_extension_rather_than_going_silent() {
    bounded!(async {
        let harness = bare_workspace();
        let answer = harness
            .handle
            .find_usages(&harness.file("README.md"), 1, 0)
            .await;

        assert_eq!(answer.outcome, Availability::NotConfigured);
        assert_eq!(answer.total, None);
        let message = answer.message.expect("a reason");
        assert!(
            message.contains("md"),
            "the reason must name the extension nobody claimed: {message}"
        );
    });
}

#[tokio::test]
async fn a_server_that_is_not_installed_is_listed_with_everywhere_that_was_searched() {
    bounded!(async {
        let harness = bare_workspace();
        // Status is a fact about resolution, not about a request: nothing has
        // been asked, and the row is already there to be rendered.
        let servers = harness.handle.status().servers;
        assert_eq!(servers.len(), 4, "four languages, none of them installed");

        for server in &servers {
            assert_eq!(server.state, Availability::NotConfigured, "{}", server.id);
            assert!(
                !server.looked_for.is_empty(),
                "\"not found\" is only actionable if the user can see where we looked: {}",
                server.id
            );
            assert!(server.hint.is_some(), "{}", server.id);
        }
    });
}

#[tokio::test]
async fn a_disabled_server_says_the_configuration_turned_it_off() {
    // Disabled and absent are different things to tell somebody, and only one of
    // them is fixed by installing something.
    bounded!(async {
        let harness = harness(json!({ "capabilities": capabilities(&[]) }));
        let answer = harness
            .handle
            .find_usages(&harness.file("Program.cs"), 1, 0)
            .await;

        assert_eq!(answer.outcome, Availability::NotConfigured);
        assert_eq!(answer.total, None);
        let message = answer.message.expect("a reason");
        assert!(
            message.contains("config.json"),
            "a switched-off server must point at the file that switched it off: {message}"
        );
    });
}

#[tokio::test]
async fn a_configured_program_that_does_not_exist_fails_and_names_the_config_file() {
    // Not `NotConfigured`: the user configured it, and it is broken. Telling
    // them to install something would send them looking in the wrong place.
    bounded!(async {
        let harness = harness_with_program(
            json!({ "capabilities": capabilities(&[]) }),
            "cb-definitely-not-a-real-language-server",
        );
        let answer = harness
            .handle
            .find_usages(&harness.file("app.ts"), 1, 0)
            .await;

        assert_eq!(answer.outcome, Availability::Failed);
        assert_eq!(answer.total, None);
        let message = answer.message.expect("a reason");
        assert!(
            message.contains(".code-basics/config.json")
                && message.contains("cb-definitely-not-a-real-language-server"),
            "the failure must name both the program and the file that named it: {message}"
        );
    });
}

// ---------------------------------------------------------------------------
// Starting is an answer
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_request_that_starts_the_server_says_starting_rather_than_waiting_for_it() {
    // Roslyn's project load is tens of seconds. A UI request that blocked behind
    // it is a frozen widget, so the request that triggers the start returns
    // immediately with the reason there is no answer yet.
    bounded!(async {
        let harness = harness(json!({ "capabilities": capabilities(&[]) }));
        let answer = harness
            .handle
            .find_usages(&harness.file("app.ts"), 1, 0)
            .await;

        assert_eq!(answer.outcome, Availability::Starting);
        assert_eq!(answer.total, None, "starting is not a count");
        assert!(answer.usages.is_empty());
        assert!(answer.message.is_some());

        // And it really did start, rather than reporting `Starting` forever.
        until(|| harness.typescript_state() == Some(Availability::Ready)).await;
    });
}

// ---------------------------------------------------------------------------
// A real answer
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_ready_server_answers_with_rows_a_true_total_and_the_server_that_said_so() {
    bounded!(async {
        let harness = built(FAKE, |root| {
            json!({
                "capabilities": capabilities(&[]),
                "steps": [{
                    "on": "textDocument/references",
                    "reply": [location(&root.join("used.ts"), 1, 14, 24)]
                }]
            })
        });
        std::fs::write(
            harness.file("used.ts"),
            "export const value = 1;\nconst other = usedSymbol;\n",
        )
        .expect("write");

        started(&harness).await;
        let answer = harness
            .handle
            .find_usages(&harness.file("main.ts"), 1, 0)
            .await;

        assert_eq!(answer.outcome, Availability::Ready);
        assert_eq!(answer.total, Some(1), "a real count, from a real answer");
        assert!(!answer.truncated);
        assert_eq!(answer.message, None);
        assert_eq!(answer.server.as_deref(), Some("typescript"));

        let usage = &answer.usages[0];
        assert_eq!(usage.path.as_deref(), Some(Path::new("used.ts")));
        assert_eq!(usage.label, "used.ts");
        assert_eq!(
            usage.line, 2,
            "1-based at the IPC boundary, like the gutter"
        );
        assert_eq!(usage.snippet, "const other = usedSymbol;");
        let highlight = usage.highlight.expect("the match is inside the snippet");
        assert_eq!((highlight.start, highlight.end), (14, 24));
    });
}

#[tokio::test]
async fn a_genuinely_empty_answer_is_ready_with_a_total_of_zero() {
    // The other half of the first test in this file. `Some(0)` and `None` are
    // different answers and this is the one that licenses "no usages".
    bounded!(async {
        let harness = harness(json!({
            "capabilities": capabilities(&[]),
            "steps": [{ "on": "textDocument/references", "reply": [] }]
        }));
        started(&harness).await;

        let answer = harness
            .handle
            .find_usages(&harness.file("main.ts"), 1, 0)
            .await;
        assert_eq!(answer.outcome, Availability::Ready);
        assert_eq!(answer.total, Some(0));
        assert!(answer.usages.is_empty());
    });
}

#[tokio::test]
async fn an_unadvertised_implementation_capability_empties_that_group_and_says_so() {
    // The three goto questions are asked concurrently and merged. A capability
    // the server does not offer must leave *its* group empty with a reason,
    // while the other two still answer — "this server cannot tell you" and
    // "there are none" are different claims.
    bounded!(async {
        let harness = built(FAKE, |root| {
            let declared = root.join("declared.ts");
            let typed = root.join("typed.ts");
            json!({
                "capabilities": capabilities(&["implementationProvider"]),
                "steps": [
                    { "on": "textDocument/definition", "reply": [location(&declared, 0, 16, 20)] },
                    { "on": "textDocument/typeDefinition", "reply": [location(&typed, 0, 12, 16)] },
                    // Scripted to answer anyway: the only thing that may keep
                    // this group empty is the missing capability.
                    { "on": "textDocument/implementation", "reply": [location(&declared, 0, 0, 4)] }
                ]
            })
        });
        std::fs::write(harness.file("declared.ts"), "export function used() {}\n").expect("write");
        std::fs::write(harness.file("typed.ts"), "export type Used = string;\n").expect("write");

        started(&harness).await;
        let answer = harness
            .handle
            .goto_definition(&harness.file("main.ts"), 1, 0)
            .await;

        assert_eq!(answer.outcome, Availability::Ready);
        assert_eq!(answer.declarations.len(), 1, "{answer:?}");
        assert_eq!(answer.type_definitions.len(), 1, "{answer:?}");
        assert!(
            answer.implementations.is_empty(),
            "an unadvertised capability must not be answered from a reply we \
             should never have asked for"
        );
        let message = answer.message.expect("an empty group must say why");
        assert!(
            message.contains("implementation"),
            "the reason must name the group it is about: {message}"
        );
    });
}

#[tokio::test]
async fn a_goto_answer_nobody_could_be_asked_for_is_not_ready_with_three_empty_lists() {
    // The one case where the outcome of a `DefinitionResult` is not `Ready`.
    // There is one outcome for three groups, so a *partial* refusal can only be
    // reported in the message (see the test above); when **no** group could be
    // asked, three empty lists under `outcome: Ready` would state three things
    // about the user's code — declared nowhere, implements nothing, has no type —
    // on the strength of a conversation that never happened.
    bounded!(async {
        let harness = harness(json!({
            "capabilities": capabilities(&[
                "definitionProvider",
                "implementationProvider",
                "typeDefinitionProvider",
            ]),
            // Scripted to answer anyway: the only thing that may empty these
            // groups is the missing capability.
            "steps": [
                { "on": "textDocument/definition", "reply": [] },
                { "on": "textDocument/implementation", "reply": [] },
                { "on": "textDocument/typeDefinition", "reply": [] }
            ]
        }));
        started(&harness).await;

        let answer = harness
            .handle
            .goto_definition(&harness.file("main.ts"), 1, 0)
            .await;

        assert_eq!(
            answer.outcome,
            Availability::Unsupported,
            "a healthy server that advertises none of the three: {answer:?}"
        );
        assert!(answer.declarations.is_empty());
        assert!(answer.implementations.is_empty());
        assert!(answer.type_definitions.is_empty());
        let message = answer.message.expect("every refused group must say why");
        for group in ["declarations", "implementations", "type definitions"] {
            assert!(
                message.contains(group),
                "the reason must name every group it is about: {message}"
            );
        }
    });
}

#[tokio::test]
async fn anchors_for_a_document_no_server_could_be_told_about_fail_rather_than_answer_empty() {
    // `documentSymbol` is a question about an **open buffer**: a server that was
    // never sent a `didOpen` answers `null`, which decodes to an empty list and
    // is indistinguishable from "this file declares nothing". A request may name
    // a file the editor never opened — a palette jump, a preview, a usage row's
    // target — so failing to read it has to become the answer rather than being
    // dropped.
    bounded!(async {
        let harness = harness(json!({
            "capabilities": capabilities(&[]),
            // Scripted to answer, so an empty list here could only come from us.
            "steps": [{ "on": "textDocument/documentSymbol", "reply": document_symbols() }]
        }));
        started(&harness).await;

        let answer = harness
            .handle
            .declaration_anchors(&harness.file("absent.ts"))
            .await;

        assert_eq!(answer.outcome, Availability::Failed, "{answer:?}");
        assert!(answer.anchors.is_empty());
        let message = answer.message.expect("a failure must say what happened");
        assert!(
            message.contains("absent.ts") && message.contains("could not be read"),
            "the reason must name the file and what went wrong: {message}"
        );
    });
}

#[tokio::test]
async fn declaration_anchors_come_from_the_documents_symbols() {
    bounded!(async {
        let harness = harness(json!({
            "capabilities": capabilities(&[]),
            "steps": [{ "on": "textDocument/documentSymbol", "reply": document_symbols() }]
        }));
        // On disk, because `documentSymbol` is a question about a buffer and the
        // request is what opens one: a file nothing can read is now a failure
        // rather than an empty answer.
        std::fs::write(harness.file("main.ts"), "export const a = 1;\n").expect("write");
        started(&harness).await;

        let answer = harness
            .handle
            .declaration_anchors(&harness.file("main.ts"))
            .await;
        assert_eq!(answer.outcome, Availability::Ready);
        assert_eq!(answer.message, None);

        let names: Vec<&str> = answer.anchors.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Collections", "TryGetElements"],
            "a namespace earns no row, and a method's row is drawn on its name \
             rather than on its whole signature"
        );
        let method = &answer.anchors[1];
        assert_eq!(method.selection_line, 26, "1-based");
        assert_eq!(
            method.character, 23,
            "0-based UTF-16, and aimed at the identifier rather than the body"
        );
    });
}

// ---------------------------------------------------------------------------
// The late server
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_document_opened_before_the_server_started_is_replayed_to_it() {
    // The rule that breaks in the app and not in a test: a server that comes up
    // after the user has already opened files is told nothing about them unless
    // the open set is replayed, and "the server has never heard of this buffer"
    // surfaces as an empty answer rather than as an error.
    //
    // Observed by scripting the fake to die the moment it receives a `didOpen`.
    // The control below is the other half of the claim: the same script, the
    // same start, no document opened — and the server stays up. So the death is
    // caused by the replay and by nothing else.
    bounded!(async {
        let script = json!({
            "capabilities": capabilities(&[]),
            "steps": [{ "on": "textDocument/didOpen", "misbehave": "exitBeforeReply" }]
        });

        let control = harness(script.clone());
        started(&control).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(
            control.typescript_state(),
            Some(Availability::Ready),
            "nothing was opened, so nothing was sent, so the server is alive"
        );

        let replayed = harness(script);
        // Before anything has started it: the mirror is all there is.
        replayed
            .handle
            .open_document(&replayed.file("main.ts"), "export const a = 1;\n")
            .await;
        // And this is what starts it.
        let answer = replayed
            .handle
            .find_usages(&replayed.file("main.ts"), 1, 0)
            .await;
        assert_eq!(answer.outcome, Availability::Starting);

        // One automatic restart is allowed per minute; the replay kills that one
        // too, so the language ends up permanently failed. Either way the only
        // thing that can have killed it is the replayed `didOpen`.
        let died = until_or(Duration::from_secs(15), || {
            replayed.typescript_state() == Some(Availability::Failed)
        })
        .await;
        assert!(
            died,
            "the replayed `didOpen` never reached the server, so every file opened \
             while it was starting is invisible to it — which surfaces as an empty \
             answer and not as an error. State: {:?}",
            replayed.typescript_state()
        );
        let status = replayed.typescript().expect("a row");
        assert!(
            status.detail.is_some(),
            "a failed server must say what happened"
        );
    });
}

// ---------------------------------------------------------------------------
// Document versions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_version_on_the_wire_is_the_mirrors_and_never_restarts_after_a_reopen() {
    // Two counters used to exist. `documents.rs` keeps a per-path high-water
    // version that survives a close, precisely so a version can never go
    // backwards; `client.rs` kept a private counter that started at 1 on every
    // `didOpen` and forgot the document on `didClose`. `session::dispatch`
    // destructured the `SyncAction` with `..` and threw the mirror's number
    // away, so `Documents::version()` was an authority nobody consulted — a
    // plausible-looking number that had never been sent.
    //
    // Nothing could see the divergence, because document sync is the one thing
    // this subsystem does that a server never answers. Hence
    // `notificationJournal` on the fake: the wire, on disk, in order.
    bounded!(async {
        let dir = tempfile::tempdir().expect("a temp dir");
        let journal = dir.path().join("notifications.jsonl");
        let mut script = json!({
            "capabilities": capabilities(&[]),
            "notificationJournal": journal.display().to_string(),
            "steps": [{ "on": "*", "reply": [] }]
        });
        announce_readiness(&mut script);
        let script_path = dir.path().join("script.json");
        std::fs::write(&script_path, serde_json::to_vec_pretty(&script).unwrap()).unwrap();
        let handle = session::start(
            dir.path().to_path_buf(),
            Some(config(&script_path, FAKE)),
            1,
        );
        let harness = Harness { handle, dir };

        started(&harness).await;
        let file = harness.file("main.ts");
        harness.handle.open_document(&file, "const a = 1;\n").await;
        harness
            .handle
            .change_document(&file, "const a = 2;\n")
            .await;
        harness.handle.close_document(&file).await;
        // The re-open is the whole point: the mirror is at 3 by now and the old
        // client counter would start again at 1.
        harness.handle.open_document(&file, "const a = 3;\n").await;
        harness
            .handle
            .change_document(&file, "const a = 4;\n")
            .await;

        let arrived = until_or(Duration::from_secs(10), || {
            sync_versions(&journal).len() >= 4
        })
        .await;
        let versions = sync_versions(&journal);
        assert!(
            arrived,
            "the fake never received four sync notifications; got {versions:?}"
        );

        let mut rising = versions.clone();
        rising.dedup();
        assert_eq!(versions, rising, "a version was repeated: {versions:?}");
        assert!(
            versions.windows(2).all(|pair| pair[0] < pair[1]),
            "the version went backwards across the close and re-open: {versions:?}. \
             That is the one thing the mirror's high-water rule exists to prevent, \
             and it means the number on the wire is not the mirror's."
        );
    });
}

/// Every `didOpen`/`didChange` version the fake recorded, in wire order.
fn sync_versions(journal: &Path) -> Vec<i64> {
    let Ok(text) = std::fs::read_to_string(journal) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| {
            matches!(
                entry["method"].as_str(),
                Some("textDocument/didOpen") | Some("textDocument/didChange")
            )
        })
        .filter_map(|entry| entry["params"]["textDocument"]["version"].as_i64())
        .collect()
}

// ---------------------------------------------------------------------------
// Death
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_server_that_dies_mid_request_reports_the_death_and_never_an_empty_list() {
    bounded!(async {
        let harness = harness(json!({
            "capabilities": capabilities(&[]),
            "steps": [{ "on": "textDocument/references", "misbehave": "exitBeforeReply" }]
        }));
        started(&harness).await;

        let answer = harness
            .handle
            .find_usages(&harness.file("main.ts"), 1, 0)
            .await;
        assert_eq!(answer.outcome, Availability::Failed);
        assert_eq!(
            answer.total, None,
            "a dead server has not told us there are no usages"
        );
        assert!(answer.usages.is_empty());
        assert!(answer.message.is_some());
    });
}

// ---------------------------------------------------------------------------
// Teardown
// ---------------------------------------------------------------------------

#[tokio::test]
async fn teardown_is_safe_twice_and_a_superseded_session_serves_nothing() {
    // `AppState` clears its caches inside a `std::sync::Mutex` guard, so
    // teardown has to be a non-blocking send — and that send is the atomic step:
    // after it, nothing may be served under the old workspace's name.
    bounded!(async {
        let harness = harness(json!({
            "capabilities": capabilities(&[]),
            "steps": [{ "on": "textDocument/references", "reply": [] }]
        }));
        started(&harness).await;

        harness.handle.request_teardown();
        harness.handle.request_teardown();

        let usages = harness
            .handle
            .find_usages(&harness.file("main.ts"), 1, 0)
            .await;
        assert_eq!(usages.outcome, Availability::Failed);
        assert_eq!(usages.total, None);
        assert!(
            usages
                .message
                .as_deref()
                .is_some_and(|message| message.contains("shut down")),
            "the reason must be the shutdown and not a made-up server failure: \
             {usages:?}"
        );

        let definitions = harness
            .handle
            .goto_definition(&harness.file("main.ts"), 1, 0)
            .await;
        assert_eq!(definitions.outcome, Availability::Failed);
        assert!(definitions.declarations.is_empty());

        let anchors = harness
            .handle
            .declaration_anchors(&harness.file("main.ts"))
            .await;
        assert_eq!(anchors.outcome, Availability::Failed);
        assert!(anchors.anchors.is_empty());

        // Documents may still be pushed at a dead session without panicking;
        // they simply go nowhere.
        harness
            .handle
            .open_document(&harness.file("main.ts"), "export const a = 1;\n")
            .await;
        harness
            .handle
            .change_document(&harness.file("main.ts"), "export const b = 2;\n")
            .await;
        harness
            .handle
            .close_document(&harness.file("main.ts"))
            .await;

        until(|| harness.handle.status().servers.is_empty()).await;
    });
}

#[tokio::test]
async fn a_handle_reports_the_root_and_generation_it_was_started_for() {
    // The generation is how `AppState` tells a live session from one that was
    // superseded while a request was in flight.
    bounded!(async {
        let harness = bare_workspace();
        assert_eq!(harness.handle.root(), harness.root());
        assert_eq!(harness.handle.generation(), 1);
    });
}
