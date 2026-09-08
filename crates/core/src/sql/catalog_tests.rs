use super::*;

use crate::sql::driver;
use crate::sql::model::SqlColumn as ResultColumn;

fn text(value: &str) -> SqlValue {
    SqlValue::Text {
        text: value.into(),
        truncated: false,
    }
}

fn result_with(columns: Vec<&str>, rows: Vec<Vec<SqlValue>>) -> SqlResultSet {
    SqlResultSet {
        columns: columns
            .into_iter()
            .map(|name| ResultColumn {
                name: name.into(),
                type_name: None,
            })
            .collect(),
        rows,
        row_cap: None,
        rows_affected: None,
        elapsed_ms: 1,
        statement_index: 0,
    }
}

// ---------------------------------------------------------------------------
// The queries (moved from src-tauri/src/commands/sql.rs)
// ---------------------------------------------------------------------------

#[test]
fn table_catalog_rows_keep_schema_and_name_separate() {
    let result = result_with(
        vec!["table_schema", "table_name"],
        vec![vec![text("sales"), text("orders")]],
    );

    assert_eq!(
        table_objects(&result).unwrap(),
        vec![SqlObject {
            kind: SqlObjectKind::Table,
            schema: Some("sales".into()),
            name: "orders".into(),
        }]
    );
}

#[test]
fn every_engine_has_a_table_catalog_query() {
    for engine in driver::ENGINES {
        let query = object_catalog_query(engine);
        assert!(
            query.to_ascii_lowercase().contains("table"),
            "{engine:?}: {query}"
        );
        assert!(
            query.to_ascii_lowercase().starts_with("select"),
            "{engine:?}: {query}"
        );
    }
}

#[test]
fn column_catalog_literals_are_escaped_for_every_engine() {
    for engine in driver::ENGINES {
        let query = column_catalog_query(engine, Some("odd'schema"), "order'items");
        assert!(query.contains("order''items"), "{engine:?}: {query}");
        assert!(!query.contains("order'items"), "{engine:?}: {query}");
    }
}

#[test]
fn column_catalog_rows_normalize_details() {
    let result = result_with(
        Vec::new(),
        vec![vec![
            text("amount"),
            text("numeric"),
            text("NO"),
            SqlValue::Null,
            text("3"),
            text(""),
            text("18"),
            text("2"),
            text("YES"),
        ]],
    );

    assert_eq!(
        table_columns(&result).unwrap(),
        vec![SqlColumn {
            name: "amount".into(),
            data_type: "numeric".into(),
            nullable: Some(false),
            default_value: None,
            ordinal: 3,
            max_length: None,
            numeric_precision: Some(18),
            numeric_scale: Some(2),
            primary_key: Some(true),
        }]
    );
}

// ---------------------------------------------------------------------------
// The wire keys, re-pinned in their new home
// ---------------------------------------------------------------------------

#[test]
fn an_object_serialises_with_the_keys_the_ui_reads() {
    let value = serde_json::to_value(SqlObject {
        kind: SqlObjectKind::Table,
        schema: None,
        name: "orders".into(),
    })
    .unwrap();

    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["kind", "name", "schema"]);
    // An absent schema crosses as an explicit null, never as a missing key.
    assert!(value.get("schema").unwrap().is_null());
    assert_eq!(value["kind"], "table");
}

#[test]
fn a_column_serialises_with_the_keys_the_ui_reads() {
    let value = serde_json::to_value(SqlColumn {
        name: "amount".into(),
        data_type: "numeric".into(),
        nullable: None,
        default_value: None,
        ordinal: 1,
        max_length: None,
        numeric_precision: None,
        numeric_scale: None,
        primary_key: None,
    })
    .unwrap();

    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "dataType",
            "defaultValue",
            "maxLength",
            "name",
            "nullable",
            "numericPrecision",
            "numericScale",
            "ordinal",
            "primaryKey",
        ]
    );
    for key in ["nullable", "defaultValue", "maxLength", "primaryKey"] {
        assert!(
            value.get(key).unwrap().is_null(),
            "{key} must cross as an explicit null"
        );
    }
}

// ---------------------------------------------------------------------------
// Identifier acceptance
// ---------------------------------------------------------------------------

#[test]
fn an_ordinary_identifier_is_accepted_unchanged() {
    for name in ["orders", "Order_Items", "ventes_françaises", "dbo", "t1"] {
        assert_eq!(identifier(name), Ok(name), "{name} must be accepted");
    }
}

