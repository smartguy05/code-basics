//! The first test in the SQL subsystem that touches a real database.
//!
//! Everything under `sql/` up to `driver/` is pure and is tested with no
//! database software installed. This file is the other half: it opens a real
//! SQLite file with the real driver and proves the properties that only an
//! end-to-end run can prove — that read-only is enforced by the *handle* and
//! not merely by the text guard, that NULL / empty / truncated survive the
//! whole journey as three distinct shapes, that a cap is reported rather than
//! applied silently, and that a failure message carries neither the file path
//! nor the connection string.
//!
//! SQLite needs no server, which is why it is the engine phase 1 picks.
//!
//! # Seeding
//!
//! `sqlx` is a dependency of `cb-core`, not a dev-dependency, so an integration
//! test cannot reach it. Every table here is therefore created *through the
//! driver itself*, with `writes_allowed: true` — which is not a workaround so
//! much as a second proof that the writes-allowed path really writes.
//!
//! The driver never creates a database (`create_if_missing(false)`), so the
//! file is created empty by the test first. A zero-length file is a valid empty
//! SQLite database.

use std::path::Path;

use cb_core::sql::driver::sqlite::SqliteDriver;
use cb_core::sql::driver::{
    stop_channel, ConnectSpec, DriverError, DriverResult, ErrorStage, Limits, ReadOnlyEnforcement,
    SqlConnection, SqlDriver, StatementOutcome,
};
use cb_core::sql::model::{RowCapReason, SqlEvent, SqlResultSet, SqlValue};
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// A generous ceiling, so a test that is about one cap is not accidentally
/// about the other.
fn wide_open() -> Limits {
    Limits {
        max_rows: 10_000,
        max_bytes: 64 * 1024 * 1024,
    }
}

/// Create the empty database file the driver will open.
fn make_db(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("probe.db");
    std::fs::File::create(&path).expect("create the empty database file");
    path
}

async fn open(path: &Path, writes_allowed: bool) -> Box<dyn SqlConnection> {
    let driver = SqliteDriver::new();
    let spec = ConnectSpec {
        dsn: path.to_string_lossy().into_owned(),
        writes_allowed,
    };
    driver
        .connect(&spec)
        .await
        .expect("the database file should open")
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

// ---------------------------------------------------------------------------
// 1. The happy path, end to end
// ---------------------------------------------------------------------------

#[tokio::test]
async fn creates_inserts_and_reads_rows_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let mut conn = open(&path, true).await;

    ok(&mut conn, "CREATE TABLE widget (id INTEGER, name TEXT)").await;
    ok(
        &mut conn,
        "INSERT INTO widget (id, name) VALUES (1, 'left'), (2, 'right')",
    )
    .await;

    let (outcome, events) = run_with(
        &mut conn,
        "SELECT id, name FROM widget ORDER BY id",
        wide_open(),
    )
    .await;
    let outcome = outcome.expect("the select should succeed");
    assert!(
        !outcome.was_stopped(),
        "a select nobody stopped must be Finished"
    );
    let result = outcome.result();

    assert_eq!(
        result
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["id", "name"]
    );
    // The declared affinities, reported by the driver rather than guessed.
    assert_eq!(result.columns[0].type_name.as_deref(), Some("INTEGER"));
    assert_eq!(result.columns[1].type_name.as_deref(), Some("TEXT"));

    assert_eq!(result.rows.len(), 2);
    assert_eq!(number(&result.rows[0][0]), "1");
    assert_eq!(
        result.rows[0][1],
        SqlValue::Text {
            text: "left".into(),
            truncated: false
        }
    );
    assert_eq!(number(&result.rows[1][0]), "2");
    assert_eq!(
        result.rows[1][1],
        SqlValue::Text {
            text: "right".into(),
            truncated: false
        }
    );

    // Every row is present: `None` is the report, and it is not the same fact
    // as an empty `RowCap`.
    assert!(result.row_cap.is_none(), "nothing was capped");
    assert!(result.is_complete());

    // The stream shape: columns are announced before any row, so an empty
    // result set still arrives with headers.
    assert!(matches!(
        events[0],
        SqlEvent::Started { statement_index: 0 }
    ));
    assert!(matches!(events[1], SqlEvent::Columns { .. }));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SqlEvent::Rows { rows, .. } if rows.len() == 2)),
        "rows should have streamed: {events:?}"
    );
    // The completion ends the stream and reports the count — it does not repeat
    // the rows, which already crossed once.
    let Some(SqlEvent::Completed { completion }) = events.last() else {
        panic!("expected the stream to end with a completion: {events:?}");
    };
    assert_eq!(completion.row_count, 2);
    assert_eq!(completion.statement_index, 0);
    assert!(completion.row_cap.is_none());
}

