//! Reading the real objects out of a real .NET process.
//!
//! A stack trace says where something broke. It never says *what was in the
//! object*, and by the time anyone looks, the process is gone. This module
//! closes that gap without a debugger: it reads the managed heap of a running
//! process, or of the crash dump the runtime wrote on its way down, and shapes
//! what it finds into the tree the UI renders.
//!
//! # Why there is a sidecar
//!
//! Walking a .NET heap means ClrMD (`Microsoft.Diagnostics.Runtime`), which is
//! a .NET library, and this application is Rust. So the walk happens in a
//! small .NET process and the answer comes back as a file.
//!
//! That is not a workaround; it is the shape this application already has.
//! [`crate::testing`] describes the pattern: *a runner streams output live and
//! writes a structured report when it finishes, and the tree is built from the
//! report afterwards.* `dotnet test` leaves a `.trx`; the inspector leaves a
//! `result.json`. Because the exchange is one file in and one file out, the
//! sidecar is a plain one-shot process — [`crate::process::Supervisor`] runs
//! it unchanged, and cancellation, process-tree kill and environment layering
//! all come free. Nothing in `process/` had to grow a second mode.
//!
//! ```text
//! .code-basics/inspect/<session>/request.json   written here
//! .code-basics/inspect/<session>/result.json    written by the sidecar
//! ```
//!
//! # What this can and cannot do
//!
//! It reads **memory**, not a running execution context. Two consequences are
//! worth stating wherever this feature is surfaced, because a user who
//! expects a debugger will otherwise be quietly misled:
//!
//! * **No method is ever called.** You see what the object holds, not what it
//!   would compute. `EstimateCost()` cannot be run.
//! * **No property is ever evaluated.** ClrMD reads *fields*, so
//!   `public int Count => _items.Length` appears as `_items`. The upside is
//!   that nothing can throw and nothing has side effects — inspecting is
//!   incapable of changing the thing being inspected.
//!
//! # The rule everything here follows
//!
//! An object graph is read through several layers of indirection, and at every
//! layer something may be unreadable — a region absent from the dump, a field
//! the JIT put in a register, a type the walker could not resolve. The
//! governing rule, inherited from [`crate::git::grouping`], is that **a wrong
//! value is much worse than no value**. A field rendered as `0` that was never
//! actually read is worse than a visible gap, because the user believes it and
//! goes and debugs the wrong thing. So every uncertainty becomes an explicit
//! [`model::ObjectValue::Unavailable`] carrying a sentence, and every cap
//! becomes an explicit [`model::ObjectValue::Elided`] — never a shorter list
//! that looks complete.

pub mod dumps;
pub mod graph;
pub mod model;
pub mod session;
pub mod sidecar;
pub mod tree;

use std::path::Path;

use anyhow::{Context, Result};

pub use model::*;

/// Read a capture the sidecar wrote.
///
/// A missing file is reported as its own error, like
/// [`crate::testing::parse_file`]: it means the sidecar never got as far as
/// answering, which is a different problem from an answer that says it could
/// not read the target.
pub fn parse_result_file(session_id: &str, path: &Path) -> Result<InspectGraph> {
    if !path.exists() {
        anyhow::bail!(
            "the inspector did not produce a result at {}. It may have been unable to \
             attach to the target, or the target may have exited before it was read.",
            path.display()
        );
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read the inspector result {}", path.display()))?;

    parse_result(session_id, &content)
}

/// Parse and shape a result document.
///
/// Split from [`parse_result_file`] so the whole pipeline — validation,
/// classification and shaping — is exercised from a string in tests, with no
/// filesystem and no .NET involved.
pub fn parse_result(session_id: &str, content: &str) -> Result<InspectGraph> {
    let raw = graph::parse(content)?;

    // The sidecar ran and explained why it could not help. Its own wording is
    // more specific than anything that could be reconstructed out here.
    if let Some(failure) = raw.failure {
        anyhow::bail!("{failure}");
    }

    let built = tree::build(&raw.nodes);

    // Warnings from the sidecar and from shaping are the same kind of thing to
    // the reader — something was not perfect and here is what — so they are
    // presented as one list.
    let mut warnings = raw.warnings;
    warnings.extend(built.warnings);

    Ok(InspectGraph {
        session_id: session_id.to_string(),
        snapshot_id: raw.snapshot_id,
        captured_at: raw.captured_at,
        target: raw.target,
        roots: built.roots,
        caps: raw.caps,
        warnings,
    })
}

#[cfg(test)]
#[path = "inspect_tests.rs"]
mod inspect_tests;
