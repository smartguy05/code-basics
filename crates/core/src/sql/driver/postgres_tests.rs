use super::*;
use crate::sql::driver::{ConnectSpec, ReadOnlyEnforcement, SqlDriver};
use crate::sql::format::Cell;

fn spec(dsn: &str, writes_allowed: bool) -> ConnectSpec {
    ConnectSpec {
        dsn: dsn.to_string(),
        writes_allowed,
    }
}

/// Decode a binary value and hand back the cell alone.
fn bin(type_name: &str, bytes: &[u8]) -> Cell {
    decode_value(type_name, WireFormat::Binary, bytes).0
}

/// Decode a text-format value and hand back the cell alone.
fn txt(type_name: &str, text: &str) -> Cell {
    decode_value(type_name, WireFormat::Text, text.as_bytes()).0
}

fn text_of(cell: Cell) -> String {
    match cell {
        Cell::Text(t) => t,
        other => panic!("expected text, got {other:?}"),
    }
}

fn numeric_of(cell: Cell) -> String {
    match cell {
        Cell::Numeric(t) => t,
        other => panic!("expected a numeric, got {other:?}"),
    }
}

fn is_error(cell: &Cell) -> bool {
    matches!(cell, Cell::Error { .. })
}

fn unsupported_name(cell: &Cell) -> &str {
    match cell {
        Cell::Unsupported { type_name } => type_name,
        other => panic!("expected unsupported, got {other:?}"),
    }
}

/// Build a Postgres binary `numeric` body.
fn numeric_bytes(weight: i16, sign: u16, dscale: u16, digits: &[i16]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(digits.len() as i16).to_be_bytes());
    out.extend_from_slice(&weight.to_be_bytes());
    out.extend_from_slice(&sign.to_be_bytes());
    out.extend_from_slice(&dscale.to_be_bytes());
    for d in digits {
        out.extend_from_slice(&d.to_be_bytes());
    }
    out
}

/// Build a Postgres binary `interval` body: microseconds, days, months.
fn interval_bytes(micros: i64, days: i32, months: i32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&micros.to_be_bytes());
    out.extend_from_slice(&days.to_be_bytes());
    out.extend_from_slice(&months.to_be_bytes());
    out
}

// ---------------------------------------------------------------------------
// 1. Read-only wording — the reason this is data on the driver
// ---------------------------------------------------------------------------

#[test]
fn a_read_only_postgres_connection_says_the_server_is_holding_the_session() {
    let d = PostgresDriver::new();
    let e = d.read_only_enforcement(false);
    assert_eq!(e, ReadOnlyEnforcement::ServerSession);
    assert_eq!(e.label(), "The server is holding this session read-only");
    // The server refuses the write; it does not run and then get undone. That
    // is the one distinction the SQL Server phase must not be allowed to blur.
    assert!(e.refuses_writes_before_they_run());
    // And it is *not* the SQLite mechanism: the handle itself is an ordinary
    // read-write connection that the server is holding read-only.
    assert!(!e.is_driver_enforced());
}

#[test]
fn allowing_writes_drops_the_claim_entirely_rather_than_weakening_it() {
    let d = PostgresDriver::new();
    assert_eq!(
        d.read_only_enforcement(true),
        ReadOnlyEnforcement::NotEnforced
    );
}

#[test]
fn the_driver_reports_the_postgres_engine() {
    assert_eq!(PostgresDriver::new().engine(), SqlEngine::Postgres);
}

#[test]
fn a_driver_carries_the_default_deadline_unless_asked_for_another() {
    assert_eq!(
        PostgresDriver::new().connect_timeout(),
        crate::sql::driver::DEFAULT_CONNECT_TIMEOUT
    );
    assert_eq!(
        PostgresDriver::default().connect_timeout(),
        PostgresDriver::new().connect_timeout()
    );
    let brief = PostgresDriver::with_connect_timeout(std::time::Duration::from_millis(250));
    assert_eq!(
        brief.connect_timeout(),
        std::time::Duration::from_millis(250)
    );
}

/// The session setting is asked for *and then read back*.
///
/// `ReadOnlyEnforcement::ServerSession` may only be reported once the setting
/// was accepted, so the driver does not merely send the `SET` — it asks the
/// server what the value actually is. These constants are the pair that makes
/// that possible, and they must name the same setting.
#[test]
fn the_read_only_session_setting_is_both_applied_and_read_back() {
    assert_eq!(
        SET_SESSION_READ_ONLY,
        "SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY"
    );
    assert_eq!(SHOW_SESSION_READ_ONLY, "SHOW default_transaction_read_only");
    assert_eq!(READ_ONLY_CONFIRMATION, "on");
}

