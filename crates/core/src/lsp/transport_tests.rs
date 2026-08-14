//! Unit tests for the parts of the transport that need no process.
//!
//! Everything that *does* need one lives in `crates/core/tests/lsp_transport.rs`
//! and drives the scripted `cb-fake-lsp` binary. The split is deliberate: the
//! server-request answering table and the restart policy are the two decisions
//! in this module that are wrong-or-right rather than fast-or-slow, and neither
//! should need a child process to prove.

use super::*;

fn ok(method: &str, params: Value) -> Value {
    answer_for(method, Some(&params)).expect("this method must be answered")
}

// ---------------------------------------------------------------------------
// The server-request answering table
// ---------------------------------------------------------------------------

#[test]
fn workspace_configuration_gets_one_null_per_requested_item() {
    // Verified real behaviour: Roslyn asks for a list of settings and waits.
    // The reply is positional, so a reply of the wrong length is not a smaller
    // answer — it is an answer about the wrong sections.
    let params = json!({
        "items": [
            { "section": "csharp|symbol_search.dotnet_search_reference_assemblies" },
            { "section": "csharp|background_analysis.dotnet_analyzer_diagnostics_scope" },
            { "section": "csharp|code_lens.dotnet_enable_references_code_lens" }
        ]
    });
    assert_eq!(
        ok("workspace/configuration", params),
        json!([null, null, null])
    );
}

#[test]
fn workspace_configuration_with_one_item_is_not_answered_with_a_bare_null() {
    // The reply is an *array* even for a single item. A bare null here is the
    // single-item bug that a three-item test would never catch.
    let params = json!({ "items": [{ "section": "csharp" }] });
    assert_eq!(ok("workspace/configuration", params), json!([null]));
}

#[test]
fn workspace_configuration_with_no_items_is_an_empty_array() {
    let params = json!({ "items": [] });
    assert_eq!(ok("workspace/configuration", params), json!([]));
}

#[test]
fn workspace_configuration_with_unreadable_items_still_answers() {
    // A server that asks for nothing readable still blocks until it is
    // answered, so the reply has to happen. An empty array is the only length
    // we can honestly claim, and it is legal.
    for params in [json!({}), json!({ "items": "not an array" }), Value::Null] {
        assert_eq!(
            answer_for("workspace/configuration", Some(&params)),
            Ok(json!([])),
            "params {params} must still produce a reply"
        );
    }
    assert_eq!(answer_for("workspace/configuration", None), Ok(json!([])));
}

#[test]
fn the_start_up_requests_roslyn_issues_are_answered_with_null() {
    for method in [
        "client/registerCapability",
        "client/unregisterCapability",
        "window/showMessageRequest",
        "window/workDoneProgress/create",
    ] {
        assert_eq!(
            answer_for(method, Some(&json!({ "token": "t" }))),
            Ok(Value::Null),
            "{method} must be answered"
        );
    }
}

#[test]
fn apply_edit_is_refused_rather_than_ignored() {
    // This app never lets a server write to the user's files behind their back.
    // Refusing is not the same as not answering: the server must be told.
    assert_eq!(
        answer_for("workspace/applyEdit", Some(&json!({ "edit": {} }))),
        Ok(json!({ "applied": false }))
    );
}

#[test]
fn an_unknown_server_request_gets_method_not_found_not_silence() {
    let answer = answer_for("workspace/somethingWeDoNotDo", None);
    let error = answer.expect_err("an unimplemented method is an error reply");
    assert_eq!(error.code, METHOD_NOT_FOUND);
    assert!(
        error.message.contains("workspace/somethingWeDoNotDo"),
        "the message should name the method: {}",
        error.message
    );
}

