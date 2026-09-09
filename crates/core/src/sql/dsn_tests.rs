use super::*;

// ---------------------------------------------------------------------------
// sniff_engine
// ---------------------------------------------------------------------------

#[test]
fn sniffs_postgres_from_a_url_scheme() {
    assert_eq!(
        sniff_engine("postgres://app:s3cr3t@db.internal:5432/orders"),
        Some(SqlEngine::Postgres)
    );
    assert_eq!(
        sniff_engine("POSTGRESQL://db.internal/orders"),
        Some(SqlEngine::Postgres)
    );
}

#[test]
fn sniffs_postgres_from_a_host_keyword() {
    assert_eq!(
        sniff_engine("Host=db.internal;Database=orders;Username=app;Password=s3cr3t"),
        Some(SqlEngine::Postgres)
    );
}

#[test]
fn sniffs_postgres_from_npgsql_server_alias_and_specific_keywords() {
    assert_eq!(
        sniff_engine(
            "Server=pg.internal;Database=app;Port=5432;User Id=svc;Password=not-real;Ssl Mode=Require;Include Error Detail=True"
        ),
        Some(SqlEngine::Postgres)
    );
}

#[test]
fn sniffs_sql_server_from_initial_catalog() {
    assert_eq!(
        sniff_engine(
            "Data Source=tcp:sqlbox,1433;Initial Catalog=Orders;User ID=app;Password=s3cr3t"
        ),
        Some(SqlEngine::SqlServer)
    );
}

#[test]
fn sniffs_sql_server_from_server_and_database() {
    assert_eq!(
        sniff_engine("Server=sqlbox;Database=Orders;User Id=app;Password=s3cr3t"),
        Some(SqlEngine::SqlServer)
    );
}

#[test]
fn sniffs_sql_server_from_trusted_connection() {
    assert_eq!(
        sniff_engine("Server=sqlbox;Trusted_Connection=True"),
        Some(SqlEngine::SqlServer)
    );
}

#[test]
fn sniffs_sqlite_from_a_file_data_source() {
    assert_eq!(
        sniff_engine(r"Data Source=C:\repo\app.db"),
        Some(SqlEngine::Sqlite)
    );
    assert_eq!(
        sniff_engine("sqlite:///var/data/app.db"),
        Some(SqlEngine::Sqlite)
    );
    assert_eq!(
        sniff_engine("Data Source=:memory:"),
        Some(SqlEngine::Sqlite)
    );
}

#[test]
fn an_ambiguous_string_yields_no_engine() {
    // `Host=` says PostgreSQL, `Server=` + `Database=` says SQL Server. Two
    // answers is not one answer, so there is no answer.
    assert_eq!(
        sniff_engine("Server=sqlbox;Host=db.internal;Database=Orders"),
        None
    );
}

#[test]
fn an_unrecognised_string_yields_no_engine() {
    // Never a default, never "probably SQL Server".
    assert_eq!(sniff_engine("mongodb://db.internal/orders"), None);
    assert_eq!(sniff_engine("Foo=bar;Baz=qux"), None);
    assert_eq!(sniff_engine(""), None);
}

// ---------------------------------------------------------------------------
// display_form
// ---------------------------------------------------------------------------

#[test]
fn the_display_form_never_contains_the_password() {
    // The URL form is where a plausible parse leaks: splitting the authority
    // the easy way keeps `app:s3cr3t@` inside the server.
    let d = display_form("postgres://app:s3cr3t@db.internal:5432/orders");
    let rendered = format!("{d:?}");
    assert!(!rendered.contains("s3cr3t"), "leaked password: {rendered}");
    assert_eq!(d.engine, Some(SqlEngine::Postgres));
    assert_eq!(d.server.as_deref(), Some("db.internal:5432"));
    assert_eq!(d.database.as_deref(), Some("orders"));
    assert_eq!(d.auth_mode, SqlAuthMode::Password);

    let d = display_form("Server=sqlbox;Database=Orders;User Id=app;Password=s3cr3t");
    let rendered = format!("{d:?}");
    assert!(!rendered.contains("s3cr3t"), "leaked password: {rendered}");
    assert_eq!(d.engine, Some(SqlEngine::SqlServer));
    assert_eq!(d.server.as_deref(), Some("sqlbox"));
    assert_eq!(d.database.as_deref(), Some("Orders"));
    assert_eq!(d.auth_mode, SqlAuthMode::Password);
}

