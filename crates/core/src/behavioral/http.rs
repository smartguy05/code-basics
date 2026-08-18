//! Diffing two HTTP responses for one replayed request.
//!
//! Pure: it takes two already-recorded responses (the network lives in
//! [`super::replay`]) and reports how they differ. The nondeterminism an HTTP
//! response carries — a `Date` header, a request id, a timestamp in the body,
//! JSON key order — is filtered out so only a *behavioral* difference survives:
//!
//! * Volatile headers ([`VOLATILE_HEADERS`], plus a caller list) are ignored.
//! * JSON bodies are compared **structurally** (keys sorted) so serialisation
//!   order is never a false delta, with timestamps/ids masked in values.
//! * A status-code change is the cleanest signal there is → [`Confidence::High`];
//!   a body declared JSON that will not parse drops to [`Confidence::Low`].

use std::collections::BTreeMap;

use serde_json::Value;

use super::console::{mask_timestamps_and_ids, multiset_minus};
use super::{BodyDelta, HeaderChange, HttpDelta};
use crate::git::attribution::Confidence;

/// Headers whose value routinely changes between two identical runs, so a
/// difference in them is never behavioral.
pub const VOLATILE_HEADERS: &[&str] = &[
    "date",
    "server",
    "set-cookie",
    "x-request-id",
    "request-id",
    "traceparent",
    "age",
    "etag",
    "last-modified",
    "x-correlation-id",
    "keep-alive",
    "connection",
];

/// One captured response, produced by [`super::replay`].
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub content_type: Option<String>,
}

impl HttpDelta {
    /// Did the two responses differ in any observable way?
    pub fn is_change(&self) -> bool {
        self.status.is_some() || !self.header_changes.is_empty() || self.body.is_some()
    }
}

/// Compare two responses. `ignore` extends [`VOLATILE_HEADERS`] with
/// scenario-specific header names.
pub fn diff_http(
    name: &str,
    before: &RecordedResponse,
    after: &RecordedResponse,
    ignore: &[&str],
) -> HttpDelta {
    let status = (before.status != after.status).then_some((before.status, after.status));
    let header_changes = diff_headers(before, after, ignore);
    let (body, body_unparseable) = diff_body(before, after);

    let confidence = if status.is_some() {
        // A status-code change is the strongest, cleanest signal.
        Confidence::High
    } else if body_unparseable {
        Confidence::Low
    } else if !header_changes.is_empty() || body.is_some() {
        Confidence::Medium
    } else {
        Confidence::High // no change at all
    };

    HttpDelta {
        name: name.to_string(),
        status,
        header_changes,
        body,
        confidence,
    }
}

fn ignored(name_lc: &str, extra: &[&str]) -> bool {
    VOLATILE_HEADERS.contains(&name_lc) || extra.iter().any(|h| h.eq_ignore_ascii_case(name_lc))
}

/// Collapse a header list to lower-cased name → joined value, dropping ignored
/// names.
fn header_map(resp: &RecordedResponse, extra: &[&str]) -> BTreeMap<String, String> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, value) in &resp.headers {
        let lc = name.to_ascii_lowercase();
        if ignored(&lc, extra) {
            continue;
        }
        map.entry(lc).or_default().push(value.clone());
    }
    map.into_iter().map(|(k, v)| (k, v.join(", "))).collect()
}

fn diff_headers(
    before: &RecordedResponse,
    after: &RecordedResponse,
    ignore: &[&str],
) -> Vec<HeaderChange> {
    let b = header_map(before, ignore);
    let a = header_map(after, ignore);

    let mut names: Vec<&String> = b.keys().collect();
    for name in a.keys() {
        if !b.contains_key(name) {
            names.push(name);
        }
    }
    names.sort();

    let mut changes = Vec::new();
    for name in names {
        let bv = b.get(name);
        let av = a.get(name);
        if bv != av {
            changes.push(HeaderChange {
                name: name.clone(),
                before: bv.cloned(),
                after: av.cloned(),
            });
        }
    }
    changes
}

fn is_json(resp: &RecordedResponse) -> bool {
    resp.content_type
        .as_deref()
        .map(|ct| ct.to_ascii_lowercase().contains("json"))
        .unwrap_or(false)
}

/// Recursively sort object keys so JSON key order is never a difference.
fn canonicalize(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let sorted: serde_json::Map<String, Value> = m
                .iter()
                .map(|(k, val)| (k.clone(), canonicalize(val)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect();
            Value::Object(sorted)
        }
        Value::Array(a) => Value::Array(a.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}

fn masked_lines(text: &str) -> Vec<String> {
    text.lines().map(mask_timestamps_and_ids).collect()
}

/// Returns `(body_delta, declared_json_but_unparseable)`.
fn diff_body(before: &RecordedResponse, after: &RecordedResponse) -> (Option<BodyDelta>, bool) {
    let both_json = is_json(before) && is_json(after);

    if both_json {
        match (
            serde_json::from_str::<Value>(&before.body),
            serde_json::from_str::<Value>(&after.body),
        ) {
            (Ok(b), Ok(a)) => {
                // Canonicalise (key order) then mask (timestamps/ids) and
                // compare the resulting lines — so both kinds of noise are gone
                // before we decide there is a difference at all.
                let b_lines = masked_lines(&serde_json::to_string_pretty(&canonicalize(&b)).unwrap_or_default());
                let a_lines = masked_lines(&serde_json::to_string_pretty(&canonicalize(&a)).unwrap_or_default());
                if b_lines == a_lines {
                    return (None, false);
                }
                return (Some(body_delta(&b_lines, &a_lines)), false);
            }
            // Declared JSON but did not parse: fall through to text, flag it.
            _ => {
                let delta = text_body_delta(before, after);
                return (delta, true);
            }
        }
    }

    (text_body_delta(before, after), false)
}

fn text_body_delta(before: &RecordedResponse, after: &RecordedResponse) -> Option<BodyDelta> {
    let b = masked_lines(&before.body);
    let a = masked_lines(&after.body);
    if b == a {
        return None;
    }
    Some(body_delta(&b, &a))
}

fn body_delta(before: &[String], after: &[String]) -> BodyDelta {
    BodyDelta {
        removed_lines: multiset_minus(before, after),
        added_lines: multiset_minus(after, before),
        normalized: true,
    }
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
