use super::*;

// ---------------------------------------------------------------------------
// Protocol version negotiation
// ---------------------------------------------------------------------------

#[test]
fn a_protocol_version_this_build_knows_is_echoed_rather_than_replaced() {
    for known in ProtocolVersion::KNOWN_VERSIONS {
        assert_eq!(negotiated_version(Some(known.as_str())), known.clone());
    }
}

#[test]
fn an_unknown_protocol_version_is_answered_with_this_builds_latest_not_refused() {
    assert_eq!(
        negotiated_version(Some("1999-01-01")),
        ProtocolVersion::LATEST
    );
    assert_eq!(negotiated_version(None), ProtocolVersion::LATEST);
}

#[test]
fn initialize_names_the_server_and_declares_the_tools_capability() {
    let result = initialize_result(None);
    assert_eq!(result.server_info.name, SERVER_NAME);
    assert_eq!(result.server_info.version, SERVER_VERSION);
    assert!(result.capabilities.tools.is_some());
    let instructions = result.instructions.expect("instructions");
    assert!(instructions.contains("read and write"));
    assert!(instructions.contains("gitignored"));
}

#[test]
fn the_server_name_is_not_the_sql_server_name() {
    assert_ne!(SERVER_NAME, crate::mcp::serve::SERVER_NAME);
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

#[test]
fn the_four_methods_route_and_nothing_else_does() {
    assert!(matches!(
        route(INITIALIZE, None),
        Ok(Route::Initialize { .. })
    ));
    assert!(matches!(route(TOOLS_LIST, None), Ok(Route::ToolsList)));
    assert!(matches!(route(PING, None), Ok(Route::Ping)));
    let error = route("resources/list", None).expect_err("no resources on this server");
    assert_eq!(error.code, ErrorCode::METHOD_NOT_FOUND);
}

#[test]
fn a_tools_call_without_a_name_is_a_protocol_error() {
    let error = route(TOOLS_CALL, Some(&json!({ "arguments": {} }))).unwrap_err();
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
}

#[test]
fn absent_arguments_are_not_invented_but_still_route() {
    let none = route(TOOLS_CALL, Some(&json!({ "name": "list_tasks" }))).unwrap();
    match none {
        Route::ToolsCall { name, arguments } => {
            assert_eq!(name, "list_tasks");
            assert_eq!(arguments, None);
        }
        other => panic!("{other:?}"),
    }
    let empty = route(
        TOOLS_CALL,
        Some(&json!({ "name": "list_tasks", "arguments": {} })),
    )
    .unwrap();
    match empty {
        Route::ToolsCall { arguments, .. } => {
            assert_eq!(arguments, Some(JsonObject::new()));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_clients_requested_version_reaches_the_initialize_route() {
    let params = json!({ "protocolVersion": "2024-11-05" });
    match route(INITIALIZE, Some(&params)) {
        Ok(Route::Initialize { requested }) => assert_eq!(requested.as_deref(), Some("2024-11-05")),
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Envelopes
// ---------------------------------------------------------------------------

#[test]
fn a_success_echoes_the_id_in_its_shape_and_carries_no_error() {
    let numeric = success(&RequestId::Num(7), json!({}));
    assert_eq!(numeric["id"], json!(7));
    assert!(numeric.get("error").is_none());

    let string = success(&RequestId::Str("a".into()), json!({}));
    assert_eq!(string["id"], json!("a"));
}

#[test]
fn a_failure_carries_error_and_no_result_key() {
    let response = failure(&RequestId::Num(1), &ErrorData::invalid_params("bad", None));
    assert!(response.get("result").is_none());
    assert!(response.get("error").is_some());
}

#[test]
fn a_parse_failure_has_a_null_id() {
    let response = parse_failure("expected value at line 1");
    assert_eq!(response["id"], Value::Null);
}
