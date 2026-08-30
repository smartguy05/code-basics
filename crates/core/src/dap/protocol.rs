//! The Debug Adapter Protocol's base messages, and the handful of requests,
//! responses and events this app actually uses.
//!
//! # What DAP shares with LSP, and what it does not
//!
//! The **framing is identical** — `Content-Length: n\r\n\r\n` then `n` bytes of
//! JSON — so this module does not have a framing layer of its own and
//! [`crate::lsp::framing`] is used unchanged. That module is pure byte work with
//! nothing LSP-specific in it, and a second copy would be a second place for the
//! fail-closed rule to drift.
//!
//! The **envelope is not** JSON-RPC and must not be treated as though it were.
//! There is no `jsonrpc` field, no `method`, no `params`, and no `error` object.
//! A message carries a `seq` of its own, a response points back with
//! `request_seq`, and failure is a `success: false` **response** rather than a
//! separate error shape. Reusing the LSP correlator here would have matched on
//! the wrong field and silently paired responses with the wrong requests.
//!
//! # Reverse requests are requests
//!
//! An adapter may send a *request* to the client (`runInTerminal`,
//! `startDebugging`). It is a `"request"` like any other and it **blocks the
//! adapter until it is answered** — the same trap [`crate::lsp::jsonrpc`]
//! documents, and the reason [`Message`] keeps the direction out of the type: a
//! request is decoded the same way whichever end sent it, so a reader cannot
//! quietly drop one by classifying it as something it need not answer.
//!
//! # Unknown messages are kept, not dropped
//!
//! Every body is [`serde_json::Value`]. Adapters differ in what they send and
//! add fields between versions, and a strongly typed body would turn an
//! unrecognised event into a decode failure of the whole stream. Typed views are
//! offered as `TryFrom`-style accessors over the value instead, so a body that
//! does not parse costs one message rather than the session.

use serde::{Deserialize, Serialize};

/// One message on the wire, in either direction.
///
/// Tagged on `type`, which is the only field all three share. `#[serde(other)]`
/// is deliberately **not** used: an unrecognised `type` is a protocol the
/// decoder does not understand, and continuing past it would mean guessing
/// whether the adapter is still waiting for something.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Message {
    Request(Request),
    Response(Response),
    Event(Event),
}

impl Message {
    /// The message's own sequence number.
    pub fn seq(&self) -> i64 {
        match self {
            Message::Request(r) => r.seq,
            Message::Response(r) => r.seq,
            Message::Event(e) => e.seq,
        }
    }
}

/// A request, from either end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub seq: i64,
    pub command: String,
    /// Absent rather than `null` when a command takes no arguments — some
    /// adapters reject an explicit `null` here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
}

/// A response to a request.
///
/// `success` is the whole of the error model: there is no `error` object. A
/// failed response may carry `message` (a short id) and a `body.error` with
/// something human-readable, and both are kept because adapters disagree about
/// which they populate.
///
/// **No `rename_all` here, unlike almost every other wire type in this crate.**
/// DAP spells this field `request_seq`, in snake case, alone among the base
/// protocol's fields. Camel-casing it produces `requestSeq`, which no adapter
/// sends — so every response would decode with `request_seq: 0`, every
/// correlation would fail, and the debugger would appear to hang on its first
/// request. Caught by a test, which is why there is one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub seq: i64,
    pub request_seq: i64,
    pub success: bool,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

impl Response {
    /// What to show the user when a request failed.
    ///
    /// Prefers the adapter's own words over the command name, and says which
    /// command failed either way — "failed" on its own has sent people looking
    /// in the wrong place more than once.
    pub fn failure_text(&self) -> String {
        let detail = self
            .message
            .as_deref()
            .filter(|m| !m.trim().is_empty())
            .map(str::to_string)
            .or_else(|| {
                self.body
                    .as_ref()
                    .and_then(|b| b.get("error"))
                    .and_then(|e| e.get("format"))
                    .and_then(|f| f.as_str())
                    .map(str::to_string)
            });

        match detail {
            Some(detail) => format!("{} failed: {detail}", self.command),
            None => format!("{} failed, with no reason given", self.command),
        }
    }
}

/// An event, always from the adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub seq: i64,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// The events whose bodies this app reads
// ---------------------------------------------------------------------------

