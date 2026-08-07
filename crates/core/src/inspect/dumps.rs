//! Crash dumps on disk: arming them, recognising them, matching them, pruning
//! them.
//!
//! # Why this needs no tooling
//!
//! The .NET runtime will write a dump of a process as it dies, on its own, if
//! three environment variables are set on it. No code change, no debugger, no
//! `dotnet-dump` install:
//!
//! ```text
//! DOTNET_DbgEnableMiniDump = 1
//! DOTNET_DbgMiniDumpType   = 2      // Heap — the type ClrMD can read
//! DOTNET_DbgMiniDumpName   = <template>
//! ```
//!
//! That is what turns "it crashed while I was at lunch" into something
//! inspectable: the crash is already captured by the time anyone looks.
//!
//! # Why the filename carries the executable
//!
//! Those variables are inherited by the **whole process tree**, so a
//! `dotnet run` arms its build host and any child it starts as well. Several
//! unrelated processes may therefore be armed by one run configuration, and a
//! dump on disk has to be attributable to the thing the user actually ran. The
//! runtime's own template expansion is the only labelling available at the
//! moment of the crash, so the name carries it: `%e_%p_%t.dmp`.
//!
//! **`%e` expands to the executable name including its extension.** A real
//! capture landed as `Crasher.exe_25764_1786044924.dmp`, not
//! `Crasher_25764_...`. That is confirmed by observation, and both the
//! template and the parser here are built around it.
//!
//! # The rule this module follows
//!
//! The same rule as the rest of [`crate::inspect`]: a wrong value is much
//! worse than no value. Here that means [`parse_dump_name`] returns `None` for
//! anything that is not exactly what [`dump_env`] would have produced, and
//! [`prune`] only ever deletes files it recognised that way. A file a user
//! dropped into the dumps directory by hand is not this module's to delete,
//! and a name it cannot fully decode is not one it will guess about.
//!
//! # Why this is opt-in
//!
//! A dump of a trivial console app measured 9.3 MB; a real service produces
//! hundreds of megabytes, per crash. Capture is per-workspace opt-in
//! ([`crate::inspect::InspectorConfig::capture_dumps`], default false) and
//! pruned by both count and total bytes.
//!
//! Dumps only ever appear for an **unhandled** crash. A caught exception
//! writes nothing, and neither does a force-kill — [`crate::process::kill`]
//! uses `taskkill /T /F`, so stopping a run from the UI correctly leaves no
//! dump behind.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::model::DumpFile;

/// Directory under `.code-basics/` holding crash dumps.
pub const DUMPS_DIR: &str = "dumps";

/// Extension the runtime writes, and the only one recognised here.
const DUMP_EXTENSION: &str = "dmp";

/// `<root>/.code-basics/dumps`
pub fn dumps_dir(root: &Path) -> PathBuf {
    crate::config::config_dir(root).join(DUMPS_DIR)
}

/// A dump filename decoded back into its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDumpName {
    /// As the runtime wrote it, extension included — `Crasher.exe`.
    pub executable: String,
    pub pid: u32,
    /// Unix seconds.
    pub captured_at: u64,
}

// ---------------------------------------------------------------------------
// Arming a run
// ---------------------------------------------------------------------------

/// The three `DOTNET_Dbg*` variables, ready to layer into an invocation.
///
/// Returns the map and nothing else. The caller layers it **under** the user's
/// own environment — exactly as [`crate::process`] layers its colour defaults —
/// so a user who sets `DOTNET_DbgMiniDumpType` themselves still wins.
pub fn dump_env(dumps_dir: &Path) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("DOTNET_DbgEnableMiniDump".to_string(), "1".to_string());
    // 2 is Heap. 1 (Mini) and 3 (Triage) omit the object data that is the
    // entire point of inspecting, and 4 (Full) adds the whole address space
    // for no gain to ClrMD at a large cost in bytes.
    env.insert("DOTNET_DbgMiniDumpType".to_string(), "2".to_string());
    env.insert(
        "DOTNET_DbgMiniDumpName".to_string(),
        name_template(dumps_dir),
    );
    env
}

