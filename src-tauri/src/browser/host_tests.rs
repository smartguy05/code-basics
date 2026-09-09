//! Tests for the two decisions in the host that are not about the webview.
//!
//! The `WebView` itself cannot be constructed without a window, so nothing that
//! calls it is reachable here. These two are: the rect that becomes an OS
//! surface's position, and the envelope every injected script answers in.

use super::*;

// --- The rect that places a real window ------------------------------------

#[test]
fn a_measured_rect_becomes_logical_bounds() {
    let bounds = bounds_for(BrowserRect {
        left: 120.0,
        top: 140.0,
        width: 720.0,
        height: 480.0,
    })
    .unwrap();
    assert_eq!(
        bounds,
        Rect {
            position: LogicalPosition::new(120.0, 140.0).into(),
            size: LogicalSize::new(720.0, 480.0).into(),
        },
        "wry's dpi types default to logical, and the panel measures CSS pixels; \
         a physical rect here would place the page at the wrong scale"
    );
}

#[test]
fn a_rect_that_is_not_a_number_is_refused_rather_than_placed() {
    // The frontend already refuses this (`browserPanelLogic.pageRect`), and this
    // side checks anyway: "the caller validated it" is not a property this side
    // can verify, and a NaN reaches `set_bounds` as an unpredictable number that
    // puts a real OS window somewhere nobody chose.
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let refused = bounds_for(BrowserRect {
            left: bad,
            top: 0.0,
            width: 100.0,
            height: 100.0,
        })
        .unwrap_err();
        assert!(refused.contains("not a number"), "{refused}");
    }
}

#[test]
fn a_degenerate_rect_is_refused() {
    // The `createResizeGate` lesson, on this side of the wire: a
    // `ResizeObserver` reports 0×0 while an element is hidden and fires once on
    // `observe()` before layout, so the first measurement of a panel is also the
    // usual one. A 0×0 child webview is not harmless — it is a real surface with
    // no size.
    for (width, height) in [(0.0, 100.0), (100.0, 0.0), (-5.0, 100.0), (100.0, -5.0)] {
        let refused = bounds_for(BrowserRect {
            left: 0.0,
            top: 0.0,
            width,
            height,
        })
        .unwrap_err();
        assert!(refused.contains("nowhere to put the page"), "{refused}");
    }
}

// --- The envelope every script answers in ----------------------------------

#[test]
fn a_bare_null_is_a_failure_and_not_an_empty_page() {
    // The Windows fact this whole envelope exists for:
    // `evaluate_script_with_callback` **ignores the exception** and fires with
    // the literal string `null` — the spike confirmed it with a script that did
    // nothing but throw. Every script from `cb_core::browser::script` is wrapped
    // so it cannot throw, so a `null` arriving here means the wrapper is gone or
    // the document was replaced mid-evaluation. Reporting that as an empty page
    // is the silent wrong answer: an empty console, an empty page, a click that
    // never happened.
    let refused = envelope_value("null").unwrap_err();
    assert!(refused.contains("returned nothing"), "{refused}");
    let refused = envelope_value("  null  ").unwrap_err();
    assert!(refused.contains("returned nothing"), "{refused}");
}

#[test]
fn an_ok_envelope_is_returned_whole() {
    let value = envelope_value(r#"{"ok":true,"value":"hello","url":"https://x.example"}"#).unwrap();
    assert_eq!(value["value"], "hello");
    assert_eq!(value["url"], "https://x.example");
}

#[test]
fn a_refusing_envelope_reports_the_pages_own_reason() {
    let refused =
        envelope_value(r#"{"ok":false,"error":"no element is held under that reference"}"#)
            .unwrap_err();
    assert_eq!(refused, "no element is held under that reference");
}

#[test]
fn an_envelope_with_no_ok_flag_is_refused_rather_than_read_optimistically() {
    // A page can reach `window.ipc` and can be anything at all. An answer whose
    // shape is not ours is not an answer.
    let refused = envelope_value(r#"{"value":"hello"}"#).unwrap_err();
    assert!(refused.contains("refused without saying why"), "{refused}");
    let refused = envelope_value(r#"{"ok":"true","value":"hello"}"#).unwrap_err();
    assert!(refused.contains("refused without saying why"), "{refused}");
    let refused = envelope_value("not json at all").unwrap_err();
    assert!(refused.contains("was not JSON"), "{refused}");
}

#[test]
fn the_envelope_arrives_json_encoded_because_the_script_returns_a_string() {
    // MEASURED, not assumed, and the tests above had assumed the other shape.
    // The injected scripts end in `return JSON.stringify({ok:true,...})`, so
    // the value `evaluate_script_with_callback` hands back is a *string*, and
    // WebView2 delivers it JSON-encoded - the Phase 4 spike saw exactly this
    // (`document.title` came back as `"focus-test"`, with the quotes). So the
    // raw text is JSON containing JSON, and parsing it once yields a
    // `Value::String` whose `.get("ok")` is `None` - which read as "the page
    // refused without saying why" and made every page read fail.
    //
    // Found by calling `browser_page_text` through the MCP server against a
    // real page. Nothing short of that could have caught it: every unit test
    // here fed the inner shape directly.
    let inner = r#"{"ok":true,"value":"hello","url":"https://x.example"}"#;
    let raw = serde_json::to_string(inner).unwrap();
    let value = envelope_value(&raw).unwrap();
    assert_eq!(value["value"], "hello");
    assert_eq!(value["url"], "https://x.example");
}

#[test]
fn a_json_encoded_refusal_still_reports_the_pages_own_reason() {
    let inner = r#"{"ok":false,"error":"the page instrumentation is not installed"}"#;
    let raw = serde_json::to_string(inner).unwrap();
    assert_eq!(
        envelope_value(&raw).unwrap_err(),
        "the page instrumentation is not installed"
    );
}

#[test]
fn a_json_string_that_is_not_an_envelope_is_refused_rather_than_unwrapped_twice() {
    // A page can return any string it likes. Only a string that parses as our
    // own envelope is unwrapped; anything else is not an answer.
    let raw = serde_json::to_string("just some text").unwrap();
    let refused = envelope_value(&raw).unwrap_err();
    assert!(refused.contains("refused without saying why"), "{refused}");
}

// --- The URL bar's rule, reached through the host's one pass-through --------

#[test]
fn the_url_bar_refuses_a_search_phrase_rather_than_searching_for_it() {
    // Pinned here as well as in `cb_core::browser::url` because this
    // pass-through is what the command calls, and a host that "helpfully" fell
    // back to a search engine would be sending what the user typed — which may
    // be a password pasted into the wrong field — over the network.
    let refused = normalize("how do I fix this").unwrap_err();
    assert!(!refused.is_empty());
    assert_eq!(normalize("example.com").unwrap(), "https://example.com");
    let refused = normalize("javascript:alert(1)").unwrap_err();
    assert!(refused.contains("javascript"), "{refused}");
}
