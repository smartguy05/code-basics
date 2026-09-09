use super::*;

use crate::browser::instances::PROTOCOL_VERSION;

const TOKEN: &str = "cafebabecafebabecafebabecafebabecafebabecafebabecafebabecafebabe";

fn request() -> Request {
    Request {
        protocol: PROTOCOL_VERSION,
        token: TOKEN.to_string(),
        tool: "browser_status".to_string(),
        arguments: Value::Null,
    }
}

fn params_of(request: &Request) -> Value {
    serde_json::to_value(request).unwrap()
}

// ---------------------------------------------------------------------------
// The request
// ---------------------------------------------------------------------------

#[test]
fn a_well_formed_request_round_trips() {
    let encoded = request_value(7, &request());
    assert_eq!(encoded["jsonrpc"], "2.0");
    assert_eq!(encoded["id"], 7);
    assert_eq!(encoded["method"], METHOD);
    let parsed = parse_request(METHOD, Some(&encoded["params"]), TOKEN).unwrap();
    assert_eq!(parsed, request());
}

#[test]
fn the_wire_keys_are_camel_case() {
    let json = params_of(&request());
    let mut keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["arguments", "protocol", "token", "tool"]);
}

#[test]
fn absent_arguments_stay_absent_rather_than_becoming_an_empty_object() {
    // An empty object is a set of arguments that happens to be empty. This
    // layer does not invent one; `tools::parse_call` treats them the same, and
    // that is its decision to make.
    let parsed = parse_request(METHOD, Some(&params_of(&request())), TOKEN).unwrap();
    assert_eq!(parsed.arguments, Value::Null);
}

#[test]
fn another_method_on_this_pipe_is_refused_by_name() {
    let problem =
        parse_request("browser.evaluate", Some(&params_of(&request())), TOKEN).unwrap_err();
    assert_eq!(
        problem,
        RequestProblem::UnknownMethod {
            method: "browser.evaluate".to_string()
        }
    );
    assert!(problem.sentence().contains("browser.evaluate"));
}

#[test]
fn a_request_with_no_params_is_a_shape_problem_and_not_a_bad_token() {
    let problem = parse_request(METHOD, None, TOKEN).unwrap_err();
    assert_eq!(problem.code(), "bad_request");
}

#[test]
fn a_protocol_mismatch_is_reported_before_the_token_is_looked_at() {
    // A caller speaking a wire this build does not know may have put the token
    // somewhere else entirely, so reporting a bad token would name the wrong
    // cause and send somebody to the wrong file.
    let mut wrong = request();
    wrong.protocol = PROTOCOL_VERSION + 1;
    wrong.token = "not the right token".to_string();
    let problem = parse_request(METHOD, Some(&params_of(&wrong)), TOKEN).unwrap_err();
    assert_eq!(
        problem,
        RequestProblem::ProtocolMismatch {
            theirs: PROTOCOL_VERSION + 1,
            ours: PROTOCOL_VERSION
        }
    );
}

#[test]
fn a_wrong_token_is_refused() {
    let mut wrong = request();
    wrong.token = "0".repeat(64);
    let problem = parse_request(METHOD, Some(&params_of(&wrong)), TOKEN).unwrap_err();
    assert_eq!(problem, RequestProblem::BadToken);
}

#[test]
fn the_token_refusal_does_not_leak_the_expected_token() {
    let mut wrong = request();
    wrong.token = "0".repeat(64);
    let problem = parse_request(METHOD, Some(&params_of(&wrong)), TOKEN).unwrap_err();
    let sentence = problem.sentence();
    assert!(
        !sentence.contains(TOKEN),
        "a refusal must not hand over the answer: {sentence}"
    );
    assert!(!sentence.contains(&"0".repeat(64)), "{sentence}");
}

#[test]
fn an_application_with_no_token_accepts_nobody() {
    // The failure that would otherwise be silent: if publishing the token ever
    // failed, an empty expectation must not turn into "everyone matches".
    assert!(!token_matches("", ""));
    let mut empty = request();
    empty.token = String::new();
    assert_eq!(
        parse_request(METHOD, Some(&params_of(&empty)), "").unwrap_err(),
        RequestProblem::BadToken
    );
}

#[test]
fn the_four_request_problems_have_four_codes_and_four_sentences() {
    let problems = [
        RequestProblem::UnknownMethod {
            method: "x".to_string(),
        },
        RequestProblem::ProtocolMismatch { theirs: 9, ours: 1 },
        RequestProblem::BadToken,
        RequestProblem::Shape {
            detail: "d".to_string(),
        },
    ];
    let codes: std::collections::BTreeSet<&str> = problems.iter().map(|p| p.code()).collect();
    assert_eq!(codes.len(), 4);
    let sentences: std::collections::BTreeSet<String> =
        problems.iter().map(|p| p.sentence()).collect();
    assert_eq!(sentences.len(), 4);
}

