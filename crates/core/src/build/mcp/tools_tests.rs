use super::*;

fn args(pairs: &[(&str, Value)]) -> JsonObject {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert((*key).to_string(), value.clone());
    }
    map
}

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
fn the_reader_tools_take_no_arguments_and_still_parse_with_junk_present() {
    // A reader tool ignores arguments entirely: extra keys are not a shape error.
    let junk = args(&[("configuration", json!("Release")), ("nonsense", json!(1))]);
    for name in [GET_ERRORS, GET_WARNINGS, GET_BUILD_STATUS] {
        assert!(parse_call(name, Some(&junk)).is_ok(), "{name}");
    }
}

#[test]
fn build_solution_reads_a_configuration() {
    let call = parse_call(
        BUILD_SOLUTION,
        Some(&args(&[("configuration", json!("Release"))])),
    )
    .unwrap();
    assert_eq!(
        call,
        BuildToolCall::BuildSolution {
            configuration: Some("Release".to_string())
        }
    );
}

#[test]
fn build_solution_with_no_configuration_is_none() {
    assert_eq!(
        parse_call(BUILD_SOLUTION, None).unwrap(),
        BuildToolCall::BuildSolution {
            configuration: None
        }
    );
}

#[test]
fn an_empty_configuration_is_no_configuration() {
    for empty in ["", "   "] {
        let call = parse_call(
            BUILD_SOLUTION,
            Some(&args(&[("configuration", json!(empty))])),
        )
        .unwrap();
        assert_eq!(
            call,
            BuildToolCall::BuildSolution {
                configuration: None
            },
            "{empty:?}"
        );
    }
}

#[test]
fn a_non_string_configuration_is_a_shape_error() {
    let error =
        parse_call(BUILD_SOLUTION, Some(&args(&[("configuration", json!(7))]))).unwrap_err();
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
}

#[test]
fn an_unknown_tool_lists_what_there_is_and_names_the_missing_one() {
    let error = parse_call("compile_everything", None).unwrap_err();
    assert_eq!(error.code, ErrorCode::METHOD_NOT_FOUND);
    assert!(
        error.message.contains("compile_everything"),
        "{}",
        error.message
    );
    assert!(error.message.contains(BUILD_SOLUTION), "{}", error.message);
}

#[test]
fn is_known_admits_exactly_the_advertised_tools() {
    for tool in ALL {
        assert!(is_known(tool), "{tool}");
    }
    assert!(!is_known("compile_everything"));
}
