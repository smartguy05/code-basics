//! One live server process, and the bytes going in and out of it.
//!
//! This layer knows about frames, ids, timeouts and death. It knows nothing
//! about LSP *semantics* — no handshake, no capabilities, no documents; that is
//! [`super::client`]'s job. What it owns is the part that is hard to get right
//! and impossible to see when it is wrong: correlating a response with the
//! request that is waiting for it, and making sure a waiter is always woken by
//! *something*.
//!
//! # Four tasks per server
//!
//! * **Writer** — drains a queue of outgoing frames into the child's stdin. A
//!   write error is a death, because a server whose stdin we cannot reach can
//!   never answer anything again — and it is the **one** death that does not
//!   imply the process has ended, so it is also the one that has to kill the
//!   tree itself. [`Transport::drop`] will not: it only kills while no death
//!   has been published.
//! * **Reader** — child stdout into [`framing::Decoder`], then
//!   [`jsonrpc::classify`]. A [`framing::FramingError`] is a death and reading
//!   stops: the stream cannot be resynchronised, and guessing where the next
//!   frame starts would silently hand one request's answer to another request's
//!   waiter — a wrong answer rather than a missing one. A read *error* is a
//!   death too, and a different one from EOF: EOF means the process is ending
//!   and the reaper is about to say so, while an error means we have gone deaf
//!   to a writer that may be perfectly healthy. The reader is also where a
//!   one-shot notification is recorded as *state* ([`SignalSink`]), because a
//!   message the server sends once cannot be delivered safely as a message.
//! * **Stderr pump** — a bounded tail of the last [`STDERR_TAIL_LINES`] lines,
//!   kept as diagnostic context for a crash message. Never user-facing output:
//!   a language server's stderr is log noise until the moment it is the only
//!   explanation available.
//! * **Reaper** — waits for the process, publishes the death, and **fails every
//!   pending request**. Without it a server that dies mid-answer leaves its
//!   caller waiting for a reply nobody will ever send.
//!
//! # The failure model, and the one thing that must never happen
//!
//! [`RequestFailure`] keeps a timeout, a dead server, a cancellation, a server
//! error and a malformed payload as five distinct things. **None of them may
//! ever be turned into an empty successful result**, here or in any layer above
//! this one. That flattening is exactly how a subsystem comes to report "0
//! usages" about a method with forty call sites.
//!
//! Nor may they be collapsed into *each other*. [`RequestFailure::TimedOut`] is
//! the tempting default — a waiter that is never woken eventually times out on
//! its own, so every dropped message silently becomes "slow" — which is why an
//! answer this layer cannot read still fails its own waiter by id, and why a
//! stream we can no longer read publishes a death rather than going quiet.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::lsp::framing::{self, Decoder};
use crate::lsp::jsonrpc::{classify, Incoming, Outgoing, RequestId, RpcError, METHOD_NOT_FOUND};
use crate::process::{
    configure_process_group, kill_tree, resolve_program, LineSplitter, Utf8Chunker,
};

/// How many stderr lines to keep. Enough to explain a crash, small enough that
/// a server that has started spinning cannot grow this without bound.
pub const STDERR_TAIL_LINES: usize = 200;

/// At most one automatic restart per this long. See [`RestartPolicy`].
pub const RESTART_WINDOW: Duration = Duration::from_secs(60);

/// How long `shutdown` is given to be answered. Short on purpose: a server that
/// will not answer this is a server we are about to kill anyway.
const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(3);

/// How long the process is given to exit on its own after `exit` and EOF.
const EXIT_GRACE: Duration = Duration::from_secs(3);

/// How many outgoing notifications are buffered for observers of
/// [`Transport::notifications`].
///
/// Deliberately unchanged at 256, and deliberately not the mechanism anything
/// depends on. Roslyn emits thousands of notifications while loading a solution,
/// so any finite number here is a window a slow subscriber can fall through; a
/// message that must not be missed is recorded by [`SignalSink`] as the reader
/// decodes it instead. Raising this would only move the window, and would hide
/// how easily a subscriber lags — which is the failure this pairing exists to
/// survive rather than avoid.
const NOTIFICATION_BUFFER: usize = 256;

/// One request waiting for its answer.
///
/// The method travels with the waiter because the *reader* sometimes has to
/// fail it — an unreadable response can be correlated by id but names no
/// method — and [`RequestFailure::Malformed`] has to say which question went
/// unanswered. Recovering it from the id afterwards is not possible.
struct PendingRequest {
    method: String,
    tx: oneshot::Sender<Result<Value, RequestFailure>>,
}

