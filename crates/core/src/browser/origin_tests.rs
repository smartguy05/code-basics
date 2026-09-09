//! Navigation refusals and origin comparison. Included by `origin.rs`.

use super::*;

fn verdict(url: &str) -> NavigationVerdict {
    navigation_verdict(url, &APP_ORIGINS)
}

fn refusal(url: &str) -> Refusal {
    match verdict(url) {
        NavigationVerdict::Refuse(r) => r,
        NavigationVerdict::Allow => panic!("{url:?} should have been refused"),
    }
}

// ---------------------------------------------------------------------------
// The app's own origins
// ---------------------------------------------------------------------------

#[test]
fn every_app_origin_is_refused() {
    for origin in APP_ORIGINS {
        match refusal(origin) {
            Refusal::AppOrigin { .. } | Refusal::Scheme { .. } => {}
            other => panic!("{origin:?} -> {other:?}"),
        }
    }
}

#[test]
fn the_dev_server_is_refused_because_it_is_this_apps_own_ui() {
    // `webview/mod.rs:1698` classes localhost:1420 as a *local* origin, which is
    // the classification that decides whether Tauri's IPC is reachable. A page
    // that redirects there would otherwise render the real UI inside the panel.
    match refusal("http://localhost:1420/") {
        Refusal::AppOrigin { origin } => assert_eq!(origin, "http://localhost:1420"),
        other => panic!("{other:?}"),
    }
    assert!(!verdict("http://localhost:1420/index.html").allowed());
    assert!(!verdict("http://127.0.0.1:1420/").allowed());
}

#[test]
fn tauri_localhost_is_refused_under_both_schemes() {
    // Its scheme is the ordinary `http`, so a scheme allowlist alone would let
    // it through. This is why there are two lists.
    assert!(!verdict("http://tauri.localhost/").allowed());
    assert!(!verdict("https://tauri.localhost/x").allowed());
    match refusal("http://TAURI.LOCALHOST/") {
        Refusal::AppOrigin { .. } => {}
        other => panic!("host comparison must be case-insensitive: {other:?}"),
    }
}

#[test]
fn a_default_port_written_out_does_not_evade_the_app_origin_list() {
    // `http://tauri.localhost:80` is the same origin as `http://tauri.localhost`
    // and would sail past a literal string comparison.
    assert!(!verdict("http://tauri.localhost:80/").allowed());
    assert!(!verdict("https://tauri.localhost:443/").allowed());
}

#[test]
fn userinfo_does_not_evade_the_app_origin_list() {
    // Userinfo is not part of an origin, so it must not create a new one.
    assert!(!verdict("http://anything@tauri.localhost/").allowed());
    assert!(!verdict("http://user:pass@localhost:1420/").allowed());
}

#[test]
fn a_different_localhost_port_is_allowed_because_that_is_the_users_deployment() {
    // The whole point of the panel. Refusing all of loopback would break it.
    assert!(verdict("http://localhost:5173/").allowed());
    assert!(verdict("http://127.0.0.1:8080/health").allowed());
}

#[test]
fn the_refusal_list_is_a_parameter_so_the_reason_is_provable() {
    // With an empty list the same URL is allowed, which proves the refusal above
    // came from the list and not from an unrelated rule.
    assert!(navigation_verdict("http://tauri.localhost/", &[]).allowed());
    assert!(!navigation_verdict("http://tauri.localhost/", &["http://tauri.localhost"]).allowed());
}

// ---------------------------------------------------------------------------
// Schemes
// ---------------------------------------------------------------------------

#[test]
fn every_dangerous_scheme_is_refused_naming_itself() {
    for (url, scheme) in [
        ("file:///C:/Windows/win.ini", "file"),
        ("javascript:alert(1)", "javascript"),
        ("data:text/html,<script>alert(1)</script>", "data"),
        ("blob:https://example.com/abc", "blob"),
        ("vbscript:msgbox(1)", "vbscript"),
        ("ms-settings:privacy", "ms-settings"),
        ("search-ms:query=secret", "search-ms"),
        ("ftp://example.com/", "ftp"),
        ("ws://example.com/", "ws"),
        ("myapp+launch://do-something", "myapp+launch"),
    ] {
        match refusal(url) {
            Refusal::Scheme { scheme: got } => assert_eq!(got, scheme, "for {url:?}"),
            other => panic!("{url:?} -> {other:?}"),
        }
    }
}