#[tokio::test]
async fn an_expression_column_reports_no_type_rather_than_the_null_type() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let mut conn = open(&path, false).await;

    let result = ok(&mut conn, "SELECT 1 + 1 AS calc").await;
    assert_eq!(number(scalar(&result)), "2");
    // sqlx spells "sqlite declared no type for this column" as the type name
    // `NULL`. `Some("NULL")` would claim the column's type *is* the null type.
    assert_eq!(result.columns[0].type_name, None);
}

// ---------------------------------------------------------------------------
// 2. NULL vs empty string vs truncated — three shapes, not one
// ---------------------------------------------------------------------------

#[tokio::test]
async fn null_empty_and_truncated_come_back_as_three_distinct_shapes() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let mut conn = open(&path, true).await;

    ok(&mut conn, "CREATE TABLE t (id INTEGER, v TEXT)").await;
    ok(&mut conn, "INSERT INTO t (id, v) VALUES (1, NULL)").await;
    ok(&mut conn, "INSERT INTO t (id, v) VALUES (2, '')").await;
    // Comfortably past `format::MAX_TEXT_CHARS` (4096).
    let long = "x".repeat(5000);
    ok(
        &mut conn,
        &format!("INSERT INTO t (id, v) VALUES (3, '{long}')"),
    )
    .await;

    let result = ok(&mut conn, "SELECT v FROM t ORDER BY id").await;
    assert_eq!(result.rows.len(), 3);

    let null = &result.rows[0][0];
    let empty = &result.rows[1][0];
    let cut = &result.rows[2][0];

    assert_eq!(*null, SqlValue::Null);
    assert_eq!(
        *empty,
        SqlValue::Text {
            text: String::new(),
            truncated: false
        }
    );
    match cut {
        SqlValue::Text { text, truncated } => {
            assert!(*truncated, "a 5000-character value must report the cut");
            assert_eq!(text.chars().count(), 4096);
        }
        other => panic!("expected truncated text, got {other:?}"),
    }

    // The point of the test: all three differ from each other, and they differ
    // on the wire too — a grid that renders any two the same has said something
    // untrue about the data.
    assert_ne!(null, empty);
    assert_ne!(empty, cut);
    assert_ne!(null, cut);
    let json: Vec<String> = [null, empty, cut]
        .iter()
        .map(|v| serde_json::to_string(v).unwrap())
        .collect();
    assert_eq!(json[0], r#"{"kind":"null"}"#);
    assert_eq!(json[1], r#"{"kind":"text","text":"","truncated":false}"#);
    assert!(
        json[2].contains(r#""truncated":true"#),
        "the truncated cell must say so: {}",
        json[2]
    );
}

// ---------------------------------------------------------------------------
// 3. Both caps, each naming the one that actually bit
// ---------------------------------------------------------------------------

/// Seed `n` narrow rows.
async fn seed_rows(conn: &mut Box<dyn SqlConnection>, n: usize) {
    ok(conn, "CREATE TABLE t (id INTEGER)").await;
    let values: Vec<String> = (1..=n).map(|i| format!("({i})")).collect();
    ok(
        conn,
        &format!("INSERT INTO t (id) VALUES {}", values.join(",")),
    )
    .await;
}

#[tokio::test]
async fn the_row_cap_is_reported_and_names_the_row_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let mut conn = open(&path, true).await;
    seed_rows(&mut conn, 5).await;

    let limits = Limits {
        max_rows: 2,
        max_bytes: 64 * 1024 * 1024,
    };
    let (outcome, _) = run_with(&mut conn, "SELECT id FROM t ORDER BY id", limits).await;
    let result = outcome.unwrap();
    let result = result.result();

    assert_eq!(result.rows.len(), 2, "only the delivered rows are present");
    let cap = result
        .row_cap
        .expect("the cap must be reported, not silent");
    assert_eq!(cap.reason, RowCapReason::RowLimit);
    assert_eq!(cap.limit, 2);
    assert!(!result.is_complete());
}

#[tokio::test]
async fn the_byte_cap_is_reported_and_names_the_byte_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let mut conn = open(&path, true).await;

    ok(&mut conn, "CREATE TABLE t (id INTEGER, v TEXT)").await;
    // Under `format::MAX_TEXT_CHARS`, so nothing is truncated and the whole
    // 3000 characters are charged to the byte budget.
    let wide = "y".repeat(3000);
    ok(
        &mut conn,
        &format!("INSERT INTO t (id, v) VALUES (1, '{wide}'), (2, '{wide}')"),
    )
    .await;

    let limits = Limits {
        max_rows: 10_000,
        max_bytes: 2000,
    };
    let (outcome, _) = run_with(&mut conn, "SELECT v FROM t ORDER BY id", limits).await;
    let result = outcome.unwrap();
    let result = result.result();

    assert_eq!(result.rows.len(), 1);
    let cap = result
        .row_cap
        .expect("the cap must be reported, not silent");
    // Not `RowLimit`: raising the row ceiling would return nothing extra, so
    // naming the wrong cap would send the reader to the wrong setting.
    assert_eq!(cap.reason, RowCapReason::ByteLimit);
    assert_eq!(cap.limit, 1);
}