/// The path template handed to the runtime.
///
/// `%e` executable (with extension), `%p` pid, `%t` unix seconds. Kept beside
/// [`parse_dump_name`] because the two must stay exact inverses.
fn name_template(dumps_dir: &Path) -> String {
    dumps_dir
        .join(format!("%e_%p_%t.{DUMP_EXTENSION}"))
        .display()
        .to_string()
}

// ---------------------------------------------------------------------------
// Recognising one of ours
// ---------------------------------------------------------------------------

/// Decode a filename written by [`name_template`], or `None`.
///
/// Parsed from the **right**: the pid and timestamp are the last two
/// underscore-separated fields, and everything before them is the executable.
/// An executable name may itself contain underscores and dots
/// (`My_App.exe_123_456.dmp`), so splitting from the left would attribute the
/// dump to a process that does not exist.
pub fn parse_dump_name(name: &str) -> Option<ParsedDumpName> {
    let stem = name.strip_suffix(&format!(".{DUMP_EXTENSION}"))?;

    let (rest, timestamp) = stem.rsplit_once('_')?;
    let (executable, pid) = rest.rsplit_once('_')?;

    if executable.is_empty() {
        return None;
    }

    Some(ParsedDumpName {
        // `str::parse` accepts a leading `+` and surrounding nothing else, so
        // digits are checked explicitly: `Foo_+1_2.dmp` is not something this
        // module wrote, and must not be treated as though it were.
        pid: all_digits(pid).then(|| pid.parse().ok())??,
        captured_at: all_digits(timestamp).then(|| timestamp.parse().ok())??,
        executable: executable.to_string(),
    })
}

fn all_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// Listing and matching
// ---------------------------------------------------------------------------

/// Every recognised dump in a directory, newest first.
///
/// Ordered by the timestamp **in the filename**, not by mtime: the filename is
/// what the runtime stamped at the moment of the crash, while mtime is subject
/// to copies, restores and coarse filesystem resolution.
///
/// A directory that does not exist yet is an empty list, not an error —
/// nothing has crashed yet is a normal state, not a problem to report.
pub fn list(dir: &Path) -> Vec<DumpFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut dumps: Vec<DumpFile> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let parsed = parse_dump_name(name.to_str()?)?;
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            Some(DumpFile {
                path: entry.path(),
                executable: parsed.executable,
                pid: parsed.pid,
                captured_at: parsed.captured_at,
                bytes: metadata.len(),
            })
        })
        .collect();

    // Path breaks ties so the order is deterministic when two dumps share a
    // second, which they do when a process tree dies all at once.
    dumps.sort_by(|a, b| {
        b.captured_at
            .cmp(&a.captured_at)
            .then_with(|| a.path.cmp(&b.path))
    });
    dumps
}

/// The newest dump for an executable, captured at or after `since`.
///
/// `since` is how a dump is attributed to a run: the caller records when it
/// started the process, and a dump older than that belongs to some earlier
/// crash. Claiming it would point the user at the wrong data, which is the one
/// outcome this feature must never produce.
///
/// The name is matched with or without `.exe`, because the caller holds a
/// project name (`MyApi`) while the file holds an executable name
/// (`MyApi.exe`). Comparison is case-insensitive: Windows paths are.
///
/// **Nothing in the application calls this yet.** The Objects tab lists every
/// dump in the workspace and lets the user choose, and no part of the UI claims
/// otherwise — attributing a dump to a run needs the run's start time, which
/// the run tab does not record. The rule is the module's rule: offer the whole
/// list rather than assert an attribution that was never made.
pub fn newest_for(dir: &Path, executable: &str, since: Option<u64>) -> Option<DumpFile> {
    let wanted = normalise_executable(executable);

    list(dir)
        .into_iter()
        .filter(|dump| since.is_none_or(|floor| dump.captured_at >= floor))
        .find(|dump| normalise_executable(&dump.executable) == wanted)
}

