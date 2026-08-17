//! One language server, from handshake to shutdown, against `cb-fake-lsp`.
//!
//! These need a live process — the client is a conversation, and a conversation
//! cannot be unit tested — so they live here rather than beside `client.rs`:
//! `env!("CARGO_BIN_EXE_cb-fake-lsp")` only exists in an integration target.
//! What *is* pure (the readiness signal test, the decode-error rewrite) is
//! covered a second time in `lsp/client_tests.rs`, where it can be exhaustive.
//!
//! The capability fixture below is the **real** `Microsoft.CodeAnalysis.
//! LanguageServer` 2.140.9 handshake captured this session, keys and all,
//! including the ones this client ignores. A fixture trimmed to what we read
//! would pass even if the reader stopped tolerating what real servers send.
//!
//! **Every test is wrapped in a timeout.** A client test that can hang reads as
//! a broken suite rather than a broken client, and this repository has already
//! lost real time to exactly that.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cb_core::lsp::client::{Client, ReadyState, RequestError, StartFailure};
use cb_core::lsp::protocol::{Position, SymbolKind, SyncKind};
use cb_core::lsp::registry::{Language, Readiness, ServerSpec, Timeouts};
use cb_core::lsp::transport::{DeathReason, RequestFailure};
use cb_core::lsp::uri::UriStyle;
use serde_json::{json, Value};
use tempfile::TempDir;

const FAKE: &str = env!("CARGO_BIN_EXE_cb-fake-lsp");

/// The outer bound on any one test. Generous — a cold Windows process spawn is
/// not fast — but finite, which is the whole point.
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The fake's own self-destruct, so a test killed halfway through cannot leave
/// a stray process on the machine.
const FAKE_SELF_TIMEOUT_MS: &str = "25000";

/// Long enough that a healthy fake always beats it, short enough that a test
/// asserting a timeout does not dominate the suite.
const SHORT: Duration = Duration::from_secs(8);

/// Run a test body under the suite's timeout.
macro_rules! bounded {
    ($body:expr) => {
        tokio::time::timeout(TEST_TIMEOUT, $body)
            .await
            .expect("the test timed out — the client hung, which is the bug")
    };
}

