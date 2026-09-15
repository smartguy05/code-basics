//! Answering an agent's editor-context tool call: check the feature, resolve the
//! pushed state, phrase the result.
//!
//! This is the application's half of [`cb_core::editor_context::wire`], and the
//! twin of [`crate::roslyn::agent`]. Unlike that one — which needs a live language
//! server and so cannot be reached from a test — [`answer`] here is a **pure
//! function over plain data**, so the two decisions that live app-side rather than
//! in [`cb_core::editor_context::render`] are tested directly (`agent_tests.rs`):
//!
//! * the feature being switched off is
//!   [`EditorRefusal::Disabled`] — distinct from nothing pushed yet, because the
//!   fix is to switch the feature on, not to open a file;
//! * the feature being on with nothing pushed yet is
//!   [`EditorRefusal::NoContext`].
//!
//! Everything past those two — an active file, an empty selection, no open tabs —
//! is [`cb_core::editor_context::render`]'s to phrase, including the genuine
//! empties it states as complete answers rather than refusals.
//!
//! # Why the feature is re-checked here, per call
//!
//! The frontend stops pushing while the feature is off, but a context pushed
//! *before* it was switched off is still sitting in [`crate::state::AppState`].
//! Serving that would leak editor state the user has since said to stop sharing,
//! so the pipe host reads the feature flag every call and hands the answer its
//! `enabled` value — see [`crate::editor_context::pipe`].

use cb_core::editor_context::answer::EditorRefusal;
use cb_core::editor_context::render;
use cb_core::editor_context::tools::EditorToolCall;
use cb_core::editor_context::wire::ToolAnswer;
use cb_core::model::EditorContext;

/// Phrase one editor-context tool call.
///
/// `enabled` is whether the `EditorContextMcp` feature is on, read fresh by the
/// caller. `context` is the state the frontend has pushed for the request's
/// workspace, or [`None`] when nothing has been pushed (or the workspace has since
/// closed). The `--workspace`-was-missing refusal
/// ([`EditorRefusal::NoWorkspace`]) is produced *before* here, by the pipe host
/// and the shim, since it is about the request rather than the state.
pub fn answer(enabled: bool, context: Option<EditorContext>, call: EditorToolCall) -> ToolAnswer {
    if !enabled {
        return EditorRefusal::Disabled.answer();
    }
    let Some(context) = context else {
        return EditorRefusal::NoContext.answer();
    };
    match call {
        EditorToolCall::GetActiveFile => render::get_active_file(&context),
        EditorToolCall::GetSelection => render::get_selection(&context),
        EditorToolCall::GetOpenFiles => render::get_open_files(&context),
        EditorToolCall::GetRecentFiles => render::get_recent_files(&context),
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
