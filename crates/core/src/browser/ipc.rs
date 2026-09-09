//! Every byte the page sends back. **This is the attack surface.**
//!
//! A wry-created child webview gets no `__TAURI_INTERNALS__`, no invoke key and
//! no ACL surface (see [`super`]), so the single `with_ipc_handler` this app
//! installs is the *only* bridge from remote content into this process. Which
//! means this function is the whole boundary, and the page on the other side of
//! it is arbitrary — it may be an app the user trusts, a page that app was
//! redirected to, or an advert in an iframe on either.
//!
//! # What a parsed message is allowed to cause
//!
//! Exactly two things, and both are bounded: an append to a [`super::ring`], or
//! the resolution of one pending evaluation the host itself asked for. Nothing
//! here navigates, nothing writes a file, nothing spawns anything, and nothing
//! grants consent — a page cannot ask for permission to be read, only the user
//! can give it.
//!
//! # The rules, each of which is a way this could have been wrong
//!
//! * **An unknown kind is counted, not applied.** A page that has worked out
//!   the message shapes will try shapes this app does not have yet; an
//!   `_ => {}` arm would make that invisible, and a fallback that treated it as
//!   the nearest known kind would let it choose. So it is refused *with the kind
//!   preserved*, and the caller counts it. A rising count is a prober, and the
//!   panel header shows it.
//! * **An evaluation reply for an id nobody is waiting for is refused.** The
//!   ids are the host's own, handed out one at a time; a page guessing them
//!   could otherwise answer a request it was not asked, and the answer would be
//!   attributed to a script this app wrote.
//! * **A payload of the wrong type is refused, not coerced.** `text: 5` is not
//!   `"5"`, and `text: {"toString": …}` is a page trying to be helpful. The
//!   field's type is part of the contract.
//! * **There is a size cap**, because the page controls the length and the host
//!   allocates.
//!
//! Every refusal names the field or the kind, so a genuinely new message shape
//! is debuggable and a hostile one is identifiable.

use serde_json::Value;
use thiserror::Error;

use super::model::NetworkSource;

/// The largest single message accepted from the page, in bytes.
///
/// Larger than any legitimate one — the biggest thing that travels this way is a
/// capped console line — and small enough that a page looping `postMessage`
/// cannot make the host allocate freely. Page *text* does not come this way at
/// all; it is the return value of an evaluated script.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// A message from the page that this app understands.
#[derive(Debug, Clone, PartialEq)]
pub enum PageMessage {
    /// A captured `console.*` call. `method` is the page's own spelling; the
    /// level is [`super::console::classify_level`]'s job, not this module's.
    Console { method: String, text: String },
    /// `window.onerror` or an unhandled promise rejection. **Not** a console
    /// call, and its level is `Error` by construction of this variant, because
    /// the page genuinely threw — see [`super::console`].
    PageError { source: String, text: String },
    /// One observed network request.
    Network {
        url: String,
        method: String,
        status: Option<u16>,
        duration_ms: Option<f64>,
        transfer_size: Option<u64>,
        source: NetworkSource,
    },
    /// The document title changed. Used for the minimized panel's label.
    Title { text: String },
    /// The answer to an evaluation the host asked for.
    ///
    /// Must still be matched against the pending set by
    /// [`accept_eval_reply`] — parsing it does not make it wanted.
    EvalReply { id: u64, payload: String },
}

/// Why a message from the page was not applied.
///
/// Every variant carries the specific thing that was wrong. There is no
/// catch-all, because the whole point is telling a new message shape apart from
/// a hostile one.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PageMessageProblem {
    #[error("the page sent {size} bytes, over the {limit}-byte limit")]
    TooLarge { size: usize, limit: usize },

    #[error("the page sent something that is not JSON: {error}")]
    NotJson { error: String },

    #[error("the page sent JSON that is not an object")]
    NotAnObject,

    #[error("the page sent a message with no string \"kind\"")]
    NoKind,

    /// The kind is preserved so the caller can count it *and name it*. A rising
    /// count of one unknown kind is a prober; a rising count of many is a page
    /// this app should learn to read.
    #[error("the page sent an unknown message kind {kind:?}, which was counted and not applied")]
    UnknownKind { kind: String },

    #[error("the page sent {kind:?} with {field:?} as {found}, but it must be {expected}")]
    WrongType {
        kind: String,
        field: String,
        expected: &'static str,
        found: &'static str,
    },

    #[error("the page answered evaluation {id}, which nothing is waiting for")]
    UnknownEvalId { id: u64 },
}