/// Wait for a condition rather than for a duration. The bounded outer timeout
/// is what turns a condition that never comes true into a failure.
async fn until(mut condition: impl FnMut() -> bool) {
    while !condition() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// A scripted fake, not yet started, so a test can adjust the spec first.
struct Fake {
    dir: TempDir,
    spec: ServerSpec,
}

/// A started client and the temp directory its script lives in. `client` is
/// declared first so it drops first: the transport's `Drop` kills the process
/// tree, and only then is the directory removed.
struct Session {
    client: Client,
    _dir: TempDir,
}

fn fake(script: Value) -> Fake {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("script.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&script).expect("a script")).expect("write");

    let mut env = BTreeMap::new();
    env.insert("CB_FAKE_LSP_SCRIPT".to_string(), path.display().to_string());
    env.insert(
        "CB_FAKE_LSP_TIMEOUT_MS".to_string(),
        FAKE_SELF_TIMEOUT_MS.to_string(),
    );

    Fake {
        spec: ServerSpec {
            id: "csharp",
            language: Language::CSharp,
            language_id: "csharp",
            program: PathBuf::from(FAKE),
            args: Vec::new(),
            env,
            uri_style: UriStyle::Plain,
            // Overridden per test. Immediate keeps the readiness machinery out
            // of the way of everything that is not about readiness.
            readiness: Readiness::Immediate,
            timeouts: Timeouts {
                first_request: SHORT,
                request: SHORT,
                document_symbol: SHORT,
            },
        },
        dir,
    }
}

async fn start(fake: Fake) -> Session {
    let Fake { dir, spec } = fake;
    let client = Client::start(spec, dir.path())
        .await
        .expect("the fake must complete the handshake");
    Session { client, _dir: dir }
}

/// A file inside the session's root. Never created on disk: the server is told
/// the text, and the fake never reads it.
fn a_file(session: &Session) -> PathBuf {
    session.client.root().join("Collections.cs")
}

fn anywhere() -> Position {
    Position {
        line: 25,
        character: 23,
    }
}

// ---------------------------------------------------------------------------
// Captured payloads — real Roslyn 2.140.9, this session
// ---------------------------------------------------------------------------

/// The real capability set, verbatim. `referencesProvider` is an *options
/// object* rather than `true`, `positionEncoding` is **absent**, and
/// `_vs_onAutoInsertProvider` is a non-standard key: all three are shapes a
/// specification-shaped reader gets wrong.
fn roslyn_capabilities() -> Value {
    json!({
        "_vs_onAutoInsertProvider": { "_vs_triggerCharacters": ["'", "/", "\n", "\""] },
        "textDocumentSync": { "openClose": true, "change": 2, "save": {} },
        "hoverProvider": true,
        "definitionProvider": true,
        "typeDefinitionProvider": true,
        "implementationProvider": true,
        "referencesProvider": { "workDoneProgress": true },
        "documentSymbolProvider": true,
        "codeLensProvider": { "resolveProvider": true },
        "renameProvider": { "prepareProvider": true },
        "workspace": {}
    })
}

/// The real `textDocument/definition` answer: `Location[]` despite
/// `linkSupport: true` having been offered.
fn definition_result() -> Value {
    json!([{
        "uri": "file:///C:/Users/AnthonyJames/Documents/Code/code-basics/sidecar/inspector/Collections.cs",
        "range": {
            "start": { "line": 25, "character": 23 },
            "end": { "line": 25, "character": 37 }
        }
    }])
}

/// The real hierarchical `documentSymbol` answer, `glyph` and all.
fn document_symbol_result() -> Value {
    json!([{
        "glyph": 48,
        "name": "CodeBasics.Inspector",
        "detail": "CodeBasics.Inspector",
        "kind": 3,
        "range": { "start": { "line": 2, "character": 0 }, "end": { "line": 107, "character": 1 } },
        "selectionRange": { "start": { "line": 2, "character": 10 }, "end": { "line": 2, "character": 30 } },
        "children": [{
            "glyph": 7,
            "name": "Collections",
            "detail": "Collections",
            "kind": 5,
            "range": { "start": { "line": 18, "character": 0 }, "end": { "line": 106, "character": 1 } },
            "selectionRange": { "start": { "line": 18, "character": 22 }, "end": { "line": 18, "character": 33 } },
            "children": [{
                "glyph": 49,
                "name": "TryGetElements(ClrObject, IReadOnlyList<ClrObject>, int) : bool",
                "detail": "TryGetElements(ClrObject, IReadOnlyList<ClrObject>, int) : bool",
                "kind": 6,
                "range": { "start": { "line": 25, "character": 4 }, "end": { "line": 47, "character": 5 } },
                "selectionRange": { "start": { "line": 25, "character": 23 }, "end": { "line": 25, "character": 37 } },
                "children": []
            }]
        }]
    }])
}

// ---------------------------------------------------------------------------
// The handshake
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_real_roslyn_handshake_is_accepted_and_read_correctly() {
    bounded!(async {
        let session = start(fake(json!({ "capabilities": roslyn_capabilities() }))).await;
        let capabilities = session.client.capabilities();

        assert!(
            capabilities.references,
            "`referencesProvider` is an options object, not `true` — reading it as \
             false would disable the feature against the one server this was built for"
        );
        assert!(capabilities.definition);
        assert!(capabilities.implementation);
        assert!(capabilities.type_definition);
        assert!(capabilities.document_symbol);
        assert_eq!(capabilities.sync, SyncKind::Incremental);
        assert_eq!(capabilities.position_encoding, None);
        assert_eq!(session.client.readiness(), ReadyState::Ready);
        assert!(session.client.pid().is_some());
    });
}

#[tokio::test]
async fn an_absent_position_encoding_means_utf16_and_is_not_a_refusal() {
    // Verified: the real server omits the field entirely. Treating silence as a
    // refusal would make C# unusable against the only server we can test.
    bounded!(async {
        let session = start(fake(json!({
            "capabilities": { "textDocumentSync": 2, "referencesProvider": true }
        })))
        .await;
        assert_eq!(session.client.capabilities().position_encoding, None);
        assert!(session.client.capabilities().encoding_is_utf16());
    });
}

#[tokio::test]
async fn a_server_that_insists_on_utf8_positions_is_refused_outright() {
    // Proceeding would compute every column on every non-ASCII line wrongly,
    // and wrongly in a way nothing downstream could detect.
    bounded!(async {
        let Fake { dir, spec } = fake(json!({
            "capabilities": {
                "textDocumentSync": 2,
                "positionEncoding": "utf-8",
                "referencesProvider": true
            }
        }));
        let failure = Client::start(spec, dir.path())
            .await
            .expect_err("a server we cannot compute positions for is unusable");

        assert_eq!(
            failure,
            StartFailure::UnsupportedEncoding {
                encoding: "utf-8".to_string()
            }
        );
        assert!(
            failure.to_string().contains("utf-8"),
            "the message must name the encoding the server insisted on: {failure}"
        );
    });
}

#[tokio::test]
async fn a_server_that_does_not_want_document_sync_is_refused_outright() {
    bounded!(async {
        // Both spellings of "none": the bare number, and an object that omits
        // `change` — which per the specification also means none.
        for capabilities in [
            json!({ "textDocumentSync": 0, "referencesProvider": true }),
            json!({ "textDocumentSync": { "openClose": true }, "referencesProvider": true }),
        ] {
            let Fake { dir, spec } = fake(json!({ "capabilities": capabilities.clone() }));
            let failure = Client::start(spec, dir.path())
                .await
                .expect_err("a server we cannot keep in sync cannot be trusted");
            assert_eq!(failure, StartFailure::NoDocumentSync, "for {capabilities}");
        }
    });
}

#[tokio::test]
async fn an_initialize_result_with_no_capabilities_is_a_handshake_failure() {
    // Not "a server that provides nothing": the field is required, so its
    // absence means the handshake was not understood at all.
    bounded!(async {
        let Fake { dir, spec } = fake(json!({ "steps": [] }));
        let failure = Client::start(spec, dir.path())
            .await
            .expect_err("an initialize result with no capabilities is unreadable");
        assert!(
            matches!(failure, StartFailure::Capabilities(_)),
            "got {failure:?}"
        );
    });
}

#[tokio::test]
async fn a_server_that_never_answers_initialize_fails_the_handshake_rather_than_hanging() {
    bounded!(async {
        let Fake { dir, mut spec } = fake(json!({
            "steps": [{ "on": "initialize", "misbehave": "never" }]
        }));
        spec.timeouts.first_request = Duration::from_millis(400);

        let failure = Client::start(spec, dir.path())
            .await
            .expect_err("a server that never answers is not a started server");
        assert_eq!(
            failure,
            StartFailure::Handshake(RequestFailure::TimedOut {
                after: Duration::from_millis(400)
            })
        );
    });
}

// ---------------------------------------------------------------------------
// Capability negotiation is a gate, not a formality
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_two_caller_owned_arguments_reach_the_server_it_spawns() {
    // `registry` documented `--clientProcessId` and `--extensionLogDirectory` as
    // "the caller's to append" and nothing appended them: the live server ran
    // with `--stdio --logLevel Warning --autoLoadProjects` and nothing else, and
    // `.code-basics/lsp-logs/` was never created — which is why an hour of
    // diagnosis had no server log to read. Nothing short of reading the spawned
    // process's own command line can see that, so the fake writes its argv down.
    bounded!(async {
        let Fake { dir, mut spec } = fake(json!({
            "capabilities": roslyn_capabilities(),
            "steps": [{
                "on": "textDocument/references",
                "misbehave": "echoArgv",
                "argvFile": "argv.txt",
                "reply": []
            }]
        }));
        // The arguments discovery chooses for Roslyn: the flags are appended to
        // these and to nothing else.
        spec.args = ["--stdio", "--logLevel", "Warning", "--autoLoadProjects"]
            .iter()
            .map(|a| a.to_string())
            .collect();
        let session = Session {
            client: Client::start(spec, dir.path()).await.expect("started"),
            _dir: dir,
        };

        let logs = session.client.root().join(".code-basics").join("lsp-logs");
        assert!(
            logs.is_dir(),
            "the log directory has to exist before the server is told to write there: {}",
            logs.display()
        );

        assert_eq!(
            session
                .client
                .references(&a_file(&session), anywhere(), true)
                .await,
            Ok(Vec::new())
        );

        // `argvFile` is relative, and `Launch::cwd` is the workspace root.
        let argv: Vec<String> = std::fs::read_to_string(session.client.root().join("argv.txt"))
            .expect("the fake records its own command line")
            .lines()
            .map(str::to_string)
            .collect();

        let pid = std::process::id().to_string();
        assert!(
            argv.windows(2)
                .any(|pair| pair == ["--clientProcessId".to_string(), pid.clone()]),
            "our own pid, so the server exits if this process dies without an \
             orderly shutdown: {argv:?}"
        );
        let named_dir = argv
            .windows(2)
            .find(|pair| pair[0] == "--extensionLogDirectory")
            .map(|pair| pair[1].clone())
            .unwrap_or_else(|| panic!("no --extensionLogDirectory in {argv:?}"));
        assert_eq!(
            PathBuf::from(named_dir),
            logs,
            "the flag has to name the directory that was created, not another one"
        );
    });
}

#[tokio::test]
async fn a_missing_implementation_provider_is_unsupported_and_never_an_empty_list() {
    // The single most important behaviour in this file. The server here is
    // scripted to answer `[]` to *both* requests — so the only thing separating
    // them is the advertised capability, and the two outcomes must differ.
    // "This server does not provide implementations" and "there are none" are
    // different claims, and rendering the second for the first is the failure
    // this whole subsystem exists to prevent.
    bounded!(async {
        let mut capabilities = roslyn_capabilities();
        capabilities
            .as_object_mut()
            .unwrap()
            .remove("implementationProvider");

        let session = start(fake(json!({
            "capabilities": capabilities,
            "steps": [
                { "on": "textDocument/implementation", "reply": [] },
                { "on": "textDocument/references", "reply": [] }
            ]
        })))
        .await;
        let file = a_file(&session);

        let unsupported = session.client.implementation(&file, anywhere()).await;
        assert_eq!(
            unsupported,
            Err(RequestError::Unsupported {
                method: "textDocument/implementation",
                capability: "implementationProvider",
            })
        );
        assert_ne!(
            unsupported,
            Ok(Vec::new()),
            "an unadvertised capability must not read as an empty answer"
        );

        let genuinely_empty = session.client.references(&file, anywhere(), true).await;
        assert_eq!(
            genuinely_empty,
            Ok(Vec::new()),
            "the same `[]` from the same server is a real zero when the \
             capability was advertised"
        );
    });
}

#[tokio::test]
async fn every_unadvertised_request_names_the_capability_it_needed() {
    bounded!(async {
        let session = start(fake(json!({
            // Sync only: nothing at all is advertised, and the catch-all would
            // happily answer every one of these with `null`.
            "capabilities": { "textDocumentSync": 2 },
            "steps": [{ "on": "*", "reply": [] }]
        })))
        .await;
        let file = a_file(&session);
        let c = &session.client;

        let failures = [
            c.references(&file, anywhere(), true).await.unwrap_err(),
            c.definition(&file, anywhere()).await.unwrap_err(),
            c.implementation(&file, anywhere()).await.unwrap_err(),
            c.type_definition(&file, anywhere()).await.unwrap_err(),
        ];
        let expected = [
            ("textDocument/references", "referencesProvider"),
            ("textDocument/definition", "definitionProvider"),
            ("textDocument/implementation", "implementationProvider"),
            ("textDocument/typeDefinition", "typeDefinitionProvider"),
        ];
        for (failure, (method, capability)) in failures.iter().zip(expected) {
            assert_eq!(failure, &RequestError::Unsupported { method, capability });
        }

        assert_eq!(
            c.document_symbols(&file).await,
            Err(RequestError::Unsupported {
                method: "textDocument/documentSymbol",
                capability: "documentSymbolProvider",
            })
        );
    });
}

// ---------------------------------------------------------------------------
// Readiness
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_immediate_server_is_ready_as_soon_as_the_handshake_is_done() {
    bounded!(async {
        let session = start(fake(json!({ "capabilities": roslyn_capabilities() }))).await;
        assert_eq!(session.client.readiness(), ReadyState::Ready);
    });
}

#[tokio::test]
async fn a_loading_server_is_reported_as_loading_and_not_as_an_empty_answer() {
    // rust-analyzer really does answer `references` from a half-built index,
    // and Roslyn answers before its projects have loaded. A low count is a
    // *wrong* answer, not a partial one — so the request is refused with the
    // reason, even though the server would have answered it.
    bounded!(async {
        let Fake { dir, mut spec } = fake(json!({
            "capabilities": roslyn_capabilities(),
            "steps": [{ "on": "textDocument/references", "reply": [] }]
        }));
        spec.readiness = Readiness::RoslynProjectInit;
        spec.timeouts.first_request = Duration::from_millis(300);
        spec.timeouts.request = Duration::from_millis(300);
        let client = Client::start(spec, dir.path()).await.expect("started");

        assert_eq!(client.readiness(), ReadyState::Loading);
        let answer = client
            .references(&dir.path().join("Collections.cs"), anywhere(), true)
            .await;
        assert_eq!(
            answer,
            Err(RequestError::StillLoading {
                method: "textDocument/references",
                waited: Duration::from_millis(300),
            })
        );
        assert_ne!(answer, Ok(Vec::new()), "still loading is not zero usages");
    });
}

#[tokio::test]
async fn roslyn_is_ready_once_project_initialization_completes() {
    bounded!(async {
        let Fake { dir, mut spec } = fake(json!({
            "capabilities": roslyn_capabilities(),
            "steps": [{ "on": "textDocument/references", "reply": [] }],
            "emitOnInitialized": [
                { "method": "workspace/projectInitializationComplete" }
            ]
        }));
        spec.readiness = Readiness::RoslynProjectInit;
        let client = Client::start(spec, dir.path()).await.expect("started");

        assert_eq!(client.wait_ready(SHORT).await, ReadyState::Ready);
        assert_eq!(
            client
                .references(&dir.path().join("Collections.cs"), anywhere(), true)
                .await,
            Ok(Vec::new()),
            "once ready, an empty answer is a real one"
        );
    });
}

#[tokio::test]
async fn the_readiness_signal_survives_a_flood_of_notifications_in_front_of_it() {
    // The Phase 5 defect, end to end. Roslyn sends thousands of notifications
    // while loading a solution and then `projectInitializationComplete` **once**,
    // so the readiness watch — reading a `broadcast` that drops the oldest
    // messages from a slow subscriber — lagged past the only message it existed
    // to see. It then waited out the 90 s ceiling, published
    // `ReadyWithCaveat`, and every count taken from that server was zero,
    // because the projects genuinely had not loaded.
    //
    // The ceiling here is an hour, so nothing in this test can pass by being
    // promoted: `Ready` inside `SHORT` means the signal itself was seen.
    //
    // The flood is scripted onto `initialize` and the signal is scripted into
    // the *middle* of it, and both details are what make the loss certain rather
    // than merely likely. The client subscribes before `initialize` goes out and
    // the readiness watch only starts once the reply is in, so nothing reads the
    // channel while the flood is written; with hundreds of notifications still to
    // come after the signal, a 256-slot ring has overwritten it before the
    // watcher exists, whatever order the tasks ran in. A flood emitted later,
    // with the watcher already draining it, is a race — measured: it passed
    // against the unfixed code.
    bounded!(async {
        let Fake { dir, mut spec } = fake(json!({
            "capabilities": roslyn_capabilities(),
            "steps": [
                { "on": "textDocument/references", "reply": [] },
                {
                    "on": "initialize",
                    "misbehave": "notificationFlood",
                    "lines": 600,
                    "thenAfter": 100,
                    "then": { "method": "workspace/projectInitializationComplete" }
                }
            ]
        }));
        spec.readiness = Readiness::RoslynProjectInit;
        let client = Client::start_with_ceiling(spec, dir.path(), Duration::from_secs(3600))
            .await
            .expect("started");

        assert_eq!(
            client.wait_ready(SHORT).await,
            ReadyState::Ready,
            "the signal arrived with 500 more notifications behind it; a watch that \
             only read the live stream lost it and reported this server loading \
             until the ceiling"
        );
        assert_eq!(
            client
                .references(&dir.path().join("Collections.cs"), anywhere(), true)
                .await,
            Ok(Vec::new()),
            "and the answer is then a real one rather than a qualified zero"
        );
    });
}

#[tokio::test]
async fn a_progress_end_for_a_matching_token_makes_the_server_ready() {
    bounded!(async {
        let Fake { dir, mut spec } = fake(json!({
            "capabilities": roslyn_capabilities(),
            "emitOnInitialized": [
                { "method": "$/progress", "params": {
                    "token": "rustAnalyzer/Indexing",
                    "value": { "kind": "begin", "title": "Indexing" }
                } },
                { "method": "$/progress", "params": {
                    "token": "rustAnalyzer/Indexing",
                    "value": { "kind": "end" }
                } }
            ]
        }));
        spec.readiness = Readiness::Progress {
            token_prefix: "rustAnalyzer",
        };
        let client = Client::start(spec, dir.path()).await.expect("started");

        assert_eq!(client.wait_ready(SHORT).await, ReadyState::Ready);
    });
}

#[tokio::test]
async fn a_progress_end_for_somebody_elses_token_leaves_the_server_loading() {
    // A `begin` for our token and an `end` for another server's work. Reading
    // either as readiness would declare a still-priming index trustworthy.
    bounded!(async {
        let Fake { dir, mut spec } = fake(json!({
            "capabilities": roslyn_capabilities(),
            "emitOnInitialized": [
                { "method": "$/progress", "params": {
                    "token": "rustAnalyzer/Indexing", "value": { "kind": "begin" }
                } },
                { "method": "$/progress", "params": {
                    "token": "somethingElse/Loading", "value": { "kind": "end" }
                } }
            ]
        }));
        spec.readiness = Readiness::Progress {
            token_prefix: "rustAnalyzer",
        };
        let client = Client::start(spec, dir.path()).await.expect("started");

        assert_eq!(
            client.wait_ready(Duration::from_millis(400)).await,
            ReadyState::Loading
        );
    });
}

#[tokio::test]
async fn a_server_that_never_reports_ready_says_so_rather_than_waiting_forever() {
    // The ceiling. Reporting ready is the only useful thing left to do, and
    // doing it *silently* would be indistinguishable from a healthy server —
    // so the caveat travels with the state.
    bounded!(async {
        let Fake { dir, mut spec } = fake(json!({ "capabilities": roslyn_capabilities() }));
        spec.readiness = Readiness::RoslynProjectInit;
        let client = Client::start_with_ceiling(spec, dir.path(), Duration::from_millis(250))
            .await
            .expect("started");

        assert_eq!(client.readiness(), ReadyState::Loading);
        let state = client.wait_ready(SHORT).await;
        match state {
            ReadyState::ReadyWithCaveat { detail } => {
                assert!(
                    detail.contains("projectInitializationComplete"),
                    "the caveat must name the signal that never came: {detail}"
                );
            }
            other => panic!("expected a caveat, got {other:?}"),
        }
        assert!(client.readiness().is_ready(), "usable, with a caveat");
    });
}

#[tokio::test]
async fn a_server_that_dies_while_loading_is_never_promoted_at_the_ceiling() {
    // The ceiling exists so a signal that never comes does not disable the
    // feature forever. It must not launder a corpse into a working server: the
    // watcher learned about death only through the notification channel
    // closing, and that channel's sender lives on the transport, so it closes
    // when the *client* is dropped and never when the server dies. A
    // `RoslynProjectInit` server that crashed during project load was therefore
    // reported "ready, answers may be incomplete" indefinitely — and
    // `readiness()` is exactly the surface a UI stops showing "loading" on.
    bounded!(async {
        let Fake { dir, mut spec } = fake(json!({
            "capabilities": roslyn_capabilities(),
            // `initialized` is a notification, and a step on it still runs, so
            // the fake dies moments after the handshake completes.
            "steps": [{ "on": "initialized", "misbehave": "exitBeforeReply" }]
        }));
        spec.readiness = Readiness::RoslynProjectInit;
        let ceiling = Duration::from_millis(250);
        let client = Client::start_with_ceiling(spec, dir.path(), ceiling)
            .await
            .expect("the handshake completes before the server goes");

        until(|| client.death().is_some()).await;
        // Well past the ceiling, so a promotion would have happened by now.
        tokio::time::sleep(ceiling * 3).await;

        assert_eq!(
            client.readiness(),
            ReadyState::Loading,
            "a dead server never became ready, and saying it did is a lie the \
             UI cannot see through"
        );
        assert!(!client.readiness().is_ready());
    });
}

// ---------------------------------------------------------------------------
// Document sync
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_version_recorded_is_the_one_the_caller_supplied() {
    // The server rejects a change that does not advance the version, so the
    // version is what makes a dropped notification detectable at all — but the
    // counter is `documents::Documents`, not this. What the client records is
    // *what it sent*, which is what `document_version` claims to return.
    //
    // The numbers below are deliberately not 1, 2, 3: a client that had quietly
    // kept counting for itself would produce those, and passing this test with
    // them would prove nothing. They are the shape the mirror really produces
    // after a close and re-open of an earlier document.
    bounded!(async {
        let session = start(fake(json!({ "capabilities": roslyn_capabilities() }))).await;
        let file = a_file(&session);
        let c = &session.client;

        assert_eq!(c.document_version(&file), None);
        c.did_open(&file, "class A {}", 7).expect("opened");
        assert_eq!(c.document_version(&file), Some(7));

        c.did_change(&file, "class A { }", 8).expect("changed");
        assert_eq!(c.document_version(&file), Some(8));
        c.did_change(&file, "class A { int x; }", 12)
            .expect("changed");
        assert_eq!(c.document_version(&file), Some(12));
    });
}

#[tokio::test]
async fn the_language_id_on_the_wire_is_the_files_and_not_the_servers() {
    // `ServerSpec::language_id_for` is pure and pinned in `registry_tests.rs`;
    // this is the half that is not pure — that `did_open` calls it at all. It
    // did not: it sent `self.spec.language_id` verbatim, so every `.tsx` in this
    // application's own frontend was opened as `typescript` and came back with
    // JSX innards reported as declarations.
    //
    // Read off the wire rather than out of the client, because the client's
    // mirror does not record the id and a server never answers a notification.
    bounded!(async {
        let mut fake = fake(json!({
            "capabilities": roslyn_capabilities(),
            "notificationJournal": "PLACEHOLDER"
        }));
        fake.spec.language = Language::TypeScript;
        fake.spec.language_id = "typescript";
        let journal = fake.dir.path().join("notifications.jsonl");
        rewrite_script(&fake, |script| {
            script["notificationJournal"] = json!(journal.display().to_string());
        });

        let session = start(fake).await;
        let root = session.client.root().to_path_buf();
        session
            .client
            .did_open(&root.join("Card.tsx"), "export const A = 1;", 1)
            .expect("opened");
        session
            .client
            .did_open(&root.join("plain.ts"), "export const B = 2;", 1)
            .expect("opened");

        until(|| opened_language_ids(&journal).len() >= 2).await;
        assert_eq!(
            opened_language_ids(&journal),
            vec!["typescriptreact".to_string(), "typescript".to_string()]
        );
    });
}

/// Every `languageId` a `didOpen` carried, in wire order.
fn opened_language_ids(journal: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(journal) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry["method"] == json!("textDocument/didOpen"))
        .filter_map(|entry| {
            entry["params"]["textDocument"]["languageId"]
                .as_str()
                .map(str::to_string)
        })
        .collect()
}

/// Edit a fake's script on disk before it is started.
///
/// The script is written by [`fake`] and the path it needs — the temp directory
/// — only exists afterwards, so a journal path cannot be part of the literal.
fn rewrite_script(fake: &Fake, edit: impl FnOnce(&mut Value)) {
    let path = fake.dir.path().join("script.json");
    let mut script: Value =
        serde_json::from_slice(&std::fs::read(&path).expect("the script")).expect("json");
    edit(&mut script);
    std::fs::write(&path, serde_json::to_vec_pretty(&script).expect("json")).expect("write");
}

#[tokio::test]
async fn a_change_to_a_document_that_was_never_opened_is_refused() {
    // Inventing a `didOpen` would send the server text it never asked about;
    // sending the change alone would be rejected. Both are worse than saying so.
    bounded!(async {
        let session = start(fake(json!({ "capabilities": roslyn_capabilities() }))).await;
        let file = a_file(&session);

        assert_eq!(
            session.client.did_change(&file, "class A {}", 1),
            Err(RequestError::NotOpen { path: file.clone() })
        );
        assert_eq!(
            session.client.did_close(&file),
            Err(RequestError::NotOpen { path: file })
        );
    });
}

#[tokio::test]
async fn closing_a_document_forgets_it_and_a_reopen_carries_the_callers_version() {
    // Forgetting on close is right — a server discards its state on `didClose`,
    // so nothing here should be remembered about it. What the client must *not*
    // do is invent a fresh number for the re-open: it did, restarting at 1 while
    // the mirror was at 3, and a version going backwards is the one thing the
    // mirror's high-water rule exists to prevent. See
    // `lsp_session.rs::the_version_on_the_wire_is_the_mirrors…`.
    bounded!(async {
        let session = start(fake(json!({ "capabilities": roslyn_capabilities() }))).await;
        let file = a_file(&session);
        let c = &session.client;

        c.did_open(&file, "class A {}", 1).expect("opened");
        c.did_change(&file, "class B {}", 2).expect("changed");
        c.did_close(&file).expect("closed");
        assert_eq!(c.document_version(&file), None);

        c.did_open(&file, "class A {}", 3).expect("reopened");
        assert_eq!(
            c.document_version(&file),
            Some(3),
            "the re-open keeps going from where the mirror left off"
        );
    });
}

#[tokio::test]
async fn opening_a_document_twice_advances_it_rather_than_reopening_it() {
    // Two `didOpen`s for one document is a protocol violation, and the client
    // owns the mirror, so it is the client's job not to commit one.
    bounded!(async {
        let session = start(fake(json!({ "capabilities": roslyn_capabilities() }))).await;
        let file = a_file(&session);
        let c = &session.client;

        c.did_open(&file, "class A {}", 1).expect("opened");
        c.did_open(&file, "class A { }", 2).expect("opened again");
        assert_eq!(c.document_version(&file), Some(2));
    });
}

// ---------------------------------------------------------------------------
// The five requests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn references_come_back_as_locations() {
    bounded!(async {
        let session = start(fake(json!({
            "capabilities": roslyn_capabilities(),
            "steps": [{ "on": "textDocument/references", "reply": definition_result() }]
        })))
        .await;
        let locations = session
            .client
            .references(&a_file(&session), anywhere(), true)
            .await
            .expect("an answer");

        assert_eq!(locations.len(), 1);
        assert!(locations[0].uri.ends_with("Collections.cs"));
        assert_eq!(locations[0].range.start.line, 25);
    });
}

