//! Every language server for one workspace, behind one actor.
//!
//! This is the only module in [`super`] that owns more than one server, and the
//! only one a Tauri command talks to. Everything below it answers a question
//! about *one* thing — one process, one conversation, one payload — and this
//! decides which of them is asked, when it is started, and what to say while it
//! is not up yet.
//!
//! # Why an actor, and not an `Arc<Mutex<..>>`
//!
//! `AppState` in `src-tauri` uses a `std::sync::Mutex`, and its workspace swap
//! clears the caches **while holding the guard** — deliberately, so that
//! check-and-act is one atomic step. A language server's shutdown is
//! asynchronous, so that guard cannot `.await` it, and a session held as shared
//! state would therefore either have to be torn down outside the guard (leaving
//! a window in which the old workspace's servers can still answer under the new
//! workspace's name) or block a runtime worker inside it.
//!
//! With an actor, teardown is a non-blocking `tx.send(Teardown)`, which is legal
//! under a `MutexGuard`, and **that send is the atomic step**: the actor
//! processes its messages in order, so nothing sent after it is ever served, and
//! the real process shutdown happens off-lock afterwards. [`LspHandle`] is a
//! cheap clone of a sender plus a shared status snapshot, and nothing else.
//!
//! The same ordering pays for a second guarantee, which is otherwise fiddly:
//! **a request is always issued behind the document changes that preceded it.**
//! `didChange` and `references` travel on the same stdin stream in the order
//! they were written, so enqueueing the change first *is* the flush — no round
//! trip, and nothing to confirm. A request aimed at a buffer the server believes
//! is two edits old resolves the caret to a different symbol and answers,
//! confidently, about something the user never asked about.
//!
//! # Starting is never awaited
//!
//! Servers start **lazily**, on the first document or request for their
//! language, and the request that triggers the start returns
//! [`Availability::Starting`] straight away. Roslyn's project load is tens of
//! seconds; a UI request that waited for it is a frozen widget, and the whole
//! point of the six-way [`Availability`] is that "not yet" can be rendered as
//! itself instead of as an empty list.
//!
//! # Where this abstains
//!
//! Every path that cannot produce a count produces the reason instead, and each
//! reason is a different one:
//!
//! * an extension no server claims → [`Availability::NotConfigured`] **naming
//!   the extension**, never silence;
//! * [`Resolution::NotFound`] → `NotConfigured` carrying `looked_for` and the
//!   hint, so the user can act;
//! * [`Resolution::Disabled`] → `NotConfigured` saying the configuration turned
//!   it off, which is a different fix from installing something;
//! * [`Resolution::Misconfigured`] → [`Availability::Failed`] naming the program
//!   *and* `.code-basics/config.json`;
//! * a server still coming up → [`Availability::Starting`]; still priming →
//!   [`Availability::Loading`]; dead → `Failed` with its last words;
//! * a capability the server never advertised → the group is empty and the
//!   `message` says which group and why. **The outcome stays
//!   [`Availability::Ready`] unless all three goto groups were refused**, because
//!   [`DefinitionResult`] carries one outcome for three lists: there is no way to
//!   spell "two of these are answers and the third is a refusal" other than the
//!   message, so a reader must surface `message` beside any empty group. When
//!   *every* group was refused the outcome becomes the **most severe** refusal
//!   (see [`worst`]) — never the first, or a dead server would be reported
//!   `Unsupported`;
//! * a count taken from a server promoted at the readiness ceiling → still
//!   `Ready` with the rows it really has, and a `message` saying the number may
//!   be low (see [`caveat_note`]). `Ready` plus a message is the only shape that
//!   can qualify a real answer without withholding it.
//!
//! None of them ever carries a `total`, because [`UsageResult::unavailable`] is
//! the only constructor they use and it is the one place that decides `total` is
//! `None`.
//!
//! # What the ordering guarantee does *not* cover
//!
//! Changes that *preceded* a request are written ahead of it, as above. The
//! reverse is not guaranteed: a request is spawned as its own task, so a
//! `Message::Change` handled a moment later can reach the server's stdin before
//! the spawned task writes its `references` frame. A caret position asked about
//! before an edit therefore resolves against text that includes it. That is
//! deliberate — the alternative is serialising every request on the actor, which
//! is what makes the widget slow — and it is survivable in a way the other order
//! is not: the question is stale by at most the keystrokes typed after asking,
//! whereas a request that overtakes the edits before it asks about a buffer the
//! server believes is two edits old.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot};

use super::client::{Client, ReadyState, RequestError, StartFailure};
use super::documents::{Documents, ServerId, SyncAction};
use super::model::{
    AnchorResult, Availability, DefinitionResult, LspStatus, ServerStatus, UsageResult,
};
use super::positions::to_lsp_line;
use super::protocol::{Location, Position};
use super::registry::{self, Language, Probe, RealProbe, Resolution, ServerSpec};
use super::results::{self, TextProvider};
use super::settings::LspConfig;
use super::transport::{Death, RestartDecision, RestartPolicy};

/// How many usage rows cross IPC at most.
///
/// The rows are capped and [`UsageResult::total`] is **not**: a cap that also
/// capped the number would be a lie, and the number is the claim a user acts on
/// when deciding whether a method is safe to change.
pub const USAGE_CAP: usize = 500;

/// How often a live server is checked for death and for a readiness change.
///
/// A poll rather than a watch because [`Client`] exposes `death()` and
/// `readiness()` as reads and hands out no channel for either; the underlying
/// `watch` receivers live on the transport and are not reachable from here. A
/// quarter second of latency on a status row is invisible, and the alternative —
/// noticing a dead server only when somebody next asks it something — would
/// leave the status surface saying `Ready` about a corpse.
const SUPERVISION_INTERVAL: Duration = Duration::from_millis(250);

/// The largest file this will read from disk to open on a server's behalf.
///
/// Mirrors [`crate::symbols::index`]'s own 1 MiB rule. A request may name a file
/// the editor never opened (from the search palette, or a jump), and telling the
/// server about it is worth a read — but not an unbounded one.
const AUTO_OPEN_MAX_BYTES: u64 = 1 << 20;

/// What every [`Availability`]-bearing answer says once the actor is gone.
const TORN_DOWN: &str = "this workspace's language servers were shut down, because the \
                         workspace was closed or replaced. Re-open the workspace to ask again.";

/// The languages a session manages. All of them are resolved when the session
/// starts and none of them is started until something asks.
const LANGUAGES: [Language; 4] = [
    Language::CSharp,
    Language::TypeScript,
    Language::Rust,
    Language::Python,
];

