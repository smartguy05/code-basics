//! The transport against a live process, one test per way a server misbehaves.
//!
//! Every one of these drives `cb-fake-lsp` — a scripted stand-in, not a real
//! language server. Three of the four servers this feature targets are not
//! installed on the development machine, and none of them can be made to die
//! mid-request or emit a broken frame on demand, so the fake is not a
//! convenience here: it is the only way these paths are covered at all.
//!
//! **Every test is wrapped in a timeout.** A transport test that can hang is
//! worse than no test: it reads as a broken suite rather than a broken
//! transport, and this repository has already lost real time to exactly that.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use cb_core::lsp::framing::{encode, Decoder};
use cb_core::lsp::jsonrpc::{classify, Incoming, RequestId};
use cb_core::lsp::transport::{
    DeathReason, Launch, Notification, RequestFailure, SignalFilter, Transport, STDERR_TAIL_LINES,
};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const FAKE: &str = env!("CARGO_BIN_EXE_cb-fake-lsp");

/// The outer bound on any one test. Generous — a cold Windows process spawn is
/// not fast — but finite, which is the whole point.
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Long enough that a healthy fake always beats it, short enough that a test
/// asserting a timeout does not dominate the suite.
const DEADLINE: Duration = Duration::from_secs(10);

/// The fake's own self-destruct, so a test killed halfway through cannot leave
/// a stray process on the machine.
const FAKE_SELF_TIMEOUT_MS: &str = "25000";

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// A running fake and the temp directory its script lives in. Dropping this
/// drops the [`Transport`], whose own `Drop` kills the process tree.
struct Fake {
    transport: Transport,
    _dir: TempDir,
}

fn start(script: Value) -> Fake {
    start_watching(script, None)
}

