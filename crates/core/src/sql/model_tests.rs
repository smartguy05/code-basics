//! The wire shape of everything the SQL console sends the frontend.
//!
//! Like `crate::lsp::model_tests`, these tests are about **keys and strings**,
//! not values: `src/ipc/types.ts` is written by hand against these names, so a
//! rename on the Rust side has to fail here rather than surface as `undefined`
//! in a results grid.
//!
//! Three of them pin rules rather than spellings, and those are the ones not to
//! "tidy": null-versus-empty-versus-truncated, numbers crossing as strings, and
//! the absence of `skip_serializing_if`.

use serde_json::{json, Value};

use super::*;

/// Every key of a JSON object, sorted.
fn keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .expect("expected a JSON object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// A key that is present and explicitly `null` — the thing the
/// no-`skip_serializing_if` rule exists to guarantee.
fn present_and_null(value: &Value, key: &str) {
    let object = value.as_object().expect("expected a JSON object");
    assert!(
        object.contains_key(key),
        "`{key}` must be present even when it has no value"
    );
    assert_eq!(
        object.get(key),
        Some(&Value::Null),
        "`{key}` must cross as an explicit null"
    );
}

fn result_set() -> SqlResultSet {
    SqlResultSet {
        columns: vec![SqlColumn {
            name: "total".into(),
            type_name: Some("numeric".into()),
        }],
        rows: vec![vec![SqlValue::Number {
            text: "12450.00".into(),
        }]],
        row_cap: None,
        rows_affected: None,
        elapsed_ms: 7,
        statement_index: 0,
    }
}

// ---------------------------------------------------------------------------
// The three rules
// ---------------------------------------------------------------------------

#[test]
fn null_empty_string_and_truncated_are_three_different_shapes() {
    let null = serde_json::to_value(SqlValue::Null).unwrap();
    let empty = serde_json::to_value(SqlValue::Text {
        text: String::new(),
        truncated: false,
    })
    .unwrap();
    let truncated = serde_json::to_value(SqlValue::Text {
        text: "abc".into(),
        truncated: true,
    })
    .unwrap();

    assert_eq!(null, json!({ "kind": "null" }));
    assert_eq!(
        empty,
        json!({ "kind": "text", "text": "", "truncated": false })
    );
    assert_eq!(
        truncated,
        json!({ "kind": "text", "text": "abc", "truncated": true })
    );

    // The point of the rule: no two of them can be confused for each other.
    assert_ne!(null, empty);
    assert_ne!(empty, truncated);
    assert_ne!(null, truncated);

    // And `truncated` is never omitted when false, or an untruncated empty
    // string would be indistinguishable from one the driver cut off.
    assert!(empty.as_object().unwrap().contains_key("truncated"));
}

#[test]
fn a_big_integer_crosses_as_a_string_not_a_number() {
    // 2^63 - 1: exact in Rust, silently rounded by a JavaScript number.
    let text = "9223372036854775807";
    let value = serde_json::to_value(SqlValue::Number { text: text.into() }).unwrap();

    assert_eq!(value, json!({ "kind": "number", "text": text }));
    assert!(
        value["text"].is_string(),
        "a number must cross as a string; a JSON number loses precision"
    );

    // The exactness the rule buys, spelled out: through an f64 this value is
    // not itself, and a console that renders the rounded one is confidently
    // wrong.
    assert_ne!(
        format!("{}", text.parse::<f64>().unwrap()),
        text.to_string(),
        "if this ever passes, the f64 round trip is lossless and the comment \
         below is wrong — it is not"
    );

    // NUMERIC(38,10) does not fit in any Rust integer either — the string is
    // the only lossless carrier.
    let wide = "12345678901234567890123456789.0123456789";
    let value = serde_json::to_value(SqlValue::Number { text: wide.into() }).unwrap();
    assert_eq!(value["text"], json!(wide));
}

#[test]
fn no_field_whose_absence_means_something_is_skip_serialized() {
    // The rule, at the source: not one `skip_serializing_if` attribute in this
    // module. Comment lines are stripped rather than matched — the module docs
    // state the rule by name, and the rule is about the attribute.
    let offenders: Vec<&str> = include_str!("model.rs")
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .filter(|line| line.contains("skip_serializing_if"))
        .collect();
    assert!(
        offenders.is_empty(),
        "`skip_serializing_if` makes \"no answer\" indistinguishable from \
         \"forgot to send one\" — see the module docs. Found: {offenders:?}"
    );

    // And what that guarantees on the wire.
    let value = serde_json::to_value(result_set()).unwrap();
    present_and_null(&value, "rowCap");
    present_and_null(&value, "rowsAffected");

    let column = serde_json::to_value(SqlColumn {
        name: "id".into(),
        type_name: None,
    })
    .unwrap();
    present_and_null(&column, "typeName");
}

#[test]
fn rows_affected_none_and_some_zero_stay_distinct() {
    let no_count = SqlResultSet {
        rows_affected: None,
        ..result_set()
    };
    let matched_nothing = SqlResultSet {
        rows_affected: Some(0),
        ..result_set()
    };

    let no_count = serde_json::to_value(no_count).unwrap();
    let matched_nothing = serde_json::to_value(matched_nothing).unwrap();

    assert_eq!(no_count["rowsAffected"], Value::Null);
    assert_eq!(matched_nothing["rowsAffected"], json!(0));
    assert_ne!(no_count["rowsAffected"], matched_nothing["rowsAffected"]);
}

#[test]
fn an_absent_row_cap_means_every_row_and_a_present_one_reports_itself() {
    let complete = serde_json::to_value(result_set()).unwrap();
    assert_eq!(complete["rowCap"], Value::Null);

    let capped = SqlResultSet {
        row_cap: Some(RowCap {
            limit: 500,
            reason: RowCapReason::ByteLimit,
        }),
        ..result_set()
    };
    let capped = serde_json::to_value(capped).unwrap();
    assert_eq!(
        capped["rowCap"],
        json!({ "limit": 500, "reason": "byteLimit" })
    );
}

#[test]
fn a_null_is_known_and_the_two_admissions_of_ignorance_are_not() {
    // `Null` is an answer the database gave, so it counts as known.
    assert!(SqlValue::Null.is_known());
    assert!(SqlValue::Text {
        text: String::new(),
        truncated: false,
    }
    .is_known());
    assert!(SqlValue::Number { text: "0".into() }.is_known());
    assert!(SqlValue::Bool { value: false }.is_known());
    assert!(SqlValue::Bytes {
        hex: String::new(),
        byte_length: 0,
        truncated: false,
    }
    .is_known());

    assert!(!SqlValue::Unsupported {
        type_name: "geography".into()
    }
    .is_known());
    assert!(!SqlValue::Unavailable { reason: "r".into() }.is_known());
}

#[test]
fn a_result_set_is_complete_exactly_when_no_cap_bit() {
    assert!(result_set().is_complete());
    assert!(!SqlResultSet {
        row_cap: Some(RowCap {
            limit: 1,
            reason: RowCapReason::RowLimit,
        }),
        ..result_set()
    }
    .is_complete());
}

// ---------------------------------------------------------------------------
// Spellings
// ---------------------------------------------------------------------------

#[test]
fn every_sql_value_variant_serialises_with_the_keys_the_ui_reads() {
    let cases: Vec<(SqlValue, Vec<&str>)> = vec![
        (SqlValue::Null, vec!["kind"]),
        (
            SqlValue::Text {
                text: "a".into(),
                truncated: false,
            },
            vec!["kind", "text", "truncated"],
        ),
        (SqlValue::Number { text: "1".into() }, vec!["kind", "text"]),
        (SqlValue::Bool { value: true }, vec!["kind", "value"]),
        (
            SqlValue::Bytes {
                hex: "0a1b".into(),
                byte_length: 2,
                truncated: false,
            },
            vec!["byteLength", "hex", "kind", "truncated"],
        ),
        (
            SqlValue::Unsupported {
                type_name: "geography".into(),
            },
            vec!["kind", "typeName"],
        ),
        (
            SqlValue::Unavailable {
                reason: "the driver could not decode this cell".into(),
            },
            vec!["kind", "reason"],
        ),
    ];

    for (value, expected) in cases {
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(keys(&json), expected, "keys of {value:?}");
        assert_eq!(
            serde_json::from_value::<SqlValue>(json).unwrap(),
            value,
            "round trip of {value:?}"
        );
    }
}

#[test]
fn every_sql_value_kind_serialises_to_its_exact_string() {
    let pairs = [
        (SqlValue::Null, "null"),
        (
            SqlValue::Text {
                text: String::new(),
                truncated: false,
            },
            "text",
        ),
        (SqlValue::Number { text: "1".into() }, "number"),
        (SqlValue::Bool { value: false }, "bool"),
        (
            SqlValue::Bytes {
                hex: String::new(),
                byte_length: 0,
                truncated: false,
            },
            "bytes",
        ),
        (
            SqlValue::Unsupported {
                type_name: "t".into(),
            },
            "unsupported",
        ),
        (SqlValue::Unavailable { reason: "r".into() }, "unavailable"),
    ];

    for (variant, kind) in pairs {
        assert_eq!(serde_json::to_value(variant).unwrap()["kind"], json!(kind));
    }
}

#[test]
fn unsupported_and_unavailable_are_different_answers() {
    let unsupported = serde_json::to_value(SqlValue::Unsupported {
        type_name: "geography".into(),
    })
    .unwrap();
    let unavailable = serde_json::to_value(SqlValue::Unavailable {
        reason: "geography".into(),
    })
    .unwrap();

    // "this build cannot decode this type" and "the driver errored on this
    // cell" must never render as one shrug.
    assert_ne!(unsupported["kind"], unavailable["kind"]);
    assert_ne!(keys(&unsupported), keys(&unavailable));
}

#[test]
fn row_cap_reasons_serialise_to_their_exact_strings() {
    let pairs = [
        (RowCapReason::RowLimit, "rowLimit"),
        (RowCapReason::ByteLimit, "byteLimit"),
    ];
    for (variant, text) in pairs {
        assert_eq!(serde_json::to_value(variant).unwrap(), json!(text));
        assert_eq!(
            serde_json::from_value::<RowCapReason>(json!(text)).unwrap(),
            variant
        );
    }
}

#[test]
fn result_set_and_column_serialise_with_the_keys_the_ui_reads() {
    assert_eq!(
        keys(&serde_json::to_value(result_set()).unwrap()),
        [
            "columns",
            "elapsedMs",
            "rowCap",
            "rows",
            "rowsAffected",
            "statementIndex"
        ]
    );
    assert_eq!(
        keys(
            &serde_json::to_value(SqlColumn {
                name: "id".into(),
                type_name: None
            })
            .unwrap()
        ),
        ["name", "typeName"]
    );
}

#[test]
fn a_result_set_round_trips() {
    let set = SqlResultSet {
        columns: vec![
            SqlColumn {
                name: "id".into(),
                type_name: Some("bigint".into()),
            },
            SqlColumn {
                name: "note".into(),
                type_name: None,
            },
        ],
        rows: vec![
            vec![
                SqlValue::Number {
                    text: "9223372036854775807".into(),
                },
                SqlValue::Null,
            ],
            vec![
                SqlValue::Number { text: "2".into() },
                SqlValue::Text {
                    text: String::new(),
                    truncated: false,
                },
            ],
        ],
        row_cap: Some(RowCap {
            limit: 2,
            reason: RowCapReason::RowLimit,
        }),
        rows_affected: Some(0),
        elapsed_ms: 12,
        statement_index: 3,
    };

    let json = serde_json::to_value(&set).unwrap();
    assert_eq!(serde_json::from_value::<SqlResultSet>(json).unwrap(), set);
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[test]
fn every_sql_event_kind_serialises_to_its_exact_string() {
    let pairs: Vec<(SqlEvent, &str, Vec<&str>)> = vec![
        (
            SqlEvent::Started { statement_index: 0 },
            "started",
            vec!["kind", "statementIndex"],
        ),
        (
            SqlEvent::Columns {
                statement_index: 0,
                columns: Vec::new(),
            },
            "columns",
            vec!["columns", "kind", "statementIndex"],
        ),
        (
            SqlEvent::Rows {
                statement_index: 0,
                rows: Vec::new(),
            },
            "rows",
            vec!["kind", "rows", "statementIndex"],
        ),
        (
            SqlEvent::Completed {
                completion: result_set().completion(),
            },
            "completed",
            vec!["completion", "kind"],
        ),
        (
            SqlEvent::Notice {
                statement_index: 0,
                message: "this is a write, and writes are allowed here".into(),
            },
            "notice",
            vec!["kind", "message", "statementIndex"],
        ),
        (
            SqlEvent::Refused {
                statement_index: 1,
                reason: "refused by a text heuristic".into(),
            },
            "refused",
            vec!["kind", "reason", "statementIndex"],
        ),
        (
            SqlEvent::Failed {
                statement_index: None,
                message: "could not connect".into(),
            },
            "failed",
            vec!["kind", "message", "statementIndex"],
        ),
        (
            SqlEvent::Finished { cancelled: false },
            "finished",
            vec!["cancelled", "kind"],
        ),
    ];

    for (event, kind, expected) in pairs {
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], json!(kind));
        assert_eq!(keys(&json), expected, "keys of {event:?}");
        assert_eq!(
            serde_json::from_value::<SqlEvent>(json).unwrap(),
            event,
            "round trip of {event:?}"
        );
    }
}

