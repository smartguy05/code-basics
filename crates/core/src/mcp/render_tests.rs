use super::*;

use crate::sql::catalog::SqlObjectKind;
use crate::sql::driver::{declared_read_only_enforcement, ENGINES};
use crate::sql::dsn::SqlEngine;
use crate::sql::model::SqlColumn as ResultColumn;

fn set(rows: Vec<Vec<SqlValue>>, cap: Option<RowCap>) -> SqlResultSet {
    SqlResultSet {
        columns: vec![
            ResultColumn {
                name: "id".into(),
                type_name: Some("int".into()),
            },
            ResultColumn {
                name: "name".into(),
                type_name: Some("text".into()),
            },
        ],
        rows,
        row_cap: cap,
        rows_affected: None,
        elapsed_ms: 4,
        statement_index: 0,
    }
}

fn text(s: &str) -> SqlValue {
    SqlValue::Text {
        text: s.into(),
        truncated: false,
    }
}

// ---------------------------------------------------------------------------
// Caps
// ---------------------------------------------------------------------------

#[test]
fn a_capped_result_renders_its_cap_first() {
    // A model that reads the rows and only then a footnote has already drawn
    // its conclusion.
    let rendered = result(&set(
        vec![vec![SqlValue::Number { text: "1".into() }, text("a")]],
        Some(RowCap {
            limit: 1,
            reason: RowCapReason::RowLimit,
        }),
    ));
    let cap_at = rendered.find("INCOMPLETE").expect("a cap notice");
    let first_row_at = rendered.find("\"id\"").expect("a row");
    assert!(cap_at < first_row_at, "the cap must lead, got:\n{rendered}");
    assert!(rendered.starts_with("INCOMPLETE"));
}

#[test]
fn a_row_cap_and_a_byte_cap_do_not_render_the_same() {
    let rows = cap_notice(RowCap {
        limit: 500,
        reason: RowCapReason::RowLimit,
    });
    let bytes = cap_notice(RowCap {
        limit: 500,
        reason: RowCapReason::ByteLimit,
    });
    assert_ne!(rows, bytes);
    assert!(bytes.contains("Raising a row limit would not return more rows"));
    assert!(!rows.contains("would not return more rows"));
}

#[test]
fn an_uncapped_empty_result_says_the_query_matched_no_rows_and_is_complete() {
    // The one implementations get wrong: silence reads as a failure.
    let rendered = result(&set(vec![], None));
    assert_eq!(rendered, NO_ROWS);
    assert!(rendered.contains("complete answer"));
    assert!(!rendered.contains("INCOMPLETE"));
}

#[test]
fn a_capped_empty_result_is_not_the_no_rows_answer() {
    let rendered = result(&set(
        vec![],
        Some(RowCap {
            limit: 0,
            reason: RowCapReason::ByteLimit,
        }),
    ));
    assert!(rendered.starts_with("INCOMPLETE"));
    assert!(!rendered.contains(NO_ROWS));
}

