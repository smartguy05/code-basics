//! The types that cross the IPC boundary for this feature.
//!
//! `src/ipc/types.ts` mirrors these by hand — there is no codegen — so the
//! `model_tests` module at the bottom pins the exact JSON keys and enum
//! spellings each type produces. A rename here that is not carried into
//! TypeScript surfaces as `undefined` in a dropdown rather than as an error,
//! and the pinning test is the only thing that catches it.
//!
//! # Two rules that are easy to "tidy" away, and must not be
//!
//! **No `skip_serializing_if`, anywhere in this file.** Every key is always
//! present and an absent value crosses as an explicit `null`, following the
//! [`crate::symbols::search::SearchHit`] precedent: "the backend has no answer"
//! must not be indistinguishable from "the backend forgot to send one".
//! `docs/architecture/ipc-contract.md` records this rule.
//!
//! **Lines and columns use different bases, deliberately.** `line` is 1-based
//! everywhere here, matching the editor gutter, [`crate::symbols`]' index and
//! the whole existing open-a-file-at-a-line chain; `character` is 0-based
//! UTF-16 code units, because there is no 1-based column convention anywhere in
//! this app and CodeMirror hands the caller exactly that. Both conversions go
//! through [`super::positions`] and nowhere else. The asymmetry is restated on
//! every field it touches — an undocumented mixed convention is how an
//! off-by-one ships.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::symbols::declarations::SymbolKind;

/// Why there is, or is not, an answer.
///
/// **Never collapse two of these into one.** A server that was never configured,
/// one that is still starting, one still loading its projects, one that died,
/// and one that does not advertise the capability are five different things to
/// tell somebody, and only [`Availability::Ready`] licenses a count. This is the
/// module-level abstain rule in its narrowest form: everything else in this
/// subsystem exists to keep these apart, so flattening them here would undo all
/// of it at the last step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Availability {
    /// No server is configured for this language, or the configured one was
    /// not found on disk. Actionable by the user, so it carries a hint.
    NotConfigured,
    /// The process is coming up and has not finished the handshake.
    Starting,
    /// Handshaken, but not yet primed — the answer would be wrong, not absent.
    Loading,
    /// The only outcome that licenses a count.
    Ready,
    /// The server died, errored, or answered something unreadable.
    Failed,
    /// The server is alive and healthy and does not offer this capability.
    Unsupported,
}

/// Every use site of one symbol, or the reason there is no list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UsageResult {
    pub outcome: Availability,
    /// The **true** number of distinct use sites.
    ///
    /// `None` unless `outcome == Ready`; `Some(0)` genuinely means "no usages",
    /// which is a real answer and not the same thing. When [`Self::truncated`]
    /// is set this still reports the full count — a cap that also caps the
    /// number is a lie, and the number is the claim the user acts on.
    pub total: Option<u32>,
    /// At most the cap's worth of rows. Empty whenever `total` is `None`.
    pub usages: Vec<Usage>,
    /// Whether [`Self::usages`] is shorter than [`Self::total`].
    pub truncated: bool,
    /// Why, in the user's terms, when the outcome is not `Ready` — **and the
    /// qualification when it is.**
    ///
    /// A `Ready` answer normally carries `None`. It carries a message when the
    /// count is real but needs saying something about: a server promoted at
    /// [`super::client::READINESS_CEILING`] answers with the rows it has, and the
    /// number may be low. `Ready` plus a message is therefore a legal and
    /// meaningful shape, and a reader that only renders this field for
    /// non-`Ready` outcomes drops exactly the qualification that keeps a count
    /// honest.
    pub message: Option<String>,
    /// Which server answered, for a status line. `None` when none did.
    pub server: Option<String>,
}

impl UsageResult {
    /// The shape for every outcome that is not an answer.
    ///
    /// Constructed rather than hand-written at each call site so that "no
    /// answer" can never accidentally ship a `total` of zero: there is exactly
    /// one place that decides `total` is `None`, and it is this one.
    pub fn unavailable(outcome: Availability, message: impl Into<String>) -> Self {
        Self {
            outcome,
            total: None,
            usages: Vec::new(),
            truncated: false,
            message: Some(message.into()),
            server: None,
        }
    }

