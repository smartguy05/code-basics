//! One language server, from handshake to shutdown, as questions and answers.
//!
//! [`super::transport`] owns bytes, ids and death; this owns *meaning*. It is
//! the layer that knows an `initialize` result gates what may be asked, that a
//! server can be running and still not worth asking, and that a document the
//! server has not been told about cannot be asked about at all.
//!
//! # Capability negotiation is a gate, not a formality
//!
//! Everything the handshake advertises is recorded, and a request whose
//! provider is absent is refused with [`RequestError::Unsupported`] before a
//! single byte goes out. That is not an optimisation. "This server does not
//! provide implementations" and "there are no implementations" are **different
//! claims**, and a server that was never asked will happily be rendered as a
//! server that found nothing. Keeping the two apart is the entire point of this
//! subsystem, and this is the place it is decided.
//!
//! Two capabilities are refusals rather than gates, because they make the whole
//! server unusable rather than one feature:
//!
//! * **`positionEncoding` other than `utf-16`.** Every column we computed would
//!   be wrong on every non-ASCII line, invisibly. Its *absence* is not a
//!   refusal — UTF-16 is the protocol's default and the real Roslyn server omits
//!   the field entirely (verified this session).
//! * **`textDocumentSync` resolving to none.** We cannot promise the server a
//!   synchronised buffer, so nothing it says about one can be trusted.
//!
//! # Loading is a third answer
//!
//! rust-analyzer answers `references` from a half-primed index, and Roslyn
//! answers before its projects have loaded. Both give a *low count*, which is a
//! wrong answer wearing the clothes of a right one. So [`Readiness`] is honoured
//! before every request, and a server that is still loading is reported as
//! loading — never as a server that found nothing.
//!
//! The wait has a ceiling ([`READINESS_CEILING`]). A signal that never comes
//! must not disable the feature forever, so the server is then treated as ready
//! — and **said so**, through [`ReadyState::ReadyWithCaveat`], because a silent
//! promotion is indistinguishable from a healthy server.
//!
//! # The document mirror
//!
//! A server answers about the buffer it was told about, not the file on disk, so
//! this tracks what each document was last told and under which version. The
//! version is what makes a dropped notification detectable: a server rejects a
//! change that does not advance it. `did_change` for a document that was never
//! opened is **refused** rather than promoted into a `didOpen` — inventing one
//! would send text the server never asked about, under a version nobody agreed.
//!
//! **The version is supplied by the caller**, not counted here:
//! [`super::documents::Documents`] is the single counter, and it is the only one
//! whose high-water rule survives a close and re-open. What this mirror records
//! is what was last *sent*.
//!
//! Changes go out as a single event whose range spans the whole document, which
//! [`super::protocol::DidChangeTextDocumentParams::whole_document`] builds and
//! `notes.md` justifies. That decision is settled; do not revisit it here.
//!
//! **The range describes the document the server holds, not the one being
//! sent** — so this mirror records the *extent* of each document alongside its
//! version, and hands it back on the next edit. Measuring it from the new text
//! is right only when an edit happens not to change the length; see
//! `did_change`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use thiserror::Error;
use tokio::sync::{broadcast, watch};

use super::protocol::{
    decode_document_symbols, decode_goto, document_end, initialize_params, method, DecodeError,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentSymbolParams, Location, Position, ReferenceParams, ServerCapabilities, Symbol,
    SyncKind, TextDocumentPositionParams,
};
use super::registry::{caller_args, takes_caller_args, Readiness, ServerSpec, Timeouts};
use super::transport::{Death, Launch, Notification, RequestFailure, SignalFilter, Transport};
use super::uri::to_file_uri;

/// How long a server is allowed to say it is still loading.
///
/// Past this it is treated as ready, with a caveat. Roslyn's project load on
/// this repository's own solution took ~20 s in the probe and scales with the
/// number of projects, so this is generous by design: cutting a genuine load
/// short would attach a caveat to a perfectly good answer, and the caveat is
/// meant to be rare enough to be worth reading.
pub const READINESS_CEILING: Duration = Duration::from_secs(90);

/// Roslyn's own "the projects are loaded" notification.
///
/// Not in [`method`] because it is not in the specification: it is one server's
/// extension, and the constant belongs where the server-specific rule is.
const PROJECT_INITIALIZATION_COMPLETE: &str = "workspace/projectInitializationComplete";

// ---------------------------------------------------------------------------
// Readiness
// ---------------------------------------------------------------------------

/// Whether this server's answers can be trusted yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadyState {
    /// Still priming. Requests are refused with [`RequestError::StillLoading`]
    /// rather than answered from an incomplete index.
    Loading,
    Ready,
    /// Ready because we ran out of patience, not because the server said so.
    ///
    /// Carries the reason, so a count taken in this state can be shown with the
    /// qualification it deserves. Treating this as plain [`ReadyState::Ready`]
    /// would be the silent promotion this variant exists to prevent.
    ReadyWithCaveat {
        detail: String,
    },
}

impl ReadyState {
    /// Whether a request may be sent. True for both ready states — the caveat
    /// qualifies the answer, it does not withhold it.
    pub fn is_ready(&self) -> bool {
        !matches!(self, Self::Loading)
    }
}

// ---------------------------------------------------------------------------
// Failures
// ---------------------------------------------------------------------------