#[tokio::test]
async fn a_single_location_object_is_one_location_and_not_a_shape_error() {
    bounded!(async {
        let session = start(fake(json!({
            "capabilities": roslyn_capabilities(),
            "steps": [{ "on": "textDocument/definition", "reply": definition_result()[0] }]
        })))
        .await;
        let locations = session
            .client
            .definition(&a_file(&session), anywhere())
            .await
            .expect("an answer");
        assert_eq!(locations.len(), 1);
    });
}

#[tokio::test]
async fn a_genuinely_empty_type_definition_answer_is_an_empty_answer() {
    // The real server answered `[]` here for a `var` whose type is in metadata.
    // That is a fact about the code, and it must survive as `Ok(vec![])`.
    bounded!(async {
        let session = start(fake(json!({
            "capabilities": roslyn_capabilities(),
            "steps": [{ "on": "textDocument/typeDefinition", "reply": [] }]
        })))
        .await;
        assert_eq!(
            session
                .client
                .type_definition(&a_file(&session), anywhere())
                .await,
            Ok(Vec::new())
        );
    });
}

#[tokio::test]
async fn document_symbols_arrive_flattened_with_their_containers() {
    bounded!(async {
        let session = start(fake(json!({
            "capabilities": roslyn_capabilities(),
            "steps": [{ "on": "textDocument/documentSymbol", "reply": document_symbol_result() }]
        })))
        .await;
        let symbols = session
            .client
            .document_symbols(&a_file(&session))
            .await
            .expect("an answer");

        assert_eq!(symbols.len(), 3);
        assert_eq!(symbols[0].name, "CodeBasics.Inspector");
        assert_eq!(symbols[0].kind, SymbolKind::Namespace);
        assert_eq!(symbols[1].name, "Collections");
        assert_eq!(
            symbols[1].container,
            vec!["CodeBasics.Inspector".to_string()]
        );
        assert_eq!(symbols[2].kind, SymbolKind::Function);
        assert_eq!(
            symbols[2].selection_range.start.character, 23,
            "the identifier range is what a references request is aimed at"
        );
    });
}

