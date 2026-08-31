use super::*;
use crate::sql::model::{RowCap, RowCapReason, SqlValue};

fn limits(max_rows: u32, max_bytes: u64) -> Limits {
    Limits {
        max_rows,
        max_bytes,
    }
}

fn text(s: &str) -> SqlValue {
    SqlValue::Text {
        text: s.to_string(),
        truncated: false,
    }
}

// ---------------------------------------------------------------------------
// The confirmation rule
// ---------------------------------------------------------------------------

#[test]
fn a_result_set_that_ends_exactly_at_the_row_limit_reports_no_cap() {
    // Three rows, limit three, and then the rows run out. Every row is present,
    // so claiming a cap would tell the user their answer is short when it is
    // complete.
    let mut t = CapTracker::new(limits(3, 1_000_000));
    assert_eq!(t.record(10), Flow::Continue);
    assert_eq!(t.record(10), Flow::Continue);
    assert_eq!(t.record(10), Flow::Full);
    assert!(t.is_full());
    assert_eq!(t.cap(), None, "no further row was ever seen");
    assert_eq!(t.rows(), 3);
}

#[test]
fn a_cap_is_reported_once_a_further_row_is_seen() {
    let mut t = CapTracker::new(limits(3, 1_000_000));
    t.record(10);
    t.record(10);
    t.record(10);
    t.confirm_more();
    assert_eq!(
        t.cap(),
        Some(RowCap {
            limit: 3,
            reason: RowCapReason::RowLimit
        })
    );
}

#[test]
fn confirming_without_reaching_capacity_reports_nothing() {
    // Defensive: a caller that calls confirm_more on a half-full tracker must
    // not conjure a cap out of it.
    let mut t = CapTracker::new(limits(10, 1_000_000));
    t.record(10);
    t.confirm_more();
    assert_eq!(t.cap(), None);
}

#[test]
fn a_zero_row_limit_is_full_before_any_row_and_reports_zero_delivered() {
    let mut t = CapTracker::new(limits(0, 1_000_000));
    assert!(t.is_full(), "no row may be delivered at all");
    assert_eq!(t.cap(), None, "not yet confirmed");
    t.confirm_more();
    assert_eq!(
        t.cap(),
        Some(RowCap {
            limit: 0,
            reason: RowCapReason::RowLimit
        })
    );
}

#[test]
fn a_zero_byte_budget_is_full_before_any_row_and_names_the_byte_budget() {
    let mut t = CapTracker::new(limits(10, 0));
    assert!(t.is_full());
    t.confirm_more();
    assert_eq!(t.cap().map(|c| c.reason), Some(RowCapReason::ByteLimit));
}

#[test]
fn recording_past_capacity_changes_nothing() {
    let mut t = CapTracker::new(limits(1, 1_000_000));
    t.record(10);
    assert_eq!(t.record(999), Flow::Full);
    assert_eq!(t.rows(), 1, "a rejected row is not counted");
    assert_eq!(t.bytes(), 10, "nor charged");
}

// ---------------------------------------------------------------------------
// Which cap bit
// ---------------------------------------------------------------------------

#[test]
fn the_byte_budget_bites_first_for_wide_rows() {
    let mut t = CapTracker::new(limits(1000, 100));
    assert_eq!(t.record(60), Flow::Continue);
    assert_eq!(t.record(60), Flow::Full);
    t.confirm_more();
    assert_eq!(
        t.cap(),
        Some(RowCap {
            limit: 2,
            reason: RowCapReason::ByteLimit
        }),
        "raising the row limit would not return more rows, so say so"
    );
}

#[test]
fn a_single_row_wider_than_the_whole_budget_is_still_delivered() {
    let mut t = CapTracker::new(limits(1000, 100));
    assert_eq!(t.record(10_000), Flow::Full);
    t.confirm_more();
    assert_eq!(
        t.cap(),
        Some(RowCap {
            limit: 1,
            reason: RowCapReason::ByteLimit
        }),
        "an empty grid for a table with data in it is the worse answer"
    );
}

#[test]
fn when_both_ceilings_bite_on_the_same_row_the_byte_budget_is_reported() {
    let mut t = CapTracker::new(limits(2, 100));
    t.record(50);
    t.record(50);
    t.confirm_more();
    assert_eq!(t.cap().map(|c| c.reason), Some(RowCapReason::ByteLimit));
}

#[test]
fn the_defaults_are_a_thousand_rows_and_eight_mebibytes() {
    let d = Limits::default();
    assert_eq!(d.max_rows, 1000);
    assert_eq!(d.max_bytes, 8 * 1024 * 1024);
}

// ---------------------------------------------------------------------------
// Byte accounting
// ---------------------------------------------------------------------------

#[test]
fn every_cell_costs_something_even_when_it_holds_nothing() {
    // A hundred NULLs is a real row with real cost; a budget that charged it
    // zero could never stop a very wide, very empty result set.
    assert_eq!(value_bytes(&SqlValue::Null), VALUE_OVERHEAD_BYTES);
    let row = vec![SqlValue::Null; 100];
    assert_eq!(row_bytes(&row), 100 * VALUE_OVERHEAD_BYTES);
}