/// Why a server could not be brought up. Each variant is a different thing to
/// tell somebody, and three of them are the server's fault rather than ours.
// Not `Clone`: `DecodeError` is not, and adding it there to satisfy a derive
// here would be a change to a module this one only consumes.
#[derive(Debug, PartialEq, Eq, Error)]
pub enum StartFailure {
    #[error("the language server could not be started: {0}")]
    Spawn(String),
    #[error(
        "{} cannot be expressed as a file: URI, so the server cannot be told which \
         folder to open",
        root.display()
    )]
    RootNotUsable { root: PathBuf },
    #[error("the server did not complete `initialize`: {0}")]
    Handshake(RequestFailure),
    #[error("the server's `initialize` result could not be read: {0}")]
    Capabilities(DecodeError),
    #[error(
        "the server insists on `{encoding}` positions; this client speaks utf-16 only, \
         and every column on every non-ASCII line would be wrong"
    )]
    UnsupportedEncoding { encoding: String },
    #[error(
        "the server does not accept document synchronisation, so nothing it says about \
         an open buffer can be trusted"
    )]
    NoDocumentSync,
}

/// Why a question was not answered.
///
/// **None of these may ever become an empty successful result.** That
/// flattening is exactly how a subsystem reports "0 usages" about a method with
/// forty call sites, and it is the failure this whole feature is written to
/// avoid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RequestError {
    /// The server never advertised this. Emphatically **not** an empty answer:
    /// the UI has to be able to say "this server does not provide
    /// implementations", which is true, rather than showing nothing.
    #[error("this server does not support {method}: it did not advertise `{capability}`")]
    Unsupported {
        method: &'static str,
        capability: &'static str,
    },
    /// The server is up but still priming. A count taken now would be low and
    /// wrong, and low and wrong reads exactly like right.
    #[error("the server is still loading, so {method} was not asked (waited {waited:?})")]
    StillLoading {
        method: &'static str,
        waited: Duration,
    },
    /// Timed out, died, was cancelled, errored, or answered unreadably. The
    /// method is carried here as well as inside the failure because one decoder
    /// serves four requests and cannot name which one it decoded.
    #[error("{method} could not be answered: {failure}")]
    Failed {
        method: &'static str,
        failure: RequestFailure,
    },
    #[error(
        "{} cannot be expressed as a file: URI, so no server can be asked about it",
        path.display()
    )]
    UnusableUri { path: PathBuf },
    #[error(
        "{} has not been opened on this server, so it cannot be changed or closed",
        path.display()
    )]
    NotOpen { path: PathBuf },
}

// ---------------------------------------------------------------------------
// The client
// ---------------------------------------------------------------------------

/// What was last sent to the server about one document.
///
/// The version and the extent travel together because they describe the same
/// notification: the next edit needs the version to advance past, and the extent
/// to replace. Keeping them in one value is what stops the two drifting.
#[derive(Debug, Clone, Copy)]
struct Sent {
    version: i32,
    /// The end of the text as the server now has it. See [`Client::did_change`].
    end: Position,
}

/// One language server, ready to be asked things.
pub struct Client {
    spec: ServerSpec,
    root: PathBuf,
    root_uri: String,
    transport: Transport,
    capabilities: ServerCapabilities,
    ready: watch::Receiver<ReadyState>,
    /// What each open document was last told: the version, and **the extent of
    /// the text that went with it**. Keyed by the path the caller passed: this
    /// map is *our* mirror of *our* messages, so the caller's spelling is the
    /// right key. Identity against a URI the server sends back is a different
    /// question, decided on paths elsewhere.
    ///
    /// The extent is here because the next `didChange` has to describe the range
    /// it is replacing in terms of the buffer the server currently holds, and
    /// this is the only place that knows what that is. Storing the end position
    /// rather than the text keeps it to a few bytes per document — see
    /// [`super::protocol::document_end`].
    documents: Mutex<HashMap<PathBuf, Sent>>,
    /// True until the server has answered something. The first answer pays for
    /// project load or cache priming and the rest do not, so it gets the far
    /// longer deadline.
    first_request: AtomicBool,
}

/// Hand-written because [`Transport`] is not `Debug` and because the useful
/// summary of a client is four facts, not a dump of every open document.
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("server", &self.spec.id)
            .field("pid", &self.transport.pid())
            .field("readiness", &self.readiness())
            .field(
                "open_documents",
                &self.documents.lock().ok().map(|open| open.len()),
            )
            .finish()
    }
}

impl Client {
    /// Start the server, complete the handshake, and refuse it if it cannot be
    /// used correctly.
    ///
    /// Returns as soon as the handshake is done — **not** when the server is
    /// ready. Readiness is observable through [`Client::readiness`] and
    /// [`Client::wait_ready`], because a caller that wants to render "loading"
    /// needs the client before the server is ready, not after.
    pub async fn start(spec: ServerSpec, root: &Path) -> Result<Self, StartFailure> {
        Self::start_with_ceiling(spec, root, READINESS_CEILING).await
    }

