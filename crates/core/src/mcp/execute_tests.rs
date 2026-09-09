use super::*;

use std::path::PathBuf;

use crate::sql::store::SecretSource;

/// A connection the user exposed to agents. `allow_writes` is a parameter
/// because the whole point of these tests is that it makes no difference.
fn exposed(engine: Option<SqlEngine>, allow_writes: bool) -> SqlConnection {
    SqlConnection {
        id: "conn-1".into(),
        name: "Reporting".into(),
        engine,
        secret: SecretSource::AppSettings {
            path: PathBuf::from("C:/code/shop/appsettings.json"),
            key: "ConnectionStrings:Default".into(),
        },
        workspace_root: Some(PathBuf::from("C:/code/shop")),
        allow_writes,
        expose_to_agents: true,
        user_named: true,
        created_at_ms: 1,
        last_used_ms: None,
    }
}

#[test]
fn a_read_is_planned() {
    let plan = agent_plan(&exposed(Some(SqlEngine::Postgres), false), "SELECT 1").unwrap();
    assert_eq!(plan.engine, SqlEngine::Postgres);
    assert_eq!(plan.sql, "SELECT 1");
    assert_eq!(plan.note, None);
}

#[test]
fn an_exposed_connection_with_allow_writes_on_still_refuses_a_write() {
    // The security property of the whole feature: consent to *see* a connection
    // is not consent to write to it, and the connection's own writes-allowed
    // setting is not consulted on this path.
    let connection = exposed(Some(SqlEngine::Postgres), true);

    // The same statement on the same connection is allowed for the human
    // console, so this is a difference this path creates rather than one it
    // inherits.
    assert!(
        crate::sql::plan::execution_plan(&connection, "INSERT INTO orders VALUES (1)", true)
            .is_ok(),
        "the console must still allow it, or this test proves nothing"
    );

    let refusal = agent_plan(&connection, "INSERT INTO orders VALUES (1)").unwrap_err();
    assert_eq!(refusal.code(), "writeRefusedUnconditionally");
}

#[test]
fn the_plan_this_path_produces_always_carries_writes_allowed_false() {
    for allow_writes in [false, true] {
        for engine in [SqlEngine::Sqlite, SqlEngine::Postgres, SqlEngine::SqlServer] {
            let plan = agent_plan(&exposed(Some(engine), allow_writes), "SELECT 1")
                .expect("a read is planned");
            assert!(
                !plan.writes_allowed,
                "{engine:?} with allow_writes={allow_writes} produced a writable plan"
            );
        }
    }
}

#[test]
fn the_connect_spec_this_path_produces_always_refuses_writes() {
    let spec = agent_connect_spec("Server=db".to_string());
    assert!(!spec.writes_allowed);
}

#[test]
fn a_write_refusal_says_that_enabling_writes_would_not_help_here() {
    // `HEURISTIC_NOTE` advises the reader to enable writes for the connection.
    // On this path that is false, and reusing the message would be the bug.
    let refusal = agent_plan(
        &exposed(Some(SqlEngine::Postgres), true),
        "UPDATE orders SET total = 0",
    )
    .unwrap_err();
    let sentence = refusal.sentence();

    assert!(
        !sentence.contains("enable writes for this connection"),
        "the agent was told to flip a switch that cannot help it: {sentence}"
    );
    assert!(
        sentence.contains("would not change this answer"),
        "the refusal must say the setting would not help: {sentence}"
    );
    assert!(
        sentence.contains("UPDATE"),
        "the refusal must still name what it found: {sentence}"
    );
}

#[test]
fn an_unrecognised_statement_is_not_reported_as_a_write() {
    // The guard keeps *refused* and *write* apart, and so must this. A model
    // told "this is a write" about something nobody classified would report a
    // fact that is not in evidence.
    let refusal =
        agent_plan(&exposed(Some(SqlEngine::Postgres), true), "!!! not sql !!!").unwrap_err();
    assert_eq!(refusal.code(), "guardRefused");
    assert!(!refusal
        .sentence()
        .contains("enable writes for this connection"));
}

#[test]
fn an_undetermined_engine_is_its_own_answer_and_names_the_connection() {
    let refusal = agent_plan(&exposed(None, false), "SELECT 1").unwrap_err();
    assert_eq!(
        refusal,
        McpRefusal::EngineUndetermined {
            connection: "conn-1".to_string()
        }
    );
}

#[test]
fn the_enforcement_this_path_would_get_is_never_the_writes_allowed_one() {
    for engine in [SqlEngine::Sqlite, SqlEngine::Postgres, SqlEngine::SqlServer] {
        assert_ne!(
            agent_enforcement(Some(engine)),
            ReadOnlyEnforcement::NotEnforced
        );
    }
    assert_eq!(agent_enforcement(None), ReadOnlyEnforcement::Undetermined);
}

#[test]
fn every_engine_refuses_a_write_on_this_path() {
    for engine in [SqlEngine::Sqlite, SqlEngine::Postgres, SqlEngine::SqlServer] {
        let refusal = agent_plan(&exposed(Some(engine), true), "DELETE FROM orders").unwrap_err();
        assert_eq!(
            refusal.code(),
            "writeRefusedUnconditionally",
            "{engine:?} let a write through"
        );
    }
}