#[tokio::test]
async fn an_unreadable_answer_names_the_request_that_was_actually_asked() {
    // One decoder serves four goto requests and hard-codes `definition` in its
    // error. If that leaks out, the UI tells the user their *definition* lookup
    // failed when they asked for the type's declaration.
    bounded!(async {
        let session = start(fake(json!({
            "capabilities": roslyn_capabilities(),
            "steps": [
                { "on": "textDocument/typeDefinition", "reply": { "surprise": true } },
                { "on": "textDocument/documentSymbol", "reply": 7 }
            ]
        })))
        .await;
        let file = a_file(&session);

        let failure = session
            .client
            .type_definition(&file, anywhere())
            .await
            .expect_err("an unreadable shape is never an empty list");
        match &failure {
            RequestError::Failed {
                method,
                failure: RequestFailure::Malformed { method: named, .. },
            } => {
                assert_eq!(*method, "textDocument/typeDefinition");
                assert_eq!(named, "textDocument/typeDefinition");
            }
            other => panic!("expected a malformed answer, got {other:?}"),
        }
        assert!(
            !failure.to_string().contains("textDocument/definition"),
            "the message must not name a request nobody made: {failure}"
        );

        let symbols = session.client.document_symbols(&file).await;
        assert!(
            matches!(
                symbols,
                Err(RequestError::Failed {
                    method: "textDocument/documentSymbol",
                    failure: RequestFailure::Malformed { .. }
                })
            ),
            "got {symbols:?}"
        );
    });
}

