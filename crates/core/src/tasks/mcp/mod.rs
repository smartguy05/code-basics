//! The Tasks MCP server: what a coding agent may do to this workspace's task
//! list, and what it is told back.
//!
//! The per-workspace task store ([`crate::tasks`]) is already reachable by the
//! human at the floating Tasks panel. This module is the other consumer: a
//! **separate process** (`cb-app mcp-tasks`, self-dispatched exactly as
//! `record-intent`, `quality-gate`, `mcp-sql` and `mcp-browser` are — see
//! [`argv`]) with no `AppState` and no window, so everything it decides has to
//! live in `cb-core` where both it and the panel can reach it.
//!
//! # Not a read-only server, on purpose
//!
//! Unlike [`crate::mcp`] (the SQL server), which forces read-only and has no
//! argument that lifts it, this server is **read *and* write**: creating,
//! updating, completing, assigning and deleting tasks is the whole point of it.
//! Managing a task list is a low-stakes operation over a personal, gitignored
//! file, and the value of letting an agent keep its own to-do list in step with
//! the user's is exactly the writes. There is therefore no read-only
//! enforcement layer here and no `execute::agent_plan` gate — [`execute::apply`]
//! applies the call to the store directly.
//!
//! # The scope *is* the boundary
//!
//! `--workspace <root>` is not a mere listing filter here (as it is for the SQL
//! server): the store is per-workspace, so the flag names the one `tasks.json`
//! this server reads and writes. An unscoped invocation has no store at all and
//! every tool answers [`answer::McpRefusal::NoWorkspace`] rather than guessing a
//! directory. The install step bakes the workspace in, which is the consent
//! boundary — an agent configured for one repository cannot reach another's
//! tasks.
//!
//! # The abstain-rather-than-guess discipline is kept
//!
//! Every answer that is not data is its own variant ([`answer`]), a missing task
//! is *not found* rather than a silent no-op, and no internal error text (an fs
//! path, a serde message) is ever forwarded — a write that could not be
//! persisted crosses as [`answer::McpRefusal::StoreWriteFailed`] with the words
//! dropped, exactly as the SQL server drops driver text.
//!
//! Layout:
//!
//! - [`argv`] — is this process the server or the application.
//! - [`answer`] — every refusal, kept apart.
//! - [`execute`] — applying one tool call to the store.
//! - [`render`] — turning an outcome into the words an agent reads.
//! - [`tools`] — the seven tools, their schemas, and how a call is read.
//! - [`serve`] — the handshake, the routing, and the envelopes.
//! - [`install`] — writing this server into an agent's configuration.
//!
//! The line framing ([`crate::mcp::ndjson`]) and the config writers
//! ([`crate::mcp::install`]) are **reused directly** rather than copied — they
//! are generic to any stdio MCP server this binary hosts. [`install`] is this
//! server's own thin layer over those writers: its entry, its `--workspace`
//! boundary and its write-capable caveats differ, but the merge does not.

pub mod answer;
pub mod argv;
pub mod execute;
pub mod install;
pub mod render;
pub mod serve;
pub mod tools;