#[test]
fn a_failure_before_any_statement_keeps_its_statement_index_present_and_null() {
    let before_any = serde_json::to_value(SqlEvent::Failed {
        statement_index: None,
        message: "could not connect".into(),
    })
    .unwrap();
    present_and_null(&before_any, "statementIndex");

    let during = serde_json::to_value(SqlEvent::Failed {
        statement_index: Some(0),
        message: "syntax error".into(),
    })
    .unwrap();
    assert_eq!(during["statementIndex"], json!(0));
}

#[test]
fn refused_failed_and_a_cancelled_finish_are_three_different_events() {
    let refused = serde_json::to_value(SqlEvent::Refused {
        statement_index: 0,
        reason: "r".into(),
    })
    .unwrap();
    let failed = serde_json::to_value(SqlEvent::Failed {
        statement_index: Some(0),
        message: "r".into(),
    })
    .unwrap();
    let cancelled = serde_json::to_value(SqlEvent::Finished { cancelled: true }).unwrap();
    let ran = serde_json::to_value(SqlEvent::Finished { cancelled: false }).unwrap();

    assert_ne!(refused["kind"], failed["kind"]);
    assert_ne!(failed["kind"], cancelled["kind"]);
    assert_ne!(cancelled["cancelled"], ran["cancelled"]);
}

