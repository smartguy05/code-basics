//! The one impure corner of behavioral testing: firing HTTP requests.
//!
//! Every other module here is pure — a parse, a diff, a tally — and is tested
//! headlessly. This module is the deliberate exception, modelled on
//! [`crate::lsp::transport`]: it is the thin layer that actually touches the
//! network, kept small so that everything *around* it stays testable. It is
//! **synchronous** (blocking [`reqwest`]); the command wraps it in
//! `tokio::task::spawn_blocking` so the blocking client never runs on the async
//! executor.
//!
//! # Why there are no network unit tests
//!
//! A unit test that binds a socket and serves HTTP is a flaky test that depends
//! on ports, timing and the OS network stack — exactly the kind this codebase
//! keeps out of `cb-core`. So, as with [`crate::lsp::transport`] and
//! [`crate::lsp::client`], the socket work itself is uncovered; the *decisions*
//! it feeds ([`super::diff_http`], [`super::prepare`]) are pure and fully
//! tested. The only pieces extracted for unit testing are the ones that decide
//! something without a socket: assembling a [`RecordedResponse`] from response
//! parts, and picking the readiness backoff sleep.

use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use reqwest::Method;

use super::http::RecordedResponse;
use super::httpfile::{HttpRequestSpec, Readiness};

/// Per-request ceiling so a hung endpoint cannot wedge the whole replay.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Assemble a [`RecordedResponse`] from the parts a response yields.
///
/// Pure and unit-tested: `content_type` is the first `content-type` header
/// (case-insensitive), so [`super::diff_http`] can decide JSON-vs-text without
/// re-scanning the header list. Headers are kept as received.
pub fn record_from_parts(
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
) -> RecordedResponse {
    let content_type = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.clone());
    RecordedResponse {
        status,
        headers,
        body,
        content_type,
    }
}

/// How long to sleep before the next readiness poll: the poll interval, but
/// never past the deadline (so the loop does not overshoot `timeout`).
///
/// Pure and unit-tested. Returns `None` when the deadline has already passed.
pub fn ready_backoff(now: Instant, deadline: Instant, interval: Duration) -> Option<Duration> {
    if now >= deadline {
        return None;
    }
    let remaining = deadline - now;
    Some(remaining.min(interval))
}

fn parse_method(method: &str) -> Result<Method, String> {
    Method::from_bytes(method.to_ascii_uppercase().as_bytes())
        .map_err(|_| format!("unsupported HTTP method `{method}`"))
}

fn client() -> Result<Client, String> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("could not build HTTP client: {e}"))
}

/// Read a finished blocking response into a [`RecordedResponse`].
fn record_response(resp: reqwest::blocking::Response) -> Result<RecordedResponse, String> {
    let status = resp.status().as_u16();
    let headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                value.to_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    let body = resp
        .text()
        .map_err(|e| format!("could not read response body: {e}"))?;
    Ok(record_from_parts(status, headers, body))
}

/// Send one request and record its response. Blocking; call off the executor.
pub fn send(req: &HttpRequestSpec) -> Result<RecordedResponse, String> {
    let method = parse_method(&req.method)?;
    let client = client()?;

    let mut builder = client.request(method, &req.url);
    for (name, value) in &req.headers {
        builder = builder.header(name, value);
    }
    if let Some(body) = &req.body {
        builder = builder.body(body.clone());
    }

    let resp = builder
        .send()
        .map_err(|e| format!("request `{}` to {} failed: {e}", req.name, req.url))?;
    record_response(resp)
}

/// Poll `r.url` with `r.method` until the response status matches
/// `r.expect_status`, or `r.timeout` elapses. Blocking; call off the executor.
///
/// On timeout returns `Err` naming the url — the caller turns that into an
/// abstain warning rather than a hard failure, so a server that never came up
/// drops the HTTP side without sinking the whole report.
pub fn await_ready(r: &Readiness) -> Result<(), String> {
    let method = parse_method(&r.method)?;
    let client = client()?;
    let deadline = Instant::now() + r.timeout;

    loop {
        // A probe error (connection refused while the app is still starting) is
        // expected, not fatal: treat it as "not ready yet" and keep polling.
        if let Ok(resp) = client.request(method.clone(), &r.url).send() {
            if resp.status().as_u16() == r.expect_status {
                return Ok(());
            }
        }

        match ready_backoff(Instant::now(), deadline, r.poll_interval) {
            Some(sleep) => std::thread::sleep(sleep),
            None => {
                return Err(format!(
                    "readiness probe never saw {} return {} within {:?}: {} — HTTP replay skipped",
                    r.url, r.expect_status, r.timeout, r.method
                ));
            }
        }
    }
}

#[cfg(test)]
#[path = "replay_tests.rs"]
mod tests;
