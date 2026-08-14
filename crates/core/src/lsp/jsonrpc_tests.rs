use super::*;

#[test]
fn a_response_carrying_a_result_is_classified_as_one() {
    match classify(br#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#).expect("valid") {
        Incoming::Response { id, result, error } => {
            assert_eq!(RequestId::Num(1), id);
            assert_eq!(Some(serde_json::json!({"ok": true})), result);
            assert!(error.is_none());
        }
        other => panic!("expected a response, got {other:?}"),
    }
}

#[test]
fn a_null_result_is_a_present_answer_and_not_a_missing_one() {
    // `textDocument/definition` answers `null` for "there is no definition".
    // Treating that as an absent field would turn a real answer into a
    // protocol error, and the UI would report a failure where the truth is
    // "nothing to go to".
    match classify(br#"{"jsonrpc":"2.0","id":7,"result":null}"#).expect("valid") {
        Incoming::Response { result, error, .. } => {
            assert_eq!(Some(serde_json::Value::Null), result);
            assert!(error.is_none());
        }
        other => panic!("expected a response, got {other:?}"),
    }
}

#[test]
fn a_response_carrying_an_error_keeps_its_code_and_message() {
    match classify(br#"{"jsonrpc":"2.0","id":"a","error":{"code":-32601,"message":"nope"}}"#)
        .expect("valid")
    {
        Incoming::Response { id, error, .. } => {
            assert_eq!(RequestId::Str("a".into()), id);
            let error = error.expect("an error object");
            assert_eq!(METHOD_NOT_FOUND, error.code);
            assert_eq!("nope", error.message);
        }
        other => panic!("expected a response, got {other:?}"),
    }
}

#[test]
fn a_response_with_both_a_result_and_an_error_is_read_as_the_error() {
    // Illegal per JSON-RPC, and shipped anyway. Reading the result would mean
    // acting on a payload the server has just said is not valid.
    match classify(br#"{"jsonrpc":"2.0","id":1,"result":[],"error":{"code":-1,"message":"x"}}"#)
        .expect("valid")
    {
        Incoming::Response { error, .. } => assert!(error.is_some(), "the error must win"),
        other => panic!("expected a response, got {other:?}"),
    }
}

#[test]
fn a_request_from_the_server_keeps_its_id_so_it_can_be_answered() {
    // The id is the whole point: an unanswered server request hangs the server
    // permanently, and the hang is indistinguishable from a slow project load.
    match classify(
        br#"{"jsonrpc":"2.0","id":3,"method":"workspace/configuration","params":{"items":[{}]}}"#,
    )
    .expect("valid")
    {
        Incoming::Request { id, method, params } => {
            assert_eq!(RequestId::Num(3), id);
            assert_eq!("workspace/configuration", method);
            assert!(params.is_some());
        }
        other => panic!("expected a request, got {other:?}"),
    }
}

#[test]
fn a_notification_is_told_apart_from_a_request_by_having_no_id() {
    match classify(br#"{"jsonrpc":"2.0","method":"window/logMessage","params":{"type":3}}"#)
        .expect("valid")
    {
        Incoming::Notification { method, .. } => assert_eq!("window/logMessage", method),
        other => panic!("expected a notification, got {other:?}"),
    }
}

#[test]
fn a_request_with_a_null_id_is_a_request_and_not_a_notification() {
    // `null` is a legal (if discouraged) request id, and it is *present*. Read
    // as a notification it would never be answered, which hangs the server.
    match classify(br#"{"jsonrpc":"2.0","id":null,"method":"x"}"#).expect("valid") {
        Incoming::Request { id, .. } => assert_eq!(RequestId::Null, id),
        other => panic!("expected a request, got {other:?}"),
    }
}

#[test]
fn a_string_id_and_a_numeric_id_are_not_the_same_id() {
    // They correlate different requests. Comparing them by their printed form
    // would let one response resolve another request's waiter.
    assert_ne!(RequestId::Num(1), RequestId::Str("1".into()));
}

#[test]
fn an_id_round_trips_through_json_unchanged() {
    for id in [
        RequestId::Num(0),
        RequestId::Num(-1),
        RequestId::Num(i64::MAX),
        RequestId::Str("abc".into()),
        RequestId::Null,
    ] {
        let encoded = serde_json::to_value(&id).expect("serialise");
        let decoded: RequestId = serde_json::from_value(encoded).expect("deserialise");
        assert_eq!(id, decoded);
    }
}

#[test]
fn a_message_that_is_neither_shape_is_refused() {
    for raw in [
        &br#"{"jsonrpc":"2.0"}"#[..],
        &br#"{"jsonrpc":"2.0","id":1}"#[..],
        &br#"{}"#[..],
    ] {
        classify(raw).expect_err("no method and no result or error is not a message");
    }
}

#[test]
fn malformed_json_is_refused_rather_than_guessed_at() {
    for raw in [&b"{"[..], &b""[..], &b"not json"[..], &b"[1,2,3]"[..]] {
        classify(raw).expect_err("unparseable");
    }
}

#[test]
fn an_outgoing_request_carries_the_version_the_method_and_the_id() {
    let encoded = serde_json::to_value(Outgoing::request(
        RequestId::Num(4),
        "textDocument/references",
        serde_json::json!({"a": 1}),
    ))
    .expect("serialise");
    assert_eq!(
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "textDocument/references",
            "params": {"a": 1},
        }),
        encoded
    );
}

#[test]
fn an_outgoing_notification_carries_no_id_field_at_all() {
    // An `"id": null` would make the server believe a response is expected.
    let encoded =
        serde_json::to_value(Outgoing::notification("initialized", serde_json::json!({})))
            .expect("serialise");
    assert_eq!(
        serde_json::json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
        encoded
    );
    assert!(
        encoded.get("id").is_none(),
        "a notification must have no id key: {encoded}"
    );
}

#[test]
fn an_outgoing_reply_carries_a_result_even_when_it_is_null() {
    // `workspace/configuration` is answered with nulls, and the key must be
    // present: a reply with neither `result` nor `error` is not a reply.
    let encoded = serde_json::to_value(Outgoing::reply(RequestId::Num(2), serde_json::Value::Null))
        .expect("serialise");
    assert_eq!(
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "result": null}),
        encoded
    );
}

#[test]
fn an_outgoing_error_reply_carries_the_code_and_no_result() {
    let encoded = serde_json::to_value(Outgoing::error_reply(
        RequestId::Num(9),
        METHOD_NOT_FOUND,
        "unsupported",
    ))
    .expect("serialise");
    assert_eq!(
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 9,
            "error": {"code": -32601, "message": "unsupported"},
        }),
        encoded
    );
}