#[test]
fn a_cell_is_charged_for_the_text_it_actually_carries() {
    assert_eq!(value_bytes(&text("abcde")), 5 + VALUE_OVERHEAD_BYTES);
}

#[test]
fn a_truncated_blob_is_charged_for_the_hex_shown_not_its_true_length() {
    // `byteLength` reports a gigabyte; the payload crossing IPC is the hex.
    let v = SqlValue::Bytes {
        hex: "00ff".to_string(),
        byte_length: 1_000_000_000,
        truncated: true,
    };
    assert_eq!(value_bytes(&v), 4 + VALUE_OVERHEAD_BYTES);
}

#[test]
fn an_admission_of_ignorance_is_charged_for_its_sentence() {
    let v = SqlValue::Unavailable {
        reason: "boom".to_string(),
    };
    assert_eq!(value_bytes(&v), 4 + VALUE_OVERHEAD_BYTES);
    let v = SqlValue::Unsupported {
        type_name: "geography".to_string(),
    };
    assert_eq!(value_bytes(&v), 9 + VALUE_OVERHEAD_BYTES);
}

// ---------------------------------------------------------------------------
// What a cell costs: the wire size, or the size the database holds
// ---------------------------------------------------------------------------

#[test]
fn a_cell_is_charged_at_the_size_the_database_holds_when_that_is_larger() {
    // The bug this encodes: a two-megabyte text value reaches the grid as a few
    // kilobytes, so charging the rendered size lets unbounded data stream past
    // the byte budget while the tracker reads a thumbnail of each row.
    let rendered = SqlValue::Text {
        text: "y".repeat(8192),
        truncated: true,
    };
    let source = 2 * 1024 * 1024;
    assert_eq!(
        charged_cell_bytes(&rendered, source),
        source + VALUE_OVERHEAD_BYTES
    );
}

#[test]
fn a_cell_is_charged_at_its_wire_size_when_that_is_larger() {
    // A number occupies eight bytes and renders wider than that; charging the
    // source size alone would undercount what actually crosses.
    let rendered = SqlValue::Number {
        text: "-9223372036854775808".to_string(),
    };
    assert_eq!(
        charged_cell_bytes(&rendered, 8),
        value_bytes(&rendered),
        "whichever is larger, never whichever came last"
    );
}

#[test]
fn a_row_charges_every_cell_and_never_zips_the_two_halves_out_of_step() {
    let row = vec![
        (SqlValue::Null, 0),
        (
            SqlValue::Text {
                text: "y".to_string(),
                truncated: true,
            },
            4096,
        ),
    ];
    assert_eq!(
        charged_row_bytes(&row),
        value_bytes(&SqlValue::Null) + 4096 + VALUE_OVERHEAD_BYTES
    );
}

#[test]
fn one_cell_larger_than_the_whole_budget_arms_the_byte_cap_rather_than_the_row_cap() {
    // The finding in full: a single enormous value must be reportable as a cap,
    // and as the *byte* cap — raising the row ceiling would return nothing.
    let mut tracker = CapTracker::new(Limits {
        max_rows: 1000,
        max_bytes: 64 * 1024,
    });
    let huge = SqlValue::Text {
        text: "y".repeat(8192),
        truncated: true,
    };
    assert_eq!(
        tracker.record(charged_row_bytes(&[(huge, 2 * 1024 * 1024)])),
        Flow::Full
    );
    tracker.confirm_more();
    let cap = tracker.cap().expect("the cap must be reported");
    assert_eq!(cap.reason, RowCapReason::ByteLimit);
    assert_eq!(cap.limit, 1, "the one row that was delivered");
}

// ---------------------------------------------------------------------------
// The bounded copy
// ---------------------------------------------------------------------------

#[test]
fn a_text_value_within_the_copy_bound_is_copied_whole() {
    let text = "y".repeat(TEXT_COPY_BYTES);
    assert_eq!(clip_text(&text), text);
    assert_eq!(clip_text(""), "");
}

#[test]
fn a_clipped_text_value_still_arrives_longer_than_the_display_cap() {
    // Otherwise `format` cannot tell a cut value from one that ends exactly at
    // the cap, and a two-megabyte cell would be rendered as complete.
    let text = "y".repeat(4 * 1024 * 1024);
    let clipped = clip_text(&text);
    assert!(
        clipped.len() > crate::sql::format::MAX_TEXT_BYTES,
        "{}",
        clipped.len()
    );
    assert!(clipped.len() <= TEXT_COPY_BYTES);
}

#[test]
fn a_clip_never_lands_inside_a_character() {
    // Four-byte characters, so the bound falls mid-character unless it is moved
    // back. Slicing a `str` at a non-boundary panics, so this is load-bearing.
    let text = "\u{1F600}".repeat(TEXT_COPY_BYTES);
    let clipped = clip_text(&text);
    assert!(clipped.chars().all(|c| c == '\u{1F600}'));
    assert!(clipped.len() <= TEXT_COPY_BYTES);
    assert!(clipped.len() > crate::sql::format::MAX_TEXT_BYTES);
}
