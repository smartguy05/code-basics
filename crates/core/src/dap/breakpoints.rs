//! The breakpoint model: what the user asked for, what the adapter agreed to,
//! and the difference between the two.
//!
//! # The distinction the whole module exists for
//!
//! A breakpoint the user set and a breakpoint the debugger has **bound** are not
//! the same thing, and collapsing them is the single most damaging mistake this
//! feature can make. An unbound breakpoint renders exactly like a bound one, the
//! user runs, execution sails past it, and the only available conclusion is that
//! the debugger is broken. The real cause is usually mundane — the assembly was
//! not built with symbols, the file is not in the debuggee, the line is a
//! comment — and every one of those is something the adapter *told us* and the
//! UI threw away.
//!
//! So [`Breakpoint`] keeps three separate facts:
//!
//! * `line` — where the user clicked. Never overwritten.
//! * `verified` — whether the adapter bound it. `false` until it says so, which
//!   means a breakpoint set before the adapter has loaded the module is
//!   *pending*, not *broken*, and becomes verified when a `breakpoint` event
//!   arrives.
//! * `actual_line` — where the adapter actually put it, when that differs.
//!   Moving a breakpoint to the next executable line is legitimate and common;
//!   silently showing it on the requested line makes the debugger look like it
//!   stopped somewhere it was not asked to.
//!
//! # Identity
//!
//! Breakpoints are keyed by `(path, line)` on this side and by the adapter's own
//! `id` on the other. Both are kept. The adapter's id is the only way to apply a
//! later `breakpoint` event, and the `(path, line)` pair is the only thing that
//! survives the adapter being restarted — a debug session ends, the ids all go,
//! and the user's breakpoints are still on screen.

use std::collections::BTreeMap;

use super::positions::{column_from_adapter, line_from_adapter};

/// Why a breakpoint is not bound, when the adapter said.
///
/// Distinct from `Option<String>` on purpose: [`BindState::Pending`] and
/// [`BindState::Rejected`] are different answers and the UI must not draw them
/// the same way. Pending is ordinary and resolves itself; rejected will not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindState {
    /// The adapter bound it. Execution will stop here.
    Verified,
    /// Sent, and the adapter has not bound it yet. Normal before the module
    /// containing the line is loaded.
    Pending,
    /// The adapter answered and said no, with its reason when it gave one.
    Rejected { reason: Option<String> },
}

impl BindState {
    /// Read a `setBreakpoints` response entry, or a `breakpoint` event body.
    ///
    /// `verified: false` with a message is a refusal; without one it is still
    /// pending. That distinction is the adapter's, not a guess: an adapter that
    /// has merely not got there yet has nothing to say about why.
    pub fn from_body(body: &serde_json::Value) -> Self {
        if body
            .get("verified")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return BindState::Verified;
        }
        let reason = body
            .get("message")
            .and_then(serde_json::Value::as_str)
            .filter(|m| !m.trim().is_empty())
            .map(str::to_string);
        match reason {
            Some(reason) => BindState::Rejected {
                reason: Some(reason),
            },
            None => BindState::Pending,
        }
    }

    pub fn is_verified(&self) -> bool {
        matches!(self, BindState::Verified)
    }
}

/// One breakpoint, as this app knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakpoint {
    /// Workspace-relative path, spelled the way the file tree spells it.
    pub path: String,
    /// The line the user clicked. 1-based, and never rewritten by the adapter.
    pub line: u32,
    /// The adapter's id for it, once it has answered. `None` before that, and
    /// after a session ends.
    pub id: Option<i64>,
    pub state: BindState,
    /// Where the adapter really put it, when that is not `line`.
    pub actual_line: Option<u32>,
    /// The column the adapter reported, when it reported one.
    pub actual_column: Option<u32>,
}

impl Breakpoint {
    /// A breakpoint the user has just set, before any adapter has seen it.
    ///
    /// Starts [`BindState::Pending`] rather than verified. With no session
    /// running that is exactly right — nothing has agreed to anything — and it
    /// keeps "the user set this" and "the debugger will stop here" from being
    /// the same field from the very first moment.
    pub fn new(path: impl Into<String>, line: u32) -> Self {
        Breakpoint {
            path: path.into(),
            line: line.max(1),
            id: None,
            state: BindState::Pending,
            actual_line: None,
            actual_column: None,
        }
    }