#[test]
fn a_known_method_with_absent_params_still_answers() {
    // The rule this whole table exists for: there is no input for which the
    // right behaviour is to say nothing, because saying nothing hangs the
    // server, and the hang looks exactly like a slow project load. Params are
    // optional on the wire, so every arm has to survive their absence.
    assert_eq!(
        answer_for("window/workDoneProgress/create", None),
        Ok(Value::Null)
    );
    assert_eq!(
        answer_for("workspace/applyEdit", None),
        Ok(json!({ "applied": false }))
    );
    assert!(
        answer_for("", None).is_err(),
        "an empty method is not one of ours"
    );
}

// ---------------------------------------------------------------------------
// The restart policy
// ---------------------------------------------------------------------------

fn a_death() -> Death {
    Death {
        code: Some(1),
        reason: DeathReason::Exited,
        stderr_tail: vec!["could not load project".to_string()],
    }
}

#[test]
fn the_first_death_is_restarted() {
    let mut policy = RestartPolicy::new();
    assert_eq!(
        policy.on_death(Instant::now(), &a_death()),
        RestartDecision::Restart
    );
}

#[test]
fn a_second_death_inside_the_window_fails_permanently_with_the_stderr_tail() {
    let mut policy = RestartPolicy::new();
    let start = Instant::now();
    assert_eq!(policy.on_death(start, &a_death()), RestartDecision::Restart);

    let soon = start + Duration::from_secs(5);
    assert_eq!(
        policy.on_death(soon, &a_death()),
        RestartDecision::Failed {
            stderr_tail: vec!["could not load project".to_string()],
        },
        "an endless respawn against a server that cannot start is worse than saying so"
    );
}

#[test]
fn a_death_after_the_window_is_restarted_again() {
    let mut policy = RestartPolicy::new();
    let start = Instant::now();
    assert_eq!(policy.on_death(start, &a_death()), RestartDecision::Restart);

    let later = start + RESTART_WINDOW + Duration::from_secs(1);
    assert_eq!(
        policy.on_death(later, &a_death()),
        RestartDecision::Restart,
        "a server that ran fine for over a minute has earned another try"
    );
}

#[test]
fn failure_is_permanent_once_it_has_happened() {
    let mut policy = RestartPolicy::new();
    let start = Instant::now();
    policy.on_death(start, &a_death());
    policy.on_death(start + Duration::from_secs(1), &a_death());

    // Long past the window. Still failed: the point is that this server is
    // known not to start, and time does not change that on its own.
    let much_later = start + RESTART_WINDOW * 10;
    assert!(matches!(
        policy.on_death(much_later, &a_death()),
        RestartDecision::Failed { .. }
    ));
    assert!(policy.is_failed());
}

// ---------------------------------------------------------------------------
// Framing of what we send
// ---------------------------------------------------------------------------

#[test]
fn a_notification_carries_no_id_at_all() {
    // `"id": null` is *present*, and a server that sees it waits for a reply
    // that will never come. See `jsonrpc`'s module doc.
    let frame = notification_frame("initialized", json!({}));
    let body = body_of(&frame);
    assert!(
        !body.as_object().unwrap().contains_key("id"),
        "a notification must have no id key: {body}"
    );
    assert_eq!(body["method"], "initialized");
}

#[test]
fn a_request_carries_the_id_it_will_be_answered_under() {
    let frame = request_frame(&RequestId::Num(7), "textDocument/references", json!({}));
    let body = body_of(&frame);
    assert_eq!(body["id"], json!(7));
    assert_eq!(body["jsonrpc"], "2.0");
}

#[test]
fn a_cancellation_names_the_request_it_supersedes() {
    let frame = cancel_frame(&RequestId::Num(12));
    let body = body_of(&frame);
    assert_eq!(body["method"], "$/cancelRequest");
    assert_eq!(body["params"]["id"], json!(12));
    assert!(!body.as_object().unwrap().contains_key("id"));
}

/// Decode one frame the way a server would, so these tests assert on the bytes
/// that actually go out rather than on the value we meant to send.
fn body_of(frame: &[u8]) -> Value {
    let mut decoder = Decoder::new();
    let frames = decoder.push(frame).expect("what we write must be framed");
    assert_eq!(frames.len(), 1, "one message per frame");
    serde_json::from_slice(&frames[0]).expect("what we write must be JSON")
}

