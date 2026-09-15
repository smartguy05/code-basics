//! The one platform seam of the editor instance registry: is this entry's
//! process still the application that wrote it?
//!
//! The **twin** of [`crate::roslyn::liveness`], and thin for the same reason — it
//! makes one query and shapes the answer — so that
//! [`super::instances::choose_instance`] can stay pure and take liveness as a
//! parameter. The probe and the identity comparison themselves live once in
//! [`crate::mcp::liveness`], shared with every MCP instance registry this binary
//! hosts.
//!
//! # A pid is not identity
//!
//! Windows reuses process ids, and the registry entry outlives the process that
//! wrote it (a crash leaves it behind). So a live pid is checked against the
//! executable path the entry recorded — the reasoning [`crate::inspect::session`]
//! uses before attaching a debugger to somebody's process.
//!
//! # An unidentifiable process is *not alive*
//!
//! [`alive`] answers `false` when the process exists but its executable path
//! cannot be read. That is the abstention, and it is the safe direction: the cost
//! of a false negative is [`super::instances::InstanceError::NoneRunning`] and the
//! user starting the application again; the cost of a false positive is asking a
//! process nobody identified for the user's editor state.

use super::instances::EditorInstance;

// The probe and the identity comparison live once in [`crate::mcp::liveness`],
// shared with the other registries; this re-export keeps `same_executable`
// reachable at the old path (tests).
pub use crate::mcp::liveness::same_executable;

/// Is `instance` still running, and still the executable it says it is?
pub fn alive(instance: &EditorInstance) -> bool {
    crate::mcp::liveness::alive_by(instance.pid, &instance.exe)
}

#[cfg(test)]
#[path = "liveness_tests.rs"]
mod tests;
