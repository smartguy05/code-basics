use super::*;

use crate::browser::consent::Category;

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
    assert_eq!(ALL.len(), 13);
}

#[test]
fn every_tool_in_all_can_be_parsed_with_its_required_arguments() {
    // A tool advertised and unreachable is the failure this pairing exists to
    // prevent; so is one reachable and unadvertised.
    let supplied = args(&[
        ("url", json!("https://example.com")),
        ("ref", json!("e1")),
        ("text", json!("hello")),
        ("key", json!("Enter")),
    ]);
    for name in ALL {
        parse_call(name, Some(&supplied))
            .unwrap_or_else(|error| panic!("{name} did not parse: {error:?}"));
    }
}

#[test]
fn an_unknown_tool_lists_what_there_is_and_names_the_missing_one() {
    let error = parse_call("browser_evaluate", None).unwrap_err();
    assert_eq!(error.code, ErrorCode::METHOD_NOT_FOUND);
    assert!(error.message.contains("browser_evaluate"));
    assert!(error.message.contains(STATUS));
}

#[test]
fn asking_for_javascript_evaluation_is_answered_with_the_narrow_alternative() {
    // The decision has to reach the model that just tried it, or it retries
    // under another name.
    let error = parse_call("browser_evaluate", None).unwrap_err();
    assert!(
        error.message.contains("no tool that evaluates JavaScript"),
        "{}",
        error.message
    );
    assert!(error.message.contains(READ_PAGE), "{}", error.message);
    assert!(error.message.contains(CLICK), "{}", error.message);
}

// ---------------------------------------------------------------------------
// The read/write line comes from `consent`, not from here
// ---------------------------------------------------------------------------

#[test]
fn status_is_the_only_tool_needing_nothing() {
    let statuses: Vec<&str> = ALL
        .iter()
        .copied()
        .filter(|name| {
            parse_call(
                name,
                Some(&args(&[
                    ("url", json!("https://example.com")),
                    ("ref", json!("e1")),
                    ("text", json!("t")),
                    ("key", json!("Enter")),
                ])),
            )
            .unwrap()
            .tool()
            .category()
                == Category::Status
        })
        .collect();
    assert_eq!(statuses, vec![STATUS]);
}

#[test]
fn navigate_is_a_write_and_not_a_read() {
    // Moving the page the user is looking at mutates state they can see, in a
    // window they may not be watching.
    let call = parse_call(
        NAVIGATE,
        Some(&args(&[("url", json!("https://example.com"))])),
    )
    .unwrap();
    assert_eq!(call.tool().category(), Category::Write);
}

#[test]
fn back_forward_and_reload_are_writes_too() {
    for name in [BACK, FORWARD, RELOAD] {
        let call = parse_call(name, None).unwrap();
        assert_eq!(call.tool().category(), Category::Write, "{name}");
    }
}

#[test]
fn the_six_reads_are_reads() {
    for name in [
        CURRENT_URL,
        PAGE_TEXT,
        READ_PAGE,
        CONSOLE_MESSAGES,
        NETWORK_REQUESTS,
    ] {
        let call = parse_call(name, None).unwrap();
        assert_eq!(call.tool().category(), Category::Read, "{name}");
    }
}

#[test]
fn every_calls_tool_name_matches_the_name_it_was_parsed_from() {
    // The two tables — this module's constants and `consent::BrowserTool::name`
    // — must agree, or a refusal names a tool the client cannot call.
    let supplied = args(&[
        ("url", json!("https://example.com")),
        ("ref", json!("e1")),
        ("text", json!("t")),
        ("key", json!("Enter")),
    ]);
    for name in ALL {
        let call = parse_call(name, Some(&supplied)).unwrap();
        assert_eq!(call.tool().name(), name);
    }
}

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

#[test]
fn a_missing_required_argument_names_the_tool_and_the_field() {
    let error = parse_call(CLICK, None).unwrap_err();
    assert!(error.message.contains(CLICK), "{}", error.message);
    assert!(error.message.contains("ref"), "{}", error.message);
}

#[test]
fn a_wrong_typed_argument_says_what_it_was() {
    let error = parse_call(NAVIGATE, Some(&args(&[("url", json!(42))]))).unwrap_err();
    assert!(error.message.contains("a number"), "{}", error.message);
}