// ---------------------------------------------------------------------------
// The handle
// ---------------------------------------------------------------------------

/// Every server for one workspace, addressed through one channel.
///
/// Cloneable and cheap: a sender, a path, a number and a shared snapshot.
/// Dropping the last clone tears the session down exactly as
/// [`LspHandle::request_teardown`] does, so a caller that forgets cannot leak a
/// `Microsoft.CodeAnalysis.LanguageServer.exe` and its `BuildHost` children.
#[derive(Clone)]
pub struct LspHandle {
    root: PathBuf,
    generation: u64,
    tx: mpsc::UnboundedSender<Message>,
    status: SharedStatus,
}

/// The status snapshot, written only by the actor and read by every handle.
///
/// A `std::sync::Mutex` on purpose, and never held across an `.await`:
/// [`LspHandle::status`] has to be callable from inside `AppState`'s own guard.
type SharedStatus = Arc<Mutex<BTreeMap<Language, ServerStatus>>>;

impl std::fmt::Debug for LspHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspHandle")
            .field("root", &self.root)
            .field("generation", &self.generation)
            .field("live", &!self.tx.is_closed())
            .finish()
    }
}

impl LspHandle {
    /// The workspace this session is for.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Which workspace generation started it, so a caller can tell a live
    /// session from one that was superseded while a request was in flight.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// What every configured server is doing, without a round trip.
    ///
    /// Read from the shared snapshot, so this is safe to call from a synchronous
    /// context and from under a lock. Servers that resolved successfully but have
    /// not been asked for anything yet are **not** listed: there is no
    /// [`Availability`] for "found, idle", and `Starting` said about a process
    /// that has not been spawned is exactly the kind of small lie this subsystem
    /// exists to refuse. A language appears the moment it is started, or
    /// immediately and permanently if it could not be resolved at all.
    pub fn status(&self) -> LspStatus {
        let servers = match self.status.lock() {
            Ok(status) => status.values().cloned().collect(),
            // A poisoned lock means the actor panicked while updating status.
            // The last consistent snapshot is still the best answer available.
            Err(poisoned) => poisoned.into_inner().values().cloned().collect(),
        };
        LspStatus { servers }
    }

    /// Ask for teardown and return immediately.
    ///
    /// Non-blocking, so it is legal under a `std::sync::MutexGuard`, and safe to
    /// call any number of times: the second send either queues behind a teardown
    /// that has already been accepted or fails against an actor that is already
    /// gone, and both are the same outcome.
    pub fn request_teardown(&self) {
        let _ = self.tx.send(Message::Teardown);
    }

    /// The editor opened a document, or replaced its contents wholesale.
    ///
    /// `async` for symmetry with the requests and because callers await it in
    /// sequence; it returns once the message is *enqueued*, which is all the
    /// ordering guarantee needs. Nothing is awaited from the server, because a
    /// notification has no reply to wait for.
    pub async fn open_document(&self, path: &Path, text: &str) {
        self.tell(Message::Open {
            path: path.to_path_buf(),
            text: text.to_string(),
        });
    }

    /// The document's contents changed.
    pub async fn change_document(&self, path: &Path, text: &str) {
        self.tell(Message::Change {
            path: path.to_path_buf(),
            text: text.to_string(),
        });
    }

    /// The editor closed the document, so the servers go back to disk.
    pub async fn close_document(&self, path: &Path) {
        self.tell(Message::Close {
            path: path.to_path_buf(),
        });
    }

    /// Every use site of the symbol at `line`/`character`.
    ///
    /// `line` is **1-based** (the editor gutter) and `character` is **0-based
    /// UTF-16 code units** (what CodeMirror hands over). The asymmetry is the IPC
    /// contract; [`to_lsp_line`] is the only thing that converts.
    pub async fn find_usages(&self, path: &Path, line: u32, character: u32) -> UsageResult {
        self.ask(
            |reply| Message::Usages {
                path: path.to_path_buf(),
                line,
                character,
                reply,
            },
            || UsageResult::unavailable(Availability::Failed, TORN_DOWN),
        )
        .await
    }

