use super::*;

fn resp(status: u16, headers: &[(&str, &str)], body: &str, ct: Option<&str>) -> RecordedResponse {
    RecordedResponse {
        status,
        headers: headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        body: body.to_string(),
        content_type: ct.map(String::from),
    }
}

fn json(status: u16, body: &str) -> RecordedResponse {
    resp(status, &[("Content-Type", "application/json")], body, Some("application/json"))
}

#[test]
fn identical_responses_no_change() {
    let r = json(200, r#"{"a":1}"#);
    let d = diff_http("get", &r, &r, &[]);
    assert!(!d.is_change());
}

#[test]
fn status_change_is_high_confidence() {
    let before = json(200, r#"{"ok":true}"#);
    let after = json(500, r#"{"ok":true}"#);
    let d = diff_http("get", &before, &after, &[]);
    assert_eq!(d.status, Some((200, 500)));
    assert_eq!(d.confidence, Confidence::High);
    assert!(d.is_change());
}

#[test]
fn volatile_date_header_is_ignored() {
    let before = json(200, r#"{"a":1}"#);
    let mut after = json(200, r#"{"a":1}"#);
    // A Date header differing is pure noise.
    after.headers.push(("Date".into(), "Tue, 01 Jan 2030".into()));
    let d = diff_http("get", &before, &after, &[]);
    assert!(!d.is_change(), "Date change must not register: {d:?}");
}

#[test]
fn non_volatile_header_change_is_reported() {
    let before = json(200, r#"{"a":1}"#);
    let mut after = json(200, r#"{"a":1}"#);
    after.headers.push(("X-Feature".into(), "on".into()));
    let d = diff_http("get", &before, &after, &[]);
    assert_eq!(d.header_changes.len(), 1);
    assert_eq!(d.header_changes[0].name, "x-feature");
    assert_eq!(d.header_changes[0].after.as_deref(), Some("on"));
    assert_eq!(d.confidence, Confidence::Medium);
}

#[test]
fn json_key_order_is_not_a_change() {
    let before = json(200, r#"{"a":1,"b":2}"#);
    let after = json(200, r#"{"b":2,"a":1}"#);
    let d = diff_http("get", &before, &after, &[]);
    assert!(!d.is_change(), "key reordering is not behavioral: {d:?}");
}

#[test]
fn json_value_change_is_reported() {
    let before = json(200, r#"{"count":1}"#);
    let after = json(200, r#"{"count":2}"#);
    let d = diff_http("get", &before, &after, &[]);
    assert!(d.body.is_some());
    assert_eq!(d.confidence, Confidence::Medium);
}

#[test]
fn timestamps_in_json_body_are_masked() {
    let before = json(200, r#"{"at":"2026-01-01T00:00:00","v":1}"#);
    let after = json(200, r#"{"at":"2026-09-09T12:00:00","v":1}"#);
    let d = diff_http("get", &before, &after, &[]);
    assert!(!d.is_change(), "only a timestamp differs — masked away: {d:?}");
}

#[test]
fn unparseable_json_body_drops_to_low() {
    // Declared JSON but not valid JSON on one side.
    let before = json(200, r#"{"a":1}"#);
    let after = json(200, "not json at all");
    let d = diff_http("get", &before, &after, &[]);
    assert!(d.body.is_some());
    assert_eq!(d.confidence, Confidence::Low);
}

#[test]
fn caller_ignore_list_extends_volatile() {
    let before = json(200, r#"{"a":1}"#);
    let mut after = json(200, r#"{"a":1}"#);
    after.headers.push(("X-Trace".into(), "abc".into()));
    // Without the ignore it would register; with it, silence.
    assert!(diff_http("g", &before, &after, &[]).is_change());
    assert!(!diff_http("g", &before, &after, &["x-trace"]).is_change());
}
