//! The per-workspace language-server configuration block.
//!
//! # Why a server is *located* rather than bundled
//!
//! Every option is large and none of them is ours to ship.
//! `Microsoft.CodeAnalysis.LanguageServer` is a self-contained .NET
//! application; `rust-analyzer` is a single binary of comparable size; the
//! TypeScript and Python servers are npm packages that pull in a whole
//! toolchain. Shipping four of those would add hundreds of megabytes to an
//! installer whose whole point is being lighter than the IDE it replaces — and
//! the licences differ per option, so bundling is not one decision but four,
//! each needing its own redistribution answer. Locating the user's own copy
//! moves that question out of this app entirely: nothing is redistributed, so
//! nothing has to be relicensed. (Roslyn's own licence was checked and is MIT,
//! but that is what makes *launching* the VS Code extension's copy safe, not
//! what would make copying it into a bundle safe.)
//!
//! The cost is that a server can be absent, which is why discovery is a first
//! class outcome in [`crate::lsp::registry`] rather than an error path — and why
//! this block exists at all: discovery cannot find a server installed somewhere
//! only the user knows about.
//!
//! # Why an unresolvable `program` is an error naming this file
//!
//! When [`ServerOverride::program`] is set and does not resolve to something
//! launchable, the answer is a failure that names `.code-basics/config.json`,
//! **not** a quiet fall back to discovery. The user asked for one specific
//! server — very often a build with a fix, or a version pinned to match the
//! project's SDK — and silently starting a *different* one produces answers
//! attributed to a server that never ran. In this subsystem those answers are
//! usage counts and definition jumps, so being quietly wrong is the worst
//! available outcome, and CLAUDE.md's rule applies at its sharpest: a wrong
//! answer is much worse than no answer. Discovery is the behaviour of an
//! *absent* `program`, and stays so.
//!
//! # What the fields do and do not do
//!
//! Every field is optional and every absence means "use the built-in default",
//! never "use nothing". That distinction is load-bearing in two places, both
//! pinned by tests: an absent `enabled` is *enabled* (so a block written to set
//! a `program` does not disable the server it just configured), and an absent
//! `args` is the built-in argument list while `"args": []` is a deliberate
//! empty one. Roslyn launched without `--stdio` never speaks, so collapsing
//! those two would look like a hung server rather than like a configuration
//! mistake.
//!
//! Unknown keys are ignored rather than rejected: this file is checked into the
//! user's repository and shared with their team, so a block written by a newer
//! build must still load for everyone on an older one. That is the same
//! tolerance [`crate::config::load`] already shows for the rest of the file.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::lsp::uri::UriStyle;

/// Per-workspace language-server settings, stored in `.code-basics/config.json`.
///
/// Absent unless the user configured something, like
/// [`crate::inspect::model::InspectorConfig`] beside it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct LspConfig {
    /// Keyed by server id: `"csharp"`, `"typescript"`, `"rust"`, `"python"` —
    /// the ids [`crate::lsp::registry`] knows. A key it does not recognise is
    /// simply never looked up.
    ///
    /// A `BTreeMap` because this file is committed: a hash order that varied
    /// per process would rewrite the file on every save with no change in
    /// meaning.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub servers: BTreeMap<String, ServerOverride>,
}

impl LspConfig {
    /// What the user said about one server, or nothing.
    ///
    /// `None` means the file never mentions this server, which is a different
    /// answer from an empty override and must stay so: a caller reads the first
    /// as "no opinion" and the second as "configured, with defaults".
    pub fn server(&self, id: &str) -> Option<&ServerOverride> {
        self.servers.get(id)
    }
}

/// What one workspace says about one server.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerOverride {
    /// `Some(false)` refuses the server; `Some(true)` confirms it; `None` means
    /// nobody has said, which is enabled. See [`Self::is_disabled`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// An explicit executable, absolute or on `PATH`. When it does not resolve
    /// the server **fails**, naming this file — see the module doc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// Replaces the built-in argument list entirely rather than appending, so a
    /// user who needs to remove a default argument can. `None` keeps the
    /// built-in list; `Some(vec![])` really means none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Environment layered over the inherited environment for this server only.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Override the `file:` URI spelling sent to this server.
    ///
    /// An escape hatch, not a routine knob: the built-in per-server default is
    /// what real servers were observed to accept, and identity is decided on
    /// paths rather than URI strings (see [`crate::lsp::uri`]), so this only
    /// matters for a server that rejects a spelling outright.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri_style: Option<UriStyleSetting>,
}

impl ServerOverride {
    /// Whether the user switched this server off.
    ///
    /// Only an explicit `false` counts. An absent flag is *not* a refusal —
    /// otherwise every block written to set a `program` would disable the very
    /// server it configures, and the symptom (a feature that reports itself
    /// unavailable) looks nothing like the cause.
    pub fn is_disabled(&self) -> bool {
        self.enabled == Some(false)
    }
}

/// The `file:` URI spelling, as a user writes it in the configuration file.
///
/// A separate type from [`UriStyle`] on purpose: that one is a transport
/// detail with no serde impl and no stability promise, and this one is a key in
/// a file people commit. Mirroring rather than deriving `Serialize` on
/// `UriStyle` keeps a rename inside the transport from silently invalidating
/// everyone's configuration file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum UriStyleSetting {
    /// `file:///C%3A/x` — the conservative form. What rust-analyzer emits.
    Encoded,
    /// `file:///C:/x` — what the C# extension sends Roslyn, and what was
    /// verified to work against the real server.
    Plain,
}

impl UriStyleSetting {
    /// The transport spelling this setting selects.
    pub fn style(self) -> UriStyle {
        match self {
            UriStyleSetting::Encoded => UriStyle::Encoded,
            UriStyleSetting::Plain => UriStyle::Plain,
        }
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod settings_tests;
