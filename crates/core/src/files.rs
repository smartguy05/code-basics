//! Workspace file access for the directory tree and file editor.
//!
//! Listing is lazy — one directory per call — so opening a workspace never
//! pays for walking the whole tree, and filtered the same way the project
//! scanner filters (`node_modules`, `bin`, `obj`, …) so the tree shows the
//! files a developer edits rather than build output.

use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::workspace::should_skip;

/// Refuse to open files larger than this in the editor. Anything bigger is
/// build output or data, not something to edit in a run-tab pane.
const MAX_EDITABLE_BYTES: u64 = 5 * 1024 * 1024;

/// One entry in a directory listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    /// Workspace-relative path, usable directly in `read_file`/`list_dir`.
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Resolve a workspace-relative path, refusing anything that could escape the
/// root: absolute paths, drive prefixes, and `..` components.
fn resolve(root: &Path, relative: &Path) -> Result<PathBuf> {
    for component in relative.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            _ => bail!("path escapes the workspace: {}", relative.display()),
        }
    }
    Ok(root.join(relative))
}

/// List one directory, directories first, both halves sorted case-insensitively.
///
/// Entries the project scanner skips (`SKIP_DIRS`) are hidden here too.
pub fn list_dir(root: &Path, relative: &Path) -> Result<Vec<DirEntry>> {
    let dir = resolve(root, relative)?;
    let mut entries = Vec::new();

    let read =
        std::fs::read_dir(&dir).with_context(|| format!("could not list {}", dir.display()))?;

    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() && should_skip(&name) {
            continue;
        }
        entries.push(DirEntry {
            path: relative.join(&name),
            is_dir: file_type.is_dir(),
            name,
        });
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}

/// Read a workspace file for the editor.
pub fn read_file(root: &Path, relative: &Path) -> Result<String> {
    let path = resolve(root, relative)?;

    let metadata = std::fs::metadata(&path)
        .with_context(|| format!("could not open {}", relative.display()))?;
    if metadata.len() > MAX_EDITABLE_BYTES {
        bail!(
            "{} is too large to edit here ({} MB)",
            relative.display(),
            metadata.len() / (1024 * 1024)
        );
    }

    let bytes =
        std::fs::read(&path).with_context(|| format!("could not read {}", relative.display()))?;
    String::from_utf8(bytes)
        .map_err(|_| anyhow::anyhow!("{} is not a text file", relative.display()))
}

/// Save the editor's contents back to a workspace file.
pub fn write_file(root: &Path, relative: &Path, content: &str) -> Result<()> {
    let path = resolve(root, relative)?;
    std::fs::write(&path, content)
        .with_context(|| format!("could not write {}", relative.display()))
}

/// A path the caller wants to *create* or *move to*.
///
/// Stricter than [`resolve`], which only has to keep a read inside the
/// workspace: creating something needs a non-empty name that is not already
/// taken, and the empty path — which resolves to the workspace root itself —
/// has to be refused explicitly or "New file" with a blank name would target
/// the root directory.
fn resolve_new(root: &Path, relative: &Path) -> Result<PathBuf> {
    if relative.as_os_str().is_empty() {
        bail!("a name is required");
    }
    let path = resolve(root, relative)?;
    if path == root {
        bail!("a name is required");
    }
    if path.exists() {
        bail!("{} already exists", relative.display());
    }
    Ok(path)
}

/// Create an empty file, and any parent directories it needs.
///
/// Refuses to overwrite: an existing path is an error rather than a silent
/// truncation, because this is reached from a "New file" menu where clobbering
/// the user's work would be unrecoverable.
pub fn create_file(root: &Path, relative: &Path) -> Result<()> {
    let path = resolve_new(root, relative)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    std::fs::write(&path, "").with_context(|| format!("could not create {}", relative.display()))
}

/// Create a directory, and any parent directories it needs.
pub fn create_dir(root: &Path, relative: &Path) -> Result<()> {
    let path = resolve_new(root, relative)?;
    std::fs::create_dir_all(&path)
        .with_context(|| format!("could not create {}", relative.display()))
}

/// Rename or move a file or directory within the workspace.
///
/// Both sides are resolved against the root, so neither the source nor the
/// destination can escape it, and an existing destination is refused rather
/// than replaced.
pub fn rename(root: &Path, from: &Path, to: &Path) -> Result<()> {
    let source = resolve(root, from)?;
    if !source.exists() {
        bail!("{} does not exist", from.display());
    }
    let destination = resolve_new(root, to)?;
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    std::fs::rename(&source, &destination)
        .with_context(|| format!("could not rename {} to {}", from.display(), to.display()))
}

