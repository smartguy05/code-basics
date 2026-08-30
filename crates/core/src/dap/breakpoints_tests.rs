//! Tests for the breakpoint model. Included by `breakpoints.rs`.

use super::*;

fn verified(line: u32) -> serde_json::Value {
    serde_json::json!({ "id": 1, "verified": true, "line": line })
}

// ---------------------------------------------------------------------------
// BindState — the distinction the module exists for
// ---------------------------------------------------------------------------

#[test]
fn a_verified_breakpoint_is_verified() {
    assert_eq!(
        BindState::from_body(&serde_json::json!({ "verified": true })),
        BindState::Verified
    );
}

#[test]
fn unverified_with_no_message_is_pending_not_rejected() {
    // Normal before the module containing the line is loaded. Drawing it as
    // rejected would send the user looking for a problem that does not exist.
    assert_eq!(
        BindState::from_body(&serde_json::json!({ "verified": false })),
        BindState::Pending
    );
    assert_eq!(
        BindState::from_body(&serde_json::json!({})),
        BindState::Pending,
        "an absent `verified` is not a refusal either"
    );
}

#[test]
fn unverified_with_a_message_is_a_refusal_carrying_its_reason() {
    // The adapter answered and said no. This will not resolve itself.
    assert_eq!(
        BindState::from_body(&serde_json::json!({
            "verified": false,
            "message": "No symbols have been loaded for this document.",
        })),
        BindState::Rejected {
            reason: Some("No symbols have been loaded for this document.".into()),
        }
    );
}

#[test]
fn an_empty_message_is_not_treated_as_a_reason() {
    assert_eq!(
        BindState::from_body(&serde_json::json!({ "verified": false, "message": "  " })),
        BindState::Pending
    );
}

#[test]
fn only_verified_counts_as_verified() {
    assert!(BindState::Verified.is_verified());
    assert!(!BindState::Pending.is_verified());
    assert!(!BindState::Rejected { reason: None }.is_verified());
}

// ---------------------------------------------------------------------------
// Breakpoint
// ---------------------------------------------------------------------------

#[test]
fn a_new_breakpoint_is_pending_not_verified() {
    // With nothing running, nothing has agreed to anything.
    let breakpoint = Breakpoint::new("src/Program.cs", 12);

    assert_eq!(breakpoint.state, BindState::Pending);
    assert_eq!(breakpoint.id, None);
    assert_eq!(breakpoint.line, 12);
    assert_eq!(breakpoint.actual_line, None);
}

#[test]
fn line_zero_is_impossible_and_becomes_line_one() {
    assert_eq!(Breakpoint::new("a.cs", 0).line, 1);
}

#[test]
fn applying_a_response_records_the_adapters_id_and_binding() {
    let mut breakpoint = Breakpoint::new("a.cs", 12);
    breakpoint.apply(&serde_json::json!({ "id": 7, "verified": true, "line": 12 }));

    assert_eq!(breakpoint.id, Some(7));
    assert_eq!(breakpoint.state, BindState::Verified);
}

#[test]
fn the_requested_line_is_never_overwritten_by_the_adapter() {
    // The user's click is a fact about the user, not about the debuggee.
    let mut breakpoint = Breakpoint::new("a.cs", 12);
    breakpoint.apply(&verified(14));

    assert_eq!(breakpoint.line, 12, "where the user clicked");
    assert_eq!(breakpoint.actual_line, Some(14), "where it really went");
    assert_eq!(breakpoint.effective_line(), 14);
}

#[test]
fn an_adapter_agreeing_records_no_adjustment() {
    // Storing the reported line unconditionally would make every breakpoint
    // look as though it had been moved.
    let mut breakpoint = Breakpoint::new("a.cs", 12);
    breakpoint.apply(&verified(12));

    assert_eq!(breakpoint.actual_line, None);
    assert_eq!(breakpoint.effective_line(), 12);
}

#[test]
fn a_column_is_kept_when_reported_and_absent_when_not() {
    let mut breakpoint = Breakpoint::new("a.cs", 12);
    breakpoint.apply(&serde_json::json!({ "verified": true, "line": 12, "column": 9 }));
    assert_eq!(breakpoint.actual_column, Some(9));

    let mut other = Breakpoint::new("a.cs", 12);
    other.apply(&verified(12));
    assert_eq!(other.actual_column, None);
}

#[test]
fn detaching_forgets_only_what_the_adapter_knew() {
    let mut breakpoint = Breakpoint::new("a.cs", 12);
    breakpoint.apply(&verified(14));

    breakpoint.detach();

    assert_eq!(breakpoint.line, 12, "the user's breakpoint survives");
    assert_eq!(breakpoint.path, "a.cs");
    assert_eq!(breakpoint.id, None);
    assert_eq!(breakpoint.state, BindState::Pending);
    assert_eq!(breakpoint.actual_line, None);
}

// ---------------------------------------------------------------------------
// BreakpointSet
// ---------------------------------------------------------------------------

