//! Tests for the browser host's data layer.
//!
//! Everything the host decides *about state* is provable here with no webview,
//! no window and no main thread — which is the whole point of the split (see
//! `shared.rs`'s module doc). Each test is named for the rule it pins, and the
//! rules are the ones whose failure would be invisible by clicking around: a
//! consent flag outliving the page it was granted for, a page's own fabricated
//! message being applied, `Ready` reported over a half-loaded document.

use super::*;

use cb_core::browser::model::NetworkSource;
use cb_core::browser::origin::navigation_verdict;
use cb_core::browser::origin::{NavigationVerdict, APP_ORIGINS};

fn on(url: &str) -> BrowserShared {
    let mut shared = BrowserShared::new();
    note_opened(&mut shared);
    note_navigation(&mut shared, url);
    note_load_finished(&mut shared, url);
    shared
}

fn console_message(method: &str, text: &str) -> PageMessage {
    PageMessage::Console {
        method: method.to_owned(),
        text: text.to_owned(),
    }
}

// --- The six states, and what each licenses --------------------------------

#[test]
fn a_fresh_host_says_the_panel_is_closed_rather_than_that_the_plugin_is_off() {
    // The distinction the enum exists for. At construction this process has been
    // told nothing about the feature, and "the user switched the browser off" is
    // a claim about a setting nobody has read. `PanelClosed` is the true
    // statement available with no information.
    let shared = BrowserShared::new();
    assert_eq!(shared.availability, BrowserAvailability::PanelClosed);
    assert!(!shared.availability.licenses_a_read());
}

#[test]
fn a_navigation_reports_loading_and_not_ready() {
    // The whole reason `Loading` exists: text read now would be *wrong*, not
    // absent, and that is the worse of the two failures. If this ever answered
    // `Ready` a half-rendered page would be served as a finished one.
    let mut shared = BrowserShared::new();
    note_opened(&mut shared);
    note_navigation(&mut shared, "https://example.com/a");
    assert_eq!(shared.availability, BrowserAvailability::Loading);
    assert!(!shared.availability.licenses_a_read());
    assert!(!state_for(&shared).availability.licenses_a_read());
}

#[test]
fn only_a_finished_load_licenses_a_read() {
    let shared = on("https://example.com/a");
    assert_eq!(shared.availability, BrowserAvailability::Ready);
    assert!(state_for(&shared).availability.licenses_a_read());
}

#[test]
fn an_opened_panel_is_blank_with_no_url_no_title_and_no_consent() {
    let mut shared = on("https://example.com/a");
    grant_consent(&mut shared, true, true).unwrap();
    apply_page_message(&mut shared, PageMessage::Title { text: "T".into() });

    note_opened(&mut shared);

    assert_eq!(shared.availability, BrowserAvailability::Blank);
    assert_eq!(shared.url, None);
    assert_eq!(shared.title, None);
    assert!(!shared.consent.reads());
    assert!(current_origin(&shared).is_none());
}

#[test]
fn a_blank_panel_carries_no_origin_so_consent_can_never_attach_to_it() {
    // `about:blank` is an opaque origin, so there is nothing for a grant to be
    // *about* — and a grant with no origin could never be reset by a navigation,
    // because there would be no origin to compare.
    let mut shared = BrowserShared::new();
    note_opened(&mut shared);
    note_navigation(&mut shared, "about:blank");
    note_load_finished(&mut shared, "about:blank");

    assert!(current_origin(&shared).is_none());
    let refused = grant_consent(&mut shared, true, false).unwrap_err();
    // The refusal now comes from the *availability* check rather than the
    // origin one, because a finished load of `about:blank` correctly stays
    // `Blank` (see the test below). Both refuse, and this one names the panel
    // state, which is the more useful of the two answers - so what this test
    // pins is unchanged: there is no origin, the grant is refused, and no
    // consent is recorded.
    assert!(refused.contains("no page loaded"), "{refused}");
    assert!(!shared.consent.reads());
}