/// [`start`] with a one-shot notification the transport must not lose.
fn start_watching(script: Value, watch_for: Option<SignalFilter>) -> Fake {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("script.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&script).expect("a script")).expect("write");

    let mut env = BTreeMap::new();
    env.insert("CB_FAKE_LSP_SCRIPT".to_string(), path.display().to_string());
    env.insert(
        "CB_FAKE_LSP_TIMEOUT_MS".to_string(),
        FAKE_SELF_TIMEOUT_MS.to_string(),
    );

    let transport = Transport::spawn(
        &Launch {
            program: PathBuf::from(FAKE),
            args: Vec::new(),
            cwd: dir.path().to_path_buf(),
            env,
        },
        watch_for,
    )
    .expect("the fake must start");

    Fake {
        transport,
        _dir: dir,
    }
}

/// Wait for a condition rather than for a duration.
///
/// The bounded outer timeout on every test is what turns a condition that never
/// becomes true into a failure instead of a hang; this loop never decides
/// anything by elapsed time.
async fn until(mut condition: impl FnMut() -> bool) {
    while !condition() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Run a test body under the suite's timeout.
macro_rules! bounded {
    ($body:expr) => {
        tokio::time::timeout(TEST_TIMEOUT, $body)
            .await
            .expect("the test timed out — the transport hung, which is the bug")
    };
}

fn died(result: &Result<Value, RequestFailure>) -> &cb_core::lsp::transport::Death {
    match result {
        Err(RequestFailure::ServerDied(death)) => death,
        other => panic!("expected the server to be reported dead, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The ordinary path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_request_gets_its_answer() {
    bounded!(async {
        let fake = start(json!({
            "capabilities": { "referencesProvider": true, "textDocumentSync": 2 }
        }));
        let result = fake
            .transport
            .request("initialize", json!({}), DEADLINE)
            .await
            .expect("a healthy server answers");

        assert_eq!(result["capabilities"]["referencesProvider"], json!(true));
        assert_eq!(result["capabilities"]["textDocumentSync"], json!(2));
        assert_eq!(fake.transport.death(), None, "still alive");
    });
}

#[tokio::test]
async fn an_initialize_result_with_no_capabilities_is_not_invented() {
    // Roslyn omits `positionEncoding` entirely, and absent has a meaning. This
    // layer must hand the result up exactly as it arrived — deciding what an
    // absent key means is `client.rs`'s job and nobody else's.
    bounded!(async {
        let fake = start(json!({ "steps": [] }));
        let result = fake
            .transport
            .request("initialize", json!({}), DEADLINE)
            .await
            .expect("an answer");
        assert_eq!(result, json!({}));
    });
}

#[tokio::test]
async fn concurrent_requests_each_resolve_to_their_own_answer() {
    // A correlation bug passes a single-request test. Three in flight with
    // deliberately inverted delays — the last issued answers first — is what
    // catches a waiter being woken by somebody else's response.
    bounded!(async {
        let fake = start(json!({
            "steps": [
                { "on": "q/slow",   "delayMs": 400, "reply": "slow" },
                { "on": "q/medium", "delayMs": 200, "reply": "medium" },
                { "on": "q/fast",   "delayMs": 10,  "reply": "fast" }
            ]
        }));
        let t = &fake.transport;
        let (slow, medium, fast) = tokio::join!(
            t.request("q/slow", json!({}), DEADLINE),
            t.request("q/medium", json!({}), DEADLINE),
            t.request("q/fast", json!({}), DEADLINE),
        );

        assert_eq!(slow, Ok(json!("slow")));
        assert_eq!(medium, Ok(json!("medium")));
        assert_eq!(fast, Ok(json!("fast")));
    });
}

#[tokio::test]
async fn a_notification_reaches_subscribers() {
    bounded!(async {
        let fake = start(json!({
            "emitOnInitialized": [
                { "method": "window/logMessage", "params": { "type": 3, "message": "loaded" } }
            ]
        }));
        let mut notifications = fake.transport.notifications();
        fake.transport
            .notify("initialized", json!({}))
            .expect("queued");

        let notification = notifications.recv().await.expect("a notification");
        assert_eq!(notification.method, "window/logMessage");
        assert_eq!(notification.params.unwrap()["message"], json!("loaded"));
    });
}

#[tokio::test]
async fn a_one_shot_notification_survives_a_subscriber_that_lost_its_place() {
    // The Phase 5 bug, end to end against a live process. Roslyn sends thousands
    // of progress notifications while loading and then
    // `workspace/projectInitializationComplete` exactly **once**. A `broadcast`
    // subscriber that has fallen behind by then loses it and there is no next
    // one, so the readiness watch waited out its 90 s ceiling, the server was
    // promoted to ready-with-caveat, and every count taken from it was zero
    // because the projects genuinely had not loaded.
    //
    // Both halves are asserted here: that the live subscriber really does lose
    // it (otherwise this test would vouch for the fix without exercising it),
    // and that the sticky record has it anyway.
    bounded!(async {
        const SIGNAL: &str = "workspace/projectInitializationComplete";
        let filter: SignalFilter = std::sync::Arc::new(|n: &Notification| {
            n.method == "workspace/projectInitializationComplete"
        });
        let fake = start_watching(
            json!({
                "steps": [{
                    "on": "initialized",
                    "misbehave": "notificationFlood",
                    "lines": 3000,
                    "then": { "method": SIGNAL, "params": { "loaded": true } }
                }]
            }),
            Some(filter),
        );

        // Subscribed and then deliberately never read, which is what a task
        // busy elsewhere looks like from the channel's point of view.
        let mut stream = fake.transport.notifications();
        let signal = fake.transport.signal();
        fake.transport
            .notify("initialized", json!({}))
            .expect("queued");

        // Sticky state, not a message: this becomes true even though the
        // receiver above was created before the flood and has read nothing.
        until(|| signal.borrow().is_some()).await;
        let recorded = signal.borrow().clone().expect("the signal was recorded");
        assert_eq!(recorded.method, SIGNAL);
        assert_eq!(recorded.params.unwrap()["loaded"], json!(true));

        // And the proof that the sticky record was needed. `Lagged` is the
        // channel saying the subscriber's oldest messages are gone.
        match stream.try_recv() {
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(dropped)) => {
                assert!(dropped > 0, "lag means messages were dropped");
            }
            other => panic!("expected the live subscriber to have lagged, got {other:?}"),
        }
    });
}

#[tokio::test]
async fn nothing_is_recorded_for_a_transport_that_is_watching_for_nothing() {
    // A `None` filter means "nothing about this server is one-shot", which must
    // not be spelled the same way as "the signal has arrived". A server that is
    // ready as soon as it answers `initialize` is started this way.
    bounded!(async {
        let fake = start(json!({
            "emitOnInitialized": [{ "method": "workspace/projectInitializationComplete" }]
        }));
        let mut notifications = fake.transport.notifications();
        fake.transport
            .notify("initialized", json!({}))
            .expect("queued");

        assert_eq!(
            notifications.recv().await.expect("a notification").method,
            "workspace/projectInitializationComplete"
        );
        assert_eq!(
            *fake.transport.signal().borrow(),
            None,
            "a transport asked to watch for nothing records nothing"
        );
    });
}

#[tokio::test]
async fn the_uri_we_send_comes_back_byte_for_byte() {
    // `echoUri`. URI spelling is a compatibility detail the transport must not
    // touch: Roslyn wants a plain colon, rust-analyzer emits `%3A`, and a
    // transport that "helpfully" normalised either would break one of them.
    bounded!(async {
        let fake = start(json!({
            "steps": [{ "on": "textDocument/references", "misbehave": "echoUri" }]
        }));
        let uri = "file:///C:/Users/Anthony/Prog.cs";
        let result = fake
            .transport
            .request(
                "textDocument/references",
                json!({ "textDocument": { "uri": uri } }),
                DEADLINE,
            )
            .await
            .expect("an answer");

        assert_eq!(result[0]["uri"], json!(uri));
    });
}

#[tokio::test]
async fn a_server_error_is_an_error_and_not_an_empty_result() {
    // The governing rule of this whole feature: a failure that arrives as
    // `Ok([])` becomes "0 usages" for a method with forty.
    bounded!(async {
        let fake = start(json!({ "steps": [] }));
        let result = fake
            .transport
            .request("textDocument/references", json!({}), DEADLINE)
            .await;

        match result {
            Err(RequestFailure::ServerError { code, .. }) => assert_eq!(code, -32601),
            other => panic!("expected a server error, got {other:?}"),
        }
    });
}

// ---------------------------------------------------------------------------
// Server→client requests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_blocking_server_request_completes_because_we_answer_it() {
    // `serverRequestThenReply` reproduces the real Roslyn start-up stall: the
    // server asks `workspace/configuration` and does not answer until it is
    // answered. This test passing means the reply went out; the counterfactual
    // is the test below.
    bounded!(async {
        let fake = start(json!({
            "steps": [{
                "on": "textDocument/references",
                "misbehave": "serverRequestThenReply",
                "reply": "after the client answered",
                "request": {
                    "method": "workspace/configuration",
                    "params": { "items": [
                        { "section": "csharp|a" },
                        { "section": "csharp|b" },
                        { "section": "csharp|c" }
                    ] }
                }
            }]
        }));
        let result = fake
            .transport
            .request("textDocument/references", json!({}), DEADLINE)
            .await;

        assert_eq!(result, Ok(json!("after the client answered")));
    });
}

#[tokio::test]
async fn an_unanswered_server_request_hangs_the_server_forever() {
    // The counterfactual, and the reason the answering table is not optional.
    // A deliberately deaf client — raw frames, no transport — reads the
    // server's question and says nothing. Proving an absence needs a bounded
    // wait rather than a condition; the wait is what the assertion is about.
    bounded!(async {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("script.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "steps": [{
                    "on": "textDocument/references",
                    "misbehave": "serverRequestThenReply",
                    "reply": "never reached"
                }]
            }))
            .unwrap(),
        )
        .unwrap();

        let mut child = tokio::process::Command::new(FAKE)
            .env("CB_FAKE_LSP_SCRIPT", &path)
            .env("CB_FAKE_LSP_TIMEOUT_MS", FAKE_SELF_TIMEOUT_MS)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the fake must start");
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();

        let body = br#"{"jsonrpc":"2.0","id":1,"method":"textDocument/references","params":{}}"#;
        stdin.write_all(&encode(body)).await.unwrap();
        stdin.flush().await.unwrap();

        let mut decoder = Decoder::new();
        let mut buf = [0u8; 8192];
        let mut asked = false;
        let mut answered = false;
        // Two seconds of deliberate deafness. The server's question arrives in
        // milliseconds; the answer to id 1 never arrives at all.
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let n = match stdout.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => n,
                };
                for frame in decoder.push(&buf[..n]).expect("framed") {
                    match classify(&frame).expect("a message") {
                        Incoming::Request { method, .. } => {
                            assert_eq!(method, "workspace/configuration");
                            asked = true;
                        }
                        Incoming::Response { id, .. } => {
                            if id == RequestId::Num(1) {
                                answered = true;
                                return;
                            }
                        }
                        Incoming::Notification { .. } => {}
                    }
                }
            }
        })
        .await;

        let _ = child.kill().await;
        assert!(asked, "the server should have asked the client something");
        assert!(
            !answered,
            "the server answered without its question being answered — \
             then the answering table proves nothing"
        );
    });
}

