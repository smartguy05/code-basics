//! Tests for [`super`]: the six ways a cell can be wrong on the way to the grid.

use super::*;

#[test]
fn a_null_is_not_an_empty_string() {
    let null = format_cell(Cell::Null);
    let empty = format_cell(Cell::Text(String::new()));

    assert_eq!(null, SqlValue::Null);
    assert_eq!(
        empty,
        SqlValue::Text {
            text: String::new(),
            truncated: false
        }
    );
    assert_ne!(
        null, empty,
        "SQL NULL and an empty string must stay distinct"
    );
}

#[test]
fn a_long_string_is_marked_truncated_not_shortened_silently() {
    let long = "a".repeat(MAX_TEXT_CHARS + 500);

    match format_cell(Cell::Text(long.clone())) {
        SqlValue::Text { text, truncated } => {
            assert!(truncated, "a cut value must say it was cut");
            assert!(text.chars().count() <= MAX_TEXT_CHARS);
            assert!(
                long.starts_with(&text),
                "the kept part is a prefix of the real value"
            );
        }
        other => panic!("expected Text, got {other:?}"),
    }

    // And a value that fits is not marked.
    match format_cell(Cell::Text("short".into())) {
        SqlValue::Text { text, truncated } => {
            assert_eq!(text, "short");
            assert!(!truncated);
        }
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn an_undecodable_type_is_unsupported_not_blank() {
    let value = format_cell(Cell::Unsupported {
        type_name: "jsonb".into(),
    });

    assert_eq!(
        value,
        SqlValue::Unsupported {
            type_name: "jsonb".into()
        }
    );
    assert_ne!(value, SqlValue::Null);
    assert_ne!(
        value,
        SqlValue::Text {
            text: String::new(),
            truncated: false
        },
        "an undecodable type must never render as a blank cell"
    );
}

#[test]
fn a_cell_the_driver_could_not_read_is_unavailable_not_null() {
    let value = format_cell(Cell::Error {
        reason: "column read failed".into(),
    });

    assert_eq!(
        value,
        SqlValue::Unavailable {
            reason: "column read failed".into()
        }
    );
    assert_ne!(value, SqlValue::Null, "a failed read is not a NULL");
    assert_ne!(
        value,
        SqlValue::Unsupported {
            type_name: "column read failed".into()
        },
        "a failed read is not an unsupported type"
    );
}

#[test]
fn a_byte_column_crosses_as_hex_with_its_true_length() {
    match format_cell(Cell::Bytes(vec![0x00, 0xde, 0xad, 0xff])) {
        SqlValue::Bytes {
            hex,
            byte_length,
            truncated,
        } => {
            assert_eq!(hex, "00deadff");
            assert_eq!(byte_length, 4);
            assert!(!truncated);
        }
        other => panic!("expected Bytes, got {other:?}"),
    }

    let big = vec![0xabu8; MAX_BYTES_RENDERED + 77];
    match format_cell(Cell::Bytes(big)) {
        SqlValue::Bytes {
            hex,
            byte_length,
            truncated,
        } => {
            assert!(truncated);
            assert_eq!(
                hex.len(),
                MAX_BYTES_RENDERED * 2,
                "two hex chars per rendered byte"
            );
            assert_eq!(
                byte_length,
                (MAX_BYTES_RENDERED + 77) as u64,
                "the true size of the blob, not the size of what was rendered"
            );
        }
        other => panic!("expected Bytes, got {other:?}"),
    }
}

#[test]
fn truncation_never_splits_a_utf8_character() {
    // Four-byte characters: a byte-indexed cut lands mid-character and would
    // panic (or, in a `from_utf8_lossy` implementation, produce mojibake).
    // The one-byte prefix is deliberate: without it the byte budget happens to
    // be a multiple of four and lands on a boundary by luck, which would let a
    // byte-slicing implementation pass.
    let wide: String = format!("x{}", "𝄞".repeat(MAX_TEXT_BYTES)); // 4 bytes each
    let value = format_cell(Cell::Text(wide.clone()));

    match value {
        SqlValue::Text { text, truncated } => {
            assert!(truncated);
            assert!(
                text.len() <= MAX_TEXT_BYTES,
                "the byte budget bounds the payload"
            );
            assert!(
                text.chars().all(|c| c == 'x' || c == '𝄞'),
                "no replacement character: truncation cut on a character boundary"
            );
            assert!(wide.starts_with(&text));
        }
        other => panic!("expected Text, got {other:?}"),
    }

    // A char-capped string of wide characters must also stay whole.
    let mixed = format!("héllo!{}", "é".repeat(MAX_TEXT_CHARS));
    match format_cell(Cell::Text(mixed.clone())) {
        SqlValue::Text { text, truncated } => {
            assert!(truncated);
            assert!(mixed.starts_with(&text));
            assert!(!text.contains('\u{fffd}'));
        }
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn numbers_cross_as_strings() {
    assert_eq!(
        format_cell(Cell::Int(9_007_199_254_740_993)),
        SqlValue::Number {
            text: "9007199254740993".into()
        },
        "an integer past 2^53 must not be rounded by JSON"
    );
    assert_eq!(
        format_cell(Cell::Numeric("0.10".into())),
        SqlValue::Number {
            text: "0.10".into()
        },
        "a decimal keeps the scale the server reported"
    );
}