#[test]
fn a_finished_load_of_about_blank_stays_blank_rather_than_becoming_ready() {
    // Found by driving the real application: `browser_status` reported "the
    // page has finished loading" for a panel with nothing in it, because
    // `about:blank` does finish loading. That collapses `Blank` into `Ready`
    // and leaves the six states as five in practice - and `Blank` exists for
    // exactly this: a panel with no page, which is not an error and not
    // something a read can be attempted against.
    //
    // The signal is the origin, not the literal url: every opaque origin is a
    // page nothing can be read from or granted for.
    let mut shared = BrowserShared::new();
    note_opened(&mut shared);
    note_navigation(&mut shared, "about:blank");
    note_load_finished(&mut shared, "about:blank");
    assert_eq!(shared.availability, BrowserAvailability::Blank);
    assert!(!shared.availability.licenses_a_read());

    // And a real page still becomes readable.
    note_navigation(&mut shared, "https://app.example.com/");
    note_load_finished(&mut shared, "https://app.example.com/");
    assert_eq!(shared.availability, BrowserAvailability::Ready);
}

// --- Consent lives and dies with the page ----------------------------------

#[test]
fn navigating_to_another_origin_drops_consent() {
    // The rule the whole consent model rests on: "the agent may read this page"
    // is a statement about a *page*. Navigating is neither a grant nor a
    // renewal.
    let mut shared = on("https://app.example.com/dashboard");
    grant_consent(&mut shared, true, true).unwrap();
    assert!(shared.consent.reads() && shared.consent.writes());

    note_navigation(&mut shared, "https://evil.example.net/");

    assert!(!shared.consent.reads());
    assert!(!shared.consent.writes());
    assert_eq!(shared.consent.origin(), None);
}

#[test]
fn navigating_within_the_same_origin_keeps_consent() {
    // The other half, and it must hold or the feature is unusable: clicking a
    // link inside the application the user just approved would re-prompt on
    // every page.
    let mut shared = on("https://app.example.com/dashboard");
    grant_consent(&mut shared, true, false).unwrap();

    note_navigation(&mut shared, "https://app.example.com/settings?tab=1");

    assert!(shared.consent.reads());
    assert_eq!(shared.consent.origin(), Some("https://app.example.com"));
}

#[test]
fn a_redirect_that_lands_on_another_origin_drops_consent_at_the_landing_url() {
    // A navigation started at an approved origin can *finish* somewhere else —
    // an SSO bounce is the ordinary case. The url the load finished at is the
    // authority, so the consent check has to run again there; checking only at
    // navigation start would leave a grant attached to the identity provider's
    // page.
    let mut shared = on("https://app.example.com/dashboard");
    grant_consent(&mut shared, true, false).unwrap();

    note_navigation(&mut shared, "https://app.example.com/login");
    note_load_finished(&mut shared, "https://login.microsoftonline.com/x");

    assert!(!shared.consent.reads());
    assert_eq!(
        shared.url.as_deref(),
        Some("https://login.microsoftonline.com/x")
    );
}

#[test]
fn closing_the_panel_clears_the_page_the_consent_and_both_logs() {
    let mut shared = on("https://app.example.com/x");
    grant_consent(&mut shared, true, true).unwrap();
    apply_page_message(&mut shared, console_message("error", "boom"));
    apply_page_message(&mut shared, PageMessage::Title { text: "T".into() });

    note_closed(&mut shared, BrowserAvailability::PluginDisabled);

    assert_eq!(shared.availability, BrowserAvailability::PluginDisabled);
    assert_eq!(shared.url, None);
    assert_eq!(shared.title, None);
    assert!(!shared.consent.reads());
    assert!(console_batch(&shared, 0).entries.is_empty());
    assert!(network_batch(&shared, 0).entries.is_empty());
}

#[test]
fn a_grant_is_refused_while_the_page_is_still_loading() {
    // Granting against a url that is mid-navigation attaches the grant to
    // whatever the page turns out to be.
    let mut shared = BrowserShared::new();
    note_opened(&mut shared);
    note_navigation(&mut shared, "https://app.example.com/x");

    let refused = grant_consent(&mut shared, true, false).unwrap_err();
    assert!(refused.contains("still loading"), "{refused}");
    assert!(!shared.consent.reads());
}

#[test]
fn write_consent_implies_read_consent() {
    let mut shared = on("https://app.example.com/x");
    let granted = grant_consent(&mut shared, false, true).unwrap();
    assert!(granted.writes());
    assert!(
        granted.reads(),
        "write-without-read is the worst combination available: a blind click is \
         the one action whose effect nobody can predict"
    );
}