    /// Name the server that produced (or failed to produce) this answer.
    #[must_use]
    pub fn with_server(mut self, server: impl Into<String>) -> Self {
        self.server = Some(server.into());
        self
    }
}

/// One row of the usages list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Workspace-relative, forward slashes, as [`crate::symbols::index`]
    /// records it.
    ///
    /// `None` when the location is outside the workspace or in a non-`file:`
    /// document — Roslyn really does answer with `source-generated:` and
    /// metadata URIs. The row still exists and is still counted; it just cannot
    /// be opened. Dropping it would make the count wrong, and joining the URI
    /// onto the root would open a different file.
    pub path: Option<PathBuf>,
    /// The relative path, or the raw URI when there is no path.
    pub label: String,
    /// **1-based**, matching the editor gutter. See the module docs.
    pub line: u32,
    /// One trimmed line of context — [`super::positions::snippet`]'s output.
    pub snippet: String,
    /// Where to underline inside [`Self::snippet`]. `None` when the match did
    /// not survive trimming: no underline is honest, an underline over the
    /// wrong characters is a claim.
    pub highlight: Option<Highlight>,
}

/// A span to underline, in **UTF-16 code units** offset into a snippet.
///
/// UTF-16 because the reader is JavaScript. [`super::positions::Snippet`] holds
/// the same span in *bytes*, and [`super::positions::byte_to_utf16`] is the only
/// thing that may convert between them.
///
/// A struct rather than a tuple struct on purpose: a tuple struct serialises as
/// a two-element **array**, and `types.ts` mirrors an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Highlight {
    pub start: u32,
    pub end: u32,
}

/// Where a symbol is declared, implemented, and typed.
///
/// Three lists rather than one because they answer three different questions,
/// and a symbol legitimately appears in more than one of them — an interface
/// method is a declaration from one angle and an implementation from another.
///
/// **One outcome for three lists.** A server that advertises `definitionProvider`
/// but not `implementationProvider` answers one group and refuses another, and
/// there is no per-group [`Availability`] to say so: the outcome stays
/// [`Availability::Ready`] and [`Self::message`] names the refused group. Only a
/// refusal of **all three** changes the outcome. So an empty group licenses "there
/// are none" *only* when `message` is `None` — otherwise it may be a question
/// nobody could ask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionResult {
    pub outcome: Availability,
    pub declarations: Vec<Target>,
    pub implementations: Vec<Target>,
    pub type_definitions: Vec<Target>,
    /// Why a group is empty, or the qualification a `Ready` answer needs. See
    /// [`UsageResult::message`]; the same rule applies here, and here it is also
    /// the *only* channel for a partially refused answer.
    pub message: Option<String>,
}

impl DefinitionResult {
    /// The shape for every outcome that is not an answer.
    pub fn unavailable(outcome: Availability, message: impl Into<String>) -> Self {
        Self {
            outcome,
            declarations: Vec::new(),
            implementations: Vec::new(),
            type_definitions: Vec::new(),
            message: Some(message.into()),
        }
    }
}

/// Somewhere to jump to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    /// Workspace-relative, forward slashes. `None` for the same reasons as
    /// [`Usage::path`], and with the same consequence: shown, not opened.
    pub path: Option<PathBuf>,
    /// The relative path, or the raw URI when there is no path.
    pub label: String,
    /// **1-based**, matching the editor gutter. See the module docs.
    pub line: u32,
    /// **0-based UTF-16 code units**, which is what CodeMirror wants. The
    /// asymmetry with [`Self::line`] is deliberate; see the module docs.
    pub character: u32,
    pub snippet: String,
    /// The enclosing declaration, when the answer carried one. A bare location
    /// does not, so this is `None` unless something enriched it.
    pub container: Option<String>,
}