#[tokio::test]
async fn the_configuration_we_answer_with_is_the_one_that_reaches_the_server() {
    // `answer_for`'s table is unit tested, but until now nothing checked that
    // its *value* was what went on the wire: the fake matched a response on its
    // id and threw the payload away, so a client replying a flat `null` to a
    // three-item `workspace/configuration` would have passed every test in the
    // suite. That reply is a real Roslyn start-up hang wearing the clothes of a
    // slow project load. `echoAnswer` sends the client's own answer back as the
    // result, which is the only way a test can see it.
    bounded!(async {
        let fake = start(json!({
            "steps": [{
                "on": "q/config",
                "misbehave": "serverRequestThenReply",
                "echoAnswer": true,
                "request": {
                    "method": "workspace/configuration",
                    "params": { "items": [
                        { "section": "csharp|a" },
                        { "section": "csharp|b" },
                        { "section": "csharp|c" }
                    ] }
                }
            }]
        }));
        let answer = fake
            .transport
            .request("q/config", json!({}), DEADLINE)
            .await
            .expect("the server answers once we have answered it");

        assert_eq!(
            answer["result"],
            json!([null, null, null]),
            "one entry per requested section, positionally — a reply of the \
             wrong length is an answer about different sections"
        );
        assert_eq!(answer["error"], Value::Null, "a result, not an error reply");
    });
}