/// Parse one message from the page. **Trusts nothing.**
pub fn parse_page_message(raw: &str) -> Result<PageMessage, PageMessageProblem> {
    if raw.len() > MAX_MESSAGE_BYTES {
        return Err(PageMessageProblem::TooLarge {
            size: raw.len(),
            limit: MAX_MESSAGE_BYTES,
        });
    }
    let value: Value = serde_json::from_str(raw).map_err(|error| PageMessageProblem::NotJson {
        error: error.to_string(),
    })?;
    let object = value.as_object().ok_or(PageMessageProblem::NotAnObject)?;
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .ok_or(PageMessageProblem::NoKind)?
        .to_owned();

    match kind.as_str() {
        "console" => Ok(PageMessage::Console {
            method: require_str(object, &kind, "method")?,
            text: require_str(object, &kind, "text")?,
        }),
        "pageError" => Ok(PageMessage::PageError {
            source: require_str(object, &kind, "source")?,
            text: require_str(object, &kind, "text")?,
        }),
        "title" => Ok(PageMessage::Title {
            text: require_str(object, &kind, "text")?,
        }),
        "network" => Ok(PageMessage::Network {
            url: require_str(object, &kind, "url")?,
            method: require_str(object, &kind, "method")?,
            // A missing or null status is a real answer for a `resource` row,
            // so it is optional — but a *present* one of the wrong type is
            // still refused rather than dropped.
            status: optional_u16(object, &kind, "status")?,
            duration_ms: optional_f64(object, &kind, "durationMs")?,
            transfer_size: optional_u64(object, &kind, "transferSize")?,
            source: network_source(object, &kind)?,
        }),
        "evalReply" => Ok(PageMessage::EvalReply {
            id: require_u64(object, &kind, "id")?,
            payload: require_str(object, &kind, "payload")?,
        }),
        _ => Err(PageMessageProblem::UnknownKind { kind }),
    }
}

/// Whether an [`PageMessage::EvalReply`] is one the host is actually waiting
/// for.
///
/// Separate from parsing on purpose: parsing knows the message's shape and
/// nothing about the host's state, and the pending set is the host's. Keeping
/// them apart is what lets the whole hostile-input boundary be tested with no
/// host at all.
///
/// The ids are the host's own, handed out one at a time. A page guessing one
/// could otherwise answer a request it was never asked, and the answer would be
/// attributed to a script this app wrote.
pub fn accept_eval_reply(id: u64, pending: &[u64]) -> Result<u64, PageMessageProblem> {
    if pending.contains(&id) {
        Ok(id)
    } else {
        Err(PageMessageProblem::UnknownEvalId { id })
    }
}

type Object = serde_json::Map<String, Value>;

fn require_str(object: &Object, kind: &str, field: &str) -> Result<String, PageMessageProblem> {
    match object.get(field) {
        Some(Value::String(text)) => Ok(text.clone()),
        other => Err(wrong_type(kind, field, "a string", other)),
    }
}

fn require_u64(object: &Object, kind: &str, field: &str) -> Result<u64, PageMessageProblem> {
    match object.get(field) {
        Some(Value::Number(number)) => number.as_u64().ok_or_else(|| {
            wrong_type(
                kind,
                field,
                "a non-negative whole number",
                object.get(field),
            )
        }),
        other => Err(wrong_type(
            kind,
            field,
            "a non-negative whole number",
            other,
        )),
    }
}

fn optional_u16(
    object: &Object,
    kind: &str,
    field: &str,
) -> Result<Option<u16>, PageMessageProblem> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| {
                wrong_type(
                    kind,
                    field,
                    "an HTTP status code or null",
                    object.get(field),
                )
            }),
        other => Err(wrong_type(
            kind,
            field,
            "an HTTP status code or null",
            other,
        )),
    }
}

fn optional_f64(
    object: &Object,
    kind: &str,
    field: &str,
) -> Result<Option<f64>, PageMessageProblem> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            // A negative or non-finite duration is not a duration. Refused
            // rather than clamped: a clamped zero looks like a fast request.
            match number.as_f64() {
                Some(seconds) if seconds.is_finite() && seconds >= 0.0 => Ok(Some(seconds)),
                _ => Err(wrong_type(
                    kind,
                    field,
                    "a duration in milliseconds, or null",
                    object.get(field),
                )),
            }
        }
        other => Err(wrong_type(
            kind,
            field,
            "a duration in milliseconds, or null",
            other,
        )),
    }
}

fn optional_u64(
    object: &Object,
    kind: &str,
    field: &str,
) -> Result<Option<u64>, PageMessageProblem> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .map(Some)
            .ok_or_else(|| wrong_type(kind, field, "a byte count, or null", object.get(field))),
        other => Err(wrong_type(kind, field, "a byte count, or null", other)),
    }
}

/// The source is an allowlist of three, and an unrecognised one is refused
/// rather than defaulted to `resource`.
///
/// Defaulting would be the tempting shortcut and it inverts the honesty the
/// field exists for: `resource` is the *least* capable source, so a mislabelled
/// `fetch` row would silently claim it has no status when it does — and a page
/// could label its own fabricated rows however it liked.
fn network_source(object: &Object, kind: &str) -> Result<NetworkSource, PageMessageProblem> {
    match object.get("source").and_then(Value::as_str) {
        Some("fetch") => Ok(NetworkSource::Fetch),
        Some("xhr") => Ok(NetworkSource::Xhr),
        Some("resource") => Ok(NetworkSource::Resource),
        _ => Err(wrong_type(
            kind,
            "source",
            "one of \"fetch\", \"xhr\" or \"resource\"",
            object.get("source"),
        )),
    }
}

fn wrong_type(
    kind: &str,
    field: &str,
    expected: &'static str,
    found: Option<&Value>,
) -> PageMessageProblem {
    PageMessageProblem::WrongType {
        kind: kind.to_owned(),
        field: field.to_owned(),
        expected,
        found: describe(found),
    }
}

fn describe(value: Option<&Value>) -> &'static str {
    match value {
        None => "absent",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "a boolean",
        Some(Value::Number(_)) => "a number",
        Some(Value::String(_)) => "a string",
        Some(Value::Array(_)) => "an array",
        Some(Value::Object(_)) => "an object",
    }
}

#[cfg(test)]
#[path = "ipc_tests.rs"]
mod tests;
