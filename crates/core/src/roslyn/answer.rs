//! Every answer this server can give that is not data — and they are all
//! different answers.
//!
//! The same abstain-rather-than-guess rule the SQL and Tasks servers apply
//! ([`crate::mcp::answer`], [`crate::tasks::mcp::answer`]), for the same reason: a
//! model reads the sentence, believes it, and acts, so the distinctions a human
//! would have recovered from unaided are the ones kept apart here.
//!
//! Two of these come **before** the language server is ever asked:
//!
//! * [`RoslynRefusal::NoWorkspace`] — this server was started without a
//!   `--workspace`, so there is no session to reach at all. Distinct from a
//!   session that answered nothing.
//! * [`RoslynRefusal::NoSession`] — the workspace is open, but its LSP session was
//!   torn down (or never started for that language). Distinct from
//!   `NoWorkspace`: the fix is to reopen the repository, not to reinstall the
//!   server.
//!
//! The rest are **availability-derived**: when the language server does answer, it
//! answers with one of [`crate::lsp::model::Availability`]'s six variants, and
//! every non-`Ready` one is its own refusal with its own code — *not configured*,
//! *starting*, *loading*, *failed* and *unsupported* are five different things to
//! tell an agent, and collapsing them into a shared "unavailable" is exactly the
//! failure this subsystem refuses. `Ready` is not a refusal; it is data, rendered
//! by [`super::render`].
//!
//! # No internal error text
//!
//! Every sentence here is generic and names no path, no OS error and no server
//! log line. The specific reason a non-`Ready` result carries travels in the
//! result's own `message` field (curated for the human editor) and is appended by
//! [`super::render`], never fabricated here.

use crate::lsp::model::Availability;

/// Why a tool call produced no data.
///
/// Every variant is rendered as a [`super::wire::ToolAnswer`] refusal — `ok:
/// false` with a code — so the model sees it and can correct itself. **Eight
/// distinct codes**; a shared one would pass every other test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoslynRefusal {
    /// This server was started without a `--workspace`, so there is no session
    /// to reach.
    NoWorkspace,
    /// The workspace is open but its language-server session is not available —
    /// torn down, or never started for this language.
    NoSession,
    /// No server is configured for this language, or the configured one was not
    /// found on disk. ([`Availability::NotConfigured`].)
    NotConfigured,
    /// The server is coming up and has not finished its handshake.
    /// ([`Availability::Starting`].)
    Starting,
    /// The server is handshaken but still loading its projects, so an answer
    /// would be wrong rather than absent. ([`Availability::Loading`].)
    Loading,
    /// The server died, errored, or answered something unreadable.
    /// ([`Availability::Failed`].)
    Failed,
    /// The server is healthy and does not offer this capability for this
    /// language. ([`Availability::Unsupported`].)
    Unsupported,
    /// The tool named a path that could escape the scoped workspace root — it was
    /// absolute, carried a drive/UNC prefix, or climbed with `..`. Refused before
    /// any file is touched, because `--workspace` is this server's whole consent
    /// boundary. See [`path_within_root`].
    BadPath,
}

impl RoslynRefusal {
    /// The availability-derived refusal for a non-`Ready` outcome, or [`None`]
    /// when the outcome is [`Availability::Ready`] — which is data, not a refusal.
    pub fn from_availability(availability: Availability) -> Option<Self> {
        match availability {
            Availability::Ready => None,
            Availability::NotConfigured => Some(Self::NotConfigured),
            Availability::Starting => Some(Self::Starting),
            Availability::Loading => Some(Self::Loading),
            Availability::Failed => Some(Self::Failed),
            Availability::Unsupported => Some(Self::Unsupported),
        }
    }

    /// A short machine-matchable name, so a model can branch without parsing
    /// prose.
    pub fn code(self) -> &'static str {
        match self {
            Self::NoWorkspace => "noWorkspace",
            Self::NoSession => "noSession",
            Self::NotConfigured => "notConfigured",
            Self::Starting => "serverStarting",
            Self::Loading => "serverLoading",
            Self::Failed => "serverFailed",
            Self::Unsupported => "unsupported",
            Self::BadPath => "badPath",
        }
    }

    /// The sentence the model reads. Generic — the specific reason, when there is
    /// one, is the result's own `message`, appended by [`super::render`].
    pub fn sentence(self) -> String {
        match self {
            Self::NoWorkspace => "This Roslyn server was started without a --workspace, so there \
                 is no language-server session to ask. It has to be installed scoped to a \
                 repository; the semantic model is per-workspace."
                .to_string(),
            Self::NoSession => "The workspace is open, but its language-server session is not \
                 available right now — it was torn down, or none is configured for this file's \
                 language. Nothing was read. Reopen the repository in code-basics."
                .to_string(),
            Self::NotConfigured => "No language server is configured for this file's language, or \
                 the configured one was not found on disk, so this question cannot be answered. \
                 Nothing was guessed."
                .to_string(),
            Self::Starting => "The language server is still starting up and has not finished its \
                 handshake, so it cannot answer yet. Ask again shortly; nothing was guessed."
                .to_string(),
            Self::Loading => "The language server is up but still loading this workspace's \
                 projects, so any answer now would be incomplete rather than merely absent. Ask \
                 again once it has finished loading."
                .to_string(),
            Self::Failed => "The language server did not answer — it exited, errored, or replied \
                 with something this build could not read. Nothing was read. Its own words are in \
                 the application; they are deliberately not forwarded here."
                .to_string(),
            Self::Unsupported => "The language server for this file is healthy but does not offer \
                 this question for this language, so there is no answer to give. This is not a \
                 failure and not an empty result — the capability simply is not there."
                .to_string(),
            Self::BadPath => "The path is not a plain workspace-relative path. This server is \
                 scoped to one repository, so a path may not be absolute, carry a drive or UNC \
                 prefix, or climb out with \"..\". Give a path relative to the workspace root, \
                 such as \"src/App.cs\"."
                .to_string(),
        }
    }
}

/// Whether a tool's `path` stays inside the scoped workspace root.
///
/// The `--workspace` scope is this server's whole consent boundary, so a path the
/// agent supplies must not be able to climb out of it and read a file in another
/// repository. Mirrors [`crate::files`]'s own `resolve`: every component must be a
/// plain name (or a harmless `.`); an absolute path, a drive or UNC prefix
/// ([`std::path::Component::Prefix`]/[`std::path::Component::RootDir`]), or any
/// `..` ([`std::path::Component::ParentDir`]) is refused. Forward slashes are
/// fine — Windows `Path` treats them as separators, which is the form the tools
/// document.
pub fn path_within_root(relative: &str) -> bool {
    use std::path::{Component, Path};
    Path::new(relative)
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
#[path = "answer_tests.rs"]
mod tests;
