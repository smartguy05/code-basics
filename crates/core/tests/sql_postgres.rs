//! The PostgreSQL half of the SQL subsystem's end-to-end proof.
//!
//! Everything under `sql/` up to `driver/` is pure and is tested with no
//! database software installed; `sql/driver/postgres.rs`'s own tests cover the
//! whole decoding path — every type map arm, both wire formats, the numeric and
//! calendar arithmetic — because that path was written as a pure function of
//! `(type name, format, bytes)` precisely so it would need no server.
//!
//! What is left here is what only a real server can settle: that the session
//! read-only setting is genuinely refused **at the server**, that the binary
//! wire protocol really does deliver what the type map expects, and that a
//! failure carries no part of the connection string.
//!
//! # These tests do not run by default and cannot run on the machine that wrote
//! them
//!
//! Every test is `#[ignore]`d and gated on `CB_SQL_PG_URL`. There is no
//! PostgreSQL server here and there is no CI, so **none of them has ever been
//! executed** — they are a specification for whoever first points this at a
//! server, not a passing suite. A test that has not run has proved nothing, and
//! saying otherwise would be the same class of claim this whole subsystem
//! refuses to make about a value it could not decode.
//!
//! To run them:
//!
//! ```text
//! CB_SQL_PG_URL=postgres://user:pass@localhost/scratch \
//!   cargo test -p cb-core --test sql_postgres -- --ignored --test-threads=1
//! ```
//!
//! The role needs `CREATE TABLE` in the target database: like the SQLite
//! integration test, every table here is created *through the driver itself*
//! with `writes_allowed: true`, which is not a workaround so much as a second
//! proof that the writes-allowed path really writes.

use cb_core::sql::driver::postgres::PostgresDriver;
use cb_core::sql::driver::{
    stop_channel, ConnectSpec, DriverError, DriverResult, ErrorStage, Limits, ReadOnlyEnforcement,
    SqlConnection, SqlDriver, StatementOutcome,
};
use cb_core::sql::model::{RowCapReason, SqlEvent, SqlResultSet, SqlValue};
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// The server to test against, or [`None`] when none was named.
///
/// Absent is a *skip*, never a failure and never a silently-passing test: each
/// test says out loud that it did not run.
fn dsn(test: &str) -> Option<String> {
    match std::env::var("CB_SQL_PG_URL") {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ => {
            eprintln!("skipping {test}: CB_SQL_PG_URL is not set, so there is no server to test");
            None
        }
    }
}

/// Skip the test unless a server was named.
macro_rules! server {
    ($name:literal) => {
        match dsn($name) {
            Some(url) => url,
            None => return,
        }
    };
}

/// A generous ceiling, so a test that is about one cap is not accidentally
/// about the other.
fn wide_open() -> Limits {
    Limits {
        max_rows: 10_000,
        max_bytes: 64 * 1024 * 1024,
    }
}

async fn open(dsn: &str, writes_allowed: bool) -> Box<dyn SqlConnection> {
    let driver = PostgresDriver::new();
    let spec = ConnectSpec {
        dsn: dsn.to_string(),
        writes_allowed,
    };
    driver
        .connect(&spec)
        .await
        .expect("the server should accept the connection")
}

/// Run one statement and collect both the outcome and every streamed event.
///
/// The channel is drained *after* the call rather than by a forwarding task, so
/// the events are captured in the order the driver actually sent them.
async fn run_with(
    conn: &mut Box<dyn SqlConnection>,
    sql: &str,
    limits: Limits,
) -> (DriverResult<StatementOutcome>, Vec<SqlEvent>) {
    let (tx, mut rx) = mpsc::channel(4096);
    let (_stop_tx, stop_rx) = stop_channel();
    let outcome = conn.run_statement(sql, 0, limits, &tx, stop_rx).await;
    drop(tx);
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    (outcome, events)
}

async fn run(conn: &mut Box<dyn SqlConnection>, sql: &str) -> DriverResult<StatementOutcome> {
    run_with(conn, sql, wide_open()).await.0
}

