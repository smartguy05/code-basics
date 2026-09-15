//! What travels on the Build control pipe, in both directions.
//!
//! Two processes: the **shim** (`cb-app mcp-build`, started by an agent, no window,
//! speaks MCP on its stdio) and the **application** (the one that runs and caches
//! the build). The shim owns no build and decides nothing about it; the application
//! owns all of it. So the pipe carries a tool call one way and *the words the agent
//! will read* the other way — [`super::render`] composes the prose inside the
//! application, and the shim's whole job is wrapping a [`ToolAnswer`] in a
//! `CallToolResult`.
//!
//! # Framing is not here
//!
//! [`crate::mcp::ndjson`] is the wire format, reused unchanged. This module is the
//! *contents* of those lines.
//!
//! # The envelope is JSON-RPC, and the method is one name
//!
//! Every request is [`METHOD`]; which tool it is travels in the params. One method
//! rather than four because the application's dispatch is over
//! [`super::tools::BuildToolCall`] — an exhaustive enum.
//!
//! # The workspace travels on every request
//!
//! [`Request::workspace`] carries the `--workspace` boundary the shim was installed
//! with. The shim uses it to choose *which* application to connect to
//! ([`super::instances::choose_instance`]); the application uses it to resolve
//! *which* workspace answers. Both need it, so it is on the wire.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The only method on this pipe.
pub const METHOD: &str = "build.tool";

/// One tool call, as the application receives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    /// The wire version the caller speaks; refused when it is not
    /// [`super::instances::PROTOCOL_VERSION`].
    pub protocol: u32,
    /// The per-launch token from the registry. A speed bump — see
    /// [`super::instances::Listener::token`].
    pub token: String,
    /// The workspace root this shim was installed for — the boundary.
    pub workspace: String,
    /// The MCP tool name, e.g. `get_errors`.
    pub tool: String,
    /// The tool's arguments, exactly as the MCP client sent them. An object, or
    /// `null` when the client sent none — the distinction is preserved rather than
    /// normalised to `{}`.
    pub arguments: Value,
}

/// What the application answers.
///
/// A refusal is **not** an error response: it is a successful reply carrying
/// `ok: false`, because the specification asks that a tool's own refusal reach the
/// model so it can correct itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnswer {
    pub ok: bool,
    /// A machine-matchable code for a refusal. `None` on success.
    pub code: Option<String>,
    /// The words the agent reads. Never empty, in either direction.
    pub text: String,
}

impl ToolAnswer {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            ok: true,
            code: None,
            text: text.into(),
        }
    }

    /// A refusal, with the code first so a model can branch on it.
    pub fn refused(code: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            ok: false,
            code: Some(code.into()),
            text: text.into(),
        }
    }
}

/// Encode a request as a JSON-RPC message. `id` is echoed back by the application
/// untouched.
pub fn request_value(id: u64, request: &Request) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": METHOD,
        "params": request,
    })
}

/// Encode an answer as a JSON-RPC response.
pub fn answer_value(id: &Value, answer: &ToolAnswer) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": answer })
}

/// Why a request could not be acted on at all — before any tool ran.
///
/// Kept apart from a tool's own refusal ([`ToolAnswer`]) because these are all
/// *the caller is broken*, and none of them is something a model can fix by
/// reasoning about the build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestProblem {
    /// A method this pipe does not have.
    UnknownMethod { method: String },
    /// A protocol number this build does not speak.
    ProtocolMismatch { theirs: u32, ours: u32 },
    /// The token did not match this launch's.
    BadToken,
    /// The params were not the declared shape.
    Shape { detail: String },
}

impl RequestProblem {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownMethod { .. } => "unknown_method",
            Self::ProtocolMismatch { .. } => "protocol_mismatch",
            Self::BadToken => "bad_token",
            Self::Shape { .. } => "bad_request",
        }
    }

    pub fn sentence(&self) -> String {
        match self {
            Self::UnknownMethod { method } => {
                format!("This pipe answers only {METHOD:?}; it was asked for {method:?}.")
            }
            Self::ProtocolMismatch { theirs, ours } => format!(
                "The caller speaks build protocol {theirs} and this application speaks {ours}, so \
                 nothing was done. Reinstall the Build MCP entry from this application."
            ),
            Self::BadToken => {
                "The token presented does not match this application's. Read it from \
                 the build instance registry this application published."
                    .to_string()
            }
            Self::Shape { detail } => format!("The request was not the expected shape: {detail}"),
        }
    }

    /// The refusal as an answer, so the caller gets prose rather than a dropped
    /// connection.
    pub fn answer(&self) -> ToolAnswer {
        ToolAnswer::refused(self.code(), self.sentence())
    }
}