#[test]
fn type_needs_both_the_element_and_the_text() {
    assert!(parse_call(TYPE, Some(&args(&[("ref", json!("e1"))]))).is_err());
    assert!(parse_call(TYPE, Some(&args(&[("text", json!("hi"))]))).is_err());
    let call = parse_call(
        TYPE,
        Some(&args(&[("ref", json!("e1")), ("text", json!("hi"))])),
    )
    .unwrap();
    assert_eq!(
        call,
        BrowserToolCall::Type {
            element: "e1".to_string(),
            text: "hi".to_string()
        }
    );
}

#[test]
fn an_empty_text_is_a_real_argument_and_not_a_missing_one() {
    // Typing nothing into a field clears it, which is a thing someone means.
    let call = parse_call(
        TYPE,
        Some(&args(&[("ref", json!("e1")), ("text", json!(""))])),
    )
    .unwrap();
    assert_eq!(
        call,
        BrowserToolCall::Type {
            element: "e1".to_string(),
            text: String::new()
        }
    );
}

#[test]
fn an_absent_cursor_is_zero() {
    assert_eq!(
        parse_call(CONSOLE_MESSAGES, None).unwrap(),
        BrowserToolCall::ConsoleMessages { since: 0 }
    );
    assert_eq!(
        parse_call(NETWORK_REQUESTS, Some(&args(&[("since", Value::Null)]))).unwrap(),
        BrowserToolCall::NetworkRequests { since: 0 }
    );
}

#[test]
fn a_cursor_is_read() {
    assert_eq!(
        parse_call(CONSOLE_MESSAGES, Some(&args(&[("since", json!(42))]))).unwrap(),
        BrowserToolCall::ConsoleMessages { since: 42 }
    );
}

#[test]
fn a_negative_or_fractional_cursor_is_refused_rather_than_clamped() {
    // A clamped cursor answers a question nobody asked, and hides the caller's
    // bug behind a plausible page of results.
    for bad in [json!(-1), json!(1.5)] {
        let error =
            parse_call(CONSOLE_MESSAGES, Some(&args(&[("since", bad.clone())]))).unwrap_err();
        assert!(
            error.message.contains("whole number"),
            "{bad}: {}",
            error.message
        );
    }
}

#[test]
fn a_cursor_of_the_wrong_type_is_refused() {
    let error = parse_call(CONSOLE_MESSAGES, Some(&args(&[("since", json!("42"))]))).unwrap_err();
    assert!(error.message.contains("a string"), "{}", error.message);
}

#[test]
fn extra_arguments_are_ignored_rather_than_refused() {
    // A client sending a field this build does not know is not a failure worth
    // refusing a read over.
    let call = parse_call(PAGE_TEXT, Some(&args(&[("somethingNew", json!(true))]))).unwrap();
    assert_eq!(call, BrowserToolCall::PageText);
}

// ---------------------------------------------------------------------------
// The honesty notes have to be in the descriptions
// ---------------------------------------------------------------------------

#[test]
fn the_network_description_carries_the_coverage_note_verbatim() {
    // A model reads the description and nothing else. A row list that looks
    // like DevTools' and silently has no headers, no bodies and nothing from
    // before the init script is worse than one that says what it is.
    let described = descriptors()
        .into_iter()
        .find(|tool| tool.name == NETWORK_REQUESTS)
        .unwrap();
    let description = described.description.clone().unwrap_or_default();
    assert!(
        description.contains(NETWORK_COVERAGE_NOTE),
        "the coverage note must be quoted, not paraphrased: {description}"
    );
    assert!(
        description.contains("NOT the browser's network panel"),
        "{description}"
    );
}

#[test]
fn the_console_description_says_what_it_cannot_see() {
    let described = descriptors()
        .into_iter()
        .find(|tool| tool.name == CONSOLE_MESSAGES)
        .unwrap();
    let description = described.description.clone().unwrap_or_default();
    assert!(description.contains("unhandledrejection"), "{description}");
    assert!(
        description.contains("absence is not evidence"),
        "{description}"
    );
}