#[tokio::test]
async fn a_request_that_is_never_answered_times_out_and_names_the_method() {
    bounded!(async {
        let Fake { dir, mut spec } = fake(json!({
            "capabilities": roslyn_capabilities(),
            "steps": [{ "on": "textDocument/references", "misbehave": "never" }]
        }));
        // Both, not just `request`: this *is* the first request, and the first
        // request deliberately gets the longer of the two budgets.
        spec.timeouts.first_request = Duration::from_millis(400);
        spec.timeouts.request = Duration::from_millis(400);
        let client = Client::start(spec, dir.path()).await.expect("started");

        let failure = client
            .references(&dir.path().join("Collections.cs"), anywhere(), true)
            .await
            .expect_err("a silent server is not an empty answer");
        assert_eq!(
            failure,
            RequestError::Failed {
                method: "textDocument/references",
                failure: RequestFailure::TimedOut {
                    after: Duration::from_millis(400)
                },
            }
        );
    });
}

#[tokio::test]
async fn a_first_request_that_timed_out_does_not_spend_the_start_up_budget() {
    // The rationale on `ask` says the first-request budget is demoted only on a
    // real answer, because a request that timed out has not established that the
    // server finished its start-up cost. Nothing tested it: the existing timeout
    // test sets `first_request` and `request` to the *same* value, so the field
    // under test can only take one value there, and no test issued a second
    // request after a failed first. Move the store above the `is_ok()` check and
    // this is the only test that notices.
    bounded!(async {
        let Fake { dir, mut spec } = fake(json!({
            "capabilities": roslyn_capabilities(),
            "steps": [
                { "on": "textDocument/references", "misbehave": "never" },
                { "on": "textDocument/definition", "delayMs": 400, "reply": definition_result() }
            ]
        }));
        spec.timeouts.first_request = Duration::from_millis(1200);
        spec.timeouts.request = Duration::from_millis(100);
        let client = Client::start(spec, dir.path()).await.expect("started");
        let file = dir.path().join("Collections.cs");

        let timed_out = client.references(&file, anywhere(), true).await;
        assert_eq!(
            timed_out,
            Err(RequestError::Failed {
                method: "textDocument/references",
                failure: RequestFailure::TimedOut {
                    after: Duration::from_millis(1200)
                },
            }),
            "the first request gets the start-up budget"
        );

        // 400 ms of work under a 100 ms steady-state budget: this only succeeds
        // because the start-up budget is still in force.
        let after = client.definition(&file, anywhere()).await;
        assert!(
            after.is_ok(),
            "a failed first request has not proved the server finished starting, \
             so the next one must still get the long budget: {after:?}"
        );
    });
}

