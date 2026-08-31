use std::borrow::Cow;

use tiberius::numeric::Numeric;
use tiberius::time::{Date, DateTime, DateTime2, DateTimeOffset, SmallDateTime, Time};
use tiberius::xml::XmlData;
use tiberius::{ColumnData, ColumnType, Uuid};

use super::*;
use crate::sql::driver::caps::{MAX_BLOB_COPY_BYTES, TEXT_COPY_BYTES};
use crate::sql::driver::{EnforcementStrength, ReadOnlyEnforcement, SqlDriver};
use crate::sql::format::{format_cell, SqlValue as Rendered};

// ---------------------------------------------------------------------------
// The read-only story: the weakest of the three engines, and it must say so
// ---------------------------------------------------------------------------

#[test]
fn a_read_only_sql_server_connection_says_the_statement_is_rolled_back() {
    let d = MssqlDriver::new();
    let e = d.read_only_enforcement(false);
    assert_eq!(e, ReadOnlyEnforcement::RolledBackTransaction);
    assert_eq!(e.strength(), EnforcementStrength::Undone);
}

#[test]
fn the_wording_never_claims_a_write_is_stopped_before_it_runs() {
    // The whole point of this engine's descriptor. A rollback undoes a write
    // that *ran*; borrowing SQLite's or PostgreSQL's promise here would tell
    // the user the server refused something it in fact executed.
    let e = MssqlDriver::new().read_only_enforcement(false);
    assert!(!e.refuses_writes_before_they_run());
    assert!(!e.is_driver_enforced());
    assert!(!e.label().contains("before it runs"));
    assert!(!e.detail().contains("rejects a write before it runs"));
}

#[test]
fn the_wording_never_calls_the_connection_safe_or_protected() {
    // There is a test elsewhere asserting no UI wording says "safe" or
    // "sandboxed". This is the driver-side half of that promise.
    let e = MssqlDriver::new().read_only_enforcement(false);
    for text in [e.label(), e.detail()] {
        let lower = text.to_lowercase();
        assert!(!lower.contains("safe"), "must not say safe: {text}");
        assert!(
            !lower.contains("protected"),
            "must not say protected: {text}"
        );
        assert!(!lower.contains("guarantee"), "must not promise: {text}");
    }
    // And it must say, in as many words, what a rollback is not.
    assert!(e.detail().contains("not a sandbox"));
    assert!(e.detail().contains("linked server"));
}

#[test]
fn allowing_writes_drops_the_claim_entirely_rather_than_weakening_it() {
    assert_eq!(
        MssqlDriver::new().read_only_enforcement(true),
        ReadOnlyEnforcement::NotEnforced
    );
}

#[test]
fn the_driver_reports_the_sql_server_engine() {
    assert_eq!(MssqlDriver::new().engine(), SqlEngine::SqlServer);
}

#[test]
fn a_new_driver_carries_the_shared_connect_deadline() {
    assert_eq!(
        MssqlDriver::new().connect_timeout(),
        super::DEFAULT_CONNECT_TIMEOUT
    );
    assert_eq!(
        MssqlDriver::with_connect_timeout(Duration::from_millis(25)).connect_timeout(),
        Duration::from_millis(25)
    );
}

// ---------------------------------------------------------------------------
// The transaction the read-only story rests on
// ---------------------------------------------------------------------------

#[test]
fn the_rollback_cannot_fail_on_a_transaction_the_statement_already_closed() {
    // The user's own SQL may COMMIT, and an error may abort the transaction on
    // the server. An unguarded ROLLBACK then raises error 3903, which would be
    // reported as "the transaction may still be open" — the opposite of the
    // truth, and the sort of false alarm that trains people to ignore it.
    assert!(ROLLBACK_SQL.contains("@@TRANCOUNT"));
    assert!(ROLLBACK_SQL.contains("ROLLBACK"));
}