type Pending = HashMap<RequestId, PendingRequest>;
type StderrTail = Arc<Mutex<VecDeque<String>>>;

// ---------------------------------------------------------------------------
// Death
// ---------------------------------------------------------------------------

/// Why a server stopped being usable.
///
/// A process that is still running but whose byte stream we can no longer read
/// is just as dead to us as one that exited, and lying about which of the two
/// happened would make the stderr tail unreadable as evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeathReason {
    /// The process exited (or was killed).
    Exited,
    /// The frame stream could not be resynchronised.
    Framing(String),
    /// Writing to the server's stdin failed.
    WriteFailed(String),
    /// Reading the server's stdout failed. Distinct from [`DeathReason::Exited`]
    /// on purpose: the process may still be running, and distinct from
    /// [`DeathReason::Framing`] because the bytes were never seen at all rather
    /// than seen and found unusable.
    ReadFailed(String),
    /// We took it down deliberately.
    ShutDown,
}

impl std::fmt::Display for DeathReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exited => write!(f, "the server process exited"),
            Self::Framing(detail) => {
                write!(f, "the server's output stopped being framed: {detail}")
            }
            Self::WriteFailed(detail) => write!(f, "could not write to the server: {detail}"),
            Self::ReadFailed(detail) => write!(f, "could not read from the server: {detail}"),
            Self::ShutDown => write!(f, "the server was shut down"),
        }
    }
}

/// A server that is no longer answering, and everything we know about why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Death {
    /// `None` when the process was signalled, or has not exited at all (a
    /// framing failure kills the *session*, not necessarily the process).
    pub code: Option<i32>,
    pub reason: DeathReason,
    /// The last [`STDERR_TAIL_LINES`] lines the server wrote. Usually the only
    /// thing that explains a start-up failure.
    pub stderr_tail: Vec<String>,
}

impl std::fmt::Display for Death {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.reason)?;
        if let Some(code) = self.code {
            write!(f, " (exit code {code})")?;
        }
        if let Some(last) = self.stderr_tail.last() {
            write!(f, "; last stderr: {last}")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Failure
// ---------------------------------------------------------------------------

/// Why a request did not produce an answer.
///
/// Five distinct things, deliberately. Collapsing any of them into "no results"
/// is the failure mode this whole feature is written to avoid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RequestFailure {
    #[error("the server returned an error ({code}): {message}")]
    ServerError { code: i64, message: String },
    #[error("the server did not answer within {after:?}")]
    TimedOut { after: Duration },
    #[error("{0}")]
    ServerDied(Death),
    #[error("the request was cancelled")]
    Cancelled,
    #[error("the server's answer to {method} could not be read: {detail}")]
    Malformed { method: String, detail: String },
}

// ---------------------------------------------------------------------------
// Launching
// ---------------------------------------------------------------------------

/// Everything needed to start one server process.
///
/// Deliberately not [`super::registry::ServerSpec`]: that type also carries
/// timeouts, readiness and URI spelling, none of which this layer has any
/// business reading. Translating one into the other is [`super::client`]'s job.
#[derive(Debug, Clone)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Layered over the inherited environment, the way [`crate::process`]
    /// layers its own defaults.
    pub env: BTreeMap<String, String>,
}

/// A message the server sent us that expects no reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub method: String,
    pub params: Option<Value>,
}

/// Which notification is the one that must not be missed.
///
/// Opaque to this layer: it is *evaluated* here and interpreted nowhere here, so
/// no LSP semantics leak into the transport. [`super::client`] builds it out of
/// its readiness rule, which is the only place that knows what "ready" means.
pub type SignalFilter = Arc<dyn Fn(&Notification) -> bool + Send + Sync>;

/// The sticky record of a one-shot notification, written by the reader.
///
/// [`Transport::notifications`] is a `broadcast`, so a subscriber that falls
/// behind loses messages — which is the right trade for progress reports and the
/// wrong one for a message the server sends **once**. Roslyn's
/// `workspace/projectInitializationComplete` is sent once, after thousands of
/// progress notifications, and a readiness watch that only read the live stream
/// missed it: the answers taken from that server were then empty because its
/// projects had genuinely not loaded.
///
/// So the match is recorded here, on the reader task, in the same step that
/// decodes the frame. Two ways of missing it close at once: a subscriber that
/// lagged past the message, and an observer that had not started when it
/// arrived. The first match wins — a signal is a fact about the past, and a
/// later notification must not overwrite the one that was waited for.
struct SignalSink {
    filter: Option<SignalFilter>,
    seen: watch::Sender<Option<Notification>>,
}

