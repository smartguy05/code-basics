use super::*;

fn verdict(sql: &str) -> Verdict {
    classify(sql, Engine::SqlServer)
}

fn pg(sql: &str) -> Verdict {
    classify(sql, Engine::Postgres)
}

fn refusal(v: &Verdict) -> &RefusalReason {
    match v {
        Verdict::Refused { reason } => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn write_kind(v: &Verdict) -> &'static str {
    match v {
        Verdict::Write { kind } => kind,
        other => panic!("expected a write, got {other:?}"),
    }
}

#[test]
fn a_plain_select_is_read_only() {
    assert_eq!(
        verdict("SELECT TOP 10 id, name FROM dbo.Customers WHERE name LIKE 'a%'"),
        Verdict::ReadOnly
    );
    assert_eq!(pg("select 1"), Verdict::ReadOnly);
    assert_eq!(
        classify("SELECT * FROM t ORDER BY id LIMIT 5", Engine::Sqlite),
        Verdict::ReadOnly
    );
}

#[test]
fn an_insert_is_a_write() {
    assert_eq!(
        write_kind(&verdict("INSERT INTO t (a) VALUES (1)")),
        "INSERT"
    );
    assert_eq!(write_kind(&verdict("UPDATE t SET a = 1")), "UPDATE");
    assert_eq!(write_kind(&verdict("DELETE FROM t WHERE id = 1")), "DELETE");
    assert_eq!(write_kind(&verdict("DROP TABLE t")), "DROP");
    assert_eq!(write_kind(&verdict("TRUNCATE TABLE t")), "TRUNCATE");
    assert_eq!(
        write_kind(&verdict("CREATE TABLE t (a INT)")),
        "CREATE TABLE"
    );
    assert_eq!(write_kind(&verdict("GRANT SELECT ON t TO bob")), "GRANT");
}

#[test]
fn a_cte_ending_in_a_select_is_read_only() {
    assert_eq!(
        pg("WITH recent AS (SELECT id FROM orders WHERE created > now()) SELECT * FROM recent"),
        Verdict::ReadOnly
    );
}

#[test]
fn a_cte_containing_an_insert_is_a_write() {
    assert_eq!(
        write_kind(&pg(
            "WITH gone AS (DELETE FROM orders WHERE id = 1 RETURNING *) SELECT * FROM gone"
        )),
        "DELETE"
    );
    assert_eq!(
        write_kind(&pg(
            "WITH added AS (INSERT INTO orders (id) VALUES (1) RETURNING *) SELECT * FROM added"
        )),
        "INSERT"
    );
    assert_eq!(
        write_kind(&pg(
            "WITH touched AS (UPDATE orders SET n = 1 RETURNING *) SELECT * FROM touched"
        )),
        "UPDATE"
    );
}

#[test]
fn select_into_creates_a_table_and_is_a_write() {
    assert_eq!(
        write_kind(&verdict("SELECT id, name INTO dbo.Copy FROM dbo.Customers")),
        "SELECT ... INTO"
    );
}

#[test]
fn a_batch_is_as_strict_as_its_strictest_statement() {
    // One write among reads makes the whole batch a write.
    assert_eq!(
        write_kind(&verdict("SELECT 1; DELETE FROM t; SELECT 2")),
        "DELETE"
    );
    // A refusal is stricter still: it outranks a write, because a refusal is
    // never lifted by the writes-allowed setting.
    let v = verdict("SELECT 1; DELETE FROM t; SELECT * FROM OPENROWSET('x','y','z')");
    assert!(
        matches!(
            refusal(&v),
            RefusalReason::DenylistedFunction { name } if name == "OPENROWSET"
        ),
        "got {v:?}"
    );
}

#[test]
fn a_comment_cannot_hide_a_keyword() {
    // The write is real; the comment must not obscure it.
    assert_eq!(
        write_kind(&verdict("/* harmless report */ DELETE FROM t")),
        "DELETE"
    );
    assert_eq!(write_kind(&verdict("-- report\nDELETE FROM t")), "DELETE");
    // ...and a keyword that lives only inside a comment is not a write.
    assert_eq!(
        verdict("SELECT id FROM t -- DELETE FROM t"),
        Verdict::ReadOnly
    );
    assert_eq!(
        verdict("SELECT /* DROP TABLE t */ id FROM t"),
        Verdict::ReadOnly
    );
}

#[test]
fn a_keyword_inside_a_string_literal_is_not_a_write() {
    assert_eq!(
        verdict("SELECT id FROM audit WHERE action = 'DELETE FROM t'"),
        Verdict::ReadOnly
    );
    assert_eq!(
        pg("SELECT id FROM audit WHERE action = 'DROP TABLE orders'"),
        Verdict::ReadOnly
    );
    // A dollar-quoted body is the case a regex guard reads worst.
    assert_eq!(pg("SELECT $$ DELETE FROM t $$"), Verdict::ReadOnly);
}

#[test]
fn a_bracketed_identifier_named_delete_is_not_a_write() {
    assert_eq!(verdict("SELECT [delete] FROM [update]"), Verdict::ReadOnly);
    assert_eq!(pg(r#"SELECT "delete" FROM "update""#), Verdict::ReadOnly);
}

#[test]
fn sql_that_will_not_parse_is_refused_not_allowed() {
    let v = verdict("SELECT FROM WHERE ORDER");
    assert!(
        matches!(refusal(&v), RefusalReason::ParseError { .. }),
        "got {v:?}"
    );
    let v = verdict("SELCT 1 FROM t");
    assert!(
        matches!(refusal(&v), RefusalReason::ParseError { .. }),
        "got {v:?}"
    );
}

#[test]
fn an_unclassifiable_statement_is_refused_rather_than_called_a_write() {
    let v = pg("SET statement_timeout = 5");
    assert!(
        matches!(refusal(&v), RefusalReason::Unrecognised { .. }),
        "got {v:?}"
    );
    let v = verdict("COMMIT");
    assert!(
        matches!(refusal(&v), RefusalReason::Unrecognised { .. }),
        "got {v:?}"
    );
}

#[test]
fn explain_is_read_only_but_explain_analyze_is_not() {
    assert_eq!(pg("EXPLAIN SELECT * FROM t"), Verdict::ReadOnly);
    assert_eq!(
        pg("EXPLAIN INSERT INTO t (a) VALUES (1)"),
        Verdict::ReadOnly
    );
    assert_eq!(
        write_kind(&pg("EXPLAIN ANALYZE INSERT INTO t (a) VALUES (1)")),
        "EXPLAIN ANALYZE"
    );
    assert_eq!(
        write_kind(&pg("EXPLAIN ANALYZE SELECT * FROM t")),
        "EXPLAIN ANALYZE"
    );
}

#[test]
fn exec_and_sp_executesql_are_refused() {
    // A denylisted name is refused, not called a write: the guard cannot see
    // what the called thing does, so it must not pretend to have classified it.
    let v = verdict("EXEC sp_executesql N'SELECT 1'");
    assert!(
        matches!(
            refusal(&v),
            RefusalReason::DenylistedFunction { name } if name == "sp_executesql"
        ),
        "got {v:?}"
    );
    let v = verdict("EXEC xp_cmdshell 'dir'");
    assert!(
        matches!(
            refusal(&v),
            RefusalReason::DenylistedFunction { name } if name == "xp_cmdshell"
        ),
        "got {v:?}"
    );
    let v = pg("SELECT pg_read_file('/etc/passwd')");
    assert!(
        matches!(
            refusal(&v),
            RefusalReason::DenylistedFunction { name } if name == "pg_read_file"
        ),
        "got {v:?}"
    );
    // A plain EXEC of a stored procedure is classified as a write, and so is
    // still stopped in read-only mode.
    let d = guard("EXEC dbo.RebuildIndexes", Engine::SqlServer, false);
    assert!(!d.allowed, "got {d:?}");

    // A denylisted name is NOT lifted by the writes-allowed setting: a refusal
    // and a write are different answers with different consequences.
    let d = guard("EXEC xp_cmdshell 'dir'", Engine::SqlServer, true);
    assert!(!d.allowed, "got {d:?}");
}

#[test]
fn the_go_batch_separator_is_refused_in_read_only_mode() {
    let v = verdict("SELECT 1\nGO\nSELECT 2");
    assert!(
        matches!(
            refusal(&v),
            RefusalReason::BatchSeparator { keyword } if keyword == "GO"
        ),
        "got {v:?}"
    );
    // ...including with a repeat count, and in any case.
    let v = verdict("SELECT 1\n  go 5  \nSELECT 2");
    assert!(
        matches!(refusal(&v), RefusalReason::BatchSeparator { .. }),
        "got {v:?}"
    );
    // `GO` is a client separator, not SQL, so writes-allowed does not lift it.
    let d = guard("SELECT 1\nGO", Engine::SqlServer, true);
    assert!(!d.allowed, "got {d:?}");
    // A column called `go` is not a batch separator.
    assert_eq!(verdict("SELECT go FROM t"), Verdict::ReadOnly);
}

#[test]
fn an_empty_statement_is_refused_rather_than_run() {
    for sql in [
        "",
        "   ",
        "\n\t ",
        ";",
        "-- just a comment",
        "/* nothing */",
    ] {
        let v = verdict(sql);
        assert!(
            matches!(refusal(&v), RefusalReason::Empty),
            "{sql:?} gave {v:?}"
        );
    }
}

#[test]
fn writes_allowed_lets_a_write_through_but_still_reports_it() {
    let blocked = guard("INSERT INTO t (a) VALUES (1)", Engine::SqlServer, false);
    assert!(!blocked.allowed);
    assert_eq!(blocked.verdict, Verdict::Write { kind: "INSERT" });

    let allowed = guard("INSERT INTO t (a) VALUES (1)", Engine::SqlServer, true);
    assert!(allowed.allowed);
    assert_eq!(allowed.verdict, Verdict::Write { kind: "INSERT" });
    let message = allowed
        .message
        .expect("an allowed write still reports itself");
    assert!(message.contains("INSERT"), "{message}");

    // A read is allowed either way and reports nothing.
    let read = guard("SELECT 1", Engine::SqlServer, false);
    assert!(read.allowed);
    assert_eq!(read.message, None);
}

#[test]
fn a_refusal_says_it_is_a_text_heuristic_and_not_a_sandbox() {
    let d = guard("DELETE FROM t", Engine::SqlServer, false);
    let message = d.message.expect("a refusal explains itself");

    assert!(
        message.contains("a heuristic over the SQL text, not a database-enforced sandbox"),
        "{message}"
    );
    // Both real fixes are offered.
    assert!(
        message.contains("enable writes for this connection"),
        "{message}"
    );
    assert!(
        message.contains("connect with a login that has no write permission"),
        "{message}"
    );
    // ...and nothing in it claims the database is protected.
    let lower = message.to_lowercase();
    assert!(!lower.contains("sandboxed"), "{message}");
    assert!(!lower.contains("is safe"), "{message}");
    assert!(!lower.contains("protected"), "{message}");

    // Every refusal carries the note, whatever the reason.
    for sql in ["SELECT FROM WHERE", "", "SELECT 1\nGO", "COMMIT"] {
        let d = guard(sql, Engine::SqlServer, false);
        let message = d.message.unwrap_or_default();
        assert!(
            message.contains("a heuristic over the SQL text, not a database-enforced sandbox"),
            "{sql:?} gave {message:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Beyond the required cases: the two places this guard abstains where a
// simpler one would answer.

#[test]
fn a_data_modifying_cte_nested_in_a_subquery_is_refused_rather_than_read() {
    // The AST walk does not reach a `WITH` inside a derived table, so the
    // text cross-check catches it — and abstains rather than calling it a
    // write, because nothing here classified the statement.
    let v = pg("SELECT * FROM (WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x) q");
    assert!(
        matches!(
            refusal(&v),
            RefusalReason::HiddenKeyword { keyword } if keyword == "DELETE"
        ),
        "got {v:?}"
    );
}

#[test]
fn a_locking_read_is_not_mistaken_for_an_update() {
    // `FOR UPDATE` takes locks; it does not write rows, and the cross-check
    // must not turn it into a refusal.
    assert_eq!(pg("SELECT * FROM t FOR UPDATE"), Verdict::ReadOnly);
}

#[test]
fn a_pragma_that_sets_a_value_is_a_write_and_one_that_reads_it_is_not() {
    assert_eq!(
        write_kind(&classify("PRAGMA journal_mode = 'WAL'", Engine::Sqlite)),
        "PRAGMA"
    );
    assert_eq!(
        classify("PRAGMA journal_mode", Engine::Sqlite),
        Verdict::ReadOnly
    );
}

#[test]
fn a_select_into_on_one_side_of_a_union_is_still_a_write() {
    assert_eq!(
        write_kind(&verdict("SELECT a FROM t UNION SELECT b INTO x FROM u")),
        "SELECT ... INTO"
    );
}

#[test]
fn attaching_a_database_is_a_write() {
    assert_eq!(
        write_kind(&classify("ATTACH DATABASE 'x.db' AS x", Engine::Sqlite)),
        "ATTACH DATABASE"
    );
}

fn lite(sql: &str) -> Verdict {
    classify(sql, Engine::Sqlite)
}

fn denylisted_name(v: &Verdict) -> &str {
    match refusal(v) {
        RefusalReason::DenylistedFunction { name } => name,
        other => panic!("expected a denylisted function, got {other:?}"),
    }
}

#[test]
fn explain_with_a_parenthesised_analyze_option_is_not_read_only() {
    // sqlparser puts the parenthesised option form in `options` and leaves the
    // `analyze` field false, so reading only the field lets a write execute
    // while the guard reports a read.
    assert_eq!(
        write_kind(&pg("EXPLAIN (ANALYZE) INSERT INTO t VALUES (1)")),
        "EXPLAIN ANALYZE"
    );
    assert_eq!(
        write_kind(&pg("explain (analyze) insert into t values (1)")),
        "EXPLAIN ANALYZE"
    );
    assert_eq!(
        write_kind(&pg("EXPLAIN (AnAlYzE, VERBOSE) DELETE FROM t")),
        "EXPLAIN ANALYZE"
    );
    assert_eq!(
        write_kind(&pg("EXPLAIN (ANALYZE) SELECT * FROM t")),
        "EXPLAIN ANALYZE"
    );
    let d = guard(
        "EXPLAIN (ANALYZE) INSERT INTO t VALUES (1)",
        Engine::Postgres,
        false,
    );
    assert!(!d.allowed, "got {d:?}");
}

#[test]
fn explain_analyze_bare_form_is_still_caught() {
    assert_eq!(
        write_kind(&pg("EXPLAIN ANALYZE INSERT INTO t (a) VALUES (1)")),
        "EXPLAIN ANALYZE"
    );
    assert_eq!(
        write_kind(&pg("explain analyze select * from t")),
        "EXPLAIN ANALYZE"
    );
}

#[test]
fn a_plan_only_explain_option_is_still_read_only() {
    assert_eq!(pg("EXPLAIN (VERBOSE) SELECT * FROM t"), Verdict::ReadOnly);
    assert_eq!(
        pg("EXPLAIN (VERBOSE, COSTS false) INSERT INTO t VALUES (1)"),
        Verdict::ReadOnly
    );
}

#[test]
fn an_unrecognised_explain_option_is_refused_rather_than_allowed() {
    let v = pg("EXPLAIN (WIBBLE) INSERT INTO t VALUES (1)");
    assert!(
        matches!(refusal(&v), RefusalReason::Unrecognised { .. }),
        "got {v:?}"
    );
}

#[test]
fn a_quoted_denylisted_function_is_still_refused() {
    assert_eq!(
        denylisted_name(&verdict("EXEC [master]..[xp_cmdshell] 'dir'")),
        "xp_cmdshell"
    );
    assert_eq!(
        denylisted_name(&pg(r#"SELECT * FROM "pg_read_file"('/etc/passwd')"#)),
        "pg_read_file"
    );
    assert_eq!(
        denylisted_name(&lite("SELECT `readfile`('/etc/passwd')")),
        "readfile"
    );
    assert_eq!(
        denylisted_name(&lite(r#"SELECT "readfile"('/etc/passwd')"#)),
        "readfile"
    );
    // Still not lifted by the writes-allowed setting.
    let d = guard(
        "EXEC [master]..[xp_cmdshell] 'dir'",
        Engine::SqlServer,
        true,
    );
    assert!(!d.allowed, "got {d:?}");
}

#[test]
fn a_string_literal_naming_a_denylisted_function_is_not_a_call() {
    assert_eq!(
        verdict("SELECT id FROM audit WHERE proc_name = 'xp_cmdshell'"),
        Verdict::ReadOnly
    );
    assert_eq!(
        pg("SELECT id FROM audit WHERE proc_name = 'pg_read_file'"),
        Verdict::ReadOnly
    );
}

#[test]
fn a_column_named_like_a_denylisted_function_is_not_a_call() {
    assert_eq!(verdict("SELECT readfile FROM settings"), Verdict::ReadOnly);
    assert_eq!(pg("SELECT t.lo_import FROM t"), Verdict::ReadOnly);
}