#[test]
fn a_session_that_does_not_confirm_read_only_is_a_connect_failure_naming_no_input() {
    let err = read_only_not_confirmed();
    assert_eq!(err.stage, crate::sql::driver::ErrorStage::Connect);
    assert!(
        err.message.contains("read-only"),
        "must say what was not achieved: {}",
        err.message
    );
    // A refusal to open, not a downgrade: the user asked for writes to be
    // impossible and must not silently get a connection where they are not.
    assert!(
        err.message.contains("not opened"),
        "must say the connection was refused: {}",
        err.message
    );
}

// ---------------------------------------------------------------------------
// 2. Connection targets — URL, Npgsql keywords, and what is refused
// ---------------------------------------------------------------------------

#[test]
fn a_postgres_url_is_handed_to_sqlx_whole_so_its_own_parameters_survive() {
    assert_eq!(
        connect_target("postgres://u:p@host:5432/db?sslmode=require"),
        Ok(PgTarget::Url(
            "postgres://u:p@host:5432/db?sslmode=require".to_string()
        ))
    );
    assert_eq!(
        connect_target("POSTGRESQL://host/db"),
        Ok(PgTarget::Url("POSTGRESQL://host/db".to_string())),
        "the scheme is case-insensitive"
    );
}

#[test]
fn npgsql_keywords_resolve_to_the_fields_sqlx_needs() {
    let target = connect_target(
        "Host=db.internal;Port=6432;Database=app;Username=svc;Password=hunter2;Application Name=cb",
    )
    .expect("an Npgsql keyword string names a host");
    let PgTarget::Keywords(k) = target else {
        panic!("expected keywords")
    };
    assert_eq!(k.host, "db.internal");
    assert_eq!(k.port, Some(6432));
    assert_eq!(k.database.as_deref(), Some("app"));
    assert_eq!(k.username.as_deref(), Some("svc"));
    assert_eq!(k.password.as_deref(), Some("hunter2"));
    assert_eq!(k.application_name.as_deref(), Some("cb"));
}

#[test]
fn the_user_keyword_is_accepted_in_every_spelling_npgsql_accepts() {
    for key in ["Username", "User ID", "UserId", "User Name", "User"] {
        let target = connect_target(&format!("Host=h;{key}=svc")).expect("names a host");
        let PgTarget::Keywords(k) = target else {
            panic!("expected keywords")
        };
        assert_eq!(k.username.as_deref(), Some("svc"), "spelling {key}");
    }
}

#[test]
fn a_connection_string_naming_no_host_is_refused_rather_than_guessed_at() {
    // `Server=` is spoken by several drivers and is deliberately not read as a
    // Postgres host: `dsn::sniff_engine` does not treat it as one either, and
    // connecting to a plausible-looking value is the wrong server, not a near
    // miss.
    assert_eq!(
        connect_target("Server=localhost;Database=app"),
        Err(PgTargetError::NoHostNamed)
    );
    assert_eq!(
        connect_target("Host=;Database=app"),
        Err(PgTargetError::NoHostNamed),
        "an empty value names nothing"
    );
    assert_eq!(connect_target("   "), Err(PgTargetError::Empty));
    assert_eq!(
        connect_target("localhost"),
        Err(PgTargetError::NoHostNamed),
        "a bare word is not a connection string"
    );
}

#[test]
fn every_target_error_is_its_own_sentence() {
    let all = [
        PgTargetError::Empty,
        PgTargetError::NoHostNamed,
        PgTargetError::BadPort,
        PgTargetError::UnknownSslMode,
    ];
    for (i, a) in all.iter().enumerate() {
        assert!(!a.message().is_empty());
        for b in &all[i + 1..] {
            assert_ne!(
                a.message(),
                b.message(),
                "{a:?} and {b:?} say the same thing"
            );
        }
    }
}

#[test]
fn a_port_that_is_not_a_port_is_refused_rather_than_defaulted() {
    assert_eq!(
        connect_target("Host=h;Port=abc"),
        Err(PgTargetError::BadPort)
    );
    assert_eq!(connect_target("Host=h;Port=0"), Err(PgTargetError::BadPort));
    assert_eq!(
        connect_target("Host=h;Port=99999"),
        Err(PgTargetError::BadPort)
    );
}