#[tokio::test]
async fn a_result_ending_exactly_on_the_ceiling_is_not_reported_as_capped() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let mut conn = open(&path, true).await;
    seed_rows(&mut conn, 3).await;

    let limits = Limits {
        max_rows: 3,
        max_bytes: 64 * 1024 * 1024,
    };
    let (outcome, _) = run_with(&mut conn, "SELECT id FROM t ORDER BY id", limits).await;
    let result = outcome.unwrap();
    let result = result.result();

    assert_eq!(result.rows.len(), 3);
    // The complement of the two tests above, and the one that is easy to get
    // wrong: the ceiling was reached but no further row exists, so the answer
    // is complete and saying otherwise would be a lie pointing the other way.
    assert_eq!(result.row_cap, None);
    assert!(result.is_complete());
}

// ---------------------------------------------------------------------------
// 4. Writes disallowed: refused by the handle, and the database is unchanged
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_read_only_connection_refuses_an_insert_and_leaves_the_database_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());

    {
        let mut seed = open(&path, true).await;
        ok(&mut seed, "CREATE TABLE t (id INTEGER)").await;
        ok(&mut seed, "INSERT INTO t (id) VALUES (1)").await;
    }

    let mut conn = open(&path, false).await;
    // The promise this connection carries is the database's, not the guard's.
    assert_eq!(conn.read_only_enforcement(), ReadOnlyEnforcement::Driver);
    assert!(conn.read_only_enforcement().is_driver_enforced());

    // The guard does not run at this layer, so nothing but the handle's own
    // mode is standing between this statement and the file.
    let err = match run(&mut conn, "INSERT INTO t (id) VALUES (2)").await {
        Err(e) => e,
        Ok(other) => panic!("a read-only handle must refuse an insert, got {other:?}"),
    };
    assert_eq!(err.stage, ErrorStage::Execute);
    assert!(
        err.message.to_lowercase().contains("readonly")
            || err.message.to_lowercase().contains("read-only")
            || err.message.to_lowercase().contains("read only"),
        "sqlite should say it refused a write to a read-only database: {}",
        err.message
    );
    drop(conn);

    // The part that matters: not that the call errored, but that nothing landed.
    let mut after = open(&path, false).await;
    let count = ok(&mut after, "SELECT COUNT(*) FROM t").await;
    assert_eq!(
        number(scalar(&count)),
        "1",
        "the refused insert must not have reached the file"
    );
    let ids = ok(&mut after, "SELECT id FROM t ORDER BY id").await;
    assert_eq!(ids.rows.len(), 1);
    assert_eq!(number(&ids.rows[0][0]), "1");
}

