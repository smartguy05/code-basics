//! Where the page may never go, and what counts as "the same page".
//!
//! Two separate jobs that share the parsing:
//!
//! * [`navigation_verdict`] is the **defence in depth** behind
//!   `with_navigation_handler`. wry's handler returns a `bool`, and returning
//!   `false` cancels the navigation, so this function is the only thing standing
//!   between a redirect chain and this application's own surfaces. It is not the
//!   only protection — a wry-created child gets no `__TAURI_INTERNALS__` at all
//!   (see [`super`]) — but a layer whose failure is silent deserves a second one.
//! * [`origin_of`] and [`same_origin`] are what [`super::consent`] compares, so
//!   that a grant dies when the page it was about does.
//!
//! # Why the dev server is on the refusal list
//!
//! `http://localhost:1420` is the Vite dev server, and `webview/mod.rs:1698`
//! classes it as a **local** origin, which is exactly the classification that
//! decides whether Tauri's IPC is reachable. In a release build there is no such
//! server and the entry is inert; in development it is the one address whose
//! contents are this application's own UI. Refusing it means a page that
//! redirects there gets a cancelled navigation rather than the app's own DOM
//! rendered inside the browser panel — which would put the real UI, with a real
//! `__TAURI_INTERNALS__`, inside a frame a remote page just navigated.
//!
//! # Refusal is by scheme *or* by origin, and both are needed
//!
//! Refusing the app's origins alone would leave `file:` and `data:` open;
//! refusing schemes alone would leave `http://tauri.localhost` open, since its
//! scheme is the ordinary `http`. So there are two lists and a URL must clear
//! both.

use thiserror::Error;

/// This application's own origins. A page may never navigate to one.
///
/// `tauri.localhost` appears under both schemes because the custom-protocol
/// host is reachable either way depending on the `withGlobalTauri`/protocol
/// configuration, and `localhost:1420` is the dev server (see the module doc).
pub const APP_ORIGINS: [&str; 5] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
    "http://localhost:1420",
    "http://127.0.0.1:1420",
];

/// Schemes a navigation may use. Everything else is refused, including every
/// custom scheme — an allowlist, because the set of interesting schemes on a
/// Windows machine (`ms-settings:`, `search-ms:`, a vendor's protocol handler)
/// is unbounded and each one hands control to something outside this app.
const ALLOWED_SCHEMES: [&str; 2] = ["http", "https"];

/// Whether a navigation may proceed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationVerdict {
    Allow,
    /// Cancel it, with the reason. The reason is shown in the panel rather than
    /// dropped: a navigation that silently does nothing looks like a hung page.
    Refuse(Refusal),
}

impl NavigationVerdict {
    /// What `with_navigation_handler` should return.
    pub fn allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

/// Why a navigation was cancelled. Each variant carries the specific thing that
/// was wrong, because "blocked" alone is indistinguishable from a broken panel.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Refusal {
    #[error(
        "{origin} is one of this application's own origins, so the browser panel will not load it"
    )]
    AppOrigin { origin: String },
    #[error("the {scheme} scheme is not loaded in the browser panel")]
    Scheme { scheme: String },
    #[error("only about:blank is available; {url} is a browser-internal page")]
    AboutPage { url: String },
    #[error("{url} could not be read as an http or https address")]
    Unreadable { url: String },
}

