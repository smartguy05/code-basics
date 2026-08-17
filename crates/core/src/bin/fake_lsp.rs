//! A scripted stand-in for a language server, used only by the transport tests.
//!
//! # Why this exists
//!
//! The transport's *failure* modes are the ones worth testing — a server that
//! dies mid-request, one that never answers, one that emits a broken frame, one
//! that asks the client a question and blocks until it is answered — and none of
//! them can be provoked reliably from a real language server. Three of the four
//! servers this feature targets are not installed on the development machine at
//! all, so a scripted fake is not a convenience here: it is the only way those
//! paths are covered.
//!
//! It speaks the wire with the crate's own [`cb_core::lsp::framing`] and
//! [`cb_core::lsp::jsonrpc`] on purpose. Using the shipped encoder against the
//! shipped decoder means a framing bug cannot hide behind a hand-rolled fake that
//! happens to agree with it.
//!
//! # Running it
//!
//! `CB_FAKE_LSP_SCRIPT` names a JSON file; the process then speaks framed
//! JSON-RPC on stdin/stdout. With no script it exits 2 rather than guessing a
//! behaviour. `CB_FAKE_LSP_TIMEOUT_MS` overrides the hard self-timeout (60 s by
//! default) after which it exits no matter what — a fake that can hang would
//! leave stray processes on a developer's machine every time a test is killed.
//!
//! ```json
//! {
//!   "capabilities": { "referencesProvider": true, "textDocumentSync": 2 },
//!   "steps": [
//!     { "on": "textDocument/references", "delayMs": 50, "reply": [] },
//!     { "on": "textDocument/definition", "misbehave": "exitBeforeReply" },
//!     { "on": "*", "reply": null }
//!   ],
//!   "emitOnInitialized": [ { "method": "window/logMessage", "params": {} } ],
//!   "serverRequests": [ { "method": "workspace/configuration", "params": {} } ]
//! }
//! ```
//!
//! `capabilities` is returned verbatim inside the `initialize` result, so a test
//! can hand the client any capability set at all — including one with no
//! `positionEncoding` key, which is the real Roslyn shape and means UTF-16.
//! Omitting `capabilities` entirely yields an `initialize` result with **no**
//! `capabilities` key, which is a different thing again.
//!
//! # Step matching
//!
//! An exact `on` match wins; otherwise a step with `on: "*"` applies. The
//! catch-all deliberately does **not** cover `initialize`, `shutdown` or `exit`:
//! it is written to describe feature requests, and letting it swallow the
//! handshake would leave a script unable to start the session it wanted to test.
//! Those three still honour an exact-match step, so their misbehaviours are
//! expressible.
//!
//! Unknown methods carrying an id get a `METHOD_NOT_FOUND` error; unknown
//! notifications are ignored, as the protocol requires.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cb_core::lsp::framing::{self, Decoder, MAX_FRAME_BYTES};
use cb_core::lsp::jsonrpc::{classify, Incoming, Outgoing, RequestId, METHOD_NOT_FOUND};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};

/// JSON-RPC's code for a request whose parameters could not be used.
const INVALID_PARAMS: i64 = -32602;

/// How long the process lives before killing itself regardless of state.
const SELF_TIMEOUT: Duration = Duration::from_secs(60);

/// Exit code for the self-timeout, so a test can tell it from a scripted exit.
const EXIT_SELF_TIMEOUT: i32 = 3;

// ---------------------------------------------------------------------------
// The script
// ---------------------------------------------------------------------------