#[test]
fn toggling_sets_then_clears_a_line() {
    let mut set = BreakpointSet::default();

    assert!(set.toggle("a.cs", 5), "now set");
    assert_eq!(set.lines_in_file("a.cs"), vec![5]);

    assert!(!set.toggle("a.cs", 5), "now cleared");
    assert!(set.in_file("a.cs").is_empty());
}

#[test]
fn a_file_with_no_breakpoints_left_stops_being_listed() {
    // Otherwise `setBreakpoints` would never be sent to clear it.
    let mut set = BreakpointSet::default();
    set.toggle("a.cs", 5);
    set.toggle("a.cs", 5);

    assert!(set.files().is_empty());
    assert!(set.is_empty());
}

#[test]
fn lines_are_kept_in_order_so_the_response_can_be_zipped_back() {
    let mut set = BreakpointSet::default();
    set.toggle("a.cs", 40);
    set.toggle("a.cs", 12);
    set.toggle("a.cs", 25);

    assert_eq!(set.lines_in_file("a.cs"), vec![12, 25, 40]);
}

#[test]
fn files_are_separate_and_listed_in_a_stable_order() {
    let mut set = BreakpointSet::default();
    set.toggle("z.cs", 1);
    set.toggle("a.cs", 1);

    assert_eq!(set.files(), vec!["a.cs".to_string(), "z.cs".to_string()]);
    assert_eq!(set.total(), 2);
}

#[test]
fn an_unknown_file_reads_as_empty_rather_than_failing() {
    let set = BreakpointSet::default();
    assert!(set.in_file("never.cs").is_empty());
    assert!(set.lines_in_file("never.cs").is_empty());
}

#[test]
fn a_response_is_applied_by_position_because_that_is_what_the_spec_promises() {
    let mut set = BreakpointSet::default();
    set.toggle("a.cs", 12);
    set.toggle("a.cs", 40);

    let applied = set.apply_response(
        "a.cs",
        &[
            serde_json::json!({ "id": 1, "verified": true, "line": 12 }),
            serde_json::json!({ "id": 2, "verified": false, "message": "no symbols" }),
        ],
    );

    assert_eq!(applied, 2);
    let entries = set.in_file("a.cs");
    assert_eq!(entries[0].id, Some(1));
    assert!(entries[0].state.is_verified());
    assert_eq!(entries[1].id, Some(2));
    assert!(matches!(entries[1].state, BindState::Rejected { .. }));
}

#[test]
fn a_short_response_applies_its_prefix_and_reports_how_far_it_got() {
    // Zipping past a length mismatch is how a breakpoint ends up wearing
    // another breakpoint's binding.
    let mut set = BreakpointSet::default();
    set.toggle("a.cs", 12);
    set.toggle("a.cs", 40);

    let applied = set.apply_response("a.cs", &[verified(12)]);

    assert_eq!(applied, 1, "the caller can say what happened");
    assert!(set.in_file("a.cs")[0].state.is_verified());
    assert_eq!(
        set.in_file("a.cs")[1].state,
        BindState::Pending,
        "the unanswered one is untouched, not given the other's binding"
    );
}

#[test]
fn a_response_for_a_file_with_no_breakpoints_applies_nothing() {
    let mut set = BreakpointSet::default();
    assert_eq!(set.apply_response("a.cs", &[verified(1)]), 0);
}

#[test]
fn a_breakpoint_event_finds_its_target_by_the_adapters_id() {
    let mut set = BreakpointSet::default();
    set.toggle("a.cs", 12);
    set.toggle("b.cs", 5);
    set.apply_response("a.cs", &[serde_json::json!({ "id": 9, "verified": false })]);

    // The module loaded; the adapter now binds it, two lines down.
    let applied = set.apply_event(&serde_json::json!({ "id": 9, "verified": true, "line": 14 }));

    assert!(applied);
    assert!(set.in_file("a.cs")[0].state.is_verified());
    assert_eq!(set.in_file("a.cs")[0].actual_line, Some(14));
    assert_eq!(set.in_file("b.cs")[0].state, BindState::Pending);
}

#[test]
fn an_event_about_a_breakpoint_we_no_longer_have_is_reported_not_ignored() {
    // Happens when one is removed while the event is in flight.
    let mut set = BreakpointSet::default();
    set.toggle("a.cs", 12);

    assert!(!set.apply_event(&serde_json::json!({ "id": 404, "verified": true })));
}

#[test]
fn an_event_with_no_id_matches_nothing() {
    let mut set = BreakpointSet::default();
    set.toggle("a.cs", 12);

    assert!(!set.apply_event(&serde_json::json!({ "verified": true })));
}

#[test]
fn ending_a_session_keeps_the_users_breakpoints_and_drops_the_adapters_facts() {
    let mut set = BreakpointSet::default();
    set.toggle("a.cs", 12);
    set.apply_response("a.cs", &[verified(14)]);

    set.detach_all();

    let entries = set.in_file("a.cs");
    assert_eq!(entries.len(), 1, "the user's breakpoint is still there");
    assert_eq!(entries[0].line, 12);
    assert_eq!(
        entries[0].state,
        BindState::Pending,
        "nothing has verified it in the next session yet"
    );
    assert_eq!(entries[0].actual_line, None);
    assert_eq!(entries[0].id, None);
}
