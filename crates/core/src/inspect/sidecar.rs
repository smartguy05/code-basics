//! Deciding how to call the inspector, without calling it.
//!
//! Everything here is a pure function over paths and values, in the shape of
//! [`crate::adapters::msbuild`]: the command line is built and asserted on
//! without a process ever starting, and the one function that does spawn is a
//! thin wrapper the tests do not need.
//!
//! The exchange is two files in one directory:
//!
//! ```text
//! .code-basics/inspect/<session-id>/request.json
//! .code-basics/inspect/<session-id>/result.json
//! ```
//!
//! Keeping both under a per-session directory means a capture is one unit to
//! keep, show or delete, and pruning is a directory listing rather than a
//! bookkeeping file that could disagree with the disk.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use super::graph::RawResult;
use super::model::{Bitness, DotnetProcess, InspectRequest};

/// Directory under `.code-basics/` holding inspector sessions.
pub const SESSIONS_DIR: &str = "inspect";
pub const REQUEST_FILE: &str = "request.json";
pub const RESULT_FILE: &str = "result.json";

/// How many sessions to keep before the oldest are removed.
pub const DEFAULT_KEEP_SESSIONS: usize = 20;

// ---------------------------------------------------------------------------
// Where things live
// ---------------------------------------------------------------------------

pub fn sessions_dir(root: &Path) -> PathBuf {
    crate::config::config_dir(root).join(SESSIONS_DIR)
}

pub fn session_dir(root: &Path, session_id: &str) -> PathBuf {
    sessions_dir(root).join(session_id)
}

pub fn request_path(root: &Path, session_id: &str) -> PathBuf {
    session_dir(root, session_id).join(REQUEST_FILE)
}

pub fn result_path(root: &Path, session_id: &str) -> PathBuf {
    session_dir(root, session_id).join(RESULT_FILE)
}

// ---------------------------------------------------------------------------
// The command line
// ---------------------------------------------------------------------------

/// Arguments for one inspection.
///
/// Named flags rather than positional arguments, because the same command line
/// is echoed into the console for the user to see — and a bare pair of long
/// temp paths says nothing about which is which.
pub fn command_args(request: &Path, result: &Path) -> Vec<String> {
    vec![
        "--request".to_string(),
        request.display().to_string(),
        "--result".to_string(),
        result.display().to_string(),
    ]
}

/// Arguments for enumerating the machine's .NET processes.
///
/// A second mode of the same executable rather than a second executable: the
/// enumeration is `DiagnosticsClient.GetPublishedProcesses()`, which ships in
/// the same package ClrMD already pulls in, and a separate binary would double
/// what has to be published and kept in step. There is no request file because
/// there is nothing to ask for — the question has no parameters.
pub fn list_command_args(result: &Path) -> Vec<String> {
    vec![
        "--list-processes".to_string(),
        "--result".to_string(),
        result.display().to_string(),
    ]
}

/// Where a process listing is written.
///
/// The temporary directory rather than `.code-basics/`, because this file is
/// neither a capture the user can reopen nor state worth keeping: it is polled,
/// read once and deleted. Putting it under the session directories would have
/// each poll compete with real captures for the retention budget.
///
/// The name is unique per call so two overlapping polls — the picker refreshing
/// while a capture revalidates — cannot read each other's half-written file.
pub fn process_list_path(session_id: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cb-inspector-processes-{session_id}.json"))
}

/// The document the sidecar writes in `--list-processes` mode.
///
/// Loose in exactly the way [`super::graph::RawResult`] is loose, and for the
/// same reason: one process the enumerator could not describe must cost that
/// process, not the whole list.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProcessList {
    schema_version: u32,
    #[serde(default)]
    processes: Vec<RawProcess>,
    #[serde(default)]
    warnings: Vec<String>,
}

/// One entry as the sidecar reported it.
///
/// `pid` and `name` are required of the sidecar but optional here, so that a
/// malformed entry is skipped rather than failing the document.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProcess {
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    path: Option<PathBuf>,
    #[serde(default)]
    parent_pid: Option<u32>,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    command_line: Option<String>,
}