#[test]
fn a_request_problem_becomes_an_answer_rather_than_a_dropped_connection() {
    let answer = RequestProblem::BadToken.answer();
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("bad_token"));
    assert!(!answer.text.is_empty());
}

// ---------------------------------------------------------------------------
// The answer
// ---------------------------------------------------------------------------

#[test]
fn an_answer_round_trips_with_the_id_it_was_given() {
    let encoded = answer_value(&json!(3), &ToolAnswer::ok("the page is ready"));
    assert_eq!(encoded["id"], 3);
    let parsed = parse_answer(&encoded).unwrap();
    assert!(parsed.ok);
    assert_eq!(parsed.code, None);
    assert_eq!(parsed.text, "the page is ready");
}

#[test]
fn a_string_id_stays_a_string_id() {
    // The `RequestId` lesson from `lsp::jsonrpc`: a numeric and a string id can
    // print the same and are different requests.
    let encoded = answer_value(&json!("7"), &ToolAnswer::ok("x"));
    assert_eq!(encoded["id"], json!("7"));
    assert_ne!(encoded["id"], json!(7));
}

#[test]
fn a_refusal_carries_its_code_and_survives_the_round_trip() {
    let refusal = ToolAnswer::refused("browser_panel_closed", "the panel is not open");
    let parsed = parse_answer(&answer_value(&json!(1), &refusal)).unwrap();
    assert_eq!(parsed, refusal);
    assert!(!parsed.ok);
}

#[test]
fn a_reply_with_neither_result_nor_error_is_malformed_and_not_an_empty_success() {
    let problem = parse_answer(&json!({ "jsonrpc": "2.0", "id": 1 })).unwrap_err();
    assert_eq!(problem.code(), "bad_reply");
}

#[test]
fn a_protocol_error_reply_is_malformed_rather_than_a_refusal() {
    // A refusal is a *successful* reply carrying ok:false. An `error` member
    // means the two builds disagree about the protocol, which is a different
    // fix from anything a model can do about the page.
    let problem = parse_answer(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": { "code": -32601, "message": "no" }
    }))
    .unwrap_err();
    assert_eq!(problem.code(), "bad_reply");
    assert!(problem.sentence().contains("reinstall") || problem.sentence().contains("Reinstall"));
}

#[test]
fn an_answer_with_no_text_is_malformed_rather_than_reported_as_a_blank_page() {
    // The silent failure this guards: a blank success reads to a model as "the
    // page has nothing on it", which is a claim about the page.
    for blank in ["", "   ", "\n"] {
        let problem = parse_answer(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "ok": true, "code": null, "text": blank }
        }))
        .unwrap_err();
        assert_eq!(problem.code(), "bad_reply", "{blank:?}");
    }
}

// ---------------------------------------------------------------------------
// The four pipe failures
// ---------------------------------------------------------------------------

#[test]
fn the_four_pipe_failures_have_four_codes_and_four_sentences() {
    let failures = [
        PipeFailure::NotConnected {
            pipe: r"\\.\pipe\x".to_string(),
            detail: "d".to_string(),
        },
        PipeFailure::Timeout {
            tool: "browser_page_text".to_string(),
            ms: 5000,
        },
        PipeFailure::Closed {
            tool: "browser_click".to_string(),
        },
        PipeFailure::Malformed {
            detail: "d".to_string(),
        },
    ];
    let codes: std::collections::BTreeSet<&str> = failures.iter().map(|f| f.code()).collect();
    assert_eq!(codes.len(), 4);
    let sentences: std::collections::BTreeSet<String> =
        failures.iter().map(|f| f.sentence()).collect();
    assert_eq!(sentences.len(), 4);
}

#[test]
fn a_timeout_is_not_reported_as_a_closed_browser() {
    // The collapse that would matter: an agent told "the browser is closed"
    // would ask the user to open it, when the panel is open and its page is
    // wedged.
    let timeout = PipeFailure::Timeout {
        tool: "browser_page_text".to_string(),
        ms: 5000,
    };
    let sentence = timeout.sentence();
    assert!(sentence.contains("5000"), "{sentence}");
    assert!(sentence.contains("browser_page_text"), "{sentence}");
    assert!(
        sentence.contains("not the same as the browser being closed"),
        "{sentence}"
    );
    assert_ne!(
        timeout.code(),
        PipeFailure::Closed { tool: "x".into() }.code()
    );
}

#[test]
fn a_connection_closed_mid_call_does_not_claim_the_tool_did_nothing() {
    // A write may already have landed. Saying "nothing happened" would be a
    // confident wrong answer about a page the agent just changed.
    let sentence = PipeFailure::Closed {
        tool: "browser_click".to_string(),
    }
    .sentence();
    assert!(sentence.contains("not known whether"), "{sentence}");
}

#[test]
fn a_pipe_failure_becomes_a_refusal_answer() {
    let answer = PipeFailure::Timeout {
        tool: "t".to_string(),
        ms: 1,
    }
    .answer();
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("timeout"));
}