/// The inline usage rows one file should show, or the reason there are none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AnchorResult {
    pub outcome: Availability,
    pub anchors: Vec<DeclarationAnchor>,
    /// Why there are no anchors, or the qualification a `Ready` answer needs. See
    /// [`UsageResult::message`].
    pub message: Option<String>,
}

impl AnchorResult {
    /// The shape for every outcome that is not an answer.
    pub fn unavailable(outcome: Availability, message: impl Into<String>) -> Self {
        Self {
            outcome,
            anchors: Vec::new(),
            message: Some(message.into()),
        }
    }
}

/// One declaration in a file that deserves an inline "N usages" row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeclarationAnchor {
    /// Stable within one file across calls, so the UI can key a widget on it
    /// without the widget remounting on every refresh — and distinct for two
    /// same-named overloads. See [`super::results::anchors`] for how.
    pub id: String,
    /// The bare identifier. Roslyn sends a whole signature; the inline row
    /// wants the name.
    pub name: String,
    pub kind: SymbolKind,
    /// Where the inline row is drawn: the declaration's first line, **1-based**.
    pub line: u32,
    /// Where a usages request must be aimed — the identifier's column, not the
    /// body's. **0-based UTF-16 code units**; see the module docs for why this
    /// disagrees with [`Self::line`].
    pub character: u32,
    /// The identifier's own line, **1-based**. Differs from [`Self::line`]
    /// whenever attributes, doc comments or a wrapped signature push the name
    /// below the start of the declaration, and [`Self::character`] is an offset
    /// into *this* line rather than into [`Self::line`].
    pub selection_line: u32,
}

// ---------------------------------------------------------------------------
// Rename
// ---------------------------------------------------------------------------

/// A rename that happened, or the reason none did.
///
/// # Why one result carries two lists of edits
///
/// **Rust writes the closed files; the frontend applies the open buffers.** For
/// an open file the truth is the CodeMirror buffer plus the server's mirror, and
/// `FileEditor`'s build effect is keyed on identity alone with no file watcher
/// and no autosave — so a disk write behind an open tab is invisible to the
/// buffer and is clobbered by the next `Ctrl+S`. Handing *every* edit back for
/// the frontend to write is worse still: it has no text for a closed file, so it
/// would have to re-implement [`super::edits`] in a layer with no tests. Hence
/// [`Self::written`] (already on disk) beside [`Self::buffers`] (for the editor
/// to dispatch), from one command and one answer.
///
/// # What this does not promise
///
/// A multi-file rename is **not one undo**. Each open buffer receives one
/// transaction, so `Ctrl+Z` in that tab reverts that file's part; the closed
/// files were written to disk and have no undo here at all. The honest surface
/// for that is the Changes tab's line-level revert, and the frontend says so
/// rather than implying an undo that does not exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RenameResult {
    pub outcome: Availability,
    /// How many edits the rename accounted for, across every file.
    ///
    /// `None` unless `outcome == Ready`; `Some(0)` means the server answered
    /// with nothing to change, which is a real answer — a symbol may genuinely
    /// have no other occurrence — and is not the same thing as nobody being
    /// asked. Same rule as [`UsageResult::total`], and here it is also what a
    /// summary line counts.
    pub total: Option<u32>,
    /// The closed files this app wrote, in the order they were written. Empty
    /// whenever `total` is `None`, and empty after a rollback.
    pub written: Vec<RenamedFile>,
    /// The open buffers' edits, for the editor to dispatch. Never written to
    /// disk here; see the type docs.
    pub buffers: Vec<BufferEdits>,
    /// Files that could not be written, or could not be restored.
    ///
    /// Non-empty means the rename did not complete. A [`RenameFailure`] with
    /// [`RenameFailure::unrecoverable`] set is the one state nothing here can
    /// put right, and it must be escalated rather than listed.
    pub failures: Vec<RenameFailure>,
    /// Why, when the outcome is not `Ready` — **and the qualification when it
    /// is.** See [`UsageResult::message`]: a `Ready` rename carries a message
    /// when the answer needs one, either because the server never finished
    /// priming (so call sites may have been *missed*, which is worse than a low
    /// count) or because some edits landed on text that is not the identifier
    /// being renamed.
    pub message: Option<String>,
    /// Which server answered, for a status line. `None` when none did.
    pub server: Option<String>,
}