/// Delete a file, or a directory and everything under it.
///
/// The workspace root itself is refused: the empty path resolves to it, and
/// "delete" on a mis-typed empty name must not erase the codebase.
pub fn delete(root: &Path, relative: &Path) -> Result<()> {
    let path = resolve(root, relative)?;
    if path == root {
        bail!("refusing to delete the workspace root");
    }
    let metadata = std::fs::symlink_metadata(&path)
        .with_context(|| format!("could not open {}", relative.display()))?;

    if metadata.is_dir() {
        std::fs::remove_dir_all(&path)
            .with_context(|| format!("could not delete {}", relative.display()))
    } else {
        std::fs::remove_file(&path)
            .with_context(|| format!("could not delete {}", relative.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, contents) in files {
            let full = dir.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, contents).unwrap();
        }
        dir
    }

    #[test]
    fn lists_directories_first_then_files_alphabetically() {
        let dir = workspace_with(&[
            ("zebra.txt", ""),
            ("Apple.txt", ""),
            ("src/main.rs", ""),
            ("docs/readme.md", ""),
        ]);

        let names: Vec<String> = list_dir(dir.path(), Path::new(""))
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();

        assert_eq!(names, ["docs", "src", "Apple.txt", "zebra.txt"]);
    }

    #[test]
    fn skips_the_directories_the_scanner_skips() {
        let dir = workspace_with(&[
            ("node_modules/dep/index.js", ""),
            ("bin/Debug/app.dll", ""),
            ("src/main.rs", ""),
        ]);

        let names: Vec<String> = list_dir(dir.path(), Path::new(""))
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();

        assert_eq!(names, ["src"]);
    }

    #[test]
    fn a_skipped_name_is_still_listed_as_a_file() {
        // Only *directories* named like build output are noise; a file named
        // `bin` (no extension) is a legitimate file.
        let dir = workspace_with(&[("bin", "#!/bin/sh")]);

        let entries = list_dir(dir.path(), Path::new("")).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].is_dir);
    }

    #[test]
    fn listing_a_subdirectory_returns_relative_paths() {
        let dir = workspace_with(&[("src/app/main.rs", "")]);

        let entries = list_dir(dir.path(), Path::new("src")).unwrap();
        assert_eq!(entries[0].path, PathBuf::from("src").join("app"));
    }

    #[test]
    fn reads_and_writes_round_trip() {
        let dir = workspace_with(&[("src/main.rs", "fn main() {}")]);
        let relative = Path::new("src/main.rs");

        assert_eq!(read_file(dir.path(), relative).unwrap(), "fn main() {}");

        write_file(dir.path(), relative, "fn main() { println!(); }").unwrap();
        assert_eq!(
            read_file(dir.path(), relative).unwrap(),
            "fn main() { println!(); }"
        );
    }

    #[test]
    fn refuses_paths_that_escape_the_workspace() {
        let dir = workspace_with(&[("a.txt", "")]);

        assert!(list_dir(dir.path(), Path::new("..")).is_err());
        assert!(read_file(dir.path(), Path::new("../secrets.txt")).is_err());
        assert!(write_file(dir.path(), Path::new("sub/../../x.txt"), "").is_err());
        assert!(read_file(dir.path(), Path::new("C:/Windows/win.ini")).is_err());
    }

    #[test]
    fn a_binary_file_is_a_clear_error_not_garbage() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app.dll"), [0u8, 159, 146, 150]).unwrap();

        let error = read_file(dir.path(), Path::new("app.dll")).unwrap_err();
        assert!(error.to_string().contains("not a text file"));
    }

    #[test]
    fn dir_entry_serialises_with_the_keys_the_ui_reads() {
        // `src/ipc/types.ts` mirrors this by hand, like the model types.
        let entry = DirEntry {
            name: "src".into(),
            path: PathBuf::from("src"),
            is_dir: true,
        };
        let json = serde_json::to_value(&entry).unwrap();
        let mut keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();

        assert_eq!(keys, ["isDir", "name", "path"]);
    }

    #[test]
    fn creates_a_file_and_its_missing_parents() {
        let dir = workspace_with(&[]);

        create_file(dir.path(), Path::new("src/app/new.rs")).unwrap();

        assert_eq!(
            read_file(dir.path(), Path::new("src/app/new.rs")).unwrap(),
            ""
        );
    }

    #[test]
    fn refuses_to_create_over_something_that_exists() {
        let dir = workspace_with(&[("src/main.rs", "fn main() {}")]);

        let error = create_file(dir.path(), Path::new("src/main.rs")).unwrap_err();
        assert!(error.to_string().contains("already exists"));
        // The original is untouched — this is the whole point of refusing.
        assert_eq!(
            read_file(dir.path(), Path::new("src/main.rs")).unwrap(),
            "fn main() {}"
        );

        assert!(create_dir(dir.path(), Path::new("src")).is_err());
    }

    #[test]
    fn an_empty_name_is_an_error_not_the_workspace_root() {
        let dir = workspace_with(&[("a.txt", "")]);

        assert!(create_file(dir.path(), Path::new("")).is_err());
        assert!(create_dir(dir.path(), Path::new("")).is_err());
        assert!(create_file(dir.path(), Path::new(".")).is_err());
        assert!(dir.path().exists());
    }

    #[test]
    fn creating_refuses_paths_that_escape_the_workspace() {
        let dir = workspace_with(&[]);

        assert!(create_file(dir.path(), Path::new("../escape.txt")).is_err());
        assert!(create_dir(dir.path(), Path::new("../escape")).is_err());
        assert!(rename(dir.path(), Path::new("a.txt"), Path::new("../a.txt")).is_err());
        assert!(delete(dir.path(), Path::new("../a.txt")).is_err());
    }

    #[test]
    fn creates_a_directory() {
        let dir = workspace_with(&[]);

        create_dir(dir.path(), Path::new("src/views")).unwrap();

        assert!(list_dir(dir.path(), Path::new("src/views"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn renames_a_file_and_leaves_nothing_behind() {
        let dir = workspace_with(&[("src/old.rs", "contents")]);

        rename(dir.path(), Path::new("src/old.rs"), Path::new("src/new.rs")).unwrap();

        assert_eq!(
            read_file(dir.path(), Path::new("src/new.rs")).unwrap(),
            "contents"
        );
        assert!(read_file(dir.path(), Path::new("src/old.rs")).is_err());
    }

    #[test]
    fn renaming_into_a_new_directory_creates_it() {
        let dir = workspace_with(&[("old.rs", "contents")]);

        rename(
            dir.path(),
            Path::new("old.rs"),
            Path::new("src/moved/new.rs"),
        )
        .unwrap();

        assert_eq!(
            read_file(dir.path(), Path::new("src/moved/new.rs")).unwrap(),
            "contents"
        );
    }

    #[test]
    fn renaming_refuses_a_missing_source_and_an_occupied_destination() {
        let dir = workspace_with(&[("a.txt", "a"), ("b.txt", "b")]);

        assert!(rename(dir.path(), Path::new("missing.txt"), Path::new("c.txt")).is_err());

        let error = rename(dir.path(), Path::new("a.txt"), Path::new("b.txt")).unwrap_err();
        assert!(error.to_string().contains("already exists"));
        // Neither side moved.
        assert_eq!(read_file(dir.path(), Path::new("a.txt")).unwrap(), "a");
        assert_eq!(read_file(dir.path(), Path::new("b.txt")).unwrap(), "b");
    }

    #[test]
    fn deletes_a_file_and_a_whole_directory() {
        let dir = workspace_with(&[("keep.txt", ""), ("drop.txt", ""), ("sub/a.txt", "")]);

        delete(dir.path(), Path::new("drop.txt")).unwrap();
        delete(dir.path(), Path::new("sub")).unwrap();

        let names: Vec<String> = list_dir(dir.path(), Path::new(""))
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["keep.txt"]);
    }

    #[test]
    fn deleting_a_missing_path_is_an_error_not_a_silent_success() {
        let dir = workspace_with(&[("a.txt", "")]);

        assert!(delete(dir.path(), Path::new("missing.txt")).is_err());
    }

    #[test]
    fn refuses_to_delete_the_workspace_root() {
        let dir = workspace_with(&[("a.txt", "")]);

        let error = delete(dir.path(), Path::new("")).unwrap_err();
        assert!(error.to_string().contains("workspace root"));
        assert!(dir.path().exists());
    }
}
