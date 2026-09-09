use super::*;

use std::path::PathBuf;

use crate::sql::catalog::SqlObjectKind;
use crate::sql::dsn::SqlEngine;
use crate::sql::model::{RowCap, RowCapReason, SqlColumn as ResultColumn, SqlValue};
use crate::sql::store::{SecretSource, SqlConnection, SqlConnectionsFile};

fn profile(id: &str, name: &str, exposed: bool) -> SqlConnection {
    SqlConnection {
        id: id.into(),
        name: name.into(),
        engine: Some(SqlEngine::Postgres),
        secret: SecretSource::AppSettings {
            path: PathBuf::from("C:/code/shop/appsettings.json"),
            key: "ConnectionStrings:Default".into(),
        },
        workspace_root: Some(PathBuf::from("C:/code/shop")),
        allow_writes: false,
        expose_to_agents: exposed,
        user_named: true,
        created_at_ms: 1_000,
        last_used_ms: None,
    }
}

fn file(connections: Vec<SqlConnection>) -> SqlConnectionsFile {
    SqlConnectionsFile {
        version: 1,
        connections,
    }
}

fn rows(columns: &[&str], data: Vec<Vec<SqlValue>>, cap: Option<RowCap>) -> SqlResultSet {
    SqlResultSet {
        columns: columns
            .iter()
            .map(|name| ResultColumn {
                name: (*name).to_string(),
                type_name: None,
            })
            .collect(),
        rows: data,
        row_cap: cap,
        rows_affected: None,
        elapsed_ms: 0,
        statement_index: 0,
    }
}

fn text(value: &str) -> SqlValue {
    SqlValue::Text {
        text: value.to_string(),
        truncated: false,
    }
}

// ---------------------------------------------------------------------------
// Protocol version negotiation
// ---------------------------------------------------------------------------

