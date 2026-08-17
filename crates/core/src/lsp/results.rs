//! LSP payloads in, the types the frontend reads out.
//!
//! Pure: no process, no async, no I/O. Reading file text is the caller's job and
//! arrives through [`TextProvider`], so every rule below is decided headlessly
//! against an injected map — which is the point, because the mistakes this layer
//! can make are the invisible kind. A dropped row is a wrong *count*, a row
//! joined onto the wrong root opens a stranger's file, and an underline
//! converted in the wrong units is off by exactly as much non-ASCII as the line
//! happens to contain.
//!
//! # The rules, and why each one is here
//!
//! * **Paths are resolved with [`crate::symbols::index::relative_to_root`] and
//!   nothing else.** URI strings are never compared for identity: Roslyn spells
//!   a drive colon plainly and rust-analyzer percent-encodes it, so string
//!   equality would report two usages where there is one. [`super::uri`]'s
//!   module docs carry the full argument.
//! * **A location that does not resolve under the root keeps its row**, with
//!   `path: None` and the raw URI as its label. Dropping it would make the count
//!   wrong; joining it onto the root would open a different file.
//! * **Duplicates go and the order is fixed.** A server can report one site
//!   twice, and an unstable order reshuffles the dropdown under the cursor.
//! * **The cap shortens the list, never the count.** A cap that also caps the
//!   number is a lie, and the number is what the user acts on.
//! * **`Highlight` is UTF-16 and [`super::positions::Snippet`] is bytes.**
//!   [`super::positions::byte_to_utf16`] is the only thing that may convert
//!   between them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::lsp::model::{
    Availability, DeclarationAnchor, DefinitionResult, Highlight, Target, Usage, UsageResult,
};
use crate::lsp::positions::{self, byte_to_utf16, to_editor_line};
use crate::lsp::protocol::{Location, Symbol};
use crate::lsp::uri;
use crate::symbols::declarations::SymbolKind;
use crate::symbols::index::relative_to_root;

/// One line of a file, however the caller has it.
///
/// A trait rather than a path-to-text closure because production reads the
/// workspace (and may prefer an open buffer over what is on disk) while tests
/// inject a map — and because a usages answer touches many files, so the
/// provider is the natural place for a caller's own cache. `&mut self` exists
/// for exactly that.
///
/// The path handed over is the **absolute** one the URI resolved to, since that
/// is the only thing that names a file; a location with no path behind it is
/// never looked up at all.
pub trait TextProvider {
    /// The line's text without its terminator, or `None` if it cannot be read.
    ///
    /// `line_zero_based` is LSP's own numbering, not the 1-based line that
    /// crosses IPC. Returning `None` is not an error: a file may have moved, and
    /// a server may answer about a buffer version we no longer have. The row
    /// survives without a snippet, because the count must stay right.
    fn line(&mut self, path: &Path, line_zero_based: u32) -> Option<String>;
}

/// A location, resolved, with everything the sort and the dedup need.
struct Resolved {
    /// Workspace-relative, forward slashes. `None` when the location is outside
    /// the root or in a non-`file:` document.
    path: Option<PathBuf>,
    /// The absolute path, kept only to read text with. Never crosses IPC.
    absolute: Option<PathBuf>,
    label: String,
    line: u32,
    character: u32,
    end_line: u32,
    end_character: u32,
}

impl Resolved {
    /// The sort key: unopenable rows last, then path, line and column.
    ///
    /// The label stands in for the path because it *is* the path for every row
    /// that has one, and it keeps the key a single comparable tuple. The end
    /// position is in the key so that two matches sharing a start — which
    /// nothing legal produces, but a server may send — still have a total order
    /// rather than an input-dependent one.
    fn key(&self) -> (bool, &str, u32, u32, u32, u32) {
        (
            self.path.is_none(),
            self.label.as_str(),
            self.line,
            self.character,
            self.end_line,
            self.end_character,
        )
    }
}

/// Turn one LSP location into a row, resolving its URI to a path or abstaining.
fn resolve(root: &Path, location: &Location) -> Resolved {
    let absolute = uri::from_file_uri(&location.uri);
    let relative = absolute
        .as_deref()
        .and_then(|absolute| relative_to_root(root, absolute));
    let label = match &relative {
        // `relative_to_root` normalises to forward slashes, so this is the same
        // spelling the symbol index and the frontend already use.
        Some(path) => path.to_string_lossy().into_owned(),
        None => location.uri.clone(),
    };

    Resolved {
        // A path outside the root is deliberately *not* kept: it would be
        // openable, but it is not a file of this workspace and the frontend's
        // open-a-file chain is rooted.
        absolute: relative.as_ref().and(absolute),
        path: relative,
        label,
        line: location.range.start.line,
        character: location.range.start.character,
        end_line: location.range.end.line,
        end_character: location.range.end.character,
    }
}

