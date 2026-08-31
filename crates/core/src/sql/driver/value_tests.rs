use super::*;
use crate::sql::format::{format_cell, Cell};
use crate::sql::model::SqlValue as Wire;

#[test]
fn null_empty_and_truncated_stay_three_different_wire_shapes() {
    assert_eq!(to_wire(format_cell(Cell::Null)), Wire::Null);
    assert_eq!(
        to_wire(format_cell(Cell::Text(String::new()))),
        Wire::Text {
            text: String::new(),
            truncated: false
        }
    );
    let long = "a".repeat(crate::sql::format::MAX_TEXT_CHARS + 1);
    match to_wire(format_cell(Cell::Text(long))) {
        Wire::Text { truncated, .. } => assert!(truncated),
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn an_undecodable_type_reaches_the_wire_named_never_blank() {
    assert_eq!(
        to_wire(format_cell(Cell::Unsupported {
            type_name: "geography".to_string()
        })),
        Wire::Unsupported {
            type_name: "geography".to_string()
        }
    );
}

#[test]
fn a_cell_error_reaches_the_wire_as_unavailable_not_as_null() {
    let v = to_wire(format_cell(Cell::Error {
        reason: "invalid utf-8".to_string(),
    }));
    assert_ne!(v, Wire::Null);
    assert_eq!(
        v,
        Wire::Unavailable {
            reason: "invalid utf-8".to_string()
        }
    );
    assert!(!v.is_known());
}

#[test]
fn numbers_cross_as_the_text_the_driver_formatted() {
    assert_eq!(
        to_wire(format_cell(Cell::Numeric("0.10".to_string()))),
        Wire::Number {
            text: "0.10".to_string()
        },
        "scale is part of the value"
    );
    assert_eq!(
        to_wire(format_cell(Cell::Int(i64::MAX))),
        Wire::Number {
            text: "9223372036854775807".to_string()
        }
    );
}

#[test]
fn a_blob_reports_its_true_length_after_the_hex_is_cut() {
    let bytes = vec![0xabu8; crate::sql::format::MAX_BYTES_RENDERED + 10];
    match to_wire(format_cell(Cell::Bytes(bytes))) {
        Wire::Bytes {
            hex,
            byte_length,
            truncated,
        } => {
            assert!(truncated);
            assert_eq!(
                byte_length,
                crate::sql::format::MAX_BYTES_RENDERED as u64 + 10
            );
            assert_eq!(hex.len(), crate::sql::format::MAX_BYTES_RENDERED * 2);
        }
        other => panic!("expected bytes, got {other:?}"),
    }
}