impl RenameResult {
    /// The shape for every outcome that is not a rename.
    ///
    /// The only place `total` is decided to be `None`, exactly as
    /// [`UsageResult::unavailable`] is — so a refusal can never ship a `total`
    /// of zero and read as "there was nothing to rename".
    pub fn unavailable(outcome: Availability, message: impl Into<String>) -> Self {
        Self {
            outcome,
            total: None,
            written: Vec::new(),
            buffers: Vec::new(),
            failures: Vec::new(),
            message: Some(message.into()),
            server: None,
        }
    }

    /// Name the server that produced (or failed to produce) this answer.
    #[must_use]
    pub fn with_server(mut self, server: impl Into<String>) -> Self {
        self.server = Some(server.into());
        self
    }
}

/// One span of one document, and the text that replaces it.
///
/// **1-based `line`, 0-based UTF-16 `character`** — the same asymmetry as every
/// other position on this surface, restated because this is the first type here
/// carrying an *end* position and both halves follow the same rule.
/// `start_line`/`end_line` are gutter lines; `start_character`/`end_character`
/// are the UTF-16 code-unit columns CodeMirror hands over and takes back. The
/// range is **half-open**: `end` is exclusive, so a zero-width range is an
/// insertion and an empty [`Self::new_text`] is a deletion. Both are ordinary —
/// the real Roslyn server answers a rename with zero-width insertions carrying a
/// minimal diff (`Walker` to `HeapWalker` inserts `"Heap"`), so an insertion is
/// the normal case rather than an edge one.
///
/// No byte offsets. Rust cannot compute them for an open buffer, because it does
/// not have that buffer's text; the editor converts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RangeEdit {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub new_text: String,
}

/// A closed file that was written to disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RenamedFile {
    /// Workspace-relative, forward slashes, as [`crate::symbols::index`] records
    /// it — so it can be fed straight to a re-index and to the Changes tab.
    /// Never absolute: a path outside the root was refused before anything was
    /// written.
    pub path: PathBuf,
    /// How many edits this file received.
    pub edits: u32,
}

/// One open buffer's edits, for the editor to apply as a single transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BufferEdits {
    /// Workspace-relative, forward slashes, matching [`RenamedFile::path`] and
    /// the `EditorSource` a workspace file tab is keyed by.
    pub path: PathBuf,
    /// In document order, non-overlapping — [`super::edits::plan`] proved that
    /// before this crossed. One transaction per file, so the tab's `Ctrl+Z`
    /// reverts that file's whole share of the rename.
    pub edits: Vec<RangeEdit>,
}

/// A file the rename could not finish with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RenameFailure {
    /// Workspace-relative, forward slashes.
    pub path: PathBuf,
    /// What went wrong, in the words of whatever refused.
    pub detail: String,
    /// Whether this file is beyond anything this app can do about it.
    ///
    /// Set when a file is left holding part of a rename with no copy of its
    /// previous contents anywhere — a restore that failed after an earlier
    /// file's write succeeded, a file whose own write failed part-way through
    /// and could not be put back, or one this app could not read back at all to
    /// find out which of those happened. That last case is deliberately in the
    /// set: unread is *unestablished*, and the comfortable reading — "the write
    /// errored, so nothing changed" — is the one that loses a file silently.
    ///
    /// A boolean rather than prose because the UI has to escalate on it — send
    /// the user to git, now — and must not have to parse English to decide.
    pub unrecoverable: bool,
}