#[test]
fn the_transaction_statements_are_plain_and_carry_no_user_text() {
    assert_eq!(BEGIN_SQL, "BEGIN TRANSACTION");
    assert!(!ROLLBACK_SQL.contains("{}"));
}

#[test]
fn a_stopped_statement_says_the_rollback_was_not_sent_and_why() {
    // Sending the rollback would first have to drain the very stream the user
    // asked to stop reading. Saying nothing would leave an open transaction
    // the reader does not know about. So it is a Notice, and it names both.
    let note = stopped_in_transaction_notice();
    assert!(note.contains("stopped"));
    assert!(note.contains("rolled back"));
    assert!(note.contains("closes") || note.contains("close"));
    // It must not claim the rollback happened here.
    assert!(!note.contains("was rolled back."));
}

#[test]
fn a_failed_rollback_says_the_write_may_still_be_there() {
    let message = rollback_failed_message("connection reset");
    assert!(message.contains("may still be open"));
    assert!(message.contains("connection reset"));
    // Never reassuring.
    assert!(!message.to_lowercase().contains("safe"));
}

#[test]
fn extra_result_sets_are_reported_rather_than_dropped() {
    let note = extra_result_sets_notice();
    assert!(note.contains("first"));
    assert!(note.to_lowercase().contains("result set"));
}

// ---------------------------------------------------------------------------
// Integrated authentication — two causes that must not collapse into one
// ---------------------------------------------------------------------------

#[test]
fn integrated_security_is_recognised_in_every_spelling_the_server_accepts() {
    for dsn in [
        "Server=db;Integrated Security=true",
        "Server=db;IntegratedSecurity=True",
        "Server=db;Integrated Security=SSPI",
        "Server=db;integratedsecurity=yes",
    ] {
        match integrated_auth(dsn) {
            IntegratedAuth::Requested {
                understood_by_tiberius,
                ..
            } => assert!(understood_by_tiberius, "tiberius reads this key: {dsn}"),
            other => panic!("{dsn} asks for integrated auth, got {other:?}"),
        }
    }
}

#[test]
fn trusted_connection_is_integrated_auth_that_the_driver_crate_does_not_read() {
    // tiberius' ADO parser knows only `IntegratedSecurity`/`Integrated
    // Security`. `Trusted_Connection` — the spelling almost every .NET
    // connection string uses — falls through to an empty SQL login, which
    // fails with "Login failed for user ''". That is a different cause wearing
    // the same message, so it is detected here instead.
    for dsn in [
        "Server=db;Trusted_Connection=True",
        "Server=db;trusted_connection=yes",
        "Server=db;Trusted Connection=True",
    ] {
        match integrated_auth(dsn) {
            IntegratedAuth::Requested {
                understood_by_tiberius,
                ref key,
            } => {
                assert!(!understood_by_tiberius, "{dsn}");
                assert!(key.to_lowercase().contains("trusted"), "{dsn} -> {key}");
            }
            other => panic!("{dsn} asks for integrated auth, got {other:?}"),
        }
    }
}

#[test]
fn an_explicit_no_is_not_a_request() {
    for dsn in [
        "Server=db;User Id=sa;Password=x;Integrated Security=false",
        "Server=db;Trusted_Connection=no",
        "Server=db;User Id=sa;Password=x",
    ] {
        assert_eq!(integrated_auth(dsn), IntegratedAuth::NotRequested, "{dsn}");
    }
}

#[test]
fn an_unreadable_integrated_security_value_is_its_own_answer_not_a_no() {
    // Reading `Integrated Security=maybe` as "not requested" would silently
    // attempt a SQL login the string never asked for.
    match integrated_auth("Server=db;Integrated Security=maybe") {
        IntegratedAuth::UnreadableValue { ref key } => {
            assert!(key.to_lowercase().contains("integrated"))
        }
        other => panic!("expected an unreadable value, got {other:?}"),
    }
}