#[test]
fn display_form_reports_integrated_auth() {
    let d = display_form("Server=sqlbox;Initial Catalog=Orders;Trusted_Connection=True");
    assert_eq!(d.auth_mode, SqlAuthMode::Integrated);
    assert_eq!(d.database.as_deref(), Some("Orders"));
}

#[test]
fn display_form_of_a_sqlite_file_states_no_auth() {
    // "the string declares no credentials" and "we could not tell" are two
    // different answers.
    let d = display_form(r"Data Source=C:\repo\app.db");
    assert_eq!(d.engine, Some(SqlEngine::Sqlite));
    assert_eq!(d.server, None);
    assert_eq!(d.database.as_deref(), Some(r"C:\repo\app.db"));
    assert_eq!(d.auth_mode, SqlAuthMode::NoneStated);
}

#[test]
fn display_form_of_an_unrecognised_string_is_unknown() {
    let d = display_form("just some prose");
    assert_eq!(d.engine, None);
    assert_eq!(d.server, None);
    assert_eq!(d.database, None);
    assert_eq!(d.auth_mode, SqlAuthMode::Unknown);
}

#[test]
fn display_form_abstains_when_a_semicolon_password_swallows_the_rest() {
    // `Password=p;q` has no provable end, so as far as this module can prove,
    // everything after it is part of the secret. Reporting a database name
    // carved out of a password is worse than reporting nothing.
    let d = display_form("Host=db.internal;Password=pa;secretTail");
    assert_eq!(d.engine, Some(SqlEngine::Postgres));
    assert_eq!(d.server.as_deref(), Some("db.internal"));
    assert_eq!(d.database, None);
    let rendered = format!("{d:?}");
    assert!(!rendered.contains("secretTail"), "leaked tail: {rendered}");
    assert!(!rendered.contains("pa"), "leaked password: {rendered}");
}

// ---------------------------------------------------------------------------
// redact
// ---------------------------------------------------------------------------

#[test]
fn redact_removes_a_password_from_a_driver_error() {
    let err = "28P01: password authentication failed for connection \
               'Host=db.internal;Username=app;Password=s3cr3t;Database=orders'";
    let out = redact(err);
    assert!(!out.contains("s3cr3t"), "leaked: {out}");
    // The parts that are not secret survive, so the message stays useful.
    assert!(out.contains("Host=db.internal"), "{out}");
    assert!(out.contains("Database=orders"), "{out}");
    assert!(out.contains(REDACTED), "{out}");
}

#[test]
fn redact_removes_a_url_userinfo_password() {
    let out =
        redact("could not connect to postgres://app:s3cr3t@db.internal:5432/orders (timeout)");
    assert!(!out.contains("s3cr3t"), "leaked: {out}");
    assert!(out.contains("db.internal:5432/orders"), "{out}");
    assert!(out.contains("(timeout)"), "{out}");
}

#[test]
fn redact_handles_a_password_containing_a_semicolon() {
    // The case naive splitting gets wrong: the password really is `pa;ss`, and
    // `ss` is not a connection-string key, so the value has not ended there.
    let out = redact("Host=db.internal;Password=pa;ss;Database=orders");
    assert!(!out.contains("pa;ss"), "leaked whole password: {out}");
    assert!(!out.contains("ss;"), "leaked password tail: {out}");
    assert!(out.contains("Host=db.internal"), "{out}");
    // `Database=` IS a known key, so that is a provable end of the value.
    assert!(out.contains("Database=orders"), "{out}");
}

#[test]
fn redact_handles_a_password_containing_an_equals_sign() {
    let out = redact("Server=sqlbox;Password=a=b=c;Database=Orders");
    assert!(!out.contains("a=b=c"), "leaked: {out}");
    assert!(out.contains("Database=Orders"), "{out}");
}

