use super::*;

use cb_core::browser::model::BrowserAvailability;
use cb_core::browser::tools;

/// A panel showing a ready page, with no consent granted — the resting state
/// every read starts from.
fn ready_page() -> BrowserShared {
    let mut data = BrowserShared::new();
    shared::note_opened(&mut data);
    shared::note_navigation(&mut data, "https://app.example.com/orders");
    shared::note_load_finished(&mut data, "https://app.example.com/orders");
    assert_eq!(data.availability, BrowserAvailability::Ready);
    data
}

fn call(name: &str) -> BrowserToolCall {
    let mut args = serde_json::Map::new();
    args.insert("url".to_string(), serde_json::json!("https://example.com"));
    args.insert("ref".to_string(), serde_json::json!("e1"));
    args.insert("text".to_string(), serde_json::json!("hello"));
    args.insert("key".to_string(), serde_json::json!("Enter"));
    tools::parse_call(name, Some(&args)).unwrap()
}

fn peer() -> Peer {
    Peer {
        pid: 12345,
        program: "codex.cmd".to_string(),
    }
}

// ---------------------------------------------------------------------------
// status_of: the projection, and the collapse it refuses
// ---------------------------------------------------------------------------

#[test]
fn the_projection_carries_the_availability_verbatim() {
    // The shortcut this guards: reporting `Ready` because there is a url would
    // describe a still-loading page and a failed navigation as readable.
    let mut data = ready_page();
    for availability in [
        BrowserAvailability::Loading,
        BrowserAvailability::Failed,
        BrowserAvailability::Blank,
        BrowserAvailability::Ready,
    ] {
        data.availability = availability;
        assert_eq!(status_of(&data, 1).availability, availability);
        assert!(status_of(&data, 1).url.is_some());
    }
}

#[test]
fn the_projection_derives_the_origin_from_the_url() {
    let status = status_of(&ready_page(), 1);
    assert_eq!(status.origin.as_deref(), Some("https://app.example.com"));
    assert_eq!(
        status.url.as_deref(),
        Some("https://app.example.com/orders")
    );
}

#[test]
fn the_projection_reports_the_consent_origin_separately_from_the_page_origin() {
    // They differ exactly when a navigation lost a grant, and that is the case
    // the status answer has to be able to describe.
    let mut data = ready_page();
    shared::grant_consent(&mut data, true, false).unwrap();
    shared::note_navigation(&mut data, "https://other.example.com/");
    shared::note_load_finished(&mut data, "https://other.example.com/");
    let status = status_of(&data, 1);
    assert_eq!(status.origin.as_deref(), Some("https://other.example.com"));
    // The grant did not survive the origin change, which is the rule under
    // test: consent is a statement about a page.
    assert!(!status.reads_allowed);
}

#[test]
fn the_projection_carries_this_process_id_so_an_ambiguity_can_be_resolved() {
    assert_eq!(status_of(&ready_page(), 4242).pid, 4242);
}

// ---------------------------------------------------------------------------
// the gate
// ---------------------------------------------------------------------------

#[test]
fn status_is_allowed_in_every_state_including_with_the_plugin_off() {
    // Gating it would make "off" indistinguishable from "refused", and it is
    // the tool that explains every other refusal.
    let mut data = BrowserShared::new();
    for availability in [
        BrowserAvailability::PluginDisabled,
        BrowserAvailability::PanelClosed,
        BrowserAvailability::Blank,
        BrowserAvailability::Loading,
        BrowserAvailability::Ready,
        BrowserAvailability::Failed,
    ] {
        data.availability = availability;
        assert!(
            gate(&BrowserToolCall::Status, &data).is_ok(),
            "{availability:?}"
        );
    }
}