#[tokio::test]
async fn a_read_only_connection_also_refuses_ddl() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    {
        let mut seed = open(&path, true).await;
        ok(&mut seed, "CREATE TABLE t (id INTEGER)").await;
    }

    let mut conn = open(&path, false).await;
    assert!(
        run(&mut conn, "CREATE TABLE sneaky (id INTEGER)")
            .await
            .is_err(),
        "the handle refuses every write, not only INSERT"
    );
    drop(conn);

    let mut after = open(&path, false).await;
    let names = ok(
        &mut after,
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    )
    .await;
    let names: Vec<String> = names
        .rows
        .iter()
        .map(|r| match &r[0] {
            SqlValue::Text { text, .. } => text.clone(),
            other => panic!("expected text, got {other:?}"),
        })
        .collect();
    assert_eq!(names, vec!["t".to_string()]);
}

#[tokio::test]
async fn the_driver_never_creates_the_database_it_was_asked_to_open() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("absent.db");
    let driver = SqliteDriver::new();
    let spec = ConnectSpec {
        dsn: missing.to_string_lossy().into_owned(),
        writes_allowed: true,
    };
    let err = driver
        .connect(&spec)
        .await
        .err()
        .expect("opening a database that does not exist must fail");
    assert_eq!(err.stage, ErrorStage::Connect);
    assert!(
        !missing.exists(),
        "a console that conjures an empty database has answered a question nobody asked"
    );
}

// ---------------------------------------------------------------------------
// 5. Writes allowed: the insert lands. And `rows_affected`.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_writable_connection_performs_the_insert() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let mut conn = open(&path, true).await;

    assert_eq!(
        conn.read_only_enforcement(),
        ReadOnlyEnforcement::NotEnforced
    );
    ok(&mut conn, "CREATE TABLE t (id INTEGER)").await;
    ok(&mut conn, "INSERT INTO t (id) VALUES (1)").await;

    // The insert is proved by the data, not by the call returning `Ok`.
    let count = ok(&mut conn, "SELECT COUNT(*) FROM t").await;
    assert_eq!(number(scalar(&count)), "1");
}

/// The SQLite driver reports `rows_affected: None` for **every** statement,
/// including a write that affected exactly one row.
///
/// This is a documented, deliberate decision in `driver/sqlite.rs`, not a gap:
/// sqlx reads the figure from `sqlite3_changes()`, which returns the change
/// count of the *last* statement rather than necessarily this one, so after a
/// `SELECT` it still holds the previous write's total. A number that may belong
/// to another statement is worse than no number.
///
/// The test pins that, rather than asserting the `Some(1)` an end-to-end brief
/// would expect, because the alternative is a driver that reports a figure it
/// cannot attribute. If the decision is ever revisited, this test is the thing
/// that has to be argued with — which is the point of writing it down.
#[tokio::test]
async fn sqlite_reports_no_rows_affected_for_any_statement() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let mut conn = open(&path, true).await;

    ok(&mut conn, "CREATE TABLE t (id INTEGER)").await;

    let insert = ok(&mut conn, "INSERT INTO t (id) VALUES (1)").await;
    assert_eq!(insert.rows_affected, None);

    // An UPDATE matching nothing, and a SELECT, would be `Some(0)` and `None`
    // on an engine that can attribute the count. On SQLite both are `None`, so
    // this driver cannot tell those two apart and does not pretend to.
    let update = ok(&mut conn, "UPDATE t SET id = 9 WHERE id = 12345").await;
    assert_eq!(update.rows_affected, None);
    let select = ok(&mut conn, "SELECT id FROM t").await;
    assert_eq!(select.rows_affected, None);

    // The UPDATE really did match nothing, which is what makes the `None` above
    // an inability to report rather than a report of zero.
    assert_eq!(number(&select.rows[0][0]), "1");
}

