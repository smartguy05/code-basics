//! That the reuse is real and behaves as the browser pipe needs. Included by
//! `framing.rs`.
//!
//! These do not re-test `mcp::ndjson` — it has its own suite. They pin the two
//! claims *this* module makes: that the reused decoder's poison rules are the
//! ones this pipe wants, and that the tighter cap is actually applied.

use super::*;

fn messages(decoder: &mut LineDecoder, bytes: &[u8]) -> Vec<Line> {
    decoder.push(bytes).expect("should not have poisoned")
}

#[test]
fn the_decoder_this_module_hands_out_carries_the_tighter_cap() {
    // The whole reason for a named constructor: a call site that forgot the
    // constant would silently get the 8 MiB MCP default.
    let mut decoder = pipe_decoder();
    let over = vec![b'x'; MAX_PIPE_LINE_BYTES + 1];
    match decoder.push(&over) {
        Err(NdjsonError::LineTooLarge { limit }) => assert_eq!(limit, MAX_PIPE_LINE_BYTES),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_pipe_cap_is_smaller_than_the_mcp_stdio_cap() {
    // A request on this pipe is a tool call, never a page dump — page text
    // travels in the reply, which this process writes. And the pipe is
    // reachable by any process running as this user, so the allocation a
    // prober can force is worth bounding harder.
    const { assert!(MAX_PIPE_LINE_BYTES < crate::mcp::ndjson::MAX_LINE_BYTES) };
}

#[test]
fn a_line_that_is_not_json_is_reported_and_the_stream_continues() {
    // The rule that makes `mcp::ndjson` the right layer rather than
    // `lsp::framing`: a newline *is* a resynchronisation point, so one bad
    // message is not a lost stream — and on this pipe a malformed line is a
    // probe worth counting rather than discarding.
    let mut decoder = pipe_decoder();
    let lines = messages(&mut decoder, b"{\"id\":1}\nnot json\n{\"id\":2}\n");
    assert_eq!(lines.len(), 3);
    assert!(matches!(lines[0], Line::Message(_)));
    match &lines[1] {
        Line::Malformed { text, .. } => assert_eq!(text, "not json"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(lines[2], Line::Message(_)));
    assert!(!decoder.is_poisoned());
}

#[test]
fn an_over_long_unterminated_line_poisons_because_the_boundary_is_unknown() {
    // The opposite rule, and it is the right one here too: nothing that follows
    // can be located, so the only correct reaction is to stop.
    let mut decoder = LineDecoder::with_limit(64);
    assert!(decoder.push(&[b'x'; 100]).is_err());
    assert!(decoder.is_poisoned());
    assert!(matches!(
        decoder.push(b"{\"id\":1}\n"),
        Err(NdjsonError::Poisoned)
    ));
}

#[test]
fn a_message_is_reassembled_across_chunk_boundaries() {
    // A pipe read has no relationship to a message boundary.
    let mut decoder = pipe_decoder();
    assert!(messages(&mut decoder, b"{\"jsonrpc\":\"2.0\",").is_empty());
    assert!(messages(&mut decoder, b"\"id\":1,\"method\":\"browser").is_empty());
    let lines = messages(&mut decoder, b"_status\"}\n");
    assert_eq!(lines.len(), 1);
    match &lines[0] {
        Line::Message(value) => assert_eq!(value["method"], serde_json::json!("browser_status")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn encoding_produces_one_line_with_the_newline_last() {
    // JSON-RPC over NDJSON forbids an embedded newline, so a pretty-printed
    // message would be read as several broken ones.
    let encoded = encode(&serde_json::json!({"id": 1, "result": {"a": "b"}}));
    assert_eq!(encoded.iter().filter(|b| **b == b'\n').count(), 1);
    assert_eq!(encoded.last(), Some(&b'\n'));
}

#[test]
fn what_this_module_encodes_is_what_its_own_decoder_reads() {
    let original = serde_json::json!({"jsonrpc":"2.0","id":9,"result":{"url":"https://a/"}});
    let mut decoder = pipe_decoder();
    let lines = messages(&mut decoder, &encode(&original));
    assert_eq!(lines, vec![Line::Message(original)]);
}
