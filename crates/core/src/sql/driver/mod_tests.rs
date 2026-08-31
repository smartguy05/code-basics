use super::*;

// ---------------------------------------------------------------------------
// Read-only wording
// ---------------------------------------------------------------------------

#[test]
fn only_the_driver_variant_claims_the_driver_is_enforcing_it() {
    assert_eq!(
        ReadOnlyEnforcement::Driver.label(),
        "Opened read-only by the driver"
    );
    assert!(ReadOnlyEnforcement::Driver.is_driver_enforced());
    assert!(!ReadOnlyEnforcement::GuardOnly.is_driver_enforced());
    assert!(!ReadOnlyEnforcement::NotEnforced.is_driver_enforced());
}

#[test]
fn the_weaker_wordings_do_not_claim_the_driver_opened_anything_read_only() {
    for weak in [
        ReadOnlyEnforcement::GuardOnly,
        ReadOnlyEnforcement::NotEnforced,
    ] {
        let label = weak.label().to_ascii_lowercase();
        assert!(
            !label.contains("by the driver"),
            "{weak:?} must not borrow the strong claim: {label}"
        );
    }
    // And the weak one must say out loud that the connection can still write,
    // rather than leaving "read-only" to be read as a guarantee.
    assert!(ReadOnlyEnforcement::GuardOnly.label().contains("can write"));
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[test]
fn a_driver_error_redacts_a_connection_string_it_was_handed() {
    let e = DriverError::connect(
        "error returned from database: Server=db;Database=app;Password=hunter2;",
    );
    assert!(
        !e.message.contains("hunter2"),
        "the secret survived: {}",
        e.message
    );
    assert!(e.message.contains("Server=db"));
}

#[test]
fn a_url_password_is_redacted_too() {
    let e = DriverError::execute("could not connect to postgres://app:s3cret@localhost/db");
    assert!(!e.message.contains("s3cret"), "{}", e.message);
}

#[test]
fn a_connect_failure_carries_no_statement_index_even_when_one_is_offered() {
    // "The connection never opened" and "statement 0 failed" are different
    // facts; a defaulted 0 would make the first read as the second.
    let e = DriverError::connect("no such file");
    match e.to_event(Some(0)) {
        SqlEvent::Failed {
            statement_index, ..
        } => assert_eq!(statement_index, None),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn an_execute_failure_keeps_the_statement_index() {
    let e = DriverError::execute("no such table: widgets");
    match e.to_event(Some(2)) {
        SqlEvent::Failed {
            statement_index, ..
        } => assert_eq!(statement_index, Some(2)),
        other => panic!("expected Failed, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Stopping
// ---------------------------------------------------------------------------

#[test]
fn a_fresh_stop_channel_is_not_already_signalled() {
    let (tx, rx) = stop_channel();
    assert!(!*rx.borrow());
    tx.send(true).unwrap();
    assert!(*rx.borrow());
}

#[test]
fn stopped_and_finished_are_distinguishable_outcomes_carrying_the_same_rows() {
    let result = SqlResultSet {
        columns: vec![],
        rows: vec![],
        row_cap: None,
        rows_affected: None,
        elapsed_ms: 1,
        statement_index: 0,
    };
    let finished = StatementOutcome::Finished(result.clone());
    let stopped = StatementOutcome::Stopped(result);
    assert_ne!(finished, stopped);
    assert!(!finished.was_stopped());
    assert!(stopped.was_stopped());
    assert_eq!(finished.result(), stopped.result());
}

// ---------------------------------------------------------------------------
// Connecting has a deadline
// ---------------------------------------------------------------------------

#[test]
fn a_connect_deadline_failure_is_not_spelled_like_a_refusal() {
    let timed_out = DriverError::connect_timed_out(std::time::Duration::from_secs(10));
    let refused = DriverError::connect("unable to open database file");
    // Two different situations for the reader: nothing answered, versus the
    // handle said no. Collapsing them would send them to the same fix.
    assert_eq!(timed_out.stage, ErrorStage::ConnectTimeout);
    assert_eq!(refused.stage, ErrorStage::Connect);
    assert_ne!(timed_out.stage, refused.stage);
    assert!(
        timed_out.message.contains("10.0 s"),
        "{}",
        timed_out.message
    );
    assert!(
        timed_out.message.contains("no statement ran"),
        "{}",
        timed_out.message
    );
}

#[test]
fn a_deadline_failure_carries_no_statement_index_either() {
    let e = DriverError::connect_timed_out(std::time::Duration::from_secs(1));
    match e.to_event(Some(0)) {
        SqlEvent::Failed {
            statement_index, ..
        } => assert_eq!(statement_index, None, "no statement had started"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn a_deadline_is_described_in_the_unit_a_person_would_read_it_in() {
    assert_eq!(
        describe_duration(std::time::Duration::from_millis(250)),
        "250 ms"
    );
    assert_eq!(describe_duration(std::time::Duration::ZERO), "0 ms");
    assert_eq!(
        describe_duration(std::time::Duration::from_secs(10)),
        "10.0 s"
    );
    assert_eq!(
        describe_duration(std::time::Duration::from_millis(1500)),
        "1.5 s"
    );
}

#[tokio::test]
async fn a_connect_that_answers_in_time_passes_its_own_result_through_untouched() {
    let ok: DriverResult<u8> =
        within_connect_deadline(std::time::Duration::from_secs(30), async { Ok(7) }).await;
    assert_eq!(ok, Ok(7));

    // A refusal inside the deadline stays a refusal — the deadline must not
    // relabel someone else’s failure.
    let refused: DriverResult<u8> =
        within_connect_deadline(std::time::Duration::from_secs(30), async {
            Err(DriverError::connect("unable to open database file"))
        })
        .await;
    assert_eq!(refused.unwrap_err().stage, ErrorStage::Connect);
}

#[tokio::test]
async fn a_connect_that_never_answers_ends_as_a_deadline_failure() {
    let never = std::future::pending::<DriverResult<u8>>();
    let err = within_connect_deadline(std::time::Duration::from_millis(10), never)
        .await
        .expect_err("a connect that never answers must not hang");
    assert_eq!(err.stage, ErrorStage::ConnectTimeout);
}

#[test]
fn every_driver_gets_a_deadline_by_default() {
    // A zero default would fail every connect; an unbounded one is the bug.
    assert!(DEFAULT_CONNECT_TIMEOUT > std::time::Duration::ZERO);
    assert!(DEFAULT_CONNECT_TIMEOUT <= std::time::Duration::from_secs(60));
}

// ---------------------------------------------------------------------------
// Read-only wording, per engine
//
// Three engines enforce read-only three genuinely different ways, and the whole
// point of these tests is that the three sentences cannot be swapped. SQL
// Server is the one that matters: it has no read-only session mode at all, so
// the best available is a transaction that is always rolled back — the write
// *runs* and is then undone. That is weaker than the other two, and the wording
// must not be allowed to drift towards them.
// ---------------------------------------------------------------------------

/// Every variant, so a new one cannot be added without these tests seeing it.
const ALL_ENFORCEMENTS: [ReadOnlyEnforcement; 6] = [
    ReadOnlyEnforcement::Driver,
    ReadOnlyEnforcement::ServerSession,
    ReadOnlyEnforcement::RolledBackTransaction,
    ReadOnlyEnforcement::GuardOnly,
    ReadOnlyEnforcement::NotEnforced,
    ReadOnlyEnforcement::Undetermined,
];

#[test]
fn no_two_enforcements_share_a_label_or_a_detail() {
    for (i, a) in ALL_ENFORCEMENTS.iter().enumerate() {
        for b in &ALL_ENFORCEMENTS[i + 1..] {
            assert_ne!(a.label(), b.label(), "{a:?} and {b:?} share a label");
            assert_ne!(a.detail(), b.detail(), "{a:?} and {b:?} share a detail");
        }
    }
}

#[test]
fn each_engine_gets_its_own_mechanism_and_its_own_words() {
    assert_eq!(
        declared_read_only_enforcement(SqlEngine::Sqlite),
        ReadOnlyEnforcement::Driver
    );
    assert_eq!(
        declared_read_only_enforcement(SqlEngine::Postgres),
        ReadOnlyEnforcement::ServerSession
    );
    assert_eq!(
        declared_read_only_enforcement(SqlEngine::SqlServer),
        ReadOnlyEnforcement::RolledBackTransaction
    );

    // And no two engines end up reading the same.
    for (i, a) in ENGINES.iter().enumerate() {
        for b in &ENGINES[i + 1..] {
            assert_ne!(
                declared_read_only_enforcement(*a).label(),
                declared_read_only_enforcement(*b).label(),
                "{a:?} and {b:?} must not read alike"
            );
        }
    }
}

#[test]
fn sql_servers_wording_never_claims_to_be_a_sandbox() {
    let e = declared_read_only_enforcement(SqlEngine::SqlServer);
    let label = e.label().to_ascii_lowercase();
    let detail = e.detail().to_ascii_lowercase();

    // It must say the weak thing out loud.
    assert!(detail.contains("not a sandbox"), "{detail}");
    assert!(detail.contains("rolled back"), "{detail}");

    // And it must not borrow either strong claim. "Before it runs" is the
    // phrase the two engines that actually refuse a write use; a rollback lets
    // the write run.
    for forbidden in [
        "before it runs",
        "refuses a write",
        "by the driver",
        "read-only session",
    ] {
        assert!(
            !label.contains(forbidden) && !detail.contains(forbidden),
            "SQL Server must not claim {forbidden:?}: {label} / {detail}"
        );
    }
}

#[test]
fn only_the_two_engines_that_refuse_a_write_say_they_do() {
    let refusing: Vec<ReadOnlyEnforcement> = ALL_ENFORCEMENTS
        .iter()
        .copied()
        .filter(|e| e.detail().contains("before it runs"))
        .collect();
    assert_eq!(
        refusing,
        vec![
            ReadOnlyEnforcement::Driver,
            ReadOnlyEnforcement::ServerSession
        ]
    );
    assert!(ReadOnlyEnforcement::Driver.refuses_writes_before_they_run());
    assert!(ReadOnlyEnforcement::ServerSession.refuses_writes_before_they_run());
    assert!(!ReadOnlyEnforcement::RolledBackTransaction.refuses_writes_before_they_run());
    assert!(!ReadOnlyEnforcement::GuardOnly.refuses_writes_before_they_run());
    assert!(!ReadOnlyEnforcement::NotEnforced.refuses_writes_before_they_run());
    assert!(!ReadOnlyEnforcement::Undetermined.refuses_writes_before_they_run());
}

#[test]
fn a_server_side_setting_is_not_reported_as_a_driver_opened_handle() {
    // `is_driver_enforced` is the narrow fact "the handle was opened read-only",
    // which is SQLite's mechanism and nobody else's. A server session setting is
    // just as strong and is still a different thing.
    assert!(ReadOnlyEnforcement::Driver.is_driver_enforced());
    for other in [
        ReadOnlyEnforcement::ServerSession,
        ReadOnlyEnforcement::RolledBackTransaction,
        ReadOnlyEnforcement::GuardOnly,
        ReadOnlyEnforcement::NotEnforced,
        ReadOnlyEnforcement::Undetermined,
    ] {
        assert!(!other.is_driver_enforced(), "{other:?}");
    }
}

#[test]
fn strength_keeps_running_and_then_undoing_apart_from_never_running() {
    use EnforcementStrength::*;
    assert_eq!(ReadOnlyEnforcement::Driver.strength(), Refused);
    assert_eq!(ReadOnlyEnforcement::ServerSession.strength(), Refused);
    assert_eq!(
        ReadOnlyEnforcement::RolledBackTransaction.strength(),
        Undone
    );
    assert_eq!(ReadOnlyEnforcement::GuardOnly.strength(), HeuristicOnly);
    assert_eq!(ReadOnlyEnforcement::NotEnforced.strength(), Nothing);
    assert_eq!(ReadOnlyEnforcement::Undetermined.strength(), Unknown);
    // The one collapse that would matter: a rollback reading as a refusal.
    assert_ne!(
        ReadOnlyEnforcement::RolledBackTransaction.strength(),
        ReadOnlyEnforcement::Driver.strength()
    );
}

// ---------------------------------------------------------------------------
// Undetermined
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_engine_stays_undetermined_rather_than_borrowing_an_answer() {
    assert_eq!(
        expected_enforcement(None, false),
        ReadOnlyEnforcement::Undetermined
    );
    let e = ReadOnlyEnforcement::Undetermined;
    assert!(!e.refuses_writes_before_they_run());
    assert!(!e.is_driver_enforced());
    let detail = e.detail().to_ascii_lowercase();
    for forbidden in ["before it runs", "rolled back", "by the driver"] {
        assert!(!detail.contains(forbidden), "{detail}");
    }
}

#[test]
fn allowing_writes_is_never_enforcement_whatever_the_engine() {
    assert_eq!(
        expected_enforcement(None, true),
        ReadOnlyEnforcement::NotEnforced
    );
    for engine in ENGINES {
        assert_eq!(
            expected_enforcement(Some(engine), true),
            ReadOnlyEnforcement::NotEnforced,
            "{engine:?}"
        );
        assert_eq!(
            expected_enforcement(Some(engine), false),
            declared_read_only_enforcement(engine),
            "{engine:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// The registry
// ---------------------------------------------------------------------------

#[test]
fn the_registry_never_hands_back_a_driver_for_a_different_engine() {
    // The copy-paste failure this exists to catch: registering SqliteDriver
    // under Postgres would open a SQLite file for a postgres DSN.
    for engine in ENGINES {
        if let Some(driver) = for_engine(engine) {
            assert_eq!(driver.engine(), engine, "registered under the wrong engine");
        }
    }
}

#[test]
fn a_registered_driver_reports_exactly_the_enforcement_its_engine_declares() {
    for engine in ENGINES {
        let Some(driver) = for_engine(engine) else {
            continue;
        };
        assert_eq!(
            driver.read_only_enforcement(false),
            declared_read_only_enforcement(engine),
            "{engine:?} driver and the declared policy have drifted apart"
        );
        assert_eq!(
            driver.read_only_enforcement(true),
            ReadOnlyEnforcement::NotEnforced,
            "{engine:?} claims enforcement while writes are allowed"
        );
    }
}

#[test]
fn sqlite_has_a_driver_in_this_build() {
    let d = for_engine(SqlEngine::Sqlite).expect("SQLite has shipped since phase 1");
    assert_eq!(d.engine(), SqlEngine::Sqlite);
}