#[tokio::test]
async fn a_server_request_we_do_not_implement_is_refused_on_the_wire_and_not_ignored() {
    // The other half of the answering table: `METHOD_NOT_FOUND` has to arrive as
    // an actual error *reply*. Silence would be the hang; the error branch was
    // built and never observed by anything.
    bounded!(async {
        let fake = start(json!({
            "steps": [{
                "on": "q/unknown",
                "misbehave": "serverRequestThenReply",
                "echoAnswer": true,
                "request": { "method": "workspace/somethingWeDoNotDo" }
            }]
        }));
        let answer = fake
            .transport
            .request("q/unknown", json!({}), DEADLINE)
            .await
            .expect("a refusal is still an answer, so the server unblocks");

        assert_eq!(answer["error"]["code"], json!(-32601));
        assert!(
            answer["error"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("workspace/somethingWeDoNotDo")),
            "the refusal must name the method: {answer}"
        );
    });
}

// ---------------------------------------------------------------------------
// Misbehaviour: the server stops answering
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_server_that_never_answers_times_out_rather_than_hanging() {
    bounded!(async {
        let fake = start(json!({
            "steps": [
                { "on": "q/silent", "misbehave": "never" },
                { "on": "*", "reply": "still here" }
            ]
        }));
        let result = fake
            .transport
            .request("q/silent", json!({}), Duration::from_millis(300))
            .await;

        assert_eq!(
            result,
            Err(RequestFailure::TimedOut {
                after: Duration::from_millis(300)
            })
        );
        assert_eq!(
            fake.transport.death(),
            None,
            "a timeout is not a death — the server is fine, this request is not"
        );

        // And the session survives it: a timed-out request must not poison the
        // transport, or one slow query would take the whole feature down.
        let after = fake.transport.request("q/other", json!({}), DEADLINE).await;
        assert_eq!(after, Ok(json!("still here")));
    });
}

#[tokio::test]
async fn a_server_that_dies_mid_request_fails_the_waiter() {
    // `exitBeforeReply`. Without the reaper this is a caller waiting forever
    // for a reply from a process that no longer exists.
    bounded!(async {
        let fake = start(json!({
            "steps": [{ "on": "q/fatal", "misbehave": "exitBeforeReply" }]
        }));
        let result = fake.transport.request("q/fatal", json!({}), DEADLINE).await;

        let death = died(&result);
        assert_eq!(death.reason, DeathReason::Exited);
        assert_eq!(death.code, Some(0));

        // Anything asked afterwards is refused immediately with the same death,
        // rather than waiting out a deadline against a corpse.
        let after = fake.transport.request("q/again", json!({}), DEADLINE).await;
        assert!(matches!(after, Err(RequestFailure::ServerDied(_))));
    });
}