/// Resolve, deduplicate and order a group of locations.
fn ordered(root: &Path, locations: &[Location]) -> Vec<Resolved> {
    let mut rows: Vec<Resolved> = locations.iter().map(|l| resolve(root, l)).collect();
    rows.sort_by(|a, b| a.key().cmp(&b.key()));
    rows.dedup_by(|a, b| a.key() == b.key());
    rows
}

/// The snippet and underline for one row.
///
/// The underline is dropped rather than guessed when the trim cuts into it —
/// [`positions::snippet`] decides that — and converted from its byte offsets to
/// UTF-16 code units here, because the reader is JavaScript. That conversion is
/// the single easiest thing in this module to ship silently wrong: on an ASCII
/// line the two agree exactly, so a missing conversion passes every test that
/// does not contain a multi-byte character.
fn context(row: &Resolved, text: &mut impl TextProvider) -> (String, Option<Highlight>) {
    let Some(absolute) = row.absolute.as_deref() else {
        return (String::new(), None);
    };
    let Some(source) = text.line(absolute, row.line) else {
        return (String::new(), None);
    };

    // A range ending on a later line covers the rest of this one, so underlining
    // to the end of the line states something true. Carrying the *second*
    // line's column into the first line's snippet would not.
    let end = if row.end_line == row.line {
        row.end_character
    } else {
        source.encode_utf16().count() as u32
    };

    let snippet = positions::snippet(&source, Some((row.character, end)));
    let highlight = snippet.highlight.map(|(start, end)| Highlight {
        start: byte_to_utf16(&snippet.text, start as usize),
        end: byte_to_utf16(&snippet.text, end as usize),
    });
    (snippet.text, highlight)
}

/// Every use site, as the usages list.
///
/// `cap` bounds the rows, never [`UsageResult::total`]: a user deciding whether
/// a method is safe to change acts on the number, so a capped number would be a
/// wrong answer rather than a shortened one.
///
/// The outcome is always [`Availability::Ready`] — this function is only reached
/// once a server has answered. Every other outcome is [`UsageResult::unavailable`],
/// built by the caller that knows *why* there was no answer.
pub fn usages(
    root: &Path,
    locations: &[Location],
    text: &mut impl TextProvider,
    cap: usize,
) -> UsageResult {
    let rows = ordered(root, locations);
    let total = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    let truncated = rows.len() > cap;

    let usages = rows
        .iter()
        .take(cap)
        .map(|row| {
            let (snippet, highlight) = context(row, text);
            Usage {
                path: row.path.clone(),
                label: row.label.clone(),
                line: to_editor_line(row.line),
                snippet,
                highlight,
            }
        })
        .collect();

    UsageResult {
        outcome: Availability::Ready,
        // `Some(0)` here and `None` in `unavailable` are different answers, and
        // this is the only place that may produce the first.
        total: Some(total),
        usages,
        truncated,
        message: None,
        server: None,
    }
}

/// The three goto answers, as somewhere to jump to.
///
/// Deduplication is **within** a group and never across them: one location is
/// legitimately both a declaration and, from another angle, an implementation —
/// an interface method is the everyday case — and collapsing the groups would
/// delete a real answer rather than a repeated one.
pub fn targets(
    root: &Path,
    definitions: &[Location],
    implementations: &[Location],
    type_definitions: &[Location],
    text: &mut impl TextProvider,
) -> DefinitionResult {
    DefinitionResult {
        outcome: Availability::Ready,
        declarations: group(root, definitions, text),
        implementations: group(root, implementations, text),
        type_definitions: group(root, type_definitions, text),
        message: None,
    }
}

fn group(root: &Path, locations: &[Location], text: &mut impl TextProvider) -> Vec<Target> {
    ordered(root, locations)
        .iter()
        .map(|row| {
            let (snippet, _) = context(row, text);
            Target {
                path: row.path.clone(),
                label: row.label.clone(),
                // 1-based line, 0-based character. The asymmetry is the IPC
                // contract: the line matches the editor gutter and the column
                // matches what CodeMirror hands back.
                line: to_editor_line(row.line),
                character: row.character,
                snippet,
                // A bare location carries no enclosing declaration, and
                // inventing one from the snippet would be a guess about a file
                // this module has not parsed.
                container: None,
            }
        })
        .collect()
}