/// Run a statement that is expected to succeed and hand back its result set.
async fn ok(conn: &mut Box<dyn SqlConnection>, sql: &str) -> SqlResultSet {
    match run(conn, sql).await {
        Ok(StatementOutcome::Finished(result)) => result,
        other => panic!("expected {sql:?} to finish, got {other:?}"),
    }
}

/// The single scalar in a one-row, one-column result.
fn scalar(result: &SqlResultSet) -> &SqlValue {
    assert_eq!(result.rows.len(), 1, "expected exactly one row");
    assert_eq!(result.rows[0].len(), 1, "expected exactly one column");
    &result.rows[0][0]
}

fn number(value: &SqlValue) -> &str {
    match value {
        SqlValue::Number { text } => text,
        other => panic!("expected a number, got {other:?}"),
    }
}

fn text(value: &SqlValue) -> &str {
    match value {
        SqlValue::Text { text, .. } => text,
        other => panic!("expected text, got {other:?}"),
    }
}

/// Give each test its own table, so a shared scratch database can run them all.
async fn fresh_table(conn: &mut Box<dyn SqlConnection>, name: &str, columns: &str) {
    ok(conn, &format!("DROP TABLE IF EXISTS {name}")).await;
    ok(conn, &format!("CREATE TABLE {name} ({columns})")).await;
}

// ---------------------------------------------------------------------------
// 1. The happy path, end to end
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn creates_inserts_and_reads_rows_back() {
    let url = server!("creates_inserts_and_reads_rows_back");
    let mut conn = open(&url, true).await;
    fresh_table(&mut conn, "cb_happy", "id int PRIMARY KEY, label text").await;
    ok(
        &mut conn,
        "INSERT INTO cb_happy (id, label) VALUES (1, 'one'), (2, 'two')",
    )
    .await;

    let result = ok(&mut conn, "SELECT id, label FROM cb_happy ORDER BY id").await;
    assert_eq!(result.rows.len(), 2);
    assert_eq!(number(&result.rows[0][0]), "1");
    assert_eq!(text(&result.rows[0][1]), "one");
    assert_eq!(result.columns.len(), 2);
    assert_eq!(result.columns[0].name, "id");
    assert_eq!(result.columns[0].type_name.as_deref(), Some("INT4"));
    assert_eq!(result.columns[1].type_name.as_deref(), Some("TEXT"));
    // Every row is here, so no cap is reported.
    assert_eq!(result.row_cap, None);
}

#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn the_columns_are_known_before_any_row_arrives() {
    let url = server!("the_columns_are_known_before_any_row_arrives");
    let mut conn = open(&url, true).await;
    fresh_table(&mut conn, "cb_empty", "id int").await;

    // A statement with no rows must still carry its headers, or an empty result
    // set is indistinguishable from a statement that has no result set at all.
    let (outcome, events) = run_with(
        &mut conn,
        "SELECT id FROM cb_empty WHERE id < 0",
        wide_open(),
    )
    .await;
    let result = match outcome {
        Ok(StatementOutcome::Finished(r)) => r,
        other => panic!("expected the empty select to finish, got {other:?}"),
    };
    assert!(result.rows.is_empty());
    assert_eq!(result.columns.len(), 1);
    let columns_event = events
        .iter()
        .find(|e| matches!(e, SqlEvent::Columns { .. }))
        .expect("the columns must be announced");
    let SqlEvent::Columns { columns, .. } = columns_event else {
        unreachable!()
    };
    assert_eq!(columns[0].name, "id");
}

// ---------------------------------------------------------------------------
// 2. The type map against a real binary wire
// ---------------------------------------------------------------------------

