use super::*;

use serde_json::json;

fn ready() -> AgentStatus {
    AgentStatus {
        availability: BrowserAvailability::Ready,
        url: Some("https://app.example.com/orders".to_string()),
        title: Some("Orders".to_string()),
        origin: Some("https://app.example.com".to_string()),
        reads_allowed: false,
        writes_allowed: false,
        consent_origin: None,
        refused_navigations: 0,
        rejected_messages: 0,
        last_refusal: None,
        pid: 4242,
    }
}

// ---------------------------------------------------------------------------
// status: the six states, and the consent position stated always
// ---------------------------------------------------------------------------

#[test]
fn every_availability_answers_with_its_own_cause() {
    // The whole point of `browser_status`. A shared "unavailable" would pass
    // every other test in this file.
    let mut sentences = std::collections::BTreeSet::new();
    for availability in [
        BrowserAvailability::PluginDisabled,
        BrowserAvailability::PanelClosed,
        BrowserAvailability::Blank,
        BrowserAvailability::Loading,
        BrowserAvailability::Ready,
        BrowserAvailability::Failed,
    ] {
        let answer = status(&AgentStatus {
            availability,
            ..ready()
        });
        assert!(
            answer.contains(availability.reason()),
            "{availability:?}: {answer}"
        );
        sentences.insert(answer);
    }
    assert_eq!(sentences.len(), 6);
}

#[test]
fn the_answer_names_the_process_so_an_ambiguity_can_be_resolved() {
    // An agent that hit `InstanceError::Ambiguous` is told to pass
    // --instance <pid>; the pid has to come from somewhere.
    assert!(status(&ready()).contains("4242"));
}

#[test]
fn no_consent_is_stated_and_names_the_control_the_user_would_click() {
    let answer = status(&ready());
    assert!(answer.contains("Permission: none"), "{answer}");
    assert!(
        answer.contains(crate::browser::consent::READ_CONSENT_ACTION),
        "the answer must quote the panel's own label, not a paraphrase: {answer}"
    );
    assert!(answer.contains("do not retry"), "{answer}");
}

#[test]
fn read_only_consent_says_which_tools_are_still_refused() {
    let answer = status(&AgentStatus {
        reads_allowed: true,
        consent_origin: Some("https://app.example.com".to_string()),
        ..ready()
    });
    assert!(answer.contains("read only"), "{answer}");
    assert!(answer.contains("navigate"), "{answer}");
    assert!(
        answer.contains(crate::browser::consent::WRITE_CONSENT_ACTION),
        "{answer}"
    );
}

#[test]
fn full_consent_says_both_categories_will_run() {
    let answer = status(&AgentStatus {
        reads_allowed: true,
        writes_allowed: true,
        consent_origin: Some("https://app.example.com".to_string()),
        ..ready()
    });
    assert!(answer.contains("read and control"), "{answer}");
}

#[test]
fn consent_granted_for_another_page_is_reported_as_exactly_that() {
    // The case that would otherwise read as a plain grant, and then every read
    // would be refused for reasons the agent cannot see.
    let answer = status(&AgentStatus {
        reads_allowed: true,
        writes_allowed: true,
        consent_origin: Some("https://other.example.com".to_string()),
        ..ready()
    });
    assert!(answer.contains("https://other.example.com"), "{answer}");
    assert!(answer.contains("https://app.example.com"), "{answer}");
    assert!(answer.contains("did not carry over"), "{answer}");
}

#[test]
fn read_consent_with_no_page_named_is_refused_and_reported_as_a_bug() {
    // Unrepresentable in `AutomationConsent`; treating it as a grant would
    // make one grant cover every page, so it is not assumed away.
    let answer = status(&AgentStatus {
        reads_allowed: true,
        consent_origin: None,
        ..ready()
    });
    assert!(answer.contains("no page named"), "{answer}");
    assert!(answer.contains("refused"), "{answer}");
}

#[test]
fn a_panel_with_no_page_says_permission_cannot_be_granted_for_it() {
    // `about:blank` has no origin, so no grant can name it — which is why
    // navigate is unusable from a blank panel.
    let answer = status(&AgentStatus {
        availability: BrowserAvailability::Blank,
        url: None,
        title: None,
        origin: None,
        ..ready()
    });
    assert!(answer.contains("no page address"), "{answer}");
    assert!(answer.contains("per page"), "{answer}");
}

#[test]
fn the_refusal_counters_are_reported_only_when_they_are_not_zero() {
    let quiet = status(&ready());
    assert!(
        !quiet.contains("navigation(s) have been refused"),
        "{quiet}"
    );
    let noisy = status(&AgentStatus {
        refused_navigations: 3,
        rejected_messages: 2,
        last_refusal: Some("tauri://localhost is this application's own origin".to_string()),
        ..ready()
    });
    assert!(noisy.contains("3 navigation(s)"), "{noisy}");
    assert!(noisy.contains("2 message(s)"), "{noisy}");
    assert!(noisy.contains("tauri://localhost"), "{noisy}");
}

