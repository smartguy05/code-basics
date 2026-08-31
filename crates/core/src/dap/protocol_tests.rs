//! Tests for the DAP base protocol. Included by `protocol.rs` under `#[cfg(test)]`.

use super::*;

fn decode(json: &str) -> Message {
    serde_json::from_str(json).expect("decodes")
}

// ---------------------------------------------------------------------------
// The envelope
// ---------------------------------------------------------------------------

#[test]
fn a_request_decodes_with_its_command_and_arguments() {
    let message = decode(
        r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"coreclr"}}"#,
    );

    let Message::Request(request) = message else {
        panic!("expected a request");
    };
    assert_eq!(request.seq, 1);
    assert_eq!(request.command, "initialize");
    assert_eq!(
        request.arguments.unwrap()["adapterID"],
        serde_json::json!("coreclr")
    );
}

#[test]
fn a_response_points_back_with_request_seq_not_seq() {
    // The whole reason `sequence.rs` exists. A response carries two numbers and
    // only one of them says what it answers.
    let message =
        decode(r#"{"seq":7,"type":"response","request_seq":3,"success":true,"command":"threads"}"#);

    let Message::Response(response) = message else {
        panic!("expected a response");
    };
    assert_eq!(response.seq, 7, "its own sequence number");
    assert_eq!(response.request_seq, 3, "the request it answers");
}

#[test]
fn an_event_decodes_with_its_name_and_body() {
    let message = decode(r#"{"seq":2,"type":"event","event":"stopped","body":{"reason":"step"}}"#);

    let Message::Event(event) = message else {
        panic!("expected an event");
    };
    assert_eq!(event.event, "stopped");
    assert_eq!(event.body.unwrap()["reason"], serde_json::json!("step"));
}

#[test]
fn every_message_reports_its_own_seq() {
    assert_eq!(decode(r#"{"seq":4,"type":"event","event":"x"}"#).seq(), 4);
    assert_eq!(
        decode(r#"{"seq":5,"type":"request","command":"x"}"#).seq(),
        5
    );
    assert_eq!(
        decode(r#"{"seq":6,"type":"response","request_seq":1,"success":true,"command":"x"}"#).seq(),
        6
    );
}

#[test]
fn an_unknown_message_type_is_refused_rather_than_guessed_at() {
    // Continuing past one would mean guessing whether the adapter is waiting
    // for something. There is no `#[serde(other)]` here on purpose.
    assert!(serde_json::from_str::<Message>(r#"{"seq":1,"type":"telemetry"}"#).is_err());
}

#[test]
fn a_request_with_no_arguments_omits_the_key_rather_than_sending_null() {
    // Some adapters reject an explicit `null` here.
    let request = Request {
        seq: 1,
        command: "threads".into(),
        arguments: None,
    };
    let json = serde_json::to_string(&Message::Request(request)).unwrap();

    assert!(!json.contains("arguments"), "{json}");
    assert!(json.contains(r#""type":"request""#), "{json}");
}

#[test]
fn a_body_this_app_does_not_understand_survives_decoding() {
    // Every body is a Value precisely so a new event does not fail the stream.
    let message = decode(r#"{"seq":9,"type":"event","event":"invented","body":{"a":[1,2]}}"#);
    let Message::Event(event) = message else {
        panic!("expected an event");
    };
    assert_eq!(event.body.unwrap()["a"][1], serde_json::json!(2));
}

// ---------------------------------------------------------------------------
// Failure text
// ---------------------------------------------------------------------------

fn failed(message: Option<&str>, body: Option<serde_json::Value>) -> Response {
    Response {
        seq: 2,
        request_seq: 1,
        success: false,
        command: "setBreakpoints".into(),
        message: message.map(str::to_string),
        body,
    }
}

#[test]
fn a_failure_prefers_the_adapters_own_words() {
    assert_eq!(
        failed(Some("no symbols loaded"), None).failure_text(),
        "setBreakpoints failed: no symbols loaded"
    );
}

#[test]
fn a_failure_falls_back_to_the_error_format_in_the_body() {
    // Adapters disagree about which of the two they populate.
    let body = serde_json::json!({ "error": { "format": "module not loaded" } });
    assert_eq!(
        failed(None, Some(body)).failure_text(),
        "setBreakpoints failed: module not loaded"
    );
}

#[test]
fn a_failure_with_nothing_to_say_still_names_the_command() {
    // "failed" on its own has sent people looking in the wrong place.
    let text = failed(None, None).failure_text();
    assert!(text.contains("setBreakpoints"), "{text}");
    assert!(text.contains("no reason given"), "{text}");
}

#[test]
fn an_empty_message_is_not_treated_as_the_adapter_having_spoken() {
    let body = serde_json::json!({ "error": { "format": "the real reason" } });
    assert_eq!(
        failed(Some("   "), Some(body)).failure_text(),
        "setBreakpoints failed: the real reason"
    );
}

// ---------------------------------------------------------------------------
// Event bodies
// ---------------------------------------------------------------------------

#[test]
fn a_stopped_event_keeps_an_absent_thread_id_absent() {
    // Absent means *every* thread stopped, which is not thread 0 stopping.
    let body = serde_json::json!({ "reason": "breakpoint", "allThreadsStopped": true });
    let stopped = Stopped::from_body(Some(&body)).unwrap();

    assert_eq!(stopped.reason, "breakpoint");
    assert_eq!(stopped.thread_id, None);
    assert!(stopped.all_threads_stopped);
}

#[test]
fn a_stopped_event_carries_the_thread_and_the_words_for_the_user() {
    let body = serde_json::json!({
        "reason": "exception",
        "threadId": 12,
        "description": "Exception thrown",
        "text": "System.InvalidOperationException: nope",
    });
    let stopped = Stopped::from_body(Some(&body)).unwrap();

    assert_eq!(stopped.thread_id, Some(12));
    assert_eq!(stopped.description.as_deref(), Some("Exception thrown"));
    assert!(stopped.text.unwrap().contains("InvalidOperationException"));
    assert!(!stopped.all_threads_stopped, "absent is not true");
}

#[test]
fn a_stopped_event_with_no_reason_is_not_invented() {
    assert!(Stopped::from_body(Some(&serde_json::json!({ "threadId": 1 }))).is_none());
    assert!(Stopped::from_body(None).is_none());
}

#[test]
fn output_with_no_category_is_console_which_is_what_the_spec_says() {
    let body = serde_json::json!({ "output": "hello\n" });
    let output = Output::from_body(Some(&body)).unwrap();

    assert_eq!(output.category, "console");
    assert_eq!(output.output, "hello\n");
}

#[test]
fn output_keeps_the_stream_the_adapter_named() {
    let body = serde_json::json!({ "category": "stderr", "output": "bad\n" });
    assert_eq!(Output::from_body(Some(&body)).unwrap().category, "stderr");
}

#[test]
fn an_exit_code_is_read_and_an_absent_one_is_not_defaulted_to_zero() {
    assert_eq!(
        exited_code(Some(&serde_json::json!({ "exitCode": 3 }))),
        Some(3)
    );
    assert_eq!(exited_code(Some(&serde_json::json!({}))), None);
    assert_eq!(exited_code(None), None);
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

#[test]
fn an_absent_capability_is_unsupported() {
    let capabilities: Capabilities = serde_json::from_value(serde_json::json!({})).unwrap();
    assert_eq!(capabilities, Capabilities::default());
    assert!(!capabilities.supports_configuration_done_request);
}

#[test]
fn capabilities_are_read_from_the_camel_case_keys_the_wire_uses() {
    let capabilities: Capabilities = serde_json::from_value(serde_json::json!({
        "supportsConfigurationDoneRequest": true,
        "supportsTerminateRequest": true,
        "supportsConditionalBreakpoints": true,
    }))
    .unwrap();

    assert!(capabilities.supports_configuration_done_request);
    assert!(capabilities.supports_terminate_request);
    assert!(capabilities.supports_conditional_breakpoints);
    assert!(!capabilities.supports_set_variable, "not claimed");
}

#[test]
fn an_unknown_capability_does_not_fail_the_handshake() {
    // Adapters add capabilities between versions.
    let capabilities: Capabilities =
        serde_json::from_value(serde_json::json!({ "supportsSomethingNew": true })).unwrap();
    assert_eq!(capabilities, Capabilities::default());
}

// ---------------------------------------------------------------------------
// Outgoing arguments
// ---------------------------------------------------------------------------

#[test]
fn initialize_declares_the_one_convention_this_app_speaks() {
    let arguments = initialize_arguments("code-basics", "coreclr");

    assert_eq!(arguments["linesStartAt1"], serde_json::json!(true));
    assert_eq!(arguments["columnsStartAt1"], serde_json::json!(true));
    assert_eq!(arguments["adapterID"], serde_json::json!("coreclr"));
    assert_eq!(arguments["pathFormat"], serde_json::json!("path"));
}

#[test]
fn initialize_does_not_claim_reverse_requests_it_cannot_answer() {
    // Claiming `runInTerminal` and then not implementing it hangs the adapter.
    let arguments = initialize_arguments("code-basics", "coreclr");
    assert_eq!(
        arguments["supportsRunInTerminalRequest"],
        serde_json::json!(false)
    );
    assert_eq!(
        arguments["supportsStartDebuggingRequest"],
        serde_json::json!(false)
    );
}

#[test]
fn set_breakpoints_sends_the_whole_file_because_the_request_replaces_the_set() {
    let arguments = set_breakpoints_arguments("C:/ws/src/Program.cs", &[12, 40]);

    assert_eq!(
        arguments["source"]["path"],
        serde_json::json!("C:/ws/src/Program.cs")
    );
    assert_eq!(arguments["breakpoints"][0]["line"], serde_json::json!(12));
    assert_eq!(arguments["breakpoints"][1]["line"], serde_json::json!(40));
    assert_eq!(arguments["lines"], serde_json::json!([12, 40]));
}

#[test]
fn clearing_a_files_breakpoints_is_an_empty_array_not_an_absent_one() {
    // An adapter given no `breakpoints` key may keep what it had.
    let arguments = set_breakpoints_arguments("a.cs", &[]);
    assert_eq!(arguments["breakpoints"], serde_json::json!([]));
    assert_eq!(arguments["lines"], serde_json::json!([]));
}