/// The decoding path is unit-tested exhaustively; this proves the *wiring* —
/// that the type names the server reports are the ones the map is keyed on, and
/// that the binary bodies really have the shape the decoders assume.
#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn the_binary_wire_delivers_what_the_type_map_expects() {
    let url = server!("the_binary_wire_delivers_what_the_type_map_expects");
    let mut conn = open(&url, true).await;

    for (sql, expected) in [
        ("SELECT true", SqlValue::Bool { value: true }),
        (
            "SELECT 42::int4",
            SqlValue::Number {
                text: "42".to_string(),
            },
        ),
        (
            "SELECT 9223372036854775807::int8",
            SqlValue::Number {
                text: "9223372036854775807".to_string(),
            },
        ),
        (
            "SELECT 0.1::float4",
            SqlValue::Number {
                text: "0.1".to_string(),
            },
        ),
        (
            "SELECT 'hello'::text",
            SqlValue::Text {
                text: "hello".to_string(),
                truncated: false,
            },
        ),
        (
            "SELECT '00010203-0405-0607-0809-0a0b0c0d0e0f'::uuid",
            SqlValue::Text {
                text: "00010203-0405-0607-0809-0a0b0c0d0e0f".to_string(),
                truncated: false,
            },
        ),
        (
            "SELECT '{\"a\": 1}'::jsonb",
            SqlValue::Text {
                text: "{\"a\": 1}".to_string(),
                truncated: false,
            },
        ),
        (
            "SELECT DATE '2024-01-01'",
            SqlValue::Text {
                text: "2024-01-01".to_string(),
                truncated: false,
            },
        ),
        (
            "SELECT TIMESTAMPTZ '2024-01-01 12:34:56+00'",
            SqlValue::Text {
                text: "2024-01-01 12:34:56+00".to_string(),
                truncated: false,
            },
        ),
        (
            "SELECT '\\xdead'::bytea",
            SqlValue::Bytes {
                hex: "dead".to_string(),
                byte_length: 2,
                truncated: false,
            },
        ),
    ] {
        let result = ok(&mut conn, sql).await;
        assert_eq!(scalar(&result), &expected, "for {sql}");
    }
}

/// `numeric` crosses as a string, with its scale intact.
///
/// The one type whose whole point is that it is *not* a float: `0.10` is not
/// `0.1` to a ledger, and a value past a double's precision must not be
/// rounded on the way to the grid.
#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn numeric_keeps_its_scale_and_its_precision() {
    let url = server!("numeric_keeps_its_scale_and_its_precision");
    let mut conn = open(&url, true).await;

    for (sql, expected) in [
        ("SELECT 0.10::numeric(4,2)", "0.10"),
        ("SELECT 12345.678::numeric", "12345.678"),
        ("SELECT (-12345.678)::numeric", "-12345.678"),
        ("SELECT 0::numeric", "0"),
        (
            "SELECT 123456789012345678901234567890.123456789::numeric",
            "123456789012345678901234567890.123456789",
        ),
        ("SELECT 'NaN'::numeric", "NaN"),
    ] {
        let result = ok(&mut conn, sql).await;
        assert_eq!(number(scalar(&result)), expected, "for {sql}");
    }
}

/// A type this build cannot decode says so, by name, rather than rendering an
/// empty cell that reads as `NULL`.
#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn an_undecodable_type_names_itself_rather_than_arriving_blank() {
    let url = server!("an_undecodable_type_names_itself_rather_than_arriving_blank");
    let mut conn = open(&url, true).await;

    for (sql, type_name) in [
        ("SELECT ARRAY[1,2,3]", "INT4[]"),
        ("SELECT '(1,2)'::point", "POINT"),
        ("SELECT '192.168.0.1'::inet", "INET"),
        ("SELECT int4range(1,5)", "INT4RANGE"),
    ] {
        let result = ok(&mut conn, sql).await;
        match scalar(&result) {
            SqlValue::Unsupported { type_name: got } => {
                assert_eq!(got, type_name, "for {sql}")
            }
            other => panic!("{sql} should be reported unsupported, got {other:?}"),
        }
    }

    // A user-defined enum resolves to its own name, not to sqlx's `?`.
    ok(&mut conn, "DROP TYPE IF EXISTS cb_mood CASCADE").await;
    ok(&mut conn, "CREATE TYPE cb_mood AS ENUM ('ok', 'bad')").await;
    let result = ok(&mut conn, "SELECT 'ok'::cb_mood").await;
    match scalar(&result) {
        SqlValue::Unsupported { type_name } => assert_eq!(type_name, "cb_mood"),
        other => panic!("an enum should be reported unsupported by name, got {other:?}"),
    }
    assert_eq!(result.columns[0].type_name.as_deref(), Some("cb_mood"));
}

