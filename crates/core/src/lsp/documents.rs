//! What each server has been told about each open document.
//!
//! Pure: no process, no async, no I/O. This decides *what notifications are
//! owed* and returns them; it never sends anything. That is the whole design.
//! Document synchronisation is a set of ordering rules — open before change,
//! never open twice, never let a version go backwards, catch a late server up —
//! and every one of them is a rule about a sequence of messages rather than
//! about a message. Rules like that are provable in microseconds against a
//! struct and are close to unprovable against a real server, where the failure
//! mode is a server that answers *slightly* wrongly and never says why.
//!
//! # Why the mirror exists at all
//!
//! A language server answers about the buffer it was told about. Nothing else.
//! It does not read the file back off disk before answering, and it has no way
//! to notice that the editor moved on. So a usages request issued against a
//! buffer the server believes is two edits old resolves the *position* in the
//! old text: the caret the user put on `Total` lands somewhere else, and the
//! answer that comes back is a real, confident, correctly-formatted list of the
//! use sites of a different symbol. There is no error anywhere in that chain.
//! Keeping this mirror, and flushing it before a request goes out, is the only
//! thing standing between the feature and that answer.
//!
//! # The rules, each of which has a test named for it
//!
//! * **`didOpen` is idempotent per (document, server).** A second `didOpen` for
//!   a document a server already has open is a protocol violation, and some
//!   servers respond by discarding what they knew about it. Re-opening is
//!   therefore turned into a change.
//! * **Versions rise monotonically per document and never restart** — including
//!   across a close and a re-open. A version that goes backwards makes a server
//!   drop the change *silently*, which is the worst available outcome: the
//!   buffer is then permanently desynchronised with nothing reporting it.
//! * **A server that has just started is told the whole open set**
//!   ([`Documents::replay_for`]). This is the rule that breaks in the app and
//!   not in a test. Roslyn takes tens of seconds to come up; every file the user
//!   opened while it was starting is invisible to it *forever* unless it is
//!   replayed, and "invisible" surfaces as an empty answer rather than as an
//!   error. A restarted server is the same case.
//! * **A close tells exactly the servers that had the document open**, and no
//!   others.
//! * **A change to a document nobody opened yields no actions**, rather than a
//!   fabricated open. The caller has a bug; inventing an open would send a
//!   server text under a version nobody agreed on and hide the bug behind an
//!   answer that happens to look right.
//!
//! # What this module is *not*
//!
//! It is not the authority on process lifetime, on URIs, or on whether a server
//! may be asked something — those are [`super::transport`], [`super::uri`] and
//! [`super::client`]. It does not know that [`super::client::Client`] keeps a
//! version counter of its own; see the note on [`SyncAction`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Which server an action is owed to.
///
/// `&'static str` rather than an owned string because that is exactly what
/// [`super::registry::ServerSpec::id`] is: every id in this app is a compile-time
/// constant naming one supported server, so an owned copy per document per
/// server would buy nothing. The conversion to `String` happens once, at the IPC
/// boundary, in [`super::model::ServerStatus`].
pub type ServerId = &'static str;

/// One notification that is owed to one server.
///
/// Returned, never sent. The caller — [`super::session`] — turns each of these
/// into the matching [`super::client::Client`] call.
///
/// **The `version` here is this mirror's, and `Client` keeps its own.**
/// `Client::did_open`/`did_change` number their notifications from a private map
/// and do not accept a version argument, so the number that actually reaches the
/// wire today is the client's. The two agree on the only property a server cares
/// about — that the number rises — and they will disagree on its value after a
/// close and re-open, because `Client` forgets a document when it is closed and
/// this mirror deliberately does not. If a server is ever seen to complain about
/// a version, the fix is to make `Client` take the version from the action
/// rather than to make this mirror restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAction {
    DidOpen {
        server: ServerId,
        path: PathBuf,
        text: String,
        version: i32,
    },
    DidChange {
        server: ServerId,
        path: PathBuf,
        text: String,
        version: i32,
    },
    DidClose {
        server: ServerId,
        path: PathBuf,
    },
}

impl SyncAction {
    /// Which server this is owed to. Convenient for a caller routing a batch.
    pub fn server(&self) -> ServerId {
        match self {
            Self::DidOpen { server, .. }
            | Self::DidChange { server, .. }
            | Self::DidClose { server, .. } => server,
        }
    }