#[test]
fn revoking_needs_no_origin_and_always_succeeds() {
    // Withdrawal must never be refusable. A user un-ticking the banner while the
    // page happens to be reloading has to be obeyed.
    let mut shared = BrowserShared::new();
    let revoked = grant_consent(&mut shared, false, false).unwrap();
    assert!(!revoked.reads() && !revoked.writes());
}

// --- Everything the page sends is hostile input -----------------------------

#[test]
fn an_unknown_console_method_is_other_and_keeps_the_pages_own_word() {
    let mut shared = on("https://x.example.com/");
    apply_page_message(&mut shared, console_message("table", "rows"));

    let batch = console_batch(&shared, 0);
    let entry = &batch.entries[0];
    assert_eq!(entry.level, ConsoleLevel::Other);
    assert_eq!(entry.method, "table");
}

#[test]
fn a_page_error_is_an_error_whichever_window_event_reported_it() {
    let mut shared = on("https://x.example.com/");
    apply_page_message(
        &mut shared,
        PageMessage::PageError {
            source: "unhandledrejection".into(),
            text: "nope".into(),
        },
    );

    let entry = &console_batch(&shared, 0).entries[0];
    assert_eq!(entry.level, ConsoleLevel::Error);
    assert_eq!(entry.method, "unhandledrejection");
}

#[test]
fn a_page_that_fabricates_an_eval_reply_is_counted_and_not_applied() {
    // The host's evaluations answer through wry's own callback, so nothing is
    // ever waiting on an id and `pending_eval` is empty by construction. A page
    // that guesses this app's own message kind therefore gets a rejection, not a
    // resolved promise.
    let mut shared = on("https://x.example.com/");
    apply_page_message(
        &mut shared,
        PageMessage::EvalReply {
            id: 7,
            payload: "{\"ok\":true,\"value\":\"stolen\"}".into(),
        },
    );

    assert_eq!(shared.rejected_messages, 1);
    assert!(shared.last_refusal.as_deref().unwrap().contains('7'));
    assert!(console_batch(&shared, 0).entries.is_empty());
    assert!(shared.pending_eval.is_empty());
}

#[test]
fn a_rejected_message_is_counted_with_its_reason() {
    let mut shared = BrowserShared::new();
    note_page_problem(
        &mut shared,
        &PageMessageProblem::UnknownKind {
            kind: "consent".into(),
        },
    );
    note_page_problem(&mut shared, &PageMessageProblem::NotAnObject);

    assert_eq!(shared.rejected_messages, 2);
    assert_eq!(
        shared.last_refusal.as_deref(),
        Some("the page sent JSON that is not an object")
    );
}

