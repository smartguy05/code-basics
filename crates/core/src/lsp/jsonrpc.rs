//! JSON-RPC 2.0, and nothing about LSP.
//!
//! Kept separate from [`super::protocol`] because the two fail differently: a
//! bad envelope means the stream is unusable, while an unexpected *payload*
//! means one request could not be answered. Conflating them would make a single
//! malformed `references` result look like a dead server.
//!
//! # Why the three shapes have to be told apart precisely
//!
//! A server→client **request** must be answered or the server blocks forever —
//! verified against the real Roslyn server, which issues
//! `workspace/configuration` during start-up and waits. A **notification** must
//! not be answered, because a reply to a message with no id is not addressed to
//! anything. The only thing separating them on the wire is the *presence* of an
//! `id` key, and `"id": null` is present. Reading a null-id request as a
//! notification is therefore a hang, and it is a hang that looks exactly like a
//! slow project load.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// The JSON-RPC error code for a method we do not implement.
pub const METHOD_NOT_FOUND: i64 = -32601;

/// Correlates a request with its response.
///
/// `Num` and `Str` are distinct even when they print the same: they identify
/// different requests, so comparing them by their printed form would let one
/// response resolve another request's waiter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Num(i64),
    Str(String),
    /// Legal, discouraged, and shipped. Present, and therefore not a notification.
    Null,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

/// A message read from a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// An answer to something we asked. Exactly one of `result`/`error` is set,
    /// except on servers that send both — see [`classify`].
    Response {
        id: RequestId,
        result: Option<Value>,
        error: Option<RpcError>,
    },
    /// Something the server is asking us. **Must be answered.**
    Request {
        id: RequestId,
        method: String,
        params: Option<Value>,
    },
    /// Something the server is telling us. Must *not* be answered.
    Notification {
        method: String,
        params: Option<Value>,
    },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("message is not valid JSON: {detail}")]
    NotJson { detail: String },
    #[error("message is not a JSON object")]
    NotAnObject,
    #[error("message is neither a request, a notification, nor a response")]
    UnknownShape,
    #[error("a response arrived with no id to correlate it to")]
    ResponseWithoutId,
}

/// Decide which of the three shapes a message body is.
pub fn classify(bytes: &[u8]) -> Result<Incoming, ProtocolError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|e| ProtocolError::NotJson {
        detail: e.to_string(),
    })?;
    let object = value.as_object().ok_or(ProtocolError::NotAnObject)?;

    if let Some(method) = object.get("method").and_then(Value::as_str) {
        let params = object.get("params").cloned();
        // `contains_key`, not `get(..).is_some()`: an explicit `"id": null`
        // makes this a request that must be answered, and `Value::Null` would
        // be indistinguishable from an absent key under a truthiness test.
        return Ok(if object.contains_key("id") {
            Incoming::Request {
                id: id_of(object.get("id")),
                method: method.to_string(),
                params,
            }
        } else {
            Incoming::Notification {
                method: method.to_string(),
                params,
            }
        });
    }

    let has_result = object.contains_key("result");
    let error = object
        .get("error")
        .filter(|e| !e.is_null())
        .and_then(|e| serde_json::from_value::<RpcError>(e.clone()).ok());
    if !has_result && error.is_none() {
        return Err(ProtocolError::UnknownShape);
    }
    let id = object.get("id").ok_or(ProtocolError::ResponseWithoutId)?;
    Ok(Incoming::Response {
        id: id_of(Some(id)),
        // A server that sends both is out of spec, and the error wins: acting on
        // a result the server has just called invalid is worse than reporting
        // the failure.
        result: if error.is_some() {
            None
        } else {
            object.get("result").cloned()
        },
        error,
    })
}

fn id_of(value: Option<&Value>) -> RequestId {
    match value {
        Some(Value::Number(n)) => n.as_i64().map_or(RequestId::Null, RequestId::Num),
        Some(Value::String(s)) => RequestId::Str(s.clone()),
        _ => RequestId::Null,
    }
}

/// A message written to a server.
///
/// One struct rather than an enum so the field-presence rules — which are what
/// the protocol actually cares about — are visible in one place: a notification
/// has **no** `id` key at all (an `"id": null` would make the server expect a
/// reply), and a reply always has a `result` key even when its value is null (a
/// message with neither `result` nor `error` is not a reply).
#[derive(Debug, Clone, Serialize)]
pub struct Outgoing {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<RequestId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Outgoing {
    fn empty() -> Self {
        Self {
            jsonrpc: "2.0",
            id: None,
            method: None,
            params: None,
            result: None,
            error: None,
        }
    }

    pub fn request(id: RequestId, method: &'static str, params: Value) -> Self {
        Self {
            id: Some(id),
            method: Some(method),
            params: Some(params),
            ..Self::empty()
        }
    }

    pub fn notification(method: &'static str, params: Value) -> Self {
        Self {
            method: Some(method),
            params: Some(params),
            ..Self::empty()
        }
    }

    pub fn reply(id: RequestId, result: Value) -> Self {
        Self {
            id: Some(id),
            // `Some(Value::Null)` on purpose: the key must be present.
            result: Some(result),
            ..Self::empty()
        }
    }

    pub fn error_reply(id: RequestId, code: i64, message: &str) -> Self {
        Self {
            id: Some(id),
            error: Some(RpcError {
                code,
                message: message.to_string(),
            }),
            ..Self::empty()
        }
    }
}

#[cfg(test)]
#[path = "jsonrpc_tests.rs"]
mod jsonrpc_tests;
