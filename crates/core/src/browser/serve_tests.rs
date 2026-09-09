use super::*;

#[test]
fn the_handshake_consults_no_registry_and_lists_every_tool_regardless() {
    // The lifetime rule: a server that fails `initialize` is dropped silently
    // by its host and the user never learns why. So the handshake must not
    // depend on an application running — and this test runs with the registry
    // pointed at a file that does not exist, which is the situation.
    let previous = std::env::var_os("CB_BROWSER_INSTANCES_PATH");
    std::env::set_var(
        "CB_BROWSER_INSTANCES_PATH",
        r"C:\nowhere\no-such-registry.json",
    );

    let result = initialize_result(None);
    assert_eq!(result.server_info.name, SERVER_NAME);
    assert!(result.capabilities.tools.is_some());
    let listed = tools_list_result();
    assert_eq!(
        listed["tools"].as_array().unwrap().len(),
        crate::browser::tools::ALL.len()
    );

    match previous {
        Some(value) => std::env::set_var("CB_BROWSER_INSTANCES_PATH", value),
        None => std::env::remove_var("CB_BROWSER_INSTANCES_PATH"),
    }
}

#[test]
fn the_server_names_itself_apart_from_the_sql_server() {
    // Two servers from one executable, and a client may have both installed.
    assert_ne!(SERVER_NAME, crate::mcp::serve::SERVER_NAME);
    assert_eq!(SERVER_NAME, "code-basics-browser");
}

#[test]
fn the_version_comes_from_the_crate_rather_than_a_literal() {
    assert_eq!(SERVER_VERSION, env!("CARGO_PKG_VERSION"));
    assert!(!SERVER_VERSION.is_empty());
}

#[test]
fn a_known_protocol_version_is_echoed_and_an_unknown_one_still_gets_a_session() {
    // Reused from `mcp::serve` rather than re-decided; this pins the reuse.
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
fn the_instructions_state_the_four_rules_a_model_cannot_infer() {
    let instructions = INSTRUCTIONS;
    assert!(
        instructions.contains("browser_status first"),
        "{instructions}"
    );
    assert!(instructions.contains("do not retry"), "{instructions}");
    assert!(instructions.contains("read and control"), "{instructions}");
    assert!(
        instructions.contains("empty list is not evidence"),
        "{instructions}"
    );
}

#[test]
fn the_instructions_say_there_is_no_javascript_evaluation_and_what_to_use_instead() {
    assert!(INSTRUCTIONS.contains("no tool that evaluates JavaScript"));
    assert!(INSTRUCTIONS.contains("browser_read_page"));
}

#[test]
fn the_instructions_say_this_is_the_users_own_session() {
    // The fact that makes every refusal here proportionate rather than fussy.
    assert!(INSTRUCTIONS.contains("logged-in session"));
    assert!(INSTRUCTIONS.contains("not a headless browser"));
}

#[test]
fn a_successful_answer_is_a_successful_result() {
    let result = answer_result(&ToolAnswer::ok("the page is ready"));
    assert_ne!(result.is_error, Some(true));
    let rendered = serde_json::to_value(&result).unwrap();
    assert!(rendered.to_string().contains("the page is ready"));
}

#[test]
fn a_refusal_is_a_tool_execution_error_and_never_a_protocol_error() {
    // The specification asks a client to hand a tool's own refusal to the
    // model; every refusal here has a real correction.
    let result = answer_result(&ToolAnswer::refused(
        "browser_panel_closed",
        "the panel is not open",
    ));
    assert_eq!(result.is_error, Some(true));
    let rendered = serde_json::to_value(&result).unwrap();
    let text = rendered.to_string();
    assert!(text.contains("browser_panel_closed"), "{text}");
    assert!(text.contains("the panel is not open"), "{text}");
}

#[test]
fn a_refusal_carries_its_code_structurally_so_a_model_need_not_parse_prose() {
    let result = answer_result(&ToolAnswer::refused("timeout", "it did not answer"));
    assert_eq!(
        result.structured_content,
        Some(json!({ "code": "timeout" }))
    );
}

#[test]
fn a_refusal_with_no_code_still_reads_as_a_refusal() {
    // Unrepresentable through `ToolAnswer::refused`, and still not assumed
    // away: a codeless refusal must not silently become a success.
    let result = answer_result(&ToolAnswer {
        ok: false,
        code: None,
        text: "something went wrong".to_string(),
    });
    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        result.structured_content,
        Some(json!({ "code": "refused" }))
    );
}

#[test]
fn an_unsupported_platform_names_the_platform_rather_than_saying_unavailable() {
    // "this build cannot do it here" and "your application is not running" are
    // different things, and only one is fixed by opening a panel.
    let answer = unsupported_answer("linux");
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("unsupported_platform"));
    assert!(answer.text.contains("linux"), "{}", answer.text);
    assert!(answer.text.contains("nothing to open"), "{}", answer.text);
    assert_ne!(
        answer.code.as_deref(),
        Some(crate::browser::instances::InstanceError::NoneRunning { hint: None }.code())
    );
}

/// The collapse this exists to stop. `call_tool` forwards a tool name to the
/// application, so with no application running an **unknown** name used to be
/// answered "No code-basics application is running" — telling an agent to start
/// the app and retry a tool that will never exist, forever. A name this server
/// does not have is a different answer from a browser it cannot reach, and the
/// no-app case is the one a client hits most.
#[test]
fn an_unknown_tool_is_named_rather_than_reported_as_a_missing_application() {
    let answer = unknown_tool_answer("browser_evaluate");
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("unknown_tool"));
    assert!(
        answer.text.contains("browser_evaluate"),
        "the refusal names the tool that was asked for: {}",
        answer.text
    );
    assert!(
        !answer
            .text
            .contains("No code-basics application is running"),
        "an unknown tool must not be reported as a missing application: {}",
        answer.text
    );
}

/// And it states the decision, so the refusal answers *why not* rather than
/// only *no* — an agent that is merely told no will keep looking for it.
#[test]
fn the_unknown_tool_refusal_states_the_evaluate_decision_and_the_alternative() {
    let answer = unknown_tool_answer("browser_evaluate");
    assert!(
        answer.text.contains("evaluates JavaScript"),
        "{}",
        answer.text
    );
    assert!(answer.text.contains(tools::READ_PAGE), "{}", answer.text);
    // It lists what there *is*, so the next call can be a real one.
    assert!(answer.text.contains(tools::STATUS), "{}", answer.text);
}

/// A `\` line continuation in a Rust string strips the newline *and* the
/// following indentation, so a sentence broken across lines reads correctly —
/// but an edit that joins the lines without removing the indentation leaves a
/// run of spaces inside text an agent reads, and no assertion about content
/// notices. This is the alarm for that whole class: it caught
/// `unsupported_answer` saying "WebView2 child          window".
#[test]
fn no_sentence_this_server_hands_out_carries_a_run_of_spaces() {
    let mut sentences = vec![
        INSTRUCTIONS.to_string(),
        unsupported_answer("linux").text,
        unknown_tool_answer("browser_evaluate").text,
    ];
    for tool in tools::descriptors() {
        sentences.push(
            tool.description
                .map(|text| text.to_string())
                .unwrap_or_default(),
        );
    }
    for sentence in sentences {
        assert!(
            !sentence.contains("  "),
            "a doubled space in text an agent reads: {sentence}"
        );
    }
}
