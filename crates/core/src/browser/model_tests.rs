//! Key-pinning for the browser wire types, and the distinctions they exist for.
//! Included by `model.rs`.
//!
//! `src/ipc/types.ts` mirrors these by hand — there is no codegen — so these
//! tests are what stops the two drifting. See `docs/architecture/ipc-contract.md`.

use super::*;

fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    keys
}

// ---------------------------------------------------------------------------
// The six states
// ---------------------------------------------------------------------------

#[test]
fn every_availability_serialises_under_its_own_camel_case_name() {
    let cases = [
        (BrowserAvailability::PluginDisabled, "pluginDisabled"),
        (BrowserAvailability::PanelClosed, "panelClosed"),
        (BrowserAvailability::Blank, "blank"),
        (BrowserAvailability::Loading, "loading"),
        (BrowserAvailability::Ready, "ready"),
        (BrowserAvailability::Failed, "failed"),
    ];
    for (state, wire) in cases {
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::json!(wire),
            "{state:?} must reach TypeScript as {wire}"
        );
    }
}

#[test]
fn there_are_six_availability_states_and_no_two_share_a_wire_name() {
    // The alarm for a variant quietly added, removed, or renamed into a
    // collision with another. Six, because each licenses something different.
    let all = [
        BrowserAvailability::PluginDisabled,
        BrowserAvailability::PanelClosed,
        BrowserAvailability::Blank,
        BrowserAvailability::Loading,
        BrowserAvailability::Ready,
        BrowserAvailability::Failed,
    ];
    let names: std::collections::BTreeSet<String> = all
        .iter()
        .map(|s| {
            serde_json::to_value(s)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(names.len(), 6);
}

#[test]
fn only_ready_licenses_a_read_and_loading_specifically_does_not() {
    // Loading is the variant the enum exists for: the text is obtainable and
    // would be *wrong*, which is worse than absent.
    assert!(BrowserAvailability::Ready.licenses_a_read());
    assert!(!BrowserAvailability::Loading.licenses_a_read());
    assert!(!BrowserAvailability::Blank.licenses_a_read());
    assert!(!BrowserAvailability::Failed.licenses_a_read());
    assert!(!BrowserAvailability::PanelClosed.licenses_a_read());
    assert!(!BrowserAvailability::PluginDisabled.licenses_a_read());
}

#[test]
fn plugin_disabled_and_panel_closed_give_different_reasons() {
    // The pair most likely to be collapsed, and the one where collapsing costs
    // the most: one is a setting, the other is one click away.
    let off = BrowserAvailability::PluginDisabled.reason();
    let closed = BrowserAvailability::PanelClosed.reason();
    assert_ne!(off, closed);
    assert!(off.contains("switched off"), "{off}");
    assert!(closed.contains("closed"), "{closed}");
}

#[test]
fn no_reason_is_a_bare_unavailable() {
    for state in [
        BrowserAvailability::PluginDisabled,
        BrowserAvailability::PanelClosed,
        BrowserAvailability::Blank,
        BrowserAvailability::Loading,
        BrowserAvailability::Ready,
        BrowserAvailability::Failed,
    ] {
        let reason = state.reason();
        assert!(reason.len() > 20, "{state:?} -> {reason:?}");
        assert!(!reason.eq_ignore_ascii_case("unavailable"));
    }
}

// ---------------------------------------------------------------------------
// Console and network rows
// ---------------------------------------------------------------------------

#[test]
fn console_entry_pins_its_keys() {
    let entry = ConsoleEntry {
        seq: 4,
        level: ConsoleLevel::Warn,
        method: "warn".into(),
        text: "careful".into(),
    };
    let json = serde_json::to_value(&entry).unwrap();
    assert_eq!(keys(&json), ["level", "method", "seq", "text"]);
    assert_eq!(json["level"], serde_json::json!("warn"));
}

#[test]
fn console_entry_keeps_the_pages_own_method_beside_our_ranking() {
    // An `Other` row is only readable because the method survives.
    let entry = ConsoleEntry {
        seq: 1,
        level: ConsoleLevel::Other,
        method: "table".into(),
        text: "[object Object]".into(),
    };
    let json = serde_json::to_value(&entry).unwrap();
    assert_eq!(json["level"], serde_json::json!("other"));
    assert_eq!(json["method"], serde_json::json!("table"));
}

#[test]
fn every_console_level_has_its_own_camel_case_name() {
    let cases = [
        (ConsoleLevel::Debug, "debug"),
        (ConsoleLevel::Log, "log"),
        (ConsoleLevel::Info, "info"),
        (ConsoleLevel::Warn, "warn"),
        (ConsoleLevel::Error, "error"),
        (ConsoleLevel::Other, "other"),
    ];
    for (level, wire) in cases {
        assert_eq!(
            serde_json::to_value(level).unwrap(),
            serde_json::json!(wire)
        );
    }
}

#[test]
fn network_entry_pins_its_keys_including_the_camel_cased_ones() {
    let entry = NetworkEntry {
        seq: 9,
        url: "https://example.com/api".into(),
        method: "POST".into(),
        status: Some(500),
        duration_ms: Some(12.5),
        transfer_size: Some(4096),
        source: NetworkSource::Fetch,
    };
    let json = serde_json::to_value(&entry).unwrap();
    assert_eq!(
        keys(&json),
        [
            "durationMs",
            "method",
            "seq",
            "source",
            "status",
            "transferSize",
            "url"
        ]
    );
    assert_eq!(json["source"], serde_json::json!("fetch"));
}

#[test]
fn an_absent_status_is_a_present_null_not_a_missing_key() {
    // The `skip_serializing_if` rule. A TypeScript reader distinguishes the two,
    // and a `resource` row genuinely has no status.
    let entry = NetworkEntry {
        seq: 1,
        url: "https://example.com/logo.png".into(),
        method: "GET".into(),
        status: None,
        duration_ms: None,
        transfer_size: None,
        source: NetworkSource::Resource,
    };
    let json = serde_json::to_value(&entry).unwrap();
    assert!(json.as_object().unwrap().contains_key("status"));
    assert!(json["status"].is_null());
    assert!(json["durationMs"].is_null());
    assert!(json["transferSize"].is_null());
}

#[test]
fn the_coverage_note_says_what_is_missing_rather_than_claiming_devtools() {
    let note = NETWORK_COVERAGE_NOTE;
    assert!(note.contains("instrumented"), "{note}");
    assert!(note.contains("no headers"), "{note}");
    assert!(note.contains("no bodies"), "{note}");
    assert!(
        note.contains("before the page"),
        "must admit the pre-init-script blind spot: {note}"
    );
}

// ---------------------------------------------------------------------------
// PageText
// ---------------------------------------------------------------------------

#[test]
fn page_text_pins_its_keys() {
    let page = PageText {
        text: "hi".into(),
        total_chars: 900,
        returned_chars: 2,
        truncated: true,
    };
    let json = serde_json::to_value(&page).unwrap();
    assert_eq!(
        keys(&json),
        ["returnedChars", "text", "totalChars", "truncated"]
    );
    // The total is the page's, never the slice's.
    assert_eq!(json["totalChars"], serde_json::json!(900));
}

// ---------------------------------------------------------------------------
// AutomationConsent — the unrepresentable state
// ---------------------------------------------------------------------------

#[test]
fn the_default_consent_grants_nothing() {
    // Reads are not free: the panel may be showing an authenticated admin page.
    let consent = AutomationConsent::default();
    assert!(!consent.reads());
    assert!(!consent.writes());
    assert_eq!(consent.origin(), None);
}

#[test]
fn no_constructor_can_produce_writes_without_reads() {
    for consent in [
        AutomationConsent::none(),
        AutomationConsent::reads_only("https://example.com"),
        AutomationConsent::reads_and_writes("https://example.com"),
    ] {
        assert!(
            !(consent.writes() && !consent.reads()),
            "{consent:?} is the shape that must be unrepresentable"
        );
    }
}

#[test]
fn deserialising_writes_without_reads_is_refused_rather_than_silently_fixed() {
    // The wire is the remaining door into the invalid shape. It is shut, and it
    // is shut by refusing rather than by quietly turning `reads` on.
    let error = serde_json::from_str::<AutomationConsent>(
        r#"{"reads":false,"writes":true,"origin":"https://example.com"}"#,
    )
    .expect_err("must not deserialise");
    assert!(
        error
            .to_string()
            .contains("write consent without read consent"),
        "{error}"
    );
}

#[test]
fn deserialising_read_consent_with_no_origin_is_refused() {
    // Consent is a statement about a page. One that names no page cannot be
    // compared against the page, so it would be permanently un-revocable.
    let error = serde_json::from_str::<AutomationConsent>(r#"{"reads":true,"writes":false}"#)
        .expect_err("must not deserialise");
    assert!(error.to_string().contains("name the origin"), "{error}");
}

#[test]
fn a_valid_consent_round_trips() {
    let consent = AutomationConsent::reads_and_writes("https://app.example.com");
    let json = serde_json::to_string(&consent).unwrap();
    assert_eq!(consent, serde_json::from_str(&json).unwrap());
}

#[test]
fn consent_pins_its_keys() {
    let json = serde_json::to_value(AutomationConsent::reads_only("https://a.example")).unwrap();
    assert_eq!(keys(&json), ["origin", "reads", "writes"]);
    assert!(json.as_object().unwrap().contains_key("origin"));
}

#[test]
fn an_absent_origin_is_a_present_null() {
    let json = serde_json::to_value(AutomationConsent::none()).unwrap();
    assert!(json.as_object().unwrap().contains_key("origin"));
    assert!(json["origin"].is_null());
}

#[test]
fn navigating_to_a_different_origin_resets_consent() {
    let consent = AutomationConsent::reads_and_writes("https://app.example.com");
    let after = consent.after_origin_change(Some("https://evil.example.com"));
    assert_eq!(after, AutomationConsent::none());
}

#[test]
fn navigating_never_grants_consent() {
    // Starting from nothing, no destination is a grant — including the origin a
    // grant would have named.
    let none = AutomationConsent::none();
    for destination in [Some("https://app.example.com"), Some("about:blank"), None] {
        assert_eq!(
            none.after_origin_change(destination),
            AutomationConsent::none()
        );
    }
}

#[test]
fn an_in_page_navigation_within_the_same_origin_keeps_consent() {
    // Otherwise every link click inside the app the user granted would re-prompt,
    // which teaches people to click Allow without reading it.
    let consent = AutomationConsent::reads_only("https://app.example.com");
    assert_eq!(
        consent.after_origin_change(Some("https://app.example.com")),
        consent
    );
}

#[test]
fn losing_the_origin_entirely_resets_consent() {
    // `about:blank` and a failed navigation both leave no origin. Neither is the
    // page the user granted.
    let consent = AutomationConsent::reads_and_writes("https://app.example.com");
    assert_eq!(consent.after_origin_change(None), AutomationConsent::none());
}