impl SignalSink {
    fn new(filter: Option<SignalFilter>) -> Self {
        let (seen, _) = watch::channel(None);
        Self { filter, seen }
    }

    fn record(&self, notification: &Notification) {
        let Some(filter) = self.filter.as_ref() else {
            return;
        };
        if !filter(notification) {
            return;
        }
        self.seen.send_if_modified(|slot| match slot {
            Some(_) => false,
            None => {
                *slot = Some(notification.clone());
                true
            }
        });
    }
}

// ---------------------------------------------------------------------------
// The transport
// ---------------------------------------------------------------------------

/// One running language server, addressed as requests and notifications.
pub struct Transport {
    pid: Option<u32>,
    /// `None` once stdin has been closed by [`Transport::shutdown`]. This is
    /// the **only** strong sender — the reader holds a
    /// [`mpsc::WeakUnboundedSender`] — so taking it really does end the writer
    /// task, which drops the writer's `ChildStdin`. EOF is the only thing some
    /// servers react to, and a second strong clone anywhere would silently turn
    /// that step into a no-op.
    out: Mutex<Option<mpsc::UnboundedSender<Vec<u8>>>>,
    pending: Arc<Mutex<Pending>>,
    next_id: AtomicI64,
    notifications: broadcast::Sender<Notification>,
    /// The one notification that may not be lost, kept as state rather than as a
    /// message. See [`SignalSink`].
    signal: Arc<SignalSink>,
    death: watch::Sender<Option<Death>>,
    stderr: StderrTail,
}

