use super::*;
use crate::sql::guard::{self, HEURISTIC_NOTE};

/// One of every refusal, built from inputs a driver message could plausibly
/// have reached. Used by the tests that must hold for the whole enum.
fn every_refusal() -> Vec<McpRefusal> {
    let mut all = vec![
        McpRefusal::NoConnectionsSaved,
        McpRefusal::NoConnectionExposed { total_saved: 3 },
        McpRefusal::ConnectionNotFound {
            selector: "reporting".to_string(),
        },
        McpRefusal::ConnectionNotExposed {
            selector: "reporting".to_string(),
        },
        McpRefusal::ConnectionAmbiguous {
            selector: "reporting".to_string(),
            candidates: vec!["a".to_string(), "b".to_string()],
        },
        McpRefusal::EngineUndetermined {
            connection: "conn-1".to_string(),
        },
        McpRefusal::EngineUnsupported {
            engine: SqlEngine::Postgres,
        },
        McpRefusal::SecretUnresolved {
            connection: "conn-1".to_string(),
        },
        McpRefusal::GuardRefused {
            reason: "This could not be parsed.".to_string(),
        },
        McpRefusal::WriteRefusedUnconditionally {
            kind: "INSERT".to_string(),
            reason: AGENT_NOTE.to_string(),
        },
        McpRefusal::IdentifierRefused {
            refusal: IdentifierRefusal::Quote,
        },
        McpRefusal::CatalogRowUnreadable {
            detail: "no column named table_name".to_string(),
        },
        McpRefusal::CatalogTruncated {
            cap: RowCap {
                limit: 500,
                reason: RowCapReason::RowLimit,
            },
        },
        McpRefusal::CatalogTruncated {
            cap: RowCap {
                limit: 500,
                reason: RowCapReason::ByteLimit,
            },
        },
    ];
    for kind in ConnectionStatusKind::ALL {
        all.push(connect_failed(kind));
    }
    for stage in [
        ErrorStage::Connect,
        ErrorStage::ConnectTimeout,
        ErrorStage::Execute,
    ] {
        all.push(statement_failed(&DriverError::new(stage, "anything")));
    }
    all
}

#[test]
fn every_refusal_says_something() {
    for refusal in every_refusal() {
        let sentence = refusal.sentence();
        assert!(
            sentence.len() > 20,
            "{:?} rendered as {sentence:?}",
            refusal.code()
        );
        assert!(!refusal.code().is_empty());
    }
}

#[test]
fn no_refusal_forwards_driver_text() {
    // The sentinel is what a real driver message routinely embeds: the
    // connection string, password and all. `dsn::redact` is a keyword
    // heuristic, and this surface does not rely on it — the words are dropped,
    // not filtered.
    const SENTINEL: &str = "Server=db;User Id=sa;Password=hunter2-SENTINEL";

    let connect = statement_failed(&DriverError::connect(SENTINEL));
    let execute = statement_failed(&DriverError::execute(SENTINEL));
    for refusal in [connect, execute] {
        assert!(
            !refusal.sentence().contains("SENTINEL"),
            "{:?} forwarded driver text: {}",
            refusal.code(),
            refusal.sentence()
        );
    }

    // Structural, not textual: neither failure variant has anywhere to put a
    // message, so no future call site can decide to pass one along.
    match statement_failed(&DriverError::execute(SENTINEL)) {
        McpRefusal::StatementFailed { stage } => assert_eq!(stage, ErrorStage::Execute),
        other => panic!("expected StatementFailed, got {other:?}"),
    }
    match connect_failed(ConnectionStatusKind::AuthFailed) {
        McpRefusal::ConnectFailed { kind } => assert_eq!(kind, ConnectionStatusKind::AuthFailed),
        other => panic!("expected ConnectFailed, got {other:?}"),
    }
}

#[test]
fn no_refusal_repeats_the_consoles_advice_to_enable_writes() {
    for refusal in every_refusal() {
        assert!(
            !refusal
                .sentence()
                .contains("enable writes for this connection"),
            "{} repeated the console's advice, which is false on this path",
            refusal.code()
        );
    }
}