/// `deny_unknown_fields` throughout: a mistyped key in a fake's script is a
/// silent behaviour change, and debugging one costs far more than the strictness.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Script {
    /// Returned verbatim as `initialize`'s `capabilities`. `None` omits the key.
    capabilities: Option<Value>,
    #[serde(default)]
    steps: Vec<Step>,
    /// Notifications pushed once the client sends `initialized`.
    #[serde(default)]
    emit_on_initialized: Vec<Message>,
    /// Server→client requests issued once the client sends `initialized`, the
    /// way Roslyn issues `workspace/configuration` during start-up. Answers are
    /// recorded but not waited on; the *blocking* form is the
    /// `serverRequestThenReply` misbehaviour.
    #[serde(default)]
    server_requests: Vec<Message>,
    /// Where to append every notification this process receives, one JSON object
    /// (`{"method": …, "params": …}`) per line.
    ///
    /// A file rather than a reply because notifications have no reply, which is
    /// precisely why they were invisible to every other test here: the document
    /// sync is the one thing this subsystem does that a server never answers, so
    /// nothing could see what was on the wire. `echoArgv` and `echoAnswer` exist
    /// for the same reason and take the same shape.
    ///
    /// Appended synchronously from `dispatch`, before the handling task is
    /// spawned, so the file's order is the wire's order.
    #[serde(default)]
    notification_journal: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Step {
    /// A method name, or `"*"` for the catch-all.
    on: String,
    #[serde(default)]
    delay_ms: u64,
    /// The `result` value. Absent means `null`, which is a legal result.
    #[serde(default)]
    reply: Option<Value>,
    #[serde(default)]
    misbehave: Option<Misbehave>,
    /// `spawnChildAndLeak`: where to record the grandchild's pid, in addition to
    /// the `window/logMessage` it always sends.
    #[serde(default)]
    pid_file: Option<PathBuf>,
    /// `stderrFlood` / `notificationFlood`: how many. Defaults to 5000 and 3000.
    #[serde(default)]
    lines: Option<usize>,
    /// `notificationFlood`: the one notification emitted inside the flood.
    ///
    /// Separate from `reply` and from `emitOnInitialized` because the *order* is
    /// the whole point: a one-shot message has to be surrounded by more chatter
    /// than any subscriber's buffer holds, which is how a real server sends
    /// `workspace/projectInitializationComplete` during a solution load.
    #[serde(default)]
    then: Option<Message>,
    /// `notificationFlood`: how many notifications precede `then`. Defaults to
    /// all of them, i.e. `then` goes last.
    ///
    /// Worth setting when a test needs the loss to be *certain* rather than
    /// likely: with enough notifications still to come after it, the one-shot
    /// message is guaranteed to have been overwritten in a bounded channel by
    /// the time anybody reads, whatever order the tasks happened to run in.
    #[serde(default)]
    then_after: Option<usize>,
    /// `echoArgv`: where to write this process's command line, one argument per
    /// line. A file rather than a reply because the command line is a list of
    /// strings and every reply shape these tests use is a list of locations.
    #[serde(default)]
    argv_file: Option<PathBuf>,
    /// `unknownIdReply`: the id nobody asked about.
    #[serde(default)]
    unknown_id: Option<RequestId>,
    /// `serverRequestThenReply`: what to ask the client before answering.
    #[serde(default)]
    request: Option<Message>,
    /// `serverRequestThenReply`: reply with the client's own answer to
    /// `request`, as `{"result": …, "error": …}`, instead of `reply`.
    ///
    /// This is the only way a test can see what the client actually put on the
    /// wire. Matching on the response *id* alone — which is all `resolve` needs
    /// — would let a client that answered `null` to a three-item
    /// `workspace/configuration` pass, and that reply is a real Roslyn start-up
    /// hang that looks exactly like a slow project load.
    #[serde(default)]
    echo_answer: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Message {
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Misbehave {
    /// Accept the request, then die without answering.
    ExitBeforeReply,
    /// Accept the request and never answer it.
    Never,
    /// Write bytes that are not a frame at all.
    GarbageFrame,
    /// Promise more body than is sent, then go quiet.
    TruncatedContentLength,
    /// Hold this reply until the *next* request has been answered.
    ReplyOutOfOrder,
    /// Answer the same id twice.
    DuplicateReply,
    /// Answer an id nobody asked about, then answer this one properly.
    UnknownIdReply,
    /// Declare a body past the frame cap.
    HugeReply,
    /// Thousands of stderr lines, then answer normally.
    StderrFlood,
    /// Thousands of notifications, then the one named by `then`. Reproduces the
    /// real Roslyn start-up: a chatty load followed by a single
    /// `workspace/projectInitializationComplete`, far enough behind the chatter
    /// that a `broadcast` subscriber has lost its place by the time it arrives.
    NotificationFlood,
    /// Spawn a grandchild that outlives this process, then answer normally.
    SpawnChildAndLeak,
    /// Answer with whatever URI the request carried.
    EchoUri,
    /// Record this process's own command line to `argvFile`, then answer
    /// normally. The only way a test can see what the client actually spawned —
    /// and an argument a doc comment promises but nobody appends is invisible
    /// from every other angle.
    EchoArgv,
    /// Ask the client something and block until it answers. Reproduces the real
    /// Roslyn start-up stall: a client that ignores server→client requests hangs
    /// here, and the test must time out rather than pass.
    ServerRequestThenReply,
    /// Answer with a frame that is framed correctly and classifies as nothing:
    /// an `id`, an `error` whose `code` is a string, and no `result`. Real
    /// servers outside Roslyn do emit non-integer codes, and the id in it
    /// belongs to a caller who is blocked.
    MalformedReply,
}

// ---------------------------------------------------------------------------
// The server
// ---------------------------------------------------------------------------

struct Server {
    script: Script,
    out: mpsc::UnboundedSender<Vec<u8>>,
    /// Replies deferred by `replyOutOfOrder`, released by the next answer — or,
    /// if no next answer ever comes, by the sweeper in `main`.
    held: Mutex<Vec<Vec<u8>>>,
    /// Whether any answer has gone out yet. The sweeper waits for one, so a
    /// held reply is still *held* rather than merely delayed.
    answered: AtomicBool,
    /// Server→client requests still waiting for an answer, keyed by the id's
    /// debug form so `1` and `"1"` stay distinct the way `RequestId` intends.
    /// The payload is the client's whole answer, so a test can assert on it.
    waiting: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    next_id: AtomicI64,
}

impl Server {
    fn step_for(&self, method: &str) -> Option<&Step> {
        if let Some(exact) = self.script.steps.iter().find(|s| s.on == method) {
            return Some(exact);
        }
        if matches!(method, "initialize" | "shutdown" | "exit") {
            return None;
        }
        self.script.steps.iter().find(|s| s.on == "*")
    }

    /// Append one received notification to `notificationJournal`, if the script
    /// asked for one.
    ///
    /// Errors are swallowed on purpose: the journal is a test's window onto the
    /// wire, not a behaviour, and a fake that died because a reader had the file
    /// open would fail the test it was meant to explain.
    fn journal(&self, method: &str, params: Option<&Value>) {
        let Some(path) = &self.script.notification_journal else {
            return;
        };
        let line = json!({"method": method, "params": params});
        let Ok(mut line) = serde_json::to_vec(&line) else {
            return;
        };
        line.push(b'\n');
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = file.write_all(&line);
        }
    }

    /// Queue raw bytes for stdout exactly as given — the escape hatch the
    /// framing misbehaviours need.
    fn write_raw(&self, bytes: Vec<u8>) {
        let _ = self.out.send(bytes);
    }

    fn write_message(&self, message: &Value) {
        let body = serde_json::to_vec(message).expect("a scripted message must serialise");
        self.write_raw(framing::encode(&body));
    }

    /// Send an answer, then release anything `replyOutOfOrder` is holding.
    fn answer(&self, message: &Value) {
        self.write_message(message);
        self.answered.store(true, Ordering::Relaxed);
        self.release_held();
    }

    /// Write out every deferred reply.
    ///
    /// Called from [`Server::answer`] and from the sweeper. Both are needed: the
    /// push into `held` happens on the request's own task, so nothing orders it
    /// before the answer that is supposed to release it, and a held reply that
    /// lost that race used to be lost outright — a fake-side flake that shows up
    /// as the transport timing out.
    fn release_held(&self) {
        let held: Vec<Vec<u8>> = std::mem::take(&mut self.held.lock().unwrap());
        for frame in held {
            self.write_raw(frame);
        }
    }

    fn reply_value(id: RequestId, result: Value) -> Value {
        serde_json::to_value(Outgoing::reply(id, result)).expect("a reply must serialise")
    }

    fn error_value(id: RequestId, code: i64, message: &str) -> Value {
        serde_json::to_value(Outgoing::error_reply(id, code, message))
            .expect("an error reply must serialise")
    }

    /// `Outgoing` pins its method to `&'static str` — right for the client,
    /// wrong for a fake whose methods come out of a JSON file — so the two
    /// script-driven shapes are built by hand.
    fn notification_value(method: &str, params: Option<Value>) -> Value {
        json!({ "jsonrpc": "2.0", "method": method, "params": params.unwrap_or(Value::Null) })
    }

    fn request_value(id: i64, method: &str, params: Option<Value>) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params.unwrap_or(Value::Null),
        })
    }

    /// Ask the client something. The receiver carries the client's answer.
    fn ask(&self, message: &Message) -> oneshot::Receiver<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.waiting
            .lock()
            .unwrap()
            .insert(format!("{:?}", RequestId::Num(id)), tx);
        self.write_message(&Self::request_value(
            id,
            &message.method,
            message.params.clone(),
        ));
        rx
    }

    /// Route a client's answer, payload and all, to whoever is waiting for it.
    fn resolve(&self, id: &RequestId, answer: Value) {
        if let Some(tx) = self.waiting.lock().unwrap().remove(&format!("{id:?}")) {
            let _ = tx.send(answer);
        } else {
            eprintln!("[fake-lsp] response to an id nobody is waiting for: {id:?}");
        }
    }
}

