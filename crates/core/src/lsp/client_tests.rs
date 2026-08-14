//! Unit tests for the parts of the client that need no server.
//!
//! Everything that *does* need one lives in `crates/core/tests/lsp_client.rs`
//! and drives the scripted `cb-fake-lsp` binary. The split follows the
//! transport's: the readiness signal table and the decode-error rewrite are
//! wrong-or-right decisions rather than fast-or-slow ones, and neither should
//! need a child process to prove — which is also the only way the awkward
//! shapes (a numeric progress token, a `begin` for the right token) get covered
//! at all, since no fake would think to send them.

use super::*;

fn notification(method: &str, params: Value) -> Notification {
    Notification {
        method: method.to_string(),
        params: Some(params),
    }
}

fn progress(token: Value, kind: &str) -> Notification {
    notification(
        method::PROGRESS,
        json!({ "token": token, "value": { "kind": kind } }),
    )
}

const RUST_ANALYZER: Readiness = Readiness::Progress {
    token_prefix: "rustAnalyzer",
};

// ---------------------------------------------------------------------------
// Which notification means "ready"
// ---------------------------------------------------------------------------

#[test]
fn a_progress_end_for_a_matching_token_is_the_signal() {
    assert!(signals_ready(
        RUST_ANALYZER,
        &progress(json!("rustAnalyzer/Indexing"), "end")
    ));
    // The prefix is a prefix, not a namespace: the token may be exactly it.
    assert!(signals_ready(
        RUST_ANALYZER,
        &progress(json!("rustAnalyzer"), "end")
    ));
}

#[test]
fn a_beginning_or_a_report_is_the_opposite_of_ready() {
    // The one that would be actively wrong: a `begin` for the very token we are
    // waiting on means the priming has *started*.
    for kind in ["begin", "report"] {
        assert!(
            !signals_ready(
                RUST_ANALYZER,
                &progress(json!("rustAnalyzer/Indexing"), kind)
            ),
            "`{kind}` must not be read as readiness"
        );
    }
}

#[test]
fn somebody_elses_progress_token_is_not_our_signal() {
    assert!(!signals_ready(
        RUST_ANALYZER,
        &progress(json!("somethingElse/Loading"), "end")
    ));
    // A near miss on case is a miss. Nothing here guesses.
    assert!(!signals_ready(
        RUST_ANALYZER,
        &progress(json!("rustanalyzer/Indexing"), "end")
    ));
}

#[test]
fn a_progress_token_we_cannot_read_is_not_a_match() {
    // A numeric token is legal and cannot be prefix-matched against anything.
    // Treating it as ours would declare a half-built index trustworthy.
    assert!(!signals_ready(RUST_ANALYZER, &progress(json!(7), "end")));
    assert!(!signals_ready(
        RUST_ANALYZER,
        &notification(method::PROGRESS, json!({ "value": { "kind": "end" } }))
    ));
    assert!(!signals_ready(
        RUST_ANALYZER,
        &notification(method::PROGRESS, json!({ "token": "rustAnalyzer" }))
    ));
    assert!(!signals_ready(
        RUST_ANALYZER,
        &Notification {
            method: method::PROGRESS.to_string(),
            params: None,
        }
    ));
}

#[test]
fn a_log_message_is_never_a_readiness_signal() {
    let chatter = notification("window/logMessage", json!({ "type": 3, "message": "end" }));
    assert!(!signals_ready(RUST_ANALYZER, &chatter));
    assert!(!signals_ready(Readiness::RoslynProjectInit, &chatter));
}

#[test]
fn roslyn_waits_for_its_own_notification_and_nothing_else() {
    assert!(signals_ready(
        Readiness::RoslynProjectInit,
        &Notification {
            method: "workspace/projectInitializationComplete".to_string(),
            params: None,
        }
    ));
    assert!(!signals_ready(
        Readiness::RoslynProjectInit,
        &progress(json!("roslyn/load"), "end")
    ));
}

#[test]
fn the_caveat_names_the_signal_that_never_came() {
    // A caveat that says only "still loading" tells a user nothing they can act
    // on or report; naming the message is what makes it a bug report.
    assert!(awaited_signal(Readiness::RoslynProjectInit)
        .contains("workspace/projectInitializationComplete"));
    let progress = awaited_signal(RUST_ANALYZER);
    assert!(progress.contains("$/progress"), "{progress}");
    assert!(progress.contains("rustAnalyzer"), "{progress}");
}

// ---------------------------------------------------------------------------
// The three readiness states
// ---------------------------------------------------------------------------