/// `Some(0)` and `None` are opposite facts, and the wire keeps them apart.
///
/// This is asserted at the serialisation boundary because no SQLite statement
/// can produce `Some(0)` (see the test above), and the distinction is a
/// property of the contract that the next driver depends on.
#[test]
fn zero_rows_affected_and_no_row_count_are_different_json() {
    let base = SqlResultSet {
        columns: Vec::new(),
        rows: Vec::new(),
        row_cap: None,
        rows_affected: None,
        elapsed_ms: 0,
        statement_index: 0,
    };
    let none = serde_json::to_string(&base).unwrap();
    let zero = serde_json::to_string(&SqlResultSet {
        rows_affected: Some(0),
        ..base
    })
    .unwrap();

    assert!(none.contains(r#""rowsAffected":null"#), "{none}");
    assert!(zero.contains(r#""rowsAffected":0"#), "{zero}");
    assert_ne!(none, zero);
}

// ---------------------------------------------------------------------------
// 6. Stopped is its own terminal state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stopping_yields_stopped_which_is_neither_finished_nor_failed() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let mut conn = open(&path, true).await;
    seed_rows(&mut conn, 20).await;

    // Signalled before the statement starts, so the outcome is deterministic:
    // the row loop checks the flag before its first await precisely because a
    // stop that arrived early would never show up as a *change*.
    let (tx, mut rx) = mpsc::channel(4096);
    let (stop_tx, stop_rx) = stop_channel();
    stop_tx.send(true).unwrap();
    let outcome = conn
        .run_statement("SELECT id FROM t ORDER BY id", 0, wide_open(), &tx, stop_rx)
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

    // The same query, unstopped, finishes — so `Stopped` is a real distinction
    // and not just what this query always does.
    let again = ok(&mut conn, "SELECT id FROM t ORDER BY id").await;
    assert_eq!(again.rows.len(), 20);

    // And neither is the third state: a failure is the `Err` arm, so it can
    // never be read as either of the two.
    assert!(run(&mut conn, "SELECT * FROM no_such_table").await.is_err());

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

// ---------------------------------------------------------------------------
// 7. A failure names neither the file nor the connection string
// ---------------------------------------------------------------------------

/// Every path fragment a leaked message could plausibly contain.
fn path_fragments(path: &Path) -> Vec<String> {
    let full = path.to_string_lossy().into_owned();
    let mut out = vec![full.clone(), full.replace('\\', "/")];
    if let Some(parent) = path.parent() {
        out.push(parent.to_string_lossy().into_owned());
    }
    if let Some(name) = path.file_name() {
        out.push(name.to_string_lossy().into_owned());
    }
    out
}

fn assert_leaks_nothing(err: &DriverError, path: &Path, secret: &str) {
    for fragment in path_fragments(path) {
        assert!(
            !err.message.contains(&fragment),
            "the failure message leaked a path fragment {fragment:?}: {}",
            err.message
        );
    }
    assert!(
        !err.message.contains(secret),
        "the failure message leaked the password: {}",
        err.message
    );
}

#[tokio::test]
async fn a_malformed_statement_fails_without_naming_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let mut conn = open(&path, true).await;

    let err = match run(&mut conn, "SELEKT * FROM nowhere").await {
        Err(e) => e,
        Ok(other) => panic!("a malformed statement must fail, got {other:?}"),
    };
    assert_eq!(err.stage, ErrorStage::Execute);
    assert!(
        !err.message.is_empty(),
        "the failure must say something useful"
    );
    assert_leaks_nothing(&err, &path, "hunter2");
}

/// A connect failure names neither the file nor the password.
///
/// Honest about what this proves: sqlite's own "unable to open database file"
/// carries no path and no DSN, so this test passes even with
/// [`DriverError::new`]'s redaction removed — verified by mutation. It is
/// therefore a *regression guard on sqlx's message*, not a proof that the
/// redaction path runs. The test below it is the one that proves that.
#[tokio::test]
async fn a_connect_failure_carries_no_password_from_the_connection_string() {
    let dir = tempfile::tempdir().unwrap();
    // Deliberately not created: the connect must fail so there is a message to
    // inspect. The DSN carries a SQLCipher-style password, which is exactly the
    // fragment a driver error routinely embeds whole.
    let missing = dir.path().join("absent.db");
    let driver = SqliteDriver::new();
    let spec = ConnectSpec {
        dsn: format!("Data Source={};Password=hunter2", missing.to_string_lossy()),
        writes_allowed: false,
    };
    let err = driver
        .connect(&spec)
        .await
        .err()
        .expect("a missing database file must fail to open");

    assert_eq!(err.stage, ErrorStage::Connect);
    assert_leaks_nothing(&err, &missing, "hunter2");
    assert!(
        !err.message.contains("Data Source"),
        "the connection string must not be echoed: {}",
        err.message
    );
}

#[tokio::test]
async fn an_unresolvable_connection_string_names_no_input_at_all() {
    let driver = SqliteDriver::new();
    let spec = ConnectSpec {
        dsn: "Server=localhost;User Id=sa;Password=hunter2".into(),
        writes_allowed: false,
    };
    let err = driver
        .connect(&spec)
        .await
        .err()
        .expect("a connection string naming no database file must fail");

    assert_eq!(err.stage, ErrorStage::Connect);
    assert!(!err.message.contains("hunter2"), "{}", err.message);
    assert!(!err.message.contains("localhost"), "{}", err.message);
    // Abstains by naming the *shape* it wanted, never by quoting what it got.
    assert!(
        err.message.contains("names no SQLite database file"),
        "{}",
        err.message
    );
}

/// The redaction path is really engaged on a real driver message.
///
/// Finding it took a probe: sqlx's SQLite errors mostly do *not* echo the
/// connection string, so the obvious "missing file" case cannot prove the
/// redaction runs (see the test above). One shape does echo — sqlx reports an
/// unknown *value* for a known query parameter verbatim:
///
/// ```text
/// error with configuration: unknown value "Password=hunter2" for `immutable`
/// ```
///
/// That message contains a `Password=` pair, so `dsn::redact` has something to
/// find, and this test fails loudly if `DriverError::new` ever stops calling it.
#[tokio::test]
async fn a_driver_message_that_does_carry_a_secret_is_redacted() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let url = format!(
        "sqlite:{}?immutable=Password=hunter2",
        path.to_string_lossy().replace('\\', "/")
    );
    let driver = SqliteDriver::new();
    let spec = ConnectSpec {
        dsn: url,
        writes_allowed: false,
    };
    let err = driver
        .connect(&spec)
        .await
        .err()
        .expect("an unknown value for `immutable` must fail to open");

    assert_eq!(err.stage, ErrorStage::Connect);
    // The evidence that the raw message really did contain the pair: the
    // redaction left its key behind and replaced its value.
    assert!(
        err.message.contains("Password=***"),
        "the secret should have been redacted in place: {}",
        err.message
    );
    assert!(
        !err.message.contains("hunter2"),
        "the password crossed into a failure message: {}",
        err.message
    );
}

/// A DSN query-parameter value that is *not* a `Password=`/`Pwd=` pair and not
/// URL userinfo is echoed by sqlx and passes through `dsn::redact` untouched.
///
/// This is not an assertion that the behaviour is right — it is the current
/// boundary of the redaction, written down so it is a decision rather than a
/// surprise. `redact` recognises two forms only; a secret a user put in
/// `?vfs=` or `?mode=` is outside both. Narrow (it needs a *known* parameter
/// carrying a bad value) but real, and it is the shape the next driver should
/// be checked against too.
#[tokio::test]
async fn a_non_password_parameter_value_is_not_redacted_and_this_is_the_known_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let url = format!(
        "sqlite:{}?vfs=hunter2",
        path.to_string_lossy().replace('\\', "/")
    );
    let driver = SqliteDriver::new();
    let spec = ConnectSpec {
        dsn: url,
        writes_allowed: false,
    };
    let err = driver
        .connect(&spec)
        .await
        .err()
        .expect("an unknown vfs must fail to open");

    assert!(
        err.message.contains("hunter2"),
        "if this now passes, `redact` has been widened — good; update this test \
         rather than deleting it: {}",
        err.message
    );
}

// ---------------------------------------------------------------------------
// 8. A dropped stop handle is not a user stop
// ---------------------------------------------------------------------------

/// Nobody stopped this query, so it must not come back as `Stopped`.
///
/// `watch::Receiver::changed()` resolves with `Err(RecvError)` the moment the
/// last sender goes away, and a `_ =` arm in a `select!` matches that as
/// readily as a real signal. Reading it as a stop reports a user action that
/// never happened *and* returns an arbitrary prefix of the rows — the two worst
/// answers at once, because the caller is told the shortfall was asked for.
#[tokio::test]
async fn dropping_the_stop_handle_is_not_reported_as_a_user_stop() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let mut conn = open(&path, true).await;
    seed_rows(&mut conn, 50).await;

    let (tx, _rx) = mpsc::channel(4096);
    let (stop_tx, stop_rx) = stop_channel();
    // The one thing that could ever ask this statement to stop is now gone.
    drop(stop_tx);

    let outcome = conn
        .run_statement("SELECT id FROM t ORDER BY id", 0, wide_open(), &tx, stop_rx)
        .await
        .expect("a dropped stop handle is not a failure");

    assert!(
        !outcome.was_stopped(),
        "no user stopped this query: {outcome:?}"
    );
    let result = outcome.result();
    assert_eq!(result.rows.len(), 50, "every row must still arrive");
    assert_eq!(result.row_cap, None, "nothing was capped either");
}

