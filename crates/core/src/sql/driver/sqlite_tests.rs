use super::*;
use crate::sql::driver::{ConnectSpec, ReadOnlyEnforcement, SqlDriver};

fn spec(dsn: &str, writes_allowed: bool) -> ConnectSpec {
    ConnectSpec {
        dsn: dsn.to_string(),
        writes_allowed,
    }
}

// ---------------------------------------------------------------------------
// Read-only wording — the reason this is data on the driver
// ---------------------------------------------------------------------------

#[test]
fn a_read_only_sqlite_connection_says_the_driver_opened_it_that_way() {
    let d = SqliteDriver::new();
    let e = d.read_only_enforcement(false);
    assert_eq!(e, ReadOnlyEnforcement::Driver);
    assert_eq!(e.label(), "Opened read-only by the driver");
    assert!(e.is_driver_enforced());
}

#[test]
fn allowing_writes_drops_the_claim_entirely_rather_than_weakening_it() {
    // Not GuardOnly: writes are permitted, so nothing is enforcing read-only
    // and saying "read-only by text check" would be false in the other
    // direction.
    let d = SqliteDriver::new();
    assert_eq!(
        d.read_only_enforcement(true),
        ReadOnlyEnforcement::NotEnforced
    );
}

#[test]
fn the_driver_reports_the_sqlite_engine() {
    assert_eq!(SqliteDriver::new().engine(), SqlEngine::Sqlite);
}

// ---------------------------------------------------------------------------
// Options — the actual enforcement, checked without opening a file
// ---------------------------------------------------------------------------

#[test]
fn options_resolve_for_every_accepted_connection_string_shape() {
    for dsn in [
        "sqlite://data/app.db",
        "sqlite:app.db",
        "Data Source=app.db",
        "Data Source=app.db;Cache=Shared",
        "C:\\work\\app.db",
        ":memory:",
    ] {
        assert!(
            SqliteDriver::options(&spec(dsn, false)).is_ok(),
            "should resolve: {dsn}"
        );
    }
}

#[test]
fn a_connection_string_naming_no_database_is_refused_without_echoing_it() {
    let secret = "Server=db;Password=hunter2";
    let err = SqliteDriver::options(&spec(secret, false)).unwrap_err();
    assert!(!err.message.contains("hunter2"), "{}", err.message);
    assert!(!err.message.contains("Server=db"), "{}", err.message);
    assert_eq!(err.stage, crate::sql::driver::ErrorStage::Connect);
}

#[test]
fn an_empty_connection_string_is_its_own_refusal() {
    assert_eq!(connect_target("   "), Err(TargetError::Empty));
    assert_ne!(
        TargetError::Empty.message(),
        TargetError::NoDatabaseNamed.message()
    );
}

// ---------------------------------------------------------------------------
// Target resolution
// ---------------------------------------------------------------------------

#[test]
fn a_sqlite_url_is_handed_to_sqlx_whole_so_its_own_parameters_survive() {
    assert_eq!(
        connect_target("sqlite://app.db?mode=ro&immutable=true"),
        Ok(SqliteTarget::Url(
            "sqlite://app.db?mode=ro&immutable=true".to_string()
        ))
    );
    assert_eq!(
        connect_target("SQLITE:app.db"),
        Ok(SqliteTarget::Url("SQLITE:app.db".to_string())),
        "the scheme is case-insensitive"
    );
}

#[test]
fn an_ado_net_data_source_keyword_names_the_file() {
    assert_eq!(
        connect_target("Data Source=./data/app.db;Cache=Shared"),
        Ok(SqliteTarget::Path("./data/app.db".to_string()))
    );
    assert_eq!(
        connect_target("DATASOURCE = app.db"),
        Ok(SqliteTarget::Path("app.db".to_string()))
    );
    assert_eq!(
        connect_target("Filename=app.db"),
        Ok(SqliteTarget::Path("app.db".to_string()))
    );
}

#[test]
fn a_bare_path_is_a_path_including_a_windows_drive_letter() {
    // `C:\work\app.db` contains a colon and no `=`; it must not be mistaken for
    // a URL scheme, and must not be routed through keyword splitting.
    assert_eq!(
        connect_target("C:\\work\\app.db"),
        Ok(SqliteTarget::Path("C:\\work\\app.db".to_string()))
    );
    assert_eq!(
        connect_target(":memory:"),
        Ok(SqliteTarget::Path(":memory:".to_string()))
    );
}

#[test]
fn keyword_pairs_that_name_no_file_are_refused_rather_than_guessed_at() {
    // `Server=` looks like a plausible thing to open. It is not a SQLite file,
    // and opening whatever is nearest would be the wrong file, not a near miss.
    assert_eq!(
        connect_target("Server=localhost;Database=app"),
        Err(TargetError::NoDatabaseNamed)
    );
    assert_eq!(
        connect_target("Data Source=;Cache=Shared"),
        Err(TargetError::NoDatabaseNamed),
        "an empty value names nothing"
    );
}