#[test]
fn a_custom_scheme_is_refused_by_allowlist_not_by_a_blocklist() {
    // The set of protocol handlers on a Windows machine is unbounded, so the
    // check must be "is it http(s)", never "is it one of the bad ones".
    for url in [
        "zoommtg://zoom.us/join?confno=1",
        "steam://run/440",
        "slack://channel?id=C1",
        "totally-made-up-scheme://x",
    ] {
        assert!(!verdict(url).allowed(), "{url:?}");
    }
}

#[test]
fn a_scheme_is_refused_however_it_is_cased() {
    assert!(!verdict("JavaScript:alert(1)").allowed());
    assert!(!verdict("FILE:///c:/x").allowed());
    assert!(verdict("HTTPS://example.com/").allowed());
}

#[test]
fn about_blank_is_allowed_and_every_other_about_page_is_not() {
    assert!(verdict("about:blank").allowed());
    assert!(verdict("ABOUT:BLANK").allowed());
    for url in ["about:config", "about:version", "about:srcdoc"] {
        match refusal(url) {
            Refusal::AboutPage { .. } => {}
            other => panic!("{url:?} -> {other:?}"),
        }
    }
}

#[test]
fn an_unreadable_url_is_refused_rather_than_given_a_scheme() {
    // This function is handed whatever the page navigated to, including a
    // relative or malformed string. Inventing `https` for it would be choosing a
    // destination on the page's behalf.
    for url in ["", "/orders/3", "example.com", "://x", "http:/example.com"] {
        match refusal(url) {
            Refusal::Unreadable { .. } | Refusal::Scheme { .. } => {}
            other => panic!("{url:?} -> {other:?}"),
        }
    }
}

#[test]
fn an_ordinary_page_is_allowed() {
    for url in [
        "https://example.com/",
        "https://app.example.com/orders?id=3#top",
        "http://staging.internal:8080/",
        "https://[2606:4700::1111]/",
    ] {
        assert!(verdict(url).allowed(), "{url:?} should be allowed");
    }
}

// ---------------------------------------------------------------------------
// origin_of
// ---------------------------------------------------------------------------

#[test]
fn origin_of_keeps_scheme_host_and_a_non_default_port() {
    assert_eq!(
        origin_of("https://app.example.com/orders?id=3#x").as_deref(),
        Some("https://app.example.com")
    );
    assert_eq!(
        origin_of("http://localhost:5173/").as_deref(),
        Some("http://localhost:5173")
    );
}

#[test]
fn origin_of_drops_a_default_port_and_lowercases_the_host() {
    assert_eq!(
        origin_of("HTTPS://App.Example.COM:443/x").as_deref(),
        Some("https://app.example.com")
    );
    assert_eq!(
        origin_of("http://example.com:80").as_deref(),
        Some("http://example.com")
    );
}

#[test]
fn origin_of_keeps_a_bracketed_ipv6_literal_whole() {
    // Splitting at the first colon would produce the origin `https://[`.
    assert_eq!(
        origin_of("https://[::1]/x").as_deref(),
        Some("https://[::1]")
    );
    assert_eq!(
        origin_of("http://[::1]:3000/x").as_deref(),
        Some("http://[::1]:3000")
    );
}

#[test]
fn origin_of_is_none_for_every_opaque_origin() {
    // On the web these have opaque origins. Giving them a comparable one here
    // would let a consent grant survive a navigation the user cannot recognise.
    for url in [
        "about:blank",
        "data:text/html,x",
        "file:///C:/x",
        "javascript:alert(1)",
        "",
        "not a url",
        "/relative/path",
    ] {
        assert_eq!(origin_of(url), None, "{url:?}");
    }
}

// ---------------------------------------------------------------------------
// same_origin
// ---------------------------------------------------------------------------

#[test]
fn same_origin_ignores_path_query_and_fragment() {
    assert!(same_origin(
        "https://app.example.com/a?x=1#y",
        "https://app.example.com/b"
    ));
}

#[test]
fn same_origin_separates_scheme_host_and_port() {
    assert!(!same_origin(
        "https://a.example.com/",
        "http://a.example.com/"
    ));
    assert!(!same_origin(
        "https://a.example.com/",
        "https://b.example.com/"
    ));
    assert!(!same_origin(
        "http://localhost:5173/",
        "http://localhost:5174/"
    ));
}

#[test]
fn two_opaque_origins_are_never_the_same_origin_as_each_other() {
    // The web's own rule, and the safe abstention. Matching them would carry a
    // grant across `about:blank` into anything.
    assert!(!same_origin("about:blank", "about:blank"));
    assert!(!same_origin("data:text/html,a", "data:text/html,a"));
    assert!(!same_origin("file:///x", "file:///x"));
    assert!(!same_origin("https://a.example.com/", "about:blank"));
}