#[test]
fn a_quoted_password_cannot_smuggle_a_key_past_the_detector() {
    // ADO.NET quotes a value containing `;`. Splitting naively on `;` reads
    // `Trusted_Connection=True` out of the *password* and refuses a connection
    // that never asked for integrated auth.
    let dsn = "Server=db;Password='a;Trusted_Connection=True';User Id=sa";
    assert_eq!(integrated_auth(dsn), IntegratedAuth::NotRequested);
    let dsn = "Server=db;Password=\"a;Integrated Security=SSPI\";User Id=sa";
    assert_eq!(integrated_auth(dsn), IntegratedAuth::NotRequested);
}

#[test]
fn integrated_auth_is_refused_before_a_socket_opens_where_it_cannot_work() {
    // `supported` is a parameter rather than a `cfg!` so both platforms'
    // behaviour is provable on either platform.
    let dsn = "Server=db;Trusted_Connection=True";
    let err = auth_preflight(dsn, false).expect_err("integrated auth is Windows-only");
    assert_eq!(err.stage, ErrorStage::Connect);
    let m = err.message.to_lowercase();
    assert!(m.contains("windows"), "{}", err.message);
    assert!(m.contains("integrated"), "{}", err.message);
    // It must name the way forward, or the reader is left guessing.
    assert!(
        m.contains("user id") || m.contains("password"),
        "{}",
        err.message
    );
    // And it must name the key that asked, so the user can find it.
    assert!(m.contains("trusted"), "{}", err.message);

    assert!(auth_preflight(dsn, true).is_ok());
}

#[test]
fn the_refusal_never_repeats_the_connection_string() {
    let dsn = "Server=db;User Id=sa;Password=hunter2;Integrated Security=SSPI";
    let err = auth_preflight(dsn, false).expect_err("Windows-only");
    assert!(!err.message.contains("hunter2"), "{}", err.message);
    assert!(!err.message.contains("db"), "{}", err.message);
}

#[test]
fn an_unreadable_integrated_security_value_is_refused_rather_than_run_as_a_sql_login() {
    let dsn = "Server=db;Integrated Security=perhaps";
    for supported in [true, false] {
        let err = auth_preflight(dsn, supported).expect_err("not a readable answer");
        assert_eq!(err.stage, ErrorStage::Connect);
        assert!(
            err.message.to_lowercase().contains("integrated"),
            "{}",
            err.message
        );
        assert!(!err.message.contains("perhaps"), "{}", err.message);
    }
}

#[test]
fn a_string_with_no_integrated_key_passes_the_preflight_on_every_platform() {
    let dsn = "Server=db;User Id=sa;Password=hunter2";
    assert!(auth_preflight(dsn, false).is_ok());
    assert!(auth_preflight(dsn, true).is_ok());
}

#[test]
fn the_stated_credentials_are_read_the_way_the_driver_crate_reads_them() {
    // Mirrors tiberius' own key list, because on Windows this is what decides
    // between `AuthMethod::Integrated` and `AuthMethod::windows(user, pw)` for
    // the spelling tiberius cannot see.
    let (user, password) = stated_credentials("Server=db;User Id=sa;Password=pw");
    assert_eq!(user.as_deref(), Some("sa"));
    assert_eq!(password.as_deref(), Some("pw"));

    let (user, password) = stated_credentials("Server=db;uid=DOM\\me;pwd=pw");
    assert_eq!(user.as_deref(), Some("DOM\\me"));
    assert_eq!(password.as_deref(), Some("pw"));

    let (user, password) = stated_credentials("Server=db;Trusted_Connection=True");
    assert_eq!(user, None);
    assert_eq!(password, None);
}

// ---------------------------------------------------------------------------
// Where to connect
// ---------------------------------------------------------------------------

#[test]
fn a_backslash_in_the_server_value_names_an_instance() {
    for dsn in [
        "Server=tcp:host\\SQLEXPRESS",
        "Server=host\\SQLEXPRESS,1433",
        "Data Source=host\\SQLEXPRESS",
        "Data Source=(local)\\SQLEXPRESS;Trusted_Connection=True",
    ] {
        assert_eq!(named_instance(dsn).as_deref(), Some("SQLEXPRESS"), "{dsn}");
    }
}