// ---------------------------------------------------------------------------
// Column type names
// ---------------------------------------------------------------------------

#[test]
fn sqlxs_spelling_of_no_declared_type_crosses_as_not_reported() {
    // `SELECT 1 + 1` has no declared type; sqlx reports it as `NULL`. Passing
    // that through as `Some("NULL")` would claim the column's type *is* the
    // null type, which is a different and untrue statement.
    assert_eq!(column_type_name("NULL"), None);
    assert_eq!(column_type_name("null"), None);
    assert_eq!(column_type_name("  "), None);
}

#[test]
fn a_real_type_name_is_passed_through_as_the_server_spelled_it() {
    assert_eq!(column_type_name("INTEGER"), Some("INTEGER".to_string()));
    assert_eq!(column_type_name("BLOB"), Some("BLOB".to_string()));
    assert_eq!(
        column_type_name("DATETIME"),
        Some("DATETIME".to_string()),
        "a declared type this build has no decoder for is still reported"
    );
}

// ---------------------------------------------------------------------------
// The connect deadline
// ---------------------------------------------------------------------------

#[test]
fn a_driver_carries_the_default_deadline_unless_asked_for_another() {
    assert_eq!(
        SqliteDriver::new().connect_timeout(),
        crate::sql::driver::DEFAULT_CONNECT_TIMEOUT
    );
    assert_eq!(
        SqliteDriver::default().connect_timeout(),
        SqliteDriver::new().connect_timeout()
    );
    let brief = SqliteDriver::with_connect_timeout(std::time::Duration::from_millis(250));
    assert_eq!(
        brief.connect_timeout(),
        std::time::Duration::from_millis(250)
    );
}

// ---------------------------------------------------------------------------
// The stop channel: three answers, not two
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_dropped_stop_sender_is_reported_as_gone_and_never_as_a_stop() {
    let (tx, mut rx) = crate::sql::driver::stop_channel();
    drop(tx);
    assert_eq!(
        stop_wait(&mut rx).await,
        StopWait::Gone,
        "nobody stopped anything: the channel merely died"
    );
}

#[tokio::test]
async fn a_signalled_stop_is_a_stop_whether_or_not_its_sender_survives() {
    let (tx, mut rx) = crate::sql::driver::stop_channel();
    tx.send(true).unwrap();
    assert_eq!(stop_wait(&mut rx).await, StopWait::Stop);

    let (tx, mut rx) = crate::sql::driver::stop_channel();
    tx.send(true).unwrap();
    drop(tx);
    assert_eq!(
        stop_wait(&mut rx).await,
        StopWait::Stop,
        "the signal was real before the sender went away"
    );
}

#[tokio::test]
async fn a_change_that_is_not_a_stop_leaves_the_channel_alive() {
    let (tx, mut rx) = crate::sql::driver::stop_channel();
    tx.send(false).unwrap();
    assert_eq!(
        stop_wait(&mut rx).await,
        StopWait::Alive,
        "a wake-up is not a stop, and the sender can still send one"
    );
}

// ---------------------------------------------------------------------------
// The per-cell read limit
// ---------------------------------------------------------------------------

#[test]
fn refusing_an_oversized_binary_value_names_the_two_sizes_and_nothing_else() {
    let reason = oversize_blob(4 * 1024 * 1024);
    assert!(reason.contains(&(4 * 1024 * 1024).to_string()), "{reason}");
    assert!(
        reason.contains(&crate::sql::driver::caps::MAX_BLOB_COPY_BYTES.to_string()),
        "{reason}"
    );
    // It says what it did *not* do, so the cell cannot be read as an empty one.
    assert!(reason.contains("were not read"), "{reason}");
}

/// `connect` opens the handle **inside** the deadline, and nowhere else.
///
/// A source pin, for the same reason `commands::sql` has one: the behaviour it
/// guards cannot be executed deterministically — `tokio::time::timeout` hands
/// back the inner future’s answer whenever that future is ready on its first
/// poll, so a real connect under a zero deadline succeeds a few times in sixty
/// (measured). What *can* be pinned is that the one call which opens a handle
/// is the one wrapped by `within_connect_deadline`; a future edit that lifts it
/// out restores an unbounded connect, which is invisible until a database
/// stops answering.
#[test]
fn the_only_call_that_opens_a_handle_is_the_one_under_the_deadline() {
    let source = include_str!("sqlite.rs");
    let opens: Vec<&str> = source
        .match_indices("connect_with(")
        .map(|(_, m)| m)
        .collect();
    assert_eq!(
        opens.len(),
        1,
        "there is exactly one place a handle is opened"
    );
    assert!(
        source.contains(
            "within_connect_deadline(deadline, async {\n                SqliteConnection::connect_with(&options)"
        ),
        "the open must sit inside the deadline; if this fails because the code was \
         reformatted, re-pin it rather than deleting it"
    );
}