/// Whether the caret is on something renameable, and where its identifier is.
///
/// The `renameable: false` case is this feature's [`UsageResult::total`]: a
/// server answering `prepareRename` with `null` has said something true about
/// the caret — it is on a keyword, a comment, a literal — so the outcome is
/// [`Availability::Ready`] and [`Self::renameable`] carries the news. Reporting
/// it as [`Availability::Failed`] would tell the user their language server is
/// broken while it is working perfectly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PrepareRenameResult {
    pub outcome: Availability,
    /// False for every outcome that is not `Ready`, and for a `Ready` answer of
    /// `null`. Never a licence on its own: an unready outcome carries a message.
    pub renameable: bool,
    /// The identifier's span, when the server named one. **1-based line,
    /// 0-based UTF-16 character**; see [`RangeEdit`].
    ///
    /// All four are `None` together. A server may answer `defaultBehavior`,
    /// which says the caret *is* renameable while naming no range at all — so a
    /// renameable answer with no range is legal, and the frontend derives the
    /// span from the buffer rather than treating it as a refusal.
    pub start_line: Option<u32>,
    pub start_character: Option<u32>,
    pub end_line: Option<u32>,
    pub end_character: Option<u32>,
    /// What to prefill the field with, when the server offered it.
    ///
    /// **`None` for the real Roslyn server**, which answers a bare `{start,
    /// end}` — so reading the identifier out of the buffer is the live path for
    /// C# and not a fallback. A design that only handled the placeholder shape
    /// opens an empty rename box.
    pub placeholder: Option<String>,
    /// Why there is no answer, or the qualification a `Ready` one needs. See
    /// [`UsageResult::message`].
    pub message: Option<String>,
    pub server: Option<String>,
}

impl PrepareRenameResult {
    /// The shape for every outcome that is not an answer.
    pub fn unavailable(outcome: Availability, message: impl Into<String>) -> Self {
        Self {
            outcome,
            renameable: false,
            start_line: None,
            start_character: None,
            end_line: None,
            end_character: None,
            placeholder: None,
            message: Some(message.into()),
            server: None,
        }
    }

    /// The server answered, and the answer is that this cannot be renamed.
    ///
    /// A constructor of its own rather than [`Self::unavailable`] with a
    /// different outcome, because the two are opposite claims and sharing a
    /// constructor is how they end up sharing a rendering: this one is `Ready`
    /// with no message, and the frontend's job is to decline the *rename*, not
    /// to report a problem with the server.
    pub fn not_renameable(server: impl Into<String>) -> Self {
        Self {
            outcome: Availability::Ready,
            renameable: false,
            start_line: None,
            start_character: None,
            end_line: None,
            end_character: None,
            placeholder: None,
            message: None,
            server: Some(server.into()),
        }
    }