#[test]
fn an_unrecognised_ssl_mode_is_refused_rather_than_quietly_downgraded() {
    // Defaulting here would turn `Ssl Mode=VerifyFull` with a typo into an
    // unverified connection, which is the one wrong answer that is worse than
    // failing to connect at all.
    assert_eq!(
        connect_target("Host=h;Ssl Mode=verify-fullish"),
        Err(PgTargetError::UnknownSslMode)
    );
    for (spelling, expected) in [
        ("disable", SslMode::Disable),
        ("allow", SslMode::Allow),
        ("prefer", SslMode::Prefer),
        ("require", SslMode::Require),
        ("verify-ca", SslMode::VerifyCa),
        ("VerifyFull", SslMode::VerifyFull),
    ] {
        let target = connect_target(&format!("Host=h;SslMode={spelling}")).expect("names a host");
        let PgTarget::Keywords(k) = target else {
            panic!("expected keywords")
        };
        assert_eq!(k.ssl_mode, Some(expected), "spelling {spelling}");
    }
}

/// A target's `Debug` must not print the password it is carrying.
///
/// `PgTarget::Url` holds a whole URL and `PgKeywords` holds the password
/// itself, and a derived `Debug` puts both into any `{:?}`, any failed
/// `assert_eq!` and any `tracing` line — which is exactly the leak the
/// subsystem docs forbid.
#[test]
fn debugging_a_target_never_prints_the_password() {
    let url = connect_target("postgres://u:hunter2@host/db").unwrap();
    let rendered = format!("{url:?}");
    assert!(!rendered.contains("hunter2"), "{rendered}");

    let kw = connect_target("Host=h;Password=hunter2").unwrap();
    let rendered = format!("{kw:?}");
    assert!(!rendered.contains("hunter2"), "{rendered}");
    assert!(
        rendered.contains(crate::sql::dsn::REDACTED),
        "a redacted field should still be visible as redacted: {rendered}"
    );
}

#[test]
fn building_options_from_a_secret_bearing_string_never_echoes_it_on_failure() {
    let err = PostgresDriver::options(&spec("Server=db;Password=hunter2", false)).unwrap_err();
    assert!(!err.message.contains("hunter2"), "{}", err.message);
    assert!(!err.message.contains("Server=db"), "{}", err.message);
    assert_eq!(err.stage, crate::sql::driver::ErrorStage::Connect);
}

#[test]
fn options_resolve_for_every_accepted_connection_string_shape() {
    for dsn in [
        "postgres://u:p@host:5432/db",
        "postgresql://host/db",
        "Host=localhost",
        "Host=localhost;Port=5432;Database=app;Username=u;Password=p",
    ] {
        assert!(
            PostgresDriver::options(&spec(dsn, false)).is_ok(),
            "should resolve: {dsn}"
        );
    }
}

#[test]
fn keyword_fields_reach_the_options_sqlx_will_connect_with() {
    let o = PostgresDriver::options(&spec(
        "Host=db.internal;Port=6432;Database=app;Username=svc",
        false,
    ))
    .unwrap();
    assert_eq!(o.get_host(), "db.internal");
    assert_eq!(o.get_port(), 6432);
    assert_eq!(o.get_database(), Some("app"));
    assert_eq!(o.get_username(), "svc");
}

/// The startup options carry the read-only default when writes are disallowed,
/// and carry nothing of the sort when they are allowed.
#[test]
fn the_read_only_startup_option_is_set_only_when_writes_are_disallowed() {
    let ro = PostgresDriver::options(&spec("Host=h", false)).unwrap();
    assert!(
        ro.get_options()
            .unwrap_or_default()
            .contains("default_transaction_read_only=on"),
        "{:?}",
        ro.get_options()
    );
    let rw = PostgresDriver::options(&spec("Host=h", true)).unwrap();
    assert!(
        !rw.get_options()
            .unwrap_or_default()
            .contains("default_transaction_read_only"),
        "{:?}",
        rw.get_options()
    );
}

// ---------------------------------------------------------------------------
// 3. Column type names
// ---------------------------------------------------------------------------

#[test]
fn sqlxs_spelling_of_an_unresolved_type_oid_crosses_as_not_reported() {
    // sqlx prints `?` for a type whose OID it never resolved. Passing that
    // through as `Some("?")` claims the column's type *is* named `?`, which is
    // a different and untrue statement; the honest answer is *not reported*.
    assert_eq!(column_type_name("?"), None);
    assert_eq!(column_type_name("   "), None);
}

#[test]
fn a_real_type_name_is_passed_through_as_the_server_spelled_it() {
    assert_eq!(column_type_name("INT4"), Some("INT4".to_string()));
    assert_eq!(
        column_type_name("TIMESTAMPTZ"),
        Some("TIMESTAMPTZ".to_string())
    );
    assert_eq!(
        column_type_name("mood"),
        Some("mood".to_string()),
        "a user-defined type this build cannot decode is still named"
    );
}