/// A stop that was signalled and *then* dropped is still a stop.
///
/// The complement of the test above: the fix must not throw away a genuine
/// signal just because the sender did not outlive it.
#[tokio::test]
async fn a_signalled_stop_whose_handle_is_then_dropped_is_still_a_stop() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let mut conn = open(&path, true).await;
    seed_rows(&mut conn, 20).await;

    let (tx, _rx) = mpsc::channel(4096);
    let (stop_tx, stop_rx) = stop_channel();
    stop_tx.send(true).unwrap();
    drop(stop_tx);

    let outcome = conn
        .run_statement("SELECT id FROM t ORDER BY id", 0, wide_open(), &tx, stop_rx)
        .await
        .expect("stopping is not a failure");
    assert!(outcome.was_stopped(), "the signal was real: {outcome:?}");
}

// ---------------------------------------------------------------------------
// 9. One enormous cell is charged at what the database holds
// ---------------------------------------------------------------------------

/// The byte budget must see the value, not the thumbnail of it.
///
/// A 2 MiB text cell reaches the grid as at most `MAX_TEXT_BYTES`, so charging
/// the *rendered* size lets an unbounded amount of data stream past an 8 MiB
/// budget while the tracker reads a few kilobytes a row. The cap is charged at
/// the size the database actually holds, which is why one such row exhausts a
/// 64 KiB budget.
#[tokio::test]
async fn one_enormous_text_cell_is_charged_at_its_true_size() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let mut conn = open(&path, true).await;

    ok(&mut conn, "CREATE TABLE t (id INTEGER, v TEXT)").await;
    // `hex(zeroblob(n))` is 2n characters of `0`, built inside sqlite so the
    // statement text stays small.
    ok(
        &mut conn,
        "INSERT INTO t (id, v) VALUES (1, hex(zeroblob(1048576))), (2, hex(zeroblob(1048576)))",
    )
    .await;

    let limits = Limits {
        max_rows: 10_000,
        // Far above what one rendered cell weighs, far below what one holds.
        max_bytes: 64 * 1024,
    };
    let (outcome, _) = run_with(&mut conn, "SELECT v FROM t ORDER BY id", limits).await;
    let result = outcome.unwrap();
    let result = result.result();

    assert_eq!(
        result.rows.len(),
        1,
        "the first row alone spends the budget"
    );
    let cap = result
        .row_cap
        .expect("the cap must be reported, not silent");
    assert_eq!(cap.reason, RowCapReason::ByteLimit);
    assert_eq!(cap.limit, 1);

    // And the delivered cell is still the honest truncated shape, not a copy of
    // two mebibytes.
    match &result.rows[0][0] {
        SqlValue::Text { text, truncated } => {
            assert!(*truncated, "a 2 MiB value must report the cut");
            assert!(
                text.len() <= cb_core::sql::format::MAX_TEXT_BYTES,
                "the rendered cell is bounded: {} bytes",
                text.len()
            );
        }
        other => panic!("expected truncated text, got {other:?}"),
    }
}