#[tokio::test]
async fn a_truncated_frame_stalls_rather_than_dying_and_is_reported_as_a_timeout() {
    // `truncatedContentLength`: a header promising 4096 bytes followed by 26
    // and then silence. Nothing is malformed — the decoder is correctly waiting
    // for the rest — so this is a timeout, not a death. Collapsing the two
    // would tell the user their server crashed when it did not.
    bounded!(async {
        let fake = start(json!({
            "steps": [{ "on": "q/short", "misbehave": "truncatedContentLength" }]
        }));
        let result = fake
            .transport
            .request("q/short", json!({}), Duration::from_millis(500))
            .await;

        assert_eq!(
            result,
            Err(RequestFailure::TimedOut {
                after: Duration::from_millis(500)
            })
        );
        assert_eq!(fake.transport.death(), None);
    });
}

// ---------------------------------------------------------------------------
// Misbehaviour: the stream stops being a stream
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_garbage_frame_kills_the_session_rather_than_being_resynchronised() {
    // Guessing where the next frame starts would hand one request's answer to
    // another request's waiter. A wrong answer is worse than a dead server.
    bounded!(async {
        let fake = start(json!({
            "steps": [{ "on": "q/garbage", "misbehave": "garbageFrame" }]
        }));
        let result = fake
            .transport
            .request("q/garbage", json!({}), DEADLINE)
            .await;

        let death = died(&result);
        assert!(
            matches!(death.reason, DeathReason::Framing(_)),
            "expected a framing death, got {:?}",
            death.reason
        );
    });
}

#[tokio::test]
async fn a_frame_larger_than_the_cap_kills_the_session() {
    // `hugeReply` declares a body past `framing::MAX_FRAME_BYTES`. The cap is
    // not a tuning knob: it bounds what a server emitting garbage can make this
    // process allocate.
    bounded!(async {
        let fake = start(json!({
            "steps": [{ "on": "q/huge", "misbehave": "hugeReply" }]
        }));
        let result = fake.transport.request("q/huge", json!({}), DEADLINE).await;

        let death = died(&result);
        match &death.reason {
            DeathReason::Framing(detail) => assert!(
                detail.contains("exceeds"),
                "the reason should name the limit: {detail}"
            ),
            other => panic!("expected a framing death, got {other:?}"),
        }
    });
}

// ---------------------------------------------------------------------------
// Misbehaviour: the server answers, but wrongly
// ---------------------------------------------------------------------------

#[tokio::test]
async fn replies_arriving_out_of_order_still_reach_their_own_waiters() {
    // The fake holds the first reply until the *next* one is sent, so the
    // answers cross on the wire. The delay on the second request is scripted
    // into the server, not slept in the test: it makes the crossing happen
    // rather than merely making it likely.
    bounded!(async {
        let fake = start(json!({
            "steps": [
                { "on": "q/held", "misbehave": "replyOutOfOrder", "reply": "held" },
                { "on": "q/later", "delayMs": 250, "reply": "later" }
            ]
        }));
        let t = &fake.transport;
        let (held, later) = tokio::join!(
            t.request("q/held", json!({}), DEADLINE),
            t.request("q/later", json!({}), DEADLINE),
        );

        assert_eq!(held, Ok(json!("held")));
        assert_eq!(later, Ok(json!("later")));
    });
}

#[tokio::test]
async fn a_held_reply_that_no_later_answer_releases_still_reaches_its_waiter() {
    // The mirror of the test above, with the timing reversed: the *other*
    // request answers first, and the held frame is pushed 300 ms later with
    // nothing after it. The fake used to flush `held` only from inside an
    // answer, so a frame pushed after the last answer was written was lost
    // outright — and the transport wore the resulting timeout.
    bounded!(async {
        let fake = start(json!({
            "steps": [
                { "on": "q/held", "delayMs": 300, "misbehave": "replyOutOfOrder", "reply": "held" },
                { "on": "q/first", "reply": "first" }
            ]
        }));
        let t = &fake.transport;
        let (held, first) = tokio::join!(
            t.request("q/held", json!({}), DEADLINE),
            t.request("q/first", json!({}), DEADLINE),
        );

        assert_eq!(first, Ok(json!("first")));
        assert_eq!(held, Ok(json!("held")));
    });
}

