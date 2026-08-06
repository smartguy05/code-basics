//! Change groups: named buckets for the files in a working tree.
//!
//! Git has no such concept — the index is the only grouping it knows — so this
//! is purely local bookkeeping, in the spirit of JetBrains' changelists. A
//! group records *which files belong together* while work is in progress, so a
//! half-finished refactor can sit visibly apart from the fix being committed
//! next.
//!
//! Consequences of git not knowing about them:
//!
//! * A file belongs to at most one group. Anything else would need somewhere
//!   to record precedence, and the UI has nowhere sensible to show it.
//! * Groups are personal, not shared: they live in
//!   `.code-basics/changelists.json`, which is gitignored alongside `results/`.
//!   Committing them would push one person's work-in-progress structure onto
//!   everyone else.
//! * Assignments survive a file becoming clean. A file that is committed and
//!   later edited again returns to the group it was in, which is more useful
//!   than silently forgetting.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

pub const CHANGELISTS_FILE: &str = "changelists.json";

/// One named group of files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Changelist {
    pub name: String,
    /// Workspace-relative paths with forward slashes, matching what
    /// `git status` reports.
    #[serde(default)]
    pub paths: Vec<String>,
}

/// The whole file: every group, in the order they are shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Changelists {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub groups: Vec<Changelist>,
}

fn default_version() -> u32 {
    1
}

impl Default for Changelists {
    fn default() -> Self {
        Self {
            version: default_version(),
            groups: Vec::new(),
        }
    }
}

impl Changelists {
    /// The group a path belongs to, if any.
    pub fn group_of(&self, path: &str) -> Option<&str> {
        let path = normalise(path);
        self.groups
            .iter()
            .find(|g| g.paths.contains(&path))
            .map(|g| g.name.as_str())
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.groups.iter().position(|g| g.name == name)
    }
}

/// Separators are normalised so a path always compares equal to the one
/// `git status` reports, whichever side wrote it.
fn normalise(path: &str) -> String {
    path.replace('\\', "/")
}

pub fn changelists_path(root: &Path) -> PathBuf {
    crate::config::config_dir(root).join(CHANGELISTS_FILE)
}

/// Read the groups, returning an empty set when the file is absent.
pub fn load(root: &Path) -> Result<Changelists> {
    let path = changelists_path(root);
    if !path.exists() {
        return Ok(Changelists::default());
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    serde_json::from_str(&content)
        .with_context(|| format!("{} is not valid change-group JSON", path.display()))
}

pub fn save(root: &Path, lists: &Changelists) -> Result<()> {
    let dir = crate::config::config_dir(root);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create {}", dir.display()))?;
    crate::config::ensure_gitignore(&dir)?;

    let path = changelists_path(root);
    let json = serde_json::to_string_pretty(lists).context("failed to serialise change groups")?;

    std::fs::write(&path, format!("{json}\n"))
        .with_context(|| format!("failed to write {}", path.display()))
}

/// Add an empty group.
pub fn create(root: &Path, name: &str) -> Result<Changelists> {
    let name = name.trim();
    if name.is_empty() {
        bail!("a change group needs a name");
    }

    let mut lists = load(root)?;
    if lists.position(name).is_some() {
        bail!("a change group called \"{name}\" already exists");
    }

    lists.groups.push(Changelist {
        name: name.to_string(),
        paths: Vec::new(),
    });
    save(root, &lists)?;
    Ok(lists)
}

/// Delete a group. Its files become ungrouped rather than disappearing.
pub fn remove(root: &Path, name: &str) -> Result<Changelists> {
    let mut lists = load(root)?;
    let index = lists
        .position(name)
        .with_context(|| format!("no change group called \"{name}\""))?;

    lists.groups.remove(index);
    save(root, &lists)?;
    Ok(lists)
}

pub fn rename(root: &Path, from: &str, to: &str) -> Result<Changelists> {
    let to = to.trim();
    if to.is_empty() {
        bail!("a change group needs a name");
    }

    let mut lists = load(root)?;
    if from != to && lists.position(to).is_some() {
        bail!("a change group called \"{to}\" already exists");
    }

    let index = lists
        .position(from)
        .with_context(|| format!("no change group called \"{from}\""))?;
    lists.groups[index].name = to.to_string();

    save(root, &lists)?;
    Ok(lists)
}

/// Put paths in a group, or remove them from every group when `group` is
/// `None`.
///
/// A path is removed from whatever group it was in first, so membership stays
/// single-valued without the caller having to unassign explicitly.
pub fn assign(root: &Path, paths: &[String], group: Option<&str>) -> Result<Changelists> {
    let mut lists = load(root)?;
    let normalised: Vec<String> = paths.iter().map(|p| normalise(p)).collect();

    let target = match group {
        Some(name) => Some(
            lists
                .position(name)
                .with_context(|| format!("no change group called \"{name}\""))?,
        ),
        None => None,
    };

    for list in &mut lists.groups {
        list.paths.retain(|p| !normalised.contains(p));
    }

    if let Some(index) = target {
        for path in normalised {
            if !lists.groups[index].paths.contains(&path) {
                lists.groups[index].paths.push(path);
            }
        }
        lists.groups[index].paths.sort();
    }

    save(root, &lists)?;
    Ok(lists)
}

#[cfg(test)]
#[path = "changelists_tests.rs"]
mod tests;