    /// Which document this is about.
    pub fn path(&self) -> &Path {
        match self {
            Self::DidOpen { path, .. }
            | Self::DidChange { path, .. }
            | Self::DidClose { path, .. } => path,
        }
    }
}

/// One open document, and who has been told about it.
#[derive(Debug, Clone)]
struct Document {
    version: i32,
    text: String,
    /// A `BTreeSet` so a batch of actions comes out in a fixed order. An
    /// unstable order would make every multi-server test's expectation depend on
    /// hash seeding, and a test that has to sort its own input cannot pin an
    /// ordering rule.
    open_in: BTreeSet<ServerId>,
}

/// The mirror of what every server has been told, and nothing else.
#[derive(Debug, Default)]
pub struct Documents {
    /// Only the documents that are open right now. A `BTreeMap` for the same
    /// reason as `open_in`: [`Documents::replay_for`] walks it, and replay order
    /// has to be reproducible.
    open: BTreeMap<PathBuf, Document>,
    /// The highest version ever used for a path, kept **after** the document is
    /// closed. This is what makes a version survive a close and re-open, and it
    /// is the reason this map is not merged into `open`.
    ///
    /// It grows by one entry per path the user has ever opened and is never
    /// pruned, which is a few dozen bytes against a rule whose violation
    /// desynchronises a buffer with no error anywhere.
    high_water: BTreeMap<PathBuf, i32>,
}

impl Documents {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tell `servers` about `path`, whose contents are `text`.
    ///
    /// Per server: a `didOpen` if that server does not have it open, a
    /// `didChange` if it does. Both carry the same, newly advanced version — the
    /// document has one state, and two servers must not be told different
    /// numbers for it.
    ///
    /// Servers that had it open but are **not** named in this call keep it open
    /// and are told about the new text: they still believe the old one, and
    /// leaving them stale is exactly the desynchronisation this module exists to
    /// prevent. A shrinking server list is not a close, so nothing here closes
    /// anything.
    pub fn open(&mut self, path: &Path, text: &str, servers: &[ServerId]) -> Vec<SyncAction> {
        let version = self.advance(path);

        // Whoever already had it, plus whoever is being named now. The union is
        // walked rather than either half, because the two halves need *different*
        // notifications and the batch still has to come out in one fixed order.
        let already_open = self
            .open
            .get(path)
            .map(|document| document.open_in.clone())
            .unwrap_or_default();
        let mut union = already_open.clone();
        union.extend(servers.iter().copied());

        let document = self.open.entry(path.to_path_buf()).or_insert(Document {
            version,
            text: String::new(),
            open_in: BTreeSet::new(),
        });
        document.version = version;
        document.text = text.to_string();
        document.open_in = union.clone();

        union
            .into_iter()
            .map(|server| {
                if already_open.contains(&server) {
                    SyncAction::DidChange {
                        server,
                        path: path.to_path_buf(),
                        text: text.to_string(),
                        version,
                    }
                } else {
                    SyncAction::DidOpen {
                        server,
                        path: path.to_path_buf(),
                        text: text.to_string(),
                        version,
                    }
                }
            })
            .collect()
    }

    /// The document's contents changed.
    ///
    /// Yields no actions at all for a document nobody opened — see the module
    /// doc for why that is not promoted into an open.
    pub fn change(&mut self, path: &Path, text: &str) -> Vec<SyncAction> {
        if !self.open.contains_key(path) {
            return Vec::new();
        }
        let version = self.advance(path);
        // `contains_key` above; the entry cannot have gone away in between.
        let Some(document) = self.open.get_mut(path) else {
            return Vec::new();
        };
        document.version = version;
        document.text = text.to_string();
        document
            .open_in
            .iter()
            .map(|server| SyncAction::DidChange {
                server,
                path: path.to_path_buf(),
                text: text.to_string(),
                version,
            })
            .collect()
    }

    /// Forget the document, telling every server that had it open.
    ///
    /// The version is *not* forgotten. A closed and re-opened file continues
    /// from where it left off.
    pub fn close(&mut self, path: &Path) -> Vec<SyncAction> {
        let Some(document) = self.open.remove(path) else {
            return Vec::new();
        };
        document
            .open_in
            .into_iter()
            .map(|server| SyncAction::DidClose {
                server,
                path: path.to_path_buf(),
            })
            .collect()
    }

