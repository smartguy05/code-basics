//! The SQL Server half of the SQL console, against a real server.
//!
//! Sibling of `sql_sqlite.rs`, and it must be read against it, because the
//! property the two prove is **not the same one**.
//!
//! SQLite opens a handle SQLite itself refuses a write on, so its integration
//! test proves *the write never ran*. SQL Server has no read-only session mode
//! at all: the driver runs every statement inside a transaction it always rolls
//! back, so the write **executes** and is then undone. The test below therefore
//! asserts the honest, weaker property — the `INSERT` **succeeds**, and the
//! table is unchanged afterwards. An expectation that the insert is refused
//! would be the comfortable one to write and would be false.
//!
//! # These tests need a server, and this machine has none
//!
//! Every test here is `#[ignore]`d *and* gated on `CB_SQL_MSSQL_URL`, so it
//! skips cleanly rather than failing when there is nothing to talk to. Two
//! gates rather than one on purpose: `#[ignore]` keeps them out of the default
//! run, and the environment check keeps `--include-ignored` honest on a machine
//! that still has no server.
//!
//! ```text
//! set CB_SQL_MSSQL_URL=Server=localhost,1433;Database=tempdb;User Id=sa;Password=…;TrustServerCertificate=true
//! cargo test -p cb-core --test sql_mssql -- --ignored
//! ```
//!
//! **Nobody has run these.** They were written against the protocol and the
//! driver, not against an observed server, and the first person with a server
//! should expect to fix something here rather than assume it passes.
//!
//! # Seeding
//!
//! As in `sql_sqlite.rs`, every table is created *through the driver itself*
//! with `writes_allowed: true`, which is a second proof that the writes-allowed
//! path really writes. Each test uses its own table name and drops it at the
//! end, because unlike a SQLite tempfile a server is shared and outlives the
//! run.

use std::time::Duration;

use cb_core::sql::driver::mssql::MssqlDriver;
use cb_core::sql::driver::{
    stop_channel, ConnectSpec, DriverResult, ErrorStage, Limits, ReadOnlyEnforcement,
    SqlConnection, SqlDriver, StatementOutcome,
};
use cb_core::sql::model::{RowCapReason, SqlEvent, SqlResultSet, SqlValue};
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// The connection string, or `None` — which every test treats as *skip*, never
/// as *fail*. A missing server is not a broken driver.
fn dsn() -> Option<String> {
    match std::env::var("CB_SQL_MSSQL_URL") {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => None,
    }
}

