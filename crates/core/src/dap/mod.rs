//! Debugging: breakpoints, stepping and a call stack, over the Debug Adapter
//! Protocol.
//!
//! This sits beside [`crate::lsp`] and beside [`crate::inspect`], and the three
//! are deliberately not versions of each other:
//!
//! * [`crate::inspect`] reads a **dead or unsuspended** heap through ClrMD. It
//!   calls no method and evaluates no property, and the process it reads keeps
//!   running. It is a photograph.
//! * `dap` **stops** the process and asks the runtime's own debugger about it.
//!   It is the thing the Objects tab's documentation keeps saying it is not.
//! * [`crate::lsp`] answers questions about code that is not running at all.
//!
//! # Layering
//!
//! The same shape as `lsp/`, for the same reason — every decision that could be
//! wrong invisibly is in a pure module that can be tested with no debugger
//! installed:
//!
//! | Module | What it decides | Touches a process |
//! |---|---|---|
//! | [`protocol`] | The wire messages, and the events whose bodies are read | no |
//! | [`sequence`] | `seq` vs `request_seq`, and what must be answered | no |
//! | [`positions`] | The line/column convention, and what it refuses to guess | no |
//! | [`breakpoints`] | Requested versus bound, and where it really landed | no |
//! | [`registry`] | Which adapter to run, or exactly what was looked for | no |
//! | [`model`] | The six states that must never collapse into one | no |
//!
//! **Framing is not in this list because it is not written twice.** DAP frames
//! messages exactly as LSP does, so [`crate::lsp::framing`] is used unchanged.
//!
//! # The governing rule
//!
//! Sharper here than anywhere else in this crate, because a debugger that is
//! subtly wrong is worse than no debugger at all — the user acts on what it
//! shows them:
//!
//! * **Not running, starting, running, paused, exited, and "no adapter
//!   installed" are six answers, not one.** [`model::DebugState`] has six
//!   variants for exactly that.
//! * **A breakpoint the user set and a breakpoint the debugger bound are
//!   different facts.** [`breakpoints::BindState`] keeps them apart, and keeps
//!   *pending* apart from *rejected* on top of that.
//! * **A missing adapter is reported with what was looked for**, never degraded
//!   into starting the process without a debugger.
//! * **Nothing here corrects an adapter's line numbers.** Where it says it put a
//!   breakpoint is shown, not silently replaced with where it was asked to —
//!   see [`positions`].

pub mod breakpoints;
pub mod model;
pub mod positions;
pub mod protocol;
pub mod registry;
pub mod sequence;