#[test]
fn redact_handles_pwd_with_odd_spacing_and_casing() {
    let out = redact("server=sqlbox;  PWD =  s3cr3t ;database=Orders");
    assert!(!out.contains("s3cr3t"), "leaked: {out}");
    assert!(out.contains("database=Orders"), "{out}");
}

#[test]
fn redact_handles_a_quoted_password() {
    let out = redact("Server=sqlbox;Password=\"pa;ss\";Database=Orders");
    assert!(!out.contains("pa;ss"), "leaked: {out}");
    assert!(out.contains("Database=Orders"), "{out}");

    let out = redact("Server=sqlbox;Password='pa;ss';Database=Orders");
    assert!(!out.contains("pa;ss"), "leaked: {out}");
    assert!(out.contains("Database=Orders"), "{out}");
}

#[test]
fn redact_elides_to_the_end_when_the_value_has_no_provable_boundary() {
    // Nothing after the password positively identifies itself as another key,
    // so the tail is elided whole rather than half-redacted.
    let out = redact("Host=db;Password=s3cr3t and then some trailing prose");
    assert!(!out.contains("s3cr3t"), "leaked: {out}");
    assert!(!out.contains("trailing prose"), "half-redacted: {out}");
    assert!(out.contains("Host=db"), "{out}");
}

#[test]
fn redact_stops_at_a_quote_because_a_bare_value_cannot_contain_one() {
    // An unquoted ADO.NET value may not contain a quote, so a quote is a
    // provable end — which keeps the surrounding sentence readable.
    let out = redact("connection string 'Host=db;Password=s3cr3t' is invalid");
    assert!(!out.contains("s3cr3t"), "leaked: {out}");
    assert!(out.contains("is invalid"), "{out}");
}

#[test]
fn redact_leaves_text_without_a_password_untouched() {
    let text = "Server=sqlbox;Database=Orders;Trusted_Connection=True";
    assert_eq!(redact(text), text);
    assert_eq!(redact("nothing to see here"), "nothing to see here");
}

#[test]
fn redact_does_not_fire_on_a_word_that_merely_ends_in_password() {
    // `mypassword` is not the `password` keyword.
    let out = redact("mypassword=keepme;Database=Orders");
    assert!(out.contains("keepme"), "{out}");
}

// ---------------------------------------------------------------------------
// Confirmed leaks: a partially redacted secret is still a leaked secret
// ---------------------------------------------------------------------------

#[test]
fn a_password_containing_a_quote_is_fully_redacted() {
    // A quote is only a provable end when it *closes* a quoted span that opened
    // before the field. Bare, it is far likelier to be a character of the
    // password — and treating it as a terminator leaked the tail.
    let out = redact("Password=ab'cd");
    assert!(!out.contains("cd"), "leaked password tail: {out}");

    // The reviewer's exact repro produced `Password=***'ss;Database=x`.
    let out = redact("Password=pa'ss;Database=x");
    assert_eq!(out, format!("Password={REDACTED}"), "leaked password tail");

    let out = redact("Server=sqlbox;Password=ab\"cd");
    assert!(!out.contains("cd"), "leaked password tail: {out}");
    assert!(out.contains("Server=sqlbox"), "{out}");
}

#[test]
fn a_password_containing_a_semicolon_does_not_leak_a_fragment_as_a_key() {
    // `server=fragment` was parsed out of the middle of the password and came
    // back as the host. Two `Server=` pairs is the evidence that one of them
    // was not written by the author — and there is no telling which.
    let d = display_form("Password=pa;server=fragment;Server=real;Database=d");
    let rendered = format!("{d:?}");
    assert!(
        !rendered.contains("fragment"),
        "leaked fragment: {rendered}"
    );
    assert_eq!(d.confidence, SqlDisplayConfidence::Refused);
    assert_eq!(d.server, None);
    assert_eq!(d.database, None);
    assert_eq!(d.engine, None);
}

