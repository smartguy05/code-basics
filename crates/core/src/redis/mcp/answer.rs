//! Every non-data answer the Redis MCP server can give, each distinct.
//!
//! The same abstain-rather-than-guess rule as [`crate::mcp::answer`], applied to
//! a consumer that cannot ask a follow-up: the distinctions a human would have
//! recovered from unaided are the ones kept apart — *nothing saved* vs *nothing
//! exposed*, *not found* vs *not exposed*, an ambiguous selector **refused not
//! resolved**, and — unique to a write-capable server — *not exposed* (a read
//! gate) vs *writes not allowed* (a write gate). No driver text ever crosses: a
//! failure is a [`RedisStatusKind`], a variant with no message.

use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::json;

use crate::redis::model::RedisStatusKind;

/// Why a Redis tool call produced no data. Every variant renders as a **tool
/// execution error**, never a protocol error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpRefusal {
    /// No saved connections at all.
    NoConnectionsSaved,
    /// Some are saved, none exposed. A count, not a list — an unexposed name is
    /// not this interface's to reveal.
    NoConnectionExposed { total_saved: usize },
    /// Nothing saved matches the selector.
    ConnectionNotFound { selector: String },
    /// One matches and it is not exposed. Distinct from not-found: the fix is a
    /// toggle.
    ConnectionNotExposed { selector: String },
    /// Several exposed connections match — refused, never resolved.
    ConnectionAmbiguous {
        selector: String,
        candidates: Vec<String>,
    },
    /// The connection string could not be re-read from where the profile says it
    /// lives.
    SecretUnresolved { connection: String },
    /// The stored connection string could not be parsed as a Redis one.
    Unparseable { connection: String },
    /// A write was attempted on a connection without `allow_writes`. The write
    /// gate, distinct from exposure.
    WritesNotAllowed,
    /// The connection did not open. Carries a *kind*, no message.
    ConnectFailed { kind: RedisStatusKind },
    /// A command reached the server and failed. Carries a *kind*, no message.
    OperationFailed { kind: RedisStatusKind },
}

impl McpRefusal {
    pub fn code(&self) -> &'static str {
        match self {
            McpRefusal::NoConnectionsSaved => "noConnectionsSaved",
            McpRefusal::NoConnectionExposed { .. } => "noConnectionExposed",
            McpRefusal::ConnectionNotFound { .. } => "connectionNotFound",
            McpRefusal::ConnectionNotExposed { .. } => "connectionNotExposed",
            McpRefusal::ConnectionAmbiguous { .. } => "connectionAmbiguous",
            McpRefusal::SecretUnresolved { .. } => "secretUnresolved",
            McpRefusal::Unparseable { .. } => "unparseable",
            McpRefusal::WritesNotAllowed => "writesNotAllowed",
            McpRefusal::ConnectFailed { .. } => "connectFailed",
            McpRefusal::OperationFailed { .. } => "operationFailed",
        }
    }

    pub fn sentence(&self) -> String {
        match self {
            McpRefusal::NoConnectionsSaved => "There are no saved Redis connections at all. One \
                has to be created in the app's Redis panel first."
                .to_string(),
            McpRefusal::NoConnectionExposed { total_saved } => format!(
                "None of the {total_saved} saved Redis connections is exposed to agents. Exposure \
                 is per connection in the app's Redis panel and is off by default; nothing here \
                 can grant it."
            ),
            McpRefusal::ConnectionNotFound { selector } => format!(
                "No saved Redis connection is called {selector:?}. Use redis.list_connections to \
                 see the ones available."
            ),
            McpRefusal::ConnectionNotExposed { selector } => format!(
                "A saved Redis connection matches {selector:?}, and it is not exposed to agents. \
                 That is a per-connection choice in the app's Redis panel; nothing here can \
                 change it."
            ),
            McpRefusal::ConnectionAmbiguous {
                selector,
                candidates,
            } => format!(
                "{selector:?} matches {} exposed connections ({}), so it is refused rather than \
                 resolved. Name one by its handle.",
                candidates.len(),
                candidates.join(", ")
            ),
            McpRefusal::SecretUnresolved { connection } => format!(
                "The connection string for {connection:?} could not be read from where its \
                 profile says it lives, so nothing was attempted."
            ),
            McpRefusal::Unparseable { connection } => format!(
                "The connection string for {connection:?} could not be parsed as a Redis address."
            ),
            McpRefusal::WritesNotAllowed => crate::redis::ops::Refusal::WritesNotAllowed.sentence(),
            McpRefusal::ConnectFailed { kind } => format!(
                "The connection did not open ({}). The server's own message is not forwarded to \
                 an agent; the person who owns this connection can see it in the app's Redis \
                 panel.",
                status_name(*kind)
            ),
            McpRefusal::OperationFailed { kind } => format!(
                "The command failed ({}). The server's own message is not forwarded to an agent.",
                status_name(*kind)
            ),
        }
    }
}

fn status_name(kind: RedisStatusKind) -> &'static str {
    match kind {
        RedisStatusKind::Ok => "ok",
        RedisStatusKind::AuthFailed => "authFailed",
        RedisStatusKind::Unreachable => "unreachable",
        RedisStatusKind::TlsFailed => "tlsFailed",
        RedisStatusKind::Timeout => "timeout",
        RedisStatusKind::SecretUnresolved => "secretUnresolved",
        RedisStatusKind::Unparseable => "unparseable",
        RedisStatusKind::Failed => "failed",
    }
}

/// A successful answer.
pub fn text_result(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(text)])
}

/// A refusal as a tool execution error, carrying its code structurally.
pub fn refusal_result(refusal: &McpRefusal) -> CallToolResult {
    let mut result = CallToolResult::error(vec![ContentBlock::text(format!(
        "{}: {}",
        refusal.code(),
        refusal.sentence()
    ))]);
    result.structured_content = Some(json!({ "code": refusal.code() }));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_refusal_has_a_distinct_code() {
        let all = [
            McpRefusal::NoConnectionsSaved,
            McpRefusal::NoConnectionExposed { total_saved: 1 },
            McpRefusal::ConnectionNotFound {
                selector: "x".into(),
            },
            McpRefusal::ConnectionNotExposed {
                selector: "x".into(),
            },
            McpRefusal::ConnectionAmbiguous {
                selector: "x".into(),
                candidates: vec!["a".into()],
            },
            McpRefusal::SecretUnresolved {
                connection: "x".into(),
            },
            McpRefusal::Unparseable {
                connection: "x".into(),
            },
            McpRefusal::WritesNotAllowed,
            McpRefusal::ConnectFailed {
                kind: RedisStatusKind::AuthFailed,
            },
            McpRefusal::OperationFailed {
                kind: RedisStatusKind::Failed,
            },
        ];
        let mut codes: Vec<&str> = all.iter().map(|r| r.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), all.len(), "codes must be distinct");
    }

    #[test]
    fn not_exposed_and_writes_not_allowed_are_different_answers() {
        assert_ne!(
            McpRefusal::ConnectionNotExposed {
                selector: "x".into()
            }
            .code(),
            McpRefusal::WritesNotAllowed.code()
        );
    }
}
