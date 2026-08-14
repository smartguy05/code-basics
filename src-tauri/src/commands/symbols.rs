//! The command palette's surface: searching the workspace, and asking after
//! the index behind it.
//!
//! Thin, like the rest of this layer. Every decision — what a query means, how
//! candidates are ranked, what "still indexing" looks like — is in
//! `cb_core::symbols`, which is tested without a windowing system. What is left
//! here is resolving state and calling it.
//!
//! # Nothing here returns an error for a missing index
//!
//! The index is built in the background precisely so that opening a workspace
//! does not block on it, which means the first few hundred milliseconds of
//! every session are spent with no index at all. That is a normal state, not a
//! failure, and reporting it as one would put an error toast in front of a user
//! who did nothing wrong and can only wait. So [`search_everywhere`] answers
//! with whatever it *can* rank — which is not nothing, because run
//! configurations come from the workspace and need no index at all (see
//! [`hits_for`]) — and [`symbol_index_status`] is the command that says why the
//! rest of the answer is missing, which is the whole reason a separate status
//! command exists rather than a flag smuggled onto the results.

use std::path::PathBuf;
use std::sync::Arc;

use cb_core::model::RunConfig;
use cb_core::symbols::index::{SymbolIndex, SymbolIndexStatus};
use cb_core::symbols::search::{self, Query, SearchHit, SearchScope};
use tauri::{Manager, State};

use crate::state::AppState;

/// How many hits to rank when the caller does not say.
///
/// A screenful and a little. The ranking is bounded to this number rather than
/// truncated after the fact, so asking for more is real work, and a palette
/// nobody scrolls past row twenty gains nothing from a longer list.
const DEFAULT_LIMIT: usize = 50;

/// Rank everything in the workspace that could match what the user typed.
///
/// The query text is passed through verbatim, trailing `:123` and all:
/// `cb_core::symbols::search` parses that suffix, and it is the only place that
/// does. Re-splitting it here — or in the TypeScript — would be a second
/// implementation of a rule that decides where the editor jumps.
#[tauri::command]
pub async fn search_everywhere(
    state: State<'_, AppState>,
    query: String,
    scope: SearchScope,
    limit: Option<u32>,
) -> Result<Vec<SearchHit>, String> {
    // Configurations come from the open workspace, so an Action hit can name
    // something to run. No workspace means no actions rather than an error, for
    // the same reason no index means no rows: the palette is reachable before
    // anything is open.
    let configs = state.workspace().map(|w| w.configs).unwrap_or_default();

    let query = Query {
        text: query,
        scope,
        limit: limit.map_or(DEFAULT_LIMIT, |n| n as usize),
    };

    // `state.symbols()` handed back an `Arc` and released the lock, so this
    // runs outside the mutex — a save reindexing a file does not queue behind
    // a search, and a search does not queue behind a background build.
    Ok(hits_for(state.symbols(), &configs, &query))
}

/// Rank a query against whatever index there is — including none.
///
/// # Why a missing index is not an early return
///
/// It used to be one, and that was wrong: the three populations a query can
/// match do not all come from the index. Files and symbols do; **run
/// configurations come from the workspace**, which is loaded synchronously
/// before the build is even spawned. Returning early because there is no index
/// therefore refused an answer that was fully available, for the 637 ms a real
/// solution takes to index warm and the 9.4 s it takes cold — and Ctrl+Shift+A,
/// which is documented as the binding for run configurations, was empty for the
/// whole of it.
///
/// So a missing index becomes an *empty* index rather than a refusal. That is
/// not a trick to reuse a code path: it is the accurate statement of what is
/// known. Zero files have been indexed and zero symbols have been found, which
/// is exactly what an index with empty lists says, and
/// `cb_core::symbols::search` then contributes no file or symbol hits and ranks
/// the configurations normally. `search` never reads
/// [`SymbolIndex::root`] — it walks `files`, `symbols` and the configurations
/// and nothing else — so the empty root below cannot reach a result, and it is
/// empty rather than the workspace's because this index describes no walk of
/// any root and naming one would claim a build that never happened.
///
/// It remains [`symbol_index_status`]'s job, not this one's, to say that the
/// file and symbol halves of the answer are still missing. A result list cannot
/// distinguish "nothing matched" from "nothing was searched", which is the whole
/// reason that command exists.
fn hits_for(
    index: Option<Arc<SymbolIndex>>,
    configs: &[RunConfig],
    query: &Query,
) -> Vec<SearchHit> {
    let index = index.unwrap_or_else(|| {
        Arc::new(SymbolIndex {
            root: PathBuf::new(),
            files: Vec::new(),
            symbols: Vec::new(),
            truncated: false,
        })
    });
    search::search(&index, configs, query)
}

