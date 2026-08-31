//! The registry of in-flight statements, and the handles that stop them.
//!
//! A clone-cheap handle over a shared map, exactly the shape
//! [`crate::pty::PtyManager`] has: the Tauri layer holds one in managed state
//! and reaches any running query by id. The map is keyed twice — connection id,
//! then query id — because a connection is what gets closed and a query is what
//! gets stopped, and the console needs both verbs.
//!
//! **This module decides nothing.** It opens no handle, runs no statement and
//! classifies no SQL; it stores [`StopHandle`]s and hands out [`StopSignal`]s.
//! Every outcome it reports is a fact about its own bookkeeping.
//!
//! # Stopping is not cancelling, and the names say so
//!
//! Signalling a stop makes the driver's row loop stop reading and drop the
//! connection. It does **not** reach the server, and on an engine that has no
//! server-side cancel it never will. Nothing here is called `cancel` or `abort`
//! for that reason, and [`StopOutcome`] keeps *signalled*, *already stopping*
//! and *no such query* apart: a stop aimed at a query that has already finished
//! is a different fact from one that landed, and reporting both as success
//! would make a race look like a working feature.
//!
//! # Removal is the runner's job
//!
//! [`SqlSessions::stop`] does not remove the entry. The task running the
//! statement removes it with [`SqlSessions::finish`] when it unwinds, so
//! `stop`-then-`finish` is the ordinary path and a `stop` on an id that is gone
//! honestly reports [`StopOutcome::NotFound`] rather than pretending to have
//! stopped something.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::driver::{stop_channel, StopHandle, StopSignal};

/// What a stop request did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOutcome {
    /// The signal was delivered to a running statement.
    Signalled,
    /// It was already signalled; this request changed nothing. Not an error,
    /// and not a second stop.
    AlreadyStopping,
    /// No statement is registered under that id — it finished, or it never
    /// started. Deliberately distinct from [`StopOutcome::Signalled`].
    NotFound,
}

/// Why a registration was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterError {
    /// That (connection, query) pair is already registered. Replacing it would
    /// drop the running statement's only stop handle on the floor, leaving a
    /// query that can never be stopped — so the caller is told instead.
    Duplicate,
}

/// One running statement's bookkeeping.
struct Running {
    stop: StopHandle,
}

/// Every in-flight statement, keyed by connection id and query id.
#[derive(Clone, Default)]
pub struct SqlSessions {
    queries: Arc<Mutex<HashMap<String, HashMap<String, Running>>>>,
}

impl SqlSessions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a statement about to run and hand back the signal its row loop
    /// watches.
    pub fn register(&self, connection: &str, query: &str) -> Result<StopSignal, RegisterError> {
        let mut map = self.lock();
        let per_connection = map.entry(connection.to_string()).or_default();
        if per_connection.contains_key(query) {
            return Err(RegisterError::Duplicate);
        }
        let (tx, rx) = stop_channel();
        per_connection.insert(query.to_string(), Running { stop: tx });
        Ok(rx)
    }

    /// Remove a statement that has ended, however it ended. Returns whether it
    /// was still registered.
    pub fn finish(&self, connection: &str, query: &str) -> bool {
        let mut map = self.lock();
        let Some(per_connection) = map.get_mut(connection) else {
            return false;
        };
        let removed = per_connection.remove(query).is_some();
        if per_connection.is_empty() {
            map.remove(connection);
        }
        removed
    }

    /// Ask one statement to stop reading. See the module docs: this is not a
    /// server-side cancel.
    pub fn stop(&self, connection: &str, query: &str) -> StopOutcome {
        let map = self.lock();
        let Some(running) = map.get(connection).and_then(|c| c.get(query)) else {
            return StopOutcome::NotFound;
        };
        signal(running)
    }

    /// Ask every statement on one connection to stop — what closing a
    /// connection does. Returns the outcome per query id, so a caller can tell
    /// how many were actually running.
    pub fn stop_connection(&self, connection: &str) -> Vec<(String, StopOutcome)> {
        let map = self.lock();
        let Some(per_connection) = map.get(connection) else {
            return Vec::new();
        };
        let mut out: Vec<(String, StopOutcome)> = per_connection
            .iter()
            .map(|(id, running)| (id.clone(), signal(running)))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// The query ids registered on one connection, sorted. Empty for a
    /// connection with nothing running — which is the same answer as for a
    /// connection that does not exist, because this registry only ever knew
    /// about *running* statements and has no opinion on open connections.
    pub fn queries(&self, connection: &str) -> Vec<String> {
        let map = self.lock();
        let mut ids: Vec<String> = map
            .get(connection)
            .map(|c| c.keys().cloned().collect())
            .unwrap_or_default();
        ids.sort();
        ids
    }

    /// Every connection id with at least one statement running, sorted.
    pub fn connections(&self) -> Vec<String> {
        let map = self.lock();
        let mut ids: Vec<String> = map.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Whether that statement is registered.
    pub fn is_running(&self, connection: &str, query: &str) -> bool {
        self.lock()
            .get(connection)
            .is_some_and(|c| c.contains_key(query))
    }

    /// How many statements are registered across every connection.
    pub fn len(&self) -> usize {
        self.lock().values().map(HashMap::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, HashMap<String, Running>>> {
        // A poisoned lock here means a previous holder panicked while holding
        // it; the map is a plain registry with no invariant to repair, so the
        // data is still usable and losing every running query would be worse.
        self.queries.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn signal(running: &Running) -> StopOutcome {
    if *running.stop.borrow() {
        return StopOutcome::AlreadyStopping;
    }
    // A send failing means every receiver is gone — the row loop has already
    // finished and dropped its signal — so nothing was stopped.
    match running.stop.send(true) {
        Ok(()) => StopOutcome::Signalled,
        Err(_) => StopOutcome::NotFound,
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
