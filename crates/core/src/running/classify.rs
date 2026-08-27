//! The pure decision behind orphan detection: given records reloaded from disk
//! and the OS's current view of each pid, decide which are genuine orphans and
//! which must be dropped. No I/O — the sysinfo probe lives in `probe.rs` and its
//! results are passed in, so every branch is testable without a live process.
//!
//! Governing rule (the crate's abstain rule, sharpened for how dangerous a wrong
//! kill is): a pid is an orphan **only** when the live process's identity matches
//! what was recorded. A pid that is gone is dropped silently; a pid now owned by a
//! *different* process (reuse) is dropped with a warning and never surfaced for a
//! kill, because killing it would kill a stranger's process.

use super::record::RunningRecord;

/// The OS's view of one pid: its image name and start time (ms since the Unix
/// epoch). Produced by `probe.rs`; `None` at the call site means no such pid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub name: String,
    pub started_at_ms: u64,
}

/// The outcome of classifying reloaded records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrphanClassification {
    /// Records whose pid is still alive and whose identity matches — safe to show
    /// and to offer a kill for.
    pub orphans: Vec<RunningRecord>,
    /// Human notes about records that were dropped for a reason worth surfacing
    /// (a pid reused by a different process). Dead pids are dropped silently.
    pub warnings: Vec<String>,
}

/// How far apart two start times may be and still be the same process. sysinfo
/// reports start time in whole seconds, and our own record is stamped a moment
/// after spawn, so a small window absorbs the rounding and the spawn latency.
const START_TOLERANCE_MS: u64 = 2_000;

/// Classify each `(record, probe result)` pair into orphans + warnings.
pub fn classify_orphans(probed: Vec<(RunningRecord, Option<ProcInfo>)>) -> OrphanClassification {
    let mut out = OrphanClassification::default();
    for (record, info) in probed {
        match info {
            // The pid is gone: the process exited (cleanly or otherwise). Nothing
            // to warn about — this is the common, expected case.
            None => {}
            Some(info) if identity_matches(&record, &info) => out.orphans.push(record),
            Some(info) => out.warnings.push(format!(
                "pid {} is now `{}`, not `{}` ({}) — the original process is gone; leaving the current one alone",
                record.pid, info.name, record.program, record.label
            )),
        }
    }
    out
}

/// Whether a live process is the same one a record described: same image name and
/// a start time within tolerance. Both halves must agree — a reused pid almost
/// always differs in start time, and the name guards the rare same-second reuse.
pub fn identity_matches(record: &RunningRecord, info: &ProcInfo) -> bool {
    names_match(&record.program, &info.name)
        && record.started_at_ms.abs_diff(info.started_at_ms) <= START_TOLERANCE_MS
}

/// Case-insensitive comparison of image names, tolerant of a `.exe` suffix on
/// either side (recorded and probed names can come from slightly different
/// sources).
fn names_match(a: &str, b: &str) -> bool {
    strip_exe(a).eq_ignore_ascii_case(strip_exe(b))
}

fn strip_exe(name: &str) -> &str {
    name.strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".EXE"))
        .unwrap_or(name)
}

#[cfg(test)]
#[path = "classify_tests.rs"]
mod tests;