#[test]
fn every_read_description_says_permission_is_needed_and_per_page() {
    for name in [
        CURRENT_URL,
        PAGE_TEXT,
        READ_PAGE,
        CONSOLE_MESSAGES,
        NETWORK_REQUESTS,
    ] {
        let described = descriptors().into_iter().find(|t| t.name == name).unwrap();
        let description = described.description.clone().unwrap_or_default();
        assert!(description.contains(READ_SUFFIX), "{name}: {description}");
    }
}

#[test]
fn every_write_description_says_it_changes_the_page_the_user_is_looking_at() {
    for name in [NAVIGATE, CLICK, TYPE, PRESS_KEY, BACK, FORWARD, RELOAD] {
        let described = descriptors().into_iter().find(|t| t.name == name).unwrap();
        let description = described.description.clone().unwrap_or_default();
        assert!(description.contains(WRITE_SUFFIX), "{name}: {description}");
    }
}

#[test]
fn the_status_description_carries_no_consent_suffix() {
    // It answers with no consent in every state; telling a model otherwise
    // would stop it calling the one tool that explains a refusal.
    let described = descriptors()
        .into_iter()
        .find(|t| t.name == STATUS)
        .unwrap();
    let description = described.description.clone().unwrap_or_default();
    assert!(!description.contains(READ_SUFFIX), "{description}");
    assert!(!description.contains(WRITE_SUFFIX), "{description}");
    assert!(description.contains("Always answers"), "{description}");
}

#[test]
fn the_element_argument_says_it_is_not_a_css_selector() {
    let described = descriptors().into_iter().find(|t| t.name == CLICK).unwrap();
    let schema = serde_json::to_value(&described.input_schema).unwrap();
    let description = schema["properties"]["ref"]["description"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(description.contains("Not a CSS selector"), "{description}");
}

#[test]
fn back_and_forward_admit_they_cannot_tell_whether_anything_happened() {
    // wry 0.55 has no go_back/go_forward, so this goes through history.back()
    // and a page with empty history does nothing and reports success.
    for name in [BACK, FORWARD] {
        let described = descriptors().into_iter().find(|t| t.name == name).unwrap();
        let description = described.description.clone().unwrap_or_default();
        assert!(
            description.contains("not detectable from here"),
            "{name}: {description}"
        );
    }
}

#[test]
fn every_schema_declares_its_required_arguments() {
    let expected: &[(&str, &[&str])] = &[
        (STATUS, &[]),
        (CURRENT_URL, &[]),
        (PAGE_TEXT, &[]),
        (READ_PAGE, &[]),
        (CONSOLE_MESSAGES, &[]),
        (NETWORK_REQUESTS, &[]),
        (NAVIGATE, &["url"]),
        (CLICK, &["ref"]),
        (TYPE, &["ref", "text"]),
        (PRESS_KEY, &["key"]),
        (BACK, &[]),
        (FORWARD, &[]),
        (RELOAD, &[]),
    ];
    for (name, required) in expected {
        let described = descriptors().into_iter().find(|t| t.name == *name).unwrap();
        let schema = serde_json::to_value(&described.input_schema).unwrap();
        let declared: Vec<String> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(declared, *required, "{name}");
    }
}

/// `browser_evaluate` is a documented *decision*, and the sentence that says so
/// must be reachable from the shim as well as from the application. There is one
/// wording, so this is also the drift alarm: reword one side and this fails.
#[test]
fn the_shim_and_the_application_refuse_an_unknown_tool_in_the_same_words() {
    let from_application = parse_call("browser_evaluate", None)
        .expect_err("an unadvertised tool is refused, not parsed");
    assert_eq!(
        from_application.message,
        unknown_tool_sentence("browser_evaluate")
    );
}

/// The name check admits exactly what `tools/list` advertised — no more (a tool
/// reachable but unadvertised) and no less (an advertised tool the shim would
/// refuse before the application ever saw it).
#[test]
fn is_known_admits_exactly_the_advertised_tools() {
    for tool in ALL {
        assert!(is_known(tool), "{tool} is advertised and must be accepted");
    }
    assert!(!is_known("browser_evaluate"));
    assert!(!is_known(""));
    assert!(!is_known("browser_status "));
}