#[tokio::test]
async fn a_request_whose_server_dies_names_the_method_and_reports_the_death() {
    bounded!(async {
        let session = start(fake(json!({
            "capabilities": roslyn_capabilities(),
            "steps": [{ "on": "textDocument/definition", "misbehave": "exitBeforeReply" }]
        })))
        .await;
        let failure = session
            .client
            .definition(&a_file(&session), anywhere())
            .await
            .expect_err("a dead server has not answered anything");

        match failure {
            RequestError::Failed {
                method,
                failure: RequestFailure::ServerDied(death),
            } => {
                assert_eq!(method, "textDocument/definition");
                assert_eq!(death.reason, DeathReason::Exited);
            }
            other => panic!("expected a death, got {other:?}"),
        }
        assert!(session.client.death().is_some());
    });
}

#[tokio::test]
async fn a_server_error_is_an_error_and_not_an_empty_list() {
    // No step at all for this method, so the fake answers `-32601`. The
    // governing rule: a failure arriving as `Ok([])` becomes "0 usages" for a
    // method with forty.
    bounded!(async {
        let session = start(fake(json!({ "capabilities": roslyn_capabilities() }))).await;
        let failure = session
            .client
            .references(&a_file(&session), anywhere(), true)
            .await
            .expect_err("an error is not an empty answer");

        match failure {
            RequestError::Failed {
                method,
                failure: RequestFailure::ServerError { code, .. },
            } => {
                assert_eq!(method, "textDocument/references");
                assert_eq!(code, -32601);
            }
            other => panic!("expected a server error, got {other:?}"),
        }
    });
}