impl Transport {
    /// Start the server and wire up the four tasks.
    ///
    /// Returns as soon as the process is spawned. Nothing here waits for the
    /// server to be *ready* — readiness is a protocol question, so it belongs
    /// to [`super::client`].
    ///
    /// `watch_for` names the one notification whose arrival must survive a slow
    /// or absent subscriber; it is recorded as state and read through
    /// [`Transport::signal`]. `None` means nothing about this server is
    /// one-shot, which is the honest answer for a server that is ready as soon
    /// as it has answered `initialize`.
    pub fn spawn(launch: &Launch, watch_for: Option<SignalFilter>) -> Result<Self> {
        // A bare name may only exist as a `.cmd` shim on Windows, which
        // `CreateProcess` will not resolve — `typescript-language-server` and
        // `pyright-langserver` both install that way.
        // `to_string_lossy` rather than a panic on non-UTF-8: an unrepresentable
        // path resolves to itself and the spawn error then names it, which is a
        // better report than refusing to try.
        let program = resolve_program(&launch.program.to_string_lossy());
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(&launch.args)
            .current_dir(&launch.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // What makes [`kill_tree`] a *tree* kill on Unix: the child becomes its
        // own process group leader, so its pid doubles as a group id and a
        // negative-pid signal reaches Roslyn's `BuildHost` children too. Without
        // this the Unix path would signal one process and orphan the rest.
        configure_process_group(&mut cmd);
        for (k, v) in &launch.env {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn().with_context(|| {
            format!(
                "failed to start the language server `{}` in {}",
                launch.program.display(),
                launch.cwd.display()
            )
        })?;

        let pid = child.id();
        let stdin = child.stdin.take().context("stdin was not piped")?;
        let stdout = child.stdout.take().context("stdout was not piped")?;
        let stderr = child.stderr.take().context("stderr was not piped")?;

        let (out_tx, out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (notifications, _) = broadcast::channel(NOTIFICATION_BUFFER);
        let signal = Arc::new(SignalSink::new(watch_for));
        let (death, _) = watch::channel(None);
        let pending: Arc<Mutex<Pending>> = Arc::new(Mutex::new(HashMap::new()));
        let tail: StderrTail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));

        tokio::spawn(write_loop(
            stdin,
            out_rx,
            death.clone(),
            pending.clone(),
            tail.clone(),
            pid,
        ));
        tokio::spawn(read_loop(
            stdout,
            // Weak, so the reader is not a second thing keeping the writer's
            // `ChildStdin` open. A strong clone here is why `shutdown`'s
            // "close stdin" step used to close nothing at all.
            out_tx.downgrade(),
            pending.clone(),
            notifications.clone(),
            signal.clone(),
            death.clone(),
            tail.clone(),
            pid,
        ));
        tokio::spawn(stderr_loop(stderr, tail.clone()));
        tokio::spawn(reap(child, death.clone(), pending.clone(), tail.clone()));

        Ok(Self {
            pid,
            out: Mutex::new(Some(out_tx)),
            pending,
            next_id: AtomicI64::new(1),
            notifications,
            signal,
            death,
            stderr: tail,
        })
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// The death, if the server has one. `None` means still usable.
    pub fn death(&self) -> Option<Death> {
        self.death.borrow().clone()
    }

    /// Watch for the death. Resolves immediately if it has already happened.
    pub fn watch_death(&self) -> watch::Receiver<Option<Death>> {
        self.death.subscribe()
    }

    /// Every notification the server sends from now on.
    ///
    /// A subscriber that falls behind loses the oldest messages rather than
    /// stalling the reader: a progress notification is worth less than the
    /// responses queued behind it. That trade is only safe for messages that
    /// repeat — a message the server sends **once** has to be read through
    /// [`Transport::signal`] instead.
    pub fn notifications(&self) -> broadcast::Receiver<Notification> {
        self.notifications.subscribe()
    }

    /// The one-shot notification named at [`Transport::spawn`], if it has
    /// arrived.
    ///
    /// `Some` the moment the reader decodes a matching frame, and `Some`
    /// thereafter — including for an observer that subscribes long afterwards,
    /// and for one that lagged past the message on the live stream. `None` for a
    /// transport spawned with no filter, which is not "it has not arrived" so
    /// much as "nothing is being waited for".
    pub fn signal(&self) -> watch::Receiver<Option<Notification>> {
        self.signal.seen.subscribe()
    }

    /// The last [`STDERR_TAIL_LINES`] lines the server wrote.
    pub fn stderr_tail(&self) -> Vec<String> {
        tail_lines(&self.stderr)
    }

    /// Claim the next request id without sending anything.
    ///
    /// Exists so a caller who may need to [`Transport::cancel`] a request knows
    /// its id *before* awaiting it. Reading the id out of `request` afterwards
    /// is not possible — by then the caller is already blocked on the answer —
    /// and guessing it from a counter would be a correlation bug waiting to
    /// happen.
    pub fn allocate_id(&self) -> RequestId {
        RequestId::Num(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Whether a request has been registered and not yet resolved.
    pub fn is_pending(&self, id: &RequestId) -> bool {
        self.pending.lock().unwrap().contains_key(id)
    }

    /// Ask the server something and wait for its answer.
    ///
    /// Returns the raw `result` value; interpreting it is
    /// [`super::protocol`]'s job. Never returns an empty success for a failure
    /// of any kind — see [`RequestFailure`].
    pub async fn request(
        &self,
        method: &str,
        params: Value,
        deadline: Duration,
    ) -> Result<Value, RequestFailure> {
        self.request_with_id(self.allocate_id(), method, params, deadline)
            .await
    }

    /// [`Transport::request`] under an id the caller already holds, so it can
    /// be superseded with [`Transport::cancel`] from somewhere else.
    pub async fn request_with_id(
        &self,
        id: RequestId,
        method: &str,
        params: Value,
        deadline: Duration,
    ) -> Result<Value, RequestFailure> {
        // Registering before sending is not tidiness, it is the fix for a race.
        // A fast server's response can arrive on the reader task before a
        // registration done *after* the send has happened, and the reader would
        // then see an id nobody is waiting for and (correctly, for a cancelled
        // request's late answer) drop it. The symptom is an intermittent
        // `TimedOut` against a server that answered promptly and correctly.
        let (tx, rx) = oneshot::channel();
        {
            // Checked under the same lock the reaper drains, so a death that
            // lands between the check and the insert cannot leave this entry
            // stranded in a map nobody will ever fail again.
            let mut guard = self.pending.lock().unwrap();
            if let Some(death) = self.death.borrow().clone() {
                return Err(RequestFailure::ServerDied(death));
            }
            guard.insert(
                id.clone(),
                PendingRequest {
                    method: method.to_string(),
                    tx,
                },
            );
        }

        if let Err(failure) = self.send(request_frame(&id, method, params)) {
            self.pending.lock().unwrap().remove(&id);
            return Err(failure);
        }

        let mut death = self.death.subscribe();
        tokio::select! {
            answer = rx => answer.unwrap_or(Err(RequestFailure::Cancelled)),
            _ = tokio::time::sleep(deadline) => {
                self.pending.lock().unwrap().remove(&id);
                // Tell the server to stop working on it. Best effort: a server
                // that is wedged will not read this either, and the caller's
                // answer does not depend on it.
                let _ = self.send(cancel_frame(&id));
                Err(RequestFailure::TimedOut { after: deadline })
            }
            death = wait_for_death(&mut death) => {
                self.pending.lock().unwrap().remove(&id);
                Err(RequestFailure::ServerDied(death))
            }
        }
    }

    /// Tell the server something. Notifications are never answered, so this
    /// only reports whether the bytes could be queued at all.
    pub fn notify(&self, method: &str, params: Value) -> Result<(), RequestFailure> {
        self.send(notification_frame(method, params))
    }

    /// Abandon a request whose answer is no longer wanted.
    ///
    /// The waiter is failed with [`RequestFailure::Cancelled`] immediately;
    /// `$/cancelRequest` is advisory and the server may still answer, in which
    /// case the response is dropped as an unknown id.
    pub fn cancel(&self, id: &RequestId) {
        if let Some(waiter) = self.pending.lock().unwrap().remove(id) {
            let _ = waiter.tx.send(Err(RequestFailure::Cancelled));
        }
        let _ = self.send(cancel_frame(id));
    }

    /// Stop the server: `shutdown`, `exit`, close stdin, then kill the tree.
    ///
    /// The kill is not a formality. Roslyn spawns `BuildHost-netcore` and
    /// `BuildHost-net472` children, and killing only the parent orphans them
    /// for the life of the user's session.
    pub async fn shutdown(&self) {
        // Both are best effort. A server that has already died answers neither,
        // and that is a successful shutdown, not a failure to report.
        let _ = self
            .request("shutdown", Value::Null, SHUTDOWN_DEADLINE)
            .await;
        let _ = self.notify("exit", Value::Null);

        // Dropping the *last strong* sender ends the writer task, which drops
        // `ChildStdin`. EOF on stdin is what a server that ignored `exit` still
        // reacts to — see the field's own doc for why the reader's handle is
        // weak.
        let closed = self.out.lock().unwrap().take();
        drop(closed);

        let mut death = self.death.subscribe();
        if tokio::time::timeout(EXIT_GRACE, wait_for_death(&mut death))
            .await
            .is_err()
        {
            if let Some(pid) = self.pid {
                tracing::warn!(pid, "language server ignored shutdown; killing the tree");
                kill_tree(pid);
            }
        }

        publish_death(
            &self.death,
            &self.pending,
            &self.stderr,
            None,
            DeathReason::ShutDown,
        );
    }
}

impl Drop for Transport {
    /// A dropped transport must not leave a server process behind. `shutdown`
    /// is the graceful path; this is the backstop for a panic or an early
    /// return that never reached it.
    fn drop(&mut self) {
        if self.death.borrow().is_none() {
            if let Some(pid) = self.pid {
                tracing::debug!(pid, "transport dropped without shutdown; killing the tree");
                kill_tree(pid);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Answering the server
// ---------------------------------------------------------------------------

/// The reply body for a server→client request.
///
/// Pure, and separated out for exactly that reason: this table is the
/// difference between a working server and one that hangs during start-up, and
/// it should be provable without a process. Verified against the real Roslyn
/// server, which issues `window/workDoneProgress/create` and
/// `workspace/configuration` while loading and **waits forever** if either goes
/// unanswered — a hang indistinguishable from a slow project load.
///
/// `Ok` is a result, `Err` is an error reply. There is no third option: every
/// input produces a reply, because silence is the one answer that hangs.
pub fn answer_for(method: &str, params: Option<&Value>) -> Result<Value, RpcError> {
    match method {
        // Positional: one entry per requested section, in order. A reply of the
        // wrong length is not a shorter answer, it is an answer about different
        // sections — so the length is taken from the request and nowhere else.
        // `null` means "no opinion, use your default", which is true of every
        // setting this app has: it configures none of them.
        "workspace/configuration" => {
            let count = params
                .and_then(|p| p.get("items"))
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            Ok(Value::Array(vec![Value::Null; count]))
        }
        // Accepted with no opinion. We register nothing dynamically, show no
        // message boxes, and track no progress tokens — but each of these still
        // blocks the server until it is answered.
        "client/registerCapability"
        | "client/unregisterCapability"
        | "window/showMessageRequest"
        | "window/workDoneProgress/create" => Ok(Value::Null),
        // Refused, deliberately and always. This app never lets a language
        // server write to the user's files behind their back; a refusal is a
        // reply, and the server is told rather than left waiting.
        "workspace/applyEdit" => Ok(json!({ "applied": false })),
        _ => Err(RpcError {
            code: METHOD_NOT_FOUND,
            message: format!("{method} is not implemented by this client"),
        }),
    }
}

// ---------------------------------------------------------------------------
// Frames we send
// ---------------------------------------------------------------------------

// `Outgoing::request`/`notification` pin their method to `&'static str`, which
// is right for a client with a fixed vocabulary and wrong for a transport that
// forwards whatever it is handed. Replies still go through `Outgoing`, since
// those have no method at all.

fn request_frame(id: &RequestId, method: &str, params: Value) -> Vec<u8> {
    frame(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
}

fn notification_frame(method: &str, params: Value) -> Vec<u8> {
    // No `id` key at all: `"id": null` is *present*, and a server that sees it
    // waits for a reply to a message that was never a request.
    frame(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
}

fn cancel_frame(id: &RequestId) -> Vec<u8> {
    notification_frame("$/cancelRequest", json!({ "id": id }))
}

fn frame(message: &Value) -> Vec<u8> {
    // A `serde_json::Value` built from `json!` cannot fail to serialise; the
    // fallback keeps this total rather than panicking on the reader task.
    framing::encode(&serde_json::to_vec(message).unwrap_or_else(|_| b"{}".to_vec()))
}

impl Transport {
    fn send(&self, bytes: Vec<u8>) -> Result<(), RequestFailure> {
        let guard = self.out.lock().unwrap();
        let queued = match guard.as_ref() {
            Some(tx) => tx.send(bytes).is_ok(),
            None => false,
        };
        drop(guard);
        if queued {
            return Ok(());
        }
        Err(RequestFailure::ServerDied(
            self.death.borrow().clone().unwrap_or_else(|| Death {
                code: None,
                reason: DeathReason::WriteFailed("the server's stdin is closed".into()),
                stderr_tail: tail_lines(&self.stderr),
            }),
        ))
    }
}

// ---------------------------------------------------------------------------
// The four tasks
// ---------------------------------------------------------------------------

/// Generic over the sink purely so a broken one can be handed to it in a unit
/// test. A write failure is a death whose process is very possibly still alive,
/// which is the one death that has to kill something, and provoking it from a
/// real child is not something a script can be made to do reliably.
#[allow(clippy::too_many_arguments)]
async fn write_loop<W: AsyncWrite + Unpin>(
    mut stdin: W,
    mut queue: mpsc::UnboundedReceiver<Vec<u8>>,
    death: watch::Sender<Option<Death>>,
    pending: Arc<Mutex<Pending>>,
    stderr: StderrTail,
    pid: Option<u32>,
) {
    while let Some(bytes) = queue.recv().await {
        let failure = match stdin.write_all(&bytes).await {
            Err(error) => Some(error),
            Ok(()) => stdin.flush().await.err(),
        };
        if let Some(error) = failure {
            publish_death(
                &death,
                &pending,
                &stderr,
                None,
                DeathReason::WriteFailed(error.to_string()),
            );
            // The one death reason that does *not* imply the process has ended.
            // A server whose stdin we cannot reach can never be told to exit, so
            // leaving it would orphan it — with Roslyn's `BuildHost` children —
            // for the life of the session, and `Drop` will not do it for us: it
            // only kills while no death has been published.
            if let Some(pid) = pid {
                tracing::warn!(pid, "cannot write to the language server; killing the tree");
                kill_tree(pid);
            }
            return;
        }
    }
    // The queue closed: `shutdown` took the sender, and the reader only ever
    // held a weak one. Dropping `stdin` here is the EOF that step is asking for.
}

/// Generic over the source for the same reason [`write_loop`] is: a read
/// *error* on a live process is a path no scripted server can produce on demand.
#[allow(clippy::too_many_arguments)]
async fn read_loop<R: AsyncRead + Unpin>(
    mut stdout: R,
    out: mpsc::WeakUnboundedSender<Vec<u8>>,
    pending: Arc<Mutex<Pending>>,
    notifications: broadcast::Sender<Notification>,
    signal: Arc<SignalSink>,
    death: watch::Sender<Option<Death>>,
    stderr: StderrTail,
    pid: Option<u32>,
) {
    let mut decoder = Decoder::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = match stdout.read(&mut buf).await {
            Ok(0) => break, // EOF; the reaper reports the exit.
            Ok(n) => n,
            Err(error) => {
                // Not the same as EOF. EOF means the process is ending and the
                // reaper is about to say so; an error means we can no longer
                // read a stream whose writer may be perfectly healthy. Sharing
                // the EOF path published no death at all, so `death()` stayed
                // `None`, the restart policy was never consulted, and every
                // later request waited out its full deadline and reported
                // `TimedOut` — "the server is slow" about a server we had gone
                // deaf to.
                publish_death(
                    &death,
                    &pending,
                    &stderr,
                    None,
                    DeathReason::ReadFailed(error.to_string()),
                );
                if let Some(pid) = pid {
                    kill_tree(pid);
                }
                return;
            }
        };
        let frames = match decoder.push(&buf[..read]) {
            Ok(frames) => frames,
            Err(error) => {
                // Fail closed. There is no way to know where the next frame
                // begins, and a resynchronisation attempt would hand one
                // request's answer to another request's waiter — a wrong
                // answer, which is far worse than a missing one.
                publish_death(
                    &death,
                    &pending,
                    &stderr,
                    None,
                    DeathReason::Framing(error.to_string()),
                );
                // The process may well still be alive and still writing. It is
                // useless to us now, and leaving it running would leak it.
                if let Some(pid) = pid {
                    kill_tree(pid);
                }
                return;
            }
        };
        for body in frames {
            dispatch(&body, &out, &pending, &notifications, &signal);
        }
    }
}

fn dispatch(
    body: &[u8],
    out: &mpsc::WeakUnboundedSender<Vec<u8>>,
    pending: &Arc<Mutex<Pending>>,
    notifications: &broadcast::Sender<Notification>,
    signal: &Arc<SignalSink>,
) {
    match classify(body) {
        Ok(Incoming::Response { id, result, error }) => {
            let waiter = pending.lock().unwrap().remove(&id);
            match waiter {
                Some(waiter) => {
                    let answer = match error {
                        Some(e) => Err(RequestFailure::ServerError {
                            code: e.code,
                            message: e.message,
                        }),
                        // A response with neither is refused by `classify`, so
                        // an absent result here is a genuine `"result": null`.
                        None => Ok(result.unwrap_or(Value::Null)),
                    };
                    let _ = waiter.tx.send(answer);
                }
                // Not an error. A cancelled request's late answer, or a second
                // answer to an id already resolved, is ordinary traffic — and
                // treating it as a protocol failure would tear down a healthy
                // server over a message that cost us nothing.
                None => tracing::debug!(?id, "response for an id nobody is waiting for; dropped"),
            }
        }
        Ok(Incoming::Notification { method, params }) => {
            let notification = Notification { method, params };
            // Recorded *before* it is published, so the sticky state can never
            // be behind the live stream: an observer that saw the notification
            // arrive and then read the signal must find it there.
            signal.record(&notification);
            // `send` fails only when nobody is listening, which is normal.
            let _ = notifications.send(notification);
        }
        Ok(Incoming::Request { id, method, params }) => {
            // Must be answered, always. See `answer_for`.
            let reply = match answer_for(&method, params.as_ref()) {
                Ok(result) => Outgoing::reply(id, result),
                Err(error) => Outgoing::error_reply(id, error.code, &error.message),
            };
            let bytes = serde_json::to_vec(&reply).unwrap_or_else(|_| b"{}".to_vec());
            // `Weak` on purpose: the reader must not be what keeps the writer's
            // `ChildStdin` open, or `shutdown` can never deliver EOF. Once the
            // transport has closed stdin there is nothing to answer with, and
            // that is the correct state rather than a failure.
            if let Some(out) = out.upgrade() {
                let _ = out.send(framing::encode(&bytes));
            }
        }
        Err(error) => {
            // The frame boundaries are still sound, so this costs one message
            // rather than the session. But if the message was *somebody's
            // answer*, dropping it silently turns "the server said something we
            // could not read" into "the server did not answer" — a different one
            // of the five failures, and the only one that reads as slowness. So
            // the id is recovered by hand and its waiter is failed by name.
            match recover_id(body) {
                Some(id) => match pending.lock().unwrap().remove(&id) {
                    Some(waiter) => {
                        let _ = waiter.tx.send(Err(RequestFailure::Malformed {
                            method: waiter.method,
                            detail: error.to_string(),
                        }));
                    }
                    None => {
                        tracing::warn!(%error, ?id, "unreadable message; nobody was waiting for it")
                    }
                },
                None => {
                    tracing::warn!(%error, "unreadable message from the language server; dropped")
                }
            }
        }
    }
}

/// The `id` of a frame [`classify`] refused, if it has one.
///
/// Deliberately narrow: only an `id` that is a number or a string, and only on
/// something that parsed as a JSON object at all. Anything looser would risk
/// resolving a waiter with a *different* request's failure, which is the
/// misdelivery this module is arranged to prevent — and an unrecoverable id is
/// still a legitimate drop.
fn recover_id(body: &[u8]) -> Option<RequestId> {
    let value: Value = serde_json::from_slice(body).ok()?;
    match value.get("id")? {
        Value::Number(n) => n.as_i64().map(RequestId::Num),
        Value::String(s) => Some(RequestId::Str(s.clone())),
        _ => None,
    }
}

async fn stderr_loop(mut stderr: tokio::process::ChildStderr, tail: StderrTail) {
    let mut chunker = Utf8Chunker::new();
    let mut splitter = LineSplitter::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = match stderr.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for line in splitter.push(&chunker.push(&buf[..read])) {
            remember_stderr(&tail, line);
        }
    }
    let rest = chunker.finish();
    if !rest.is_empty() {
        for line in splitter.push(&rest) {
            remember_stderr(&tail, line);
        }
    }
    if let Some(last) = splitter.finish() {
        remember_stderr(&tail, last);
    }
}

async fn reap(
    mut child: tokio::process::Child,
    death: watch::Sender<Option<Death>>,
    pending: Arc<Mutex<Pending>>,
    stderr: StderrTail,
) {
    let code = match child.wait().await {
        Ok(status) => status.code(),
        Err(_) => None,
    };
    publish_death(&death, &pending, &stderr, code, DeathReason::Exited);
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn remember_stderr(tail: &StderrTail, line: String) {
    tracing::debug!(target: "cb_core::lsp", %line, "language server stderr");
    let mut guard = tail.lock().unwrap();
    if guard.len() == STDERR_TAIL_LINES {
        guard.pop_front();
    }
    guard.push_back(line);
}

fn tail_lines(tail: &StderrTail) -> Vec<String> {
    tail.lock().unwrap().iter().cloned().collect()
}

/// Record the death and fail every waiter.
///
/// The first death published wins. Whatever noticed first is the true cause:
/// a framing failure followed by an exit is a framing failure, and overwriting
/// it with "exited" would throw away the only useful part of the report.
fn publish_death(
    death: &watch::Sender<Option<Death>>,
    pending: &Arc<Mutex<Pending>>,
    stderr: &StderrTail,
    code: Option<i32>,
    reason: DeathReason,
) {
    let candidate = Death {
        code,
        reason,
        stderr_tail: tail_lines(stderr),
    };
    death.send_if_modified(|slot| match slot {
        Some(_) => false,
        None => {
            *slot = Some(candidate);
            true
        }
    });

    let published = death
        .borrow()
        .clone()
        .expect("a death has just been published");

    // Every pending request, or the caller waits forever for a reply that
    // cannot come. This is the whole reason the reaper exists.
    let waiters: Vec<_> = pending.lock().unwrap().drain().collect();
    for (id, waiter) in waiters {
        tracing::debug!(?id, method = %waiter.method, "failing a pending request: the server is gone");
        let _ = waiter
            .tx
            .send(Err(RequestFailure::ServerDied(published.clone())));
    }
}

/// Resolve when the server dies. Resolves immediately if it already has.
async fn wait_for_death(rx: &mut watch::Receiver<Option<Death>>) -> Death {
    loop {
        // Scoped so the `Ref` is dropped before the await below: holding it
        // across a suspension point would make this future non-`Send`.
        let current = rx.borrow_and_update().clone();
        if let Some(death) = current {
            return death;
        }
        if rx.changed().await.is_err() {
            // The sender is owned by the `Transport` this was subscribed from,
            // so this is unreachable while the caller holds it. Never resolving
            // is the right answer anyway: this future exists to lose a `select!`
            // race, and returning a fabricated death would report one.
            std::future::pending::<()>().await;
        }
    }
}

// ---------------------------------------------------------------------------
// Restart policy
// ---------------------------------------------------------------------------

/// What to do about a server that has just died.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartDecision {
    Restart,
    /// Permanent. Carries the stderr tail, because by the time a user is told
    /// a language feature is off, the server's own last words are the only
    /// thing that can explain why.
    Failed {
        stderr_tail: Vec<String>,
    },
}

/// At most one automatic restart per [`RESTART_WINDOW`], then permanent failure.
///
/// A server that cannot start will not start on the fifth attempt either, and
/// an endless respawn loop burns the user's CPU while hiding the reason. Time
/// is injected rather than read so this is testable without waiting a minute.
#[derive(Debug, Clone, Default)]
pub struct RestartPolicy {
    window: Option<Duration>,
    last_restart: Option<Instant>,
    failed: bool,
}

impl RestartPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_window(window: Duration) -> Self {
        Self {
            window: Some(window),
            ..Self::default()
        }
    }

    pub fn is_failed(&self) -> bool {
        self.failed
    }

    pub fn on_death(&mut self, now: Instant, death: &Death) -> RestartDecision {
        let window = self.window.unwrap_or(RESTART_WINDOW);
        let too_soon = self
            .last_restart
            .is_some_and(|last| now.duration_since(last) < window);

        if self.failed || too_soon {
            self.failed = true;
            return RestartDecision::Failed {
                stderr_tail: death.stderr_tail.clone(),
            };
        }
        self.last_restart = Some(now);
        RestartDecision::Restart
    }
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod transport_tests;
