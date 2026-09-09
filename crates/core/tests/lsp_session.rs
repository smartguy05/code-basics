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
use cb_core::lsp::registry::{Language, Probe};
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
        // Spelled as the real Roslyn server spells it (measured 2026-09-04): an
        // options object, with `prepareProvider` inside it rather than a second
        // top-level key.
        "renameProvider": { "prepareProvider": true },
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

    let handle = session::start(
        dir.path().to_path_buf(),
        Some(config(&path, program)),
        1,
        &[],
    );
    Harness { handle, dir }
}

/// A harness that eagerly warms `warm` at start, as `open_workspace` does for the
/// languages a workspace contains.
fn built_warming(script: Value, warm: &[cb_core::lsp::registry::Language]) -> Harness {
    let dir = tempfile::tempdir().expect("a temp dir");
    let mut script = script;
    announce_readiness(&mut script);
    let path = dir.path().join("script.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&script).expect("a script")).expect("write");
    let handle = session::start(dir.path().to_path_buf(), Some(config(&path, FAKE)), 1, warm);
    Harness { handle, dir }
}

#[tokio::test]
async fn warming_a_present_language_starts_its_server_with_no_request() {
    // `open_workspace` warms the languages a workspace contains; the server must
    // come up on its own, before any usages or rename request. That is the whole
    // point of warming on open — Roslyn's project load is what it hides.
    bounded!(async {
        let harness = built_warming(
            json!({ "capabilities": capabilities(&[]) }),
            &[Language::TypeScript],
        );
        until(|| harness.typescript_state() == Some(Availability::Ready)).await;
    });
}

#[tokio::test]
async fn warming_only_touches_the_named_languages() {
    // Warm a *disabled* language (C#): `ensure_started` acts only on an `Idle`
    // server with a spec, so a disabled one is a no-op. TypeScript is left
    // unwarmed and stays unstarted — an `Idle` resolved server produces no status
    // row — proving warming reached neither the disabled language nor an unnamed
    // one.
    bounded!(async {
        let harness = built_warming(
            json!({ "capabilities": capabilities(&[]) }),
            &[Language::CSharp],
        );
        let started = until_or(Duration::from_millis(750), || {
            harness.typescript_state().is_some()
        })
        .await;
        assert!(
            !started,
            "an unwarmed language must not start: {:?}",
            harness.typescript_state()
        );
    });
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
        &[],
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
            &[],
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

// ---------------------------------------------------------------------------
// Rename: the answers that are not renames
// ---------------------------------------------------------------------------

/// A `documentChanges` answer, which is the only shape the real Roslyn server
/// sends — it ignores this client's declared `documentChanges: false` outright.
fn workspace_edit(entries: Vec<(PathBuf, Vec<Value>)>) -> Value {
    let changes: Vec<Value> = entries
        .into_iter()
        .map(|(path, edits)| {
            json!({
                "textDocument": {
                    "uri": to_file_uri(&path, UriStyle::Encoded).expect("an absolute path"),
                    // Null, as the real server sends it — which is why the
                    // version cannot be used to detect a stale mirror.
                    "version": Value::Null
                },
                "edits": edits
            })
        })
        .collect();
    json!({ "documentChanges": changes })
}

/// One whole-identifier replacement (0-based, LSP's own numbering).
fn text_edit(line: u32, start: u32, end: u32, new_text: &str) -> Value {
    json!({
        "range": {
            "start": { "line": line, "character": start },
            "end": { "line": line, "character": end }
        },
        "newText": new_text
    })
}

#[tokio::test]
async fn a_rename_with_no_server_installed_is_not_configured_and_renames_nothing() {
    // The rename equivalent of the first test in this file, and a worse failure
    // than a wrong count: an empty `Ready` here would report a successful rename
    // of a symbol nothing was asked about.
    bounded!(async {
        let harness = bare_workspace();
        let answer = harness
            .handle
            .rename(&harness.file("app.ts"), 1, 0, "Old", "New")
            .await;

        assert_eq!(answer.outcome, Availability::NotConfigured);
        assert_eq!(
            answer.total, None,
            "nobody was asked, so there is no count of edits"
        );
        assert!(answer.written.is_empty());
        assert!(answer.buffers.is_empty());
        assert!(answer.failures.is_empty());
        assert!(answer
            .message
            .expect("a reason")
            .contains("language server"));

        let prepared = harness
            .handle
            .prepare_rename(&harness.file("app.ts"), 1, 0)
            .await;
        assert_eq!(prepared.outcome, Availability::NotConfigured);
        assert!(
            !prepared.renameable,
            "an unconfigured server has not said the symbol is renameable"
        );
        assert!(prepared.start_line.is_none());
    });
}

#[tokio::test]
async fn a_rename_that_starts_the_server_says_starting_rather_than_renaming_nothing() {
    bounded!(async {
        let harness = harness(json!({ "capabilities": capabilities(&[]) }));
        let answer = harness
            .handle
            .rename(&harness.file("app.ts"), 1, 0, "Old", "New")
            .await;

        assert_eq!(answer.outcome, Availability::Starting);
        assert_eq!(answer.total, None, "starting is not a rename");
        assert!(answer.written.is_empty());
        until(|| harness.typescript_state() == Some(Availability::Ready)).await;
    });
}

#[tokio::test]
async fn a_rename_a_server_does_not_advertise_is_unsupported_and_not_an_empty_edit() {
    // `Unsupported` is the one outcome that tells the user retrying is
    // pointless, and it must not arrive as a successful rename of nothing. The
    // fake is scripted to answer anyway, so the only thing that can empty this
    // is the missing capability.
    bounded!(async {
        let harness = harness(json!({
            "capabilities": capabilities(&["renameProvider"]),
            "steps": [
                { "on": "textDocument/rename", "reply": { "changes": {} } },
                { "on": "textDocument/prepareRename", "reply": Value::Null }
            ]
        }));
        std::fs::write(harness.file("main.ts"), "export const a = 1;\n").expect("write");
        started(&harness).await;

        let answer = harness
            .handle
            .rename(&harness.file("main.ts"), 1, 13, "a", "b")
            .await;
        assert_eq!(answer.outcome, Availability::Unsupported);
        assert_eq!(answer.total, None);
        assert!(answer.written.is_empty());
        assert!(answer.buffers.is_empty());

        let prepared = harness
            .handle
            .prepare_rename(&harness.file("main.ts"), 1, 13)
            .await;
        assert_eq!(prepared.outcome, Availability::Unsupported);
        assert!(!prepared.renameable);
    });
}

#[tokio::test]
async fn a_server_that_renames_but_cannot_prepare_is_unsupported_only_for_the_prepare() {
    // Two capability fields rather than one. A server advertising
    // `renameProvider: true` with no `prepareProvider` answers `-32601` to
    // `prepareRename`, and reading that as "this server cannot rename" would
    // refuse a rename that works perfectly well.
    bounded!(async {
        let mut caps = capabilities(&["renameProvider"]);
        caps.as_object_mut()
            .unwrap()
            .insert("renameProvider".into(), json!(true));
        let harness = harness(json!({
            "capabilities": caps,
            "steps": [{ "on": "textDocument/rename", "reply": { "changes": {} } }]
        }));
        std::fs::write(harness.file("main.ts"), "export const a = 1;\n").expect("write");
        started(&harness).await;

        let prepared = harness
            .handle
            .prepare_rename(&harness.file("main.ts"), 1, 13)
            .await;
        assert_eq!(prepared.outcome, Availability::Unsupported);
        assert!(!prepared.renameable);

        let answer = harness
            .handle
            .rename(&harness.file("main.ts"), 1, 13, "a", "b")
            .await;
        assert_eq!(
            answer.outcome,
            Availability::Ready,
            "the rename itself is available: {answer:?}"
        );
        assert_eq!(answer.total, Some(0));
    });
}

#[tokio::test]
async fn a_server_that_answers_an_empty_edit_is_ready_with_zero_edits() {
    // `Some(0)` and `None` again. A server may genuinely find nothing to change,
    // and that is a real answer — not the same thing as nobody being asked, and
    // not a failure.
    bounded!(async {
        let harness = harness(json!({
            "capabilities": capabilities(&[]),
            "steps": [{ "on": "textDocument/rename", "reply": Value::Null }]
        }));
        std::fs::write(harness.file("main.ts"), "export const a = 1;\n").expect("write");
        started(&harness).await;

        let answer = harness
            .handle
            .rename(&harness.file("main.ts"), 1, 13, "a", "b")
            .await;
        assert_eq!(answer.outcome, Availability::Ready, "{answer:?}");
        assert_eq!(answer.total, Some(0));
        assert!(answer.written.is_empty());
        assert!(answer.buffers.is_empty());
        assert!(answer.failures.is_empty());
        assert_eq!(answer.message, None);
        assert_eq!(answer.server.as_deref(), Some("typescript"));
    });
}

#[tokio::test]
async fn a_rename_for_a_document_the_server_was_never_told_about_is_refused_not_answered() {
    // The single most important line in the feature, from the outside. A rename
    // uses `Needs::OpenDocument` where `references` uses `Needs::Position`,
    // because the ranges coming back are applied to *our* text and so must have
    // been computed from our text.
    //
    // The fake is scripted to **never answer** a rename, so a request that went
    // out would hang until the request deadline — which is what makes this a test
    // of the refusal *preceding the send* rather than of what happens to the
    // answer afterwards. Written the obvious way (a scripted reply naming an
    // unreadable file) it passed under `Needs::Position` too, because `rename.rs`
    // then refuses the unreadable file on its own and says something similar.
    // Hence both halves here: a short deadline, and the phrase only the session's
    // own refusal uses.
    bounded!(async {
        let harness = harness(json!({
            "capabilities": capabilities(&[]),
            "steps": [{ "on": "textDocument/rename", "misbehave": "never" }]
        }));
        started(&harness).await;

        // `absent.ts` is neither on disk nor in any buffer, so no server could be
        // told about it.
        let answer = tokio::time::timeout(
            Duration::from_secs(5),
            harness
                .handle
                .rename(&harness.file("absent.ts"), 1, 0, "Old", "New"),
        )
        .await
        .expect(
            "the rename must be refused without asking the server — this hung, so the \
             request went out about a document the server was never told about",
        );

        assert_eq!(answer.outcome, Availability::Failed, "{answer:?}");
        assert_eq!(answer.total, None);
        assert!(answer.written.is_empty());
        assert!(answer.buffers.is_empty());
        let message = answer.message.expect("a refusal must say what happened");
        assert!(
            message.contains("absent.ts") && message.contains("no server could be told about it"),
            "the reason must be the session's own refusal to ask, naming the file: {message}"
        );
    });
}

#[tokio::test]
async fn a_server_that_dies_mid_rename_reports_the_death_and_never_an_empty_edit() {
    bounded!(async {
        let harness = harness(json!({
            "capabilities": capabilities(&[]),
            "steps": [{ "on": "textDocument/rename", "misbehave": "exitBeforeReply" }]
        }));
        std::fs::write(harness.file("main.ts"), "export const a = 1;\n").expect("write");
        started(&harness).await;

        let answer = harness
            .handle
            .rename(&harness.file("main.ts"), 1, 13, "a", "b")
            .await;
        assert_eq!(answer.outcome, Availability::Failed);
        assert_eq!(
            answer.total, None,
            "a dead server has not told us there was nothing to rename"
        );
        assert!(answer.written.is_empty());
        assert!(answer.message.is_some());
    });
}

#[tokio::test]
async fn a_prepare_rename_answering_null_is_ready_and_simply_not_renameable() {
    // The caret is on a keyword, a comment or a literal. That is a true answer
    // about the caret, so the outcome stays `ready` and `renameable` carries the
    // news — reporting `failed` would tell the user their language server is
    // broken while it is working perfectly.
    bounded!(async {
        let harness = harness(json!({
            "capabilities": capabilities(&[]),
            "steps": [{ "on": "textDocument/prepareRename", "reply": Value::Null }]
        }));
        std::fs::write(harness.file("main.ts"), "export const a = 1;\n").expect("write");
        started(&harness).await;

        let prepared = harness
            .handle
            .prepare_rename(&harness.file("main.ts"), 1, 0)
            .await;
        assert_eq!(prepared.outcome, Availability::Ready, "{prepared:?}");
        assert!(!prepared.renameable);
        assert_eq!(prepared.message, None, "there is nothing wrong to report");
        assert_eq!(prepared.server.as_deref(), Some("typescript"));
        assert!(prepared.start_line.is_none());
    });
}

#[tokio::test]
async fn a_prepare_rename_range_arrives_one_based_with_no_placeholder() {
    // The measured Roslyn shape: a bare `{start, end}` with no placeholder at
    // all, which is why the field's prefill is read out of the buffer.
    bounded!(async {
        let harness = harness(json!({
            "capabilities": capabilities(&[]),
            "steps": [{
                "on": "textDocument/prepareRename",
                "reply": {
                    "start": { "line": 0, "character": 13 },
                    "end": { "line": 0, "character": 14 }
                }
            }]
        }));
        std::fs::write(harness.file("main.ts"), "export const a = 1;\n").expect("write");
        started(&harness).await;

        let prepared = harness
            .handle
            .prepare_rename(&harness.file("main.ts"), 1, 13)
            .await;
        assert_eq!(prepared.outcome, Availability::Ready, "{prepared:?}");
        assert!(prepared.renameable);
        assert_eq!(
            (prepared.start_line, prepared.start_character),
            (Some(1), Some(13)),
            "1-based line, 0-based UTF-16 character"
        );
        assert_eq!(
            (prepared.end_line, prepared.end_character),
            (Some(1), Some(14))
        );
        assert_eq!(prepared.placeholder, None);
    });
}

#[tokio::test]
async fn a_rename_writes_the_closed_file_and_hands_the_open_buffer_back() {
    // The split, end to end: Rust writes what nobody has open, and the editor is
    // handed the rest. A disk write behind an open tab is invisible to the
    // buffer and clobbered by the next save, which is why the two halves cannot
    // be done in one place.
    bounded!(async {
        let harness = built(FAKE, |root| {
            json!({
                "capabilities": capabilities(&[]),
                "steps": [{
                    "on": "textDocument/rename",
                    "reply": workspace_edit(vec![
                        (root.join("main.ts"), vec![text_edit(0, 13, 23, "renamedSymbol")]),
                        (root.join("used.ts"), vec![text_edit(0, 14, 24, "renamedSymbol")]),
                    ])
                }]
            })
        });
        std::fs::write(harness.file("used.ts"), "const other = usedSymbol;\n").expect("write");
        started(&harness).await;

        // `main.ts` is an open buffer and has never been written to disk.
        harness
            .handle
            .open_document(&harness.file("main.ts"), "export const usedSymbol = 1;\n")
            .await;

        let answer = harness
            .handle
            .rename(
                &harness.file("main.ts"),
                1,
                13,
                "usedSymbol",
                "renamedSymbol",
            )
            .await;

        assert_eq!(answer.outcome, Availability::Ready, "{answer:?}");
        assert_eq!(answer.total, Some(2));
        assert!(answer.failures.is_empty());
        assert_eq!(answer.server.as_deref(), Some("typescript"));

        assert_eq!(answer.written.len(), 1, "{answer:?}");
        assert_eq!(answer.written[0].path, Path::new("used.ts"));
        assert_eq!(answer.written[0].edits, 1);
        assert_eq!(
            std::fs::read_to_string(harness.file("used.ts")).expect("read"),
            "const other = renamedSymbol;\n"
        );

        assert_eq!(answer.buffers.len(), 1, "{answer:?}");
        assert_eq!(answer.buffers[0].path, Path::new("main.ts"));
        let edit = &answer.buffers[0].edits[0];
        assert_eq!(
            (edit.start_line, edit.start_character),
            (1, 13),
            "1-based line, 0-based UTF-16 character"
        );
        assert_eq!((edit.end_line, edit.end_character), (1, 23));
        assert_eq!(edit.new_text, "renamedSymbol");
        assert!(
            !harness.file("main.ts").exists(),
            "the open buffer must never be written to disk"
        );
    });
}

#[tokio::test]
async fn a_torn_down_session_refuses_a_rename_rather_than_reporting_one() {
    bounded!(async {
        let harness = harness(json!({
            "capabilities": capabilities(&[]),
            "steps": [{ "on": "textDocument/rename", "reply": Value::Null }]
        }));
        std::fs::write(harness.file("main.ts"), "export const a = 1;\n").expect("write");
        started(&harness).await;
        harness.handle.request_teardown();

        let answer = harness
            .handle
            .rename(&harness.file("main.ts"), 1, 13, "a", "b")
            .await;
        assert_eq!(answer.outcome, Availability::Failed);
        assert_eq!(answer.total, None);
        assert!(answer.written.is_empty());
        assert!(
            answer
                .message
                .as_deref()
                .is_some_and(|message| message.contains("shut down")),
            "the reason must be the shutdown and not a made-up server failure: {answer:?}"
        );

        let prepared = harness
            .handle
            .prepare_rename(&harness.file("main.ts"), 1, 13)
            .await;
        assert_eq!(prepared.outcome, Availability::Failed);
        assert!(!prepared.renameable);
        assert!(prepared
            .message
            .as_deref()
            .is_some_and(|message| message.contains("shut down")));
    });
}
