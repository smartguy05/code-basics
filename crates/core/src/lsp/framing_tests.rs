use super::*;

/// Push one slice and expect exactly one frame out of it.
fn one(decoder: &mut Decoder, bytes: &[u8]) -> Vec<u8> {
    let mut frames = decoder.push(bytes).expect("framing should succeed");
    assert_eq!(1, frames.len(), "expected exactly one frame");
    frames.remove(0)
}

#[test]
fn encode_writes_a_content_length_header_and_a_blank_line() {
    assert_eq!(
        b"Content-Length: 2\r\n\r\n{}".to_vec(),
        encode(b"{}"),
        "the header, a CRLF, a blank CRLF, then the body verbatim"
    );
}

#[test]
fn content_length_counts_bytes_and_not_characters() {
    // Three characters, six bytes. A `chars().count()` here would truncate the
    // body and desynchronise the stream from this point on, permanently.
    let body = "\"é\"".as_bytes();
    assert_eq!(4, body.len());
    let framed = encode(body);
    let header = String::from_utf8(framed[..framed.len() - body.len()].to_vec()).unwrap();
    assert!(
        header.contains("Content-Length: 4"),
        "expected a byte count, got {header:?}"
    );
}

#[test]
fn a_whole_frame_in_one_push_comes_straight_back_out() {
    let mut decoder = Decoder::new();
    assert_eq!(b"{}".to_vec(), one(&mut decoder, &encode(b"{}")));
    assert_eq!(0, decoder.buffered(), "nothing should be left over");
}

