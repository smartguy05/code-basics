//! The recents policy: what running a command remembers, what it forgets, and
//! how the picker orders what is left. Pure, so the clock and the workspace root
//! are arguments rather than ambient state.

use std::path::Path;

use super::model::{Launchable, LauncherFile, LauncherGroups};

/// How many unpinned recents are kept. Pinned entries are exempt — a pin is the
/// user saying "keep this", and a cap that could drop one would make pinning a
/// suggestion rather than a promise.
pub const MAX_UNPINNED: usize = 30;

/// Record that `command` ran in `cwd`, returning the entry's id.
///
/// Identity is `(command, cwd)`: the same command line in two checkouts is two
/// entries, which is what makes the "this codebase" grouping possible without a
/// second per-repository store. A re-run **updates** its entry — bumping the
/// clock and the count, and adopting the `shell` flag that actually ran — while
/// preserving the two things only the user sets: the pin and the rename.
pub fn record_run(
    file: &mut LauncherFile,
    command: &str,
    cwd: &Path,
    shell: bool,
    now_ms: u64,
) -> String {
    let command = command.trim();
    if let Some(existing) = file
        .entries
        .iter_mut()
        .find(|e| e.command == command && same_path(&e.cwd, cwd))
    {
        existing.last_run_ms = now_ms;
        existing.run_count = existing.run_count.saturating_add(1);
        existing.shell = shell;
        return existing.id.clone();
    }

    let id = uuid::Uuid::new_v4().to_string();
    file.entries.push(Launchable {
        id: id.clone(),
        command: command.to_string(),
        cwd: cwd.to_path_buf(),
        env: std::collections::BTreeMap::new(),
        label: None,
        shell,
        pinned: false,
        last_run_ms: now_ms,
        run_count: 1,
    });
    evict_oldest_unpinned(file);
    id
}

/// Drop the least recently run unpinned entries until the cap is met.
fn evict_oldest_unpinned(file: &mut LauncherFile) {
    loop {
        let unpinned = file.entries.iter().filter(|e| !e.pinned).count();
        if unpinned <= MAX_UNPINNED {
            return;
        }
        let oldest = file
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.pinned)
            .min_by_key(|(_, e)| e.last_run_ms)
            .map(|(index, _)| index);
        match oldest {
            Some(index) => {
                file.entries.remove(index);
            }
            // Unreachable while `unpinned > MAX_UNPINNED`, but a `return` here
            // rather than an `unwrap` keeps a future refactor from looping.
            None => return,
        }
    }
}

/// Split entries into the active codebase's and everything else.
///
/// Within each group: pinned first, then most recently run. `run_count` is shown
/// but deliberately does not rank — a command run once this morning is more
/// likely to be wanted than one run fifty times last year.
pub fn group(entries: &[Launchable], active_root: Option<&Path>) -> LauncherGroups {
    let mut this_codebase: Vec<Launchable> = Vec::new();
    let mut global: Vec<Launchable> = Vec::new();

    for entry in entries {
        let here = active_root.is_some_and(|root| within_root(root, &entry.cwd));
        if here {
            this_codebase.push(entry.clone());
        } else {
            global.push(entry.clone());
        }
    }

    sort_for_display(&mut this_codebase);
    sort_for_display(&mut global);
    LauncherGroups {
        this_codebase,
        global,
    }
}

fn sort_for_display(entries: &mut [Launchable]) {
    entries.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.last_run_ms.cmp(&a.last_run_ms))
            .then(a.command.cmp(&b.command))
    });
}

/// Whether `cwd` is `root` or sits inside it.
///
/// Compared on a normalised string rather than with `strip_prefix` because the
/// two sides come from different places — the workspace root the app scanned and
/// a path the user typed — so separators, a trailing slash and (on Windows) case
/// all legitimately differ. The `/` appended to the root is what stops `/repo2`
/// reading as inside `/repo`. An empty root contains nothing: it would otherwise
/// swallow every entry into "this codebase".
pub fn within_root(root: &Path, cwd: &Path) -> bool {
    let root = normalise(root);
    if root.is_empty() {
        return false;
    }
    let cwd = normalise(cwd);
    cwd == root || cwd.starts_with(&format!("{root}/"))
}

/// Whether two paths name the same directory, by the same comparison rule.
fn same_path(a: &Path, b: &Path) -> bool {
    normalise(a) == normalise(b)
}

fn normalise(path: &Path) -> String {
    let text = path
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

/// Set or clear an entry's user-given label. A blank name clears the rename
/// rather than storing an entry with no visible title. Returns whether the id
/// was found.
pub fn rename(file: &mut LauncherFile, id: &str, label: Option<&str>) -> bool {
    let Some(entry) = file.entries.iter_mut().find(|e| e.id == id) else {
        return false;
    };
    entry.label = label
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string);
    true
}

/// Pin or unpin an entry. Returns whether the id was found.
pub fn set_pinned(file: &mut LauncherFile, id: &str, pinned: bool) -> bool {
    let Some(entry) = file.entries.iter_mut().find(|e| e.id == id) else {
        return false;
    };
    entry.pinned = pinned;
    if !pinned {
        // Unpinning re-exposes the entry to the cap it was exempt from.
        evict_oldest_unpinned(file);
    }
    true
}

/// Forget an entry. Returns whether anything was removed.
pub fn remove(file: &mut LauncherFile, id: &str) -> bool {
    let before = file.entries.len();
    file.entries.retain(|e| e.id != id);
    file.entries.len() != before
}

/// Find an entry by id.
pub fn find<'a>(entries: &'a [Launchable], id: &str) -> Option<&'a Launchable> {
    entries.iter().find(|e| e.id == id)
}

#[cfg(test)]
#[path = "recents_tests.rs"]
mod tests;