#[test]
fn a_caveat_is_ready_and_loading_is_not() {
    assert!(!ReadyState::Loading.is_ready());
    assert!(ReadyState::Ready.is_ready());
    assert!(
        ReadyState::ReadyWithCaveat {
            detail: "no signal".to_string()
        }
        .is_ready(),
        "the caveat qualifies the answer; it does not withhold it"
    );
}

// ---------------------------------------------------------------------------
// Where the readiness watch reads the signal from
// ---------------------------------------------------------------------------
//
// The Phase 5 defect. `workspace/projectInitializationComplete` is sent **once**,
// the notification channel is a `broadcast` that drops the oldest messages from
// a subscriber that falls behind, and Roslyn sends far more chatter during a
// solution load than any buffer holds. So the watch had lagged past the only
// message it existed to see, waited out the 90 s ceiling, published
// `ReadyWithCaveat`, and every count taken from that server was a confident zero.
//
// Both tests use a ceiling an hour long, so nothing here can pass by promotion:
// `Ready` (not `ReadyWithCaveat`) is the assertion, and it has to be reached
// promptly.

/// Long enough that the ceiling cannot be what ends either test.
const NEVER_REACHED: Duration = Duration::from_secs(3600);

fn the_roslyn_signal() -> Notification {
    Notification {
        method: PROJECT_INITIALIZATION_COMPLETE.to_string(),
        params: None,
    }
}