// ---------------------------------------------------------------------------
// The completion carries no rows
// ---------------------------------------------------------------------------

#[test]
fn a_completion_reports_the_row_count_and_never_the_rows() {
    // The rows crossed once already, in `SqlEvent::Rows`. A completion that
    // repeated them sent every result set across IPC twice.
    let set = result_set();
    let completion = set.completion();
    assert_eq!(completion.row_count, set.rows.len() as u64);

    let json = serde_json::to_value(SqlEvent::Completed { completion }).unwrap();
    assert_eq!(
        keys(&json["completion"]),
        vec![
            "elapsedMs",
            "rowCap",
            "rowCount",
            "rowsAffected",
            "statementIndex",
        ],
        "a completion must carry no `rows` and no `columns`"
    );
    assert!(
        !serde_json::to_string(&json).unwrap().contains("12450.00"),
        "a cell reached the frontend a second time inside the completion: {json}"
    );
}

#[test]
fn a_completion_keeps_every_absent_answer_present_and_null() {
    let mut set = result_set();
    set.row_cap = None;
    set.rows_affected = None;
    let json = serde_json::to_value(set.completion()).unwrap();
    present_and_null(&json, "rowCap");
    present_and_null(&json, "rowsAffected");
}

#[test]
fn a_capped_completion_still_names_the_cap_that_bit() {
    // `row_cap`'s presence *is* the truncation report, and it is the one thing
    // the row stream cannot say for itself.
    let mut set = result_set();
    set.row_cap = Some(RowCap {
        limit: 1,
        reason: RowCapReason::ByteLimit,
    });
    assert_eq!(
        set.completion().row_cap,
        Some(RowCap {
            limit: 1,
            reason: RowCapReason::ByteLimit,
        })
    );
}