#[test]
fn a_plain_host_names_no_instance() {
    for dsn in [
        "Server=tcp:host,1433",
        "Server=host",
        "Data Source=127.0.0.1,1433;User Id=sa;Password=x",
        "User Id=sa;Password=x",
    ] {
        assert_eq!(named_instance(dsn), None, "{dsn}");
    }
}

#[test]
fn a_jdbc_url_is_read_as_jdbc_and_everything_else_as_ado() {
    assert_eq!(dialect("jdbc:sqlserver://host:1433"), Dialect::Jdbc);
    assert_eq!(dialect("JDBC:sqlserver://host:1433"), Dialect::Jdbc);
    assert_eq!(dialect("Server=host;Database=app"), Dialect::Ado);
}

// ---------------------------------------------------------------------------
// Column type names
// ---------------------------------------------------------------------------

#[test]
fn a_column_with_no_type_is_not_reported_rather_than_named_null() {
    // Same rule as SQLite's `column_type_name`: `Some("null")` claims the
    // column's type *is* the null type, which is a different and untrue claim.
    assert_eq!(column_type_name(ColumnType::Null), None);
}

#[test]
fn a_token_that_names_exactly_one_sql_type_is_named_by_it() {
    for (token, name) in [
        (ColumnType::Bit, "bit"),
        (ColumnType::Bitn, "bit"),
        (ColumnType::Int1, "tinyint"),
        (ColumnType::Int2, "smallint"),
        (ColumnType::Int4, "int"),
        (ColumnType::Int8, "bigint"),
        (ColumnType::Float4, "real"),
        (ColumnType::Float8, "float"),
        (ColumnType::Money, "money"),
        (ColumnType::Money4, "smallmoney"),
        (ColumnType::Datetime, "datetime"),
        (ColumnType::Datetime4, "smalldatetime"),
        (ColumnType::Daten, "date"),
        (ColumnType::Timen, "time"),
        (ColumnType::Datetime2, "datetime2"),
        (ColumnType::DatetimeOffsetn, "datetimeoffset"),
        (ColumnType::Decimaln, "decimal"),
        (ColumnType::Numericn, "numeric"),
        (ColumnType::Guid, "uniqueidentifier"),
        (ColumnType::BigVarBin, "varbinary"),
        (ColumnType::BigBinary, "binary"),
        (ColumnType::BigVarChar, "varchar"),
        (ColumnType::BigChar, "char"),
        (ColumnType::NVarchar, "nvarchar"),
        (ColumnType::NChar, "nchar"),
        (ColumnType::Xml, "xml"),
        (ColumnType::Text, "text"),
        (ColumnType::Image, "image"),
        (ColumnType::NText, "ntext"),
        (ColumnType::SSVariant, "sql_variant"),
        (ColumnType::Udt, "udt"),
    ] {
        assert_eq!(
            column_type_name(token).as_deref(),
            Some(name),
            "{token:?} should be named {name}"
        );
    }
}

#[test]
fn a_token_that_names_a_family_is_not_narrowed_into_a_guess() {
    // `Intn` is a nullable integer of unstated width — tinyint through bigint.
    // Reporting "int" would name a width the server never sent.
    for (token, name) in [
        (ColumnType::Intn, "intn"),
        (ColumnType::Floatn, "floatn"),
        (ColumnType::Datetimen, "datetimen"),
    ] {
        assert_eq!(column_type_name(token).as_deref(), Some(name), "{token:?}");
    }
}

// ---------------------------------------------------------------------------
// Decoding: driver value in, `Cell` out — no rendering decided here
// ---------------------------------------------------------------------------

/// Every cell goes through `format_cell` in the row loop, so the tests check
/// what actually reaches the wire.
fn rendered(data: &ColumnData<'_>) -> Rendered {
    format_cell(decode_cell(data).0)
}

