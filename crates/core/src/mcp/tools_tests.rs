use super::*;

use rmcp::model::ProtocolVersion;

fn args(pairs: &[(&str, Value)]) -> JsonObject {
    let mut map = Map::new();
    for (name, value) in pairs {
        map.insert((*name).to_string(), value.clone());
    }
    map
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

// ---------------------------------------------------------------------------
// The surface
// ---------------------------------------------------------------------------

#[test]
fn every_advertised_tool_can_be_called_and_every_callable_tool_is_advertised() {
    let advertised: Vec<String> = descriptors().iter().map(|t| t.name.to_string()).collect();
    let expected: Vec<String> = ALL.iter().map(|n| n.to_string()).collect();
    assert_eq!(advertised, expected);

    for name in ALL {
        // Called with no arguments at all, a real tool answers either with a
        // call or with an *invalid params* error — never method-not-found.
        match parse_call(name, None) {
            Ok(_) => {}
            Err(error) => assert_eq!(
                error.code,
                ErrorCode::INVALID_PARAMS,
                "{name} is advertised and not routed"
            ),
        }
    }
}

#[test]
fn the_two_deliberate_omissions_stay_omitted() {
    // Drift alarms for the module docs. An explain tool would be unsupported
    // for SQL Server while buying nothing, and a discover tool is a filesystem
    // scraper aimed at connection strings.
    for absent in ["sql.explain", "sql.discover"] {
        assert!(
            !ALL.contains(&absent),
            "{absent} was added; the module docs say why it is not here"
        );
        assert_eq!(
            parse_call(absent, None).unwrap_err().code,
            ErrorCode::METHOD_NOT_FOUND
        );
    }
}

#[test]
fn every_description_says_the_interface_is_read_only() {
    // A client may render a tool description without the server instructions,
    // and a model that read only the description must not think a write is one
    // argument away.
    for tool in descriptors() {
        let description = tool.description.expect("a description").to_string();
        assert!(
            description.contains("read-only"),
            "{} does not say it is read-only: {description}",
            tool.name
        );
    }
}

#[test]
fn every_schema_is_an_object_whose_required_fields_it_declares() {
    for tool in descriptors() {
        let schema = tool.input_schema.as_ref();
        assert_eq!(schema.get("type"), Some(&text("object")), "{}", tool.name);
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .expect("properties");
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .expect("required");
        for name in required {
            let name = name.as_str().expect("a field name");
            assert!(
                properties.contains_key(name),
                "{} requires {name} and does not declare it",
                tool.name
            );
        }
    }
}

#[test]
fn the_protocol_version_is_never_a_literal_in_this_tree() {
    // The SDK is dual-era and carries every spec revision; hardcoding one here
    // would pin this server to whichever era happened to be current the day it
    // was written.
    assert!(ProtocolVersion::KNOWN_VERSIONS.contains(&ProtocolVersion::LATEST));
    assert!(ProtocolVersion::KNOWN_VERSIONS.len() >= 2);
}

// ---------------------------------------------------------------------------
// Reading a call
// ---------------------------------------------------------------------------

#[test]
fn a_call_with_its_arguments_is_read() {
    assert_eq!(
        parse_call(LIST_CONNECTIONS, None).unwrap(),
        ToolCall::ListConnections
    );
    assert_eq!(
        parse_call(
            QUERY,
            Some(&args(&[
                ("connection", text("conn-1")),
                ("sql", text("SELECT 1")),
            ]))
        )
        .unwrap(),
        ToolCall::Query {
            connection: "conn-1".into(),
            sql: "SELECT 1".into()
        }
    );
    assert_eq!(
        parse_call(
            DESCRIBE_TABLE,
            Some(&args(&[
                ("connection", text("conn-1")),
                ("table", text("orders")),
                ("schema", text("sales")),
            ]))
        )
        .unwrap(),
        ToolCall::DescribeTable {
            connection: "conn-1".into(),
            schema: Some("sales".into()),
            table: "orders".into()
        }
    );
}

#[test]
fn an_omitted_schema_means_every_schema_and_an_empty_one_does_not() {
    let omitted = parse_call(
        DESCRIBE_TABLE,
        Some(&args(&[
            ("connection", text("conn-1")),
            ("table", text("orders")),
        ])),
    )
    .unwrap();
    let null = parse_call(
        DESCRIBE_TABLE,
        Some(&args(&[
            ("connection", text("conn-1")),
            ("table", text("orders")),
            ("schema", Value::Null),
        ])),
    )
    .unwrap();
    let empty = parse_call(
        DESCRIBE_TABLE,
        Some(&args(&[
            ("connection", text("conn-1")),
            ("table", text("orders")),
            ("schema", text("")),
        ])),
    )
    .unwrap();

    assert_eq!(omitted, null);
    assert_ne!(
        omitted, empty,
        "an empty schema must not be read as no schema; it is refused downstream"
    );
    match empty {
        ToolCall::DescribeTable { schema, .. } => assert_eq!(schema, Some(String::new())),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_unknown_tool_is_a_protocol_error_and_names_the_ones_that_exist() {
    let error = parse_call("sql.drop_everything", None).unwrap_err();
    assert_eq!(error.code, ErrorCode::METHOD_NOT_FOUND);
    assert!(error.message.contains(QUERY), "{}", error.message);
}

#[test]
fn a_missing_argument_is_a_protocol_error_that_names_the_field() {
    let error = parse_call(QUERY, Some(&args(&[("connection", text("conn-1"))]))).unwrap_err();
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert!(error.message.contains("sql"), "{}", error.message);
}

#[test]
fn an_argument_of_the_wrong_type_is_a_protocol_error_that_says_what_it_was() {
    let error = parse_call(
        QUERY,
        Some(&args(&[
            ("connection", text("conn-1")),
            ("sql", Value::Number(7.into())),
        ])),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert!(error.message.contains("a number"), "{}", error.message);

    let bad_schema = parse_call(
        DESCRIBE_TABLE,
        Some(&args(&[
            ("connection", text("conn-1")),
            ("table", text("orders")),
            ("schema", Value::Bool(true)),
        ])),
    )
    .unwrap_err();
    assert_eq!(bad_schema.code, ErrorCode::INVALID_PARAMS);
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

#[test]
fn a_refusal_is_a_tool_error_and_not_a_protocol_error() {
    // The spec says a client SHOULD hand tool execution errors to the model so
    // it can correct itself; a protocol error is usually rendered opaquely and
    // the words never reach it.
    let refusal = McpRefusal::ConnectionNotExposed {
        selector: "billing".into(),
    };
    let result = refusal_result(&refusal);
    assert_eq!(result.is_error, Some(true));
    let rendered = format!("{:?}", result.content);
    assert!(rendered.contains("connectionNotExposed"), "{rendered}");
    assert!(rendered.contains("billing"), "{rendered}");
    assert_eq!(
        result.structured_content,
        Some(json!({ "code": "connectionNotExposed" }))
    );
}

#[test]
fn a_successful_answer_is_not_an_error() {
    let result = text_result("2 rows");
    assert_eq!(result.is_error, Some(false));
}