#[test]
fn every_read_is_refused_with_no_consent_and_the_refusal_names_the_control() {
    let data = ready_page();
    for name in [
        tools::CURRENT_URL,
        tools::PAGE_TEXT,
        tools::READ_PAGE,
        tools::CONSOLE_MESSAGES,
        tools::NETWORK_REQUESTS,
    ] {
        let refusal = gate(&call(name), &data).unwrap_err();
        assert_eq!(refusal.code.as_deref(), Some("consent_required"), "{name}");
        assert!(
            refusal
                .text
                .contains(cb_core::browser::consent::READ_CONSENT_ACTION),
            "{name}: {}",
            refusal.text
        );
    }
}

#[test]
fn read_consent_does_not_open_a_single_write() {
    let mut data = ready_page();
    shared::grant_consent(&mut data, true, false).unwrap();
    assert!(gate(&call(tools::PAGE_TEXT), &data).is_ok());
    for name in [
        tools::NAVIGATE,
        tools::CLICK,
        tools::TYPE,
        tools::PRESS_KEY,
        tools::BACK,
        tools::FORWARD,
        tools::RELOAD,
    ] {
        let refusal = gate(&call(name), &data).unwrap_err();
        assert_eq!(
            refusal.code.as_deref(),
            Some("control_consent_required"),
            "{name}"
        );
        assert!(
            refusal
                .text
                .contains(cb_core::browser::consent::WRITE_CONSENT_ACTION),
            "{name}: {}",
            refusal.text
        );
    }
}

#[test]
fn full_consent_opens_both_categories() {
    let mut data = ready_page();
    shared::grant_consent(&mut data, true, true).unwrap();
    for name in tools::ALL {
        assert!(gate(&call(name), &data).is_ok(), "{name}");
    }
}

#[test]
fn a_loading_page_is_refused_even_with_consent_granted() {
    // The text is obtainable and would be wrong, which is the worse of the two
    // failures — the reason `Loading` is a variant at all.
    let mut data = ready_page();
    shared::grant_consent(&mut data, true, true).unwrap();
    data.availability = BrowserAvailability::Loading;
    let refusal = gate(&call(tools::PAGE_TEXT), &data).unwrap_err();
    assert_eq!(refusal.code.as_deref(), Some("page_loading"));
    assert!(refusal.text.contains("still loading"), "{}", refusal.text);
}

// ---------------------------------------------------------------------------
// refusal_code: where the six states could quietly become one
// ---------------------------------------------------------------------------

#[test]
fn the_five_unreadable_states_have_five_codes() {
    let mut data = ready_page();
    let codes: std::collections::BTreeSet<&str> = [
        BrowserAvailability::PluginDisabled,
        BrowserAvailability::PanelClosed,
        BrowserAvailability::Blank,
        BrowserAvailability::Loading,
        BrowserAvailability::Failed,
    ]
    .into_iter()
    .map(|availability| {
        data.availability = availability;
        refusal_code(&call(tools::PAGE_TEXT), &data)
    })
    .collect();
    assert_eq!(
        codes.len(),
        5,
        "a shared code would make every state one answer: {codes:?}"
    );
}

#[test]
fn a_state_refusal_and_a_consent_refusal_are_different_codes() {
    // Different fixes: one is something to do to the browser, the other is a
    // control only the user can click.
    let mut data = ready_page();
    let consent = refusal_code(&call(tools::PAGE_TEXT), &data);
    data.availability = BrowserAvailability::PanelClosed;
    let state = refusal_code(&call(tools::PAGE_TEXT), &data);
    assert_ne!(consent, state);
    assert_eq!(consent, "consent_required");
    assert_eq!(state, "browser_panel_closed");
}

#[test]
fn a_grant_for_another_page_is_its_own_code() {
    // The user did click Allow. An agent told "no permission" asks for it
    // again; one told "it was for another page" knows the navigation lost it.
    let mut data = ready_page();
    shared::grant_consent(&mut data, true, true).unwrap();
    shared::note_navigation(&mut data, "https://other.example.com/");
    shared::note_load_finished(&mut data, "https://other.example.com/");
    // The origin change already dropped the grant, so reconstruct the state
    // this code exists for: a grant naming a page that is not the one shown.
    data.consent =
        cb_core::browser::model::AutomationConsent::reads_and_writes("https://app.example.com");
    assert_eq!(
        refusal_code(&call(tools::PAGE_TEXT), &data),
        "consent_other_page"
    );
    let refusal = gate(&call(tools::PAGE_TEXT), &data).unwrap_err();
    assert!(
        refusal.text.contains("https://app.example.com")
            && refusal.text.contains("https://other.example.com"),
        "{}",
        refusal.text
    );
}