#[test]
fn a_populated_result_lists_its_rows_as_json_keyed_by_column() {
    let rendered = result(&set(
        vec![
            vec![SqlValue::Number { text: "1".into() }, text("ada")],
            vec![SqlValue::Number { text: "2".into() }, SqlValue::Null],
        ],
        None,
    ));
    assert!(rendered.contains("2 rows"));
    assert!(
        rendered.contains(r#"{"id":"1","name":"ada"}"#),
        "{rendered}"
    );
    assert!(rendered.contains(r#"{"id":"2","name":null}"#), "{rendered}");
}

// ---------------------------------------------------------------------------
// Cells
// ---------------------------------------------------------------------------

#[test]
fn a_number_crosses_as_a_string_so_a_wide_numeric_keeps_its_digits() {
    let rendered = value(&SqlValue::Number {
        text: "12345678901234567890.1234567890".into(),
    });
    assert_eq!(
        rendered,
        Value::String("12345678901234567890.1234567890".into())
    );
}

#[test]
fn an_unreadable_cell_is_not_rendered_as_null() {
    // `null` already means "the database said there is no value here".
    assert_eq!(value(&SqlValue::Null), Value::Null);
    for unreadable in [
        SqlValue::Unsupported {
            type_name: "geography".into(),
        },
        SqlValue::Unavailable {
            reason: "the driver failed".into(),
        },
    ] {
        let rendered = value(&unreadable);
        assert_ne!(rendered, Value::Null, "{unreadable:?} collapsed into null");
        assert!(rendered.get("unreadable").is_some(), "{rendered}");
    }
}

#[test]
fn a_truncated_string_does_not_read_as_a_complete_one() {
    let complete = value(&text("abc"));
    let cut = value(&SqlValue::Text {
        text: "abc".into(),
        truncated: true,
    });
    assert_ne!(complete, cut);
    assert_eq!(cut.get("truncated"), Some(&Value::Bool(true)));
}

// ---------------------------------------------------------------------------
// The honesty surface
// ---------------------------------------------------------------------------

#[test]
fn the_sql_server_rendering_never_says_a_write_is_refused_before_it_runs() {
    // For SQL Server the write executes and is rolled back. An agent reading
    // "read-only" as "cannot write" is being told something untrue.
    let rendered = enforcement(declared_read_only_enforcement(SqlEngine::SqlServer));
    assert!(
        !rendered.contains("before it runs"),
        "the rolled-back mechanism was described as a refusal:\n{rendered}"
    );
    assert!(rendered.contains("undone"), "{rendered}");
    assert!(rendered.contains("not a sandbox"), "{rendered}");
}

#[test]
fn the_two_mechanisms_that_do_refuse_say_so() {
    for engine in [SqlEngine::Sqlite, SqlEngine::Postgres] {
        let rendered = enforcement(declared_read_only_enforcement(engine));
        assert!(
            rendered.contains("before it runs"),
            "{engine:?} did not say the write is refused before it runs:\n{rendered}"
        );
    }
}

#[test]
fn every_engine_has_its_own_enforcement_rendering() {
    let mut rendered: Vec<String> = ENGINES
        .into_iter()
        .map(|engine| enforcement(declared_read_only_enforcement(engine)))
        .collect();
    rendered.sort();
    let mut unique = rendered.clone();
    unique.dedup();
    assert_eq!(
        rendered.len(),
        unique.len(),
        "two engines describe their enforcement identically"
    );
}

#[test]
fn refused_and_undone_are_not_ranked_against_each_other_in_words_either() {
    assert_ne!(
        strength(EnforcementStrength::Refused),
        strength(EnforcementStrength::Undone)
    );
    assert!(!strength(EnforcementStrength::Undone).contains("before it runs"));
    assert!(strength(EnforcementStrength::Undone).contains("not stopped"));
}

// ---------------------------------------------------------------------------
// Listings
// ---------------------------------------------------------------------------

#[test]
fn a_scoped_listing_says_what_it_left_out_and_that_the_scope_is_not_a_boundary() {
    let listed = ExposedListing {
        connections: vec![super::super::expose::AgentConnection {
            id: "conn-1".into(),
            name: "Reporting".into(),
            engine: Some(SqlEngine::Postgres),
        }],
        scoped_out: 2,
    };
    let rendered = connections(&listed);
    assert!(rendered.contains("conn-1") && rendered.contains("PostgreSQL"));
    assert!(rendered.contains("not a security boundary"), "{rendered}");
    assert!(rendered.contains('2'));
}

#[test]
fn an_unscoped_listing_adds_no_caveat() {
    let listed = ExposedListing {
        connections: vec![super::super::expose::AgentConnection {
            id: "conn-1".into(),
            name: "Reporting".into(),
            engine: None,
        }],
        scoped_out: 0,
    };
    let rendered = connections(&listed);
    assert!(!rendered.contains("security boundary"));
    assert!(
        rendered.contains("engine not determined"),
        "an undetermined engine must say so rather than be blank: {rendered}"
    );
}

#[test]
fn an_empty_table_list_says_it_is_complete_rather_than_looking_like_a_failure() {
    assert!(tables(&[]).contains("complete answer"));
}

#[test]
fn a_table_is_qualified_by_its_schema_when_it_has_one() {
    let objects = vec![
        SqlObject {
            kind: SqlObjectKind::Table,
            schema: Some("sales".into()),
            name: "orders".into(),
        },
        SqlObject {
            kind: SqlObjectKind::Table,
            schema: None,
            name: "migrations".into(),
        },
    ];
    let rendered = tables(&objects);
    assert!(rendered.contains("sales.orders"));
    assert!(rendered.contains("migrations"));
    assert!(!rendered.contains(".migrations"));
}

#[test]
fn a_fact_the_engine_did_not_report_is_not_rendered_as_a_no() {
    let column = CatalogColumn {
        name: "total".into(),
        data_type: "numeric".into(),
        nullable: None,
        default_value: None,
        ordinal: 3,
        max_length: None,
        numeric_precision: None,
        numeric_scale: None,
        primary_key: None,
    };
    let rendered = columns(std::slice::from_ref(&column));
    assert!(
        rendered.contains("nullability not reported"),
        "an unreported nullability must not read as `not null`: {rendered}"
    );
    assert!(!rendered.contains("not null"));
    assert!(!rendered.contains("primary key"));
}

#[test]
fn a_reported_fact_is_rendered() {
    let column = CatalogColumn {
        name: "id".into(),
        data_type: "int".into(),
        nullable: Some(false),
        default_value: Some("nextval('s')".into()),
        ordinal: 1,
        max_length: None,
        numeric_precision: Some(10),
        numeric_scale: Some(0),
        primary_key: Some(true),
    };
    let rendered = columns(std::slice::from_ref(&column));
    assert!(rendered.contains("not null"));
    assert!(rendered.contains("primary key"));
    assert!(rendered.contains("numeric(10,0)"));
    assert!(rendered.contains("default nextval('s')"));
}

#[test]
fn an_empty_column_list_does_not_claim_the_table_is_missing() {
    let rendered = columns(&[]);
    assert!(rendered.contains("cannot tell those apart"), "{rendered}");
}

#[test]
fn a_status_names_its_kind_and_carries_no_driver_text() {
    for kind in ConnectionStatusKind::ALL {
        let rendered = status(kind);
        assert!(rendered.starts_with(kind.name()));
        assert!(rendered.contains(kind.detail()));
    }
}

#[test]
fn a_cell_the_driver_could_not_decode_admits_it_without_quoting_the_driver() {
    // `Cell::Error`'s reason comes from `sqlx`'s own `e.to_string()`, and a
    // driver error routinely embeds the whole connection string. This module's
    // rule is that driver words are dropped rather than filtered, because
    // `dsn::redact` is a keyword heuristic and the reader here pastes what it is
    // given into a transcript, a commit and a model provider's logs.
    let rendered = value(&SqlValue::Unavailable {
        reason: "error returned from database: password authentication failed for \
                 Server=db;Uid=sa;Password=DSN-SENTINEL"
            .into(),
    });

    let json = serde_json::to_string(&rendered).expect("serialises");
    assert!(
        !json.contains("DSN-SENTINEL"),
        "driver text reached the agent: {json}"
    );
    assert!(
        !json.contains("password"),
        "driver text reached the agent: {json}"
    );
    assert!(
        json.contains("unreadable"),
        "the cell must still admit it is unreadable: {json}"
    );
}

#[test]
fn an_unsupported_type_still_names_the_type_because_that_is_ours_and_not_the_drivers() {
    // The contrast that keeps the rule from reading as blanket secrecy: a type
    // name is the schema's own word, which this app read out of the catalog, so
    // there is nothing to leak and it is genuinely useful.
    let rendered = value(&SqlValue::Unsupported {
        type_name: "geography".into(),
    });
    let json = serde_json::to_string(&rendered).expect("serialises");
    assert!(json.contains("geography"), "{json}");
}