// ---------------------------------------------------------------------------
// 4. The type map: what this build decodes, and what it admits it cannot
// ---------------------------------------------------------------------------

#[test]
fn every_type_this_build_decodes_is_named_exactly_once() {
    for (name, expected) in [
        ("BOOL", PgDecoder::Bool),
        ("INT2", PgDecoder::Int2),
        ("INT4", PgDecoder::Int4),
        ("INT8", PgDecoder::Int8),
        ("OID", PgDecoder::Oid),
        ("FLOAT4", PgDecoder::Float4),
        ("FLOAT8", PgDecoder::Float8),
        ("NUMERIC", PgDecoder::Numeric),
        ("TEXT", PgDecoder::Text),
        ("VARCHAR", PgDecoder::Text),
        ("CHAR", PgDecoder::Text),
        ("\"CHAR\"", PgDecoder::Text),
        ("NAME", PgDecoder::Text),
        ("UNKNOWN", PgDecoder::Text),
        ("BYTEA", PgDecoder::Bytes),
        ("UUID", PgDecoder::Uuid),
        ("JSON", PgDecoder::Json),
        ("JSONB", PgDecoder::Jsonb),
        ("DATE", PgDecoder::Date),
        ("TIME", PgDecoder::Time),
        ("TIMESTAMP", PgDecoder::Timestamp),
        ("TIMESTAMPTZ", PgDecoder::TimestampTz),
        ("INTERVAL", PgDecoder::Interval),
    ] {
        assert_eq!(decoder_for(name), Some(expected), "type {name}");
        assert_eq!(
            decoder_for(&name.to_ascii_lowercase()),
            Some(expected),
            "type {name} lowercased"
        );
    }
}

#[test]
fn a_type_this_build_cannot_decode_says_so_and_names_it() {
    // Arrays, ranges, geometric and network types, `money` (whose rendering
    // depends on the server's locale), `timetz` (whose binary zone field has a
    // sign convention this build cannot verify against a server) and every
    // user-defined type.
    for name in [
        "INT4[]",
        "TEXT[]",
        "NUMERIC[]",
        "MONEY",
        "TIMETZ",
        "INT4RANGE",
        "POINT",
        "INET",
        "BIT",
        "VARBIT",
        "XML",
        "TSVECTOR",
        "RECORD",
        "VOID",
        "mood",
        "hstore",
        "?",
    ] {
        assert_eq!(decoder_for(name), None, "type {name} must abstain");
        let cell = bin(name, &[0x01, 0x02]);
        assert_eq!(
            unsupported_name(&cell),
            name,
            "an undecodable value names its type rather than rendering blank"
        );
        // And it is never mistaken for a NULL, an empty string, or a failure.
        assert!(!matches!(cell, Cell::Null | Cell::Text(_)));
        assert!(!is_error(&cell));
        // The same answer in either wire format: the decoder table is chosen by
        // the type, not by how the value happened to arrive.
        assert_eq!(unsupported_name(&txt(name, "anything")), name);
    }
}

/// An undecodable value can neither panic nor come back empty, whatever bytes
/// arrive with it.
#[test]
fn an_unsupported_type_survives_any_payload_without_panicking_or_blanking() {
    for bytes in [
        vec![],
        vec![0x00],
        vec![0xff; 64],
        (0..=255u8).collect::<Vec<u8>>(),
    ] {
        let (cell, source) = decode_value("mood", WireFormat::Binary, &bytes);
        assert_eq!(unsupported_name(&cell), "mood");
        assert_eq!(source, bytes.len() as u64);
        assert!(matches!(
            crate::sql::format::format_cell(cell),
            crate::sql::format::SqlValue::Unsupported { .. }
        ));
    }
}

// ---------------------------------------------------------------------------
// 5. Binary decoding
// ---------------------------------------------------------------------------

#[test]
fn booleans_decode_from_the_one_byte_postgres_sends() {
    assert_eq!(bin("BOOL", &[1]), Cell::Bool(true));
    assert_eq!(bin("BOOL", &[0]), Cell::Bool(false));
    // Not `false`: an unexpected encoding is an admission, not a value.
    assert!(is_error(&bin("BOOL", &[2])));
    assert!(is_error(&bin("BOOL", &[])));
    assert!(is_error(&bin("BOOL", &[0, 0])));
}