#[test]
fn display_form_refuses_rather_than_guessing_when_the_value_end_is_uncertain() {
    // Refused and "not recognisable as a connection string" are different
    // answers and must not collapse.
    let prose = display_form("just some prose");
    assert_eq!(prose.confidence, SqlDisplayConfidence::Described);
    assert_eq!(prose.auth_mode, SqlAuthMode::Unknown);

    // A repeated key: one of the two came out of the password.
    let d = display_form("Host=db;Password=pa;host=fragment;Host=real;Database=d");
    assert_eq!(d.confidence, SqlDisplayConfidence::Refused);
    assert!(!format!("{d:?}").contains("fragment"), "{d:?}");

    // A repeated password key is ambiguous for the same reason.
    let d = display_form("Server=s;Password=a;Password=b;Database=d");
    assert_eq!(d.confidence, SqlDisplayConfidence::Refused);

    // An ordinary string is still described.
    let d = display_form("Server=sqlbox;Database=Orders;User Id=app;Password=s3cr3t");
    assert_eq!(d.confidence, SqlDisplayConfidence::Described);
    assert_eq!(d.server.as_deref(), Some("sqlbox"));
}

#[test]
fn a_url_form_password_is_fully_redacted() {
    // An unencoded `/` in the userinfo ended the authority early, so the
    // password was never found and crossed both paths intact.
    let text = "could not connect to postgres://app:pa/ss@db.internal/orders";
    let out = redact(text);
    assert!(!out.contains("pa/ss"), "leaked password: {out}");
    assert!(!out.contains("ss@"), "leaked password tail: {out}");
    assert!(out.contains("db.internal/orders"), "{out}");

    let d = display_form("postgres://app:pa/ss@db.internal/orders");
    let rendered = format!("{d:?}");
    assert!(
        !rendered.contains("pa"),
        "leaked password fragment: {rendered}"
    );
}

#[test]
fn redact_covers_the_password_key_spellings_and_an_empty_value() {
    // `Pwd`, `pwd`, spacing around the `=`, and a repeat of the key.
    let out = redact("Server=s;Pwd=one;pwd = two;PASSWORD=three;Database=d");
    for secret in ["one", "two", "three"] {
        assert!(!out.contains(secret), "leaked {secret}: {out}");
    }
    assert!(out.contains("Database=d"), "{out}");

    // An empty value has nothing to leak, and the string stays readable.
    let out = redact("Server=s;Password=;Database=d");
    assert!(out.contains("Server=s"), "{out}");
    assert!(out.contains("Database=d"), "{out}");

    // A trailing `Password=` with nothing after it.
    assert_eq!(
        redact("Server=s;Password="),
        format!("Server=s;Password={REDACTED}")
    );
}

// ---------------------------------------------------------------------------
// Confirmed leak: a secret one level below the top-level `key=value` structure
// ---------------------------------------------------------------------------

#[test]
fn redact_finds_a_password_key_nested_inside_another_value() {
    // sqlx echoes an unknown *value* for a known SQLite query parameter
    // verbatim, so the secret arrives one level down from the `key=value`
    // structure the scanner walked: `immutable=` swallowed the whole of
    // `Password=hunter2` as its own value and nothing looked inside it.
    let out = redact("sqlite:/data/app.db?immutable=Password=hunter2");
    assert!(!out.contains("hunter2"), "leaked: {out}");

    // The shape sqlx actually produces, quotes and all.
    let out = redact(
        "error with configuration: unknown value 'Password=hunter2' \
         for configuration property 'immutable'",
    );
    assert!(!out.contains("hunter2"), "leaked: {out}");
    // The quote closes a span opened before the value, so the rest of the
    // sentence survives and the error is still diagnosable.
    assert!(out.contains("'immutable'"), "over-redacted: {out}");

    // The same shape through the other query parameters named in the report.
    let out = redact("unable to open database: vfs=Pwd=hunter2");
    assert!(!out.contains("hunter2"), "leaked: {out}");
    let out = redact("sqlite:app.db?mode=PASSWORD = hunter2");
    assert!(!out.contains("hunter2"), "leaked: {out}");
}

#[test]
fn redact_leaves_a_nested_pair_that_is_not_a_secret_alone() {
    // The widening is positional, not lexical: `key=value` inside a value is
    // now reachable, but only a *password* keyword redacts anything. An error
    // carrying no secret must stay readable, or failures stop being
    // diagnosable — which is its own harm.
    let text =
        "error with configuration: unknown value 'mode=ro' for configuration property 'immutable'";
    assert_eq!(redact(text), text);
}