#[test]
fn a_pushed_row_carries_the_sequence_the_ring_gave_it() {
    // `Slice` hands out `&T` and no sequence, so the row's own `seq` field is
    // the only way a reader learns its cursor. This pins the pre-computed number
    // against the ring's actual numbering.
    let mut shared = on("https://x.example.com/");
    apply_page_message(&mut shared, console_message("log", "one"));
    apply_page_message(&mut shared, console_message("log", "two"));

    let batch = console_batch(&shared, 0);
    assert_eq!(
        batch.entries.iter().map(|e| e.seq).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(batch.next_cursor, 2);
    assert!(console_batch(&shared, 2).entries.is_empty());
}

#[test]
fn a_cursor_that_missed_entries_is_told_how_many() {
    let mut shared = on("https://x.example.com/");
    for i in 0..(DEFAULT_CAPACITY + 5) {
        apply_page_message(&mut shared, console_message("log", &format!("{i}")));
    }

    let batch = console_batch(&shared, 0);
    assert_eq!(batch.entries.len(), DEFAULT_CAPACITY);
    assert_eq!(
        batch.missed, 5,
        "a silent gap is the failure the ring exists to prevent"
    );
}

#[test]
fn every_network_answer_states_what_the_instrumentation_cannot_see() {
    // These rows are not DevTools' network panel, and a list that looks like one
    // while silently missing every header, every body and every request made
    // before the init script ran is worse than one that says so.
    let mut shared = on("https://x.example.com/");
    apply_page_message(
        &mut shared,
        PageMessage::Network {
            url: "https://x.example.com/api".into(),
            method: "GET".into(),
            status: None,
            duration_ms: Some(12.0),
            transfer_size: Some(30),
            source: NetworkSource::Resource,
        },
    );

    let batch = network_batch(&shared, 0);
    assert_eq!(batch.coverage, NETWORK_COVERAGE_NOTE);
    assert_eq!(batch.entries[0].status, None);
    assert_eq!(batch.entries[0].source, NetworkSource::Resource);

    // And an empty log still carries it — the honesty is a property of the
    // answer, not of having rows.
    let empty = network_batch(&BrowserShared::new(), 0);
    assert_eq!(empty.coverage, NETWORK_COVERAGE_NOTE);
}

// --- A refused navigation is not a navigation ------------------------------

#[test]
fn a_refused_navigation_leaves_the_url_alone_and_is_counted() {
    // What is on screen has not changed, so reporting the refused address as the
    // current one would be a wrong answer about what the user is looking at.
    let mut shared = on("https://app.example.com/dashboard");
    grant_consent(&mut shared, true, false).unwrap();

    let verdict = navigation_verdict("tauri://localhost/", &APP_ORIGINS);
    let NavigationVerdict::Refuse(refusal) = verdict else {
        panic!("the app's own origin must be refused");
    };
    note_refused_navigation(&mut shared, &refusal);

    assert_eq!(
        shared.url.as_deref(),
        Some("https://app.example.com/dashboard")
    );
    assert!(
        shared.consent.reads(),
        "a refused navigation is not a page change"
    );
    assert_eq!(shared.refused_navigations, 1);
    assert!(shared.last_refusal.is_some());
}

#[test]
fn a_navigation_this_app_could_not_start_is_failed_and_drops_consent() {
    let mut shared = on("https://app.example.com/x");
    grant_consent(&mut shared, true, false).unwrap();

    note_failed(&mut shared, "load_url refused: the webview is gone");

    assert_eq!(shared.availability, BrowserAvailability::Failed);
    assert!(!shared.availability.licenses_a_read());
    assert!(!shared.consent.reads());
}

// --- The wire shape --------------------------------------------------------

#[test]
fn snapshot_serialises_with_the_keys_the_ui_reads() {
    // `src/ipc/types.ts` mirrors this by hand; this is what stops the two
    // drifting. No key may be absent — an absent key and a key holding `null`
    // are different things to a TypeScript reader.
    let mut shared = on("https://app.example.com/x");
    grant_consent(&mut shared, true, false).unwrap();
    apply_page_message(
        &mut shared,
        PageMessage::Title {
            text: "Home".into(),
        },
    );

    let json = serde_json::to_value(snapshot(&shared)).unwrap();
    let mut keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "availability",
            "consent",
            "lastAgentRequest",
            "lastRefusal",
            "origin",
            "refusedNavigations",
            "rejectedMessages",
            "title",
            "url",
        ]
    );
    assert_eq!(json["availability"], "ready");
    assert_eq!(json["origin"], "https://app.example.com");
    assert_eq!(json["consent"]["reads"], true);
    assert_eq!(json["consent"]["writes"], false);
    assert_eq!(json["lastRefusal"], serde_json::Value::Null);
    assert_eq!(json["lastAgentRequest"], serde_json::Value::Null);
}

#[test]
fn the_batches_and_the_rect_serialise_with_the_keys_the_ui_reads() {
    let console = serde_json::to_value(console_batch(&BrowserShared::new(), 0)).unwrap();
    let mut keys: Vec<&str> = console
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    keys.sort();
    assert_eq!(keys, vec!["discarded", "entries", "missed", "nextCursor"]);

    let network = serde_json::to_value(network_batch(&BrowserShared::new(), 0)).unwrap();
    let mut keys: Vec<&str> = network
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["coverage", "discarded", "entries", "missed", "nextCursor"]
    );

    let rect: BrowserRect =
        serde_json::from_str(r#"{"left":10,"top":20,"width":800,"height":600}"#).unwrap();
    assert_eq!(rect.width, 800.0);
}

// --- One page's log is never served as the next page's ----------------------