#[test]
fn a_ready_page_with_no_origin_is_refused_rather_than_treated_as_matching() {
    // Should not happen — the two facts arrive separately — and it is the one
    // combination where guessing would make a single grant cover every page.
    let mut data = ready_page();
    data.url = Some("about:blank".to_string());
    assert_eq!(
        refusal_code(&call(tools::PAGE_TEXT), &data),
        "no_page_origin"
    );
    assert!(gate(&call(tools::PAGE_TEXT), &data).is_err());
}

// ---------------------------------------------------------------------------
// the banner record
// ---------------------------------------------------------------------------

#[test]
fn the_record_names_the_program_the_tool_and_whether_it_would_change_the_page() {
    let record = request_record(&call(tools::CLICK), &peer(), true);
    assert_eq!(record.pid, 12345);
    assert_eq!(record.program, "codex.cmd");
    assert_eq!(record.tool, "browser_click");
    assert!(record.needs_writes);
    assert!(record.refused);
}

#[test]
fn a_read_is_recorded_as_not_needing_writes() {
    let record = request_record(&call(tools::PAGE_TEXT), &peer(), false);
    assert!(!record.needs_writes);
    assert!(!record.refused);
    assert_eq!(record.tool, "browser_page_text");
}

#[test]
fn the_record_dies_with_the_page_exactly_as_consent_does() {
    // "codex.cmd wanted to read this page" is a statement about a page, and so
    // is consent — so the two are cleared by the same event. Leaving the
    // record up across an origin change would put a request about somewhere
    // else above a control that grants access to *this* page.
    let mut data = ready_page();
    shared::note_agent_request(
        &mut data,
        request_record(&call(tools::PAGE_TEXT), &peer(), true),
    );
    assert!(data.last_agent_request.is_some());
    shared::note_navigation(&mut data, "https://other.example.com/");
    assert!(data.last_agent_request.is_none());
    assert!(!data.consent.reads(), "consent dies on the same event");
}

#[test]
fn a_same_origin_navigation_keeps_the_record_because_it_keeps_the_consent() {
    // Dropping it on every path change would hide a request the user has not
    // answered yet — and the grant it is asking for would still be the right
    // one, because consent is scoped to the origin.
    let mut data = ready_page();
    shared::grant_consent(&mut data, true, false).unwrap();
    shared::note_agent_request(
        &mut data,
        request_record(&call(tools::PAGE_TEXT), &peer(), true),
    );
    shared::note_navigation(&mut data, "https://app.example.com/invoices");
    assert!(data.last_agent_request.is_some());
    assert!(data.consent.reads(), "consent survives the same origin");
}

#[test]
fn the_record_is_cleared_when_the_panel_closes() {
    let mut data = ready_page();
    shared::note_agent_request(
        &mut data,
        request_record(&call(tools::PAGE_TEXT), &peer(), true),
    );
    shared::note_closed(&mut data, BrowserAvailability::PanelClosed);
    assert!(data.last_agent_request.is_none());
    assert!(!data.consent.reads());
}

#[test]
fn the_program_name_is_whatever_the_peer_reported_and_never_from_the_call() {
    // A self-reported name is the one field a rogue caller would lie about, so
    // this must come from `Peer` — which the pipe builds from the OS.
    let record = request_record(
        &call(tools::PAGE_TEXT),
        &Peer {
            pid: 7,
            program: "definitely-not-malware.exe".to_string(),
        },
        false,
    );
    assert_eq!(record.program, "definitely-not-malware.exe");
    assert_eq!(record.pid, 7);
}