#[test]
fn integers_decode_big_endian_at_every_width() {
    assert_eq!(bin("INT2", &42i16.to_be_bytes()), Cell::Int(42));
    assert_eq!(bin("INT2", &(-1i16).to_be_bytes()), Cell::Int(-1));
    assert_eq!(bin("INT4", &42i32.to_be_bytes()), Cell::Int(42));
    assert_eq!(
        bin("INT4", &i32::MIN.to_be_bytes()),
        Cell::Int(i32::MIN as i64)
    );
    assert_eq!(bin("INT8", &i64::MAX.to_be_bytes()), Cell::Int(i64::MAX));
    // `oid` is unsigned: reading it as a signed i32 turns 4294967295 into -1.
    assert_eq!(
        bin("OID", &u32::MAX.to_be_bytes()),
        Cell::Int(u32::MAX as i64)
    );
    for name in ["INT2", "INT4", "INT8", "OID"] {
        assert!(is_error(&bin(name, &[0x01])), "{name} at the wrong width");
    }
}

#[test]
fn a_float4_is_rendered_at_its_own_precision_and_not_widened_into_noise() {
    // 0.1f32 widened to f64 is 0.10000000149011612, and printing *that* is a
    // number the database does not hold. The shortest round-trip rendering of
    // the f32 is what Postgres itself prints.
    assert_eq!(numeric_of(bin("FLOAT4", &0.1f32.to_be_bytes())), "0.1");
    assert_eq!(numeric_of(bin("FLOAT4", &(-2.5f32).to_be_bytes())), "-2.5");
    assert!(is_error(&bin("FLOAT4", &[0, 0])));
}

#[test]
fn the_three_values_json_cannot_carry_are_spelled_the_same_way_everywhere() {
    // `format::format_cell` spells them `NaN` / `Infinity` / `-Infinity`, and a
    // float4 taking the other path must not spell them `inf`, which is what
    // Rust's own `to_string` produces.
    assert_eq!(numeric_of(bin("FLOAT4", &f32::NAN.to_be_bytes())), "NaN");
    assert_eq!(
        numeric_of(bin("FLOAT4", &f32::INFINITY.to_be_bytes())),
        "Infinity"
    );
    assert_eq!(
        numeric_of(bin("FLOAT4", &f32::NEG_INFINITY.to_be_bytes())),
        "-Infinity"
    );
    assert_eq!(
        crate::sql::format::format_cell(bin("FLOAT8", &f64::INFINITY.to_be_bytes())),
        crate::sql::format::SqlValue::Number {
            text: "Infinity".to_string()
        }
    );
}

#[test]
fn a_float8_stays_a_float() {
    assert_eq!(bin("FLOAT8", &1.5f64.to_be_bytes()), Cell::Float(1.5));
    assert!(is_error(&bin("FLOAT8", &[0, 0, 0, 0])));
}

#[test]
fn numeric_crosses_as_a_string_with_its_scale_intact() {
    // `0.10` is not `0.1` to a ledger, and neither is a value a double could
    // not hold. The scale is part of what the database stores.
    assert_eq!(
        numeric_of(bin("NUMERIC", &numeric_bytes(-1, 0x0000, 2, &[1000]))),
        "0.10"
    );
    assert_eq!(
        numeric_of(bin("NUMERIC", &numeric_bytes(0, 0x0000, 1, &[1, 5000]))),
        "1.5"
    );
    assert_eq!(
        numeric_of(bin("NUMERIC", &numeric_bytes(0, 0x0000, 0, &[]))),
        "0"
    );
    assert_eq!(
        numeric_of(bin("NUMERIC", &numeric_bytes(0, 0x0000, 2, &[]))),
        "0.00"
    );
    assert_eq!(
        numeric_of(bin(
            "NUMERIC",
            &numeric_bytes(1, 0x4000, 3, &[1, 2345, 6780])
        )),
        "-12345.678"
    );
    assert_eq!(
        numeric_of(bin("NUMERIC", &numeric_bytes(1, 0x0000, 0, &[1]))),
        "10000",
        "a missing trailing digit group is four zeroes, not nothing"
    );
    assert_eq!(
        numeric_of(bin("NUMERIC", &numeric_bytes(-2, 0x0000, 8, &[1234]))),
        "0.00001234",
        "a weight below -1 is leading zeroes in the fraction"
    );
}

#[test]
fn the_numeric_special_values_keep_the_spellings_the_rest_of_the_console_uses() {
    assert_eq!(
        numeric_of(bin("NUMERIC", &numeric_bytes(0, 0xC000, 0, &[]))),
        "NaN"
    );
    assert_eq!(
        numeric_of(bin("NUMERIC", &numeric_bytes(0, 0xD000, 0, &[]))),
        "Infinity"
    );
    assert_eq!(
        numeric_of(bin("NUMERIC", &numeric_bytes(0, 0xF000, 0, &[]))),
        "-Infinity"
    );
}