// ---------------------------------------------------------------------------
// current_url
// ---------------------------------------------------------------------------

#[test]
fn a_page_with_no_title_says_so_rather_than_showing_an_empty_one() {
    let answer = current_url(Some("https://example.com"), None);
    assert!(answer.contains("has not set one"), "{answer}");
    let empty = current_url(Some("https://example.com"), Some(""));
    assert!(empty.contains("has not set one"), "{empty}");
}

#[test]
fn no_url_is_its_own_answer() {
    assert!(current_url(None, None).contains("no page address"));
}

// ---------------------------------------------------------------------------
// page_text: a cut is never quotable as the whole page
// ---------------------------------------------------------------------------

#[test]
fn untruncated_text_says_nothing_was_cut() {
    let answer = page_text(&PageText {
        text: "hello".to_string(),
        total_chars: 5,
        returned_chars: 5,
        truncated: false,
    });
    assert!(answer.contains("nothing was cut"), "{answer}");
    assert!(answer.ends_with("hello"), "{answer}");
}

#[test]
fn truncated_text_reports_the_pages_real_length_and_says_it_is_cut() {
    let answer = page_text(&PageText {
        text: "abc".to_string(),
        total_chars: 90_000,
        returned_chars: 3,
        truncated: true,
    });
    assert!(answer.contains("90000"), "{answer}");
    assert!(answer.contains("CUT"), "{answer}");
    assert!(
        answer.contains("do not treat what follows as the whole page"),
        "{answer}"
    );
}

#[test]
fn a_page_that_rendered_no_text_says_that_is_the_page_and_not_a_failure() {
    // The distinction that matters: a canvas application legitimately has no
    // text, and reporting it as a read failure sends the agent retrying.
    let answer = page_text(&PageText {
        text: "   ".to_string(),
        total_chars: 3,
        returned_chars: 3,
        truncated: false,
    });
    assert!(answer.contains("rendered no text"), "{answer}");
    assert!(
        answer.contains("not a \nfailure") || answer.contains("not a failure"),
        "{answer}"
    );
}

// ---------------------------------------------------------------------------
// outline
// ---------------------------------------------------------------------------

fn element(reference: &str, role: &str, name: &str) -> Value {
    json!({ "ref": reference, "role": role, "name": name, "type": null, "disabled": false })
}

#[test]
fn an_outline_lists_the_refs_click_and_type_address() {
    let answer = outline(&json!({
        "ok": true,
        "value": [element("e1", "button", "Save"), element("e2", "a", "Home")],
        "total": 2,
        "truncated": false
    }))
    .unwrap();
    assert!(answer.contains("e1"), "{answer}");
    assert!(answer.contains("button"), "{answer}");
    assert!(answer.contains("Save"), "{answer}");
    assert!(answer.contains("2 elements"), "{answer}");
}

#[test]
fn a_cut_outline_says_the_element_you_need_may_be_missing() {
    let answer = outline(&json!({
        "ok": true,
        "value": [element("e1", "button", "Save")],
        "total": 400,
        "truncated": true
    }))
    .unwrap();
    assert!(answer.contains("400"), "{answer}");
    assert!(answer.contains("CUT"), "{answer}");
    assert!(answer.contains("may be among them"), "{answer}");
}

#[test]
fn an_outline_that_only_filtered_non_rendered_elements_is_not_reported_as_cut() {
    // Two different facts: the cap cut the list, and some matches are not
    // rendered so they were left out. A page with a few `display:none` menu
    // items and two hidden inputs hits the second and never the first, and
    // "the list is CUT ... narrow the page" over a complete outline teaches an
    // agent to distrust every answer.
    let answer = outline(&json!({
        "ok": true,
        "value": [element("e1", "button", "Save")],
        "total": 9,
        "truncated": false
    }))
    .unwrap();
    assert!(!answer.contains("CUT"), "{answer}");
    assert!(answer.contains("1 element"), "{answer}");
    assert!(
        answer.contains("8") && answer.contains("not rendered"),
        "the filtered matches should be stated: {answer}"
    );
}

#[test]
fn an_outline_says_it_cannot_see_into_a_frame() {
    // `document.querySelectorAll` is the top document's. Content inside any
    // nested document was never in scope, so a count with no scope clause is a
    // completeness claim the read cannot support.
    let answer = outline(&json!({
        "ok": true,
        "value": [element("e1", "button", "Save")],
        "total": 1,
        "truncated": false
    }))
    .unwrap();
    assert!(answer.contains("frame"), "{answer}");
}