/// The machine's .NET processes, and anything the enumerator could not do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessList {
    pub processes: Vec<DotnetProcess>,
    pub warnings: Vec<String>,
}

/// Parse a process listing.
///
/// Entries missing a pid or a name are dropped and counted in a warning: an
/// attach target with no number to attach to is not a target, and one with no
/// name would appear in the picker as a blank row the user cannot identify.
/// A pid of zero is dropped for the same reason — it is the idle process on
/// Windows and never a managed one, so it can only be a placeholder.
pub fn parse_process_list(content: &str) -> Result<ProcessList> {
    let raw: RawProcessList = serde_json::from_str(content)
        .context("the inspector wrote a process list that could not be read")?;

    if raw.schema_version != super::model::SCHEMA_VERSION {
        anyhow::bail!(
            "the inspector wrote a version {} process list, but this build of code-basics reads \
             version {}. The bundled inspector is out of step with the application; rebuilding \
             it should resolve this.",
            raw.schema_version,
            super::model::SCHEMA_VERSION
        );
    }

    let total = raw.processes.len();
    let processes: Vec<DotnetProcess> = raw
        .processes
        .into_iter()
        .filter_map(|p| {
            let pid = p.pid.filter(|pid| *pid != 0)?;
            let name = p.name.filter(|n| !n.trim().is_empty())?;
            Some(DotnetProcess {
                pid,
                name,
                path: p.path,
                // A parent that is its own child is a reading of the world that
                // cannot be true; discarded here so no walk has to cope with it.
                parent_pid: p.parent_pid.filter(|parent| *parent != pid && *parent != 0),
                started_at: p.started_at,
                command_line: p.command_line,
            })
        })
        .collect();

    let mut warnings = raw.warnings;
    let dropped = total - processes.len();
    if dropped > 0 {
        warnings.push(format!(
            "{dropped} .NET process(es) were listed without a usable id or name and were left out"
        ));
    }

    Ok(ProcessList {
        processes,
        warnings,
    })
}

/// Read a process listing the sidecar wrote.
///
/// A missing file means the sidecar never got as far as answering, which is a
/// different problem from an empty machine — the caller decides whether that is
/// worth reporting, and for a polled picker it is not.
pub fn parse_process_list_file(path: &Path) -> Result<ProcessList> {
    if !path.exists() {
        anyhow::bail!(
            "the inspector did not produce a process list at {}",
            path.display()
        );
    }
    let content = std::fs::read_to_string(path).with_context(|| {
        format!(
            "failed to read the inspector process list {}",
            path.display()
        )
    })?;
    parse_process_list(&content)
}

/// Create a session directory and write the request into it.
///
/// Returns the request path, so the caller passes it straight to
/// [`command_args`] without rebuilding it.
pub fn write_request(root: &Path, session_id: &str, request: &InspectRequest) -> Result<PathBuf> {
    let dir = session_dir(root, session_id);
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;

    // Every feature that puts state under `.code-basics/` ignores it itself —
    // see `changelists.rs` and `intents/mod.rs`. Captures are regenerated,
    // personal, and full of one machine's absolute paths. Failing to write the
    // ignore file is not worth failing the capture over, but it is worth
    // attempting on the path that creates the directory.
    let _ = crate::config::ensure_gitignore(&crate::config::config_dir(root));

    let path = dir.join(REQUEST_FILE);
    let json = serde_json::to_string_pretty(request).context("failed to serialise the request")?;
    std::fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))?;

    Ok(path)
}

// ---------------------------------------------------------------------------
// Which build of the sidecar
// ---------------------------------------------------------------------------

/// The file name of the sidecar for a given architecture.
///
/// Two builds ship because ClrMD can only read a target of its own bitness,
/// and a 32-bit .NET process is still common enough (anything pinned to x86 by
/// a native dependency) to be worth serving.
pub fn sidecar_file_name(bitness: Bitness) -> &'static str {
    match bitness {
        Bitness::X64 => "cb-inspector-win-x64.exe",
        Bitness::X86 => "cb-inspector-win-x86.exe",
    }
}

