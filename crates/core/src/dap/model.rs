//! The debug-session types that cross IPC, and the distinctions they exist to
//! keep apart.
//!
//! `src/ipc/types.ts` mirrors these by hand, like every other wire type here,
//! and the key-pinning tests at the bottom are what stop the two drifting. See
//! `docs/architecture/ipc-contract.md`.
//!
//! # The rule this module is built around
//!
//! **Not running, starting, running, paused, exited and "no adapter installed"
//! are six different answers and must never collapse into one.** They are six
//! because each licenses something different: only `Paused` licenses a call
//! stack, only `Running` licenses a *pause* button, `NotInstalled` is the only
//! one the user can act on and the only one that must name what was looked for,
//! and `Exited` carries a code that `NotRunning` has no business inventing.
//!
//! The same discipline as [`crate::lsp::model::Availability`], and for the same
//! reason: a UI given one boolean draws a debugger that looks broken when it is
//! merely starting, or looks ready when nothing is attached.
//!
//! There is no `skip_serializing_if` anywhere in this file. An absent key and a
//! key holding `null` are different things to a TypeScript reader, and every
//! one of these fields is read by the UI to decide what to draw.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Where a debug session is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum DebugState {
    /// Nothing has been started. The ordinary resting state.
    NotRunning,
    /// No debug adapter could be found for this ecosystem.
    ///
    /// Carries what was looked for and how to get it, because this is the one
    /// state the user can fix and a bare "unavailable" sends them to the wrong
    /// place. Never silently degraded into an ordinary run — a Debug button
    /// that quietly does not debug is worse than one that says why it cannot.
    NotInstalled {
        /// What this app searched for, in the order it searched.
        ///
        /// Renamed explicitly: `rename_all` on a tagged enum renames the
        /// *variants*, not a struct variant's fields, so without this the wire
        /// carries `looked_for` while `types.ts` reads `lookedFor` and the UI
        /// silently shows an empty list. Caught by a key-pinning test.
        #[serde(rename = "lookedFor")]
        looked_for: Vec<String>,
        /// One sentence on how to install it.
        hint: String,
    },
    /// The adapter process is up and the handshake is in progress.
    Starting,
    /// Handshaken and the debuggee is running. No stack is available.
    Running,
    /// Stopped at a breakpoint, a step, or an exception.
    ///
    /// The only state that licenses a call stack — hence the thread id, which
    /// is what every subsequent `stackTrace` needs.
    Paused {
        /// `breakpoint`, `step`, `exception`, `pause`, … the adapter's word.
        reason: String,
        /// The thread that stopped. `None` means the adapter said every thread
        /// stopped without naming one, which is a real answer: a `stackTrace`
        /// cannot be sent until a thread is chosen from `threads`.
        #[serde(rename = "threadId")]
        thread_id: Option<i64>,
        /// A sentence for the user, when the adapter offered one — an
        /// exception's message, typically.
        description: Option<String>,
    },
    /// The debuggee finished. The code is `None` when the adapter reported a
    /// termination without one, which is not the same as exiting with 0.
    Exited { code: Option<i64> },
    /// The session ended badly: the adapter died, or answered unreadably.
    Failed { detail: String },
}

impl DebugState {
    /// Whether a call stack, scopes or variables may be requested.
    ///
    /// One predicate rather than a `matches!` at each call site, because there
    /// is exactly one state that permits it and every site must agree.
    pub fn is_paused(&self) -> bool {
        matches!(self, DebugState::Paused { .. })
    }

    /// Whether the session is live enough to be stopped.
    pub fn is_live(&self) -> bool {
        matches!(
            self,
            DebugState::Starting | DebugState::Running | DebugState::Paused { .. }
        )
    }
}

/// One frame of a call stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StackFrame {
    /// The adapter's id, needed to ask for this frame's scopes.
    pub id: i64,
    pub name: String,
    /// The file, when the adapter gave a path. `None` for a frame with no
    /// source — framework code, generated code, a native frame — which is a
    /// real and common answer and must not become an empty string the UI then
    /// tries to open.
    pub path: Option<String>,
    /// 1-based, already through [`super::positions::line_from_adapter`].
    pub line: Option<u32>,
    pub column: Option<u32>,
    /// The adapter marked this frame as not the user's own code (`subtle`, or a
    /// non-normal presentation hint). Shown differently rather than hidden:
    /// hiding frames is how a stack stops explaining how it got somewhere.
    pub subtle: bool,
}

/// One named value in a scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    pub name: String,
    /// The adapter's rendering of the value. Never computed here — see below.
    pub value: String,
    /// The declared type, when the adapter reported one.
    pub type_name: Option<String>,
    /// Non-zero when this can be expanded. It is the handle a `variables`
    /// request needs, so it is kept rather than reduced to a bool.
    pub variables_reference: i64,
}

impl Variable {
    pub fn is_expandable(&self) -> bool {
        self.variables_reference != 0
    }
}

/// One of a frame's scopes — Locals, Arguments, Globals, …
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub name: String,
    pub variables_reference: i64,
    /// The adapter warned that reading this scope is slow. Passed through so
    /// the UI can leave it collapsed rather than making the user wait for a
    /// scope they did not ask about.
    pub expensive: bool,
}

/// One thread of the debuggee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: i64,
    pub name: String,
}

/// What the UI polls for: the whole of a session's visible state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DebugStatus {
    pub state: DebugState,
    /// The configuration this session is debugging, for the label.
    pub config_id: Option<String>,
    /// Empty unless paused.
    pub threads: Vec<Thread>,
    /// The stopped thread's frames. Empty unless paused.
    pub stack: Vec<StackFrame>,
}

impl DebugStatus {
    /// The resting state: nothing started, nothing to show.
    pub fn idle() -> Self {
        DebugStatus {
            state: DebugState::NotRunning,
            config_id: None,
            threads: Vec::new(),
            stack: Vec::new(),
        }
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