// ---------------------------------------------------------------------------
// Deaths that no scripted server can produce
// ---------------------------------------------------------------------------
//
// A read error on a live process, and a write to a stdin that has broken while
// the child is still running, are the two failures a fake cannot be made to
// commit on demand — and both are paths where an omission is invisible: the
// first reports nothing at all, and the second leaks a process tree. The loops
// are generic over their stream for exactly this.

use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};

use tokio::io::ReadBuf;

/// Some bytes, then a genuine I/O error rather than EOF.
struct BreaksAfterOneRead {
    first: Option<Vec<u8>>,
}

impl AsyncRead for BreaksAfterOneRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.first.take() {
            Some(bytes) => {
                buf.put_slice(&bytes);
                Poll::Ready(Ok(()))
            }
            None => Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "the read end broke",
            ))),
        }
    }
}

/// A stdin that is gone even though the process is not.
struct BrokenStdin;

impl AsyncWrite for BrokenStdin {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut TaskContext<'_>,
        _buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "the write end broke",
        )))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct Wiring {
    death: watch::Sender<Option<Death>>,
    pending: Arc<Mutex<Pending>>,
    notifications: broadcast::Sender<Notification>,
    signal: Arc<SignalSink>,
    out: mpsc::UnboundedSender<Vec<u8>>,
    out_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    stderr: StderrTail,
}

fn wiring() -> Wiring {
    wiring_watching(None, 8)
}

/// [`wiring`] with a one-shot filter and the real broadcast capacity, for the
/// tests about a signal a lagging subscriber loses.
fn wiring_watching(watch_for: Option<SignalFilter>, capacity: usize) -> Wiring {
    let (death, _) = watch::channel(None);
    let (notifications, _) = broadcast::channel(capacity);
    let (out, out_rx) = mpsc::unbounded_channel();
    Wiring {
        death,
        pending: Arc::new(Mutex::new(HashMap::new())),
        notifications,
        signal: Arc::new(SignalSink::new(watch_for)),
        out,
        out_rx,
        stderr: Arc::new(Mutex::new(VecDeque::new())),
    }
}

/// Register a waiter the way `request_with_id` does, and hand back its receiver.
fn waiting_on(
    pending: &Arc<Mutex<Pending>>,
    id: RequestId,
    method: &str,
) -> oneshot::Receiver<Result<Value, RequestFailure>> {
    let (tx, rx) = oneshot::channel();
    pending.lock().unwrap().insert(
        id,
        PendingRequest {
            method: method.to_string(),
            tx,
        },
    );
    rx
}

#[tokio::test]
async fn a_read_error_is_a_death_and_not_a_silent_stall() {
    // The failure this replaces: the error arm shared the EOF path, so nothing
    // was published. `death()` stayed `None`, so the restart policy was never
    // consulted and every later request waited out its whole deadline and
    // reported `TimedOut` — a server that looks merely slow, forever.
    let w = wiring();
    let waiter = waiting_on(&w.pending, RequestId::Num(1), "textDocument/references");

    tokio::time::timeout(
        Duration::from_secs(5),
        read_loop(
            BreaksAfterOneRead { first: None },
            w.out.downgrade(),
            w.pending.clone(),
            w.notifications.clone(),
            w.signal.clone(),
            w.death.clone(),
            w.stderr.clone(),
            None,
        ),
    )
    .await
    .expect("the reader must return rather than hang");

    let death = w.death.borrow().clone().expect("a read error is a death");
    match &death.reason {
        DeathReason::ReadFailed(detail) => assert!(
            detail.contains("broke"),
            "the reason must carry the io error, not discard it: {detail}"
        ),
        other => panic!("expected a read failure, got {other:?}"),
    }
    assert!(
        matches!(waiter.await, Ok(Err(RequestFailure::ServerDied(_)))),
        "a waiter must be failed by the death, not left to time out"
    );
}

