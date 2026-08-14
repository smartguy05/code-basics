//! Architecture-diagram commands.
//!
//! Eight commands over `cb_core::architecture`, each a few lines: derive the
//! project graph, derive the component map, render either, list the diagrams
//! on disk, read one, save one, check some Mermaid source. Every decision they
//! could be tempted to make — which directory a diagram belongs in, what an
//! edge means, what counts as valid Mermaid — is already made in `cb-core` and
//! tested there.
//!
//! # Nothing is cached
//!
//! `arch_project_graph` re-reads every manifest on every call, and nothing is
//! kept in [`AppState`]. That is the same reasoning as `intent_groups`: the
//! inputs are files the user edits continuously while the workspace stays
//! open, so a cached graph would go stale with nothing able to notice, and a
//! stale graph here is worse than a stale intent card — it is an arrow
//! asserting a dependency that has since been deleted. The cost is a handful
//! of small file reads for an artifact a user opens deliberately rather than
//! continuously.
//!
//! # Why a broken diagram is still saved
//!
//! [`arch_write_diagram`] validates and then writes **regardless of the
//! result**, returning the error rather than raising it. Refusing the write
//! would be the reflex, and it is wrong here for a reason specific to what is
//! being edited: Mermaid source passes through invalid states on the way to
//! every valid one, so a save that refuses broken input is a save the user
//! cannot use while they are still working — they would be one editor crash,
//! one accidental tab close or one machine restart away from losing the
//! diagram they were drawing. Losing a person's work to protect them from
//! their own half-finished edit is the worse failure.
//!
//! Refusing is also unnecessary, because nothing downstream trusts a stored
//! diagram. [`cb_core::architecture::store::parse`] never fails, and the
//! renderer runs [`mermaid::validate`] over whatever it is handed before
//! drawing it — that check is the boundary, not this one. The validation done
//! here is therefore a *diagnostic*: it rides back with the successful save so
//! the UI can name the problem beside the editor without a second round trip,
//! and so it is impossible for a caller to save a broken diagram without being
//! told. `Ok(None)` means saved and valid; `Ok(Some(error))` means saved and
//! broken; `Err` means not saved at all.
//!
//! The one thing this does not do is let a save *change* a file's provenance,
//! and that refusal is absolute — but it lives in `store::write`, which takes
//! the derivation from the file already on disk rather than from the text
//! being saved.

use std::path::PathBuf;

use cb_core::architecture::components;
use cb_core::architecture::graph::{self, ArchGraph};
use cb_core::architecture::mermaid::{self, ValidationError};
use cb_core::architecture::store::{self, DiagramFile};
use cb_core::symbols::index::SymbolIndex;
use tauri::State;

use crate::state::AppState;

/// The project graph, derived from the manifests as they are on disk now.
#[tauri::command]
pub async fn arch_project_graph(state: State<'_, AppState>) -> Result<ArchGraph, String> {
    Ok(graph::project_graph(&state.workspace()?))
}

/// The current workspace rendered as Mermaid source.
///
/// Renders and returns; it does not store the result. Writing the derived
/// diagram to disk is a separate act with its own provenance, and doing it as
/// a side effect of asking to see the picture would put a regenerated file
/// under the user's `.code-basics/` every time they opened the tab.
#[tauri::command]
pub async fn arch_render_graph(state: State<'_, AppState>) -> Result<String, String> {
    Ok(mermaid::render(&graph::project_graph(&state.workspace()?)))
}

/// Every diagram stored in this workspace, committed ones first.
#[tauri::command]
pub async fn arch_list_diagrams(state: State<'_, AppState>) -> Result<Vec<DiagramFile>, String> {
    let root = state.workspace_root()?;
    store::list(&root).map_err(|e| format!("{e:#}"))
}

/// One diagram by name, exactly as it is on disk — front matter included.
#[tauri::command]
pub async fn arch_read_diagram(state: State<'_, AppState>, name: String) -> Result<String, String> {
    let root = state.workspace_root()?;
    store::read(&root, &name).map_err(|e| format!("{e:#}"))
}

/// Save a person's edit of a diagram, valid or not.
///
/// Returns the validation error the saved text carries, or `None` when there
/// is none — see the module documentation for why a broken diagram is written
/// rather than refused.
///
/// The file may **move** as a result of this call: editing a derived diagram
/// promotes it out of the gitignored regenerated directory, because a file
/// that is overwritten on every scan would throw the edit away. A caller
/// showing a list of diagrams therefore has to re-list after a save rather
/// than assume the path it had is still right.
#[tauri::command]
pub async fn arch_write_diagram(
    state: State<'_, AppState>,
    name: String,
    contents: String,
) -> Result<Option<ValidationError>, String> {
    let root = state.workspace_root()?;
    store::write(&root, &name, &contents).map_err(|e| format!("{e:#}"))?;
    Ok(mermaid::validate(&contents).err())
}