    /// Name the server that produced (or failed to produce) this answer.
    #[must_use]
    pub fn with_server(mut self, server: impl Into<String>) -> Self {
        self.server = Some(server.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Type hierarchy
// ---------------------------------------------------------------------------

/// One type in an inheritance graph, and where it is declared.
///
/// The same abstain rules as [`Target`]: [`Self::path`] is `None` for a location
/// outside the workspace or in a non-`file:` document (Roslyn answers
/// `source-generated:` and metadata URIs), so the node is shown but not opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TypeNode {
    /// The type's name, as the server rendered it.
    pub name: String,
    pub kind: SymbolKind,
    /// The server's own one-line detail (a namespace, a signature), when it sent
    /// one. `None` rather than an invented value.
    pub detail: Option<String>,
    /// Workspace-relative, forward slashes. `None` for the same reasons as
    /// [`Target::path`], with the same consequence: shown, not opened.
    pub path: Option<PathBuf>,
    /// The relative path, or the raw URI when there is no path.
    pub label: String,
    /// **1-based**, matching the editor gutter. See the module docs.
    pub line: u32,
    /// **0-based UTF-16 code units**, which is what CodeMirror wants. The
    /// asymmetry with [`Self::line`] is deliberate; see the module docs.
    pub character: u32,
}

/// A type's place in the inheritance graph, or the reason there is none.
///
/// [`Self::item`] is the type the caret landed on. **`None` is a real answer**:
/// the caret was not on a type at all, so the outcome is [`Availability::Ready`]
/// with an empty everything, exactly as [`UsageResult::total`] of `Some(0)` is a
/// real "no usages" and not a failure. Reporting "not on a type" as
/// [`Availability::Failed`] would tell the user their server is broken while it
/// answered perfectly.
///
/// **One outcome for two lists.** A server that answers `supertypes` but refuses
/// `subtypes` — or the reverse — keeps [`Availability::Ready`] and names the
/// refused direction in [`Self::message`]; only a refusal of *both* changes the
/// outcome. So an empty list licenses "there are none" only when `message` is
/// `None`. The same rule as [`DefinitionResult`], for the same reason: there is
/// no per-list [`Availability`] to spell a partial refusal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TypeHierarchyResult {
    pub outcome: Availability,
    /// The type the request resolved to, or `None` when the caret was not on one.
    pub item: Option<TypeNode>,
    /// Types this one derives from, base-most last is **not** promised — the
    /// order is the server's.
    pub supertypes: Vec<TypeNode>,
    /// Types that derive from this one.
    pub subtypes: Vec<TypeNode>,
    /// Why a direction is empty, or the qualification a `Ready` answer needs. See
    /// [`UsageResult::message`]; here it is also the only channel for a partially
    /// refused answer.
    pub message: Option<String>,
    pub server: Option<String>,
}

impl TypeHierarchyResult {
    /// The shape for every outcome that is not an answer.
    pub fn unavailable(outcome: Availability, message: impl Into<String>) -> Self {
        Self {
            outcome,
            item: None,
            supertypes: Vec::new(),
            subtypes: Vec::new(),
            message: Some(message.into()),
            server: None,
        }
    }

    /// Name the server that produced (or failed to produce) this answer.
    #[must_use]
    pub fn with_server(mut self, server: impl Into<String>) -> Self {
        self.server = Some(server.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Overloads (signature help)
// ---------------------------------------------------------------------------

/// One callable signature available at a call site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SignatureInfo {
    /// The whole signature, exactly as the server rendered it.
    pub label: String,
    /// The server's documentation, plain-text — a `MarkupContent`'s value is
    /// taken verbatim rather than rendered. `None` when it sent none.
    pub documentation: Option<String>,
    pub parameters: Vec<ParameterInfo>,
}

/// One parameter of a signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ParameterInfo {
    /// The parameter's own text. The server may name it as a substring **or** as
    /// a pair of UTF-16 offsets into the signature label; both are resolved to
    /// the substring before they cross, so this is always the text itself.
    pub label: String,
    pub documentation: Option<String>,
}

/// The overloads at a call site, or the reason there are none.
///
/// An empty [`Self::signatures`] on a `Ready` outcome is a real answer — the
/// caret is not inside a call — and not a failure, the same rule as everywhere
/// else on this surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OverloadResult {
    pub outcome: Availability,
    pub signatures: Vec<SignatureInfo>,
    /// Which signature the server thinks is active, **0-based** into
    /// [`Self::signatures`]. `None` when it did not say — never guessed at 0,
    /// because the server not choosing is a different fact from it choosing the
    /// first.
    pub active_signature: Option<u32>,
    /// Which parameter of the active signature is active, **0-based**. `None`
    /// when the server did not say.
    pub active_parameter: Option<u32>,
    pub message: Option<String>,
    pub server: Option<String>,
}

impl OverloadResult {
    /// The shape for every outcome that is not an answer.
    pub fn unavailable(outcome: Availability, message: impl Into<String>) -> Self {
        Self {
            outcome,
            signatures: Vec::new(),
            active_signature: None,
            active_parameter: None,
            message: Some(message.into()),
            server: None,
        }
    }

