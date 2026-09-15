use super::*;

use serde_json::json;

fn args(pairs: &[(&str, Value)]) -> JsonObject {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert((*key).to_string(), value.clone());
    }
    map
}

// ---------------------------------------------------------------------------
// Advertised and reachable are the same set
// ---------------------------------------------------------------------------

#[test]
fn every_advertised_tool_parses_and_every_parsed_tool_is_advertised() {
    let advertised: Vec<String> = descriptors()
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();
    assert_eq!(
        advertised,
        ALL.iter().map(|n| n.to_string()).collect::<Vec<_>>()
    );
    assert_eq!(ALL.len(), 4);
}

#[test]
fn every_tool_in_all_parses_with_no_arguments() {
    for name in ALL {
        let call = parse_call(name, None)
            .unwrap_or_else(|error| panic!("{name} did not parse: {error:?}"));
        assert_eq!(call.name(), name);
    }
}

#[test]
fn the_four_tools_map_to_the_four_calls() {
    assert_eq!(
        parse_call(GET_ACTIVE_FILE, None).unwrap(),
        EditorToolCall::GetActiveFile
    );
    assert_eq!(
        parse_call(GET_SELECTION, None).unwrap(),
        EditorToolCall::GetSelection
    );
    assert_eq!(
        parse_call(GET_OPEN_FILES, None).unwrap(),
        EditorToolCall::GetOpenFiles
    );
    assert_eq!(
        parse_call(GET_RECENT_FILES, None).unwrap(),
        EditorToolCall::GetRecentFiles
    );
}

#[test]
fn an_unknown_tool_lists_what_there_is_and_names_the_missing_one() {
    let error = parse_call("get_symbol_info", None).unwrap_err();
    assert_eq!(error.code, ErrorCode::METHOD_NOT_FOUND);
    assert!(
        error.message.contains("get_symbol_info"),
        "{}",
        error.message
    );
    assert!(error.message.contains(GET_ACTIVE_FILE), "{}", error.message);
}

#[test]
fn is_known_admits_exactly_the_advertised_tools() {
    for tool in ALL {
        assert!(is_known(tool), "{tool}");
    }
    assert!(!is_known("get_symbol_info"));
    assert!(!is_known(""));
    assert!(!is_known("get_active_file "));
}

#[test]
fn the_shim_and_the_application_refuse_an_unknown_tool_in_the_same_words() {
    let from_application =
        parse_call("get_symbol_info", None).expect_err("an unadvertised tool is refused");
    assert_eq!(
        from_application.message,
        unknown_tool_sentence("get_symbol_info")
    );
}

// ---------------------------------------------------------------------------
// The tools take no arguments; spurious ones are ignored, not refused
// ---------------------------------------------------------------------------

#[test]
fn spurious_arguments_are_ignored_rather_than_refused() {
    // The tools take none; inventing a shape error over arguments a tool ignores
    // would be a lie about what it needs.
    for name in ALL {
        let call = parse_call(name, Some(&args(&[("path", json!("src/App.cs"))])))
            .unwrap_or_else(|error| panic!("{name} refused ignorable args: {error:?}"));
        assert_eq!(call.name(), name);
    }
}

// ---------------------------------------------------------------------------
// The schemas declare no arguments
// ---------------------------------------------------------------------------

#[test]
fn every_schema_requires_nothing_and_declares_no_properties() {
    for tool in descriptors() {
        let name = tool.name.to_string();
        let schema = serde_json::to_value(tool.input_schema).unwrap();
        let required = schema["required"].as_array().unwrap();
        assert!(required.is_empty(), "{name}: {schema}");
        let props = schema["properties"].as_object().unwrap();
        assert!(props.is_empty(), "{name}: {schema}");
    }
}

// ---------------------------------------------------------------------------
// The descriptions carry the honesty notes
// ---------------------------------------------------------------------------

#[test]
fn every_description_says_it_can_abstain() {
    for tool in descriptors() {
        let description = tool.description.clone().unwrap_or_default();
        assert!(
            description.contains("says so plainly rather than guessing"),
            "{}: {description}",
            tool.name
        );
    }
}

#[test]
fn the_position_convention_is_stated_where_positions_are_reported() {
    for name in [GET_ACTIVE_FILE, GET_SELECTION] {
        let tool = descriptors().into_iter().find(|t| t.name == name).unwrap();
        let description = tool.description.unwrap_or_default();
        assert!(
            description.contains("1-based") && description.contains("0-based UTF-16"),
            "{name}: {description}"
        );
    }
}