#[tokio::test]
async fn a_frame_read_before_the_error_is_still_delivered() {
    // The error must not cost the bytes that arrived before it.
    let w = wiring();
    let waiter = waiting_on(&w.pending, RequestId::Num(4), "textDocument/definition");
    let frame = framing::encode(br#"{"jsonrpc":"2.0","id":4,"result":"in time"}"#);

    tokio::time::timeout(
        Duration::from_secs(5),
        read_loop(
            BreaksAfterOneRead { first: Some(frame) },
            w.out.downgrade(),
            w.pending.clone(),
            w.notifications.clone(),
            w.signal.clone(),
            w.death.clone(),
            w.stderr.clone(),
            None,
        ),
    )
    .await
    .expect("the reader must return");

    assert_eq!(waiter.await, Ok(Ok(json!("in time"))));
}

#[tokio::test]
async fn a_write_failure_kills_the_process_it_can_no_longer_reach() {
    // The one death that does *not* imply the process has ended, and therefore
    // the one after which `Drop` will not kill anything: it only kills while no
    // death has been published. Before this, a server whose stdin broke while it
    // was still running survived — with its children — for the life of the app.
    //
    // Needs `sh` on PATH (Git Bash on Windows), the same assumption the
    // `process::` tests already make.
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg("sleep 30")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("a stand-in process — is `sh` on PATH? run cargo test from Git Bash");
    let pid = child.id();

    let w = wiring();
    let waiter = waiting_on(&w.pending, RequestId::Num(2), "textDocument/references");
    w.out.send(b"anything at all".to_vec()).expect("queued");
    let queue = w.out_rx;
    drop(w.out);

    tokio::time::timeout(
        Duration::from_secs(5),
        write_loop(
            BrokenStdin,
            queue,
            w.death.clone(),
            w.pending.clone(),
            w.stderr.clone(),
            Some(pid),
        ),
    )
    .await
    .expect("the writer must return rather than hang");

    let death = w
        .death
        .borrow()
        .clone()
        .expect("a write failure is a death");
    assert!(
        matches!(death.reason, DeathReason::WriteFailed(_)),
        "got {:?}",
        death.reason
    );
    assert!(matches!(
        waiter.await,
        Ok(Err(RequestFailure::ServerDied(_)))
    ));

    // The process itself, which nothing else would ever have killed.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            _ if Instant::now() > deadline => {
                let _ = child.kill();
                panic!("the process outlived a write failure — that is the leak");
            }
            _ => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    }
}

// ---------------------------------------------------------------------------
// An answer we cannot read is not an answer that never came
// ---------------------------------------------------------------------------

#[test]
fn an_unreadable_response_fails_its_own_waiter_by_name() {
    // A string error `code` — legal JSON, correctly framed, and classified as
    // nothing. Dropping it turned "the server said no in 3 ms" into "the server
    // did not answer within 20 s": a different one of the five failures, and the
    // only one that reads as slowness.
    let w = wiring();
    let mut waiter = waiting_on(&w.pending, RequestId::Num(3), "textDocument/references");

    dispatch(
        br#"{"jsonrpc":"2.0","id":3,"error":{"code":"E_FAIL","message":"boom"}}"#,
        &w.out.downgrade(),
        &w.pending,
        &w.notifications,
        &w.signal,
    );

    match waiter.try_recv() {
        Ok(Err(RequestFailure::Malformed { method, detail })) => {
            assert_eq!(method, "textDocument/references");
            assert!(!detail.is_empty(), "the reason must say what was wrong");
        }
        other => panic!("expected a malformed answer, got {other:?}"),
    }
    assert!(
        w.pending.lock().unwrap().is_empty(),
        "the waiter must be removed, not left to time out as well"
    );
}

