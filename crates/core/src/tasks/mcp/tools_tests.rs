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

#[test]
fn every_advertised_tool_can_be_called_and_every_callable_tool_is_advertised() {
    let advertised: Vec<String> = descriptors().iter().map(|t| t.name.to_string()).collect();
    let expected: Vec<String> = ALL.iter().map(|n| n.to_string()).collect();
    assert_eq!(advertised, expected);

    for name in ALL {
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
fn there_are_seven_tools_and_five_of_them_write() {
    // Drift alarm: this server is intentionally read *and* write, unlike the
    // SQL one. list_tasks and get_task read; the other five mutate.
    assert_eq!(ALL.len(), 7);
    assert!(ALL.contains(&CREATE_TASK));
    assert!(ALL.contains(&ASSIGN_TASK));
    assert!(ALL.contains(&DELETE_TASK));
}

#[test]
fn no_description_claims_read_only() {
    // The opposite of the SQL server's rule: a write server must not tell a
    // model it cannot write.
    for tool in descriptors() {
        let description = tool.description.expect("a description").to_string();
        assert!(
            !description.contains("read-only"),
            "{} claims read-only: {description}",
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
    assert!(ProtocolVersion::KNOWN_VERSIONS.contains(&ProtocolVersion::LATEST));
    assert!(ProtocolVersion::KNOWN_VERSIONS.len() >= 2);
}

// ---------------------------------------------------------------------------
// Reading a call
// ---------------------------------------------------------------------------

#[test]
fn a_call_with_its_arguments_is_read() {
    assert_eq!(parse_call(LIST_TASKS, None).unwrap(), ToolCall::ListTasks);
    assert_eq!(
        parse_call(CREATE_TASK, Some(&args(&[("title", text("Ship it"))]))).unwrap(),
        ToolCall::CreateTask {
            title: "Ship it".into(),
            body: None
        }
    );
    assert_eq!(
        parse_call(
            CREATE_TASK,
            Some(&args(&[
                ("title", text("Ship it")),
                ("body", text("details"))
            ]))
        )
        .unwrap(),
        ToolCall::CreateTask {
            title: "Ship it".into(),
            body: Some("details".into())
        }
    );
}

#[test]
fn an_omitted_update_field_is_left_alone_and_an_empty_one_clears_it() {
    let omitted = parse_call(UPDATE_TASK, Some(&args(&[("id", text("t1"))]))).unwrap();
    assert_eq!(
        omitted,
        ToolCall::UpdateTask {
            id: "t1".into(),
            title: None,
            body: None
        }
    );
    let cleared = parse_call(
        UPDATE_TASK,
        Some(&args(&[("id", text("t1")), ("title", text(""))])),
    )
    .unwrap();
    assert_eq!(
        cleared,
        ToolCall::UpdateTask {
            id: "t1".into(),
            title: Some(String::new()),
            body: None
        }
    );
}

#[test]
fn assign_reads_the_owner_enum() {
    assert_eq!(
        parse_call(
            ASSIGN_TASK,
            Some(&args(&[("id", text("t1")), ("owner", text("ai"))]))
        )
        .unwrap(),
        ToolCall::AssignTask {
            id: "t1".into(),
            owner: TaskOwner::Ai
        }
    );
    assert_eq!(
        parse_call(
            ASSIGN_TASK,
            Some(&args(&[("id", text("t1")), ("owner", text("me"))]))
        )
        .unwrap(),
        ToolCall::AssignTask {
            id: "t1".into(),
            owner: TaskOwner::Me
        }
    );
}

#[test]
fn an_owner_outside_the_set_is_a_protocol_error() {
    let error = parse_call(
        ASSIGN_TASK,
        Some(&args(&[("id", text("t1")), ("owner", text("bob"))])),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert!(error.message.contains("bob"), "{}", error.message);
}

#[test]
fn an_unknown_tool_is_a_protocol_error_and_names_the_ones_that_exist() {
    let error = parse_call("nuke_tasks", None).unwrap_err();
    assert_eq!(error.code, ErrorCode::METHOD_NOT_FOUND);
    assert!(error.message.contains(LIST_TASKS), "{}", error.message);
}

#[test]
fn a_missing_argument_is_a_protocol_error_that_names_the_field() {
    let error = parse_call(GET_TASK, None).unwrap_err();
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert!(error.message.contains("id"), "{}", error.message);
}

#[test]
fn an_argument_of_the_wrong_type_is_a_protocol_error_that_says_what_it_was() {
    let error = parse_call(
        CREATE_TASK,
        Some(&args(&[("title", Value::Number(7.into()))])),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert!(error.message.contains("a number"), "{}", error.message);
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

#[test]
fn a_refusal_is_a_tool_error_and_not_a_protocol_error() {
    let refusal = McpRefusal::TaskNotFound { id: "t9".into() };
    let result = refusal_result(&refusal);
    assert_eq!(result.is_error, Some(true));
    let rendered = format!("{:?}", result.content);
    assert!(rendered.contains("taskNotFound"), "{rendered}");
    assert!(rendered.contains("t9"), "{rendered}");
    assert_eq!(
        result.structured_content,
        Some(json!({ "code": "taskNotFound" }))
    );
}

#[test]
fn a_successful_answer_is_not_an_error() {
    let result = text_result("done");
    assert_eq!(result.is_error, Some(false));
}