/// Why a capture could not be taken.
///
/// A stable code rather than a parsed sentence: the retry decision must not
/// depend on wording that is written for a human and free to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCode {
    /// The target is a different architecture than this sidecar.
    BitnessMismatch,
    /// Not a .NET process, or the runtime had not started yet.
    NotManaged,
    /// The OS refused. Usually the target runs as another user.
    AccessDenied,
    /// It exited between being listed and being read.
    TargetGone,
    /// Anything the sidecar could name but this build does not know about.
    Other,
}

/// A failure the sidecar reported, with its own wording preserved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarFailure {
    pub code: FailureCode,
    pub message: String,
}

fn failure_code(raw: &str) -> FailureCode {
    match raw {
        "bitnessMismatch" => FailureCode::BitnessMismatch,
        "notManaged" => FailureCode::NotManaged,
        "accessDenied" => FailureCode::AccessDenied,
        "targetGone" => FailureCode::TargetGone,
        _ => FailureCode::Other,
    }
}

/// The failure a result carries, if it carries one.
pub fn failure_of(raw: &RawResult) -> Option<SidecarFailure> {
    let message = raw.failure.clone()?;
    Some(SidecarFailure {
        code: raw
            .failure_code
            .as_deref()
            .map(failure_code)
            .unwrap_or(FailureCode::Other),
        message,
    })
}

/// Which sidecar to try next, if any.
///
/// x64 is tried first because almost everything is, and a wrong first guess
/// costs one fast process launch. **Only** a reported bitness mismatch earns a
/// second attempt: retrying on a generic failure would double the wait for
/// every genuine problem — an access denial or a dead process fails exactly
/// the same way twice — and would bury the real message behind a second one.
pub fn next_attempt(tried: Bitness, failure: &SidecarFailure) -> Option<Bitness> {
    match (tried, failure.code) {
        (Bitness::X64, FailureCode::BitnessMismatch) => Some(Bitness::X86),
        // Already the fallback; there is nothing narrower to try.
        (Bitness::X86, _) => None,
        _ => None,
    }
}

/// Resolve the sidecar executable.
///
/// `CB_INSPECTOR_PATH` wins when set, which is what makes the feature usable
/// under `pnpm tauri dev` before any bundling exists: point it at a
/// `dotnet publish` output and everything downstream behaves identically.
pub fn resolve(bundled_dir: Option<&Path>, bitness: Bitness) -> Option<PathBuf> {
    if let Some(override_path) = std::env::var_os("CB_INSPECTOR_PATH") {
        let path = PathBuf::from(override_path);
        // A directory means "the publish output"; a file means that exact
        // binary, so a developer can point at one build and skip the naming
        // convention entirely.
        let candidate = if path.is_dir() {
            path.join(sidecar_file_name(bitness))
        } else {
            path
        };
        if candidate.exists() {
            return Some(candidate);
        }
    }

    let candidate = bundled_dir?.join(sidecar_file_name(bitness));
    candidate.exists().then_some(candidate)
}

// ---------------------------------------------------------------------------
// Retention
// ---------------------------------------------------------------------------

/// Delete all but the `keep` newest sessions, returning what was removed.
///
/// Captures accumulate silently — one per expansion, several per debugging
/// session — and each holds a JSON graph of somebody's production data. Left
/// alone this becomes both a disk problem and a privacy one.
pub fn retain_newest(dir: &Path, keep: usize) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .with_context(|| format!("failed to list {}", dir.display()))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if !entry.file_type().ok()?.is_dir() {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect();

    // Newest first, then everything past `keep` goes. The path breaks ties so
    // the order is deterministic when timestamps collide, which they do on
    // filesystems with coarse resolution.
    sessions.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let mut removed = Vec::new();
    for (_, path) in sessions.into_iter().skip(keep) {
        // A session that will not delete — held open by a viewer, say — is not
        // worth failing a capture over.
        if std::fs::remove_dir_all(&path).is_ok() {
            removed.push(path);
        }
    }

    Ok(removed)
}

#[cfg(test)]
#[path = "sidecar_tests.rs"]
mod sidecar_tests;