/// The refusal has to say *which* rule bit, or the reader cannot tell a name
/// that is merely too long from one carrying an injection hazard.
#[test]
fn each_refused_identifier_names_the_rule_it_broke() {
    let long = "t".repeat(MAX_IDENTIFIER_BYTES + 1);
    let cases: Vec<(String, IdentifierRefusal)> = vec![
        (String::new(), IdentifierRefusal::Empty),
        (
            long.clone(),
            IdentifierRefusal::TooLong {
                bytes: MAX_IDENTIFIER_BYTES + 1,
            },
        ),
        ("or\0ders".into(), IdentifierRefusal::Nul),
        ("o'ders".into(), IdentifierRefusal::Quote),
        ("o\\ders".into(), IdentifierRefusal::Backslash),
        ("or\nders".into(), IdentifierRefusal::Newline),
        ("or\rders".into(), IdentifierRefusal::Newline),
        ("or\tders".into(), IdentifierRefusal::Control { code: 9 }),
    ];

    let mut sentences = Vec::new();
    for (name, expected) in cases {
        let refusal = identifier(&name).expect_err(&format!("{name:?} must be refused"));
        assert_eq!(refusal, expected, "for {name:?}");
        let sentence = refusal.sentence();
        assert!(!sentence.trim().is_empty(), "for {name:?}");
        sentences.push(sentence);
    }

    // Every rule reads differently: a shared sentence would collapse the
    // variants back into "invalid identifier" at the only place a reader looks.
    let mut unique = sentences.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        sentences.len() - 1,
        "only the two newline spellings may share a sentence: {sentences:?}"
    );
}

/// The backslash rule is the one that is easy to talk out of, because doubling
/// the quote *looks* sufficient. It is not, under PostgreSQL's
/// `standard_conforming_strings = off`, where `\'` escapes the escape.
#[test]
fn a_backslash_is_refused_even_though_the_quote_would_be_doubled() {
    let hostile = "orders\\'; drop table users; --";
    assert_eq!(identifier(hostile), Err(IdentifierRefusal::Backslash));
    // ...and the second layer really would have doubled it, which is exactly
    // why the first layer has to exist.
    assert!(sql_literal(hostile).contains("\\''"));
}

#[test]
fn an_identifier_at_the_length_limit_is_accepted_and_one_past_it_is_not() {
    let at = "t".repeat(MAX_IDENTIFIER_BYTES);
    assert!(identifier(&at).is_ok());
    let past = "t".repeat(MAX_IDENTIFIER_BYTES + 1);
    assert!(matches!(
        identifier(&past),
        Err(IdentifierRefusal::TooLong { .. })
    ));
}

/// The limit is bytes, not characters, because it is the wire that has the
/// limit — a multi-byte name must not slip past by counting short.
#[test]
fn the_length_limit_counts_bytes_and_not_characters() {
    let name = "é".repeat(MAX_IDENTIFIER_BYTES / 2 + 1);
    assert!(name.chars().count() <= MAX_IDENTIFIER_BYTES);
    assert!(matches!(
        identifier(&name),
        Err(IdentifierRefusal::TooLong { .. })
    ));
}

// ---------------------------------------------------------------------------
// An omitted schema means EVERY schema, not the schema whose name is "".
//
// `schema.unwrap_or("")` fed an equality predicate, so `None` asked for the
// schema literally named "" — which no server has. The tool description tells
// the model "omit it to search every schema", and `tools_tests.rs` has a test
// named `an_omitted_schema_means_every_schema_and_an_empty_one_does_not` that
// only checks argument parsing and never follows `None` into the query. So the
// query returned nothing and the renderer reported the table as possibly
// absent: a confident wrong answer, which is the one thing this subsystem is
// built to avoid.
// ---------------------------------------------------------------------------

#[test]
fn an_omitted_schema_does_not_become_an_equality_test_against_the_empty_string() {
    for engine in [SqlEngine::Postgres, SqlEngine::SqlServer] {
        let query = column_catalog_query(engine, None, "orders");
        let lowered = query.to_lowercase();
        assert!(
            !lowered.contains("table_schema = ''"),
            "{engine:?} asks for the schema named \"\", which no server has: {query}"
        );
    }
}

#[test]
fn an_omitted_schema_leaves_the_schema_unconstrained_on_every_engine() {
    for engine in [SqlEngine::Postgres, SqlEngine::SqlServer, SqlEngine::Sqlite] {
        let query = column_catalog_query(engine, None, "orders");
        let lowered = query.to_lowercase();
        assert!(
            !lowered.contains("table_schema"),
            "{engine:?} must not constrain the schema when none was given: {query}"
        );
    }
}

#[test]
fn a_named_schema_still_constrains_the_query() {
    // The other half: omitting is not the same as naming, and a named schema
    // must still narrow the answer or `describe_table` would merge two tables
    // of the same name from different schemas.
    let query = column_catalog_query(SqlEngine::Postgres, Some("sales"), "orders");
    assert!(query.contains("table_schema = 'sales'"), "{query}");

    let query = column_catalog_query(SqlEngine::SqlServer, Some("sales"), "orders");
    assert!(query.contains("TABLE_SCHEMA = 'sales'"), "{query}");
}

#[test]
fn an_omitted_schema_still_orders_and_still_names_the_table() {
    // Dropping the predicate must not drop the rest of the WHERE clause with
    // it — the table name is the part that cannot be lost.
    for engine in [SqlEngine::Postgres, SqlEngine::SqlServer] {
        let query = column_catalog_query(engine, None, "orders");
        assert!(
            query.to_lowercase().contains("table_name = 'orders'"),
            "{engine:?}: {query}"
        );
        assert!(
            query.to_lowercase().contains("order by"),
            "{engine:?}: {query}"
        );
    }
}
