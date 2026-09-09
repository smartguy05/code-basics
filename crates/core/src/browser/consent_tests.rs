//! Who may read and drive the page. Included by `consent.rs`.

use super::*;

const ORIGIN: &str = "https://app.example.com";
const OTHER: &str = "https://admin.example.com";

const READS: [BrowserTool; 6] = [
    BrowserTool::CurrentUrl,
    BrowserTool::PageText,
    BrowserTool::ReadPage,
    BrowserTool::ConsoleMessages,
    BrowserTool::NetworkRequests,
    BrowserTool::Screenshot,
];

const WRITES: [BrowserTool; 7] = [
    BrowserTool::Navigate,
    BrowserTool::Click,
    BrowserTool::Type,
    BrowserTool::PressKey,
    BrowserTool::Back,
    BrowserTool::Forward,
    BrowserTool::Reload,
];

// ---------------------------------------------------------------------------
// Categories
// ---------------------------------------------------------------------------

#[test]
fn navigate_is_a_write_not_a_read() {
    // Moving the page the user is looking at mutates state they can see, in a
    // window they may not be watching. Filing it under reads would make
    // "let the agent look" also mean "let the agent take me elsewhere".
    assert_eq!(BrowserTool::Navigate.category(), Category::Write);
}

#[test]
fn history_and_reload_are_writes_too() {
    // Same reasoning: they move the page.
    for tool in [BrowserTool::Back, BrowserTool::Forward, BrowserTool::Reload] {
        assert_eq!(tool.category(), Category::Write, "{tool:?}");
    }
}

#[test]
fn a_screenshot_is_a_read() {
    // It is a picture of exactly what the read tools would return in words.
    assert_eq!(BrowserTool::Screenshot.category(), Category::Read);
}

#[test]
fn status_is_its_own_category() {
    assert_eq!(BrowserTool::Status.category(), Category::Status);
}

#[test]
fn every_tool_has_a_distinct_mcp_name() {
    let mut names: Vec<&str> = std::iter::once(BrowserTool::Status)
        .chain(READS)
        .chain(WRITES)
        .map(BrowserTool::name)
        .collect();
    let count = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), count);
    assert!(names.iter().all(|n| n.starts_with("browser_")));
}

// ---------------------------------------------------------------------------
// Status always answers
// ---------------------------------------------------------------------------