    /// Name the server that produced (or failed to produce) this answer.
    #[must_use]
    pub fn with_server(mut self, server: impl Into<String>) -> Self {
        self.server = Some(server.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Diagnostics (pull)
// ---------------------------------------------------------------------------

/// How serious one diagnostic is.
///
/// The four the protocol numbers 1..=4. A diagnostic that omits its severity is
/// rendered [`Self::Warning`] — a deliberate middle rather than a guess in either
/// direction: `Error` over-alarms and `Hint` hides. Roslyn, rust-analyzer and the
/// TypeScript server all send one, so the fallback is an edge, not the norm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

/// One diagnostic on the file that was asked about.
///
/// No path: every row is on the requested file, so joining one would be noise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticRow {
    pub severity: DiagnosticSeverity,
    /// **1-based** start line, matching the editor gutter. See the module docs.
    pub line: u32,
    /// **0-based UTF-16 code units** start column. The asymmetry with
    /// [`Self::line`] is deliberate; see the module docs.
    pub character: u32,
    /// **1-based** end line, half-open with the start (the range's `end` is
    /// exclusive, as everywhere else here).
    pub end_line: u32,
    /// **0-based UTF-16 code units** end column.
    pub end_character: u32,
    pub message: String,
    /// The tool that raised it (`roslyn`, `rustc`, `ts`), when it said. `None`
    /// rather than a guessed one.
    pub source: Option<String>,
    /// The rule/error code, as a string even when the server sent a number.
    /// `None` when it named none.
    pub code: Option<String>,
}

/// Every diagnostic on one file, or the reason there is no list.
///
/// **Pull, not push.** These come from a `textDocument/diagnostic` request, so an
/// empty [`Self::diagnostics`] on a `Ready` outcome means the file is clean — a
/// real answer. The push `publishDiagnostics` stream is deliberately not the
/// source: it arrives asynchronously after `didOpen`, so "empty because clean"
/// and "empty because not yet arrived" would be the same shape, which is the one
/// ambiguity this subsystem refuses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsResult {
    pub outcome: Availability,
    pub diagnostics: Vec<DiagnosticRow>,
    /// Why there is no list, or the qualification a `Ready` answer needs. See
    /// [`UsageResult::message`].
    pub message: Option<String>,
    pub server: Option<String>,
}

impl DiagnosticsResult {
    /// The shape for every outcome that is not an answer.
    pub fn unavailable(outcome: Availability, message: impl Into<String>) -> Self {
        Self {
            outcome,
            diagnostics: Vec::new(),
            message: Some(message.into()),
            server: None,
        }
    }

    /// Name the server that produced (or failed to produce) this answer.
    #[must_use]
    pub fn with_server(mut self, server: impl Into<String>) -> Self {
        self.server = Some(server.into());
        self
    }
}

/// What every configured server is doing, for the status surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LspStatus {
    pub servers: Vec<ServerStatus>,
}

/// One server's state, and what to do about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub id: String,
    /// Human-readable, for the row's label.
    pub language: String,
    pub state: Availability,
    /// The version line, the exit code, the error — whatever there is to say.
    pub detail: Option<String>,
    /// Why this server's answers may be incomplete, when they may be.
    ///
    /// Set only for a server promoted at the readiness ceiling
    /// (`ReadyState::ReadyWithCaveat`), whose [`Self::state`] is
    /// [`Availability::Ready`] because requests do proceed. A field of its own
    /// rather than prose in [`Self::detail`]: `detail` is the program path for a
    /// plain ready server, so a caveat delivered there is a state the UI cannot
    /// tell from a healthy one without parsing English — and the status
    /// indicator, whose rule is to say nothing about a ready server, dropped it.
    pub caveat: Option<String>,
    /// Everywhere that was searched for the program. Empty once one was found.
    ///
    /// A list rather than a sentence because "not found" is only actionable if
    /// the user can see *where* we looked.
    pub looked_for: Vec<String>,
    /// What the user could do about it, when there is something.
    pub hint: Option<String>,
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod model_tests;
