//! Every answer this server can give that is not data — and they are all
//! different answers.
//!
//! The **twin** of [`crate::roslyn::answer`], applying the same
//! abstain-rather-than-guess rule ([`crate::mcp::answer`],
//! [`crate::tasks::mcp::answer`]) for the same reason: a model reads the sentence,
//! believes it, and acts, so the distinctions a human would have recovered from
//! unaided are the ones kept apart here.
//!
//! Three of these come **before** any editor state is rendered:
//!
//! * [`EditorRefusal::NoWorkspace`] — this server was started without a
//!   `--workspace`, so there is no session to reach at all.
//! * [`EditorRefusal::Disabled`] — the workspace is open, but the
//!   `EditorContextMcp` feature is switched off, so the frontend has stopped
//!   pushing editor state and the app will not answer. Distinct from
//!   `NoWorkspace`: the fix is to switch the feature on, not to reinstall.
//! * [`EditorRefusal::NoContext`] — the feature is on but no editor state has
//!   been pushed yet (the app just opened, or no file has been touched).
//!   Distinct from `Disabled`: nothing is wrong, there is simply nothing to
//!   report *yet*.
//!
//! And one comes from [`super::render`] when there **is** state but the tool has
//! nothing to point at:
//!
//! * [`EditorRefusal::NoActiveFile`] — `get_active_file` / `get_selection` asked
//!   about the focused file, and there is no active tab. An honest refusal, not a
//!   fabricated path or position.
//!
//! An empty selection, no open tabs, and no recent files are **not** refusals:
//! they are genuine data answers rendered plainly by [`super::render`], the way a
//! `Ready`-but-empty result is data in the Roslyn server.
//!
//! # No internal error text
//!
//! Every sentence here is generic and names no path, no OS error and no log line.

use super::wire::ToolAnswer;

/// Why a tool call produced no data.
///
/// Every variant is rendered as a [`super::wire::ToolAnswer`] refusal — `ok:
/// false` with a code — so the model sees it and can correct itself. **Four
/// distinct codes**; a shared one would pass every other test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorRefusal {
    /// This server was started without a `--workspace`, so there is no session
    /// to reach.
    NoWorkspace,
    /// The workspace is open, but the editor-context feature is switched off, so
    /// no live state is being pushed.
    Disabled,
    /// The feature is on but no editor state has been pushed yet.
    NoContext,
    /// There is editor state, but no file is active — so `get_active_file` and
    /// `get_selection` have nothing to point at.
    NoActiveFile,
}

impl EditorRefusal {
    /// A short machine-matchable name, so a model can branch without parsing
    /// prose.
    pub fn code(self) -> &'static str {
        match self {
            Self::NoWorkspace => "noWorkspace",
            Self::Disabled => "featureDisabled",
            Self::NoContext => "noContext",
            Self::NoActiveFile => "noActiveFile",
        }
    }

    /// The sentence the model reads. Generic — it names no path, no OS error and
    /// no log line.
    pub fn sentence(self) -> String {
        match self {
            Self::NoWorkspace => {
                "This editor-context server was started without a --workspace, so \
                 there is no editor session to ask. It has to be installed scoped to a repository; \
                 the editor state is per-workspace."
                    .to_string()
            }
            Self::Disabled => "The editor-context feature is switched off in code-basics, so the \
                 application is not sharing live editor state. Nothing was read. Ask the user to \
                 switch the \"Editor context MCP\" feature back on if they want it exposed."
                .to_string(),
            Self::NoContext => "The workspace is open and the feature is on, but no editor state \
                 has been reported yet — the application may have just started, or no file has \
                 been opened. Nothing was guessed. Ask again once a file is open."
                .to_string(),
            Self::NoActiveFile => "No file is active in the editor right now, so there is no \
                 focused file or cursor to report. This is the complete answer, not a truncated \
                 one; nothing was fabricated."
                .to_string(),
        }
    }

    /// This refusal as a [`ToolAnswer`] — `ok: false` with the code first, so a
    /// model can branch on it, exactly as [`super::wire::ToolAnswer::refused`]
    /// shapes every other refusal on this pipe.
    pub fn answer(self) -> ToolAnswer {
        ToolAnswer::refused(self.code(), self.sentence())
    }
}

#[cfg(test)]
#[path = "answer_tests.rs"]
mod tests;