#[tokio::test]
async fn an_unreadable_answer_fails_its_waiter_rather_than_timing_out() {
    // A framed, well-formed JSON object carrying the waiter's id and an `error`
    // whose `code` is a string — a shape non-Roslyn servers do emit and
    // `classify` cannot read. Dropping it silently made the caller wait out its
    // whole deadline: "the server did not answer within 10 s" about a server
    // that answered in milliseconds and said no.
    bounded!(async {
        let fake = start(json!({
            "steps": [{ "on": "q/unreadable", "misbehave": "malformedReply" }]
        }));
        let result = fake
            .transport
            .request("q/unreadable", json!({}), DEADLINE)
            .await;

        match result {
            Err(RequestFailure::Malformed { method, detail }) => {
                assert_eq!(method, "q/unreadable");
                assert!(!detail.is_empty(), "the reason has to say something");
            }
            other => panic!("expected a malformed answer, got {other:?}"),
        }
        assert_eq!(
            fake.transport.death(),
            None,
            "one unreadable message costs one answer, not the session"
        );
    });
}

#[tokio::test]
async fn a_duplicate_reply_is_dropped_and_the_session_survives() {
    bounded!(async {
        let fake = start(json!({
            "steps": [
                { "on": "q/twice", "misbehave": "duplicateReply", "reply": "once" },
                { "on": "*", "reply": "after" }
            ]
        }));
        let first = fake.transport.request("q/twice", json!({}), DEADLINE).await;
        assert_eq!(first, Ok(json!("once")));

        // The second copy resolved nothing and must not have torn anything
        // down: a stray response is ordinary traffic, not a protocol failure.
        let second = fake.transport.request("q/next", json!({}), DEADLINE).await;
        assert_eq!(second, Ok(json!("after")));
        assert_eq!(fake.transport.death(), None);
    });
}

#[tokio::test]
async fn a_reply_for_an_id_nobody_asked_about_is_dropped_not_misdelivered() {
    // The fake sends id 987654321 and *then* the genuine answer. Handing the
    // stray one to the waiting request would be the correlation bug this whole
    // module is arranged to prevent.
    bounded!(async {
        let fake = start(json!({
            "steps": [{
                "on": "q/stray",
                "misbehave": "unknownIdReply",
                "reply": "the real answer",
                "unknownId": 987654321
            }]
        }));
        let result = fake.transport.request("q/stray", json!({}), DEADLINE).await;

        assert_eq!(result, Ok(json!("the real answer")));
        assert_eq!(fake.transport.death(), None);
    });
}

// ---------------------------------------------------------------------------
// Diagnostics and lifetime
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_flood_of_stderr_is_bounded_and_does_not_block_the_answer() {
    bounded!(async {
        let fake = start(json!({
            "steps": [{
                "on": "q/noisy",
                "misbehave": "stderrFlood",
                "lines": 3000,
                "reply": "answered anyway"
            }]
        }));
        let result = fake.transport.request("q/noisy", json!({}), DEADLINE).await;
        assert_eq!(result, Ok(json!("answered anyway")));

        // Wait for the tail to fill, then assert it never exceeds the bound —
        // 3000 lines held in memory per server is exactly what the cap exists
        // to prevent.
        let t = &fake.transport;
        until(|| t.stderr_tail().len() >= STDERR_TAIL_LINES).await;
        let tail = t.stderr_tail();
        assert_eq!(tail.len(), STDERR_TAIL_LINES);
        assert!(
            tail.last().unwrap().contains("flood line"),
            "the tail should be the most recent lines: {:?}",
            tail.last()
        );
    });
}

#[tokio::test]
async fn a_superseded_request_is_cancelled_rather_than_left_waiting() {
    bounded!(async {
        let fake = start(json!({
            "steps": [{ "on": "q/superseded", "delayMs": 3000, "reply": "too late" }]
        }));
        let t = &fake.transport;
        let id = t.allocate_id();

        let (answer, ()) = tokio::join!(
            t.request_with_id(id.clone(), "q/superseded", json!({}), DEADLINE),
            async {
                // Cancelling before registration would prove nothing, so wait
                // for the condition rather than for a duration.
                until(|| t.is_pending(&id)).await;
                t.cancel(&id);
            }
        );

        assert_eq!(answer, Err(RequestFailure::Cancelled));
        assert_eq!(fake.transport.death(), None);
    });
}

