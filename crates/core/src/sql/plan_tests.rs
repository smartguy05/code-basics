use super::*;

use std::path::PathBuf;

use crate::sql::store::{SecretSource, SqlConnection};

/// A saved profile for `engine`, secret and all.
fn profile(engine: Option<SqlEngine>) -> SqlConnection {
    SqlConnection {
        id: "c1".into(),
        name: "Orders".into(),
        engine,
        secret: SecretSource::Literal {
            connection_string: "Data Source=app.db".into(),
        },
        workspace_root: Some(PathBuf::from("C:/code/shop")),
        allow_writes: false,
        expose_to_agents: false,
        user_named: false,
        created_at_ms: 10,
        last_used_ms: None,
    }
}

// ---------------------------------------------------------------------------
// The refusal sentences
// ---------------------------------------------------------------------------

/// A reported defect. The unsupported-engine sentence carried a hard-coded
/// "SQLite is the only engine currently supported", written when it was true.
/// Postgres and SQL Server have shipped drivers since (`driver::ENGINES` has
/// three and `driver::for_engine` answers for all three), so the sentence now
/// tells the reader something false about the build they are running — and it
/// is the sentence an agent would repeat.
#[test]
fn engine_unsupported_names_the_engine_and_not_a_stale_supported_list() {
    for engine in driver::ENGINES {
        let sentence = Refusal::EngineUnsupported { engine }.sentence();
        assert!(
            sentence.contains(engine_name(engine)),
            "the refusal must name the engine it is about: {sentence}"
        );
        assert!(
            !sentence.contains("SQLite is the only engine currently supported"),
            "the refusal must not repeat a supported-engine list that stopped being \
             true: {sentence}"
        );
    }
}

/// The sentence is derived, not typed: every engine this build can actually
/// open is named, and none that it cannot.
#[test]
fn the_unsupported_sentence_lists_exactly_the_engines_this_build_can_open() {
    let sentence = Refusal::EngineUnsupported {
        engine: SqlEngine::Postgres,
    }
    .sentence();
    for engine in driver::ENGINES {
        let named = sentence.matches(engine_name(engine)).count() > 0;
        if driver::for_engine(engine).is_some() {
            assert!(
                named,
                "{} has a driver in this build and must be listed: {sentence}",
                engine_name(engine)
            );
        }
    }
}

#[test]
fn an_undetermined_engine_says_to_choose_one_rather_than_naming_a_driver() {
    let sentence = Refusal::EngineUnknown.sentence();
    assert!(sentence.contains("engine"), "{sentence}");
    assert!(
        !sentence.contains("driver for"),
        "an undetermined engine is not a missing driver: {sentence}"
    );
}

#[test]
fn a_guard_refusal_is_described_by_the_guards_own_words_and_nothing_else() {
    let reason = "Sentinel guard wording.".to_string();
    assert_eq!(
        Refusal::Guard {
            reason: reason.clone()
        }
        .sentence(),
        reason
    );
}

#[test]
fn a_verdict_with_no_message_still_names_the_heuristic() {
    let decision = guard::Decision {
        allowed: false,
        message: None,
        ..guard::guard("delete from t", guard::Engine::Sqlite, false)
    };
    let reason = refusal_reason(&decision);
    assert!(!reason.trim().is_empty());
    assert!(reason.contains(guard::HEURISTIC_NOTE), "{reason}");
}

// ---------------------------------------------------------------------------
// The plan
// ---------------------------------------------------------------------------

#[test]
fn a_read_is_planned_and_carries_no_note() {
    let plan = execution_plan(&profile(Some(SqlEngine::Sqlite)), "select 1", false).unwrap();
    assert_eq!(plan.engine, SqlEngine::Sqlite);
    assert_eq!(plan.sql, "select 1");
    assert!(!plan.writes_allowed);
    assert_eq!(plan.note, None);
}

/// The consent flag decides what *may* be sent; it does not overrule a verdict.
/// A statement the guard refuses on its own terms — one it could not parse, or
/// parsed into something it does not recognise as a read — stays refused with
/// writes on, because "writes are allowed here" is not "anything is allowed
/// here".
#[test]
fn a_refused_verdict_is_not_lifted_by_writes_allowed() {
    let connection = profile(Some(SqlEngine::Sqlite));
    let unparseable = "this is not sql at all ((";

    let strict = execution_plan(&connection, unparseable, false);
    let permissive = execution_plan(&connection, unparseable, true);

    assert!(
        matches!(strict, Err(Refusal::Guard { .. })),
        "expected the guard to refuse unparseable text: {strict:?}"
    );
    assert_eq!(
        strict, permissive,
        "allowing writes must not turn a refused verdict into a plan"
    );
}

#[test]
fn an_undetermined_engine_never_reaches_the_guard() {
    assert_eq!(
        execution_plan(&profile(None), "drop table t", true),
        Err(Refusal::EngineUnknown)
    );
}

#[test]
fn a_write_is_refused_when_consent_is_withheld_and_planned_when_it_is_given() {
    let connection = profile(Some(SqlEngine::Sqlite));
    assert!(matches!(
        execution_plan(&connection, "delete from orders", false),
        Err(Refusal::Guard { .. })
    ));
    let allowed = execution_plan(&connection, "delete from orders", true).unwrap();
    assert!(allowed.writes_allowed);
    assert!(
        allowed.note.is_some(),
        "an allowed write must still say that it is a write"
    );
}

#[test]
fn every_engine_maps_to_its_own_guard_dialect() {
    let mut seen = Vec::new();
    for engine in driver::ENGINES {
        let dialect = guard_engine(engine);
        assert!(
            !seen.contains(&dialect),
            "{engine:?} shares a dialect with an engine already mapped"
        );
        seen.push(dialect);
    }
}