    /// Where the symbol at `line`/`character` is declared, implemented and typed.
    pub async fn goto_definition(
        &self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> DefinitionResult {
        self.ask(
            |reply| Message::Definition {
                path: path.to_path_buf(),
                line,
                character,
                reply,
            },
            || DefinitionResult::unavailable(Availability::Failed, TORN_DOWN),
        )
        .await
    }

    /// Which declarations in a file deserve an inline "N usages" row.
    pub async fn declaration_anchors(&self, path: &Path) -> AnchorResult {
        self.ask(
            |reply| Message::Anchors {
                path: path.to_path_buf(),
                reply,
            },
            || AnchorResult::unavailable(Availability::Failed, TORN_DOWN),
        )
        .await
    }

    /// Enqueue a notification, or drop it because there is nobody left to tell.
    fn tell(&self, message: Message) {
        let _ = self.tx.send(message);
    }

    /// Enqueue a request and wait for its answer, or report the shutdown.
    ///
    /// Both failure paths — the send and the receive — mean the same thing: the
    /// actor is gone, so this workspace's servers are gone.
    ///
    /// The outcome is [`Availability::Failed`], which is the least wrong of six
    /// and is **not** a claim that a server misbehaved: none of the six says "this
    /// session no longer exists", `NotConfigured` would say something false about
    /// the user's machine, and the remaining four are worse. The distinction that
    /// matters is carried by the message ([`TORN_DOWN`]), which names the closed
    /// workspace and says to re-open it — so a reader must render the message and
    /// not the bucket. Do not read this as keeping the shutdown *out* of the
    /// failed bucket: it is in it, deliberately, with a reason attached.
    async fn ask<T>(
        &self,
        message: impl FnOnce(oneshot::Sender<T>) -> Message,
        gone: impl FnOnce() -> T,
    ) -> T {
        let (reply, answer) = oneshot::channel();
        if self.tx.send(message(reply)).is_err() {
            return gone();
        }
        match answer.await {
            Ok(answer) => answer,
            Err(_) => gone(),
        }
    }
}

/// Start a session for `root`, resolving every language but starting none.
///
/// Must be called from inside a Tokio runtime: the actor is a spawned task.
/// Resolution happens **here**, synchronously, so the status surface is complete
/// the moment the caller has a handle — a handful of directory reads against a
/// question the user will ask as soon as the window is up.
pub fn start(root: PathBuf, config: Option<LspConfig>, generation: u64) -> LspHandle {
    start_with_probe(root, config, generation, Arc::new(RealProbe))
}

/// [`start`] with the machine injected.
///
/// Exists for the same reason [`registry::Probe`] does: "no server is installed"
/// is the interesting case and cannot be arranged on a developer's box, where
/// this repository's own author really does have Roslyn.
pub fn start_with_probe(
    root: PathBuf,
    config: Option<LspConfig>,
    generation: u64,
    probe: Arc<dyn Probe + Send + Sync>,
) -> LspHandle {
    let status: SharedStatus = Arc::new(Mutex::new(BTreeMap::new()));
    let (tx, rx) = mpsc::unbounded_channel();

    let mut session = Session {
        root: root.clone(),
        servers: BTreeMap::new(),
        documents: BTreeMap::new(),
        status: Arc::clone(&status),
        // Weak, so the actor's own copy cannot keep the channel open: when every
        // `LspHandle` is dropped the receive loop has to end and tear down, and
        // a strong sender held here would mean it never did.
        tx: tx.downgrade(),
        probe,
    };
    session.resolve_all(config.as_ref());
    tokio::spawn(session.run(rx));

    LspHandle {
        root,
        generation,
        tx,
        status,
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Everything the actor reacts to, from handles and from its own tasks.
enum Message {
    Open {
        path: PathBuf,
        text: String,
    },
    Change {
        path: PathBuf,
        text: String,
    },
    Close {
        path: PathBuf,
    },
    Usages {
        path: PathBuf,
        line: u32,
        character: u32,
        reply: oneshot::Sender<UsageResult>,
    },
    Definition {
        path: PathBuf,
        line: u32,
        character: u32,
        reply: oneshot::Sender<DefinitionResult>,
    },
    Anchors {
        path: PathBuf,
        reply: oneshot::Sender<AnchorResult>,
    },
    /// A start attempt finished. Boxed because a [`Client`] is far larger than
    /// every other variant here.
    Started {
        language: Language,
        epoch: u64,
        outcome: Box<Result<Client, StartFailure>>,
    },
    /// The supervisor saw a server die.
    Died {
        language: Language,
        epoch: u64,
        death: Box<Death>,
    },
    /// The supervisor saw a server's readiness change.
    Readiness {
        language: Language,
        epoch: u64,
    },
    Teardown,
}

// ---------------------------------------------------------------------------
// One server's state
// ---------------------------------------------------------------------------

/// What is happening with one language's server.
enum State {
    /// Resolved and startable, and nothing has asked for it yet.
    Idle,
    /// A process is coming up. The request that caused this was answered
    /// [`Availability::Starting`] rather than made to wait for it.
    Starting,
    Up(Arc<Client>),
    /// It will not be tried again until the workspace is re-opened. Carries what
    /// there is to tell the user, including the server's last words.
    Failed(String),
    /// It was never startable: absent, switched off, or configured wrongly. The
    /// outcome is kept because those are not the same thing.
    Unavailable {
        outcome: Availability,
        message: String,
    },
}

/// One language, its server, and the history that decides whether to try again.
struct Server {
    spec: Option<Box<ServerSpec>>,
    state: State,
    /// At most one automatic restart per minute, then permanent failure. Owned
    /// here rather than in the transport because the decision outlives the
    /// process it is about.
    policy: RestartPolicy,
    /// Bumped on every start attempt, so a `Started` or `Died` message about a
    /// process this session has already replaced is discarded instead of
    /// overwriting the state of its successor.
    epoch: u64,
    looked_for: Vec<String>,
    hint: Option<String>,
}

// ---------------------------------------------------------------------------
// The actor
// ---------------------------------------------------------------------------

/// The state every message is handled against. Never shared: it lives in the
/// actor's task and is reached only through the channel.
struct Session {
    root: PathBuf,
    servers: BTreeMap<Language, Server>,
    /// One mirror per language, so [`Documents::replay_for`] means exactly what
    /// it says. A single mirror keyed by path would replay a `.rs` file to the C#
    /// server, and filtering the result afterwards would leave the mirror's own
    /// record of who has what open permanently wrong.
    documents: BTreeMap<Language, Documents>,
    status: SharedStatus,
    tx: mpsc::WeakUnboundedSender<Message>,
    probe: Arc<dyn Probe + Send + Sync>,
}

impl Session {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Message>) {
        while let Some(message) = rx.recv().await {
            match message {
                Message::Open { path, text } => self.on_open(path, text),
                Message::Change { path, text } => self.on_change(path, text),
                Message::Close { path } => self.on_close(path),
                Message::Usages {
                    path,
                    line,
                    character,
                    reply,
                } => self.on_usages(path, line, character, reply),
                Message::Definition {
                    path,
                    line,
                    character,
                    reply,
                } => self.on_definition(path, line, character, reply),
                Message::Anchors { path, reply } => self.on_anchors(path, reply),
                Message::Started {
                    language,
                    epoch,
                    outcome,
                } => self.on_started(language, epoch, *outcome),
                Message::Died {
                    language,
                    epoch,
                    death,
                } => self.on_died(language, epoch, &death),
                Message::Readiness { language, epoch } => self.on_readiness(language, epoch),
                Message::Teardown => break,
            }
        }
        // Reached by a teardown *and* by the last handle being dropped. Both mean
        // the same thing, and only one of them is polite enough to say so.
        self.teardown();
    }

    // -----------------------------------------------------------------------
    // Resolution
    // -----------------------------------------------------------------------

    /// Decide, once, which server serves each language — or why none does.
    ///
    /// Done for all four up front rather than on demand so that "Python is not
    /// installed, here is how to get it" is on the status surface before anybody
    /// opens a `.py` file. Nothing is *started*: that is lazy, and stays lazy.
    fn resolve_all(&mut self, config: Option<&LspConfig>) {
        for language in LANGUAGES {
            let server = match registry::resolve(language, config, &*self.probe) {
                Resolution::Found(spec) => Server {
                    spec: Some(spec),
                    state: State::Idle,
                    policy: RestartPolicy::new(),
                    epoch: 0,
                    looked_for: Vec::new(),
                    hint: None,
                },
                Resolution::NotFound {
                    looked_for, hint, ..
                } => Server {
                    spec: None,
                    state: State::Unavailable {
                        outcome: Availability::NotConfigured,
                        message: format!(
                            "no {} language server was found on this machine. {hint}",
                            name(language)
                        ),
                    },
                    policy: RestartPolicy::new(),
                    epoch: 0,
                    looked_for,
                    hint: Some(hint),
                },
                Resolution::Disabled { .. } => Server {
                    spec: None,
                    state: State::Unavailable {
                        outcome: Availability::NotConfigured,
                        message: format!(
                            "the {} language server is switched off: \
                             `lsp.servers.{}.enabled` is false in .code-basics/config.json.",
                            name(language),
                            language.id()
                        ),
                    },
                    policy: RestartPolicy::new(),
                    epoch: 0,
                    looked_for: Vec::new(),
                    hint: None,
                },
                // `Failed`, not `NotConfigured`: it *is* configured, and it is
                // broken. `detail` already names the program and the file.
                Resolution::Misconfigured { detail, .. } => Server {
                    spec: None,
                    state: State::Unavailable {
                        outcome: Availability::Failed,
                        message: detail,
                    },
                    policy: RestartPolicy::new(),
                    epoch: 0,
                    looked_for: Vec::new(),
                    hint: None,
                },
            };
            self.servers.insert(language, server);
            self.refresh_status(language);
        }
    }

    /// Start one language's server, unless it is already up, coming up, or
    /// unusable.
    fn ensure_started(&mut self, language: Language) {
        let root = self.root.clone();
        let tx = self.tx.clone();

        let Some(server) = self.servers.get_mut(&language) else {
            return;
        };
        if !matches!(server.state, State::Idle) {
            return;
        }
        let Some(spec) = server.spec.clone() else {
            return;
        };
        server.epoch += 1;
        let epoch = server.epoch;
        server.state = State::Starting;

        tokio::spawn(async move {
            let outcome = Client::start(*spec, &root).await;
            match tx.upgrade() {
                Some(tx) => {
                    let _ = tx.send(Message::Started {
                        language,
                        epoch,
                        outcome: Box::new(outcome),
                    });
                }
                // Nobody is left to hand it to, so the client is dropped here —
                // which is what kills the process tree. A server started for a
                // workspace that has since been closed must not outlive it.
                None => tracing::debug!(
                    server = language.id(),
                    "the session was gone before its server finished starting"
                ),
            }
        });

        self.refresh_status(language);
    }

    fn on_started(
        &mut self,
        language: Language,
        epoch: u64,
        outcome: Result<Client, StartFailure>,
    ) {
        let Some(server) = self.servers.get_mut(&language) else {
            return;
        };
        if server.epoch != epoch {
            // Superseded by a restart. Dropping `outcome` here kills the process
            // it carries, which is the whole reason this is not just ignored.
            tracing::debug!(server = language.id(), "discarding a superseded server");
            return;
        }

        match outcome {
            Ok(client) => {
                let client = Arc::new(client);
                server.state = State::Up(Arc::clone(&client));
                spawn_supervisor(language, epoch, &client, self.tx.clone());

                // The rule that breaks in the app and not in a test: every
                // document opened while this server was starting is invisible to
                // it forever unless it is told now, and "never heard of that
                // buffer" comes back as an empty answer rather than as an error.
                let actions = self
                    .documents
                    .entry(language)
                    .or_default()
                    .replay_for(language.id());
                dispatch(&client, &actions);
            }
            Err(failure) => {
                server.state = State::Failed(failure.to_string());
            }
        }
        self.refresh_status(language);
    }

    fn on_died(&mut self, language: Language, epoch: u64, death: &Death) {
        let restart = {
            let Some(server) = self.servers.get_mut(&language) else {
                return;
            };
            if server.epoch != epoch {
                return;
            }
            match server.policy.on_death(Instant::now(), death) {
                RestartDecision::Restart => {
                    tracing::warn!(server = language.id(), %death, "restarting the language server");
                    server.state = State::Idle;
                    true
                }
                RestartDecision::Failed { stderr_tail } => {
                    tracing::error!(server = language.id(), %death, "the language server failed");
                    server.state = State::Failed(failure_detail(death, &stderr_tail));
                    false
                }
            }
        };

        if restart {
            // Straight away rather than on the next request: the user has files
            // open, and the replay that follows is what makes the new process
            // useful. `ensure_started` refreshes the status itself.
            self.ensure_started(language);
        } else {
            self.refresh_status(language);
        }
    }

    fn on_readiness(&mut self, language: Language, epoch: u64) {
        if self
            .servers
            .get(&language)
            .is_some_and(|server| server.epoch == epoch)
        {
            self.refresh_status(language);
        }
    }

    // -----------------------------------------------------------------------
    // Documents
    // -----------------------------------------------------------------------

    fn on_open(&mut self, path: PathBuf, text: String) {
        let path = self.absolute(&path);
        let Some(language) = language_of(&path) else {
            // Nothing serves it, so nothing is mirrored. A request about it will
            // say so, naming the extension.
            return;
        };
        // Opening a file of a language is exactly the event that starts its
        // server: the user is looking at the file, and the answer they will ask
        // for in a moment takes tens of seconds to become available.
        self.ensure_started(language);

        let client = self.live_client(language);
        // Named only when there is somebody to tell. A server that is not up yet
        // is caught up by the replay, not by being recorded as though it had been
        // told something it never heard.
        let servers: Vec<ServerId> = if client.is_some() {
            vec![language.id()]
        } else {
            Vec::new()
        };
        let actions = self
            .documents
            .entry(language)
            .or_default()
            .open(&path, &text, &servers);
        if let Some(client) = client {
            dispatch(&client, &actions);
        }
    }

    fn on_change(&mut self, path: PathBuf, text: String) {
        let path = self.absolute(&path);
        let Some(language) = language_of(&path) else {
            return;
        };
        // The mirror is advanced even when no server is listening yet, so the
        // replay a late server receives is the *edited* text and not the text as
        // it was when the file was opened.
        let actions = self
            .documents
            .entry(language)
            .or_default()
            .change(&path, &text);
        if let Some(client) = self.live_client(language) {
            dispatch(&client, &actions);
        }
    }

    fn on_close(&mut self, path: PathBuf) {
        let path = self.absolute(&path);
        let Some(language) = language_of(&path) else {
            return;
        };
        let actions = self.documents.entry(language).or_default().close(&path);
        if let Some(client) = self.live_client(language) {
            dispatch(&client, &actions);
        }
    }

    // -----------------------------------------------------------------------
    // Requests
    // -----------------------------------------------------------------------

    fn on_usages(
        &mut self,
        path: PathBuf,
        line: u32,
        character: u32,
        reply: oneshot::Sender<UsageResult>,
    ) {
        let prepared = match self.prepare(&path, Needs::Position) {
            Ok(prepared) => prepared,
            Err(unready) => {
                let _ = reply.send(unready.usages());
                return;
            }
        };
        let Prepared {
            client,
            server,
            root,
            path,
            mut text,
            caveat,
        } = prepared;
        let position = request_position(line, character);

        tokio::spawn(async move {
            // `include_declaration: false`. The row this count is drawn on *is*
            // the declaration, so counting it would report "1 usage" for a symbol
            // nothing uses — which is the one number a reader would act on.
            let answer = client.references(&path, position, false).await;
            let result = match answer {
                Ok(locations) => {
                    let mut result = results::usages(&root, &locations, &mut text, USAGE_CAP)
                        .with_server(server);
                    // A count is only ever qualified, never withheld: the rows
                    // are real, and `Ready` with a message is expressible in the
                    // contract precisely so a promoted server's number can be
                    // shown for what it is.
                    result.message = with_caveat(result.message.take(), caveat.as_deref());
                    result
                }
                Err(error) => UsageResult::unavailable(availability_for(&error), error.to_string())
                    .with_server(server),
            };
            let _ = reply.send(result);
        });
    }

    fn on_definition(
        &mut self,
        path: PathBuf,
        line: u32,
        character: u32,
        reply: oneshot::Sender<DefinitionResult>,
    ) {
        let prepared = match self.prepare(&path, Needs::Position) {
            Ok(prepared) => prepared,
            Err(unready) => {
                let _ = reply.send(unready.definition());
                return;
            }
        };
        let Prepared {
            client,
            root,
            path,
            mut text,
            caveat,
            ..
        } = prepared;
        let position = request_position(line, character);

        tokio::spawn(async move {
            // Concurrently: they are three independent questions to one server,
            // and asking them in sequence would triple the wait for no reason.
            let (definitions, implementations, type_definitions) = tokio::join!(
                client.definition(&path, position),
                client.implementation(&path, position),
                client.type_definition(&path, position),
            );

            let mut notes = Vec::new();
            let mut refusals = Vec::new();
            let definitions = group(definitions, "declarations", &mut notes, &mut refusals);
            let implementations = group(
                implementations,
                "implementations",
                &mut notes,
                &mut refusals,
            );
            let type_definitions = group(
                type_definitions,
                "type definitions",
                &mut notes,
                &mut refusals,
            );

            let mut result = results::targets(
                &root,
                &definitions,
                &implementations,
                &type_definitions,
                &mut text,
            );
            // An empty group must never read as "there are none" when the reason
            // it is empty is that nobody could be asked. When all three failed
            // the whole answer takes the **most severe** reason — never the first,
            // because the groups are asked in a fixed order and a server that
            // does not advertise `definitionProvider` and then dies would
            // otherwise be reported `Unsupported`, the one outcome that says
            // retrying is pointless. When only some refused, the outcome stays
            // `Ready` and the note says which group is missing: there is one
            // outcome for three groups, so a partial refusal cannot be spelled
            // any other way, and the note is the only signal a reader gets.
            if refusals.len() == 3 {
                result.outcome = worst(&refusals);
            }
            result.message = with_caveat(
                (!notes.is_empty()).then(|| notes.join(" ")),
                caveat.as_deref(),
            );
            let _ = reply.send(result);
        });
    }

    fn on_anchors(&mut self, path: PathBuf, reply: oneshot::Sender<AnchorResult>) {
        // `Needs::OpenDocument`, unlike the other two: `documentSymbol` is a
        // question about an open buffer, so a server that was never sent
        // `didOpen` answers `null` — which decodes to an empty list and would
        // reach the user as "this file declares nothing".
        let prepared = match self.prepare(&path, Needs::OpenDocument) {
            Ok(prepared) => prepared,
            Err(unready) => {
                let _ = reply.send(unready.anchors());
                return;
            }
        };
        let Prepared {
            client,
            path,
            caveat,
            ..
        } = prepared;

        tokio::spawn(async move {
            let result = match client.document_symbols(&path).await {
                Ok(symbols) => AnchorResult {
                    outcome: Availability::Ready,
                    anchors: results::anchors(&symbols),
                    message: with_caveat(None, caveat.as_deref()),
                },
                Err(error) => {
                    AnchorResult::unavailable(availability_for(&error), error.to_string())
                }
            };
            let _ = reply.send(result);
        });
    }

    /// Everything a request needs, or the reason it cannot be asked.
    ///
    /// Runs on the actor, so every document notification it emits is written to
    /// the server's stdin **before** the request that follows — including the ones
    /// enqueued by earlier `change_document` calls, which were handled by earlier
    /// iterations of the loop. That ordering *is* the flush: `didChange` and
    /// `references` travel the same stream, and asking the server to confirm the
    /// change would be a round trip nobody needs.
    ///
    /// # Why the disk read and the buffer snapshot are on the actor
    ///
    /// Both are unbounded-ish latency on the message loop, and moving them into
    /// the spawned request task was considered and refused: the `didOpen` that
    /// [`Session::ensure_open`] emits **must** be written ahead of the request,
    /// and it must also stay ordered against the `didChange` of whatever the user
    /// types next — which is a property of being written from the actor, in the
    /// order the actor handled the messages. A read moved off the actor buys
    /// latency and pays with the one guarantee this module's ordering argument
    /// rests on. The read is therefore bounded instead ([`AUTO_OPEN_MAX_BYTES`])
    /// and happens at most once per file per server, and the snapshot copies only
    /// buffers the editor actually has open.
    fn prepare(&mut self, path: &Path, needs: Needs) -> Result<Prepared, Unready> {
        let path = self.absolute(path);

        let Some(extension) = path.extension().and_then(|e| e.to_str()) else {
            return Err(Unready::not_configured(format!(
                "{} has no file extension, so no language server claims it.",
                path.display()
            )));
        };
        let Some(language) = registry::language_for_extension(extension) else {
            return Err(Unready::not_configured(format!(
                "no language server is configured for `.{extension}` files. This app can \
                 answer for C#, TypeScript/JavaScript, Rust and Python."
            )));
        };

        self.ensure_started(language);
        let Some(server) = self.servers.get(&language) else {
            return Err(Unready::not_configured(format!(
                "{} is not a language this session knows about.",
                name(language)
            )));
        };

        let client = match &server.state {
            // `Idle` is unreachable after `ensure_started`, and answering it as
            // anything other than "coming up" would be a guess.
            State::Idle | State::Starting => {
                return Err(Unready {
                    outcome: Availability::Starting,
                    message: format!(
                        "the {} language server is still starting. Ask again in a moment — \
                         the answer is not empty, it is not ready.",
                        name(language)
                    ),
                    server: None,
                })
            }
            State::Failed(detail) => {
                return Err(Unready {
                    outcome: Availability::Failed,
                    message: detail.clone(),
                    server: Some(language.id().to_string()),
                })
            }
            State::Unavailable { outcome, message } => {
                return Err(Unready {
                    outcome: *outcome,
                    message: message.clone(),
                    server: None,
                })
            }
            State::Up(client) => Arc::clone(client),
        };

        // The supervisor will notice this within a quarter second and decide
        // whether to restart; this request is over either way, and it is over
        // with a reason rather than with an empty list.
        if let Some(death) = client.death() {
            return Err(Unready {
                outcome: Availability::Failed,
                message: failure_detail(&death, &death.stderr_tail),
                server: Some(language.id().to_string()),
            });
        }

        // Checked here rather than left to `Client::ask`, which *waits* for
        // readiness up to the request deadline — 180 seconds for Roslyn. Waiting
        // is right for a batch caller and wrong for a widget, and "still loading"
        // is a perfectly good thing to render.
        let readiness = client.readiness();
        if !readiness.is_ready() {
            return Err(Unready {
                outcome: Availability::Loading,
                message: format!(
                    "the {} language server is still loading this workspace. A count taken \
                     now would be low, which reads exactly like a correct one.",
                    name(language)
                ),
                server: Some(language.id().to_string()),
            });
        }

        if let Err(reason) = self.ensure_open(language, &path, &client) {
            // For a question *about a buffer* this is the whole answer: the
            // server was never told about the document, so its `null` would
            // decode to an empty list and read as "nothing is declared here".
            // For `references` and the goto questions it is not — the server
            // knows the file from the project — so those proceed, which is what
            // keeps a maybe from becoming a no.
            if needs == Needs::OpenDocument {
                return Err(Unready {
                    outcome: Availability::Failed,
                    message: reason,
                    server: Some(language.id().to_string()),
                });
            }
        }

        Ok(Prepared {
            client,
            server: language.id().to_string(),
            root: self.root.clone(),
            path,
            text: WorkspaceText::new(self.buffers()),
            caveat: caveat_note(&readiness),
        })
    }

    /// Make sure this server has been told about the document being asked about.
    ///
    /// A request can name a file the editor never opened — from the search
    /// palette, or a jump into a file that is merely being previewed — and a
    /// server answers about the buffer it was told about and nothing else.
    ///
    /// Returns why the server could **not** be told, rather than abandoning the
    /// open silently: for `references` and the goto questions that failure is
    /// survivable (the server knows the file from the project, and refusing to
    /// ask would turn a maybe into a no), but for `documentSymbol` it is the
    /// answer — and the caller is the only one who knows which it is asking.
    fn ensure_open(
        &mut self,
        language: Language,
        path: &Path,
        client: &Client,
    ) -> Result<(), String> {
        let id = language.id();
        let documents = self.documents.entry(language).or_default();
        if documents.servers_for(path).contains(&id) {
            return Ok(());
        }

        let text = match documents.text(path) {
            // Mirrored already (opened while this server was down, say) — the
            // buffer is the truth, and it is newer than the file on disk.
            Some(text) => text.to_string(),
            None => match readable(path) {
                Some(text) => text,
                None => {
                    return Err(format!(
                        "{} could not be read — it is missing, unreadable, or larger \
                         than {} bytes — so no server could be told about it.",
                        path.display(),
                        AUTO_OPEN_MAX_BYTES
                    ))
                }
            },
        };
        let actions = documents.open(path, &text, &[id]);
        dispatch(client, &actions);
        Ok(())
    }

    /// Every open buffer, for the snippet reader. The buffer rather than the file
    /// on disk, because an unsaved edit is the normal state of a file somebody is
    /// asking questions about.
    fn buffers(&self) -> BTreeMap<PathBuf, String> {
        let mut buffers = BTreeMap::new();
        for documents in self.documents.values() {
            for path in documents.open_paths() {
                if let Some(text) = documents.text(path) {
                    buffers.insert(path.to_path_buf(), text.to_string());
                }
            }
        }
        buffers
    }

    /// The server for a language, if there is a live one to send notifications to.
    fn live_client(&self, language: Language) -> Option<Arc<Client>> {
        match self.servers.get(&language).map(|server| &server.state) {
            Some(State::Up(client)) if client.death().is_none() => Some(Arc::clone(client)),
            _ => None,
        }
    }

    /// A path as this session names it.
    ///
    /// Relative paths are joined onto the root, because the frontend's own
    /// spelling of a file is workspace-relative (that is what the file tree, the
    /// symbol index and the Changes tab all use) while a URI that came back from
    /// a server has already been resolved to an absolute path.
    fn absolute(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }

    // -----------------------------------------------------------------------
    // Status
    // -----------------------------------------------------------------------

    /// Recompute one language's status row from its state.
    fn refresh_status(&self, language: Language) {
        let row = self.servers.get(&language).and_then(|server| {
            let mut caveat = None;
            let (state, detail) = match &server.state {
                // Resolved and untouched. Deliberately not listed: see
                // `LspHandle::status`.
                State::Idle => return None,
                State::Starting => (Availability::Starting, None),
                State::Failed(detail) => (Availability::Failed, Some(detail.clone())),
                State::Unavailable { outcome, message } => (*outcome, Some(message.clone())),
                State::Up(client) => {
                    let row = running_row(
                        &client.readiness(),
                        &client.spec().program.display().to_string(),
                    );
                    caveat = row.caveat;
                    (row.state, row.detail)
                }
            };
            Some(ServerStatus {
                id: language.id().to_string(),
                language: name(language).to_string(),
                state,
                detail,
                caveat,
                looked_for: server.looked_for.clone(),
                hint: server.hint.clone(),
            })
        });

        let mut status = match self.status.lock() {
            Ok(status) => status,
            Err(poisoned) => poisoned.into_inner(),
        };
        match row {
            Some(row) => {
                status.insert(language, row);
            }
            None => {
                status.remove(&language);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Teardown
    // -----------------------------------------------------------------------

    /// Shut every server down and stop being able to answer anything.
    ///
    /// The shutdowns are spawned rather than awaited, and for two reasons: each
    /// one is bounded by the transport's own shutdown and exit grace, so four in
    /// sequence would be twenty-odd seconds of an actor that has already been
    /// superseded — and the task holds the last `Arc`, so a runtime that never
    /// polls it still drops the client and kills the tree.
    fn teardown(&mut self) {
        for (language, server) in &mut self.servers {
            if let State::Up(client) = std::mem::replace(&mut server.state, State::Idle) {
                tracing::debug!(server = language.id(), "shutting down");
                tokio::spawn(async move { client.shutdown().await });
            }
        }
        self.documents.clear();
        // Cleared, not frozen: nothing is running, and a snapshot that still said
        // `Ready` would be read as a session that can answer.
        if let Ok(mut status) = self.status.lock() {
            status.clear();
        }
    }
}

// ---------------------------------------------------------------------------
// Supervision
// ---------------------------------------------------------------------------

/// Watch one client for death and for readiness changes.
///
/// Holds a [`Weak`], so it cannot keep a client (or its process) alive past the
/// session: the loop simply ends the first time the upgrade fails.
fn spawn_supervisor(
    language: Language,
    epoch: u64,
    client: &Arc<Client>,
    tx: mpsc::WeakUnboundedSender<Message>,
) {
    // The whole state, not its `is_ready()` bit. `ReadyWithCaveat` and `Ready`
    // are both ready and say different things about every count the server
    // answers, and a status row is only recomputed when this reports a change —
    // so comparing the bit left a server whose projects finished loading after
    // the ceiling permanently described as one whose answers may be incomplete.
    let mut last = client.readiness();
    let client = Arc::downgrade(client);

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(SUPERVISION_INTERVAL).await;
            let (Some(client), Some(tx)) = (client.upgrade(), tx.upgrade()) else {
                return;
            };

            if let Some(death) = client.death() {
                let _ = tx.send(Message::Died {
                    language,
                    epoch,
                    death: Box::new(death),
                });
                return;
            }

            let ready = client.readiness();
            if ready != last {
                last = ready;
                let _ = tx.send(Message::Readiness { language, epoch });
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Turning a state into an answer
// ---------------------------------------------------------------------------

/// Everything a spawned request task needs, with nothing borrowed from the actor.
struct Prepared {
    client: Arc<Client>,
    server: String,
    root: PathBuf,
    path: PathBuf,
    text: WorkspaceText,
    /// The qualification a count taken in this state has earned, if any. See
    /// [`caveat_note`].
    caveat: Option<String>,
}

/// What a request needs of the document before it can be believed.
///
/// The distinction is not a preference: `textDocument/documentSymbol` is a
/// question about an *open buffer* and answers `null` for anything else, while
/// `references`, `definition`, `implementation` and `typeDefinition` are
/// questions about a position in a project the server has already loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Needs {
    /// A position in a file the server may know from the project.
    Position,
    /// A buffer the server must have been sent a `didOpen` for.
    OpenDocument,
}

/// Why a request could not be asked, in the three shapes the frontend reads.
struct Unready {
    outcome: Availability,
    message: String,
    /// `None` when no server was involved at all — an absent one names nothing.
    server: Option<String>,
}

impl Unready {
    fn not_configured(message: String) -> Self {
        Self {
            outcome: Availability::NotConfigured,
            message,
            server: None,
        }
    }

    fn usages(self) -> UsageResult {
        let result = UsageResult::unavailable(self.outcome, self.message);
        match self.server {
            Some(server) => result.with_server(server),
            None => result,
        }
    }

    fn definition(self) -> DefinitionResult {
        DefinitionResult::unavailable(self.outcome, self.message)
    }

    fn anchors(self) -> AnchorResult {
        AnchorResult::unavailable(self.outcome, self.message)
    }
}

/// One goto group's locations, or nothing plus a note saying why nothing.
///
/// The note is the entire point. An empty `implementations` list that says
/// nothing is read as "this symbol has no implementations", which is a claim
/// about the user's code rather than about our conversation with a server.
fn group(
    answer: Result<Vec<Location>, RequestError>,
    what: &str,
    notes: &mut Vec<String>,
    refusals: &mut Vec<Availability>,
) -> Vec<Location> {
    match answer {
        Ok(locations) => locations,
        Err(error) => {
            notes.push(format!("No {what}: {error}"));
            refusals.push(availability_for(&error));
            Vec::new()
        }
    }
}

/// Where to aim a request, converting the one direction of the IPC convention
/// that a scripted test cannot see.
///
/// Extracted from the two call sites so it can be *pinned*: every reply in the
/// integration tests is scripted independently of the position asked about, so a
/// call site that forgot [`to_lsp_line`] would aim every request one line below
/// the caret with the whole suite still green — and a server that resolves no
/// symbol at that position answers `[]`, which arrives as a confident "0 usages".
///
/// `line` is **1-based** (the editor gutter) and `character` is **0-based UTF-16
/// code units** (what CodeMirror hands over). The asymmetry is deliberate and is
/// the IPC contract in both directions.
fn request_position(line: u32, character: u32) -> Position {
    Position {
        line: to_lsp_line(line),
        character,
    }
}

/// The reason a wholly refused answer reports, out of the reasons it collected.
///
/// Severity and not order. The three goto groups are asked — and pushed — in a
/// fixed order, so `refusals[0]` would let a server that merely lacks
/// `definitionProvider` and then *dies* be reported as [`Availability::Unsupported`],
/// which [`Availability`] defines as a healthy server lacking a capability and is
/// the one outcome that tells the user retrying is pointless.
///
/// Only `Unsupported`, `Loading` and `Failed` are reachable here (see
/// [`availability_for`]); the rest are ranked anyway so that adding a variant
/// cannot silently make `Ready` win.
fn worst(refusals: &[Availability]) -> Availability {
    fn rank(outcome: Availability) -> u8 {
        match outcome {
            // Never a refusal, and ranked last so it can never become one.
            Availability::Ready => 0,
            Availability::Unsupported => 1,
            Availability::NotConfigured => 2,
            Availability::Starting => 3,
            Availability::Loading => 4,
            Availability::Failed => 5,
        }
    }
    refusals
        .iter()
        .copied()
        .max_by_key(|outcome| rank(*outcome))
        .unwrap_or(Availability::Failed)
}

/// One running server's status row, minus the parts that are not about
/// readiness.
struct RunningRow {
    state: Availability,
    /// What the program is, or what it is doing. Never the caveat.
    detail: Option<String>,
    /// Set only for [`ReadyState::ReadyWithCaveat`]. See [`running_row`].
    caveat: Option<String>,
}

/// Turn a running server's readiness into the row the status surface draws.
///
/// Extracted from `refresh_status` so the one decision in it can be tested: a
/// server promoted at the ceiling is `Ready`, and its caveat is a **field of its
/// own** rather than prose in `detail`. `detail` already means the program path
/// for a plain `Ready`, so a caveat delivered there is a state the frontend
/// cannot distinguish from a healthy server without parsing English — and it did
/// not: the titlebar indicator drops every `ready` row, so a ceiling-promoted
/// server was byte-identical to a primed one on the one surface that exists to
/// explain server state, while every count in the file said it might be low.
fn running_row(readiness: &ReadyState, program: &str) -> RunningRow {
    match readiness {
        ReadyState::Loading => RunningRow {
            state: Availability::Loading,
            detail: Some(format!("{program} is loading this workspace")),
            caveat: None,
        },
        ReadyState::ReadyWithCaveat { detail } => RunningRow {
            state: Availability::Ready,
            detail: Some(program.to_string()),
            caveat: Some(detail.clone()),
        },
        ReadyState::Ready => RunningRow {
            state: Availability::Ready,
            detail: Some(program.to_string()),
            caveat: None,
        },
    }
}

/// The qualification a count taken from a server promoted at the readiness
/// ceiling has earned, or nothing at all.
///
/// [`ReadyState::is_ready`] is deliberately true for
/// [`ReadyState::ReadyWithCaveat`], so the request proceeds — and
/// [`Client::ask`]'s own contract makes qualifying the result the caller's job,
/// because the answer itself is byte-identical to one from a primed server. This
/// module is that caller, and this is where the caveat re-enters the answer.
/// Without it, "3 usages" from a Roslyn that never finished loading a solution is
/// indistinguishable from "3 usages", which is the exact failure the whole
/// subsystem exists to refuse.
fn caveat_note(readiness: &ReadyState) -> Option<String> {
    match readiness {
        ReadyState::ReadyWithCaveat { detail } => Some(format!(
            "this answer was taken from a server that never finished priming ({detail}), \
             so a count may be low."
        )),
        ReadyState::Ready | ReadyState::Loading => None,
    }
}

/// Add the caveat to whatever the answer already had to say.
///
/// Both halves are about different things — one group could not be asked, and the
/// whole answer came from a server that never finished priming — so neither may
/// replace the other.
fn with_caveat(message: Option<String>, caveat: Option<&str>) -> Option<String> {
    match (message, caveat) {
        (Some(message), Some(caveat)) => Some(format!("{message} {caveat}")),
        (Some(message), None) => Some(message),
        (None, Some(caveat)) => Some(caveat.to_string()),
        (None, None) => None,
    }
}

/// Which outcome one request failure is.
fn availability_for(error: &RequestError) -> Availability {
    match error {
        RequestError::Unsupported { .. } => Availability::Unsupported,
        RequestError::StillLoading { .. } => Availability::Loading,
        // `UnusableUri` and `NotOpen` are ours rather than the server's, but they
        // are still "the question could not be put", which is what `Failed` says
        // and is not what any other variant says.
        RequestError::Failed { .. }
        | RequestError::UnusableUri { .. }
        | RequestError::NotOpen { .. } => Availability::Failed,
    }
}

/// What to tell the user about a server that has died.
///
/// The stderr tail is the point: by the time somebody is told a language feature
/// is off, the server's own last words are usually the only thing that explains
/// why. Bounded to the last few lines, because the tail holds two hundred.
fn failure_detail(death: &Death, stderr_tail: &[String]) -> String {
    let mut detail = death.to_string();
    let tail: Vec<&String> = stderr_tail.iter().rev().take(5).rev().collect();
    if tail.len() > 1 {
        detail.push_str("\nLast output:");
        for line in tail {
            detail.push('\n');
            detail.push_str(line);
        }
    }
    detail
}

/// The human name of a language, for a status row and for a sentence.
fn name(language: Language) -> &'static str {
    match language {
        Language::CSharp => "C#",
        Language::TypeScript => "TypeScript",
        Language::Rust => "Rust",
        Language::Python => "Python",
    }
}

/// Which language, if any, serves this path.
fn language_of(path: &Path) -> Option<Language> {
    let extension = path.extension().and_then(|e| e.to_str())?;
    registry::language_for_extension(extension)
}

/// A file small enough to hand to a server, or nothing.
fn readable(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > AUTO_OPEN_MAX_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Send one batch of notifications to the server they are owed to.
///
/// A refusal is logged and never propagated: [`Documents`] and
/// [`Client`]'s own mirror can only disagree if one of them has a bug, and
/// failing a user's *next* request because a notification was refused would
/// report the wrong problem in the wrong place.
fn dispatch(client: &Client, actions: &[SyncAction]) {
    for action in actions {
        let outcome = match action {
            SyncAction::DidOpen { path, text, .. } => client.did_open(path, text),
            SyncAction::DidChange { path, text, .. } => client.did_change(path, text),
            SyncAction::DidClose { path, .. } => client.did_close(path),
        };
        if let Err(error) = outcome {
            tracing::warn!(
                server = action.server(),
                path = %action.path().display(),
                %error,
                "a document notification was refused"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Reading lines for snippets
// ---------------------------------------------------------------------------

/// The workspace's text as [`results`] wants it: open buffers first, disk after.
///
/// Buffers first because a server answers about the buffer, so a snippet read
/// from disk would show a line the answer was not about. Disk reads are cached
/// per file, because one usages answer commonly touches the same file many times.
struct WorkspaceText {
    buffers: BTreeMap<PathBuf, String>,
    disk: BTreeMap<PathBuf, Option<Vec<String>>>,
}

impl WorkspaceText {
    fn new(buffers: BTreeMap<PathBuf, String>) -> Self {
        Self {
            buffers,
            disk: BTreeMap::new(),
        }
    }
}

impl TextProvider for WorkspaceText {
    fn line(&mut self, path: &Path, line_zero_based: u32) -> Option<String> {
        let index = line_zero_based as usize;
        if let Some(text) = self.buffers.get(path) {
            return text.lines().nth(index).map(str::to_string);
        }
        let lines = self.disk.entry(path.to_path_buf()).or_insert_with(|| {
            readable(path).map(|text| text.lines().map(str::to_string).collect())
        });
        lines.as_ref()?.get(index).cloned()
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod session_tests;