#[test]
fn a_protocol_version_this_build_knows_is_echoed_rather_than_replaced() {
    for known in ProtocolVersion::KNOWN_VERSIONS {
        assert_eq!(
            negotiated_version(Some(known.as_str())),
            known.clone(),
            "a version the SDK knows must be answered with itself"
        );
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
fn the_answered_version_is_never_a_literal_this_file_wrote() {
    // The drift alarm: this build's default has to come from the SDK's own
    // constant, so a spec release moves it without an edit here.
    let value = serde_json::to_value(negotiated_version(None)).unwrap();
    assert_eq!(value, serde_json::json!(ProtocolVersion::LATEST.as_str()));
}

// ---------------------------------------------------------------------------
// initialize
// ---------------------------------------------------------------------------

#[test]
fn initialize_declares_tools_and_declares_nothing_it_does_not_have() {
    let result = initialize_result(Some(ProtocolVersion::V_2025_06_18.as_str()));
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(value["protocolVersion"], "2025-06-18");
    assert!(
        value["capabilities"]["tools"].is_object(),
        "the server has tools and must say so: {value}"
    );
    for absent in ["resources", "prompts", "logging", "completions"] {
        assert!(
            value["capabilities"].get(absent).is_none(),
            "{absent} is not implemented and must not be advertised: {value}"
        );
    }
}

#[test]
fn initialize_names_this_app_and_states_the_read_only_rule_up_front() {
    let result = initialize_result(None);
    assert_eq!(result.server_info.name, SERVER_NAME);
    assert_eq!(result.server_info.version, SERVER_VERSION);
    let instructions = result.instructions.expect("instructions must be present");
    assert!(
        instructions.contains("read-only"),
        "a client that reads only the instructions must learn the one rule: {instructions}"
    );
    assert!(
        instructions.contains("exposed"),
        "and that exposure is per connection and off by default: {instructions}"
    );
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

#[test]
fn every_advertised_tool_can_be_reached_by_name() {
    for name in tools::ALL {
        let params = serde_json::json!({ "name": name, "arguments": {} });
        match route(TOOLS_CALL, Some(&params)) {
            Ok(Route::ToolsCall { name: routed, .. }) => assert_eq!(routed, name),
            other => panic!("{name} did not route to a tool call: {other:?}"),
        }
    }
}

#[test]
fn a_tool_call_with_no_name_is_invalid_params_rather_than_an_unknown_tool() {
    let error = route(TOOLS_CALL, Some(&serde_json::json!({ "arguments": {} })))
        .expect_err("a call with no tool name cannot be routed");
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
}

#[test]
fn absent_arguments_are_not_the_same_as_empty_ones_and_both_route() {
    let none = route(
        TOOLS_CALL,
        Some(&serde_json::json!({ "name": tools::LIST_CONNECTIONS })),
    );
    assert!(
        matches!(
            none,
            Ok(Route::ToolsCall {
                arguments: None,
                ..
            })
        ),
        "an absent arguments key must stay absent: {none:?}"
    );
    let empty = route(
        TOOLS_CALL,
        Some(&serde_json::json!({ "name": tools::LIST_CONNECTIONS, "arguments": {} })),
    );
    assert!(
        matches!(
            empty,
            Ok(Route::ToolsCall {
                arguments: Some(_),
                ..
            })
        ),
        "an empty object is an argument set, not a missing one: {empty:?}"
    );
}

#[test]
fn the_three_other_methods_route_and_anything_else_is_method_not_found() {
    assert!(matches!(
        route("initialize", None),
        Ok(Route::Initialize { .. })
    ));
    assert!(matches!(route("tools/list", None), Ok(Route::ToolsList)));
    assert!(matches!(route("ping", None), Ok(Route::Ping)));

    let error = route("resources/list", None).expect_err("this server has no resources");
    assert_eq!(error.code, ErrorCode::METHOD_NOT_FOUND);
    assert!(
        error.message.contains("resources/list"),
        "the refusal must name what was asked for: {}",
        error.message
    );
}

#[test]
fn the_clients_requested_version_reaches_the_initialize_route() {
    let params = serde_json::json!({ "protocolVersion": "2024-11-05" });
    match route("initialize", Some(&params)) {
        Ok(Route::Initialize { requested }) => assert_eq!(requested.as_deref(), Some("2024-11-05")),
        other => panic!("initialize did not carry the requested version: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Envelopes
// ---------------------------------------------------------------------------

#[test]
fn a_response_echoes_the_request_id_in_the_shape_it_arrived() {
    let numeric = success(&RequestId::Num(7), serde_json::json!({}));
    assert_eq!(numeric["id"], serde_json::json!(7));
    assert_eq!(numeric["jsonrpc"], "2.0");

    let string = success(&RequestId::Str("a".into()), serde_json::json!({}));
    assert_eq!(
        string["id"],
        serde_json::json!("a"),
        "a string id must not come back as a number"
    );

    let null = failure(
        &RequestId::Null,
        &ErrorData::new(ErrorCode::INVALID_REQUEST, "no", None),
    );
    assert!(null["id"].is_null());
    assert_eq!(null["error"]["code"], -32600);
    assert!(
        null.get("result").is_none(),
        "a failure carries no result key"
    );
}

#[test]
fn a_line_that_was_not_json_is_answered_with_a_null_id_parse_error() {
    let response = parse_failure("expected value at line 1");
    assert!(response["id"].is_null());
    assert_eq!(response["error"]["code"], -32700);
}

// ---------------------------------------------------------------------------
// sql.list_connections
// ---------------------------------------------------------------------------

#[test]
fn an_empty_listing_says_which_of_the_two_reasons_it_is() {
    let nothing_saved = list_connections_answer(&file(vec![]), None)
        .expect_err("an empty listing is a refusal, not an empty page");
    assert_eq!(nothing_saved, McpRefusal::NoConnectionsSaved);

    let nothing_exposed = list_connections_answer(
        &file(vec![profile("a", "A", false), profile("b", "B", false)]),
        None,
    )
    .expect_err("saved but unexposed is still a refusal");
    assert_eq!(
        nothing_exposed,
        McpRefusal::NoConnectionExposed { total_saved: 2 }
    );
}

#[test]
fn a_scope_that_left_every_exposed_connection_out_is_told_rather_than_called_nothing_exposed() {
    // The bug this pins: reporting `noConnectionExposed` here would tell the
    // reader to flip a toggle that is already on, when the real answer is that
    // this server was started with a scope.
    let answer = list_connections_answer(
        &file(vec![profile("a", "A", true)]),
        Some(&PathBuf::from("C:/code/other")),
    )
    .expect("a scope is not an absence of consent");
    assert!(
        answer.contains('1') && answer.contains("scope"),
        "the scoped-out count and the reason must both be said: {answer}"
    );
}

#[test]
fn a_listing_with_connections_renders_them() {
    let answer = list_connections_answer(&file(vec![profile("a", "Shop", true)]), None)
        .expect("an exposed connection is an answer");
    assert!(answer.contains("Shop"), "{answer}");
}

// ---------------------------------------------------------------------------
// Catalog listings
// ---------------------------------------------------------------------------

#[test]
fn a_capped_catalog_listing_is_refused_rather_than_returned_as_a_short_one() {
    let capped = rows(
        &["schema", "name"],
        vec![vec![text("dbo"), text("orders")]],
        Some(RowCap {
            limit: 1,
            reason: RowCapReason::RowLimit,
        }),
    );
    assert_eq!(
        catalog_objects(&capped),
        Err(McpRefusal::CatalogTruncated {
            cap: RowCap {
                limit: 1,
                reason: RowCapReason::RowLimit
            }
        })
    );
    assert!(
        matches!(
            catalog_columns(&capped),
            Err(McpRefusal::CatalogTruncated { .. })
        ),
        "the column listing has the same rule"
    );
}

#[test]
fn a_catalog_row_this_build_cannot_read_is_refused_rather_than_skipped() {
    let wrong_shape = rows(&["nothing"], vec![vec![text("dbo")]], None);
    let refusal = catalog_objects(&wrong_shape)
        .expect_err("a row that is not the expected shape is not a table");
    assert!(
        matches!(refusal, McpRefusal::CatalogRowUnreadable { .. }),
        "got {refusal:?}"
    );
}

#[test]
fn a_complete_catalog_listing_is_returned() {
    let listing = rows(
        &["schema", "name"],
        vec![vec![text("dbo"), text("orders")]],
        None,
    );
    let objects = catalog_objects(&listing).expect("a complete listing is an answer");
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].name, "orders");
    assert_eq!(objects[0].kind, SqlObjectKind::Table);
}