// ---------------------------------------------------------------------------
// Lifetime
// ---------------------------------------------------------------------------

#[tokio::test]
async fn shutdown_stops_the_server_process() {
    bounded!(async {
        let session = start(fake(json!({ "capabilities": roslyn_capabilities() }))).await;
        let pid = session.client.pid().expect("a spawned process has a pid");
        assert!(pid_alive(pid), "the fake should be running");

        session.client.shutdown().await;
        until(|| !pid_alive(pid)).await;
        assert!(session.client.death().is_some());
    });
}

#[tokio::test]
async fn a_path_outside_any_root_is_still_a_usable_uri_but_a_relative_one_is_not() {
    // `to_file_uri` abstains on a relative path, and the client must surface
    // that as its own failure rather than sending a URI it made up.
    bounded!(async {
        let session = start(fake(json!({
            "capabilities": roslyn_capabilities(),
            "steps": [{ "on": "textDocument/definition", "reply": definition_result() }]
        })))
        .await;

        // The half the name promised and the body never tested. Every other path
        // in this file is built from `session.client.root().join(..)`, so a
        // `uri_for` that rejected anything outside the open root would pass the
        // whole suite — and a location outside the root has to survive as a
        // result, or the count is wrong.
        let outside = std::env::temp_dir().join("Elsewhere.cs");
        assert!(
            session.client.did_open(&outside, "class A {}", 1).is_ok(),
            "an absolute path outside the root is a perfectly good file: URI"
        );
        assert!(
            session
                .client
                .definition(&outside, anywhere())
                .await
                .is_ok(),
            "and it can be asked about"
        );

        let relative = Path::new("Collections.cs");
        assert_eq!(
            session.client.did_open(relative, "class A {}", 1),
            Err(RequestError::UnusableUri {
                path: relative.to_path_buf()
            })
        );
        assert_eq!(
            session.client.definition(relative, anywhere()).await,
            Err(RequestError::UnusableUri {
                path: relative.to_path_buf()
            })
        );
    });
}

// ---------------------------------------------------------------------------
// Is a process still there?
// ---------------------------------------------------------------------------

/// Whether `pid` names a live process. Shelling out rather than taking a
/// dependency: this is a test helper, and the crate takes no new dependencies
/// for one.
fn pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("tasklist")
            .args(["/NH", "/FI", &format!("PID eq {pid}")])
            .output();
        match output {
            Ok(out) => String::from_utf8_lossy(&out.stdout).contains(&pid.to_string()),
            Err(_) => false,
        }
    }

    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}