fn source_bytes(data: &ColumnData<'_>) -> u64 {
    decode_cell(data).1
}

#[test]
fn a_null_of_every_storage_class_stays_a_null() {
    let nulls: Vec<ColumnData<'static>> = vec![
        ColumnData::U8(None),
        ColumnData::I16(None),
        ColumnData::I32(None),
        ColumnData::I64(None),
        ColumnData::F32(None),
        ColumnData::F64(None),
        ColumnData::Bit(None),
        ColumnData::String(None),
        ColumnData::Guid(None),
        ColumnData::Binary(None),
        ColumnData::Numeric(None),
        ColumnData::Xml(None),
        ColumnData::DateTime(None),
        ColumnData::SmallDateTime(None),
        ColumnData::Time(None),
        ColumnData::Date(None),
        ColumnData::DateTime2(None),
        ColumnData::DateTimeOffset(None),
    ];
    for data in &nulls {
        assert_eq!(decode_cell(data).0, Cell::Null, "{data:?}");
        assert_eq!(source_bytes(data), 0, "{data:?}");
    }
}

#[test]
fn integers_of_every_width_cross_as_numbers() {
    for (data, text) in [
        (ColumnData::U8(Some(255)), "255"),
        (ColumnData::I16(Some(-32768)), "-32768"),
        (ColumnData::I32(Some(7)), "7"),
        (ColumnData::I64(Some(i64::MIN)), "-9223372036854775808"),
    ] {
        assert_eq!(
            rendered(&data),
            Rendered::Number {
                text: text.to_string()
            }
        );
    }
}

#[test]
fn a_bit_crosses_as_a_boolean_and_not_as_a_number() {
    assert_eq!(
        rendered(&ColumnData::Bit(Some(true))),
        Rendered::Bool { value: true }
    );
}

// `3.14` here is a decimal chosen for its widening artefact, not an
// approximation of pi; clippy cannot tell the difference and the artefact is
// the whole point of the test, so the literal stays and the lint is silenced.
#[allow(clippy::approx_constant)]
#[test]
fn a_32_bit_float_is_not_widened_into_precision_it_never_had() {
    // `3.14f32 as f64` is 3.140000104904175. Those thirteen digits are an
    // artefact of the widening, not anything the server stored, and on screen
    // they are indistinguishable from a value that really is that precise.
    assert_eq!(
        rendered(&ColumnData::F32(Some(3.14))),
        Rendered::Number {
            text: "3.14".to_string()
        }
    );
    assert_eq!(
        rendered(&ColumnData::F64(Some(3.14))),
        Rendered::Number {
            text: "3.14".to_string()
        }
    );
}

#[test]
fn a_non_finite_float_still_crosses_as_something_json_can_carry() {
    for data in [
        ColumnData::F32(Some(f32::NAN)),
        ColumnData::F64(Some(f64::NAN)),
    ] {
        assert_eq!(
            rendered(&data),
            Rendered::Number {
                text: "NaN".to_string()
            },
            "{data:?}"
        );
    }
    assert_eq!(
        rendered(&ColumnData::F32(Some(f32::NEG_INFINITY))),
        Rendered::Number {
            text: "-Infinity".to_string()
        }
    );
}

#[test]
fn a_decimal_keeps_the_scale_the_ledger_wrote() {
    for (value, scale, text) in [
        (0i128, 0u8, "0"),
        (123, 0, "123"),
        (100, 2, "1.00"),
        (5, 3, "0.005"),
        (-5, 3, "-0.005"),
        (-12345, 2, "-123.45"),
        (10, 1, "1.0"),
        (0, 4, "0.0000"),
    ] {
        assert_eq!(numeric_text(value, scale), text, "{value} scale {scale}");
    }
}

