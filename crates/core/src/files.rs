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

    let read = std::fs::read_dir(&dir)
        .with_context(|| format!("could not list {}", dir.display()))?;

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

    let bytes = std::fs::read(&path)
        .with_context(|| format!("could not read {}", relative.display()))?;
    String::from_utf8(bytes)
        .map_err(|_| anyhow::anyhow!("{} is not a text file", relative.display()))
}

/// Save the editor's contents back to a workspace file.
pub fn write_file(root: &Path, relative: &Path, content: &str) -> Result<()> {
    let path = resolve(root, relative)?;
    std::fs::write(&path, content)
        .with_context(|| format!("could not write {}", relative.display()))
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
}