#[test]
fn a_frame_split_at_every_possible_boundary_still_reassembles() {
    let framed = encode(br#"{"jsonrpc":"2.0","id":1,"result":null}"#);
    // The split point is not ours to choose: a pipe read can end anywhere,
    // including inside the header, inside the CRLFCRLF, and inside the body.
    for split in 1..framed.len() {
        let mut decoder = Decoder::new();
        let first = decoder.push(&framed[..split]).expect("first half");
        let second = decoder.push(&framed[split..]).expect("second half");
        let mut frames = first;
        frames.extend(second);
        assert_eq!(
            1,
            frames.len(),
            "one frame should survive a split at byte {split}"
        );
        assert_eq!(
            br#"{"jsonrpc":"2.0","id":1,"result":null}"#.to_vec(),
            frames[0]
        );
    }
}

#[test]
fn two_frames_in_one_push_both_come_out_in_order() {
    let mut decoder = Decoder::new();
    let mut bytes = encode(b"{\"a\":1}");
    bytes.extend(encode(b"{\"b\":2}"));
    let frames = decoder.push(&bytes).expect("both frames");
    assert_eq!(
        vec![b"{\"a\":1}".to_vec(), b"{\"b\":2}".to_vec()],
        frames,
        "order is the protocol's only ordering guarantee; it must be preserved"
    );
}

#[test]
fn a_byte_at_a_time_is_the_same_as_a_whole_frame() {
    let framed = encode(b"{\"x\":true}");
    let mut decoder = Decoder::new();
    let mut frames = Vec::new();
    for byte in &framed {
        frames.extend(decoder.push(&[*byte]).expect("one byte at a time"));
    }
    assert_eq!(vec![b"{\"x\":true}".to_vec()], frames);
}

#[test]
fn header_names_are_matched_case_insensitively() {
    let mut decoder = Decoder::new();
    // Real servers have shipped every casing of this; the RFC the LSP spec
    // borrows the header syntax from makes field names case-insensitive.
    let framed = b"content-length: 2\r\n\r\n{}";
    assert_eq!(b"{}".to_vec(), one(&mut decoder, framed));
}

#[test]
fn a_content_type_header_is_ignored_rather_than_refused() {
    let mut decoder = Decoder::new();
    let framed =
        b"Content-Length: 2\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{}";
    assert_eq!(b"{}".to_vec(), one(&mut decoder, framed));
}

#[test]
fn surrounding_whitespace_in_the_header_value_is_tolerated() {
    let mut decoder = Decoder::new();
    assert_eq!(
        b"{}".to_vec(),
        one(&mut decoder, b"Content-Length:2\r\n\r\n{}"),
        "no space after the colon is still a valid header"
    );
}

#[test]
fn a_bare_newline_terminator_is_refused_rather_than_tolerated() {
    let mut decoder = Decoder::new();
    // Tolerating `\n\n` would mean disagreeing with every real server about
    // where the body starts, and the disagreement is silent: the body would be
    // read one byte short and every later frame would be misaligned.
    let error = decoder
        .push(b"Content-Length: 2\n\n{}")
        .expect_err("a bare LF must not terminate the header block");
    assert!(
        matches!(error, FramingError::MalformedHeader { .. }),
        "unexpected error: {error:?}"
    );
}

#[test]
fn a_header_block_with_no_content_length_is_refused() {
    let mut decoder = Decoder::new();
    let error = decoder
        .push(b"Content-Type: application/json\r\n\r\n{}")
        .expect_err("there is no way to know where this body ends");
    assert!(
        matches!(error, FramingError::MissingContentLength { .. }),
        "unexpected error: {error:?}"
    );
}

#[test]
fn a_content_length_that_is_not_a_number_is_refused() {
    let mut decoder = Decoder::new();
    let error = decoder
        .push(b"Content-Length: banana\r\n\r\n{}")
        .expect_err("a non-numeric length cannot be guessed at");
    match error {
        FramingError::BadContentLength { value } => assert_eq!("banana", value),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn a_frame_larger_than_the_cap_is_refused_before_it_is_buffered() {
    let mut decoder = Decoder::with_limits(64, 1024);
    let error = decoder
        .push(b"Content-Length: 65\r\n\r\n")
        .expect_err("the cap exists so a server emitting nonsense cannot exhaust memory");
    match error {
        FramingError::FrameTooLarge { len } => assert_eq!(65, len),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn a_frame_exactly_at_the_cap_is_allowed() {
    // An off-by-one here would refuse a legitimate payload, which is the
    // failure this test exists to prevent.
    let body = vec![b'x'; 64];
    let mut decoder = Decoder::with_limits(64, 1024);
    assert_eq!(body, one(&mut decoder, &encode(&body)));
}

#[test]
fn an_endless_header_block_is_refused_rather_than_buffered_forever() {
    let mut decoder = Decoder::with_limits(1024, 32);
    let error = decoder
        .push(&[b'x'; 64])
        .expect_err("without the header cap this grows until the process dies");
    assert!(
        matches!(error, FramingError::HeaderTooLarge),
        "unexpected error: {error:?}"
    );
}

#[test]
fn an_empty_body_is_a_legal_frame() {
    let mut decoder = Decoder::new();
    assert_eq!(
        Vec::<u8>::new(),
        one(&mut decoder, b"Content-Length: 0\r\n\r\n")
    );
}

#[test]
fn a_body_holding_the_header_delimiter_is_not_cut_short_by_it() {
    // The body is length-delimited, not delimiter-delimited. A scan for the
    // next `\r\n\r\n` would end this frame in the middle of a string literal.
    let body = b"{\"text\":\"a\\r\\n\\r\\nb\"}";
    let mut decoder = Decoder::new();
    assert_eq!(body.to_vec(), one(&mut decoder, &encode(body)));
    let literal = b"{\"t\":\"\r\n\r\n\"}";
    assert_eq!(literal.to_vec(), one(&mut decoder, &encode(literal)));
}

#[test]
fn buffered_reports_the_whole_incomplete_frame_including_its_header() {
    // Nothing is consumed until a frame is whole, so the header is still held
    // too — the next push re-parses it. That keeps the buffer one contiguous
    // region, and this test pins the meaning of `buffered` so a later
    // optimisation cannot quietly change what a diagnostic number means.
    let header = b"Content-Length: 10\r\n\r\n";
    let mut decoder = Decoder::new();
    let mut partial = header.to_vec();
    partial.extend_from_slice(b"abc");
    assert!(decoder.push(&partial).unwrap().is_empty());
    assert_eq!(header.len() + 3, decoder.buffered());
    assert_eq!(b"abcdefghij".to_vec(), one(&mut decoder, b"defghij"));
    assert_eq!(0, decoder.buffered(), "a completed frame is released");
}

#[test]
fn a_decoder_that_has_refused_once_keeps_refusing() {
    // The stream cannot be resynchronised: there is no way to know where the
    // next frame starts, and guessing would silently corrupt every later
    // answer. Failing closed is the only honest option.
    let mut decoder = Decoder::new();
    decoder
        .push(b"Content-Length: nope\r\n\r\n")
        .expect_err("first refusal");
    decoder
        .push(&encode(b"{}"))
        .expect_err("a perfectly good frame after a broken one is still refused");
}