/// Whether there is an index to search, and whether one is being built.
#[tauri::command]
pub async fn symbol_index_status(state: State<'_, AppState>) -> Result<SymbolIndexStatus, String> {
    let index = state.symbols();
    Ok(SymbolIndexStatus::of(
        index.as_deref(),
        state.symbols_building(),
    ))
}

/// Throw the cache away and re-read the workspace from source, in the
/// background.
///
/// Returns as soon as the build is started, not when it finishes. A rebuild is
/// the escape hatch for an index the user can see is wrong, and on the .NET
/// solution this application was written for it takes the better part of a
/// second warm and nine seconds against a cold filesystem — long enough that
/// blocking the IPC call would freeze the window. The caller watches
/// [`symbol_index_status`] instead.
///
/// The old index deliberately stays in place until the new one lands. It is
/// stale by assumption, which is why the user asked, but a stale palette is
/// still more useful than an empty one for the seconds this takes.
#[tauri::command]
pub async fn rebuild_symbol_index(app: tauri::AppHandle) -> Result<(), String> {
    let workspace = app.state::<AppState>().workspace()?;
    spawn_build(app.clone(), workspace, Rebuild::FromSource);
    Ok(())
}

/// Whether a build may reuse `.code-basics/symbols.json`.
#[derive(Clone, Copy)]
pub enum Rebuild {
    /// The ordinary path: trust the per-file fingerprints the cache recorded.
    Cached,
    /// Ignore the cache entirely. Only for a user who has asked, because it
    /// costs the full walk.
    FromSource,
}

/// Build the index for `workspace` on a background thread and store it.
///
/// **Returns immediately.** This is the point of the whole arrangement: a cold
/// build measured 20 ms on this repository but 637 ms on a real 2,864-file .NET
/// solution and 9.4 s against a cold filesystem cache, and every one of those
/// milliseconds would otherwise sit between the user pressing "open" and seeing
/// their projects.
///
/// The stale-write guard lives in [`AppState::record_symbols`] rather than
/// here, with the reasoning: a user who opens A and then B before A finishes
/// must not get A's file list under B's root.
///
/// # What happens when the build panics
///
/// The panic is not caught, and what that means differs by profile — the note
/// that used to be here asserted both halves at once and contradicted itself,
/// so both are now stated separately, each checked:
///
/// * **In a debug build** (what `pnpm tauri dev` runs) the panic unwinds this
///   thread and no other. The window survives, every other feature goes on
///   working, and the palette is left with whatever index it already had.
/// * **In a release build** the root `Cargo.toml` sets `panic = "abort"` under
///   `[profile.release]` (line 32), so the panic takes the whole process. It
///   could not be caught here even if catching it were wanted.
///
/// In neither case is the build's death allowed to disable the recovery from
/// it. The `building` flag is owned by the [`crate::state::SymbolsBuildGuard`]
/// moved into the thread below rather than cleared at the end of it, so an
/// unwind clears the flag on the way out and the palette's Rebuild button —
/// which is disabled while the flag is set — is still there to be pressed. In
/// the aborting build there is nothing left to enable.
pub fn spawn_build(app: tauri::AppHandle, workspace: cb_core::workspace::Workspace, mode: Rebuild) {
    let building = app.state::<AppState>().begin_symbols_build();

    std::thread::spawn(move || {
        // Moved in and held for the whole build. It clears the "building" flag
        // when this closure ends, by any route, unless a newer build has since
        // taken ownership of the flag.
        let _building = building;

        let index = match mode {
            Rebuild::Cached => {
                cb_core::symbols::cache::build_cached(&workspace.root, &workspace.projects)
            }
            Rebuild::FromSource => {
                cb_core::symbols::cache::rebuild(&workspace.root, &workspace.projects)
            }
        };
        app.state::<AppState>().record_symbols(index);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    use cb_core::model::{ConfigSource, RunKind};
    use cb_core::symbols::search::HitKind;

    #[test]
    fn a_configuration_still_matches_while_the_index_is_still_being_built() {
        let configs = [RunConfig::new(
            "c1",
            "Run Api",
            RunKind::App,
            "dotnet",
            ConfigSource::Detected,
        )];
        let query = Query {
            text: "runapi".to_string(),
            scope: SearchScope::All,
            limit: DEFAULT_LIMIT,
        };

        let hits = hits_for(None, &configs, &query);

        assert_eq!(hits.len(), 1, "got {:?}", hits);
        assert_eq!(hits[0].kind, HitKind::Action);
        assert_eq!(hits[0].action_id.as_deref(), Some("c1"));
    }
}