// ---------------------------------------------------------------------------
// 3. NULL vs empty string vs truncated — three shapes, not one
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn null_empty_and_truncated_come_back_as_three_distinct_shapes() {
    let url = server!("null_empty_and_truncated_come_back_as_three_distinct_shapes");
    let mut conn = open(&url, true).await;
    fresh_table(&mut conn, "cb_shapes", "id int, value text").await;
    ok(
        &mut conn,
        "INSERT INTO cb_shapes VALUES (1, NULL), (2, ''), (3, repeat('x', 20000))",
    )
    .await;

    let result = ok(&mut conn, "SELECT value FROM cb_shapes ORDER BY id").await;
    assert_eq!(result.rows.len(), 3);
    assert_eq!(
        result.rows[0][0],
        SqlValue::Null,
        "the database holds no value here"
    );
    assert_eq!(
        result.rows[1][0],
        SqlValue::Text {
            text: String::new(),
            truncated: false
        },
        "the empty string is a value, and is not a NULL"
    );
    match &result.rows[2][0] {
        SqlValue::Text { text, truncated } => {
            assert!(*truncated, "a cut value must say it was cut");
            assert!(text.len() < 20_000, "and must actually have been cut");
        }
        other => panic!("expected truncated text, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 4. Both caps, each naming the one that actually bit
// ---------------------------------------------------------------------------

async fn seed_rows(conn: &mut Box<dyn SqlConnection>, table: &str, n: usize) {
    fresh_table(conn, table, "id int, filler text").await;
    ok(
        conn,
        &format!(
            "INSERT INTO {table} SELECT g, repeat('y', 100) FROM generate_series(1, {n}) AS g"
        ),
    )
    .await;
}

#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn the_row_cap_is_reported_and_names_the_row_limit() {
    let url = server!("the_row_cap_is_reported_and_names_the_row_limit");
    let mut conn = open(&url, true).await;
    seed_rows(&mut conn, "cb_rowcap", 50).await;

    let (outcome, _) = run_with(
        &mut conn,
        "SELECT id FROM cb_rowcap ORDER BY id",
        Limits {
            max_rows: 10,
            max_bytes: 64 * 1024 * 1024,
        },
    )
    .await;
    let result = match outcome {
        Ok(StatementOutcome::Finished(r)) => r,
        other => panic!("a capped statement still finishes, got {other:?}"),
    };
    assert_eq!(result.rows.len(), 10);
    let cap = result
        .row_cap
        .expect("a shortened result must report its cap");
    assert_eq!(cap.reason, RowCapReason::RowLimit);
    assert_eq!(cap.limit, 10);
    assert!(!result.is_complete());
}

#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn the_byte_cap_is_reported_and_names_the_byte_limit() {
    let url = server!("the_byte_cap_is_reported_and_names_the_byte_limit");
    let mut conn = open(&url, true).await;
    seed_rows(&mut conn, "cb_bytecap", 50).await;

    let (outcome, _) = run_with(
        &mut conn,
        "SELECT filler FROM cb_bytecap ORDER BY id",
        Limits {
            max_rows: 10_000,
            max_bytes: 1_000,
        },
    )
    .await;
    let result = match outcome {
        Ok(StatementOutcome::Finished(r)) => r,
        other => panic!("a capped statement still finishes, got {other:?}"),
    };
    let cap = result
        .row_cap
        .expect("a shortened result must report its cap");
    // Raising the row limit would return no further rows, which is exactly why
    // this is a different answer from `RowLimit`.
    assert_eq!(cap.reason, RowCapReason::ByteLimit);
    assert!(result.rows.len() < 50);
}

#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn a_result_ending_exactly_on_the_ceiling_is_not_reported_as_capped() {
    let url = server!("a_result_ending_exactly_on_the_ceiling_is_not_reported_as_capped");
    let mut conn = open(&url, true).await;
    seed_rows(&mut conn, "cb_exact", 10).await;

    let (outcome, _) = run_with(
        &mut conn,
        "SELECT id FROM cb_exact ORDER BY id",
        Limits {
            max_rows: 10,
            max_bytes: 64 * 1024 * 1024,
        },
    )
    .await;
    let result = match outcome {
        Ok(StatementOutcome::Finished(r)) => r,
        other => panic!("expected it to finish, got {other:?}"),
    };
    assert_eq!(result.rows.len(), 10);
    // Every row is present. Reporting a cap here would tell the user their
    // answer is incomplete when they are looking at all of it.
    assert_eq!(result.row_cap, None);
    assert!(result.is_complete());
}