#[test]
fn an_empty_outline_is_a_finding_about_the_page_not_a_failure() {
    let answer =
        outline(&json!({ "ok": true, "value": [], "total": 0, "truncated": false })).unwrap();
    assert!(answer.contains("No interactive"), "{answer}");
    assert!(answer.contains("legitimately"), "{answer}");
}

#[test]
fn an_unnamed_control_is_said_to_be_unnamed_rather_than_rendered_blank() {
    let answer = outline(&json!({
        "ok": true,
        "value": [element("e1", "button", "")],
        "total": 1,
        "truncated": false
    }))
    .unwrap();
    assert!(answer.contains("no accessible name"), "{answer}");
}

#[test]
fn a_disabled_control_is_marked_so_a_click_is_not_planned_against_it() {
    let answer = outline(&json!({
        "ok": true,
        "value": [json!({ "ref": "e1", "role": "button", "name": "Save", "type": "submit", "disabled": true })],
        "total": 1,
        "truncated": false
    }))
    .unwrap();
    assert!(answer.contains("DISABLED"), "{answer}");
    assert!(answer.contains("[submit]"), "{answer}");
}

#[test]
fn an_outline_that_is_not_a_list_is_refused_rather_than_invented() {
    // A fabricated outline produces clicks on nothing, which is worse than a
    // stated failure.
    let refused = outline(&json!({ "ok": true, "value": "everything is fine" })).unwrap_err();
    assert!(refused.contains("no element references"), "{refused}");
}

// ---------------------------------------------------------------------------
// console and network: a gap is never silent
// ---------------------------------------------------------------------------

fn console_entry(seq: u64, level: ConsoleLevel, method: &str, text: &str) -> ConsoleEntry {
    ConsoleEntry {
        seq,
        level,
        method: method.to_string(),
        text: text.to_string(),
    }
}

#[test]
fn console_rows_carry_the_ranking_and_the_pages_own_method() {
    let answer = console(
        &[console_entry(4, ConsoleLevel::Other, "table", "rows")],
        0,
        0,
        4,
    );
    assert!(answer.contains("unranked(table)"), "{answer}");
    assert!(answer.contains("[4]"), "{answer}");
}

#[test]
fn a_level_matching_its_method_is_not_printed_twice() {
    let answer = console(
        &[console_entry(1, ConsoleLevel::Warn, "warn", "careful")],
        0,
        0,
        1,
    );
    assert!(answer.contains("warn: careful"), "{answer}");
    assert!(!answer.contains("warn(warn)"), "{answer}");
}

#[test]
fn an_empty_console_read_says_nothing_was_captured() {
    let answer = console(&[], 0, 0, 0);
    assert!(
        answer.contains("No console messages have been captured"),
        "{answer}"
    );
}

#[test]
fn evicted_entries_are_reported_and_never_folded_into_a_quiet_page() {
    // The failure this exists to stop: an agent reading "no messages" when the
    // buffer threw away the ones it needed concludes the page is quiet.
    let answer = console(&[], 17, 0, 17);
    assert!(answer.contains("17 earlier"), "{answer}");
    assert!(answer.contains("not a quiet page"), "{answer}");
    assert!(answer.contains("partial record"), "{answer}");
}

#[test]
fn entries_discarded_with_a_previous_page_are_not_called_buffer_evictions() {
    // "Cleared because the page it belonged to is gone" and "evicted because
    // the buffer overflowed" are two claims, and only the second says the
    // record in front of the reader is partial. Merged, a complete record of a
    // quiet page was reported as a partial record of a noisy one.
    let answer = console(&[], 0, 40, 42);
    assert!(answer.contains("40"), "{answer}");
    assert!(
        answer.contains("no longer open"),
        "the page boundary should be named: {answer}"
    );
    assert!(
        !answer.contains("not a quiet page"),
        "nothing was evicted from this page's log: {answer}"
    );
}

#[test]
fn the_two_gaps_are_reported_separately_when_both_happened() {
    let answer = console(&[], 5, 40, 60);
    assert!(answer.contains("40"), "{answer}");
    assert!(answer.contains("no longer open"), "{answer}");
    assert!(answer.contains("5 earlier"), "{answer}");
    assert!(answer.contains("not a quiet page"), "{answer}");
}

#[test]
fn every_read_states_the_cursor_to_pass_next() {
    assert!(console(&[], 0, 0, 9).contains("since=9"));
    assert!(network(&[], 0, 0, 12).contains("since=12"));
}

fn network_entry(seq: u64, status: Option<u16>, source: NetworkSource) -> NetworkEntry {
    NetworkEntry {
        seq,
        url: "https://api.example.com/orders".to_string(),
        method: "GET".to_string(),
        status,
        duration_ms: Some(12.4),
        transfer_size: Some(2048),
        source,
    }
}