/// Wait for the watch to publish a ready state, bounded so a hang is a failure.
async fn ready_within(mut ready: watch::Receiver<ReadyState>, bound: Duration) -> ReadyState {
    tokio::time::timeout(bound, async {
        loop {
            let state = ready.borrow_and_update().clone();
            if state.is_ready() {
                return state;
            }
            ready.changed().await.expect("the watch outlives this wait");
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the server was never reported ready within {bound:?}"))
}

#[tokio::test]
async fn a_signal_recorded_before_the_watch_started_still_makes_the_server_ready() {
    // The second way to miss it, independent of lag: the watch is spawned after
    // `initialize` returns, and the server may have sent its signal inside that
    // window. Sticky state is *read*, so starting late is not too late.
    let (notifications, _keep_alive) = broadcast::channel(8);
    let (signal, signal_rx) = watch::channel(Some(the_roslyn_signal()));
    let (death, death_rx) = watch::channel(None);

    let ready = spawn_readiness_watch(
        Readiness::RoslynProjectInit,
        notifications.subscribe(),
        signal_rx,
        NEVER_REACHED,
        death_rx,
    );

    assert_eq!(
        ready_within(ready, Duration::from_secs(5)).await,
        ReadyState::Ready,
        "a signal that arrived before this watcher existed must still count, and \
         must not arrive as a caveat"
    );
    drop((notifications, signal, death));
}

#[tokio::test]
async fn a_watch_that_lagged_past_the_chatter_is_still_ready_when_the_signal_lands() {
    // Lag stays non-fatal — a chatty but healthy server must not be stranded on
    // "loading" — and it no longer costs the signal, because the signal does not
    // travel on the stream that lagged.
    let (notifications, _keep_alive) = broadcast::channel(2);
    let stream = notifications.subscribe();
    for n in 0..16 {
        notifications
            .send(notification(
                "window/logMessage",
                json!({ "type": 4, "message": format!("loading {n}") }),
            ))
            .expect("a subscriber exists");
    }

    let (signal, signal_rx) = watch::channel(None);
    let (death, death_rx) = watch::channel(None);
    let ready = spawn_readiness_watch(
        Readiness::RoslynProjectInit,
        stream,
        signal_rx,
        NEVER_REACHED,
        death_rx,
    );

    // Nothing has said "ready" yet: chatter is not a signal, and lag is not one
    // either.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(*ready.borrow(), ReadyState::Loading);

    signal
        .send(Some(the_roslyn_signal()))
        .expect("the watch is listening");
    assert_eq!(
        ready_within(ready, Duration::from_secs(5)).await,
        ReadyState::Ready,
        "a watcher that gave up on lag would still be loading here"
    );
    drop((notifications, death));
}

/// Wait for the watch to publish a state that is not the one already there.
///
/// Fails rather than hangs on a closed channel: a watcher that published its
/// last word and returned drops the sender, and "nothing more will ever be said"
/// is the defect under test, not a reason to wait.
async fn next_state_within(ready: &mut watch::Receiver<ReadyState>, bound: Duration) -> ReadyState {
    tokio::time::timeout(bound, async {
        ready
            .changed()
            .await
            .expect("the watcher still holds its sender");
        ready.borrow_and_update().clone()
    })
    .await
    .unwrap_or_else(|_| panic!("no further readiness was published within {bound:?}"))
}

#[tokio::test]
async fn a_server_promoted_at_the_ceiling_is_corrected_when_the_signal_finally_lands() {
    // The ceiling is a statement about how long we were prepared to wait, not a
    // verdict on the server. A solution that takes longer than the ceiling to
    // load still finishes loading, and the signal saying so is sticky state
    // sitting in the transport — so a watcher that published the caveat and
    // returned left every later count on every file in the workspace rendered as
    // a lower bound ("at least 1 usage", or "Usages unknown" at zero) for the
    // life of the app, with exact numbers available the whole time.
    let (notifications, _keep_alive) = broadcast::channel(8);
    let (signal, signal_rx) = watch::channel(None);
    let (death, death_rx) = watch::channel(None);

    let mut ready = spawn_readiness_watch(
        Readiness::RoslynProjectInit,
        notifications.subscribe(),
        signal_rx,
        Duration::from_millis(100),
        death_rx,
    );

    match next_state_within(&mut ready, Duration::from_secs(5)).await {
        ReadyState::ReadyWithCaveat { .. } => {}
        other => panic!("expected the ceiling to publish a caveat, got {other:?}"),
    }

    // The projects finish loading 30 s late, as a large `.sln` routinely does.
    signal
        .send(Some(the_roslyn_signal()))
        .expect("the watch is listening");

    assert_eq!(
        next_state_within(&mut ready, Duration::from_secs(5)).await,
        ReadyState::Ready,
        "the signal the caveat named has now arrived, so the caveat is no longer \
         true and must be withdrawn"
    );
    drop((notifications, death));
}

#[tokio::test]
async fn a_server_that_dies_after_the_ceiling_is_not_upgraded_by_a_late_signal() {
    // The correction must not become a second way to launder a corpse: once the
    // process is gone, nothing it said before it went makes its answers
    // trustworthy, and `Ready` is the state a UI stops explaining itself on.
    let (notifications, _keep_alive) = broadcast::channel(8);
    let (signal, signal_rx) = watch::channel(None);
    let (death, death_rx) = watch::channel(None);

    let mut ready = spawn_readiness_watch(
        Readiness::RoslynProjectInit,
        notifications.subscribe(),
        signal_rx,
        Duration::from_millis(100),
        death_rx,
    );
    match next_state_within(&mut ready, Duration::from_secs(5)).await {
        ReadyState::ReadyWithCaveat { .. } => {}
        other => panic!("expected the ceiling to publish a caveat, got {other:?}"),
    }

    death
        .send(Some(Death {
            code: Some(1),
            reason: crate::lsp::transport::DeathReason::Exited,
            stderr_tail: Vec::new(),
        }))
        .expect("the watch is listening");
    tokio::time::sleep(Duration::from_millis(50)).await;
    // Ignored rather than expected: the watcher is entitled to have stopped
    // listening the moment the server died, and a signal nobody receives is the
    // right outcome for one that arrives after the corpse.
    let _ = signal.send(Some(the_roslyn_signal()));
    tokio::time::sleep(Duration::from_millis(100)).await;

    match &*ready.borrow() {
        ReadyState::ReadyWithCaveat { .. } => {}
        other => panic!("a dead server must keep its caveat, got {other:?}"),
    }
    drop(notifications);
}

// ---------------------------------------------------------------------------
// The log directory
// ---------------------------------------------------------------------------

#[test]
fn the_log_directory_keeps_the_newest_logs_and_deletes_the_rest() {
    // Roslyn writes a fresh log per launch and nothing else ever deletes one, so
    // an unbounded directory is a workspace that grows for as long as the app is
    // used. Every other capture directory in this codebase (`inspect::dumps`) is
    // pruned by both count and bytes; this one was pruned by neither.
    let dir = tempfile::tempdir().expect("a temp dir");
    let root = dir.path();
    for (i, size) in [(0usize, 10u64), (1, 10), (2, 10), (3, 10)] {
        let path = root.join(format!("log-{i}.txt"));
        std::fs::write(&path, vec![b'x'; size as usize]).expect("written");
        // Distinct mtimes, oldest first, so "oldest" is a fact rather than a
        // filesystem coincidence.
        filetime_backdated(&path, 4 - i as u64);
    }

    let removed = prune_log_dir(root, 2, 1024);
    assert_eq!(removed, 2, "two of the four are older than the newest two");
    let left = surviving_names(root);
    assert_eq!(left, vec!["log-2.txt".to_string(), "log-3.txt".to_string()]);
}

#[test]
fn a_log_directory_over_its_byte_budget_is_trimmed_even_within_the_count() {
    // Count and bytes are both limits and either can bind first: one 40 MB log
    // is under any count and is still the whole budget.
    let dir = tempfile::tempdir().expect("a temp dir");
    let root = dir.path();
    for i in 0..3usize {
        let path = root.join(format!("log-{i}.txt"));
        std::fs::write(&path, vec![b'x'; 100]).expect("written");
        filetime_backdated(&path, 3 - i as u64);
    }

    let removed = prune_log_dir(root, 10, 150);
    assert_eq!(
        removed, 2,
        "only the newest fits in the budget, so the other two go"
    );
    assert_eq!(surviving_names(root), vec!["log-2.txt".to_string()]);
}

/// Set a file's modified time this many seconds into the past.
fn filetime_backdated(path: &Path, seconds_ago: u64) {
    let when = std::time::SystemTime::now() - Duration::from_secs(seconds_ago * 10);
    let file = std::fs::File::options()
        .write(true)
        .open(path)
        .expect("openable");
    file.set_modified(when).expect("mtime is settable");
}

fn surviving_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("readable")
        .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().to_string()))
        .collect();
    names.sort();
    names
}