// ---------------------------------------------------------------------------
// Handling one incoming message
// ---------------------------------------------------------------------------

/// A request and a notification differ here only in whether there is an id to
/// answer; every misbehaviour that writes bytes rather than a reply applies to
/// both, so they share one handler.
async fn handle(server: Arc<Server>, id: Option<RequestId>, method: String, params: Option<Value>) {
    let step = server.step_for(&method);
    let (delay, misbehave) = match step {
        Some(step) => (step.delay_ms, step.misbehave),
        None => (0, None),
    };
    let mut echoed: Option<Value> = None;

    if delay > 0 {
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }

    // The default result for a method with no step. `initialize` gets the
    // scripted capabilities; the handshake's other half answers null; anything
    // else we were not told about is honestly a method we do not implement.
    let scripted_result = step.and_then(|s| s.reply.clone());
    let default_result = match method.as_str() {
        "initialize" => Some(match &server.script.capabilities {
            Some(capabilities) => json!({ "capabilities": capabilities }),
            None => json!({}),
        }),
        "shutdown" => Some(Value::Null),
        _ if step.is_some() => Some(Value::Null),
        _ => None,
    };
    let result = scripted_result.or(default_result);

    match misbehave {
        Some(Misbehave::ExitBeforeReply) => {
            // A beat for the writer task to drain anything already queued, so
            // this reads as "died before answering" and not "died mid-frame".
            tokio::time::sleep(Duration::from_millis(20)).await;
            std::process::exit(0);
        }
        Some(Misbehave::Never) => {
            eprintln!("[fake-lsp] {method}: scripted to never answer");
            // `exit` scripted to never answer means the process stays up, which
            // is the case where the client has to kill the tree itself.
            return;
        }
        Some(Misbehave::GarbageFrame) => {
            server.write_raw(b"<<< this is not a frame >>>\r\n\r\n{}".to_vec());
            return;
        }
        Some(Misbehave::TruncatedContentLength) => {
            let mut bytes = b"Content-Length: 4096\r\n\r\n".to_vec();
            bytes.extend_from_slice(br#"{"jsonrpc":"2.0","result":"#);
            server.write_raw(bytes);
            // Deliberate silence afterwards: the client must see a stall, not an
            // EOF, so the process stays alive until its self-timeout.
            return;
        }
        Some(Misbehave::HugeReply) => {
            // The header alone is the violation the client must refuse, so the
            // body is only started. Writing all 32 MiB would deadlock against a
            // client that has (correctly) already stopped reading.
            let mut bytes = format!("Content-Length: {}\r\n\r\n", MAX_FRAME_BYTES + 1).into_bytes();
            bytes.extend_from_slice(&[b'x'; 64 * 1024]);
            server.write_raw(bytes);
            return;
        }
        Some(Misbehave::StderrFlood) => {
            let lines = step.and_then(|s| s.lines).unwrap_or(5000);
            for n in 0..lines {
                eprintln!("[fake-lsp] flood line {n} of {lines}");
            }
        }
        Some(Misbehave::NotificationFlood) => {
            let count = step.and_then(|s| s.lines).unwrap_or(3000);
            let then = step.and_then(|s| s.then.clone());
            let at = step.and_then(|s| s.then_after).unwrap_or(count).min(count);
            for n in 0..count {
                if n == at {
                    // Once, and only ever once — which is the whole property
                    // being reproduced.
                    if let Some(then) = then.as_ref() {
                        server.write_message(&Server::notification_value(
                            &then.method,
                            then.params.clone(),
                        ));
                    }
                }
                server.write_message(&Server::notification_value(
                    "window/logMessage",
                    Some(json!({ "type": 4, "message": format!("flood {n} of {count}") })),
                ));
            }
            if at >= count {
                if let Some(then) = then {
                    server.write_message(&Server::notification_value(&then.method, then.params));
                }
            }
        }
        Some(Misbehave::SpawnChildAndLeak) => {
            spawn_and_leak(&server, step.and_then(|s| s.pid_file.as_deref()));
        }
        Some(Misbehave::ServerRequestThenReply) => {
            let ask = step.and_then(|s| s.request.clone()).unwrap_or(Message {
                method: "workspace/configuration".to_string(),
                params: Some(json!({ "items": [{ "section": "csharp" }] })),
            });
            // No timeout on purpose. If the client never answers, this reply
            // never comes and the test times out — which is exactly the real
            // Roslyn stall this reproduces.
            let answer = server.ask(&ask).await;
            if step.is_some_and(|s| s.echo_answer) {
                echoed = Some(answer.unwrap_or(Value::Null));
            }
        }
        Some(Misbehave::MalformedReply) => {
            // Only meaningful for a request: the whole point is the blocked
            // caller whose id is in the frame.
            if let Some(id) = id.as_ref() {
                server.write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    // A string code. Legal JSON, framed correctly, and not a
                    // response any reader can classify.
                    "error": { "code": "E_FAIL", "message": "boom" },
                }));
            }
            return;
        }
        Some(Misbehave::EchoArgv) => {
            if let Some(path) = step.and_then(|s| s.argv_file.as_deref()) {
                let argv: Vec<String> = std::env::args().collect();
                if let Err(error) = std::fs::write(path, argv.join("\n")) {
                    eprintln!("[fake-lsp] could not write the argv file: {error}");
                }
            }
        }
        Some(Misbehave::EchoUri)
        | Some(Misbehave::ReplyOutOfOrder)
        | Some(Misbehave::DuplicateReply)
        | Some(Misbehave::UnknownIdReply)
        | None => {}
    }

    // Everything past here needs an id to answer. A notification stops here.
    let Some(id) = id else {
        return;
    };

    let message = match misbehave {
        _ if echoed.is_some() => Server::reply_value(id.clone(), echoed.unwrap_or(Value::Null)),
        Some(Misbehave::EchoUri) => match uri_of(params.as_ref()) {
            Some(uri) => Server::reply_value(id.clone(), json!([location(&uri)])),
            None => Server::error_value(id.clone(), INVALID_PARAMS, "no uri in params"),
        },
        _ => match result {
            Some(result) => Server::reply_value(id.clone(), result),
            None => Server::error_value(id.clone(), METHOD_NOT_FOUND, &format!("no {method} here")),
        },
    };

    match misbehave {
        Some(Misbehave::ReplyOutOfOrder) => {
            let body = serde_json::to_vec(&message).expect("a reply must serialise");
            server.held.lock().unwrap().push(framing::encode(&body));
        }
        Some(Misbehave::DuplicateReply) => {
            server.answer(&message);
            server.answer(&message);
        }
        Some(Misbehave::UnknownIdReply) => {
            let stray = step
                .and_then(|s| s.unknown_id.clone())
                .unwrap_or(RequestId::Num(987_654_321));
            server.answer(&Server::reply_value(stray, json!("nobody asked")));
            // The real answer still follows, so a test can prove the stray was
            // dropped *and* that the genuine reply still resolves its waiter.
            server.answer(&message);
        }
        _ => server.answer(&message),
    }
}