/// The component map: the services this workspace runs and the data stores
/// they declare, derived from the manifests and configuration as they are on
/// disk now.
///
/// A different question from [`arch_project_graph`], and the two must not be
/// confused: the project map is *what is in this repository*, this is *what
/// this system consists of at run time*. An empty result is therefore a real
/// answer — a repository of class libraries has no components — and this
/// command never falls back to the project graph to avoid returning one.
/// `cb_core::architecture::components` argues that at length.
#[tauri::command]
pub async fn arch_component_graph(state: State<'_, AppState>) -> Result<ArchGraph, String> {
    component_graph(&state)
}

/// The component map rendered as Mermaid source.
///
/// A second command rather than a parameter on [`arch_render_graph`], because
/// that one takes no arguments and adding one would change the shape of a call
/// the diagram view already makes. Both go through the same renderer — the
/// component map is an ordinary [`ArchGraph`], which is the whole reason
/// `components` assembles one.
///
/// The warnings do not survive rendering: Mermaid source is nodes and edges.
/// A caller that shows the picture and not [`arch_component_graph`]'s
/// `warnings` is showing a map with no way to tell what was left off it, so
/// call both.
#[tauri::command]
pub async fn arch_render_component_graph(state: State<'_, AppState>) -> Result<String, String> {
    Ok(mermaid::render(&component_graph(&state)?))
}

/// Derive the component map against whatever symbol index exists — including
/// none.
///
/// # Why a missing index is answered rather than refused
///
/// This is the palette's precedent, and it applies here for a stronger reason
/// than it does there. The index is built on a background thread after a
/// workspace opens, so the first few hundred milliseconds of every session
/// have no index at all; `commands::symbols` calls that a normal state and
/// answers with what it can. Here the missing piece is smaller still.
/// [`components::component_graph`] documents what an empty index costs — the
/// ASP.NET route producer's *details*, and nothing else, because no node and
/// no edge in this graph is ever created by a route. So the map derived
/// without an index is a **smaller** map, never a different one, and refusing
/// to draw it would withhold an answer that was fully available.
///
/// What it must not do is let the difference go unsaid, since a reader cannot
/// tell a map with no route details from one whose services declare no routes.
/// That note goes into [`ArchGraph::warnings`] — the field a caller is already
/// obliged to surface — rather than onto a separate status flag, so a UI that
/// shows the warnings cannot show this map without it. It is the one warning
/// in that list that is about the inputs being incomplete rather than about
/// the workspace not lining up.
fn component_graph(state: &State<'_, AppState>) -> Result<ArchGraph, String> {
    let workspace = state.workspace()?;
    let index = state.symbols();

    let mut graph = match &index {
        Some(index) => components::component_graph(&workspace, index),
        None => components::component_graph(&workspace, &empty_index(workspace.root.clone())),
    };

    if index.is_none() {
        graph.warnings.push(if state.symbols_building() {
            "The symbol index is still being built, so no ASP.NET route details were \
             read for this map. No box and no arrow comes from a route, so nothing \
             here is wrong — there is less of it. Ask again when indexing has \
             finished."
                .to_string()
        } else {
            "There is no symbol index, so no ASP.NET route details were read for this \
             map. No box and no arrow comes from a route, so nothing here is wrong — \
             there is less of it. Rebuilding the index and asking again will fill in \
             the routes."
                .to_string()
        });
    }

    Ok(graph)
}

/// An index with nothing in it, rooted where the workspace is.
///
/// Stands in for the index that has not been built yet. The root is carried
/// rather than left blank so that anything comparing an index against the open
/// workspace sees the truth; the emptiness is the whole point, and it costs
/// route details only.
fn empty_index(root: PathBuf) -> SymbolIndex {
    SymbolIndex {
        root,
        files: Vec::new(),
        symbols: Vec::new(),
        truncated: false,
    }
}

/// Check Mermaid source without storing it.
///
/// Invalid source is the answer, not a failure: `None` means it will render,
/// `Some(error)` names the rule and the line. Returning `Err` for a diagram
/// the user is midway through typing would make an ordinary editing state look
/// like the command itself had gone wrong.
#[tauri::command]
pub fn arch_validate(source: String) -> Option<ValidationError> {
    mermaid::validate(&source).err()
}