#[test]
fn an_unreadable_frame_with_no_id_strands_nobody() {
    // Nothing to correlate it to, so it is a genuine drop — and it must not
    // resolve some *other* waiter, which is the misdelivery this module exists
    // to prevent.
    let w = wiring();
    let waiter = waiting_on(&w.pending, RequestId::Num(9), "textDocument/references");

    for body in [
        &b"not json at all"[..],
        br#"{"jsonrpc":"2.0"}"#,
        br#"{"jsonrpc":"2.0","id":{"nested":true},"error":{"code":"E"}}"#,
    ] {
        dispatch(
            body,
            &w.out.downgrade(),
            &w.pending,
            &w.notifications,
            &w.signal,
        );
    }

    assert!(
        w.pending.lock().unwrap().contains_key(&RequestId::Num(9)),
        "an unaddressable frame must not resolve somebody else's request"
    );
    drop(waiter);
}

// ---------------------------------------------------------------------------
// A signal the server sends once
// ---------------------------------------------------------------------------
//
// The Phase 5 defect, reproduced without a process. `notifications` is a
// `broadcast`, so a subscriber that falls behind loses the oldest messages —
// correct for progress reports, and fatal for a message sent **once**. Roslyn
// sends thousands of notifications while loading and then
// `workspace/projectInitializationComplete` exactly once, so the readiness
// watch lagged past the only message it existed to see, waited out its 90 s
// ceiling, and every count taken from that server was zero.
//
// These are deliberately *not* about the buffer size: raising
// `NOTIFICATION_BUFFER` moves the window and closes nothing.

const READY_SIGNAL: &str = "workspace/projectInitializationComplete";

fn watching_for_the_signal() -> SignalFilter {
    Arc::new(|notification: &Notification| notification.method == READY_SIGNAL)
}

fn notification_frame_bytes(method: &str, params: Value) -> Vec<u8> {
    let body = json!({ "jsonrpc": "2.0", "method": method, "params": params });
    framing::encode(&serde_json::to_vec(&body).expect("a notification must serialise"))
}

#[tokio::test]
async fn a_one_shot_notification_is_recorded_even_when_the_stream_drops_it() {
    // One signal, then twice the buffer's worth of chatter, into a subscriber
    // that never reads. The channel is then guaranteed — not likely — to have
    // discarded the signal, which is exactly what a busy watcher task suffers.
    let w = wiring_watching(Some(watching_for_the_signal()), NOTIFICATION_BUFFER);
    let mut stream = w.notifications.subscribe();
    let seen = w.signal.seen.subscribe();

    let mut bytes = notification_frame_bytes(READY_SIGNAL, json!({ "loaded": true }));
    for n in 0..(NOTIFICATION_BUFFER * 2) {
        bytes.extend(notification_frame_bytes(
            "window/logMessage",
            json!({ "type": 4, "message": format!("progress {n}") }),
        ));
    }

    tokio::time::timeout(
        Duration::from_secs(5),
        read_loop(
            bytes.as_slice(),
            w.out.downgrade(),
            w.pending.clone(),
            w.notifications.clone(),
            w.signal.clone(),
            w.death.clone(),
            w.stderr.clone(),
            None,
        ),
    )
    .await
    .expect("the reader must reach EOF");

    // Half the point: the live stream really has lost it.
    assert!(
        matches!(
            stream.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(_))
        ),
        "the subscriber must have fallen behind, or this test proves nothing"
    );
    let mut still_readable = Vec::new();
    while let Ok(notification) = stream.try_recv() {
        still_readable.push(notification.method);
    }
    assert!(
        !still_readable.iter().any(|method| method == READY_SIGNAL),
        "the signal is gone from the stream; that is the bug being survived"
    );

    // The other half: it was recorded as the frame was decoded, so it is here
    // regardless.
    let recorded = seen.borrow().clone().expect("the signal must be recorded");
    assert_eq!(recorded.method, READY_SIGNAL);
    assert_eq!(
        recorded.params.expect("params travel with it")["loaded"],
        json!(true),
        "the whole notification is kept, not merely the fact of it"
    );
}