    /// Everything a server that has just started must be told to catch up.
    ///
    /// One `didOpen` per open document, in path order, carrying the version and
    /// the text the mirror currently holds. Versions are not advanced: the
    /// document did not change, a server that missed it simply needs the current
    /// state.
    ///
    /// This deliberately does **not** skip documents the set already records as
    /// open on this server, because the caller for this method is a server that
    /// has just started or just *re*started, and a restarted process remembers
    /// nothing. Skipping would leave the one case this method exists for — the
    /// restart — silently unserved.
    ///
    /// # Why `&mut self`
    ///
    /// The sketch this was written from had it as `&self`. It cannot be: a
    /// server that is replayed has the documents open from that moment on, and a
    /// query that does not record that leaves the server out of every subsequent
    /// [`Documents::change`] — so the late-starting server the method exists for
    /// receives the open set once and then never hears about an edit again. That
    /// failure is invisible (the server answers, just from stale text), which
    /// makes the borrow-checker complaint the caller gets from the honest
    /// signature the cheapest possible way to find out.
    pub fn replay_for(&mut self, server: ServerId) -> Vec<SyncAction> {
        self.open
            .iter_mut()
            .map(|(path, document)| {
                // Recorded as open *before* the caller has sent anything, so a
                // change arriving between the replay and the send still reaches
                // this server. The opposite order loses the edit entirely.
                document.open_in.insert(server);
                SyncAction::DidOpen {
                    server,
                    path: path.clone(),
                    text: document.text.clone(),
                    version: document.version,
                }
            })
            .collect()
    }

    /// What the servers currently believe this document contains.
    ///
    /// **This is the editor's buffer, not the file on disk**, and the difference
    /// is the entire point. [`super::results`] reads it for snippets and
    /// [`super::session`] flushes it before issuing a request, because a request
    /// aimed at a buffer the server thinks is older resolves the caret to a
    /// different symbol and returns a confident answer about the wrong one. Disk
    /// text would be wrong in exactly the same way for any unsaved edit, which
    /// is the normal state of a file somebody is asking questions about.
    ///
    /// `None` once the document is closed: there is then no buffer, and the
    /// servers have gone back to disk themselves.
    pub fn text(&self, path: &Path) -> Option<&str> {
        self.open.get(path).map(|document| document.text.as_str())
    }

    /// The version the servers were last told, while the document is open.
    ///
    /// `None` after a close — but the number is remembered internally, so the
    /// next open continues from it rather than restarting.
    pub fn version(&self, path: &Path) -> Option<i32> {
        self.open.get(path).map(|document| document.version)
    }

    /// Whether the mirror holds this document at all.
    pub fn is_open(&self, path: &Path) -> bool {
        self.open.contains_key(path)
    }

    /// Every open document, in path order.
    pub fn open_paths(&self) -> Vec<&Path> {
        self.open.keys().map(PathBuf::as_path).collect()
    }

    /// Which servers have this document open, in id order.
    pub fn servers_for(&self, path: &Path) -> Vec<ServerId> {
        self.open
            .get(path)
            .map(|document| document.open_in.iter().copied().collect())
            .unwrap_or_default()
    }

    /// The next version for `path`, recorded so it survives a close.
    ///
    /// Saturating rather than wrapping at [`i32::MAX`]: `+1` there would send
    /// `-2147483648`, which is a version going *backwards* — the one thing a
    /// server drops silently, leaving the buffer desynchronised with nothing
    /// anywhere reporting it. Repeating the ceiling value is a change the server
    /// may reject loudly, which is strictly the better failure.
    fn advance(&mut self, path: &Path) -> i32 {
        let slot = self.high_water.entry(path.to_path_buf()).or_insert(0);
        *slot = slot.saturating_add(1);
        *slot
    }

    /// Put a document's version at `version`, for the ceiling test only.
    ///
    /// The saturating rule at [`i32::MAX`] is unreachable through the public API
    /// — it needs two billion edits — and a rule that cannot be exercised is a
    /// rule that silently is not there. This seam is `#[cfg(test)]` so it cannot
    /// be reached from the app, where a caller setting a version by hand would
    /// be doing exactly the thing the monotonic rule forbids.
    #[cfg(test)]
    fn force_version(&mut self, path: &Path, version: i32) {
        if let Some(document) = self.open.get_mut(path) {
            document.version = version;
        }
        self.high_water.insert(path.to_path_buf(), version);
    }
}

#[cfg(test)]
#[path = "documents_tests.rs"]
mod documents_tests;