// ---------------------------------------------------------------------------
// Naming the question that was asked
// ---------------------------------------------------------------------------

#[test]
fn a_decode_failure_is_renamed_after_the_request_that_was_actually_made() {
    // `decode_goto` hard-codes `textDocument/definition` because one decoder
    // serves four requests. Letting that through names the wrong question.
    let from_the_decoder = DecodeError::Shape {
        what: method::DEFINITION,
        detail: "an object with keys [surprise]".to_string(),
    };
    let error = malformed(method::TYPE_DEFINITION, from_the_decoder);

    match &error {
        RequestError::Failed {
            method,
            failure:
                RequestFailure::Malformed {
                    method: named,
                    detail,
                },
        } => {
            assert_eq!(*method, method::TYPE_DEFINITION);
            assert_eq!(named, method::TYPE_DEFINITION);
            assert_eq!(detail, "an object with keys [surprise]");
        }
        other => panic!("expected a malformed failure, got {other:?}"),
    }
    assert!(
        !error.to_string().contains(method::DEFINITION),
        "the rendered message must not name a request nobody made: {error}"
    );
}

#[test]
fn an_unsupported_request_says_which_capability_was_missing() {
    let error = RequestError::Unsupported {
        method: method::IMPLEMENTATION,
        capability: "implementationProvider",
    };
    let rendered = error.to_string();
    assert!(rendered.contains(method::IMPLEMENTATION), "{rendered}");
    assert!(rendered.contains("implementationProvider"), "{rendered}");
    assert!(
        rendered.contains("does not support"),
        "the sentence has to be sayable to a user: {rendered}"
    );
}

// ---------------------------------------------------------------------------
// Deadlines
// ---------------------------------------------------------------------------

#[test]
fn the_first_request_gets_the_start_up_budget_and_the_rest_do_not() {
    let timeouts = Timeouts {
        first_request: Duration::from_secs(180),
        request: Duration::from_secs(20),
        document_symbol: Duration::from_secs(15),
    };

    assert_eq!(
        deadline_for(&timeouts, timeouts.request, true),
        Duration::from_secs(180),
        "the first answer pays for project load"
    );
    assert_eq!(
        deadline_for(&timeouts, timeouts.request, false),
        Duration::from_secs(20),
        "and the rest must not, or a dead server looks merely slow"
    );
    assert_eq!(
        deadline_for(&timeouts, timeouts.document_symbol, false),
        Duration::from_secs(15)
    );
}

#[test]
fn a_larger_per_request_budget_is_never_shortened_by_the_first_request_rule() {
    // Configuration is a user's to write, and one that gives `document_symbol`
    // longer than `first_request` means it, so `max` and not a swap.
    let timeouts = Timeouts {
        first_request: Duration::from_secs(5),
        request: Duration::from_secs(20),
        document_symbol: Duration::from_secs(30),
    };
    assert_eq!(
        deadline_for(&timeouts, timeouts.document_symbol, true),
        Duration::from_secs(30)
    );
}
