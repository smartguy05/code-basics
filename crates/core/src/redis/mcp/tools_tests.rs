use super::*;

fn args(v: Value) -> JsonObject {
    v.as_object().unwrap().clone()
}

#[test]
fn every_tool_parses() {
    for name in ALL {
        assert!(is_known(name));
    }
    assert!(!is_known("redis.nope"));
}

#[test]
fn write_tools_are_flagged_and_reads_are_not() {
    assert!(is_write(SET_STRING));
    assert!(is_write(STREAM_ADD));
    assert!(is_write(DELETE_KEY));
    assert!(!is_write(GET_KEY));
    assert!(!is_write(SCAN_KEYS));
    assert!(!is_write(LIST_CONNECTIONS));
}

#[test]
fn scan_reads_optional_fields() {
    let call = parse_call(
        SCAN_KEYS,
        Some(&args(
            json!({ "connection": "c", "match": "u:*", "count": 50 }),
        )),
    )
    .unwrap();
    assert_eq!(
        call,
        ToolCall::ScanKeys {
            connection: "c".into(),
            pattern: Some("u:*".into()),
            cursor: None,
            count: Some(50),
        }
    );
}

#[test]
fn stream_add_reads_a_field_map() {
    let call = parse_call(
        STREAM_ADD,
        Some(&args(
            json!({ "connection": "c", "key": "s", "fields": { "a": "1", "b": "2" } }),
        )),
    )
    .unwrap();
    match call {
        ToolCall::StreamAdd { fields, id, .. } => {
            assert_eq!(id, None);
            assert!(fields.contains(&("a".to_string(), "1".to_string())));
            assert!(fields.contains(&("b".to_string(), "2".to_string())));
        }
        other => panic!("wrong call: {other:?}"),
    }
}

#[test]
fn a_non_string_field_value_is_a_shape_error() {
    let err = parse_call(
        STREAM_ADD,
        Some(&args(
            json!({ "connection": "c", "key": "s", "fields": { "a": 1 } }),
        )),
    );
    assert!(err.is_err());
}

#[test]
fn zset_add_requires_a_numeric_score() {
    assert!(parse_call(
        ZSET_ADD,
        Some(&args(
            json!({ "connection": "c", "key": "z", "member": "m", "score": 1.5 })
        ))
    )
    .is_ok());
    assert!(parse_call(
        ZSET_ADD,
        Some(&args(
            json!({ "connection": "c", "key": "z", "member": "m", "score": "high" })
        ))
    )
    .is_err());
}

#[test]
fn an_unknown_tool_is_a_method_not_found() {
    let err = parse_call("redis.nope", None).unwrap_err();
    assert_eq!(err.code, ErrorCode::METHOD_NOT_FOUND);
}