/// Decide whether the page may navigate to `url`.
///
/// `app_origins` is a parameter rather than a constant read so a test can prove
/// the refusal happens *because* the origin is on the list, and so a host that
/// learns its real dev-server port at runtime can pass it in. Pass
/// [`APP_ORIGINS`] in production.
pub fn navigation_verdict(url: &str, app_origins: &[&str]) -> NavigationVerdict {
    let trimmed = url.trim();

    // `about:blank` is the panel's own empty state and the one exception.
    if trimmed.eq_ignore_ascii_case("about:blank") {
        return NavigationVerdict::Allow;
    }
    if let Some(rest) = strip_scheme_ci(trimmed, "about") {
        return NavigationVerdict::Refuse(Refusal::AboutPage {
            url: format!("about:{rest}"),
        });
    }

    let Some(scheme) = scheme_of(trimmed) else {
        // No parseable scheme at all. Refused rather than assumed: this
        // function is handed whatever the page navigated to, including a
        // relative or malformed string, and inventing `https` for it would be
        // deciding a destination on a page's behalf.
        return NavigationVerdict::Refuse(Refusal::Unreadable {
            url: trimmed.to_owned(),
        });
    };
    if !ALLOWED_SCHEMES.contains(&scheme.as_str()) {
        return NavigationVerdict::Refuse(Refusal::Scheme { scheme });
    }

    let Some(origin) = origin_of(trimmed) else {
        return NavigationVerdict::Refuse(Refusal::Unreadable {
            url: trimmed.to_owned(),
        });
    };
    if app_origins
        .iter()
        .any(|app| origins_equal(app, origin.as_str()))
    {
        return NavigationVerdict::Refuse(Refusal::AppOrigin { origin });
    }
    NavigationVerdict::Allow
}

/// The origin of a URL: `scheme://host[:port]`, lowercased, with a default port
/// removed.
///
/// `None` for anything without a scheme and a host — including `about:blank`,
/// `data:` and `file:`, all of which have **opaque** origins on the web and must
/// not be given a comparable one here. That is what makes [`same_origin`]
/// refuse them rather than match them to each other.
pub fn origin_of(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let at = trimmed.find(':')?;
    let scheme = trimmed[..at].to_ascii_lowercase();
    if !scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
    {
        return None;
    }
    if !scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    let rest = trimmed[at + 1..].strip_prefix("//")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // Userinfo is not part of an origin, and leaving it in would make
    // `https://user@a.example` and `https://a.example` different origins — so a
    // grant would silently not apply after a login redirect stripped it.
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if host_port.is_empty() {
        return None;
    }
    let (host, port) = split_host_port(host_port);
    if host.is_empty() {
        return None;
    }
    let host = host.to_ascii_lowercase();
    match port {
        Some(port) if !is_default_port(&scheme, port) => Some(format!("{scheme}://{host}:{port}")),
        _ => Some(format!("{scheme}://{host}")),
    }
}

/// Whether two URLs are the same origin.
///
/// **False whenever either origin is opaque** ([`origin_of`] answered `None`).
/// That is the web's own rule and the safe abstention: two `data:` URLs are not
/// the same origin as each other, and treating them as one would let a page
/// carry a consent grant across a navigation the user would never recognise.
pub fn same_origin(a: &str, b: &str) -> bool {
    match (origin_of(a), origin_of(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

fn origins_equal(a: &str, b: &str) -> bool {
    match (origin_of(a), origin_of(b)) {
        (Some(a), Some(b)) => a == b,
        // An entry on the app-origin list that does not parse as an origin
        // (`tauri://localhost` parses fine; a typo would not) falls back to a
        // case-insensitive literal comparison rather than silently matching
        // nothing.
        _ => a.eq_ignore_ascii_case(b),
    }
}

fn scheme_of(url: &str) -> Option<String> {
    let at = url.find(':')?;
    let scheme = &url[..at];
    let mut chars = scheme.chars();
    if !chars.next()?.is_ascii_alphabetic() {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }
    Some(scheme.to_ascii_lowercase())
}

fn strip_scheme_ci<'a>(url: &'a str, scheme: &str) -> Option<&'a str> {
    let at = url.find(':')?;
    if url[..at].eq_ignore_ascii_case(scheme) {
        Some(&url[at + 1..])
    } else {
        None
    }
}

/// Split an authority into host and port, keeping a bracketed IPv6 literal
/// whole — `[::1]:3000` splits after the bracket, not at the first colon.
fn split_host_port(host_port: &str) -> (&str, Option<&str>) {
    if let Some(close) = host_port.find(']') {
        let (host, rest) = host_port.split_at(close + 1);
        return (host, rest.strip_prefix(':'));
    }
    match host_port.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (host_port, None),
    }
}

fn is_default_port(scheme: &str, port: &str) -> bool {
    matches!((scheme, port), ("http", "80") | ("https", "443"))
}

#[cfg(test)]
#[path = "origin_tests.rs"]
mod tests;