#[test]
fn a_decimal_reaches_the_wire_as_its_own_text_not_as_a_float() {
    let data = ColumnData::Numeric(Some(Numeric::new_with_scale(100, 2)));
    assert_eq!(
        rendered(&data),
        Rendered::Number {
            text: "1.00".to_string()
        }
    );
}

#[test]
fn a_guid_crosses_as_its_canonical_text() {
    let id = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
    let data = ColumnData::Guid(Some(id));
    assert_eq!(
        rendered(&data),
        Rendered::Text {
            text: id.to_string(),
            truncated: false
        }
    );
}

#[test]
fn a_text_value_is_copied_under_a_bound_but_charged_at_its_true_size() {
    let long = "x".repeat(TEXT_COPY_BYTES * 3);
    let data = ColumnData::String(Some(Cow::Owned(long.clone())));
    let (cell, source) = decode_cell(&data);
    assert_eq!(
        source,
        long.len() as u64,
        "charged at what the server holds"
    );
    match cell {
        Cell::Text(copied) => assert!(
            copied.len() <= TEXT_COPY_BYTES,
            "copied {} bytes",
            copied.len()
        ),
        other => panic!("expected text, got {other:?}"),
    }
    // And the wire cell still says it was cut.
    match rendered(&data) {
        Rendered::Text { truncated, .. } => assert!(truncated),
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn an_empty_string_is_not_a_null() {
    assert_eq!(
        rendered(&ColumnData::String(Some(Cow::Borrowed("")))),
        Rendered::Text {
            text: String::new(),
            truncated: false
        }
    );
}

#[test]
fn xml_is_read_as_text_under_the_same_bound() {
    let body = "<a/>".to_string();
    let data = ColumnData::Xml(Some(Cow::Owned(XmlData::new(body.clone()))));
    let (cell, source) = decode_cell(&data);
    assert_eq!(source, body.len() as u64);
    assert_eq!(cell, Cell::Text(body));
}

#[test]
fn a_binary_value_under_the_bound_is_read_whole() {
    let bytes = vec![0u8, 1, 2, 255];
    let data = ColumnData::Binary(Some(Cow::Owned(bytes.clone())));
    let (cell, source) = decode_cell(&data);
    assert_eq!(source, 4);
    assert_eq!(cell, Cell::Bytes(bytes));
}

#[test]
fn an_oversize_binary_value_abstains_and_names_the_size_it_declined() {
    // The wire shape reports `byteLength` — the size of the *whole* value — so
    // a clipped copy would report the prefix's length as the value's.
    let big = vec![7u8; (MAX_BLOB_COPY_BYTES + 1) as usize];
    let data = ColumnData::Binary(Some(Cow::Owned(big)));
    let (cell, source) = decode_cell(&data);
    assert_eq!(source, MAX_BLOB_COPY_BYTES + 1, "still charged in full");
    match cell {
        Cell::Error { reason } => {
            assert!(
                reason.contains(&(MAX_BLOB_COPY_BYTES + 1).to_string()),
                "{reason}"
            );
        }
        other => panic!("expected an abstention, got {other:?}"),
    }
    // And it renders as unavailable, never as an empty or truncated blob.
    assert!(matches!(rendered(&data), Rendered::Unavailable { .. }));
}

#[test]
fn a_date_or_time_value_names_the_type_it_could_not_decode() {
    // This build enables neither of tiberius' `chrono`/`time` features, so a
    // temporal column arrives as raw TDS day and increment counts. Turning
    // those into a calendar date here would be arithmetic nobody in this
    // repository could check against a server, and a wrong date is
    // indistinguishable from a right one. So the cell says which type it is
    // rather than showing a number that looks like data.
    for (data, name) in [
        (ColumnData::DateTime(Some(DateTime::new(1, 0))), "datetime"),
        (
            ColumnData::SmallDateTime(Some(SmallDateTime::new(1, 0))),
            "smalldatetime",
        ),
        (ColumnData::Date(Some(Date::new(1))), "date"),
        (ColumnData::Time(Some(Time::new(1, 7))), "time"),
        (
            ColumnData::DateTime2(Some(DateTime2::new(Date::new(1), Time::new(1, 7)))),
            "datetime2",
        ),
        (
            ColumnData::DateTimeOffset(Some(DateTimeOffset::new(
                DateTime2::new(Date::new(1), Time::new(1, 7)),
                60,
            ))),
            "datetimeoffset",
        ),
    ] {
        match decode_cell(&data).0 {
            Cell::Unsupported { type_name } => assert_eq!(type_name, name),
            other => panic!("{data:?} should abstain, got {other:?}"),
        }
        // Never a blank cell, which reads as NULL.
        assert!(matches!(rendered(&data), Rendered::Unsupported { .. }));
    }
}

// ---------------------------------------------------------------------------
// Failures carry no connection string
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_string_that_cannot_be_parsed_fails_at_connect_without_echoing_itself() {
    // Reaches the real `connect` and fails inside `Config::from_ado_string`, so
    // no socket is opened and no server is needed. What it pins is that this
    // driver's own `.map_err` goes through `DriverError::connect`, which
    // redacts.
    //
    // **Weaker than the SQLite equivalent, and knowingly so.** That test found
    // a message shape sqlx genuinely echoes a secret in, and fails without
    // `redact`. Nothing in tiberius or `connection-string` was found to echo
    // more than a single offending character, so this is redaction proved *by
    // construction* rather than by observation. Whoever first runs
    // `tests/sql_mssql.rs` against a real server should look for a server-side
    // message that carries the DSN and pin it there.
    // A short deadline so that a machine which somehow *does* resolve `db`
    // fails fast instead of sitting on the ten-second default; either way no
    // statement runs, which is what the stage assertion below says.
    let driver = MssqlDriver::with_connect_timeout(Duration::from_millis(200));
    let spec = ConnectSpec {
        dsn: "Server=db;Password=hunter2;;=".to_string(),
        writes_allowed: false,
    };
    let err = driver
        .connect(&spec)
        .await
        .err()
        .expect("an unparseable connection string");
    assert!(
        matches!(err.stage, ErrorStage::Connect | ErrorStage::ConnectTimeout),
        "nothing ran, so this is never an execute failure: {err:?}"
    );
    assert!(!err.message.contains("hunter2"), "{}", err.message);
}

// ---------------------------------------------------------------------------
// The keyword splitter these decisions rest on
// ---------------------------------------------------------------------------

#[test]
fn keyword_pairs_lowercase_the_key_and_keep_the_value() {
    let pairs = keyword_pairs("Server=Db;User Id=SA");
    assert_eq!(
        pairs,
        vec![
            ("server".to_string(), "Db".to_string()),
            ("user id".to_string(), "SA".to_string()),
        ]
    );
}

#[test]
fn a_quoted_value_may_contain_the_separator_and_is_unwrapped() {
    let pairs = keyword_pairs("Password='a;b';Server=db");
    assert_eq!(
        pairs,
        vec![
            ("password".to_string(), "a;b".to_string()),
            ("server".to_string(), "db".to_string()),
        ]
    );
    let pairs = keyword_pairs("Password=\"a;b\";Server=db");
    assert_eq!(pairs[0], ("password".to_string(), "a;b".to_string()));
}

#[test]
fn a_doubled_quote_inside_a_quoted_value_is_one_quote() {
    let pairs = keyword_pairs("Password='a''b';Server=db");
    assert_eq!(pairs[0], ("password".to_string(), "a'b".to_string()));
    assert_eq!(pairs[1], ("server".to_string(), "db".to_string()));
}

#[test]
fn an_unterminated_quote_swallows_the_rest_rather_than_splitting_a_secret() {
    // The same bias as `dsn::redact`: where the end of a value cannot be
    // proved, treat the rest as part of it. Splitting it would invent keys out
    // of password text.
    let pairs = keyword_pairs("Password='a;b;Server=db");
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].0, "password");
}