#[test]
fn a_malformed_numeric_is_an_admission_rather_than_a_wrong_number() {
    assert!(is_error(&bin("NUMERIC", &[])));
    assert!(
        is_error(&bin("NUMERIC", &[0, 1, 0, 0, 0, 0, 0, 0])),
        "the header claims one digit group and none follows"
    );
    let mut sign_unknown = numeric_bytes(0, 0x1234, 0, &[]);
    assert!(is_error(&bin("NUMERIC", &sign_unknown)));
    sign_unknown.truncate(3);
    assert!(is_error(&bin("NUMERIC", &sign_unknown)));
    let mut negative_ndigits = numeric_bytes(0, 0, 0, &[]);
    negative_ndigits[0] = 0xff;
    negative_ndigits[1] = 0xff;
    assert!(is_error(&bin("NUMERIC", &negative_ndigits)));
}

#[test]
fn text_arrives_the_same_way_in_both_wire_formats() {
    assert_eq!(text_of(bin("TEXT", "hello".as_bytes())), "hello");
    assert_eq!(text_of(txt("VARCHAR", "hello")), "hello");
    assert_eq!(
        text_of(bin("TEXT", "".as_bytes())),
        "",
        "the empty string is a value, not a NULL"
    );
    // Invalid UTF-8 is admitted rather than lossily replaced: a replacement
    // character where a byte was is a value the database does not hold.
    assert!(is_error(&bin("TEXT", &[0xff, 0xfe])));
}

#[test]
fn bytea_decodes_from_raw_bytes_and_from_postgres_hex_text() {
    assert_eq!(bin("BYTEA", &[0xde, 0xad]), Cell::Bytes(vec![0xde, 0xad]));
    assert_eq!(txt("BYTEA", "\\xdead"), Cell::Bytes(vec![0xde, 0xad]));
    assert_eq!(txt("BYTEA", "\\x"), Cell::Bytes(vec![]));
    // The pre-9.0 escape format is a different encoding and is not guessed at.
    assert!(is_error(&txt("BYTEA", "abc")));
    assert!(is_error(&txt("BYTEA", "\\xzz")));
    assert!(is_error(&txt("BYTEA", "\\xabc")));
}

#[test]
fn a_uuid_is_rendered_in_the_canonical_hyphenated_form() {
    let bytes: Vec<u8> = (0..16u8).collect();
    assert_eq!(
        text_of(bin("UUID", &bytes)),
        "00010203-0405-0607-0809-0a0b0c0d0e0f"
    );
    assert!(is_error(&bin("UUID", &bytes[..15])));
    assert_eq!(
        text_of(txt("UUID", "00010203-0405-0607-0809-0a0b0c0d0e0f")),
        "00010203-0405-0607-0809-0a0b0c0d0e0f"
    );
}