#[tokio::test]
async fn a_signal_that_arrived_before_anybody_watched_still_counts() {
    // The second, independent way to miss it: the readiness watch is spawned
    // after `initialize` returns, and a server can send its signal inside that
    // window. Sticky state is read, not received, so subscribing afterwards is
    // not too late.
    let w = wiring_watching(Some(watching_for_the_signal()), NOTIFICATION_BUFFER);

    tokio::time::timeout(
        Duration::from_secs(5),
        read_loop(
            notification_frame_bytes(READY_SIGNAL, Value::Null).as_slice(),
            w.out.downgrade(),
            w.pending.clone(),
            w.notifications.clone(),
            w.signal.clone(),
            w.death.clone(),
            w.stderr.clone(),
            None,
        ),
    )
    .await
    .expect("the reader must reach EOF");

    // Subscribed only now — after the notification has been and gone.
    let late = w.signal.seen.subscribe();
    assert_eq!(
        late.borrow().as_ref().map(|n| n.method.clone()),
        Some(READY_SIGNAL.to_string()),
        "a signal that arrived before this observer existed must still count"
    );
}

#[test]
fn only_the_notification_the_filter_names_is_recorded() {
    let w = wiring_watching(Some(watching_for_the_signal()), 8);
    for method in [
        "window/logMessage",
        "$/progress",
        "textDocument/publishDiagnostics",
    ] {
        dispatch(
            &notification_body(method),
            &w.out.downgrade(),
            &w.pending,
            &w.notifications,
            &w.signal,
        );
    }
    assert_eq!(
        *w.signal.seen.borrow(),
        None,
        "chatter is not the signal, and a false positive declares a half-loaded server ready"
    );
}

#[test]
fn the_first_match_wins_and_a_later_one_does_not_overwrite_it() {
    // A signal is a fact about the past. If a server repeats it, the record must
    // still be the arrival that readiness was waiting for.
    let w = wiring_watching(Some(watching_for_the_signal()), 8);
    for which in ["first", "second"] {
        let body = json!({
            "jsonrpc": "2.0",
            "method": READY_SIGNAL,
            "params": { "which": which },
        });
        dispatch(
            &serde_json::to_vec(&body).expect("serialises"),
            &w.out.downgrade(),
            &w.pending,
            &w.notifications,
            &w.signal,
        );
    }

    let recorded = w.signal.seen.borrow().clone().expect("recorded");
    assert_eq!(recorded.params.expect("params")["which"], json!("first"));
}

#[test]
fn a_transport_watching_for_nothing_records_nothing() {
    // `None` is "nothing about this server is one-shot", not "it has not
    // arrived yet" — and the two must not be spelled the same way.
    let w = wiring();
    dispatch(
        &notification_body(READY_SIGNAL),
        &w.out.downgrade(),
        &w.pending,
        &w.notifications,
        &w.signal,
    );
    assert_eq!(*w.signal.seen.borrow(), None);
}

fn notification_body(method: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({ "jsonrpc": "2.0", "method": method, "params": {} }))
        .expect("serialises")
}

// ---------------------------------------------------------------------------
// The bounded stderr tail
// ---------------------------------------------------------------------------

#[test]
fn the_stderr_tail_keeps_the_last_lines_and_drops_the_rest() {
    let tail = Arc::new(Mutex::new(VecDeque::new()));
    for n in 0..(STDERR_TAIL_LINES * 3) {
        remember_stderr(&tail, format!("line {n}"));
    }

    let lines = tail_lines(&tail);
    assert_eq!(lines.len(), STDERR_TAIL_LINES);
    // The *last* lines: a crash message is explained by what happened just
    // before it, not by the server's start-up banner.
    assert_eq!(
        lines.last().unwrap(),
        &format!("line {}", STDERR_TAIL_LINES * 3 - 1)
    );
    assert_eq!(
        lines.first().unwrap(),
        &format!("line {}", STDERR_TAIL_LINES * 2)
    );
}
