//! The one platform seam of the running registry: ask the OS whether a pid is
//! alive and, if so, its identity (image name + start time). Wraps `sysinfo` so
//! the decision logic in `classify.rs` can stay pure and testable. Thin on
//! purpose, like `process/kill.rs` — it makes exactly one query and shapes the
//! answer into a [`ProcInfo`].

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use super::classify::ProcInfo;

/// The OS's current view of `pid`, or `None` if no process by that id exists.
///
/// Refreshes only the one pid, so this is cheap enough to call per-orphan at
/// startup and once more immediately before a kill (the PID-reuse re-check).
pub fn probe(pid: u32) -> Option<ProcInfo> {
    let mut sys = System::new();
    let target = Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[target]),
        true,
        ProcessRefreshKind::nothing(),
    );
    let process = sys.process(target)?;
    Some(ProcInfo {
        name: process.name().to_string_lossy().into_owned(),
        // sysinfo reports start time in whole seconds since the Unix epoch;
        // classify tolerates the resulting rounding.
        started_at_ms: process.start_time().saturating_mul(1000),
    })
}