    /// Apply what the adapter said about this breakpoint.
    ///
    /// `actual_line` is recorded only when it *differs* from the requested line,
    /// so "the adapter agreed with me" and "the adapter moved it one line" stay
    /// distinguishable — storing it unconditionally would make every breakpoint
    /// look adjusted.
    pub fn apply(&mut self, body: &serde_json::Value) {
        self.state = BindState::from_body(body);
        if let Some(id) = body.get("id").and_then(serde_json::Value::as_i64) {
            self.id = Some(id);
        }
        let reported = body
            .get("line")
            .and_then(serde_json::Value::as_i64)
            .map(line_from_adapter);
        self.actual_line = reported.filter(|line| *line != self.line);
        self.actual_column =
            column_from_adapter(body.get("column").and_then(serde_json::Value::as_i64));
    }

    /// The line execution will really stop on: the adapter's, when it moved it.
    pub fn effective_line(&self) -> u32 {
        self.actual_line.unwrap_or(self.line)
    }

    /// Forget everything only a live adapter could know.
    ///
    /// Called when a session ends. The user's breakpoints stay — they are the
    /// user's, not the session's — but the id, the binding and the adjusted line
    /// all belonged to a process that has gone, and keeping them would show the
    /// next session's gutter as verified before anything had verified it.
    pub fn detach(&mut self) {
        self.id = None;
        self.state = BindState::Pending;
        self.actual_line = None;
        self.actual_column = None;
    }
}

/// Every breakpoint in the workspace, grouped by file.
///
/// A `BTreeMap` keyed by path, holding lines in order, because `setBreakpoints`
/// is sent **per source file and replaces that file's whole set** — so "the
/// breakpoints in this file" is the unit every operation needs, and a flat list
/// would have to be regrouped on every call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BreakpointSet {
    by_path: BTreeMap<String, Vec<Breakpoint>>,
}

impl BreakpointSet {
    /// Toggle a line, returning whether it is now set.
    pub fn toggle(&mut self, path: &str, line: u32) -> bool {
        let line = line.max(1);
        let entries = self.by_path.entry(path.to_string()).or_default();
        match entries.iter().position(|b| b.line == line) {
            Some(index) => {
                entries.remove(index);
                if entries.is_empty() {
                    self.by_path.remove(path);
                }
                false
            }
            None => {
                entries.push(Breakpoint::new(path, line));
                entries.sort_by_key(|b| b.line);
                true
            }
        }
    }

    /// The breakpoints in one file, in line order. Empty for a file with none.
    pub fn in_file(&self, path: &str) -> &[Breakpoint] {
        self.by_path.get(path).map_or(&[], Vec::as_slice)
    }

    /// The lines to send for one file, in the order `setBreakpoints` will list
    /// them — so the response's array can be zipped straight back onto them.
    pub fn lines_in_file(&self, path: &str) -> Vec<u32> {
        self.in_file(path).iter().map(|b| b.line).collect()
    }

    /// Every file holding at least one breakpoint.
    pub fn files(&self) -> Vec<String> {
        self.by_path.keys().cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }

    pub fn total(&self) -> usize {
        self.by_path.values().map(Vec::len).sum()
    }

    /// Apply a `setBreakpoints` response's `breakpoints` array to one file.
    ///
    /// The specification says the array comes back **in the order it was sent**
    /// and with the same length, so it is applied by position. A length mismatch
    /// is an adapter not keeping that promise: the shared prefix is applied and
    /// the count is returned, so the caller can say what happened rather than
    /// applying entries to the wrong lines. Zipping past a mismatch is precisely
    /// how a breakpoint ends up wearing another breakpoint's binding.
    pub fn apply_response(&mut self, path: &str, reported: &[serde_json::Value]) -> usize {
        let Some(entries) = self.by_path.get_mut(path) else {
            return 0;
        };
        let mut applied = 0;
        for (breakpoint, body) in entries.iter_mut().zip(reported) {
            breakpoint.apply(body);
            applied += 1;
        }
        applied
    }

    /// Apply a `breakpoint` event, which names its target by the adapter's id.
    ///
    /// Returns whether anything matched. `false` is worth acting on: it means
    /// the adapter is talking about a breakpoint this side does not have, which
    /// happens when one is removed while the event is in flight.
    pub fn apply_event(&mut self, body: &serde_json::Value) -> bool {
        let Some(id) = body.get("id").and_then(serde_json::Value::as_i64) else {
            return false;
        };
        for entries in self.by_path.values_mut() {
            if let Some(breakpoint) = entries.iter_mut().find(|b| b.id == Some(id)) {
                breakpoint.apply(body);
                return true;
            }
        }
        false
    }

    /// Drop every adapter-supplied fact, keeping the user's own breakpoints.
    /// Called when a session ends — see [`Breakpoint::detach`].
    pub fn detach_all(&mut self) {
        for entries in self.by_path.values_mut() {
            for breakpoint in entries.iter_mut() {
                breakpoint.detach();
            }
        }
    }
}

#[cfg(test)]
#[path = "breakpoints_tests.rs"]
mod tests;
