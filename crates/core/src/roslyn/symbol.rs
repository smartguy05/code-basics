//! Resolving a `{symbol}` target to a position, using a file's declaration
//! anchors.
//!
//! The agent has no caret. So the three position-taking tools accept a `symbol`
//! name as an alternative to `{line, character}` ([`super::tools::Target`]), and
//! the application resolves it against the anchors its warm language server
//! already produces for that file ([`crate::lsp::session::LspHandle::declaration_anchors`],
//! built by [`crate::lsp::results::anchors`]).
//!
//! This is the pure decision the app-side dispatch delegates to, so the two rules
//! that matter are testable without a language server:
//!
//! * **Ambiguity is refused, never resolved.** A name that matches more than one
//!   declaration is listed with the lines it was found on, and the agent is told
//!   to pass a position. Picking one would be the guess this subsystem refuses.
//! * **An abstaining anchor result surfaces as itself.** If the anchors could not
//!   be produced — the server is still loading, torn down, unsupported — that is
//!   the answer, *not* "no such symbol": a missing symbol and a server that has
//!   not finished loading are different facts with different fixes.

use crate::lsp::model::{AnchorResult, Availability, DeclarationAnchor};

use super::answer::RoslynRefusal;
use super::wire::ToolAnswer;

/// The refusal code for a symbol name that matches nothing in the file.
pub const SYMBOL_NOT_FOUND: &str = "symbolNotFound";

/// The refusal code for a symbol name that matches more than one declaration.
pub const AMBIGUOUS_SYMBOL: &str = "ambiguousSymbol";

/// The outcome of resolving a `{symbol}` target against a file's anchors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Located {
    /// Exactly one declaration matched — aim the request at this position. `line`
    /// is **1-based** and `character` is **0-based UTF-16 code units**, the
    /// convention the whole LSP surface uses.
    At { line: u32, character: u32 },
    /// There is nothing to aim at — send this refusal back instead of guessing.
    Refused(ToolAnswer),
}

/// Resolve a symbol name to the position of its declaration's identifier.
///
/// `path` is only used to phrase the refusals; the anchors are already the ones
/// for that file.
pub fn locate_symbol(name: &str, path: &str, anchors: &AnchorResult) -> Located {
    // An anchor result that is not `Ready` is an abstention, and it must surface
    // as that state rather than as "no such symbol".
    if anchors.outcome != Availability::Ready {
        return Located::Refused(unavailable(anchors));
    }

    let wanted = name.trim();
    let matches: Vec<&DeclarationAnchor> = anchors
        .anchors
        .iter()
        .filter(|anchor| anchor.name == wanted)
        .collect();

    match matches.as_slice() {
        [] => Located::Refused(not_found(wanted, path, anchors)),
        [only] => Located::At {
            line: only.selection_line,
            character: only.character,
        },
        several => Located::Refused(ambiguous(wanted, path, several)),
    }
}

/// The refusal for an anchor result that abstained. Mirrors
/// [`super::render`]'s handling: the result's own curated `message` is preferred,
/// falling back to the generic sentence for the state.
fn unavailable(anchors: &AnchorResult) -> ToolAnswer {
    let refusal = RoslynRefusal::from_availability(anchors.outcome)
        .expect("unavailable is only reached for a non-Ready anchor result");
    let base = match &anchors.message {
        Some(message) if !message.trim().is_empty() => message.trim().to_string(),
        _ => refusal.sentence(),
    };
    ToolAnswer::refused(refusal.code(), base)
}

/// A symbol name that matched no declaration. The names that *are* present are
/// listed rather than leaving the agent to guess what it should have said.
fn not_found(name: &str, path: &str, anchors: &AnchorResult) -> ToolAnswer {
    let mut names: Vec<&str> = anchors
        .anchors
        .iter()
        .map(|anchor| anchor.name.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();

    let found = if names.is_empty() {
        "no declarations were found in it".to_string()
    } else {
        format!("the declarations found are: {}", names.join(", "))
    };
    ToolAnswer::refused(
        SYMBOL_NOT_FOUND,
        format!(
            "No declaration named {name:?} was found in {path:?}, so there is no position to aim \
             at. Nothing was guessed; {found}. Pass an exact position (line and character) \
             instead, or check the name."
        ),
    )
}

/// A symbol name that matched several declarations. **Refused, never resolved.**
fn ambiguous(name: &str, path: &str, matches: &[&DeclarationAnchor]) -> ToolAnswer {
    let lines = matches
        .iter()
        .map(|anchor| format!("line {}", anchor.selection_line))
        .collect::<Vec<_>>()
        .join(", ");
    ToolAnswer::refused(
        AMBIGUOUS_SYMBOL,
        format!(
            "The name {name:?} matches {count} declarations in {path:?} ({lines}), so which one \
             you mean is genuinely unknown. Nothing was chosen. Pass an exact position (line and \
             character) to choose one — resolve_overloads is the natural way to disambiguate an \
             overloaded method.",
            count = matches.len()
        ),
    )
}

#[cfg(test)]
#[path = "symbol_tests.rs"]
mod tests;