/// Skip cleanly when there is no server. The `return` in the caller is what
/// makes the test pass rather than fail.
macro_rules! server {
    () => {
        match dsn() {
            Some(dsn) => dsn,
            None => {
                eprintln!("skipped: CB_SQL_MSSQL_URL is not set");
                return;
            }
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
    let driver = MssqlDriver::with_connect_timeout(Duration::from_secs(20));
    let spec = ConnectSpec {
        dsn: dsn.to_string(),
        writes_allowed,
    };
    driver.connect(&spec).await.expect("the server should open")
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
    let (tx, mut rx) = mpsc::channel(8192);
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

async fn ok(conn: &mut Box<dyn SqlConnection>, sql: &str) -> SqlResultSet {
    match run(conn, sql).await {
        Ok(StatementOutcome::Finished(result)) => result,
        other => panic!("expected {sql:?} to finish, got {other:?}"),
    }
}

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

/// Create a table with `body` and hand back its name; drop it with [`drop_table`].
async fn make_table(dsn: &str, name: &str, body: &str) {
    let mut w = open(dsn, true).await;
    let _ = run(&mut w, &format!("DROP TABLE IF EXISTS {name}")).await;
    ok(&mut w, &format!("CREATE TABLE {name} ({body})")).await;
}

async fn drop_table(dsn: &str, name: &str) {
    let mut w = open(dsn, true).await;
    let _ = run(&mut w, &format!("DROP TABLE IF EXISTS {name}")).await;
}

/// One `SELECT COUNT(*)`, on its own writable connection.
async fn count(dsn: &str, table: &str) -> u64 {
    let mut w = open(dsn, true).await;
    let result = ok(&mut w, &format!("SELECT COUNT(*) FROM {table}")).await;
    number(scalar(&result)).parse().expect("a count")
}

// ---------------------------------------------------------------------------
// 1. The happy path, end to end
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn creates_inserts_and_reads_rows_back() {
    let dsn = server!();
    let table = "cb_sql_happy";
    make_table(&dsn, table, "id INT NOT NULL, name NVARCHAR(50) NULL").await;

    let mut w = open(&dsn, true).await;
    ok(
        &mut w,
        &format!("INSERT INTO {table} (id, name) VALUES (1, N'one'), (2, N'two')"),
    )
    .await;

    let mut r = open(&dsn, false).await;
    let result = ok(&mut r, &format!("SELECT id, name FROM {table} ORDER BY id")).await;
    assert_eq!(result.columns.len(), 2);
    assert_eq!(result.columns[0].name, "id");
    assert_eq!(result.rows.len(), 2);
    assert_eq!(number(&result.rows[0][0]), "1");
    assert_eq!(
        result.rows[1][1],
        SqlValue::Text {
            text: "two".to_string(),
            truncated: false
        }
    );
    assert!(result.is_complete(), "no cap should have bitten");

    drop_table(&dsn, table).await;
}

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn the_events_arrive_in_the_order_the_grid_is_built_from() {
    let dsn = server!();
    let mut r = open(&dsn, false).await;
    let (_outcome, events) = run_with(&mut r, "SELECT 1 AS a", wide_open()).await;

    assert!(matches!(events[0], SqlEvent::Started { .. }));
    assert!(
        events.iter().any(|e| matches!(e, SqlEvent::Columns { .. })),
        "columns must arrive before rows: {events:?}"
    );
    let columns_at = events
        .iter()
        .position(|e| matches!(e, SqlEvent::Columns { .. }))
        .unwrap();
    let rows_at = events
        .iter()
        .position(|e| matches!(e, SqlEvent::Rows { .. }))
        .unwrap();
    assert!(columns_at < rows_at);
    assert!(matches!(events.last(), Some(SqlEvent::Completed { .. })));
}

// ---------------------------------------------------------------------------
// 2. Read-only: the write RUNS and is undone. This is the weaker guarantee.
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_write_on_a_read_only_connection_runs_and_is_rolled_back() {
    // The single most important test in this file, and the one whose
    // *expectation* differs from SQLite's. There is no session read-only mode
    // on SQL Server, so the insert is not refused — it succeeds, inside a
    // transaction the driver then rolls back. What is proved here is that the
    // rollback actually happened: the row count is unchanged afterwards.
    let dsn = server!();
    let table = "cb_sql_rollback";
    make_table(&dsn, table, "id INT NOT NULL").await;

    let mut w = open(&dsn, true).await;
    ok(&mut w, &format!("INSERT INTO {table} (id) VALUES (1)")).await;
    assert_eq!(count(&dsn, table).await, 1);

    // The guard sits *above* the driver, so reaching this code with a write is
    // exactly the case the transaction exists for.
    let mut r = open(&dsn, false).await;
    let outcome = run(&mut r, &format!("INSERT INTO {table} (id) VALUES (2)")).await;
    assert!(
        matches!(outcome, Ok(StatementOutcome::Finished(_))),
        "the write is not refused on this engine: {outcome:?}"
    );

    assert_eq!(
        count(&dsn, table).await,
        1,
        "the rolled-back insert must not be there"
    );
    drop_table(&dsn, table).await;
}

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_read_only_connection_reports_the_mechanism_it_actually_uses() {
    let dsn = server!();
    let r = open(&dsn, false).await;
    assert_eq!(
        r.read_only_enforcement(),
        ReadOnlyEnforcement::RolledBackTransaction
    );
    assert!(!r.read_only_enforcement().refuses_writes_before_they_run());

    let w = open(&dsn, true).await;
    assert_eq!(
        w.read_only_enforcement(),
        ReadOnlyEnforcement::NotEnforced,
        "writes were allowed, so nothing is enforcing read-only"
    );
}

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_second_read_on_the_same_connection_is_not_inside_the_first_transaction() {
    // The transaction is per statement, so the rollback of one must not leave
    // the next one running in a closed or nested one.
    let dsn = server!();
    let mut r = open(&dsn, false).await;
    for expected in ["1", "2", "3"] {
        let result = ok(&mut r, &format!("SELECT {expected}")).await;
        assert_eq!(number(scalar(&result)), expected);
    }
}

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_write_a_read_only_connection_allowed_through_leaves_nothing_behind_even_in_a_batch() {
    // A write hidden after a read in one batch is still inside the same
    // transaction, so it is undone with it.
    let dsn = server!();
    let table = "cb_sql_batch";
    make_table(&dsn, table, "id INT NOT NULL").await;

    let mut r = open(&dsn, false).await;
    let outcome = run(
        &mut r,
        &format!("SELECT 1; INSERT INTO {table} (id) VALUES (9)"),
    )
    .await;
    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(count(&dsn, table).await, 0);

    drop_table(&dsn, table).await;
}

// ---------------------------------------------------------------------------
// 3. NULL, empty and truncated are three shapes
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn null_and_empty_and_truncated_stay_three_distinct_shapes() {
    let dsn = server!();
    let mut r = open(&dsn, false).await;

    let result = ok(&mut r, "SELECT CAST(NULL AS NVARCHAR(10))").await;
    assert_eq!(*scalar(&result), SqlValue::Null);

    let result = ok(&mut r, "SELECT N''").await;
    assert_eq!(
        *scalar(&result),
        SqlValue::Text {
            text: String::new(),
            truncated: false
        },
        "an empty string is not a NULL"
    );

    let result = ok(
        &mut r,
        "SELECT REPLICATE(CAST(N'x' AS NVARCHAR(MAX)), 200000)",
    )
    .await;
    match scalar(&result) {
        SqlValue::Text { truncated, .. } => assert!(*truncated, "a long value must say it was cut"),
        other => panic!("expected text, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_decimal_keeps_the_scale_the_server_wrote() {
    let dsn = server!();
    let mut r = open(&dsn, false).await;
    let result = ok(&mut r, "SELECT CAST(1.10 AS DECIMAL(10,2))").await;
    assert_eq!(
        number(scalar(&result)),
        "1.10",
        "0.10 is not 0.1 to a ledger"
    );
}

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_bigint_past_a_javascript_number_survives_as_text() {
    let dsn = server!();
    let mut r = open(&dsn, false).await;
    let result = ok(&mut r, "SELECT CAST(9007199254740993 AS BIGINT)").await;
    assert_eq!(number(scalar(&result)), "9007199254740993");
}

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_temporal_value_abstains_rather_than_showing_a_derived_date() {
    // Pinned deliberately: this build has no date decoder, and the cell says
    // which type it could not read. If a tiberius date feature is enabled
    // later, this test is the thing that should be changed — and its failure
    // is the reminder that the change was intentional.
    let dsn = server!();
    let mut r = open(&dsn, false).await;
    let result = ok(&mut r, "SELECT CAST('2020-01-02' AS DATE)").await;
    match scalar(&result) {
        SqlValue::Unsupported { type_name } => assert_eq!(type_name, "date"),
        other => panic!("expected an abstention, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_column_the_server_gives_no_name_or_type_for_is_reported_honestly() {
    let dsn = server!();
    let mut r = open(&dsn, false).await;
    let result = ok(&mut r, "SELECT 1 + 1 AS calc").await;
    assert_eq!(result.columns[0].name, "calc");
    // An expression still has a concrete TDS type; what must never appear is a
    // type name of "null".
    assert_ne!(result.columns[0].type_name.as_deref(), Some("null"));
}

// ---------------------------------------------------------------------------
// 4. Caps are reported, never applied silently
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn the_row_cap_is_reported_when_a_row_was_actually_left_behind() {
    let dsn = server!();
    let table = "cb_sql_cap";
    make_table(&dsn, table, "id INT NOT NULL").await;
    let mut w = open(&dsn, true).await;
    ok(&mut w, &format!("INSERT INTO {table} (id) VALUES (1),(2)")).await;

    let mut r = open(&dsn, false).await;
    let sql = format!("SELECT id FROM {table} ORDER BY id");

    let (outcome, _) = run_with(
        &mut r,
        &sql,
        Limits {
            max_rows: 1,
            max_bytes: 64 * 1024 * 1024,
        },
    )
    .await;
    let result = outcome.unwrap().result().clone();
    assert_eq!(result.rows.len(), 1);
    let cap = result.row_cap.expect("a row was left behind");
    assert_eq!(cap.reason, RowCapReason::RowLimit);

    // Exactly on the ceiling is *not* a cap: the answer is complete.
    let (outcome, _) = run_with(
        &mut r,
        &sql,
        Limits {
            max_rows: 2,
            max_bytes: 64 * 1024 * 1024,
        },
    )
    .await;
    let result = outcome.unwrap().result().clone();
    assert_eq!(result.rows.len(), 2);
    assert!(
        result.is_complete(),
        "a table holding exactly max_rows rows is complete, not truncated"
    );

    drop_table(&dsn, table).await;
}

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn the_byte_cap_is_charged_at_what_the_server_holds_not_at_what_is_rendered() {
    // A column of large values renders to a few kilobytes a row under
    // `format`'s display caps. Charging the rendered size would let it stream
    // past the budget indefinitely.
    let dsn = server!();
    let mut r = open(&dsn, false).await;
    let sql = "SELECT REPLICATE(CAST(N'x' AS NVARCHAR(MAX)), 100000) AS big \
               FROM (VALUES (1),(2),(3),(4),(5)) AS t(n)";
    let (outcome, _) = run_with(
        &mut r,
        sql,
        Limits {
            max_rows: 10_000,
            max_bytes: 64 * 1024,
        },
    )
    .await;
    let result = outcome.unwrap().result().clone();
    let cap = result.row_cap.expect("the byte budget should have bitten");
    assert_eq!(cap.reason, RowCapReason::ByteLimit);
}

// ---------------------------------------------------------------------------
// 5. Stopping, and what it leaves behind
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_pre_signalled_stop_is_its_own_terminal_state_not_a_failure() {
    let dsn = server!();
    let mut r = open(&dsn, false).await;

    let (tx, _rx) = mpsc::channel(1024);
    let (stop_tx, stop_rx) = stop_channel();
    stop_tx.send(true).unwrap();
    let outcome = r
        .run_statement("SELECT 1", 0, wide_open(), &tx, stop_rx)
        .await;
    match outcome {
        Ok(StatementOutcome::Stopped(result)) => assert!(result.rows.is_empty()),
        other => panic!("expected Stopped, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_stop_inside_the_read_only_transaction_says_so_and_stops_the_connection() {
    // No rollback is sent after a stop — sending one would first drain the
    // stream the user asked to stop reading — so the connection is poisoned and
    // the reader is told. Both halves are checked, because either alone would
    // be a silent leak of an open transaction.
    let dsn = server!();
    let mut r = open(&dsn, false).await;

    let (tx, mut rx) = mpsc::channel(1024);
    let (stop_tx, stop_rx) = stop_channel();
    stop_tx.send(true).unwrap();
    let outcome = r
        .run_statement("SELECT 1", 0, wide_open(), &tx, stop_rx)
        .await;
    assert!(matches!(outcome, Ok(StatementOutcome::Stopped(_))));
    drop(tx);

    let mut notices = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let SqlEvent::Notice { message, .. } = event {
            notices.push(message);
        }
    }
    assert!(
        notices.iter().any(|m| m.contains("rolled back")),
        "the open transaction must be reported: {notices:?}"
    );

    let after = run(&mut r, "SELECT 2").await;
    let err = after.expect_err("a poisoned connection runs nothing further");
    assert_eq!(err.stage, ErrorStage::Execute);
    assert!(err.message.contains("will not run another statement"));
}

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_writable_connection_is_never_poisoned_by_a_stop() {
    // There is no transaction when writes are allowed, so there is nothing to
    // leave open and nothing to stop the connection for.
    let dsn = server!();
    let mut w = open(&dsn, true).await;

    let (tx, _rx) = mpsc::channel(1024);
    let (stop_tx, stop_rx) = stop_channel();
    stop_tx.send(true).unwrap();
    let outcome = w
        .run_statement("SELECT 1", 0, wide_open(), &tx, stop_rx)
        .await;
    assert!(matches!(outcome, Ok(StatementOutcome::Stopped(_))));

    let result = ok(&mut w, "SELECT 7").await;
    assert_eq!(number(scalar(&result)), "7");
}

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn dropping_the_stop_handle_is_not_reported_as_a_user_stop() {
    // The bug `StopWait` exists for: `watch::Receiver::changed()` resolves with
    // `Err` the instant the last sender drops, and a `_ =` arm in a `select!`
    // matches that as readily as a real signal — reporting an action nobody
    // took, with an arbitrary prefix of the rows.
    let dsn = server!();
    let table = "cb_sql_stopdrop";
    make_table(&dsn, table, "id INT NOT NULL").await;
    let mut w = open(&dsn, true).await;
    ok(
        &mut w,
        &format!("INSERT INTO {table} (id) SELECT TOP (50) ROW_NUMBER() OVER (ORDER BY object_id) FROM sys.objects"),
    )
    .await;

    let mut r = open(&dsn, false).await;
    let (tx, _rx) = mpsc::channel(4096);
    let (stop_tx, stop_rx) = stop_channel();
    drop(stop_tx); // no stop was ever signalled
    let outcome = r
        .run_statement(
            &format!("SELECT id FROM {table}"),
            0,
            wide_open(),
            &tx,
            stop_rx,
        )
        .await;
    match outcome {
        Ok(StatementOutcome::Finished(result)) => assert_eq!(result.rows.len(), 50),
        other => panic!("nobody stopped this: {other:?}"),
    }

    drop_table(&dsn, table).await;
}

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_signalled_stop_whose_handle_is_then_dropped_is_still_a_stop() {
    // The other half of the same rule: the *flag* is read, not the channel's
    // liveness, so a stop that landed before its sender went away still counts.
    let dsn = server!();
    let mut r = open(&dsn, false).await;
    let (tx, _rx) = mpsc::channel(1024);
    let (stop_tx, stop_rx) = stop_channel();
    stop_tx.send(true).unwrap();
    drop(stop_tx);
    let outcome = r
        .run_statement("SELECT 1", 0, wide_open(), &tx, stop_rx)
        .await;
    assert!(matches!(outcome, Ok(StatementOutcome::Stopped(_))));
}

// ---------------------------------------------------------------------------
// 6. Failures name the problem and never the connection string
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_statement_error_is_an_execute_failure_and_not_a_connect_one() {
    let dsn = server!();
    let mut r = open(&dsn, false).await;
    let err = run(&mut r, "SELECT * FROM cb_sql_no_such_table")
        .await
        .expect_err("the table does not exist");
    assert_eq!(err.stage, ErrorStage::Execute);
}

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_connect_failure_never_echoes_the_password() {
    // `DriverError::new` runs every message through `dsn::redact`, and this is
    // the test that would notice its removal. tiberius embeds the address and
    // sometimes the whole config in its errors, so the shape that matters is a
    // failure built from a string that carries a secret.
    let driver = MssqlDriver::with_connect_timeout(Duration::from_secs(5));
    let spec = ConnectSpec {
        dsn: "Server=127.0.0.1,1;Database=x;User Id=sa;Password=hunter2;\
              TrustServerCertificate=true"
            .to_string(),
        writes_allowed: false,
    };
    let err = driver
        .connect(&spec)
        .await
        .err()
        .expect("port 1 answers nothing");
    assert!(
        !err.message.contains("hunter2"),
        "the password leaked: {}",
        err.message
    );
}

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_connect_that_never_answers_times_out_as_its_own_stage() {
    // Neither a refusal nor a user stop. `10.255.255.1` is reserved and
    // routable-but-silent on most networks; a machine where it answers will
    // fail this test loudly rather than quietly.
    let driver = MssqlDriver::with_connect_timeout(Duration::from_millis(400));
    let spec = ConnectSpec {
        dsn: "Server=10.255.255.1,1433;User Id=sa;Password=x".to_string(),
        writes_allowed: false,
    };
    let err = driver.connect(&spec).await.err().expect("nothing answers");
    assert!(
        matches!(err.stage, ErrorStage::ConnectTimeout | ErrorStage::Connect),
        "{err:?}"
    );
}

// ---------------------------------------------------------------------------
// 7. rows_affected stays honest
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn rows_affected_is_never_reported_by_this_driver() {
    // tiberius surfaces a row count only through `ExecuteResult`, which is the
    // non-streaming path. `rows.len()` is a different number wearing this one's
    // name — rows returned is not rows affected.
    let dsn = server!();
    let table = "cb_sql_affected";
    make_table(&dsn, table, "id INT NOT NULL").await;

    let mut w = open(&dsn, true).await;
    let result = ok(&mut w, &format!("INSERT INTO {table} (id) VALUES (1)")).await;
    assert_eq!(result.rows_affected, None);

    let result = ok(&mut w, &format!("SELECT id FROM {table}")).await;
    assert_eq!(result.rows_affected, None);
    assert_eq!(result.rows.len(), 1, "the rows themselves are still here");

    drop_table(&dsn, table).await;
}

// ---------------------------------------------------------------------------
// 8. A second result set is reported, not dropped
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_second_result_set_is_named_rather_than_silently_discarded() {
    let dsn = server!();
    let mut r = open(&dsn, false).await;
    let (outcome, events) = run_with(&mut r, "SELECT 1 AS a; SELECT 2 AS b", wide_open()).await;
    let result = outcome.unwrap().result().clone();
    assert_eq!(
        result.columns[0].name, "a",
        "the first set is the one shown"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            SqlEvent::Notice { message, .. } if message.contains("more than one result set")
        )),
        "a dropped result set must be reported: {events:?}"
    );
}

// ---------------------------------------------------------------------------
// 9. A statement with no result set still says it has no columns
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs a SQL Server; set CB_SQL_MSSQL_URL"]
async fn a_statement_with_no_result_set_reports_no_columns_rather_than_nothing() {
    let dsn = server!();
    let table = "cb_sql_nocols";
    make_table(&dsn, table, "id INT NOT NULL").await;

    let mut w = open(&dsn, true).await;
    let (outcome, events) = run_with(
        &mut w,
        &format!("INSERT INTO {table} (id) VALUES (1)"),
        wide_open(),
    )
    .await;
    assert!(outcome.is_ok());
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SqlEvent::Columns { columns, .. } if columns.is_empty())),
        "an absent Columns event is indistinguishable from one still in flight: {events:?}"
    );

    drop_table(&dsn, table).await;
}