#[test]
fn a_navigation_to_another_document_clears_both_logs() {
    // `note_closed`'s own rule, applied where it was missing: "they are a log
    // of a page that no longer exists, and serving them afterwards would
    // attribute one page's errors to the next". `Ring::clear`'s doc says it is
    // "used when the page navigates" and no navigation path called it, so a
    // checkout page's `console.error("payment declined")` was answered as the
    // settings page's complete console record, `missed: 0` and all.
    let mut shared = on("https://app.example.com/checkout");
    grant_consent(&mut shared, true, false).unwrap();
    apply_page_message(&mut shared, console_message("error", "payment declined"));
    assert_eq!(console_batch(&shared, 0).entries.len(), 1);

    note_navigation(&mut shared, "https://app.example.com/settings");
    note_load_finished(&mut shared, "https://app.example.com/settings");

    let batch = console_batch(&shared, 0);
    assert!(
        batch.entries.is_empty(),
        "the previous document's rows survived: {batch:?}"
    );
    assert_eq!(
        batch.missed, 0,
        "nothing this page logged was evicted from the buffer"
    );
    assert_eq!(
        batch.discarded, 1,
        "the discarded row must be admitted, not silently dropped"
    );
    assert!(network_batch(&shared, 0).entries.is_empty());
}

#[test]
fn the_repeated_navigation_events_for_one_load_do_not_clear_the_new_pages_own_log() {
    // One user navigation reaches `note_navigation` up to three times (the
    // `load` call, wry's navigation handler and `PageLoadEvent::Started`), so
    // clearing unconditionally would discard rows the *new* document had
    // already logged. Keyed on the url changing instead.
    let mut shared = on("https://app.example.com/a");
    note_navigation(&mut shared, "https://app.example.com/b");
    apply_page_message(&mut shared, console_message("log", "b is loading"));
    note_navigation(&mut shared, "https://app.example.com/b");
    note_navigation(&mut shared, "https://app.example.com/b");

    let batch = console_batch(&shared, 0);
    assert_eq!(batch.entries.len(), 1, "{batch:?}");
    assert_eq!(batch.entries[0].text, "b is loading");
}

#[test]
fn consent_for_a_new_page_never_licenses_reading_the_previous_pages_capture() {
    // The consent boundary. A page the user granted nothing for logs several
    // hundred attacker-authored lines and fetches URLs of its choosing; the
    // user then navigates to their own admin console and grants a read there.
    // Serving the first page's rows under that grant hands the agent the
    // contents of a page consent was never given for - and, since the rows
    // carry no origin, presents them as the admin console's own.
    let mut shared = on("https://evil.example/");
    for i in 0..40 {
        apply_page_message(&mut shared, console_message("log", &format!("noise {i}")));
    }

    note_navigation(&mut shared, "https://admin.internal.corp/");
    note_load_finished(&mut shared, "https://admin.internal.corp/");
    grant_consent(&mut shared, true, false).unwrap();

    let batch = console_batch(&shared, 0);
    assert!(batch.entries.is_empty(), "{batch:?}");
    assert_eq!(batch.discarded, 40);
    assert_eq!(batch.missed, 0);
}

#[test]
fn a_redirect_that_changes_origin_at_load_finish_clears_the_logs_too() {
    // An SSO bounce can move the origin at load-finish, which is the one place
    // `note_navigation` may not have seen it. Clearing there is keyed on the
    // **origin** rather than the url: at ordinary load-finish the rows already
    // belong to the document that just loaded, and clearing them would throw
    // away the current page's own console.
    let mut shared = on("https://app.example.com/x");
    apply_page_message(&mut shared, console_message("error", "session expired"));

    note_load_finished(&mut shared, "https://login.microsoftonline.com/x");

    let batch = console_batch(&shared, 0);
    assert!(batch.entries.is_empty(), "{batch:?}");
    assert_eq!(batch.discarded, 1);
}

#[test]
fn an_ordinary_load_finish_keeps_the_rows_the_page_has_already_logged() {
    let mut shared = BrowserShared::new();
    note_opened(&mut shared);
    note_navigation(&mut shared, "https://app.example.com/x");
    apply_page_message(&mut shared, console_message("log", "inline script ran"));
    note_load_finished(&mut shared, "https://app.example.com/x");

    let batch = console_batch(&shared, 0);
    assert_eq!(batch.entries.len(), 1, "{batch:?}");
}