// ---------------------------------------------------------------------------
// 5. Writes disallowed: refused *by the server*, and the database is unchanged
// ---------------------------------------------------------------------------

/// The heart of this file.
///
/// `ReadOnlyEnforcement::ServerSession` claims the **server** refuses the
/// write before it runs — a much stronger claim than the text guard's, and one
/// that only a server can settle. Everything else in this file could be wrong
/// and the console would merely be inconvenient; if this is wrong, the console
/// tells the user a write is impossible while it is not.
#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn a_read_only_connection_has_the_server_refuse_a_write_and_the_table_is_unchanged() {
    let url =
        server!("a_read_only_connection_has_the_server_refuse_a_write_and_the_table_is_unchanged");
    let mut writer = open(&url, true).await;
    fresh_table(&mut writer, "cb_readonly", "id int").await;
    ok(&mut writer, "INSERT INTO cb_readonly VALUES (1)").await;

    let mut reader = open(&url, false).await;
    assert_eq!(
        reader.read_only_enforcement(),
        ReadOnlyEnforcement::ServerSession,
        "the connection may only carry this once the server confirmed the setting"
    );

    // The server holds the session read-only, so it reports the setting itself.
    let shown = ok(&mut reader, "SHOW default_transaction_read_only").await;
    assert_eq!(text(scalar(&shown)), "on");

    let err = match run(&mut reader, "INSERT INTO cb_readonly VALUES (2)").await {
        Err(e) => e,
        other => panic!("a read-only session must refuse an insert, got {other:?}"),
    };
    assert_eq!(err.stage, ErrorStage::Execute);
    assert!(
        err.message.to_lowercase().contains("read-only")
            || err.message.to_lowercase().contains("read only"),
        "the server's own refusal should say why: {}",
        err.message
    );

    // And nothing landed. The distinction that matters against SQL Server: the
    // statement did not run and get undone — it never ran.
    let after = ok(&mut writer, "SELECT count(*) FROM cb_readonly").await;
    assert_eq!(number(scalar(&after)), "1");
}

#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn a_read_only_connection_also_refuses_ddl() {
    let url = server!("a_read_only_connection_also_refuses_ddl");
    let mut reader = open(&url, false).await;
    assert!(
        run(&mut reader, "CREATE TABLE cb_should_not_exist (id int)")
            .await
            .is_err(),
        "a read-only session refuses DDL as well as DML"
    );

    let mut writer = open(&url, true).await;
    let exists = ok(
        &mut writer,
        "SELECT count(*) FROM information_schema.tables WHERE table_name = 'cb_should_not_exist'",
    )
    .await;
    assert_eq!(number(scalar(&exists)), "0");
}

/// A read-only connection is still a working *reading* connection.
///
/// The guarantee is that writes are refused, not that the connection is
/// crippled — and a test that only proved things fail would pass against a
/// driver that refused everything.
#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn a_read_only_connection_still_reads() {
    let url = server!("a_read_only_connection_still_reads");
    let mut writer = open(&url, true).await;
    fresh_table(&mut writer, "cb_ro_reads", "id int").await;
    ok(&mut writer, "INSERT INTO cb_ro_reads VALUES (7)").await;

    let mut reader = open(&url, false).await;
    let result = ok(&mut reader, "SELECT id FROM cb_ro_reads").await;
    assert_eq!(number(scalar(&result)), "7");
}

