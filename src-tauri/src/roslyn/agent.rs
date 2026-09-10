//! Answering an agent's Roslyn tool call: resolve the location, ask the warm
//! session, phrase the result.
//!
//! This is the application's half of [`cb_core::roslyn::wire`]. The shim owns no
//! semantic model and phrases nothing, so a call arrives here as a
//! [`RoslynToolCall`] and leaves as a [`ToolAnswer`] whose words came from
//! [`cb_core::roslyn::render`].
//!
//! # What is decided here, and what is not
//!
//! Almost nothing is decided here, deliberately:
//!
//! * **whether the session can answer at all** is the presence of an
//!   [`LspHandle`] — [`None`] is [`RoslynRefusal::NoSession`];
//! * **which position a `{symbol}` resolves to** — and whether it is ambiguous —
//!   is [`cb_core::roslyn::symbol::locate_symbol`];
//! * **what every answer says** is [`cb_core::roslyn::render`].
//!
//! [`answer`] itself is an `async` mapping from an enum variant onto a handle
//! method and a renderer. It is not reachable from a test (it needs a running
//! language server), which is why it contains no decision — every branch is one
//! variant onto one call onto one renderer, and the only two-armed matches are
//! the position-or-symbol resolution and the handle's own result, both of which
//! were decided by a tested pure function.
//!
//! # No per-call consent gate
//!
//! A local language server answering read-only questions about an already-open
//! local repository is the SQL/Tasks trust level, and the boundary is the
//! `--workspace` scope baked into the install. [`Peer`] and [`peer_label`] stay
//! plumbed so a banner can be added later with no wire change — but nothing gates
//! on them today.

use std::path::Path;

use cb_core::lsp::session::LspHandle;
use cb_core::roslyn::answer::RoslynRefusal;
use cb_core::roslyn::render;
use cb_core::roslyn::symbol::{self, Located};
use cb_core::roslyn::tools::{RoslynToolCall, Target};
use cb_core::roslyn::wire::ToolAnswer;

/// The program on the other end of the pipe.
///
/// Named by the OS, never by the caller — see [`super::pipe::peer_label`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub pid: u32,
    pub program: String,
}

/// The refusal for a [`RoslynRefusal`], as a [`ToolAnswer`].
fn refusal(reason: RoslynRefusal) -> ToolAnswer {
    ToolAnswer::refused(reason.code(), reason.sentence())
}

/// Resolve a tool's location to a `(line, character)`, or the refusal to send
/// back.
///
/// A [`Target::Position`] is taken as given; a [`Target::Symbol`] is resolved
/// against the file's declaration anchors — the same anchors the warm server
/// already produces for the editor's inline usage rows — and the ambiguity rule
/// is [`symbol::locate_symbol`]'s, not this function's.
async fn position(
    handle: &LspHandle,
    abs: &Path,
    rel: &str,
    target: Target,
) -> Result<(u32, u32), ToolAnswer> {
    match target {
        Target::Position { line, character } => Ok((line, character)),
        Target::Symbol { name } => {
            let anchors = handle.declaration_anchors(abs).await;
            match symbol::locate_symbol(&name, rel, &anchors) {
                Located::At { line, character } => Ok((line, character)),
                Located::Refused(answer) => Err(answer),
            }
        }
    }
}

/// Run one call against the workspace's warm session and phrase its answer.
///
/// `handle` is the session for the request's `--workspace`, resolved by the pipe
/// through [`crate::state::AppState::lsp_for_root`]; [`None`] means the workspace
/// is open but its session is unavailable (torn down, or none for this language),
/// which is [`RoslynRefusal::NoSession`].
pub async fn answer(handle: Option<LspHandle>, call: RoslynToolCall, _peer: &Peer) -> ToolAnswer {
    let Some(handle) = handle else {
        return refusal(RoslynRefusal::NoSession);
    };

    // The tool names a workspace-relative path; the session works in absolute
    // paths (the editor passes CodeMirror's own absolute path), so it is joined
    // onto the session's root. Forward slashes are fine — the URI layer emits
    // them regardless. But `--workspace` is this server's whole consent boundary,
    // so a path that could climb out of the root (absolute, drive/UNC-prefixed,
    // or `..`) is refused before anything is joined or read — the decision is the
    // tested pure guard, not this untestable body.
    let rel = call.path().to_string();
    if !cb_core::roslyn::answer::path_within_root(&rel) {
        return refusal(RoslynRefusal::BadPath);
    }
    let abs = handle.root().join(&rel);

    match call {
        RoslynToolCall::GetDiagnostics { .. } => {
            render::diagnostics(&handle.get_diagnostics(&abs).await)
        }
        RoslynToolCall::FindReferences { target, .. } => {
            match position(&handle, &abs, &rel, target).await {
                Ok((line, character)) => {
                    render::find_references(&handle.find_references(&abs, line, character).await)
                }
                Err(refused) => refused,
            }
        }
        RoslynToolCall::GetTypeHierarchy { target, .. } => {
            match position(&handle, &abs, &rel, target).await {
                Ok((line, character)) => {
                    render::type_hierarchy(&handle.get_type_hierarchy(&abs, line, character).await)
                }
                Err(refused) => refused,
            }
        }
        RoslynToolCall::ResolveOverloads { target, .. } => {
            match position(&handle, &abs, &rel, target).await {
                Ok((line, character)) => {
                    render::overloads(&handle.resolve_overloads(&abs, line, character).await)
                }
                Err(refused) => refused,
            }
        }
    }
}
