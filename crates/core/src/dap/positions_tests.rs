//! Tests for the line/column conventions. Included by `positions.rs`.

use super::*;

#[test]
fn this_app_declares_one_based_lines_and_columns() {
    // Everything in this app is 1-based already — the editor gutter,
    // `SymbolIndex::line`, the open-a-file-at-a-line chain. Declaring 1 and
    // then subtracting one somewhere would be an off-by-one nobody could find,
    // because both ends would be individually correct.
    // Compared against what `initialize` actually sends, rather than asserted
    // as a bare constant: the pair being `true` is only meaningful if it is the
    // pair that goes on the wire, and that is what could drift.
    let arguments = crate::dap::protocol::initialize_arguments("code-basics", "coreclr");
    assert_eq!(
        arguments["linesStartAt1"],
        serde_json::json!(CLIENT_LINES_START_AT_1)
    );
    assert_eq!(
        arguments["columnsStartAt1"],
        serde_json::json!(CLIENT_COLUMNS_START_AT_1)
    );
}

#[test]
fn a_line_from_an_adapter_honouring_the_negotiation_is_unchanged() {
    assert_eq!(line_from_adapter(1), 1);
    assert_eq!(line_from_adapter(42), 42);
    assert_eq!(line_from_adapter(100_000), 100_000);
}

#[test]
fn line_zero_becomes_line_one_rather_than_panicking_or_wrapping() {
    // A 0-based adapter's first line. Clamped, not shifted — see below.
    assert_eq!(line_from_adapter(0), 1);
    assert_eq!(line_from_adapter(-5), 1);
}

#[test]
fn a_zero_based_stream_is_not_shifted_up_by_one() {
    // The clamp refuses only values impossible under *either* convention. A
    // shift would be a guess about which one the adapter used, made from a
    // number consistent with both — and it would silently undo an adapter's
    // legitimate move of a breakpoint to the next executable line.
    assert_eq!(line_from_adapter(10), 10, "not 11");
    assert_eq!(line_from_adapter(1), 1, "not 2");
}

#[test]
fn a_line_beyond_a_u32_is_clamped_rather_than_truncated() {
    assert_eq!(line_from_adapter(i64::MAX), u32::MAX);
}

#[test]
fn an_absent_column_stays_absent_because_it_is_a_real_answer() {
    // "somewhere on that line" is not column 1.
    assert_eq!(column_from_adapter(None), None);
    assert_eq!(column_from_adapter(Some(7)), Some(7));
}

#[test]
fn a_column_is_clamped_the_same_way_a_line_is() {
    assert_eq!(column_from_adapter(Some(0)), Some(1));
    assert_eq!(column_from_adapter(Some(-3)), Some(1));
    assert_eq!(column_from_adapter(Some(i64::MAX)), Some(u32::MAX));
}

#[test]
fn a_line_going_out_is_never_zero() {
    // An adapter told lines start at 1 and then handed a 0 is entitled to do
    // anything at all with it.
    assert_eq!(line_to_adapter(0), 1);
    assert_eq!(line_to_adapter(1), 1);
    assert_eq!(line_to_adapter(99), 99);
}

#[test]
fn a_line_round_trips_through_both_directions() {
    for line in [1u32, 2, 40, 12_345] {
        assert_eq!(line_from_adapter(line_to_adapter(line)), line);
    }
}