#[test]
fn json_and_jsonb_differ_by_exactly_one_version_byte() {
    assert_eq!(text_of(bin("JSON", br#"{"a":1}"#)), r#"{"a":1}"#);
    let mut jsonb = vec![1u8];
    jsonb.extend_from_slice(br#"{"a":1}"#);
    assert_eq!(text_of(bin("JSONB", &jsonb)), r#"{"a":1}"#);
    // A jsonb body whose version byte is not 1 is a format this build has not
    // seen; decoding it as if it were version 1 would drop or shift a byte.
    let mut future = vec![2u8];
    future.extend_from_slice(br#"{"a":1}"#);
    assert!(is_error(&bin("JSONB", &future)));
    assert!(is_error(&bin("JSONB", &[])));
    // In text format they are both just the JSON text.
    assert_eq!(text_of(txt("JSONB", r#"{"a":1}"#)), r#"{"a":1}"#);
}

#[test]
fn dates_count_days_from_the_postgres_epoch_in_both_directions() {
    assert_eq!(text_of(bin("DATE", &0i32.to_be_bytes())), "2000-01-01");
    assert_eq!(text_of(bin("DATE", &1i32.to_be_bytes())), "2000-01-02");
    assert_eq!(text_of(bin("DATE", &(-1i32).to_be_bytes())), "1999-12-31");
    assert_eq!(text_of(bin("DATE", &8766i32.to_be_bytes())), "2024-01-01");
    assert_eq!(text_of(bin("DATE", &59i32.to_be_bytes())), "2000-02-29");
    // Postgres' own infinities, which are not dates and must not be rendered as
    // some enormous year.
    assert_eq!(text_of(bin("DATE", &i32::MAX.to_be_bytes())), "infinity");
    assert_eq!(text_of(bin("DATE", &i32::MIN.to_be_bytes())), "-infinity");
    assert!(is_error(&bin("DATE", &[0, 0])));
}

#[test]
fn a_year_before_the_common_era_is_labelled_bc_rather_than_signed() {
    // Postgres has no year zero: the proleptic year 0 is 1 BC.
    assert_eq!(
        text_of(bin("DATE", &(-730_485i32).to_be_bytes())),
        "0001-01-01 BC"
    );
}

#[test]
fn times_render_their_fraction_only_when_there_is_one() {
    assert_eq!(text_of(bin("TIME", &0i64.to_be_bytes())), "00:00:00");
    assert_eq!(
        text_of(bin("TIME", &45_296_000_000i64.to_be_bytes())),
        "12:34:56"
    );
    assert_eq!(
        text_of(bin("TIME", &45_296_500_000i64.to_be_bytes())),
        "12:34:56.5",
        "trailing zeroes of the fraction are not printed"
    );
    assert_eq!(
        text_of(bin("TIME", &45_296_000_001i64.to_be_bytes())),
        "12:34:56.000001"
    );
    assert!(is_error(&bin("TIME", &[0, 0, 0, 0])));
}

#[test]
fn timestamps_carry_the_date_and_the_time_and_say_when_they_are_in_utc() {
    assert_eq!(
        text_of(bin("TIMESTAMP", &0i64.to_be_bytes())),
        "2000-01-01 00:00:00"
    );
    assert_eq!(
        text_of(bin("TIMESTAMP", &(-1i64).to_be_bytes())),
        "1999-12-31 23:59:59.999999",
        "a microsecond before the epoch is the previous day, not a negative time"
    );
    // The value is UTC and the rendering says so, rather than being silently
    // shown in a zone nobody named.
    assert_eq!(
        text_of(bin("TIMESTAMPTZ", &0i64.to_be_bytes())),
        "2000-01-01 00:00:00+00"
    );
    assert_eq!(
        text_of(bin("TIMESTAMPTZ", &757_427_696_000_000i64.to_be_bytes())),
        "2024-01-01 12:34:56+00"
    );
    for name in ["TIMESTAMP", "TIMESTAMPTZ"] {
        assert_eq!(text_of(bin(name, &i64::MAX.to_be_bytes())), "infinity");
        assert_eq!(text_of(bin(name, &i64::MIN.to_be_bytes())), "-infinity");
        assert!(is_error(&bin(name, &[0, 0])));
    }
}

#[test]
fn an_interval_keeps_its_three_fields_apart_because_they_are_not_convertible() {
    // A month is not 30 days and a day is not 24 hours, so the three fields are
    // rendered as three fields rather than summed into one duration.
    assert_eq!(
        text_of(bin("INTERVAL", &interval_bytes(14_706_000_000, 3, 14))),
        "1 year 2 mons 3 days 04:05:06"
    );
    assert_eq!(
        text_of(bin("INTERVAL", &interval_bytes(0, 0, 0))),
        "00:00:00"
    );
    assert_eq!(
        text_of(bin("INTERVAL", &interval_bytes(0, 1, 1))),
        "1 mon 1 day"
    );
    assert_eq!(
        text_of(bin("INTERVAL", &interval_bytes(-3_600_000_000, 0, 0))),
        "-01:00:00"
    );
    assert_eq!(
        text_of(bin("INTERVAL", &interval_bytes(500_000, 0, 0))),
        "00:00:00.5"
    );
    assert!(is_error(&bin("INTERVAL", &[0; 8])));
}

// ---------------------------------------------------------------------------
// 6. Text-format decoding — the simple query protocol's answers
// ---------------------------------------------------------------------------

#[test]
fn text_format_scalars_parse_rather_than_pass_through_unchecked() {
    assert_eq!(txt("BOOL", "t"), Cell::Bool(true));
    assert_eq!(txt("BOOL", "f"), Cell::Bool(false));
    assert!(is_error(&txt("BOOL", "true")));
    assert_eq!(txt("INT4", "42"), Cell::Int(42));
    assert_eq!(txt("INT8", "-9223372036854775808"), Cell::Int(i64::MIN));
    assert!(is_error(&txt("INT4", "4 2")));
    assert!(is_error(&txt("INT4", "")));
}

#[test]
fn a_text_format_number_is_kept_as_the_server_spelled_it_once_it_has_been_checked() {
    // The server's own rendering is the most faithful one there is, so it is
    // kept — but it is validated first, so a byte sequence that is not a number
    // cannot arrive in the grid *as* a number.
    assert_eq!(numeric_of(txt("NUMERIC", "0.10")), "0.10");
    assert_eq!(numeric_of(txt("NUMERIC", "-1e10")), "-1e10");
    assert_eq!(numeric_of(txt("NUMERIC", "NaN")), "NaN");
    assert_eq!(numeric_of(txt("FLOAT8", "Infinity")), "Infinity");
    assert_eq!(numeric_of(txt("FLOAT4", "0.1")), "0.1");
    assert!(is_error(&txt("NUMERIC", "one")));
    assert!(is_error(&txt("FLOAT8", "")));
}

#[test]
fn text_format_dates_and_intervals_are_the_servers_own_rendering() {
    assert_eq!(text_of(txt("DATE", "2024-01-01")), "2024-01-01");
    assert_eq!(
        text_of(txt("TIMESTAMPTZ", "2024-01-01 12:34:56+00")),
        "2024-01-01 12:34:56+00"
    );
    assert_eq!(text_of(txt("INTERVAL", "1 year 2 mons")), "1 year 2 mons");
}

// ---------------------------------------------------------------------------
// 7. The two caps, charged at the value's true source size
// ---------------------------------------------------------------------------

#[test]
fn every_decode_reports_the_bytes_the_server_actually_sent() {
    for (name, bytes) in [
        ("INT4", vec![0, 0, 0, 1]),
        ("TEXT", b"hello".to_vec()),
        ("BYTEA", vec![0u8; 100]),
        ("mood", vec![0u8; 7]),
    ] {
        let (_, source) = decode_value(name, WireFormat::Binary, &bytes);
        assert_eq!(source, bytes.len() as u64, "type {name}");
    }
}

#[test]
fn an_overlong_text_value_is_clipped_for_memory_but_charged_at_its_full_size() {
    let huge = "x".repeat(crate::sql::driver::caps::TEXT_COPY_BYTES * 3);
    let (cell, source) = decode_value("TEXT", WireFormat::Binary, huge.as_bytes());
    assert_eq!(source, huge.len() as u64, "the budget sees the whole value");
    let kept = text_of(cell);
    assert!(
        kept.len() <= crate::sql::driver::caps::TEXT_COPY_BYTES,
        "kept {} bytes",
        kept.len()
    );
    assert!(
        kept.len() > crate::sql::format::MAX_TEXT_BYTES,
        "what arrives must still be longer than the display cap, or a clipped \
         value would be indistinguishable from one that ends there"
    );
}

#[test]
fn an_oversized_binary_value_abstains_and_names_the_size_it_declined_to_read() {
    let huge = vec![0u8; (crate::sql::driver::caps::MAX_BLOB_COPY_BYTES + 1) as usize];
    let (cell, source) = decode_value("BYTEA", WireFormat::Binary, &huge);
    assert_eq!(source, huge.len() as u64);
    let Cell::Error { reason } = cell else {
        panic!("an oversized blob must abstain rather than be shortened")
    };
    assert!(reason.contains(&huge.len().to_string()), "{reason}");
    assert!(reason.contains("were not read"), "{reason}");
}

// ---------------------------------------------------------------------------
// 8. The connect deadline
// ---------------------------------------------------------------------------

/// `connect` opens the handle **inside** the deadline, and nowhere else.
///
/// A source pin, for the same reason `sqlite.rs` has one: a real connect under
/// a zero deadline is not deterministic, but what *can* be pinned is that the
/// one call which opens a handle is the one wrapped by
/// `within_connect_deadline`. A future edit that lifts it out restores an
/// unbounded connect, which is invisible until a database stops answering.
#[test]
fn the_only_call_that_opens_a_handle_is_the_one_under_the_deadline() {
    let source = include_str!("postgres.rs");
    assert_eq!(
        source.match_indices("connect_with(").count(),
        1,
        "there is exactly one place a handle is opened"
    );
    assert!(
        source.contains("within_connect_deadline(deadline, async {"),
        "the open must sit inside the deadline; if this fails because the code \
         was reformatted, re-pin it rather than deleting it"
    );
}

/// The session is made read-only inside the same deadline as the connect.
///
/// A handle that has opened but whose `SET` has not answered is not a usable
/// connection, and nothing watches the stop signal until the row loop starts —
/// so an unbounded `SET` reopens exactly the window the deadline exists to
/// close.
#[test]
fn making_the_session_read_only_happens_inside_the_connect_deadline() {
    let source = include_str!("postgres.rs");
    let deadline_at = source
        .find("within_connect_deadline(deadline, async {")
        .expect("the deadline wrapper");
    let enforce_at = source
        .find("enforce_session_read_only(")
        .expect("the read-only step");
    assert!(
        enforce_at > deadline_at,
        "the read-only step must be called from inside the deadline block"
    );
}
