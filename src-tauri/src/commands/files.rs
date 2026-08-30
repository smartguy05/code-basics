//! Workspace file commands, for the Run tab's directory tree and file editor.

use std::path::{Path, PathBuf};

use cb_core::files::{self, DirEntry};
use cb_core::symbols::index;
use tauri::State;

use crate::state::AppState;

/// List one directory of the workspace, filtered like the project scan.
#[tauri::command]
pub async fn fs_list_dir(
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<DirEntry>, String> {
    let root = state.workspace_root()?;
    files::list_dir(&root, &PathBuf::from(path)).map_err(|e| format!("{e:#}"))
}

/// Read a workspace file for the editor.
#[tauri::command]
pub async fn fs_read_file(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let root = state.workspace_root()?;
    files::read_file(&root, &PathBuf::from(path)).map_err(|e| format!("{e:#}"))
}

/// Save the editor's contents back to a workspace file.
#[tauri::command]
pub async fn fs_write_file(
    state: State<'_, AppState>,
    path: String,
    content: String,
) -> Result<(), String> {
    let root = state.workspace_root()?;
    let relative = PathBuf::from(&path);
    files::write_file(&root, &relative, &content).map_err(|e| format!("{e:#}"))?;
    // The file tree's paths are already relative to the workspace, so this join
    // is the identity as far as the index is concerned — it exists so that both
    // editors hand the re-index the one thing they can both state without
    // ambiguity: where the file is.
    reindex_saved_file(&state, &root.join(&relative));
    Ok(())
}

/// Create an empty file, and any parent directories it needs.
///
/// Refuses to overwrite an existing path — see `cb_core::files::create_file`.
#[tauri::command]
pub async fn fs_create_file(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let root = state.workspace_root()?;
    let relative = PathBuf::from(&path);
    files::create_file(&root, &relative).map_err(|e| format!("{e:#}"))?;
    reindex_saved_file(&state, &root.join(&relative));
    Ok(())
}

/// Create a directory, and any parent directories it needs.
///
/// Nothing is re-indexed: an empty directory declares nothing, and the index
/// keys files, not folders.
#[tauri::command]
pub async fn fs_create_dir(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let root = state.workspace_root()?;
    files::create_dir(&root, &PathBuf::from(path)).map_err(|e| format!("{e:#}"))
}

/// Rename or move a file or directory within the workspace.
///
/// The index is corrected in both directions: the old key is dropped and the
/// new one indexed. A renamed *directory* is not walked — its descendants keep
/// their old keys until the next rescan, which `unindex_moved_path` explains.
#[tauri::command]
pub async fn fs_rename(state: State<'_, AppState>, from: String, to: String) -> Result<(), String> {
    let root = state.workspace_root()?;
    let (from, to) = (PathBuf::from(from), PathBuf::from(to));
    files::rename(&root, &from, &to).map_err(|e| format!("{e:#}"))?;
    unindex_moved_path(&state, &root.join(&from));
    reindex_saved_file(&state, &root.join(&to));
    Ok(())
}

/// Delete a file, or a directory and everything under it.
#[tauri::command]
pub async fn fs_delete(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let root = state.workspace_root()?;
    let relative = PathBuf::from(&path);
    files::delete(&root, &relative).map_err(|e| format!("{e:#}"))?;
    unindex_moved_path(&state, &root.join(&relative));
    Ok(())
}

/// Drop a path that has just been deleted or moved away from the index.
///
/// The mirror of [`reindex_saved_file`], and absolute for the same reason: the
/// caller states where the file was, and `relative_to_root` re-keys it under
/// whatever root is open now.
///
/// # The directory case, stated rather than hidden
///
/// Deleting or renaming a *directory* removes every file under it, and this
/// drops only the key that names the directory itself — which the index never
/// held, so it is a no-op. The descendants keep their entries until the next
/// rescan or rebuild, and the palette will offer paths that no longer resolve.
///
/// That is a smaller wrong answer than the alternative it was weighed against.
/// Walking the deleted subtree is impossible — it is already gone — so the only
/// way to catch the descendants is a prefix sweep of the index, and a prefix
/// over `files` is exactly the operation that cannot distinguish `src/app` from
/// `src/apple`. Getting that wrong silently unindexes a directory the user
/// still has. A stale entry is visible the moment it is clicked and is
/// self-correcting; a wrongly swept one is invisible.
///
/// Failure is silent for the same reason it is in [`reindex_saved_file`]: the
/// deletion happened, and reporting it as failed because a palette entry
/// lingered would be a false report of the thing the user cares about.
fn unindex_moved_path(state: &AppState, absolute: &Path) {
    let Ok(workspace) = state.workspace() else {
        return;
    };
    let Some(relative) = index::relative_to_root(&workspace.root, absolute) else {
        return;
    };
    state.update_symbols(&workspace.root, |current| {
        index::remove_file(current, &relative)
    });
}

/// Re-index the one file that was just written, named by its absolute path.
///
/// Shared with `git_write_file` rather than copied into it. The Changes tab
/// writes working-tree files through libgit2 and the Run tab writes them
/// through `cb_core::files`, but they are the same files and a palette that
/// only noticed one of the two editors would be stale in a way that depended
/// on which tab the user happened to be in.
///
/// # Why the argument is absolute
///
/// Because the two callers do not mean the same thing by a relative path, and
/// nothing in the string says which they meant. The Run tab's paths come from
/// the workspace file tree; the Changes tab's come from git, and `Repo::open`
/// uses `Repository::discover`, so its working directory is the repository
/// root, which may be an *ancestor* of the opened workspace. Open
/// `C:/repo/src/Api` inside a repository at `C:/repo` and the Changes tab lists
/// `src/Api/Program.cs`; taking that as workspace-relative asks the index for
/// `C:/repo/src/Api/src/Api/Program.cs`, which is nothing at all. The file the
/// user edited then keeps its stale entry — the exact staleness this call
/// exists to prevent — while the path that named nothing became a palette row
/// of its own.
///
/// An absolute path is the one description both callers can produce without
/// knowing anything about the other's root, and re-keying it is
/// `cb_core::symbols::index::relative_to_root`'s single job. A file that turns
/// out not to be under the workspace at all — ordinary in a repository wider
/// than the workspace — is left alone rather than keyed against a root it does
/// not sit under.
///
/// # What it costs
///
/// Measured, not asserted — this doc claimed "microseconds" for a while and was
/// out by three orders of magnitude. On a synthesised workspace of 2,865 files
/// and 17,184 symbols, the shape of a large .NET solution, one save costs
/// roughly **0.3 ms** in `index_file` (a `stat`, a read and a word scan of the
/// one file) and **0.1 ms** in `replace_file`, against **0.17–0.27 s** to build
/// the whole index from scratch. Both are held under the symbols mutex, and
/// both are dwarfed by the third cost: `AppState::update_symbols` goes through
/// `Arc::make_mut`, which deep-clones the entire index — about 2.2 MB here, a
/// median of **2.5–3.6 ms** — whenever a search is holding the same handle, and
/// costs 100 ns when nothing is. So a save is a few milliseconds in the worst
/// case and a few hundred microseconds in the common one.
///
/// A few milliseconds is still two orders of magnitude under a rebuild, which
/// is why this happens inline on the save rather than being deferred or
/// debounced. Without it the palette goes stale the moment anybody edits
/// anything: a renamed symbol goes on being offered under its old name, and
/// jumping to it lands on a line that has moved.
///
/// Attribution is taken from the workspace's current project list, the same
/// input a full build uses, so the entry this produces is the entry a rebuild
/// would produce. A save into build output produces nothing at all —
/// `cb_core::symbols::index::replace_file` re-checks the path against the
/// walk's own exclusions, because an entry that survives until the next rebuild
/// and then vanishes is the quiet inconsistency worth avoiding.
///
/// Failure here is silent by design. The file *was* saved; telling the user
/// their save failed because a palette entry could not be refreshed would be a
/// false report of the thing they actually care about, and the index is
/// self-correcting on the next rescan or rebuild.
pub(crate) fn reindex_saved_file(state: &AppState, absolute: &Path) {
    let Ok(workspace) = state.workspace() else {
        return;
    };
    let Some(relative) = index::relative_to_root(&workspace.root, absolute) else {
        return;
    };
    let symbols = index::index_file(&workspace.root, &relative, &workspace.projects);
    // The root is handed on rather than assumed. The workspace was read above,
    // the disk read between there and here takes long enough for another
    // workspace to have been opened, and `relative` is a key that means
    // different files under different roots — so the splice is authorised
    // against this root, inside the lock that makes the check worth anything.
    // See `AppState::update_symbols`; a refusal here is the same silent,
    // self-correcting no-op as every other failure on this path.
    state.update_symbols(&workspace.root, |current| {
        index::replace_file(current, &relative, symbols)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    use cb_core::symbols::index::SymbolIndex;
    use cb_core::workspace::Workspace;

    /// A directory of this test's own under the system temp directory.
    ///
    /// `tempfile` is a dependency of `cb-core`, not of this crate, and the one
    /// thing these tests need it for — a directory nobody else is writing to —
    /// is a name and a `create_dir_all`. Named per test rather than randomised
    /// so a leftover directory from a failed run is identifiable and is cleared
    /// by the next one.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cb-app-reindex-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn workspace_at(root: &Path) -> Workspace {
        Workspace {
            root: root.to_path_buf(),
            name: "w".into(),
            projects: Vec::new(),
            configs: Vec::new(),
            solutions: Vec::new(),
            favorites: Vec::new(),
            order: Vec::new(),
        }
    }

    /// An app with `root` open and a freshly built index over it, which is the
    /// state every save arrives in.
    fn opened(root: &Path) -> AppState {
        let state = AppState::default();
        state.set_workspace(workspace_at(root)).unwrap();
        assert!(state.record_symbols(index::build(root, &[])));
        state
    }

    fn names(index: &SymbolIndex) -> Vec<&str> {
        index.symbols.iter().map(|s| s.name.as_str()).collect()
    }

    fn files(index: &SymbolIndex) -> Vec<String> {
        index
            .files
            .iter()
            .map(|f| f.to_string_lossy().replace('\\', "/"))
            .collect()
    }

    #[test]
    fn a_save_under_a_workspace_that_is_its_own_repository_refreshes_that_file() {
        // The common arrangement: the user opened the repository root, so the
        // two roots coincide and every path means the same thing under either.
        let root = scratch("same-root");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.rs"), "pub fn before() {}\n").unwrap();
        let state = opened(&root);

        fs::write(root.join("src/a.rs"), "pub fn after() {}\n").unwrap();
        reindex_saved_file(&state, &root.join("src/a.rs"));

        let index = state.symbols().unwrap();
        assert_eq!(names(&index), ["after"]);
        assert_eq!(files(&index), ["src/a.rs"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_save_named_from_a_repository_above_the_workspace_refreshes_the_edited_file() {
        // The Changes tab's paths are relative to the repository. With the
        // workspace opened below it, `src/Api/Program.cs` and `Program.cs` are
        // the same file, and only the second is a key this index knows.
        let repo = scratch("repo-above");
        let root = repo.join("src/Api");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("Program.cs"), "public class Before {}\n").unwrap();
        let state = opened(&root);

        fs::write(root.join("Program.cs"), "public class After {}\n").unwrap();
        // Spelled the way libgit2 spells a working directory joined with a
        // status path, mixed separators and all.
        reindex_saved_file(&state, &repo.join("src/Api/Program.cs"));

        let index = state.symbols().unwrap();
        assert_eq!(names(&index), ["After"]);
        assert_eq!(
            files(&index),
            ["Program.cs"],
            "the repository-relative spelling must not become a second entry"
        );
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn a_save_elsewhere_in_the_repository_leaves_the_workspaces_index_alone() {
        // A repository wider than the workspace contains files the workspace
        // does not, and the diff view can save one. It belongs to no key here.
        let repo = scratch("outside-workspace");
        let root = repo.join("src/Api");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(repo.join("src/Web")).unwrap();
        fs::write(root.join("Program.cs"), "public class Api {}\n").unwrap();
        fs::write(repo.join("src/Web/Program.cs"), "public class Web {}\n").unwrap();
        let state = opened(&root);

        reindex_saved_file(&state, &repo.join("src/Web/Program.cs"));

        let index = state.symbols().unwrap();
        assert_eq!(names(&index), ["Api"]);
        assert_eq!(files(&index), ["Program.cs"]);
        let _ = fs::remove_dir_all(&repo);
    }
}
