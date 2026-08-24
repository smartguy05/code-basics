//! Socket-free tests for the two decisions replay makes without the network.
//!
//! The actual request/poll paths are deliberately untested (see the module
//! doc): a test that binds a port is flaky and does not belong in `cb-core`.

use super::*;

#[test]
fn record_from_parts_picks_content_type_case_insensitively() {
    let resp = record_from_parts(
        200,
        vec![
            ("X-Trace".into(), "abc".into()),
            (
                "Content-Type".into(),
                "application/json; charset=utf-8".into(),
            ),
        ],
        "{}".into(),
    );
    assert_eq!(resp.status, 200);
    assert_eq!(
        resp.content_type.as_deref(),
        Some("application/json; charset=utf-8")
    );
    assert_eq!(resp.body, "{}");
    assert_eq!(resp.headers.len(), 2);
}

#[test]
fn record_from_parts_has_no_content_type_when_absent() {
    let resp = record_from_parts(204, vec![], String::new());
    assert!(resp.content_type.is_none());
}

#[test]
fn ready_backoff_caps_at_the_deadline() {
    let now = Instant::now();
    let deadline = now + Duration::from_millis(100);
    // Interval longer than what remains → sleep only up to the deadline.
    let sleep = ready_backoff(now, deadline, Duration::from_secs(5)).unwrap();
    assert!(sleep <= Duration::from_millis(100));
}

#[test]
fn ready_backoff_uses_the_interval_when_it_fits() {
    let now = Instant::now();
    let deadline = now + Duration::from_secs(10);
    let sleep = ready_backoff(now, deadline, Duration::from_millis(500)).unwrap();
    assert_eq!(sleep, Duration::from_millis(500));
}

#[test]
fn ready_backoff_returns_none_past_the_deadline() {
    let now = Instant::now();
    let deadline = now - Duration::from_millis(1);
    assert!(ready_backoff(now, deadline, Duration::from_millis(500)).is_none());
}
