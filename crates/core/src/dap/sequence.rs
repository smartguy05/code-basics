//! Sequence numbers and the request/response correlation built on them.
//!
//! DAP's envelope is not JSON-RPC, and the difference is exactly the kind that
//! produces a bug nobody can see: a response points at the request it answers
//! with **`request_seq`**, while carrying a **`seq`** of its own. Matching on
//! `seq` — the field JSON-RPC would have used — pairs every response with the
//! wrong request, and the symptom is a debugger that returns the previous
//! command's answer. So the two are never confused here: `seq` is minted by
//! [`Sequencer::next_seq`] and `request_seq` is only ever read.
//!
//! # What this refuses to guess
//!
//! * A response to a request nobody sent is **reported**, not dropped. It means
//!   the adapter and the client disagree about what has been asked, and the
//!   quiet version of that is a session that hangs later for no visible reason.
//! * A **duplicate** response is reported for the same reason: the first one has
//!   already been handed to a waiter, so the second cannot be delivered and
//!   pretending otherwise loses it.
//! * Nothing here times out. A timeout is a policy the session layer owns,
//!   because "the adapter is slow" and "the adapter is gone" are different
//!   answers and only the layer holding the process can tell them apart.

use std::collections::HashMap;

use thiserror::Error;

use super::protocol::{Message, Request, Response};

/// Mints the outgoing `seq` numbers.
///
/// The specification requires them to start at 1 and increase by one per
/// message *sent by that end*, counting every message rather than every
/// request — so this is bumped for events and responses too, not just requests.
#[derive(Debug)]
pub struct Sequencer {
    next: i64,
}

impl Default for Sequencer {
    fn default() -> Self {
        Sequencer { next: 1 }
    }
}

impl Sequencer {
    /// The next sequence number, consuming it.
    ///
    /// Named `next_seq` rather than `next` so it cannot be mistaken for
    /// `Iterator::next` at a call site — this hands out protocol numbers, not
    /// elements, and the two have very different consequences when confused.
    pub fn next_seq(&mut self) -> i64 {
        let seq = self.next;
        self.next += 1;
        seq
    }

    /// What the next call would return, without consuming it.
    pub fn peek(&self) -> i64 {
        self.next
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CorrelationError {
    #[error("the adapter answered request {request_seq}, which was never sent")]
    Unknown { request_seq: i64 },
    #[error("the adapter answered request {request_seq} twice")]
    Duplicate { request_seq: i64 },
}

/// What one outstanding request was, so a response can be reported usefully.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub seq: i64,
    pub command: String,
}

/// Tracks which requests are outstanding and pairs responses back to them.
#[derive(Debug, Default)]
pub struct Correlator {
    sequencer: Sequencer,
    pending: HashMap<i64, String>,
}

impl Correlator {
    /// Build a request, recording it as outstanding.
    pub fn request(&mut self, command: &str, arguments: Option<serde_json::Value>) -> Request {
        let seq = self.sequencer.next_seq();
        self.pending.insert(seq, command.to_string());
        Request {
            seq,
            command: command.to_string(),
            arguments,
        }
    }

    /// Mint a sequence number for a message that is not a request — an event, or
    /// a response to one of the adapter's own requests.
    ///
    /// Separate from [`Correlator::request`] because nothing is expected back:
    /// recording it as pending would leave an entry no response ever clears, and
    /// [`Correlator::outstanding`] would report a session as busy forever.
    pub fn next_seq(&mut self) -> i64 {
        self.sequencer.next_seq()
    }

    /// Match a response to the request it answers, and stop tracking it.
    pub fn resolve(&mut self, response: &Response) -> Result<Pending, CorrelationError> {
        let command = self.pending.remove(&response.request_seq).ok_or({
            // Two causes, one symptom, and they are worth distinguishing in the
            // message: either the request was never sent, or this is the second
            // response to it. Only the second is recoverable by ignoring it, so
            // the caller is told which.
            if response.request_seq < self.sequencer.peek() {
                CorrelationError::Duplicate {
                    request_seq: response.request_seq,
                }
            } else {
                CorrelationError::Unknown {
                    request_seq: response.request_seq,
                }
            }
        })?;

        Ok(Pending {
            seq: response.request_seq,
            command,
        })
    }

    /// How many requests are still waiting for an answer.
    pub fn outstanding(&self) -> usize {
        self.pending.len()
    }

    /// The commands still waiting, for a shutdown that wants to say what it
    /// abandoned rather than going quiet.
    pub fn outstanding_commands(&self) -> Vec<String> {
        let mut commands: Vec<String> = self.pending.values().cloned().collect();
        commands.sort();
        commands
    }

    /// Forget every outstanding request, returning what was abandoned.
    ///
    /// For a teardown: the process is going away, so no response is coming, and
    /// leaving the entries would make a reused correlator report work that can
    /// never finish.
    pub fn abandon_all(&mut self) -> Vec<String> {
        let commands = self.outstanding_commands();
        self.pending.clear();
        commands
    }
}

/// Whether a message obliges *this* end to send something back.
///
/// The one question a reader has to get right. An adapter that sends
/// `runInTerminal` blocks until it is answered, and a reader that classified it
/// as "nothing for me to do" would hang the session with no error anywhere —
/// the same failure [`crate::lsp::jsonrpc`] documents for a server request with
/// a null id. Stated as a function so the rule is in one place and testable
/// without a process.
pub fn needs_reply(message: &Message) -> bool {
    matches!(message, Message::Request(_))
}

#[cfg(test)]
#[path = "sequence_tests.rs"]
mod tests;