/// Why the debuggee stopped, and which thread noticed.
///
/// `thread_id` is an `Option` because the specification makes it optional, and
/// the distinction is load-bearing: absent means *every* thread stopped, which
/// is a different thing from thread 0 stopping. Defaulting it to some number
/// would send the next `stackTrace` at a thread that does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stopped {
    /// `breakpoint`, `step`, `exception`, `pause`, `entry`, … Adapters invent
    /// their own, so this stays a string.
    pub reason: String,
    pub thread_id: Option<i64>,
    /// A sentence to show the user, when the adapter offered one.
    pub description: Option<String>,
    /// More detail still — an exception's message, typically.
    pub text: Option<String>,
    /// The adapter is telling us every thread stopped, not just this one.
    pub all_threads_stopped: bool,
}

impl Stopped {
    pub fn from_body(body: Option<&serde_json::Value>) -> Option<Self> {
        let body = body?;
        Some(Stopped {
            reason: body.get("reason")?.as_str()?.to_string(),
            thread_id: body.get("threadId").and_then(serde_json::Value::as_i64),
            description: string_field(body, "description"),
            text: string_field(body, "text"),
            all_threads_stopped: body
                .get("allThreadsStopped")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        })
    }
}

/// A line of the debuggee's output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// `stdout`, `stderr`, `console`, `telemetry`, … Defaults to `console`,
    /// which is what the specification says an absent category means.
    pub category: String,
    pub output: String,
}

impl Output {
    pub fn from_body(body: Option<&serde_json::Value>) -> Option<Self> {
        let body = body?;
        Some(Output {
            category: string_field(body, "category").unwrap_or_else(|| "console".to_string()),
            output: body.get("output")?.as_str()?.to_string(),
        })
    }
}

/// The debuggee exited, with the code it exited with.
pub fn exited_code(body: Option<&serde_json::Value>) -> Option<i64> {
    body?.get("exitCode")?.as_i64()
}

fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_string)
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

/// The parts of an adapter's `initialize` response this app acts on.
///
/// Every field is a plain `bool` defaulting to `false`, because the
/// specification says an absent capability means unsupported. That is the one
/// place a default is honest here: the adapter was asked and did not claim it.
///
/// What is *not* modelled is anything this app does not yet use. A capability
/// list that pretended to be complete would invite reading a field that is
/// always `false` because nothing ever populates it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Capabilities {
    /// `configurationDone` is expected after the initial breakpoints are set.
    /// When this is false that request must **not** be sent — several adapters
    /// answer an unsupported command with a failure that reads like a crash.
    pub supports_configuration_done_request: bool,
    /// `terminate` (a graceful stop) exists, as distinct from `disconnect`.
    pub supports_terminate_request: bool,
    /// `setVariable` exists.
    pub supports_set_variable: bool,
    /// Breakpoints can be conditional.
    pub supports_conditional_breakpoints: bool,
    /// `evaluate` accepts `context: "hover"`.
    pub supports_evaluate_for_hovers: bool,
    /// The adapter can report which lines a breakpoint may be placed on.
    pub supports_breakpoint_locations_request: bool,
}

// ---------------------------------------------------------------------------
// Building the requests this app sends
// ---------------------------------------------------------------------------

/// `initialize` arguments.
///
/// The two `lines_start_at_1` / `columns_start_at_1` flags are the client
/// telling the adapter which convention *it* will use — see
/// [`super::positions`]. They are sent as `true`/`true` and nothing here ever
/// varies them, so there is exactly one convention on this side of the wire.
pub fn initialize_arguments(client_id: &str, adapter_id: &str) -> serde_json::Value {
    serde_json::json!({
        "clientID": client_id,
        "clientName": "code-basics",
        "adapterID": adapter_id,
        "locale": "en",
        "linesStartAt1": true,
        "columnsStartAt1": true,
        "pathFormat": "path",
        "supportsRunInTerminalRequest": false,
        "supportsStartDebuggingRequest": false,
        "supportsVariableType": true,
        "supportsProgressReporting": false,
    })
}

/// `setBreakpoints` arguments for one source file.
///
/// The whole file's breakpoints are sent every time, because that is what the
/// request means: it *replaces* the set for that source. Sending only the new
/// one silently clears the others, which looks exactly like a debugger ignoring
/// your breakpoints.
pub fn set_breakpoints_arguments(path: &str, lines: &[u32]) -> serde_json::Value {
    serde_json::json!({
        "source": { "path": path },
        "breakpoints": lines
            .iter()
            .map(|line| serde_json::json!({ "line": line }))
            .collect::<Vec<_>>(),
        // Sent for adapters that predate the `breakpoints` array. Harmless to
        // the ones that do not read it, and the two must agree.
        "lines": lines,
    })
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