/// A blob small enough to copy reports its exact size; a huge one abstains.
///
/// `SqlValue::Bytes` carries `byteLength` — *the size of the whole value* — so
/// a shortened copy cannot be handed to the renderer: it would report the
/// prefix's length as the value's length, which is a lie rather than a cut.
/// The oversize case therefore says out loud that it did not read the contents,
/// and still charges the budget at the real size.
#[tokio::test]
async fn a_blob_past_the_per_cell_read_limit_abstains_rather_than_lying_about_its_size() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    let mut conn = open(&path, true).await;

    let small = ok(&mut conn, "SELECT zeroblob(1024)").await;
    match scalar(&small) {
        SqlValue::Bytes {
            byte_length,
            truncated,
            ..
        } => {
            assert_eq!(*byte_length, 1024, "the true length, not the rendered one");
            assert!(*truncated, "only the first bytes are rendered as hex");
        }
        other => panic!("expected bytes, got {other:?}"),
    }

    let oversize = cb_core::sql::driver::MAX_BLOB_COPY_BYTES + 1;
    let huge = ok(&mut conn, &format!("SELECT zeroblob({oversize})")).await;
    match scalar(&huge) {
        SqlValue::Unavailable { reason } => {
            assert!(
                reason.contains(&oversize.to_string()),
                "the reason must name the size it refused to read: {reason}"
            );
        }
        other => panic!("expected an unavailable cell, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 10. Connecting has a deadline
// ---------------------------------------------------------------------------

/// The deadline is carried by the driver every caller builds.
///
/// The *behaviour* is proved where it can be proved deterministically: in the
/// unit tests, over `within_connect_deadline` with a future that never answers,
/// and by a source pin that keeps `connect` going through it. It deliberately
/// is **not** proved here by giving a real connect a zero deadline — that test
/// was written, run, and removed: `tokio::time::timeout` returns the inner
/// future’s answer whenever that future is ready on its first poll, whatever
/// the deadline, so a local SQLite file wins the race a few times in sixty.
/// The same trap is already recorded against `sql_test`’s own timeout; this is
/// the second time it has been walked into.
#[tokio::test]
async fn every_connection_this_driver_opens_carries_a_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_db(dir.path());
    assert!(
        SqliteDriver::new().connect_timeout() > std::time::Duration::ZERO,
        "an unbounded connect is the bug this exists to stop"
    );

    // And a generous deadline changes nothing about an ordinary open.
    let driver = SqliteDriver::with_connect_timeout(std::time::Duration::from_secs(30));
    let spec = ConnectSpec {
        dsn: path.to_string_lossy().into_owned(),
        writes_allowed: false,
    };
    let conn = driver.connect(&spec).await.expect("the file should open");
    assert_eq!(
        conn.read_only_enforcement(),
        ReadOnlyEnforcement::Driver,
        "the deadline must not have changed how the handle was opened"
    );
}
