//! Tests for sequencing and correlation. Included by `sequence.rs`.

use super::*;

use crate::dap::protocol::{Event, Request, Response};

fn response(request_seq: i64, command: &str) -> Response {
    Response {
        seq: 100,
        request_seq,
        success: true,
        command: command.to_string(),
        message: None,
        body: None,
    }
}

// ---------------------------------------------------------------------------
// Sequencer
// ---------------------------------------------------------------------------

#[test]
fn sequence_numbers_start_at_one_as_the_specification_requires() {
    let mut sequencer = Sequencer::default();
    assert_eq!(sequencer.next_seq(), 1);
    assert_eq!(sequencer.next_seq(), 2);
    assert_eq!(sequencer.next_seq(), 3);
}

#[test]
fn peeking_does_not_consume() {
    let mut sequencer = Sequencer::default();
    assert_eq!(sequencer.peek(), 1);
    assert_eq!(sequencer.peek(), 1);
    assert_eq!(sequencer.next_seq(), 1);
    assert_eq!(sequencer.peek(), 2);
}

// ---------------------------------------------------------------------------
// Correlation
// ---------------------------------------------------------------------------

#[test]
fn a_response_is_matched_by_request_seq_not_by_its_own_seq() {
    // The bug this whole module exists to prevent: matching on `seq` pairs
    // every response with the wrong request.
    let mut correlator = Correlator::default();
    let first = correlator.request("initialize", None);
    let second = correlator.request("threads", None);
    assert_eq!((first.seq, second.seq), (1, 2));

    // Answered out of order, and each response carries an unrelated `seq`.
    let resolved = correlator
        .resolve(&response(second.seq, "threads"))
        .unwrap();
    assert_eq!(resolved.command, "threads");
    assert_eq!(resolved.seq, 2);

    let resolved = correlator
        .resolve(&response(first.seq, "initialize"))
        .unwrap();
    assert_eq!(resolved.command, "initialize");
}

#[test]
fn resolving_stops_tracking_the_request() {
    let mut correlator = Correlator::default();
    let request = correlator.request("threads", None);
    assert_eq!(correlator.outstanding(), 1);

    correlator
        .resolve(&response(request.seq, "threads"))
        .unwrap();
    assert_eq!(correlator.outstanding(), 0);
}

#[test]
fn a_second_response_to_the_same_request_is_reported_as_a_duplicate() {
    // It cannot be delivered — the first already went to the waiter — and
    // pretending otherwise loses it silently.
    let mut correlator = Correlator::default();
    let request = correlator.request("threads", None);
    correlator
        .resolve(&response(request.seq, "threads"))
        .unwrap();

    assert_eq!(
        correlator.resolve(&response(request.seq, "threads")),
        Err(CorrelationError::Duplicate { request_seq: 1 })
    );
}

#[test]
fn a_response_to_a_request_never_sent_is_reported_not_dropped() {
    // It means the two ends disagree about what has been asked, and the quiet
    // version of that is a session that hangs later for no visible reason.
    let mut correlator = Correlator::default();
    correlator.request("initialize", None);

    assert_eq!(
        correlator.resolve(&response(99, "threads")),
        Err(CorrelationError::Unknown { request_seq: 99 })
    );
}

#[test]
fn the_two_failures_are_distinguished_because_only_one_is_recoverable() {
    let mut correlator = Correlator::default();
    let request = correlator.request("threads", None);
    correlator
        .resolve(&response(request.seq, "threads"))
        .unwrap();

    // Below the high-water mark: it was sent, and answered already.
    assert!(matches!(
        correlator.resolve(&response(1, "threads")),
        Err(CorrelationError::Duplicate { .. })
    ));
    // Above it: never sent at all.
    assert!(matches!(
        correlator.resolve(&response(50, "threads")),
        Err(CorrelationError::Unknown { .. })
    ));
}

#[test]
fn a_non_request_message_takes_a_sequence_number_without_becoming_outstanding() {
    // Recording it would leave an entry no response ever clears.
    let mut correlator = Correlator::default();
    let seq = correlator.next_seq();

    assert_eq!(seq, 1);
    assert_eq!(correlator.outstanding(), 0);
    // And the numbering is shared: the next request continues from here.
    assert_eq!(correlator.request("threads", None).seq, 2);
}

#[test]
fn outstanding_commands_are_named_and_sorted_for_a_readable_teardown() {
    let mut correlator = Correlator::default();
    correlator.request("threads", None);
    correlator.request("initialize", None);
    correlator.request("stackTrace", None);

    assert_eq!(
        correlator.outstanding_commands(),
        vec!["initialize", "stackTrace", "threads"]
    );
}

#[test]
fn abandoning_reports_what_was_given_up_and_leaves_nothing_behind() {
    let mut correlator = Correlator::default();
    correlator.request("threads", None);
    correlator.request("evaluate", None);

    assert_eq!(correlator.abandon_all(), vec!["evaluate", "threads"]);
    assert_eq!(correlator.outstanding(), 0);
    // A reused correlator must not report work that can never finish.
    assert!(correlator.outstanding_commands().is_empty());
}

#[test]
fn arguments_are_carried_onto_the_request_untouched() {
    let mut correlator = Correlator::default();
    let arguments = serde_json::json!({ "threadId": 7 });
    let request = correlator.request("continue", Some(arguments.clone()));

    assert_eq!(request.command, "continue");
    assert_eq!(request.arguments, Some(arguments));
}

// ---------------------------------------------------------------------------
// What has to be answered
// ---------------------------------------------------------------------------

#[test]
fn an_adapters_request_must_be_answered_or_it_hangs_forever() {
    // `runInTerminal` blocks the adapter until the client replies. A reader
    // that classified it as "nothing for me to do" would hang the session with
    // no error anywhere.
    let reverse = Message::Request(Request {
        seq: 4,
        command: "runInTerminal".into(),
        arguments: None,
    });
    assert!(needs_reply(&reverse));
}

#[test]
fn an_event_and_a_response_oblige_nothing() {
    let event = Message::Event(Event {
        seq: 1,
        event: "stopped".into(),
        body: None,
    });
    assert!(!needs_reply(&event));
    assert!(!needs_reply(&Message::Response(response(1, "threads"))));
}