// ---------------------------------------------------------------------------
// 6. Writes allowed: the insert lands, and `rows_affected` is this statement's
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn a_writable_connection_performs_the_insert() {
    let url = server!("a_writable_connection_performs_the_insert");
    let mut conn = open(&url, true).await;
    assert_eq!(
        conn.read_only_enforcement(),
        ReadOnlyEnforcement::NotEnforced,
        "nothing is enforcing read-only, and saying `by text check` would be \
         false in the other direction"
    );
    fresh_table(&mut conn, "cb_writes", "id int").await;
    ok(&mut conn, "INSERT INTO cb_writes VALUES (1), (2), (3)").await;

    let count = ok(&mut conn, "SELECT count(*) FROM cb_writes").await;
    assert_eq!(number(scalar(&count)), "3");
}

/// Unlike SQLite's, a Postgres row count belongs to the statement that produced
/// it, so it is reported — and `Some(0)` stays distinct from `None`.
#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn rows_affected_is_this_statements_own_count() {
    let url = server!("rows_affected_is_this_statements_own_count");
    let mut conn = open(&url, true).await;
    fresh_table(&mut conn, "cb_affected", "id int").await;

    let inserted = ok(&mut conn, "INSERT INTO cb_affected VALUES (1), (2), (3)").await;
    assert_eq!(inserted.rows_affected, Some(3));

    // A statement that matched nothing reports `Some(0)` — it ran and changed
    // nothing — which is a different answer from having no count to report.
    let none_matched = ok(&mut conn, "DELETE FROM cb_affected WHERE id > 100").await;
    assert_eq!(none_matched.rows_affected, Some(0));

    // And the count that follows a SELECT is that SELECT's, not the insert's:
    // this is the exact bug that makes SQLite's `rows_affected` always `None`.
    let selected = ok(&mut conn, "SELECT id FROM cb_affected ORDER BY id").await;
    assert_eq!(selected.rows.len(), 3);
    assert_eq!(selected.rows_affected, Some(3));
}

// ---------------------------------------------------------------------------
// 7. Stopped is its own terminal state
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn stopping_yields_stopped_which_is_neither_finished_nor_failed() {
    let url = server!("stopping_yields_stopped_which_is_neither_finished_nor_failed");
    let mut conn = open(&url, true).await;
    seed_rows(&mut conn, "cb_stop", 20).await;

    // Signalled before the statement starts, so the outcome is deterministic:
    // the row loop checks the flag before its first await precisely because a
    // stop that arrived early would never show up as a *change*.
    let (tx, mut rx) = mpsc::channel(4096);
    let (stop_tx, stop_rx) = stop_channel();
    stop_tx.send(true).unwrap();
    let outcome = conn
        .run_statement(
            "SELECT id FROM cb_stop ORDER BY id",
            0,
            wide_open(),
            &tx,
            stop_rx,
        )
        .await
        .expect("stopping is not a failure");
    drop(tx);

    assert!(outcome.was_stopped());
    let stopped = match &outcome {
        StatementOutcome::Stopped(result) => result,
        StatementOutcome::Finished(_) => panic!("a stopped statement must not read as Finished"),
    };
    assert!(stopped.rows.is_empty(), "no row was delivered");
    // A stop is not a cap: the rows are missing because the reader stopped, not
    // because a ceiling bit, and reporting a `RowCap` here would blame the
    // wrong thing.
    assert_eq!(stopped.row_cap, None);
    // And it is not a row count either: the loop never reached the end, so
    // there is no count to report.
    assert_eq!(stopped.rows_affected, None);

    // The same query, unstopped, finishes — so `Stopped` is a real distinction
    // and not just what this query always does.
    let again = ok(&mut conn, "SELECT id FROM cb_stop ORDER BY id").await;
    assert_eq!(again.rows.len(), 20);

    // And neither is the third state: a failure is the `Err` arm, so it can
    // never be read as either of the two.
    assert!(run(&mut conn, "SELECT * FROM cb_no_such_table")
        .await
        .is_err());

    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SqlEvent::Rows { rows, .. } if !rows.is_empty())),
        "a stopped run must not have streamed rows: {events:?}"
    );
}