/// `textDocument/*` carries its uri under `textDocument`; a couple of methods
/// carry it flat. Both spellings are read, and nothing is invented if neither
/// is there.
fn uri_of(params: Option<&Value>) -> Option<String> {
    let params = params?;
    params
        .pointer("/textDocument/uri")
        .or_else(|| params.pointer("/uri"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn location(uri: &str) -> Value {
    json!({
        "uri": uri,
        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
    })
}

/// Spawn a grandchild that outlives this process unless the client kills the
/// whole tree. `sh` on PATH is already an assumption of the `process::` tests.
fn spawn_and_leak(server: &Server, pid_file: Option<&std::path::Path>) {
    match std::process::Command::new("sh")
        .arg("-c")
        .arg("sleep 300")
        .spawn()
    {
        Ok(child) => {
            let pid = child.id();
            // Deliberately not waited on and deliberately not killed.
            std::mem::forget(child);
            if let Some(path) = pid_file {
                if let Err(error) = std::fs::write(path, pid.to_string()) {
                    eprintln!("[fake-lsp] could not write the pid file: {error}");
                }
            }
            server.write_message(&Server::notification_value(
                "window/logMessage",
                Some(json!({ "type": 3, "message": format!("grandchild pid {pid}") })),
            ));
        }
        Err(error) => {
            eprintln!("[fake-lsp] could not spawn the grandchild: {error}");
        }
    }
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let Some(path) = std::env::var_os("CB_FAKE_LSP_SCRIPT") else {
        eprintln!("[fake-lsp] CB_FAKE_LSP_SCRIPT is not set; refusing to guess a behaviour");
        std::process::exit(2);
    };
    let script: Script = match std::fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
    {
        Ok(script) => script,
        Err(detail) => {
            eprintln!("[fake-lsp] unusable script {path:?}: {detail}");
            std::process::exit(2);
        }
    };

    let timeout = std::env::var("CB_FAKE_LSP_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(SELF_TIMEOUT, Duration::from_millis);
    tokio::spawn(async move {
        tokio::time::sleep(timeout).await;
        eprintln!("[fake-lsp] self-timeout after {timeout:?}; exiting");
        std::process::exit(EXIT_SELF_TIMEOUT);
    });

    let (out, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(bytes) = rx.recv().await {
            if stdout.write_all(&bytes).await.is_err() || stdout.flush().await.is_err() {
                return;
            }
        }
    });

    let server = Arc::new(Server {
        script,
        out,
        held: Mutex::new(Vec::new()),
        answered: AtomicBool::new(false),
        waiting: Mutex::new(HashMap::new()),
        next_id: AtomicI64::new(1),
    });

    // The held-reply sweeper. `replyOutOfOrder` pushes its frame from the
    // request's own task and `answer` flushes from another, so nothing orders
    // the two: a held reply whose flush ran first would never be written at
    // all, and the transport would be blamed for the timeout.
    {
        let server = Arc::clone(&server);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
                if server.answered.load(Ordering::Relaxed) {
                    server.release_held();
                }
            }
        });
    }

    let mut stdin = tokio::io::stdin();
    let mut decoder = Decoder::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = match stdin.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let frames = match decoder.push(&buf[..read]) {
            Ok(frames) => frames,
            Err(error) => {
                eprintln!("[fake-lsp] the client's stream is unframed: {error}");
                break;
            }
        };
        for frame in frames {
            dispatch(&server, &frame);
        }
    }
}