/// Read a `build.tool` request, checking the method, the shape, the protocol and
/// the token — **in that order**.
///
/// The order matters and is pinned by a test: a caller speaking a protocol this
/// build does not know may have put the token somewhere else entirely, so
/// reporting a bad token for it would name the wrong cause.
pub fn parse_request(
    method: &str,
    params: Option<&Value>,
    expected_token: &str,
) -> Result<Request, RequestProblem> {
    if method != METHOD {
        return Err(RequestProblem::UnknownMethod {
            method: method.to_string(),
        });
    }
    let params = params.ok_or_else(|| RequestProblem::Shape {
        detail: format!("{METHOD} takes params and none were sent"),
    })?;
    let request: Request =
        serde_json::from_value(params.clone()).map_err(|error| RequestProblem::Shape {
            detail: error.to_string(),
        })?;

    if request.protocol != super::instances::PROTOCOL_VERSION {
        return Err(RequestProblem::ProtocolMismatch {
            theirs: request.protocol,
            ours: super::instances::PROTOCOL_VERSION,
        });
    }
    if !token_matches(&request.token, expected_token) {
        return Err(RequestProblem::BadToken);
    }
    Ok(request)
}

/// Whether a presented token is this launch's.
///
/// The empty-token case is refused explicitly: an application that somehow
/// published no token must not accept every caller that also sends none.
pub fn token_matches(presented: &str, expected: &str) -> bool {
    !expected.is_empty() && presented == expected
}

/// Why a request never produced an answer, on the **shim's** side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipeFailure {
    /// The pipe named in the registry could not be opened. The ordinary cause is
    /// the application exiting between the registry read and the connect.
    NotConnected { pipe: String, detail: String },
    /// The application did not answer inside the deadline. Its own answer, never
    /// folded into "not running": a build is legitimately slow.
    Timeout { tool: String, ms: u64 },
    /// The pipe closed part-way through — the application exited mid-call.
    Closed { tool: String },
    /// A reply this build could not read.
    Malformed { detail: String },
}

impl PipeFailure {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotConnected { .. } => "app_not_reachable",
            Self::Timeout { .. } => "timeout",
            Self::Closed { .. } => "app_exited",
            Self::Malformed { .. } => "bad_reply",
        }
    }

    pub fn sentence(&self) -> String {
        match self {
            Self::NotConnected { pipe, detail } => format!(
                "The code-basics application published {pipe} but the connection was refused \
                 ({detail}). Nothing was read. The usual cause is the application closing between \
                 publishing and now — ask again after confirming it is running."
            ),
            Self::Timeout { tool, ms } => format!(
                "{tool} did not answer within {ms}ms, so nothing can be reported. This is not the \
                 same as the application being closed: it is running and did not reply in time, \
                 which for a large solution can mean the build is still going."
            ),
            Self::Closed { tool } => format!(
                "The code-basics application closed the connection during {tool}, so the call did \
                 not complete and nothing can be reported."
            ),
            Self::Malformed { detail } => format!(
                "The application's reply could not be read ({detail}), so its answer is unknown. \
                 This server and the running application are probably different builds; reinstall \
                 the Build MCP entry from the running application."
            ),
        }
    }

    pub fn answer(&self) -> ToolAnswer {
        ToolAnswer::refused(self.code(), self.sentence())
    }
}

/// Read a reply. Anything that is not a well-formed [`ToolAnswer`] with text is
/// [`PipeFailure::Malformed`] rather than a silently empty success.
pub fn parse_answer(value: &Value) -> Result<ToolAnswer, PipeFailure> {
    if let Some(error) = value.get("error") {
        return Err(PipeFailure::Malformed {
            detail: format!("the application answered with a protocol error: {error}"),
        });
    }
    let result = value.get("result").ok_or_else(|| PipeFailure::Malformed {
        detail: "the reply carried neither a result nor an error".to_string(),
    })?;
    let answer: ToolAnswer =
        serde_json::from_value(result.clone()).map_err(|error| PipeFailure::Malformed {
            detail: error.to_string(),
        })?;
    if answer.text.trim().is_empty() {
        return Err(PipeFailure::Malformed {
            detail: "the application's answer had no text in it".to_string(),
        });
    }
    Ok(answer)
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