#[tokio::test]
async fn shutdown_stops_the_process() {
    bounded!(async {
        let fake = start(json!({ "steps": [] }));
        let pid = fake.transport.pid().expect("a spawned process has a pid");
        assert!(pid_alive(pid), "the fake should be running");

        fake.transport.shutdown().await;
        until(|| !pid_alive(pid)).await;
        assert!(fake.transport.death().is_some());
    });
}

#[tokio::test]
async fn dropping_the_transport_kills_the_whole_tree() {
    // `spawnChildAndLeak` starts a grandchild that deliberately outlives the
    // fake. Killing only the process we spawned is what orphans Roslyn's
    // `BuildHost-netcore` / `BuildHost-net472` children for the life of the
    // user's session, so the kill has to walk the tree.
    //
    // Needs `sh` on PATH (Git Bash on Windows) — the same assumption the
    // `process::` tests already make.
    //
    // This is a real assertion on Unix as well as Windows: `Transport::spawn`
    // calls `process::configure_process_group`, so the child is its own group
    // leader and `process::kill_tree`'s negative-pid signal reaches the
    // grandchild. It would not have done before those two were exported — the
    // transport's own copy of `kill_tree` signalled a single pid.
    bounded!(async {
        let dir = tempfile::tempdir().expect("a temp dir");
        let pid_file = dir.path().join("leaked-pid.txt");
        let fake = start(json!({
            "steps": [{
                "on": "q/leak",
                "misbehave": "spawnChildAndLeak",
                "pidFile": pid_file,
                "reply": "spawned"
            }]
        }));

        let result = fake.transport.request("q/leak", json!({}), DEADLINE).await;
        assert_eq!(result, Ok(json!("spawned")));

        // Bounded on its own, and short. Without `sh` the pid file never
        // appears, and letting the outer `bounded!` catch that would report
        // "the transport hung, which is the bug" thirty seconds later — a
        // misdiagnosis this repository has already paid for once.
        tokio::time::timeout(Duration::from_secs(5), until(|| pid_file.exists()))
            .await
            .expect("no pid file — is `sh` on PATH? run cargo test from Git Bash");
        let grandchild: u32 = std::fs::read_to_string(&pid_file)
            .expect("the pid file")
            .trim()
            .parse()
            .expect("a pid");
        assert!(pid_alive(grandchild), "the grandchild should be running");

        let server = fake.transport.pid().expect("a pid");
        drop(fake);

        until(|| !pid_alive(server)).await;
        until(|| !pid_alive(grandchild)).await;
    });
}

// ---------------------------------------------------------------------------
// Is a process still there?
// ---------------------------------------------------------------------------

/// Whether `pid` names a live process.
///
/// Shelling out rather than taking a dependency: this is a test helper, and the
/// crate takes no new dependencies for one.
fn pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("tasklist")
            .args(["/NH", "/FI", &format!("PID eq {pid}")])
            .output();
        match output {
            // `tasklist` with no match still exits 0 and prints an INFO line,
            // so the pid itself is what has to be looked for.
            Ok(out) => String::from_utf8_lossy(&out.stdout).contains(&pid.to_string()),
            Err(_) => false,
        }
    }

    #[cfg(unix)]
    {
        // Through `sh`, not `Command::new("kill")`. `kill` is a POSIX **shell
        // builtin** and a standalone `/bin/kill` only exists where procps is
        // installed — it is not in a Debian slim image, and neither is `ps`. So
        // the direct spawn failed to start at all and `unwrap_or(false)` turned
        // that into "the process is dead".
        //
        // Which made this helper answer `false` for **every** pid on Unix. Both
        // liveness preconditions in this file failed the first time they were
        // ever run on Linux, and worse, the two `until(|| !pid_alive(..))` waits
        // that are the actual subject of `dropping_the_transport_kills_the_whole_tree`
        // would have returned instantly and asserted nothing. A test that cannot
        // fail is not evidence, and this one was shipped as evidence.
        //
        // `sh` is already a hard requirement of this file (the fake's leaked
        // grandchild is `sh`), so it costs nothing.
        std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("kill -0 {pid}"))
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
