use super::*;

const C: &str = "conn-1";

#[test]
fn a_fresh_registry_knows_about_nothing() {
    let s = SqlSessions::new();
    assert!(s.is_empty());
    assert_eq!(s.connections(), Vec::<String>::new());
    assert_eq!(s.queries(C), Vec::<String>::new());
    assert!(!s.is_running(C, "q1"));
}

#[test]
fn a_registered_query_hands_back_an_unsignalled_stop_signal() {
    let s = SqlSessions::new();
    let rx = s.register(C, "q1").expect("first registration");
    assert!(!*rx.borrow(), "a statement must not start already stopping");
    assert!(s.is_running(C, "q1"));
    assert_eq!(s.queries(C), vec!["q1".to_string()]);
    assert_eq!(s.connections(), vec![C.to_string()]);
    assert_eq!(s.len(), 1);
}

#[test]
fn a_duplicate_query_id_is_refused_rather_than_replacing_the_live_handle() {
    // Replacing would leave the running statement with a handle nobody holds,
    // so it could never be stopped — a failure with no symptom until someone
    // presses Stop.
    let s = SqlSessions::new();
    let first = s.register(C, "q1").unwrap();
    assert_eq!(s.register(C, "q1").err(), Some(RegisterError::Duplicate));
    assert_eq!(s.stop(C, "q1"), StopOutcome::Signalled);
    assert!(*first.borrow(), "the original handle is still the live one");
}

#[test]
fn the_same_query_id_on_another_connection_is_a_different_statement() {
    let s = SqlSessions::new();
    let a = s.register("conn-a", "q1").unwrap();
    let b = s.register("conn-b", "q1").unwrap();
    assert_eq!(s.stop("conn-a", "q1"), StopOutcome::Signalled);
    assert!(*a.borrow());
    assert!(!*b.borrow(), "the other connection was untouched");
    assert_eq!(s.connections(), vec!["conn-a", "conn-b"]);
}

// ---------------------------------------------------------------------------
// Stopping
// ---------------------------------------------------------------------------

#[test]
fn stopping_signals_the_row_loop() {
    let s = SqlSessions::new();
    let rx = s.register(C, "q1").unwrap();
    assert_eq!(s.stop(C, "q1"), StopOutcome::Signalled);
    assert!(*rx.borrow());
}

#[test]
fn stopping_twice_says_so_rather_than_reporting_a_second_success() {
    let s = SqlSessions::new();
    let _rx = s.register(C, "q1").unwrap();
    assert_eq!(s.stop(C, "q1"), StopOutcome::Signalled);
    assert_eq!(s.stop(C, "q1"), StopOutcome::AlreadyStopping);
}

#[test]
fn stopping_a_query_that_is_not_registered_is_not_a_success() {
    let s = SqlSessions::new();
    assert_eq!(s.stop(C, "q1"), StopOutcome::NotFound);
    let _rx = s.register(C, "q1").unwrap();
    assert_eq!(s.stop(C, "q2"), StopOutcome::NotFound);
    assert_eq!(s.stop("other", "q1"), StopOutcome::NotFound);
}

#[test]
fn a_stop_does_not_deregister_the_query() {
    // Removal is the runner's job. If `stop` removed the entry, a second Stop
    // click would report NotFound for a statement still visibly running.
    let s = SqlSessions::new();
    let _rx = s.register(C, "q1").unwrap();
    s.stop(C, "q1");
    assert!(s.is_running(C, "q1"));
}

#[test]
fn a_query_whose_row_loop_has_already_gone_reports_not_found_not_signalled() {
    let s = SqlSessions::new();
    let rx = s.register(C, "q1").unwrap();
    drop(rx);
    assert_eq!(
        s.stop(C, "q1"),
        StopOutcome::NotFound,
        "nothing is listening, so nothing was stopped"
    );
}

#[test]
fn stopping_a_connection_reports_every_statement_it_signalled() {
    let s = SqlSessions::new();
    let a = s.register(C, "q1").unwrap();
    let b = s.register(C, "q2").unwrap();
    let _other = s.register("conn-2", "q9").unwrap();

    assert_eq!(
        s.stop_connection(C),
        vec![
            ("q1".to_string(), StopOutcome::Signalled),
            ("q2".to_string(), StopOutcome::Signalled),
        ]
    );
    assert!(*a.borrow() && *b.borrow());
    assert_eq!(
        s.queries("conn-2"),
        vec!["q9".to_string()],
        "another connection is untouched"
    );
}

#[test]
fn stopping_a_connection_with_nothing_running_signals_nothing() {
    let s = SqlSessions::new();
    assert_eq!(s.stop_connection(C), Vec::new());
}

#[test]
fn stopping_a_connection_reports_the_ones_already_stopping_separately() {
    let s = SqlSessions::new();
    let _a = s.register(C, "q1").unwrap();
    let _b = s.register(C, "q2").unwrap();
    s.stop(C, "q1");
    assert_eq!(
        s.stop_connection(C),
        vec![
            ("q1".to_string(), StopOutcome::AlreadyStopping),
            ("q2".to_string(), StopOutcome::Signalled),
        ]
    );
}

// ---------------------------------------------------------------------------
// Finishing
// ---------------------------------------------------------------------------

#[test]
fn finishing_removes_the_query_and_says_whether_it_was_there() {
    let s = SqlSessions::new();
    let _rx = s.register(C, "q1").unwrap();
    assert!(s.finish(C, "q1"));
    assert!(!s.is_running(C, "q1"));
    assert!(!s.finish(C, "q1"), "the second finish removed nothing");
    assert!(!s.finish("never-seen", "q1"));
}

#[test]
fn the_last_query_leaving_removes_the_connection_from_the_listing() {
    // Otherwise `connections()` grows for the life of the process and reports
    // connections with nothing running as if they had work in flight.
    let s = SqlSessions::new();
    let _rx = s.register(C, "q1").unwrap();
    s.finish(C, "q1");
    assert_eq!(s.connections(), Vec::<String>::new());
    assert!(s.is_empty());
}

#[test]
fn finishing_one_query_leaves_its_siblings_alone() {
    let s = SqlSessions::new();
    let _a = s.register(C, "q1").unwrap();
    let b = s.register(C, "q2").unwrap();
    s.finish(C, "q1");
    assert_eq!(s.queries(C), vec!["q2".to_string()]);
    assert_eq!(s.stop(C, "q2"), StopOutcome::Signalled);
    assert!(*b.borrow());
}

#[test]
fn a_finished_id_can_be_registered_again() {
    let s = SqlSessions::new();
    let _a = s.register(C, "q1").unwrap();
    s.finish(C, "q1");
    assert!(s.register(C, "q1").is_ok());
}

// ---------------------------------------------------------------------------
// The handle is shared, not copied
// ---------------------------------------------------------------------------

#[test]
fn a_clone_of_the_registry_sees_the_same_statements() {
    let s = SqlSessions::new();
    let clone = s.clone();
    let rx = s.register(C, "q1").unwrap();
    assert!(clone.is_running(C, "q1"));
    assert_eq!(clone.stop(C, "q1"), StopOutcome::Signalled);
    assert!(*rx.borrow());
    clone.finish(C, "q1");
    assert!(s.is_empty());
}
