use super::*;

#[test]
fn the_handshake_lists_every_tool() {
    let result = initialize_result(None);
    assert_eq!(result.server_info.name, SERVER_NAME);
    assert!(result.capabilities.tools.is_some());
    let listed = tools_list_result(&crate::tool_gate::ToolGateFile::default());
    assert_eq!(
        listed["tools"].as_array().unwrap().len(),
        crate::build::mcp::tools::ALL.len()
    );
}

#[test]
fn a_disabled_tool_is_dropped_from_the_listing() {
    let mut gate = crate::tool_gate::ToolGateFile::default();
    let first = crate::build::mcp::tools::ALL[0];
    gate.set(crate::tool_gate::ServerId::Build, first, false);
    let listed = tools_list_result(&gate);
    let names: Vec<&str> = listed["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(
        !names.contains(&first),
        "the disabled tool is not advertised"
    );
    assert_eq!(names.len(), crate::build::mcp::tools::ALL.len() - 1);
}

#[test]
fn a_disabled_tool_answer_is_a_refusal_with_the_shared_code() {
    let answer = disabled_tool_answer("build_solution");
    assert!(!answer.ok);
    assert_eq!(
        answer.code.as_deref(),
        Some(crate::tool_gate::DISABLED_CODE)
    );
    assert!(answer.text.contains("build_solution"));
}

#[test]
fn the_server_names_itself_apart_from_the_other_servers() {
    assert_eq!(SERVER_NAME, "code-basics-build");
    assert_ne!(SERVER_NAME, crate::mcp::serve::SERVER_NAME);
    assert_ne!(SERVER_NAME, crate::roslyn::serve::SERVER_NAME);
    assert_ne!(SERVER_NAME, crate::browser::serve::SERVER_NAME);
    assert_ne!(SERVER_NAME, crate::tasks::mcp::serve::SERVER_NAME);
}

#[test]
fn the_version_comes_from_the_crate_rather_than_a_literal() {
    assert_eq!(SERVER_VERSION, env!("CARGO_PKG_VERSION"));
    assert!(!SERVER_VERSION.is_empty());
}

#[test]
fn a_known_protocol_version_is_echoed_and_an_unknown_one_still_gets_a_session() {
    let known = rmcp::model::ProtocolVersion::LATEST;
    assert_eq!(
        initialize_result(Some(known.as_str())).protocol_version,
        known
    );
    assert_eq!(
        initialize_result(Some("1066-01-01")).protocol_version,
        rmcp::model::ProtocolVersion::LATEST
    );
}

#[test]
fn the_instructions_state_the_rules_a_model_cannot_infer() {
    assert!(
        INSTRUCTIONS.contains("not scraped from console"),
        "{INSTRUCTIONS}"
    );
    assert!(
        INSTRUCTIONS.contains("six distinct answers"),
        "{INSTRUCTIONS}"
    );
    assert!(
        INSTRUCTIONS.contains("scoped to the workspace"),
        "{INSTRUCTIONS}"
    );
}

#[test]
fn a_successful_answer_is_a_successful_result() {
    let result = answer_result(&ToolAnswer::ok("build succeeded"));
    assert_ne!(result.is_error, Some(true));
    let rendered = serde_json::to_value(&result).unwrap();
    assert!(rendered.to_string().contains("build succeeded"));
}

#[test]
fn a_refusal_is_a_tool_execution_error_and_carries_its_code() {
    let result = answer_result(&ToolAnswer::refused("neverBuilt", "no build has run"));
    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        result.structured_content,
        Some(serde_json::json!({ "code": "neverBuilt" }))
    );
    let text = serde_json::to_value(&result).unwrap().to_string();
    assert!(text.contains("neverBuilt"), "{text}");
}

#[test]
fn an_unknown_tool_is_named_rather_than_reported_as_a_missing_application() {
    let answer = unknown_tool_answer("compile_everything");
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("unknown_tool"));
    assert!(
        answer.text.contains("compile_everything"),
        "{}",
        answer.text
    );
    assert!(
        !answer
            .text
            .contains("No code-basics application is running"),
        "{}",
        answer.text
    );
}

#[test]
fn the_shim_unknown_tool_wording_matches_the_application_parse() {
    assert_eq!(
        unknown_tool_answer("x").text,
        crate::build::mcp::tools::unknown_tool_sentence("x")
    );
}