    /// [`Client::start`] with the readiness ceiling injected.
    ///
    /// Exists so the ceiling is testable at all: the real one is 90 seconds, and
    /// a rule that can only be exercised by waiting a minute and a half is a
    /// rule that never gets exercised.
    pub async fn start_with_ceiling(
        spec: ServerSpec,
        root: &Path,
        ceiling: Duration,
    ) -> Result<Self, StartFailure> {
        let root_uri =
            to_file_uri(root, spec.uri_style).ok_or_else(|| StartFailure::RootNotUsable {
                root: root.to_path_buf(),
            })?;

        // Our own pid and this workspace's log directory: the two arguments
        // `registry` documents as the caller's, because it knows neither.
        let mut args = spec.args.clone();
        // Asked before the directory is created, not after: a server whose command
        // line takes neither flag would otherwise leave a `lsp-logs/` in every
        // workspace that can never receive a file.
        let log_dir = takes_caller_args(&spec)
            .then(|| prepare_log_dir(root))
            .flatten();
        args.extend(caller_args(&spec, std::process::id(), log_dir.as_deref()));

        // The one notification this server's readiness turns on, recorded by the
        // transport's reader as state. Built once and shared with the readiness
        // watcher below: the matcher may be stateful, and two of them would each
        // see only part of the stream.
        let readiness = spec.readiness;
        let watch_for = readiness_filter(readiness);

        let transport = Transport::spawn(
            &Launch {
                program: spec.program.clone(),
                args,
                cwd: root.to_path_buf(),
                env: spec.env.clone(),
            },
            watch_for.clone(),
        )
        .map_err(|error| StartFailure::Spawn(format!("{error:#}")))?;

        // Subscribed before `initialize` goes out, not after `initialized`: a
        // server may start reporting progress the moment it is asked to
        // initialise. The sticky signal below covers the same window and does
        // not depend on this subscription keeping up, but a receiver created
        // here still costs nothing and reads the stream from the start.
        let notifications = transport.notifications();

        let root_name = root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace");
        // Our own pid, so the server exits if this process dies without getting
        // to `shutdown`. A fabricated one would make it exit when a stranger's
        // process ends.
        let params = initialize_params(Some(std::process::id()), &root_uri, root_name);
        let result = transport
            .request(method::INITIALIZE, params, spec.timeouts.first_request)
            .await
            .map_err(StartFailure::Handshake)?;

        let capabilities = ServerCapabilities::from_initialize_result(&result)
            .map_err(StartFailure::Capabilities)?;

        // The two gates that condemn the whole server. Shut it down politely
        // rather than dropping it: it is healthy, it is simply not one we can
        // use, and leaving `Microsoft.CodeAnalysis.LanguageServer.exe` to be
        // killed by the `Drop` backstop is a worse way to say so.
        if !capabilities.encoding_is_utf16() {
            let encoding = capabilities
                .position_encoding
                .clone()
                .unwrap_or_else(|| "an unnamed encoding".to_string());
            transport.shutdown().await;
            return Err(StartFailure::UnsupportedEncoding { encoding });
        }
        if capabilities.sync == SyncKind::None {
            transport.shutdown().await;
            return Err(StartFailure::NoDocumentSync);
        }

        let ready = spawn_readiness_watch(
            spec.readiness,
            watch_for.clone(),
            notifications,
            transport.signal(),
            ceiling,
            transport.watch_death(),
        );

        transport
            .notify(method::INITIALIZED, json!({}))
            .map_err(StartFailure::Handshake)?;

        Ok(Self {
            spec,
            root: root.to_path_buf(),
            root_uri,
            transport,
            capabilities,
            ready,
            documents: Mutex::new(HashMap::new()),
            first_request: AtomicBool::new(true),
        })
    }