#[test]
fn a_zero_status_from_a_patched_fetch_is_an_opaque_response_not_a_failure() {
    // The only producer of `Some(0)` is the fetch *fulfilled* handler, so this
    // request completed. Calling it a failure reports a working `no-cors`
    // beacon as broken, and there is no other value it could have arrived as.
    let answer = network(&[network_entry(1, Some(0), NetworkSource::Fetch)], 0, 0, 1);
    assert!(answer.contains("opaque"), "{answer}");
    assert!(
        !answer.contains("failed before a response"),
        "a completed opaque response must not be reported as a failure: {answer}"
    );
}

#[test]
fn an_errored_xhr_row_is_a_failure_and_not_a_resource_timing_limitation() {
    let answer = network(&[network_entry(3, None, NetworkSource::Xhr)], 0, 0, 3);
    let row = answer.lines().find(|l| l.starts_with("[3]")).unwrap();
    assert!(row.contains("failed before a response"), "{row}");
    assert!(!row.contains("resource timing"), "{row}");
}

#[test]
fn every_network_answer_carries_the_coverage_note_verbatim() {
    // Including the empty one — an empty list is exactly where a reader would
    // otherwise conclude no requests were made.
    for entries in [
        Vec::new(),
        vec![network_entry(1, Some(200), NetworkSource::Fetch)],
    ] {
        let answer = network(&entries, 0, 0, 1);
        assert!(answer.contains(NETWORK_COVERAGE_NOTE), "{answer}");
    }
}

#[test]
fn a_missing_status_is_read_against_the_source_that_could_not_supply_one() {
    // This test previously asserted the opposite, under the name
    // `a_zero_status_is_a_failed_request_and_not_an_unobservable_one`, and its
    // premise was wrong rather than merely differently worded: it constructed
    // the value by hand instead of taking it from `script::init_script`, which
    // is the only producer. There, a *rejected* fetch posts `status: null` and
    // an errored XHR posts `self.status || null` (so a zero status becomes
    // null too), while `0` is reachable only from the fetch **success** path,
    // `(response && response.status) || 0` - a resolved opaque (`no-cors`) or
    // opaque-redirect response. So the two labels were effectively swapped.
    // The status wording must therefore be read against `source`.
    let failed = network(&[network_entry(1, None, NetworkSource::Fetch)], 0, 0, 1);
    assert!(failed.contains("failed before a response"), "{failed}");
    // Scoped to the row: the coverage note names resource timing on every
    // answer, which is a statement about the instrumentation, not about a row.
    let row = failed.lines().find(|l| l.starts_with("[1]")).unwrap();
    assert!(
        !row.contains("resource timing"),
        "a patched fetch row must not blame resource timing: {row}"
    );
    let unknown = network(&[network_entry(2, None, NetworkSource::Resource)], 0, 0, 2);
    assert!(unknown.contains("cannot see one"), "{unknown}");
    assert_ne!(failed, unknown);
}

#[test]
fn a_resource_timing_row_says_what_it_cannot_report() {
    let answer = network(&[network_entry(1, None, NetworkSource::Resource)], 0, 0, 1);
    assert!(
        answer.contains("no method, status, headers or body"),
        "{answer}"
    );
}

#[test]
fn an_unknown_duration_or_size_is_said_to_be_unknown_rather_than_zero() {
    let answer = network(
        &[NetworkEntry {
            duration_ms: None,
            transfer_size: None,
            ..network_entry(1, Some(200), NetworkSource::Xhr)
        }],
        0,
        0,
        1,
    );
    assert!(answer.contains("duration unknown"), "{answer}");
    assert!(answer.contains("size unknown"), "{answer}");
    assert!(!answer.contains("0ms"), "{answer}");
}

// ---------------------------------------------------------------------------
// the state-changing answers claim only what they asked for
// ---------------------------------------------------------------------------

#[test]
fn a_navigation_says_it_started_and_that_consent_did_not_come_with_it() {
    let answer = navigated("https://example.com");
    assert!(answer.contains("started"), "{answer}");
    assert!(answer.contains("has not finished loading"), "{answer}");
    assert!(answer.contains("does not apply here"), "{answer}");
}

#[test]
fn a_history_step_never_claims_it_moved() {
    // wry 0.55 has no go_back; this goes through history.back(), and a page
    // with empty history does nothing and reports success.
    for direction in ["back", "forward"] {
        let answer = history_step(direction);
        assert!(answer.contains("Asked the page"), "{answer}");
        assert!(answer.contains("not observable from here"), "{answer}");
        assert!(!answer.contains("went"), "{answer}");
    }
}

#[test]
fn an_action_reports_what_the_page_said_it_acted_on() {
    let answer = acted("Clicked e1", &json!({ "clicked": "e1", "role": "button" }));
    assert!(answer.contains("Clicked e1"), "{answer}");
    assert!(answer.contains("\"clicked\":\"e1\""), "{answer}");
}
