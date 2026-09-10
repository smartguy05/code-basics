use super::*;

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
fn every_tool_in_all_parses_with_a_path_and_a_position() {
    let supplied = args(&[
        ("path", json!("src/App.cs")),
        ("line", json!(10)),
        ("character", json!(4)),
    ]);
    for name in ALL {
        let call = parse_call(name, Some(&supplied))
            .unwrap_or_else(|error| panic!("{name} did not parse: {error:?}"));
        assert_eq!(call.name(), name);
        assert_eq!(call.path(), "src/App.cs");
    }
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
    assert!(error.message.contains(FIND_REFERENCES), "{}", error.message);
}

#[test]
fn is_known_admits_exactly_the_advertised_tools() {
    for tool in ALL {
        assert!(is_known(tool), "{tool}");
    }
    assert!(!is_known("get_symbol_info"));
    assert!(!is_known(""));
    assert!(!is_known("find_references "));
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
// path is required on every tool
// ---------------------------------------------------------------------------

#[test]
fn every_tool_requires_a_path() {
    for name in ALL {
        let error = parse_call(name, Some(&args(&[("symbol", json!("Foo"))]))).unwrap_err();
        assert!(error.message.contains("path"), "{name}: {}", error.message);
    }
}

#[test]
fn an_empty_path_is_refused() {
    let error = parse_call(GET_DIAGNOSTICS, Some(&args(&[("path", json!("   "))]))).unwrap_err();
    assert!(error.message.contains("empty"), "{}", error.message);
}

#[test]
fn a_wrong_typed_path_says_what_it_was() {
    let error = parse_call(GET_DIAGNOSTICS, Some(&args(&[("path", json!(42))]))).unwrap_err();
    assert!(error.message.contains("a number"), "{}", error.message);
}

// ---------------------------------------------------------------------------
// get_diagnostics is whole-file
// ---------------------------------------------------------------------------

#[test]
fn get_diagnostics_takes_only_a_path_and_ignores_a_position() {
    // Pull diagnostics are document-scoped; a position would be a lie in the
    // schema, and passing one must not change the call.
    let with_position = parse_call(
        GET_DIAGNOSTICS,
        Some(&args(&[
            ("path", json!("src/App.cs")),
            ("line", json!(3)),
            ("character", json!(0)),
        ])),
    )
    .unwrap();
    assert_eq!(
        with_position,
        RoslynToolCall::GetDiagnostics {
            path: "src/App.cs".to_string()
        }
    );
    let plain = parse_call(
        GET_DIAGNOSTICS,
        Some(&args(&[("path", json!("src/App.cs"))])),
    )
    .unwrap();
    assert_eq!(plain, with_position);
}

#[test]
fn the_diagnostics_schema_requires_only_path() {
    let schema = serde_json::to_value(
        descriptors()
            .into_iter()
            .find(|t| t.name == GET_DIAGNOSTICS)
            .unwrap()
            .input_schema,
    )
    .unwrap();
    let required: Vec<String> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(required, vec!["path"]);
    // And no line/character property, so a client is never told to pass one.
    let props = schema["properties"].as_object().unwrap();
    assert!(!props.contains_key("line"), "{props:?}");
    assert!(!props.contains_key("character"), "{props:?}");
}

// ---------------------------------------------------------------------------
// position vs symbol
// ---------------------------------------------------------------------------

#[test]
fn a_position_is_read_1_based_line_and_0_based_character() {
    let call = parse_call(
        FIND_REFERENCES,
        Some(&args(&[
            ("path", json!("src/App.cs")),
            ("line", json!(12)),
            ("character", json!(7)),
        ])),
    )
    .unwrap();
    assert_eq!(
        call,
        RoslynToolCall::FindReferences {
            path: "src/App.cs".to_string(),
            target: Target::Position {
                line: 12,
                character: 7
            }
        }
    );
}

#[test]
fn a_symbol_is_read_when_no_position_is_given() {
    let call = parse_call(
        FIND_REFERENCES,
        Some(&args(&[
            ("path", json!("src/App.cs")),
            ("symbol", json!("Widget")),
        ])),
    )
    .unwrap();
    assert_eq!(
        call,
        RoslynToolCall::FindReferences {
            path: "src/App.cs".to_string(),
            target: Target::Symbol {
                name: "Widget".to_string()
            }
        }
    );
}

#[test]
fn a_complete_position_wins_when_both_a_position_and_a_symbol_are_given() {
    let call = parse_call(
        GET_TYPE_HIERARCHY,
        Some(&args(&[
            ("path", json!("src/App.cs")),
            ("line", json!(4)),
            ("character", json!(2)),
            ("symbol", json!("Widget")),
        ])),
    )
    .unwrap();
    assert_eq!(
        call,
        RoslynToolCall::GetTypeHierarchy {
            path: "src/App.cs".to_string(),
            target: Target::Position {
                line: 4,
                character: 2
            }
        }
    );
}

#[test]
fn an_incomplete_position_is_a_shape_error_even_beside_a_symbol() {
    for pairs in [
        vec![("path", json!("a.cs")), ("line", json!(3))],
        vec![("path", json!("a.cs")), ("character", json!(3))],
        vec![
            ("path", json!("a.cs")),
            ("line", json!(3)),
            ("symbol", json!("Widget")),
        ],
    ] {
        let error = parse_call(FIND_REFERENCES, Some(&args(&pairs))).unwrap_err();
        assert!(
            error.message.contains("both"),
            "{pairs:?}: {}",
            error.message
        );
    }
}

#[test]
fn neither_a_position_nor_a_symbol_names_both_ways_in() {
    let error = parse_call(FIND_REFERENCES, Some(&args(&[("path", json!("a.cs"))]))).unwrap_err();
    assert!(error.message.contains("line"), "{}", error.message);
    assert!(error.message.contains("symbol"), "{}", error.message);
}

#[test]
fn a_zero_line_is_refused_because_lines_are_1_based() {
    let error = parse_call(
        FIND_REFERENCES,
        Some(&args(&[
            ("path", json!("a.cs")),
            ("line", json!(0)),
            ("character", json!(0)),
        ])),
    )
    .unwrap_err();
    assert!(error.message.contains("1-based"), "{}", error.message);
}

#[test]
fn a_zero_character_is_a_real_column_and_not_missing() {
    // The first column is 0; it must not read as absent.
    let call = parse_call(
        FIND_REFERENCES,
        Some(&args(&[
            ("path", json!("a.cs")),
            ("line", json!(1)),
            ("character", json!(0)),
        ])),
    )
    .unwrap();
    assert_eq!(
        call,
        RoslynToolCall::FindReferences {
            path: "a.cs".to_string(),
            target: Target::Position {
                line: 1,
                character: 0
            }
        }
    );
}

#[test]
fn an_empty_symbol_is_no_symbol() {
    // With no position and an empty symbol, there is no location at all.
    let error = parse_call(
        FIND_REFERENCES,
        Some(&args(&[("path", json!("a.cs")), ("symbol", json!("  "))])),
    )
    .unwrap_err();
    assert!(error.message.contains("location"), "{}", error.message);
}

#[test]
fn a_fractional_line_is_refused_rather_than_truncated() {
    let error = parse_call(
        FIND_REFERENCES,
        Some(&args(&[
            ("path", json!("a.cs")),
            ("line", json!(1.5)),
            ("character", json!(0)),
        ])),
    )
    .unwrap_err();
    assert!(error.message.contains("whole number"), "{}", error.message);
}

// ---------------------------------------------------------------------------
// the descriptions carry the honesty notes
// ---------------------------------------------------------------------------

#[test]
fn every_description_says_it_can_abstain() {
    for tool in descriptors() {
        let description = tool.description.clone().unwrap_or_default();
        assert!(
            description.contains("says so rather than guessing"),
            "{}: {description}",
            tool.name
        );
    }
}

#[test]
fn the_position_tools_explain_the_location_forms_and_diagnostics_does_not() {
    for name in [FIND_REFERENCES, GET_TYPE_HIERARCHY, RESOLVE_OVERLOADS] {
        let tool = descriptors().into_iter().find(|t| t.name == name).unwrap();
        let description = tool.description.unwrap_or_default();
        assert!(
            description.contains("either an exact position"),
            "{name}: {description}"
        );
    }
    let diagnostics = descriptors()
        .into_iter()
        .find(|t| t.name == GET_DIAGNOSTICS)
        .unwrap();
    assert!(
        !diagnostics
            .description
            .unwrap_or_default()
            .contains("either an exact position"),
        "get_diagnostics is whole-file and must not describe a position"
    );
}

#[test]
fn every_position_schema_requires_only_path() {
    for name in [FIND_REFERENCES, GET_TYPE_HIERARCHY, RESOLVE_OVERLOADS] {
        let schema = serde_json::to_value(
            descriptors()
                .into_iter()
                .find(|t| t.name == name)
                .unwrap()
                .input_schema,
        )
        .unwrap();
        let required: Vec<String> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(required, vec!["path"], "{name}");
        let props = schema["properties"].as_object().unwrap();
        for key in ["line", "character", "symbol"] {
            assert!(props.contains_key(key), "{name} missing {key}");
        }
    }
}