#[test]
fn status_is_allowed_in_every_state_with_no_consent() {
    // Its entire job is reporting which of the six states this is. Gating it
    // would make "off" indistinguishable from "refused".
    for availability in [
        BrowserAvailability::PluginDisabled,
        BrowserAvailability::PanelClosed,
        BrowserAvailability::Blank,
        BrowserAvailability::Loading,
        BrowserAvailability::Ready,
        BrowserAvailability::Failed,
    ] {
        let state = BrowserState {
            availability,
            origin: None,
        };
        assert!(
            decide(BrowserTool::Status, &AutomationConsent::none(), &state).allowed(),
            "{availability:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// The default refuses everything else
// ---------------------------------------------------------------------------

#[test]
fn with_no_consent_every_read_is_refused() {
    // Reads are not free: the panel may be showing an authenticated admin page.
    let state = BrowserState::ready(ORIGIN);
    for tool in READS {
        let decision = decide(tool, &AutomationConsent::none(), &state);
        assert!(!decision.allowed(), "{tool:?} must be refused by default");
    }
}

#[test]
fn with_no_consent_every_write_is_refused() {
    let state = BrowserState::ready(ORIGIN);
    for tool in WRITES {
        assert!(
            !decide(tool, &AutomationConsent::none(), &state).allowed(),
            "{tool:?}"
        );
    }
}

#[test]
fn a_read_refusal_names_the_origin_and_the_control_the_user_would_click() {
    // "Not permitted" sends the agent into retries and the user to the wrong
    // settings page.
    let state = BrowserState::ready(ORIGIN);
    let decision = decide(BrowserTool::PageText, &AutomationConsent::none(), &state);
    let reason = decision.reason().expect("refused");
    assert!(reason.contains(ORIGIN), "{reason}");
    assert!(reason.contains(READ_CONSENT_ACTION), "{reason}");
    assert!(reason.contains("Nothing was read"), "{reason}");
    assert!(reason.contains("not remembered"), "{reason}");
}

#[test]
fn a_write_refusal_names_the_write_control_not_the_read_one() {
    // Sending the user to the read button for a click they asked for would
    // leave them clicking Allow twice and not knowing why the second was needed.
    let state = BrowserState::ready(ORIGIN);
    let reason = decide(BrowserTool::Click, &AutomationConsent::none(), &state)
        .reason()
        .expect("refused")
        .to_owned();
    assert!(reason.contains(WRITE_CONSENT_ACTION), "{reason}");
    assert!(reason.contains("Nothing was changed"), "{reason}");
}

// ---------------------------------------------------------------------------
// Read consent does not imply write consent
// ---------------------------------------------------------------------------

#[test]
fn read_consent_allows_reads() {
    let state = BrowserState::ready(ORIGIN);
    let consent = AutomationConsent::reads_only(ORIGIN);
    for tool in READS {
        assert!(decide(tool, &consent, &state).allowed(), "{tool:?}");
    }
}

#[test]
fn read_consent_does_not_allow_a_single_write() {
    // The asymmetry the whole model exists for.
    let state = BrowserState::ready(ORIGIN);
    let consent = AutomationConsent::reads_only(ORIGIN);
    for tool in WRITES {
        assert!(!decide(tool, &consent, &state).allowed(), "{tool:?}");
    }
}

#[test]
fn full_consent_allows_both() {
    let state = BrowserState::ready(ORIGIN);
    let consent = AutomationConsent::reads_and_writes(ORIGIN);
    for tool in READS.iter().chain(WRITES.iter()) {
        assert!(decide(*tool, &consent, &state).allowed(), "{tool:?}");
    }
}

// ---------------------------------------------------------------------------
// Consent is scoped to an origin
// ---------------------------------------------------------------------------

#[test]
fn consent_for_one_origin_does_not_apply_to_another() {
    // The failure this scoping exists to prevent: the user allows a read of
    // their own staging site, the page redirects, and the grant follows it.
    let consent = AutomationConsent::reads_and_writes(ORIGIN);
    let elsewhere = BrowserState::ready(OTHER);
    for tool in READS.iter().chain(WRITES.iter()) {
        assert!(!decide(*tool, &consent, &elsewhere).allowed(), "{tool:?}");
    }
}

#[test]
fn a_grant_for_a_different_origin_says_which_page_it_was_for() {
    // The user did click Allow. Reporting "no consent" would have them clicking
    // it again without understanding why the first one stopped counting.
    let consent = AutomationConsent::reads_only(ORIGIN);
    let reason = decide(BrowserTool::PageText, &consent, &BrowserState::ready(OTHER))
        .reason()
        .expect("refused")
        .to_owned();
    assert!(
        reason.contains(ORIGIN),
        "must name the granted origin: {reason}"
    );
    assert!(
        reason.contains(OTHER),
        "must name the current origin: {reason}"
    );
    assert!(reason.contains("per page"), "{reason}");
}

#[test]
fn origins_are_compared_exactly_and_not_by_prefix() {
    // `https://app.example.com.evil.example` starts with the granted origin.
    let consent = AutomationConsent::reads_only(ORIGIN);
    for hostile in [
        "https://app.example.com.evil.example",
        "https://app.example.com:8443",
        "http://app.example.com",
        "https://evil.example/?x=https://app.example.com",
    ] {
        assert!(
            !decide(
                BrowserTool::PageText,
                &consent,
                &BrowserState::ready(hostile)
            )
            .allowed(),
            "{hostile}"
        );
    }
}

// ---------------------------------------------------------------------------
// Availability gates everything but status
// ---------------------------------------------------------------------------

#[test]
fn a_read_is_refused_while_the_page_is_still_loading_even_with_full_consent() {
    // `Loading` is the state the enum exists for: the text is obtainable and
    // would be wrong, which is worse than absent.
    let state = BrowserState {
        availability: BrowserAvailability::Loading,
        origin: Some(ORIGIN.into()),
    };
    let decision = decide(
        BrowserTool::PageText,
        &AutomationConsent::reads_and_writes(ORIGIN),
        &state,
    );
    assert!(!decision.allowed());
    let reason = decision.reason().unwrap();
    assert!(reason.contains("still loading"), "{reason}");
}

#[test]
fn each_unavailable_state_refuses_with_its_own_reason() {
    // The six states must not collapse in the refusal either: an agent told
    // "unavailable" cannot tell a switched-off plugin from a closed panel.
    let consent = AutomationConsent::reads_and_writes(ORIGIN);
    let mut seen = std::collections::BTreeSet::new();
    for availability in [
        BrowserAvailability::PluginDisabled,
        BrowserAvailability::PanelClosed,
        BrowserAvailability::Blank,
        BrowserAvailability::Loading,
        BrowserAvailability::Failed,
    ] {
        let state = BrowserState {
            availability,
            origin: Some(ORIGIN.into()),
        };
        let decision = decide(BrowserTool::PageText, &consent, &state);
        assert!(!decision.allowed(), "{availability:?}");
        seen.insert(decision.reason().unwrap().to_owned());
    }
    assert_eq!(seen.len(), 5, "each state must give its own reason");
}

#[test]
fn a_refusal_for_an_unavailable_browser_names_the_tool() {
    let reason = decide(
        BrowserTool::Click,
        &AutomationConsent::none(),
        &BrowserState::plugin_disabled(),
    )
    .reason()
    .expect("refused")
    .to_owned();
    assert!(reason.contains("browser_click"), "{reason}");
}

#[test]
fn a_ready_page_with_no_determinable_origin_refuses_rather_than_matching_anything() {
    // The two facts arrive separately, and this is the combination where a
    // guess is dangerous: an unknown origin that "matched" would make one
    // grant cover every page.
    let state = BrowserState {
        availability: BrowserAvailability::Ready,
        origin: None,
    };
    let decision = decide(
        BrowserTool::PageText,
        &AutomationConsent::reads_and_writes(ORIGIN),
        &state,
    );
    assert!(!decision.allowed());
    assert!(
        decision.reason().unwrap().contains("origin"),
        "{decision:?}"
    );
}

// ---------------------------------------------------------------------------
// The consequence worth stating
// ---------------------------------------------------------------------------

#[test]
fn navigate_cannot_be_used_from_a_blank_panel_and_that_is_deliberate() {
    // A blank page has no origin, so no grant can name it, so `browser_navigate`
    // is unusable until a person points the panel somewhere. That is the
    // conservative reading of "consent is a statement about a page" and it is
    // recorded here so nobody reads it as a bug and opens the hole: an
    // exemption for `Blank` would be an un-consented navigation, which is
    // exactly what the write category exists to require consent for.
    let blank = BrowserState {
        availability: BrowserAvailability::Blank,
        origin: None,
    };
    for consent in [
        AutomationConsent::none(),
        AutomationConsent::reads_and_writes(ORIGIN),
    ] {
        assert!(!decide(BrowserTool::Navigate, &consent, &blank).allowed());
    }
}

#[test]
fn the_typescript_banner_quotes_the_consent_labels() {
    // Every refusal an agent reads is built around these exact words - "Click
    // \"Allow agents to read this page\" in the browser panel" - and the panel
    // is the only place that grant can be made. A reworded button therefore
    // sends the user hunting for a control that does not exist, and neither
    // side's tests would notice: this is the only thing that does.
    //
    // The same drift-alarm shape as `TERMINAL_STACK_SPAN`, reaching across the
    // language boundary because there is no codegen here (see
    // `docs/architecture/ipc-contract.md`).
    let logic = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src/components/browserPanelLogic.ts");
    let source = std::fs::read_to_string(&logic)
        .unwrap_or_else(|error| panic!("{}: {error}", logic.display()));

    for label in [READ_CONSENT_ACTION, WRITE_CONSENT_ACTION] {
        assert!(
            source.contains(&format!("\"{label}\"")),
            "{} must quote {label:?} verbatim; the refusals in this module name it as the \
             control the user would click",
            logic.display()
        );
    }
}

// --- The read must have run on the page the grant was for -------------------

#[test]
fn a_read_whose_script_ran_on_another_origin_is_refused() {
    // The gate is evaluated under the shared mutex and released before the
    // script is evaluated on the main thread, so the document the script runs
    // against need not be the one the decision was made about: a `meta refresh`
    // or a scripted `location.replace` can land elsewhere in between. The
    // scripts return their own `location.href`; it was read and thrown away, so
    // the answer named no address and the mismatch was undetectable.
    let refusal = read_ran_on_the_granted_page(
        BrowserTool::PageText,
        Some("https://a.example"),
        Some("https://b.example/inbox"),
    )
    .expect_err("a read that landed on another origin must not be answered");
    assert!(refusal.contains("https://a.example"), "{refusal}");
    assert!(refusal.contains("https://b.example"), "{refusal}");
    assert!(refusal.contains("browser_page_text"), "{refusal}");
}

#[test]
fn a_read_on_the_granted_origin_is_answered() {
    assert!(read_ran_on_the_granted_page(
        BrowserTool::PageText,
        Some("https://a.example"),
        Some("https://a.example/other/path?q=1"),
    )
    .is_ok());
}

#[test]
fn a_read_whose_page_reported_no_address_is_refused_rather_than_assumed_to_match() {
    // `WebView::url()` returns an empty string for a `data:` URL, and a page
    // that tore down mid-evaluation answers nothing. An unknown address cannot
    // be compared to a grant, and treating it as matching would make one grant
    // cover every page - the same reason `decide` refuses a `Ready` page with
    // no origin.
    let refusal =
        read_ran_on_the_granted_page(BrowserTool::ReadPage, Some("https://a.example"), None)
            .expect_err("an unknown address must not pass for the granted one");
    assert!(refusal.contains("could not say"), "{refusal}");
}

#[test]
fn a_read_with_no_grant_recorded_is_left_to_the_gate() {
    // This check is a *second* one, after `decide`. It exists to catch a page
    // that moved under it, not to re-answer whether consent exists, and
    // inventing a refusal here would report the wrong reason.
    assert!(
        read_ran_on_the_granted_page(BrowserTool::PageText, None, Some("https://a.example/x"))
            .is_ok()
    );
}
