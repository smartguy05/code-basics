use super::*;

use crate::build::mcp::instances::PROTOCOL_VERSION;

const TOKEN: &str = "cafebabecafebabecafebabecafebabecafebabecafebabecafebabecafebabe";

fn request() -> Request {
    Request {
        protocol: PROTOCOL_VERSION,
        token: TOKEN.to_string(),
        workspace: r"C:\code\repo".to_string(),
        tool: "get_errors".to_string(),
        arguments: Value::Null,
    }
}

fn params_of(request: &Request) -> Value {
    serde_json::to_value(request).unwrap()
}

#[test]
fn a_well_formed_request_round_trips_including_the_workspace() {
    let encoded = request_value(7, &request());
    assert_eq!(encoded["jsonrpc"], "2.0");
    assert_eq!(encoded["id"], 7);
    assert_eq!(encoded["method"], METHOD);
    let parsed = parse_request(METHOD, Some(&encoded["params"]), TOKEN).unwrap();
    assert_eq!(parsed, request());
    assert_eq!(parsed.workspace, r"C:\code\repo");
}

#[test]
fn the_wire_keys_are_camel_case_and_include_workspace() {
    let json = params_of(&request());
    let mut keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["arguments", "protocol", "token", "tool", "workspace"]
    );
}

#[test]
fn absent_arguments_stay_absent_rather_than_becoming_an_empty_object() {
    let parsed = parse_request(METHOD, Some(&params_of(&request())), TOKEN).unwrap();
    assert_eq!(parsed.arguments, Value::Null);
}

#[test]
fn another_method_on_this_pipe_is_refused_by_name() {
    let problem = parse_request("build.other", Some(&params_of(&request())), TOKEN).unwrap_err();
    assert_eq!(
        problem,
        RequestProblem::UnknownMethod {
            method: "build.other".to_string()
        }
    );
    assert!(problem.sentence().contains("build.other"));
}

#[test]
fn a_request_with_no_params_is_a_shape_problem_and_not_a_bad_token() {
    let problem = parse_request(METHOD, None, TOKEN).unwrap_err();
    assert_eq!(problem.code(), "bad_request");
}

#[test]
fn a_protocol_mismatch_is_reported_before_the_token_is_looked_at() {
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
    assert!(
        !problem.sentence().contains(TOKEN),
        "{}",
        problem.sentence()
    );
}

#[test]
fn an_application_with_no_token_accepts_nobody() {
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

#[test]
fn an_answer_round_trips_with_the_id_it_was_given() {
    let encoded = answer_value(&json!(3), &ToolAnswer::ok("build succeeded"));
    assert_eq!(encoded["id"], 3);
    let parsed = parse_answer(&encoded).unwrap();
    assert!(parsed.ok);
    assert_eq!(parsed.code, None);
    assert_eq!(parsed.text, "build succeeded");
}

#[test]
fn a_string_id_stays_a_string_id() {
    let encoded = answer_value(&json!("7"), &ToolAnswer::ok("x"));
    assert_eq!(encoded["id"], json!("7"));
    assert_ne!(encoded["id"], json!(7));
}

#[test]
fn a_refusal_carries_its_code_and_survives_the_round_trip() {
    let refusal = ToolAnswer::refused("neverBuilt", "no build has run");
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
fn an_answer_with_no_text_is_malformed_rather_than_reported_as_empty() {
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

#[test]
fn the_four_pipe_failures_have_four_codes_and_four_sentences() {
    let failures = [
        PipeFailure::NotConnected {
            pipe: r"\\.\pipe\x".to_string(),
            detail: "d".to_string(),
        },
        PipeFailure::Timeout {
            tool: "build_solution".to_string(),
            ms: 60000,
        },
        PipeFailure::Closed {
            tool: "get_errors".to_string(),
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
fn a_timeout_is_not_reported_as_a_closed_application() {
    let timeout = PipeFailure::Timeout {
        tool: "build_solution".to_string(),
        ms: 60000,
    };
    let sentence = timeout.sentence();
    assert!(sentence.contains("60000"), "{sentence}");
    assert!(sentence.contains("build_solution"), "{sentence}");
    assert!(sentence.contains("not the same as"), "{sentence}");
    assert_ne!(
        timeout.code(),
        PipeFailure::Closed { tool: "x".into() }.code()
    );
}