/// Which declarations in a file deserve an inline "N usages" row.
///
/// # The rule
///
/// **Types and callables always; named storage only when it is a member of a
/// type; nothing else.** Concretely: a class, struct, interface, trait, enum or
/// type alias, and any function, method or constructor, are anchors. A variable,
/// field, property or constant is an anchor **only** when the declaration
/// immediately enclosing it is one of those type kinds. A namespace and anything
/// [`crate::symbols::declarations::SymbolKind::Other`] never is.
///
/// The awkward case is that LSP's `Field` and `Variable` both arrive as
/// [`SymbolKind::Variable`] — [`super::protocol::symbol_kind`] collapses them
/// deliberately, because a field and a property are different things in every
/// language that has both — so the kind alone still cannot tell a field on a
/// type from a local in a method. (`Property` is now its own kind, but it does
/// not get a shortcut here: the same container rule decides it, so a server that
/// reports a property with a container this file does not know is treated like
/// any other unplaceable member.) The container chain can: a local's
/// nearest enclosing declaration is the method it lives in, and a property's is
/// the type it belongs to. Where the chain does not settle it — no container at
/// all, or a container name that is not itself in this file's symbols — the
/// answer is **no anchor**, because a missing row costs a click and a row on
/// every local in a method is noise on every line of the file.
///
/// # Ids
///
/// [`DeclarationAnchor::id`] is
/// `container.name@selection_range.start.line:selection_range.start.character` —
/// LSP's own numbering, **0-based on both**, and deliberately *not* the 1-based
/// [`DeclarationAnchor::selection_line`] that crosses IPC. The id is opaque and
/// nothing may parse it, so the units cost nothing; naming the IPC field here
/// would leave the next reader off by one. It is a function of the declaration
/// alone: it does not depend on what else
/// is in the file, so adding a sibling does not renumber it and a second call
/// over an unchanged file produces the same ids. Two same-named overloads on one
/// line differ in their selection *character*, which is why the column is in
/// there. A repeated symbol — which nothing legal produces but a server may send
/// — would otherwise collide, so an occurrence counter is appended to the second
/// and later of any exact duplicate; two rows sharing an id is a UI defect
/// rather than an honest duplicate.
pub fn anchors(symbols: &[Symbol]) -> Vec<DeclarationAnchor> {
    let kinds: HashMap<&str, SymbolKind> = symbols
        .iter()
        .map(|symbol| (symbol.name.as_str(), symbol.kind))
        .collect();

    let mut seen: HashMap<String, u32> = HashMap::new();
    symbols
        .iter()
        .filter(|symbol| admits(symbol, &kinds))
        .map(|symbol| {
            let base = format!(
                "{}{}@{}:{}",
                symbol
                    .container
                    .last()
                    .map(|c| format!("{c}."))
                    .unwrap_or_default(),
                symbol.name,
                symbol.selection_range.start.line,
                symbol.selection_range.start.character
            );
            let occurrence = seen.entry(base.clone()).or_insert(0);
            let id = if *occurrence == 0 {
                base
            } else {
                format!("{base}#{occurrence}")
            };
            *occurrence += 1;

            DeclarationAnchor {
                id,
                name: bare_name(&symbol.name),
                kind: symbol.kind,
                // Where the row draws: the whole declaration's first line.
                line: to_editor_line(symbol.range.start.line),
                // Where a `references` request must be aimed, and an offset into
                // `selection_line` rather than into `line`.
                character: symbol.selection_range.start.character,
                selection_line: to_editor_line(symbol.selection_range.start.line),
            }
        })
        .collect()
}

/// Whether this declaration earns an inline row. See [`anchors`] for the rule.
fn admits(symbol: &Symbol, kinds: &HashMap<&str, SymbolKind>) -> bool {
    match symbol.kind {
        SymbolKind::Function
        | SymbolKind::Class
        | SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::Interface
        | SymbolKind::Trait
        | SymbolKind::Type => true,
        // Named storage: a member of a type, or a local. Only the container
        // separates them.
        SymbolKind::Property | SymbolKind::Variable | SymbolKind::Constant => symbol
            .container
            .last()
            .and_then(|name| kinds.get(name.as_str()))
            .is_some_and(|kind| is_type(*kind)),
        // A namespace is not a use site anybody counts, and `Other` is the kind
        // this app gives a line it could not place.
        SymbolKind::Namespace | SymbolKind::Other => false,
    }
}

/// Whether a kind is something that can *contain* members.
fn is_type(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Interface
            | SymbolKind::Trait
            | SymbolKind::Type
    )
}

/// The identifier out of whatever the server called the symbol.
///
/// Roslyn sends a whole signature —
/// `TryGetElements(ClrObject, IReadOnlyList<ClrObject>, int) : bool` — which is
/// right for a picker row and far too wide for one drawn inline above a
/// declaration. The parameter list goes first, then a trailing ` : type`; the
/// second cut only runs when there was no parameter list, so a method whose
/// *parameter* type contains a colon cannot lose its name. Generic parameters
/// stay: `Cache<TKey, TValue>` is the name, not a signature.
fn bare_name(name: &str) -> String {
    let head = name.split_once('(').map_or(name, |(head, _)| head);
    let head = if head.len() == name.len() {
        head.split_once(" : ").map_or(head, |(head, _)| head)
    } else {
        head
    };
    head.trim().to_string()
}

#[cfg(test)]
#[path = "results_tests.rs"]
mod results_tests;