fn dispatch(server: &Arc<Server>, frame: &[u8]) {
    match classify(frame) {
        // The payload, not just the id. `answer_for`'s *value* is the thing a
        // real server acts on, and nothing else in these tests can see it.
        Ok(Incoming::Response { id, result, error }) => server.resolve(
            &id,
            json!({
                "result": result,
                "error": error.map(|e| json!({ "code": e.code, "message": e.message })),
            }),
        ),
        Ok(Incoming::Request { id, method, params }) => {
            // One task per request, so a delayed or blocking step cannot stop
            // stdin being read — which is the only way `serverRequestThenReply`
            // can ever see its answer.
            tokio::spawn(handle(Arc::clone(server), Some(id), method, params));
        }
        Ok(Incoming::Notification { method, params }) => {
            server.journal(&method, params.as_ref());
            let handled = tokio::spawn(handle(
                Arc::clone(server),
                None,
                method.clone(),
                params.clone(),
            ));
            match method.as_str() {
                "initialized" => {
                    let server = Arc::clone(server);
                    tokio::spawn(async move {
                        for message in &server.script.emit_on_initialized {
                            server.write_message(&Server::notification_value(
                                &message.method,
                                message.params.clone(),
                            ));
                        }
                        for message in &server.script.server_requests {
                            // The answer is recorded by `resolve`, not waited
                            // on: start-up requests do not gate anything here.
                            drop(server.ask(message));
                        }
                    });
                }
                "exit" => {
                    let server = Arc::clone(server);
                    tokio::spawn(async move {
                        // A step on `exit` gets to run first: `never` there means
                        // the process ignores `exit`, which is the case where the
                        // client has to kill the tree.
                        let _ = handled.await;
                        if server
                            .step_for("exit")
                            .and_then(|s| s.misbehave)
                            .is_some_and(|m| m == Misbehave::Never)
                        {
                            eprintln!("[fake-lsp] scripted to ignore exit");
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        std::process::exit(0);
                    });
                }
                _ => {}
            }
        }
        Err(error) => eprintln!("[fake-lsp] unreadable message: {error}"),
    }
}
