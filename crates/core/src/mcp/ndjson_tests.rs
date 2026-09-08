use super::*;
use serde_json::json;

fn decode(chunks: &[&str]) -> Vec<Line> {
    let mut decoder = LineDecoder::new();
    let mut out = Vec::new();
    for chunk in chunks {
        out.extend(decoder.push(chunk.as_bytes()).expect("no framing error"));
    }
    out
}

#[test]
fn encoded_output_contains_exactly_one_newline_and_it_is_last() {
    // The guard against somebody reaching for `to_string_pretty` because every
    // other writer in this tree does. A pretty message carries newlines inside
    // it, which the specification forbids on this transport.
    let message = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": { "tools": [ { "name": "sql.query", "nested": { "deep": [1, 2, 3] } } ] }
    });
    let bytes = encode(&message);

    assert_eq!(
        bytes.iter().filter(|b| **b == b'\n').count(),
        1,
        "encoded: {}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(bytes.last(), Some(&b'\n'));
}

#[test]
fn a_string_containing_a_newline_is_escaped_rather_than_emitted() {
    let bytes = encode(&json!({ "text": "one\ntwo" }));
    assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 1);
    assert!(String::from_utf8_lossy(&bytes).contains(r"one\ntwo"));
}

#[test]
fn a_message_is_decoded_from_one_chunk() {
    let lines = decode(&["{\"a\":1}\n"]);
    assert_eq!(lines, vec![Line::Message(json!({ "a": 1 }))]);
}

#[test]
fn a_message_split_across_chunks_is_reassembled() {
    let lines = decode(&["{\"a\":", "1,\"b\":", "2}\n{\"c\":3}\n"]);
    assert_eq!(
        lines,
        vec![
            Line::Message(json!({ "a": 1, "b": 2 })),
            Line::Message(json!({ "c": 3 })),
        ]
    );
}

#[test]
fn a_blank_line_yields_nothing() {
    assert_eq!(decode(&["\n", "\r\n", "   \n"]), vec![]);
}

#[test]
fn crlf_endings_are_stripped_before_parsing() {
    assert_eq!(
        decode(&["{\"a\":1}\r\n"]),
        vec![Line::Message(json!({ "a": 1 }))]
    );
}

#[test]
fn an_incomplete_trailing_line_is_held_rather_than_parsed() {
    let mut decoder = LineDecoder::new();
    assert_eq!(decoder.push(b"{\"a\":1}").unwrap(), vec![]);
    assert_eq!(
        decoder.push(b"\n").unwrap(),
        vec![Line::Message(json!({ "a": 1 }))]
    );
}

#[test]
fn a_malformed_line_does_not_poison_the_decoder() {
    // The contrast with `lsp::framing`, which fails closed. A newline is a
    // resynchronisation point, so one bad message is not a lost stream.
    let mut decoder = LineDecoder::new();
    let lines = decoder.push(b"not json\n{\"a\":1}\n").unwrap();
    assert!(
        matches!(lines[0], Line::Malformed { .. }),
        "expected the bad line to be reported, got {:?}",
        lines[0]
    );
    assert_eq!(lines[1], Line::Message(json!({ "a": 1 })));
    assert!(!decoder.is_poisoned());
    assert_eq!(
        decoder.push(b"{\"b\":2}\n").unwrap(),
        vec![Line::Message(json!({ "b": 2 }))]
    );
}

#[test]
fn a_malformed_line_is_reported_rather_than_dropped() {
    let lines = decode(&["oops\n"]);
    match &lines[0] {
        Line::Malformed { text, error } => {
            assert_eq!(text, "oops");
            assert!(!error.is_empty());
        }
        other => panic!("expected Malformed, got {other:?}"),
    }
}

#[test]
fn an_over_long_line_poisons_because_the_boundary_is_unknown() {
    // The one case where `lsp::framing`'s fail-closed reasoning *does* carry:
    // no terminator was ever seen, so where the next message begins is not
    // knowable and no later byte can be trusted.
    let mut decoder = LineDecoder::with_limit(16);
    let error = decoder.push(&[b'x'; 64]).unwrap_err();
    assert_eq!(error, NdjsonError::LineTooLarge { limit: 16 });
    assert!(decoder.is_poisoned());
    assert_eq!(
        decoder.push(b"{\"a\":1}\n").unwrap_err(),
        NdjsonError::Poisoned,
        "a poisoned decoder must refuse even a well-formed line"
    );
}

#[test]
fn a_long_but_terminated_line_is_not_over_long() {
    // The cap is on what is *pending*: a line that arrived complete inside one
    // chunk has a known boundary however big it is.
    let mut decoder = LineDecoder::with_limit(16);
    let payload = format!("{{\"a\":\"{}\"}}\n", "y".repeat(64));
    let lines = decoder.push(payload.as_bytes()).unwrap();
    assert_eq!(lines.len(), 1);
    assert!(!decoder.is_poisoned());
}

#[test]
fn several_messages_in_one_chunk_all_arrive() {
    let lines = decode(&["{\"a\":1}\n{\"b\":2}\n{\"c\":3}\n"]);
    assert_eq!(lines.len(), 3);
}
