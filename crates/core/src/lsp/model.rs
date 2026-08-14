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