    pub fn spec(&self) -> &ServerSpec {
        &self.spec
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn root_uri(&self) -> &str {
        &self.root_uri
    }

    /// What the server advertised. The gate every request is checked against.
    pub fn capabilities(&self) -> &ServerCapabilities {
        &self.capabilities
    }

    pub fn pid(&self) -> Option<u32> {
        self.transport.pid()
    }

    /// The death, if the server has one. `None` means still usable.
    pub fn death(&self) -> Option<Death> {
        self.transport.death()
    }

    /// The server's last words. Usually the only thing that explains a
    /// start-up failure.
    pub fn stderr_tail(&self) -> Vec<String> {
        self.transport.stderr_tail()
    }

    /// Whether the server's answers can be trusted yet, right now.
    pub fn readiness(&self) -> ReadyState {
        self.ready.borrow().clone()
    }

    /// Wait until the server is ready, it dies, or `deadline` passes.
    ///
    /// Returns whatever the state is at that point — including
    /// [`ReadyState::Loading`], which is a real answer and not a failure to
    /// produce one.
    pub async fn wait_ready(&self, deadline: Duration) -> ReadyState {
        // The current value is read through the *same* handle that is then
        // awaited, and read with `borrow_and_update`, so anything published
        // after this line is guaranteed to wake `changed()` below. Reading it
        // through a second handle would leave a window in which readiness
        // arrives, is marked seen by a later `clone`, and is then waited out.
        let mut ready = self.ready.clone();
        let current = ready.borrow_and_update().clone();
        if current.is_ready() {
            return current;
        }

        // A read-only handle for the arms that report whatever state we end on.
        let fallback = self.ready.clone();
        let mut death = self.transport.watch_death();
        let becomes_ready = async {
            loop {
                if ready.changed().await.is_err() {
                    // The watcher task is gone; whatever it last published
                    // stands, and the arms below read it.
                    return std::future::pending::<ReadyState>().await;
                }
                let state = ready.borrow_and_update().clone();
                if state.is_ready() {
                    return state;
                }
            }
        };

        tokio::select! {
            state = becomes_ready => state,
            // A dead server will never become ready, and waiting out the
            // deadline against a corpse just delays the real error.
            () = died(&mut death) => fallback.borrow().clone(),
            () = tokio::time::sleep(deadline) => fallback.borrow().clone(),
        }
    }

    /// `shutdown`, `exit`, close stdin, then kill the tree. Delegated whole:
    /// process lifetime is the transport's business, not this layer's.
    pub async fn shutdown(&self) {
        self.transport.shutdown().await;
    }

    // -----------------------------------------------------------------------
    // Document sync
    // -----------------------------------------------------------------------

    /// Tell the server about a document and its contents, under `version`.
    ///
    /// Opening a document that is already open would be a protocol violation,
    /// so it is sent as a change instead. The client owns the mirror; keeping
    /// the server's view legal is therefore its job and not the caller's.
    ///
    /// See [`Client::did_change`] for where `version` comes from and why it is
    /// an argument rather than a counter kept here.
    pub fn did_open(&self, path: &Path, text: &str, version: i32) -> Result<(), RequestError> {
        let uri = self.uri_for(path)?;
        let already_open = self.documents.lock().unwrap().contains_key(path);
        if already_open {
            return self.did_change(path, text, version);
        }

        // Per file, not per server: a `.tsx` opened as `typescript` is parsed as
        // TypeScript, and `documentSymbol` then reports declarations that are
        // really the innards of a JSX expression. See `ServerSpec::language_id_for`.
        let params =
            DidOpenTextDocumentParams::new(&uri, self.spec.language_id_for(path), version, text);
        self.send(method::DID_OPEN, &params)?;
        self.documents.lock().unwrap().insert(
            path.to_path_buf(),
            Sent {
                version,
                end: document_end(text),
            },
        );
        Ok(())
    }

    /// Replace what the server has for a document, under `version`.
    ///
    /// Refused for a document that was never opened: the server would reject
    /// the change, and opening it here on the caller's behalf would send text
    /// under a version nobody agreed on.
    ///
    /// # Why the version is an argument
    ///
    /// This used to be a private counter here — `1` on `did_open`, `+1` on each
    /// change, forgotten on `did_close`. [`super::documents::Documents`] keeps
    /// its own, under a high-water rule that survives a close so a version can
    /// never go backwards, and it is the mirror that decides what is owed to
    /// whom. Two counters for one number meant the mirror's was an authority
    /// nobody consulted: after a close and re-open it said 3 while the wire said
    /// 1, and the next person to read it — to log a desync, to decide whether a
    /// change is stale — would have got a number that was never sent.
    ///
    /// So there is one counter and it lives in the mirror. What is recorded here
    /// is now only *what was last sent*, which is exactly what
    /// [`Client::document_version`] claims to return. Monotonicity is the
    /// caller's guarantee, and `documents.rs` is where it is enforced and pinned.
    ///
    /// # Why the range comes from here
    ///
    /// The notification replaces a *range* of the server's buffer, so that range
    /// has to describe the buffer the server currently holds — the text of the
    /// previous notification, not the text of this one. This client is the only
    /// thing that knows what that was, so it records the extent alongside the
    /// version and hands it back here. Measuring the range from the new text
    /// instead crashes tsserver on any edit that lengthens the document and, far
    /// worse, silently leaves a stale tail on any edit that shortens it; see
    /// [`DidChangeTextDocumentParams::whole_document`].
    pub fn did_change(&self, path: &Path, text: &str, version: i32) -> Result<(), RequestError> {
        let uri = self.uri_for(path)?;
        let previous_end = {
            let mut documents = self.documents.lock().unwrap();
            let slot = documents
                .get_mut(path)
                .ok_or_else(|| RequestError::NotOpen {
                    path: path.to_path_buf(),
                })?;
            let previous_end = slot.end;
            // Recorded before the send, and left recorded if the send fails: a
            // version must never be reused, and a server that missed one
            // notices the gap. Reusing it would look like a legal edit.
            //
            // The extent moves with it. If the send fails the server never got
            // this text, so the *next* range would be measured against a buffer
            // it does not have — but a failed send is a dead transport (see
            // `send`), and every request after it fails too, so there is no next
            // range. Keeping the two fields in step is what matters.
            *slot = Sent {
                version,
                end: document_end(text),
            };
            previous_end
        };

        let params = DidChangeTextDocumentParams::whole_document(&uri, version, previous_end, text);
        self.send(method::DID_CHANGE, &params)
    }

    /// Tell the server to forget a document. It goes back to the file on disk.
    pub fn did_close(&self, path: &Path) -> Result<(), RequestError> {
        let uri = self.uri_for(path)?;
        let known = self.documents.lock().unwrap().remove(path).is_some();
        if !known {
            return Err(RequestError::NotOpen {
                path: path.to_path_buf(),
            });
        }
        self.send(method::DID_CLOSE, &DidCloseTextDocumentParams::new(&uri))
    }

    /// Which version the server was last told about, if the document is open.
    pub fn document_version(&self, path: &Path) -> Option<i32> {
        self.documents
            .lock()
            .unwrap()
            .get(path)
            .map(|sent| sent.version)
    }

    // -----------------------------------------------------------------------
    // The five questions
    // -----------------------------------------------------------------------

    /// Every use site of the symbol at `position`.
    ///
    /// `include_declaration` is the caller's choice: with it, a list of one
    /// means "declared, never used"; without it, "used once".
    pub async fn references(
        &self,
        path: &Path,
        position: Position,
        include_declaration: bool,
    ) -> Result<Vec<Location>, RequestError> {
        let method = method::REFERENCES;
        self.require(self.capabilities.references, method, "referencesProvider")?;
        let uri = self.uri_for(path)?;
        let params = ReferenceParams::new(&uri, position, include_declaration);
        let value = self
            .ask(method, as_params(&params), self.spec.timeouts.request)
            .await?;
        decode_goto(value).map_err(|error| malformed(method, error))
    }

    pub async fn definition(
        &self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<Location>, RequestError> {
        self.goto(
            method::DEFINITION,
            self.capabilities.definition,
            "definitionProvider",
            path,
            position,
        )
        .await
    }

    pub async fn implementation(
        &self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<Location>, RequestError> {
        self.goto(
            method::IMPLEMENTATION,
            self.capabilities.implementation,
            "implementationProvider",
            path,
            position,
        )
        .await
    }

    pub async fn type_definition(
        &self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<Location>, RequestError> {
        self.goto(
            method::TYPE_DEFINITION,
            self.capabilities.type_definition,
            "typeDefinitionProvider",
            path,
            position,
        )
        .await
    }

    /// Every declaration in a document, flattened, with its enclosing chain.
    pub async fn document_symbols(&self, path: &Path) -> Result<Vec<Symbol>, RequestError> {
        let method = method::DOCUMENT_SYMBOL;
        self.require(
            self.capabilities.document_symbol,
            method,
            "documentSymbolProvider",
        )?;
        let uri = self.uri_for(path)?;
        let params = DocumentSymbolParams::new(&uri);
        let value = self
            .ask(
                method,
                as_params(&params),
                self.spec.timeouts.document_symbol,
            )
            .await?;
        decode_document_symbols(value).map_err(|error| malformed(method, error))
    }

    // -----------------------------------------------------------------------
    // Shared machinery
    // -----------------------------------------------------------------------

    /// The three goto requests differ only in their method name and the
    /// capability that gates them; one body, so they cannot drift apart.
    async fn goto(
        &self,
        method: &'static str,
        provided: bool,
        capability: &'static str,
        path: &Path,
        position: Position,
    ) -> Result<Vec<Location>, RequestError> {
        self.require(provided, method, capability)?;
        let uri = self.uri_for(path)?;
        let params = TextDocumentPositionParams::new(&uri, position);
        let value = self
            .ask(method, as_params(&params), self.spec.timeouts.request)
            .await?;
        decode_goto(value).map_err(|error| malformed(method, error))
    }

    fn require(
        &self,
        provided: bool,
        method: &'static str,
        capability: &'static str,
    ) -> Result<(), RequestError> {
        if provided {
            return Ok(());
        }
        Err(RequestError::Unsupported { method, capability })
    }

    /// Wait for readiness, then ask, then remember that the first answer has
    /// been paid for.
    ///
    /// **The answer does not carry the state it was taken under.** A result
    /// obtained from a server promoted at the ceiling
    /// ([`ReadyState::ReadyWithCaveat`]) is byte-identical to one from a server
    /// that said it was ready, and `Result<_, RequestError>` has no channel for
    /// the difference. Every caller that renders a *count* must therefore read
    /// [`Client::readiness`] alongside the result and qualify it — **before**
    /// awaiting the answer, not after. The caveat is withdrawn if the signal it
    /// named eventually arrives (see [`watch_readiness`]), so a state read taken
    /// afterwards can miss a caveat that was in force while the request was in
    /// flight. Read first and the qualification can only ever be too cautious,
    /// which is the direction this subsystem errs in. `session::prepare` is that
    /// caller and reads it that way round.
    async fn ask(
        &self,
        method: &'static str,
        params: Value,
        base: Duration,
    ) -> Result<Value, RequestError> {
        let first = self.first_request.load(Ordering::Acquire);
        let deadline = deadline_for(&self.spec.timeouts, base, first);

        // The readiness wait and the request each get the full deadline. They
        // are two different waits — a server that is loading has not been asked
        // anything yet — and splitting one budget between them would make a
        // slow load look like a slow answer.
        if !self.wait_ready(deadline).await.is_ready() && self.transport.death().is_none() {
            return Err(RequestError::StillLoading {
                method,
                waited: deadline,
            });
        }

        let answer = self.transport.request(method, params, deadline).await;
        if answer.is_ok() {
            // Only on a real answer. A first request that timed out has not
            // established that the server has finished paying its start-up
            // cost, and demoting the deadline would make the next one fail too.
            self.first_request.store(false, Ordering::Release);
        }
        answer.map_err(|failure| RequestError::Failed { method, failure })
    }

    fn send(&self, method: &'static str, params: &impl Serialize) -> Result<(), RequestError> {
        self.transport
            .notify(method, as_params(params))
            .map_err(|failure| RequestError::Failed { method, failure })
    }

    fn uri_for(&self, path: &Path) -> Result<String, RequestError> {
        to_file_uri(path, self.spec.uri_style).ok_or_else(|| RequestError::UnusableUri {
            path: path.to_path_buf(),
        })
    }
}

/// Create this workspace's language-server log directory, or report that we
/// could not.
///
/// `None` means the server is started without `--extensionLogDirectory`, which
/// costs its logs and nothing else: a server that cannot write logs still
/// answers questions, and refusing to start one over a directory would turn a
/// diagnostic convenience into an outage. The absence is logged because the
/// logs' whole purpose is to be there when something else goes wrong — an hour
/// of this feature's diagnosis was spent on a server that had none.
fn prepare_log_dir(root: &Path) -> Option<PathBuf> {
    let dir = crate::config::lsp_log_dir(root);
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {
            // Before the server starts, so the file it is about to write is not a
            // candidate and no handle we might need is closed under it.
            let removed = prune_log_dir(&dir, LOG_KEEP, LOG_MAX_BYTES);
            if removed > 0 {
                tracing::debug!(removed, "pruned old language server logs");
            }
            Some(dir)
        }
        Err(error) => {
            tracing::warn!(
                directory = %dir.display(),
                %error,
                "could not create the language server log directory; starting the server without one"
            );
            None
        }
    }
}

/// How many launches' logs to keep, and how many bytes they may take.
///
/// Both, because either can bind first: a workspace opened fifty times leaves
/// fifty small logs, and one server that logged a stack trace per file leaves a
/// single large one. The numbers are small on purpose — these are diagnostics for
/// the launch that just happened, and the newest is the only one anybody has
/// asked for so far.
const LOG_KEEP: usize = 10;
const LOG_MAX_BYTES: u64 = 32 * 1024 * 1024;

/// Delete all but the newest logs, and return how many went.
///
/// Newest-first by modified time, keeping at most `keep` files and at most
/// `max_bytes` of them; the newest file is kept whatever its size, because
/// deleting the only log there is would defeat the point of having a directory.
/// Top-level files only — a subdirectory is not something this code created and
/// is not something it will remove.
///
/// Every failure is ignored rather than reported: another server may hold a
/// handle to a file it is still writing (which on Windows can refuse the delete),
/// and a workspace that cannot be tidied must still get its language server.
fn prune_log_dir(dir: &Path, keep: usize, max_bytes: u64) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut logs: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            Some((
                meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                meta.len(),
                entry.path(),
            ))
        })
        .collect();
    // Newest first, so both limits collapse into one question: how many of the
    // leading logs survive? Ties break on the path, so the answer is stable.
    logs.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));

    let mut running = 0u64;
    let mut removed = 0;
    for (index, (_, size, path)) in logs.iter().enumerate() {
        running = running.saturating_add(*size);
        if index < keep && (index == 0 || running <= max_bytes) {
            continue;
        }
        if std::fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Serialise params, or send `null` rather than panicking.
///
/// Every type this is called with is a struct of strings and numbers, so it
/// cannot fail. If it somehow did, a `null` params reaches the server and comes
/// back as an error the caller sees — which is a better outcome than taking the
/// process down on a reader task.
fn as_params(params: &impl Serialize) -> Value {
    serde_json::to_value(params).unwrap_or(Value::Null)
}

/// Rewrite a decode failure so it names the request that was actually asked.
///
/// [`decode_goto`] hard-codes `textDocument/definition` in its error, because
/// one decoder serves four requests and is not told which one it is decoding.
/// Letting that through would tell a user their *definition* lookup failed when
/// they asked where a type is declared. The decoder's own `what` is therefore
/// discarded here and the caller's method used instead; its `detail` — which
/// key set arrived — is the part worth keeping.
fn malformed(method: &'static str, error: DecodeError) -> RequestError {
    let DecodeError::Shape { detail, .. } = error;
    RequestError::Failed {
        method,
        failure: RequestFailure::Malformed {
            method: method.to_string(),
            detail,
        },
    }
}

/// The deadline for one request.
///
/// The first answer waits for project load or cache priming; the rest do not.
/// One shared number would be either far too long for a steady-state request
/// (so a dead server looks slow) or far too short for the first (so a healthy
/// server looks dead). `max` rather than a straight swap, because
/// `document_symbol`'s own budget could in principle be the larger of the two.
fn deadline_for(timeouts: &Timeouts, base: Duration, first: bool) -> Duration {
    if first {
        base.max(timeouts.first_request)
    } else {
        base
    }
}

// ---------------------------------------------------------------------------
// Readiness
// ---------------------------------------------------------------------------

/// Start watching for the server's own "I am ready" signal.
///
/// [`Readiness::Immediate`] needs no watcher at all: the channel is created
/// already ready and its sender dropped, and a `watch` receiver keeps the last
/// value after that.
fn spawn_readiness_watch(
    readiness: Readiness,
    matches: Option<SignalFilter>,
    notifications: broadcast::Receiver<Notification>,
    signal: watch::Receiver<Option<Notification>>,
    ceiling: Duration,
    death: watch::Receiver<Option<Death>>,
) -> watch::Receiver<ReadyState> {
    // `None` is exactly `Immediate` — the two are decided in one place,
    // `readiness_filter`, so they cannot drift apart.
    let Some(matches) = matches else {
        let (_, rx) = watch::channel(ReadyState::Ready);
        return rx;
    };

    let (tx, rx) = watch::channel(ReadyState::Loading);
    tokio::spawn(watch_readiness(
        readiness,
        matches,
        notifications,
        signal,
        ceiling,
        death,
        tx,
    ));
    rx
}

/// Wait for the signal, then publish readiness — from two sources for the same
/// message.
///
/// `signal` is the transport's sticky record, written by the reader as it
/// decoded the frame, and it is the one that has to be right: the signal is
/// sent **once**, so a subscriber that lagged past it, or that had not started
/// when it arrived, would otherwise wait out the ceiling against a server that
/// had already said it was ready. `notifications` is the live stream, kept
/// because it is what makes the sticky state observably *not* the only path.
///
/// The task outlives the ceiling. It publishes at most twice — the caveat when
/// the ceiling fires, then plain [`ReadyState::Ready`] if the signal turns up
/// after all — and ends on the first of the signal, the server's death, or the
/// transport closing.
async fn watch_readiness(
    readiness: Readiness,
    matches: SignalFilter,
    mut notifications: broadcast::Receiver<Notification>,
    mut signal: watch::Receiver<Option<Notification>>,
    ceiling: Duration,
    mut death: watch::Receiver<Option<Death>>,
    ready: watch::Sender<ReadyState>,
) {
    // Read through the same handle that is then awaited, so a signal published
    // between the two cannot be marked seen and then waited for.
    let recorded = async {
        loop {
            if signal.borrow_and_update().is_some() {
                return true;
            }
            if signal.changed().await.is_err() {
                // The transport is gone, so nothing will ever be recorded. The
                // live stream and the death arm still decide the outcome.
                return std::future::pending::<bool>().await;
            }
        }
    };

    let streamed = async {
        loop {
            match notifications.recv().await {
                Ok(notification) if matches(&notification) => return true,
                Ok(_) => {}
                // Lag costs whatever was dropped, and that is accepted: a chatty
                // but healthy server must not be stranded on "loading" because
                // this task fell behind. What it must not cost is the signal
                // itself, and it cannot — the reader recorded that as state
                // before it was ever published here, and `recorded` above reads
                // it. Roslyn sends thousands of progress notifications before
                // the one message that matters, so this arm lagging past it is
                // the normal case and not the exceptional one.
                Err(broadcast::error::RecvError::Lagged(dropped)) => {
                    tracing::debug!(dropped, "fell behind the server's notifications");
                }
                // Only reachable once the `Client` itself is gone: the
                // broadcast sender is a field on the transport, so a dead
                // *server* never closes it. The death is watched separately
                // below for exactly that reason.
                Err(broadcast::error::RecvError::Closed) => return false,
            }
        }
    };

    let signalled = async {
        tokio::select! {
            // Biased so a recorded signal wins a tie: it is the arm that cannot
            // have missed anything, and both arms report the same message.
            biased;
            seen = recorded => seen,
            seen = streamed => seen,
        }
    };

    // The ceiling and the signal are watched together for the whole life of the
    // task, and death is a first-class arm of the same `select!` rather than a
    // channel side effect.
    //
    // The ceiling firing is not the end of the wait. It says how long we were
    // prepared to wait, and a solution that takes longer than the ceiling to load
    // still finishes loading — at which point the caveat is no longer true and is
    // withdrawn. Stopping at the caveat left every count in the workspace
    // rendered as a lower bound for the life of the app, with exact numbers
    // available the whole time.
    //
    // Death, by contrast, ends the task without publishing anything: "ready, with
    // a caveat" said about an exited process would render a corpse as a working
    // server in the one place a UI looks to decide whether to stop showing
    // "loading", and a signal that arrives after the process is gone does not
    // make its answers trustworthy either.
    tokio::pin!(signalled);
    let ceiling_reached = tokio::time::sleep(ceiling);
    tokio::pin!(ceiling_reached);
    let mut caveated = false;

    loop {
        tokio::select! {
            seen = &mut signalled => {
                if seen {
                    let _ = ready.send(ReadyState::Ready);
                }
                return;
            }
            // Disarmed once it has fired: `sleep` stays ready for ever after its
            // deadline, so an unguarded arm would spin.
            () = &mut ceiling_reached, if !caveated => {
                caveated = true;
                let detail = format!(
                    "the server did not send {} within {ceiling:?}. It is being treated as \
                     ready, so its answers may be incomplete.",
                    awaited_signal(readiness)
                );
                tracing::warn!(%detail, "language server readiness ceiling reached");
                let _ = ready.send(ReadyState::ReadyWithCaveat { detail });
            }
            () = died(&mut death) => {
                tracing::debug!(
                    caveated,
                    "the server died; its readiness will not be revised again"
                );
                return;
            }
        }
    }
}

/// Whether this notification is the one being waited for.
///
/// Abstains on anything it cannot read: a progress token that is a number
/// cannot be prefix-matched, so it is not a match. Guessing here would declare
/// a still-priming index trustworthy, which is exactly the wrong-count failure
/// the readiness states exist to prevent.
fn signals_ready(readiness: Readiness, notification: &Notification) -> bool {
    match readiness {
        // Never reached — no watcher is spawned for it — and `true` is still
        // the honest answer: an immediate server is ready whatever arrives.
        Readiness::Immediate => true,
        Readiness::RoslynProjectInit => notification.method == PROJECT_INITIALIZATION_COMPLETE,
        Readiness::FirstDiagnostics => notification.method == method::PUBLISH_DIAGNOSTICS,
        Readiness::Progress { token_prefix } => {
            if notification.method != method::PROGRESS {
                return false;
            }
            let Some(params) = notification.params.as_ref() else {
                return false;
            };
            let token = params.get("token").and_then(Value::as_str);
            let kind = params.pointer("/value/kind").and_then(Value::as_str);
            // Only `end`. A `begin` or a `report` for the very token we are
            // waiting on means the work is *running*, which is the opposite.
            matches!((token, kind), (Some(token), Some("end")) if token.starts_with(token_prefix))
        }
        // Stateless matching cannot answer this one: see [`TitledProgress`].
        // Reached only by a caller that ignored `readiness_filter`, and `false`
        // is the safe answer — it waits rather than promoting.
        Readiness::ProgressTitle { .. } => false,
    }
}

/// The one matcher for a server's readiness signal, shared by the transport's
/// sticky record and by the live stream.
///
/// One `Arc` rather than one closure each, because [`Readiness::ProgressTitle`]
/// is **stateful**: two independent matchers would each see only some of the
/// notifications and neither would reliably pair a `begin` with its `end`.
///
/// `None` for [`Readiness::Immediate`], which waits for nothing, so there is
/// nothing for the transport's reader to keep.
fn readiness_filter(readiness: Readiness) -> Option<SignalFilter> {
    match readiness {
        Readiness::Immediate => None,
        Readiness::ProgressTitle { title_prefix } => {
            let state = TitledProgress::new(title_prefix);
            Some(Arc::new(move |notification: &Notification| {
                state.matches(notification)
            }))
        }
        readiness => Some(Arc::new(move |notification: &Notification| {
            signals_ready(readiness, notification)
        })),
    }
}

/// Pairs a `$/progress` `end` with the `begin` that named the work.
///
/// Stateful because the protocol leaves no alternative. An `end` value carries
/// `kind` and nothing else — no title, no description — so the only way to know
/// *what* ended is to have remembered the token when its `begin` went past. A
/// server that mints an opaque token per unit of work therefore cannot be
/// matched by [`Readiness::Progress`] at all, and matching every `end` instead
/// would promote at the first piece of work the server happened to finish.
///
/// Tokens are keyed by their JSON spelling, so the legal numeric token `7` and
/// the equally legal string token `"7"` stay distinct.
struct TitledProgress {
    title_prefix: &'static str,
    /// Tokens whose `begin` matched, awaiting their `end`.
    ///
    /// Capped: a server that begins matching work and never ends it would
    /// otherwise grow this without bound for the life of the process. Past the
    /// cap new tokens are dropped rather than evicting old ones — the earliest
    /// match is the one readiness is waiting for, and forgetting it to make room
    /// for a later one is the one outcome that would strand the server on
    /// "loading" forever.
    watching: Mutex<HashSet<String>>,
}

impl TitledProgress {
    /// Far above the one or two a real server has in flight, far below anything
    /// that matters for memory.
    const MAX_TRACKED: usize = 64;

    fn new(title_prefix: &'static str) -> Self {
        Self {
            title_prefix,
            watching: Mutex::new(HashSet::new()),
        }
    }

    fn matches(&self, notification: &Notification) -> bool {
        if notification.method != method::PROGRESS {
            return false;
        }
        let Some(params) = notification.params.as_ref() else {
            return false;
        };
        let Some(token) = params.get("token") else {
            return false;
        };
        let token = token.to_string();
        let kind = params.pointer("/value/kind").and_then(Value::as_str);

        let mut watching = match self.watching.lock() {
            Ok(watching) => watching,
            // The reader task is the only writer, so this is unreachable in
            // practice; carrying on with the last consistent set beats refusing
            // to ever report readiness again.
            Err(poisoned) => poisoned.into_inner(),
        };
        match kind {
            Some("begin") => {
                let titled = params
                    .pointer("/value/title")
                    .and_then(Value::as_str)
                    .is_some_and(|title| title.starts_with(self.title_prefix));
                if titled && watching.len() < Self::MAX_TRACKED {
                    watching.insert(token);
                }
                // A beginning is the opposite of ready, however well it matches.
                false
            }
            Some("end") => watching.contains(&token),
            _ => false,
        }
    }
}

/// What the ceiling gave up waiting for, in the caveat's own words.
fn awaited_signal(readiness: Readiness) -> String {
    match readiness {
        Readiness::Immediate => "anything (it was ready already)".to_string(),
        Readiness::RoslynProjectInit => format!("`{PROJECT_INITIALIZATION_COMPLETE}`"),
        Readiness::FirstDiagnostics => format!("any `{}`", method::PUBLISH_DIAGNOSTICS),
        Readiness::Progress { token_prefix } => format!(
            "a `{}` end for a token beginning `{token_prefix}`",
            method::PROGRESS
        ),
        Readiness::ProgressTitle { title_prefix } => format!(
            "a `{}` end for the work titled `{title_prefix}`",
            method::PROGRESS
        ),
    }
}

/// Resolve when the server dies; never, if it does not.
///
/// Never resolving is correct: this future exists only to lose a `select!`
/// race, and fabricating a death to end it would report one.
async fn died(rx: &mut watch::Receiver<Option<Death>>) {
    loop {
        let dead = rx.borrow_and_update().is_some();
        if dead {
            return;
        }
        if rx.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod client_tests;