/// Compare on the bare name: `MyApi`, `MyApi.exe` and `myapi.EXE` are one
/// process as far as matching is concerned.
fn normalise_executable(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

// ---------------------------------------------------------------------------
// Retention
// ---------------------------------------------------------------------------

/// Delete dumps beyond the newest `keep`, and beyond a total byte budget.
///
/// Both limits apply, oldest deleted first, because either alone is wrong: a
/// count says nothing about size when one dump is 600 MB, and a byte budget
/// alone would let a hundred small dumps of somebody's production memory sit
/// there indefinitely. The budget is applied to what survives the count limit,
/// and the newest dump is kept even if it alone exceeds the budget — a limit
/// that leaves nothing to inspect has quietly turned the feature off.
///
/// Only files [`parse_dump_name`] recognises are ever considered, so anything
/// else in the directory survives untouched. A file that will not delete —
/// open in a dump viewer, most likely — is skipped rather than failing the
/// whole prune, and simply gets another chance next time.
///
/// Returns what was actually removed, so the caller reports deletions that
/// happened rather than deletions that were intended.
pub fn prune(dir: &Path, keep: usize, max_bytes: Option<u64>) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    // Cheap, but it distinguishes "nothing to prune" from "the directory is
    // unreadable", which is worth surfacing rather than silently doing nothing.
    std::fs::read_dir(dir).with_context(|| format!("failed to list {}", dir.display()))?;

    // Newest first, so both limits collapse into one question: how many of the
    // leading dumps survive?
    let dumps = list(dir);

    let mut survivors = keep.min(dumps.len());

    if let Some(budget) = max_bytes {
        let mut running = 0u64;
        for (index, dump) in dumps.iter().take(survivors).enumerate() {
            running = running.saturating_add(dump.bytes);
            if running > budget {
                // `max(1)` keeps the newest whatever its size.
                survivors = index.max(1);
                break;
            }
        }
    }

    let mut removed = Vec::new();
    // Oldest first, so a locked file or an interruption leaves the most useful
    // dumps behind rather than an arbitrary subset.
    for dump in dumps[survivors..].iter().rev() {
        if std::fs::remove_file(&dump.path).is_ok() {
            removed.push(dump.path.clone());
        }
    }

    Ok(removed)
}

/// Delete dumps written by a collector this app switched on but did not name,
/// keeping the newest ones within `max_bytes`.
///
/// VSTest's `--blame-crash-collect-always` writes its dump into the
/// `--results-directory`, under a name of its own choosing, so
/// [`parse_dump_name`] does not recognise it and [`prune`] will not touch it.
/// Those files are still hundreds of megabytes of process memory, and they
/// exist only because this application passed the flag — leaving them outside
/// the budget means the budget does not bound what the feature writes.
///
/// The rules are narrower than [`prune`] precisely because the names cannot be
/// decoded: only `.dmp` files, only under a directory this application owns and
/// regenerates (`.code-basics/results`), ordered by modification time because
/// there is no timestamp in the name to trust. Nothing else is touched, and a
/// file that will not delete is skipped rather than failing the sweep.
///
/// `max_bytes` of 0 deletes all of them: unlike [`prune`] there is no "keep the
/// newest whatever its size" exception, because the caller has already spent
/// the budget on dumps it *can* attribute to a process.
pub fn prune_unnamed(dir: &Path, max_bytes: u64) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut found = Vec::new();
    collect_unnamed(dir, 0, &mut found)?;

    // Newest first, so the survivors are the dumps of the most recent run.
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let mut running = 0u64;
    let mut removed = Vec::new();
    for (path, _, bytes) in found {
        running = running.saturating_add(bytes);
        if running > max_bytes && std::fs::remove_file(&path).is_ok() {
            removed.push(path);
        }
    }

    Ok(removed)
}

/// How deep to look for a collector's dump. VSTest nests one directory per test
/// host; a bound is cheaper than trusting a directory this app does not write.
const UNNAMED_SEARCH_DEPTH: u32 = 4;

/// `(path, modified, bytes)` for every `.dmp` under `dir`.
fn collect_unnamed(dir: &Path, depth: u32, out: &mut Vec<(PathBuf, u64, u64)>) -> Result<()> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("failed to list {}", dir.display()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };

        if metadata.is_dir() {
            if depth < UNNAMED_SEARCH_DEPTH {
                // An unreadable subdirectory is not a reason to abandon the
                // sweep of the ones that can be read.
                let _ = collect_unnamed(&path, depth + 1, out);
            }
            continue;
        }

        let is_dump = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case(DUMP_EXTENSION));
        if !is_dump {
            continue;
        }

        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        out.push((path, modified, metadata.len()));
    }

    Ok(())
}

#[cfg(test)]
#[path = "dumps_tests.rs"]
mod dumps_tests;
