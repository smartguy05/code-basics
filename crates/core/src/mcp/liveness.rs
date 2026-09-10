//! Is a registry entry's process still the executable it recorded?
//!
//! Shared by every MCP instance registry this binary hosts — the browser server
//! ([`crate::browser::liveness`]) and the Roslyn server
//! ([`crate::roslyn::liveness`]) — which held byte-for-byte copies of this before.
//! It is the one platform seam of those registries: it makes one query and shapes
//! the answer, so each [`super::super`] `choose_instance` can stay pure and take
//! liveness as a parameter.
//!
//! # A pid is not identity
//!
//! Windows reuses process ids, and a registry entry outlives the process that
//! wrote it (a crash leaves it behind). So a live pid is only accepted when the
//! running process's executable path matches the one the entry recorded — the
//! reasoning [`crate::inspect::session`] uses before attaching a debugger to
//! somebody's process. Without it, an agent could be handed a pipe name belonging
//! to a stranger's process that happened to inherit the number.
//!
//! # An unidentifiable process is *not alive*
//!
//! [`alive_by`] answers `false` when the process exists but its executable path
//! cannot be read. That is the abstention, and it is the safe direction: the cost
//! of a false negative is the caller reporting nothing is running and the user
//! starting the application again; the cost of a false positive is asking a
//! process nobody identified to act.

use std::path::Path;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Is the process at `pid` running, and still the executable at `exe`?
pub fn alive_by(pid: u32, exe: &str) -> bool {
    let mut system = System::new();
    let target = Pid::from_u32(pid);
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[target]),
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );
    let Some(process) = system.process(target) else {
        return false;
    };
    match process.exe() {
        Some(path) => same_executable(path, exe),
        // Refused rather than accepted — see the module docs.
        None => false,
    }
}

/// Whether a probed path and a recorded one name the same executable.
///
/// Compared case-insensitively, because Windows paths are, and a registry entry
/// written as `C:\Apps\cb-app.exe` must still match a probe reporting
/// `c:\apps\cb-app.exe`. Separators are normalised for the same reason.
///
/// Deliberately **not** canonicalised: canonicalising touches the filesystem,
/// this runs on every candidate on every tool call, and a path that has become
/// unreadable would then answer *not the same executable* — a wrong answer rather
/// than a cautious one, because the process is demonstrably running from it.
pub fn same_executable(probed: &Path, recorded: &str) -> bool {
    fn normalise(value: &str) -> String {
        value.replace('/', "\\").to_lowercase()
    }
    let probed = probed.to_string_lossy();
    !recorded.trim().is_empty() && normalise(&probed) == normalise(recorded)
}

#[cfg(test)]
#[path = "liveness_tests.rs"]
mod tests;