#[test]
fn no_connection_exposed_and_no_connection_saved_are_different_answers() {
    let saved = McpRefusal::NoConnectionsSaved;
    let exposed = McpRefusal::NoConnectionExposed { total_saved: 4 };
    assert_ne!(saved.code(), exposed.code());
    assert_ne!(saved.sentence(), exposed.sentence());
    assert!(exposed.sentence().contains('4'));
    assert!(
        !saved.sentence().contains("exposed"),
        "the empty-store answer must not send the reader to a toggle that would change nothing"
    );
}

#[test]
fn not_exposed_and_not_found_are_different_answers() {
    let missing = McpRefusal::ConnectionNotFound {
        selector: "billing".to_string(),
    };
    let hidden = McpRefusal::ConnectionNotExposed {
        selector: "billing".to_string(),
    };
    assert_ne!(missing.code(), hidden.code());
    assert_ne!(missing.sentence(), hidden.sentence());
}

#[test]
fn an_ambiguous_selector_names_the_candidates_and_never_picks() {
    let refusal = McpRefusal::ConnectionAmbiguous {
        selector: "prod".to_string(),
        candidates: vec!["conn-1".to_string(), "conn-2".to_string()],
    };
    let sentence = refusal.sentence();
    assert!(sentence.contains("conn-1") && sentence.contains("conn-2"));
    assert!(sentence.contains("refused"));
}

#[test]
fn a_row_cap_and_a_byte_cap_are_different_answers() {
    // Raising the row limit would not return more rows, so the two must not
    // render the same sentence.
    let rows = McpRefusal::CatalogTruncated {
        cap: RowCap {
            limit: 200,
            reason: RowCapReason::RowLimit,
        },
    };
    let bytes = McpRefusal::CatalogTruncated {
        cap: RowCap {
            limit: 200,
            reason: RowCapReason::ByteLimit,
        },
    };
    assert_ne!(rows.sentence(), bytes.sentence());
    assert!(bytes
        .sentence()
        .contains("Raising the row limit would not return more rows"));
    assert!(!rows
        .sentence()
        .contains("Raising the row limit would not return more rows"));
}

#[test]
fn a_truncated_catalog_is_not_reported_as_a_short_list() {
    let refusal = McpRefusal::CatalogTruncated {
        cap: RowCap {
            limit: 200,
            reason: RowCapReason::RowLimit,
        },
    };
    assert!(refusal.sentence().contains("incomplete"));
}

#[test]
fn every_connection_status_kind_has_its_own_name_and_detail() {
    let mut names: Vec<&str> = ConnectionStatusKind::ALL.iter().map(|k| k.name()).collect();
    names.sort_unstable();
    let unique = {
        let mut n = names.clone();
        n.dedup();
        n
    };
    assert_eq!(names, unique, "two kinds share a wire name");

    for kind in ConnectionStatusKind::ALL {
        assert!(!kind.detail().is_empty(), "{} has no detail", kind.name());
    }
    assert_ne!(
        ConnectionStatusKind::Unreachable.detail(),
        ConnectionStatusKind::CannotOpenFile.detail(),
        "a network answer and a file answer must not read the same"
    );
}

#[test]
fn restating_a_guard_sentence_keeps_the_finding_and_replaces_the_advice() {
    // The guard's own sentence for a real refusal, so this cannot drift from
    // what `guard::guard` actually produces.
    let decision = guard::guard("DROP TABLE orders", guard::Engine::Postgres, false);
    let console = decision.message.expect("a refusal carries a sentence");
    assert!(
        console.contains("enable writes for this connection"),
        "the console sentence this test is about has changed: {console}"
    );

    let agent = restate_for_agent(&console);
    assert!(
        !agent.contains("enable writes for this connection"),
        "the agent sentence must not advise a switch that cannot help: {agent}"
    );
    assert!(agent.contains(AGENT_NOTE));
    assert!(
        agent.starts_with(console.strip_suffix(HEURISTIC_NOTE).unwrap().trim_end()),
        "the finding is the guard's own words and must survive: {agent}"
    );
}

#[test]
fn a_guard_sentence_without_the_console_note_keeps_all_of_itself() {
    let agent = restate_for_agent("Something new the guard says.");
    assert!(agent.starts_with("Something new the guard says."));
    assert!(agent.contains(AGENT_NOTE));
}

#[test]
fn the_agent_note_says_the_setting_would_not_help() {
    assert!(AGENT_NOTE.contains("would not"));
    assert!(!AGENT_NOTE.contains("enable writes for this connection"));
}