/// A dropped stop sender is not a stop.
///
/// The `StopWait` rule, end to end: the row loop must read to the end when the
/// channel merely dies, rather than reporting an action nobody took and
/// handing back an arbitrary prefix of the rows.
#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn dropping_the_stop_sender_lets_the_statement_finish() {
    let url = server!("dropping_the_stop_sender_lets_the_statement_finish");
    let mut conn = open(&url, true).await;
    seed_rows(&mut conn, "cb_nostop", 20).await;

    let (tx, _rx) = mpsc::channel(4096);
    let (stop_tx, stop_rx) = stop_channel();
    drop(stop_tx);
    let outcome = conn
        .run_statement(
            "SELECT id FROM cb_nostop ORDER BY id",
            0,
            wide_open(),
            &tx,
            stop_rx,
        )
        .await
        .expect("a dead stop channel is not a failure");
    assert!(
        !outcome.was_stopped(),
        "nobody stopped anything: the channel merely died"
    );
    assert_eq!(outcome.result().rows.len(), 20, "every row is read");
}

// ---------------------------------------------------------------------------
// 8. A failure names neither the server nor the connection string
// ---------------------------------------------------------------------------

fn assert_leaks_nothing(err: &DriverError, secret: &str) {
    assert!(
        !err.message.contains(secret),
        "the failure message leaked the password: {}",
        err.message
    );
    for fragment in ["postgres://", "Password=", "password="] {
        assert!(
            !err.message.contains(fragment),
            "the failure message echoed the connection string ({fragment}): {}",
            err.message
        );
    }
}

#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn a_malformed_statement_fails_without_naming_the_connection_string() {
    let url = server!("a_malformed_statement_fails_without_naming_the_connection_string");
    let mut conn = open(&url, true).await;

    let err = match run(&mut conn, "SELEKT * FROM nowhere").await {
        Err(e) => e,
        other => panic!("a malformed statement must fail, got {other:?}"),
    };
    assert_eq!(err.stage, ErrorStage::Execute);
    assert!(
        !err.message.is_empty(),
        "the failure must say something useful"
    );
    assert_leaks_nothing(&err, "hunter2");
}

/// A connect failure carries no password, even though the DSN it was built from
/// is the one thing a driver error routinely embeds whole.
///
/// Deliberately points at a database that does not exist on the *named* server,
/// so the failure comes from the server rather than from name resolution.
#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn a_connect_failure_carries_no_password_from_the_connection_string() {
    let _url = server!("a_connect_failure_carries_no_password_from_the_connection_string");
    let driver = PostgresDriver::new();
    let spec = ConnectSpec {
        // A host that will not resolve, with a password beside it: the point is
        // the message, not which failure produced it.
        dsn: "Host=cb-no-such-host.invalid;Database=app;Username=u;Password=hunter2".to_string(),
        writes_allowed: false,
    };
    let err = driver
        .connect(&spec)
        .await
        .err()
        .expect("an unreachable host must fail to connect");

    assert!(
        matches!(err.stage, ErrorStage::Connect | ErrorStage::ConnectTimeout),
        "got {:?}",
        err.stage
    );
    assert_leaks_nothing(&err, "hunter2");
}

/// A connection string naming no host never reaches the network, and its
/// refusal echoes none of it.
#[tokio::test]
#[ignore = "needs a PostgreSQL server; set CB_SQL_PG_URL"]
async fn a_connection_string_naming_no_host_is_refused_before_anything_is_sent() {
    let _url = server!("a_connection_string_naming_no_host_is_refused_before_anything_is_sent");
    let driver = PostgresDriver::new();
    let spec = ConnectSpec {
        dsn: "Server=localhost;Database=app;Password=hunter2".to_string(),
        writes_allowed: false,
    };
    let err = driver
        .connect(&spec)
        .await
        .err()
        .expect("a string naming no host must be refused");
    assert_eq!(err.stage, ErrorStage::Connect);
    assert_leaks_nothing(&err, "hunter2");
}
