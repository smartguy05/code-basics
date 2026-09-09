//! What the URL bar accepts, and — mostly — what it refuses.
//!
//! # This is not a search box
//!
//! The single most consequential decision in this module is that a **search
//! phrase is refused rather than searched**. Every real browser sends what it
//! cannot parse to a search engine, and copying that here would mean a mistyped
//! internal hostname, a pasted fragment of a log line, or a half-typed
//! connection string leaving the machine as a query to a third party — a guess
//! with a network request attached, made on the user's behalf, silently. The
//! panel exists to look at a deployment, so the refusal names the problem and
//! the user can paste a URL or type a search engine's own address themselves.
//!
//! # The two conveniences that *are* offered
//!
//! Both are inferences from the text alone, both are reversible by typing a
//! scheme, and neither reaches the network to decide:
//!
//! * A bare host that looks like a hostname becomes `https`. Defaulting to
//!   `http` would silently downgrade every address a user pastes without a
//!   scheme.
//! * `localhost:5173`, `127.0.0.1:8080` and `[::1]:3000` become `http`,
//!   because a development server is overwhelmingly plain HTTP and an `https`
//!   guess there fails with a TLS error that names nothing useful.
//!
//! Note that a loopback address is normalised here and *refused* by
//! [`super::origin::navigation_verdict`] when it is one of the app's own
//! origins. The two are deliberately separate: this module decides what the
//! text meant, that one decides whether the page may go there.

use thiserror::Error;

/// Why a typed address was not turned into a URL.
///
/// Every variant names the input's actual problem. There is no catch-all,
/// because "invalid URL" is exactly the message that makes a user retype the
/// same thing.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UrlError {
    #[error("type an address to load")]
    Empty,

    /// A scheme this browser will not navigate to.
    ///
    /// **Names the scheme.** `javascript:` in particular is the one a user can
    /// be talked into pasting, and it executes in whatever page is loaded — so
    /// the refusal has to say which word was the problem, or it reads as a bug.
    #[error("{scheme}: addresses are not loaded in this browser ({why})")]
    RefusedScheme { scheme: String, why: String },

    /// It is not an address at all.
    ///
    /// The refusal that replaces a search. Says so explicitly, because a user
    /// who typed a phrase expects *something* to happen and needs to know that
    /// nothing did, and why.
    #[error("{input:?} is not an address, and this browser does not search: paste a URL, or type a search engine's own address")]
    NotAnAddress { input: String },

    /// A control character or whitespace inside what is otherwise a URL.
    ///
    /// Refused rather than stripped: a `\n` or a `%0a` smuggled into a pasted
    /// address is not a typo to be tidied up.
    #[error("the address contains a control character or an embedded space, so it was not loaded")]
    Malformed,

    /// An address with a scheme but no host (`https://`, `http:///path`).
    #[error("{input:?} names a scheme but no host")]
    NoHost { input: String },
}

/// The only schemes this browser will navigate to at all, plus `about:blank`
/// which is handled separately because it has no host.
const ALLOWED_SCHEMES: [&str; 2] = ["http", "https"];

/// Turn what the user typed into a URL to load, or say why not.
///
/// Never reaches the network, never consults a search engine, and never returns
/// something the user did not ask for.
pub fn normalize_input(input: &str) -> Result<String, UrlError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(UrlError::Empty);
    }
    // Control characters anywhere, and whitespace *inside*, are refused rather
    // than stripped. A pasted address containing a newline is a smuggling
    // attempt or a broken copy; neither should be silently repaired.
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(UrlError::Malformed);
    }

    if let Some((scheme, rest)) = split_scheme(trimmed) {
        let lower = scheme.to_ascii_lowercase();
        if lower == "about" {
            // `about:blank` is the only one: it is the panel's own empty state.
            // Every other `about:` page is a browser-internal surface this app
            // does not host and cannot vouch for.
            return if rest.eq_ignore_ascii_case("blank") {
                Ok("about:blank".to_owned())
            } else {
                Err(UrlError::RefusedScheme {
                    scheme: lower,
                    why: "only about:blank is available in this panel".to_owned(),
                })
            };
        }
        if !ALLOWED_SCHEMES.contains(&lower.as_str()) {
            return Err(UrlError::RefusedScheme {
                scheme: lower.clone(),
                why: refusal_reason(&lower).to_owned(),
            });
        }
        // An http(s) URL. It must have a host: `https://` alone, or a triple
        // slash, is a scheme with nothing behind it.
        let after_slashes = rest.strip_prefix("//").unwrap_or(rest);
        let host = after_slashes
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default();
        if host.is_empty() {
            return Err(UrlError::NoHost {
                input: trimmed.to_owned(),
            });
        }
        // Whitespace inside a URL is refused. (Leading and trailing were
        // trimmed above; a space in the middle means this was probably a
        // sentence that happened to start with a scheme.)
        if trimmed.chars().any(char::is_whitespace) {
            return Err(UrlError::Malformed);
        }
        return Ok(format!("{lower}:{rest}"));
    }

    // No scheme. Whitespace now settles it: an address has none, and a phrase
    // almost always does.
    if trimmed.chars().any(char::is_whitespace) {
        return Err(UrlError::NotAnAddress {
            input: trimmed.to_owned(),
        });
    }

    let host = trimmed
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    if is_loopback_host(&host) {
        return Ok(format!("http://{trimmed}"));
    }
    if looks_like_a_hostname(&host) {
        return Ok(format!("https://{trimmed}"));
    }
    Err(UrlError::NotAnAddress {
        input: trimmed.to_owned(),
    })
}

/// Split `scheme:rest` when the text really does begin with a scheme.
///
/// # The ambiguity, and how it is settled
///
/// `localhost:5173` and `javascript:alert(1)` have the same shape: an RFC 3986
/// scheme name, a colon, and something. The scheme grammar alone does not
/// separate them — `localhost` is a perfectly legal scheme name — so reading
/// the grammar and stopping there classifies every `host:port` the user types
/// as an unknown scheme and refuses it. That was the first version and
/// `a_host_with_a_port_is_not_read_as_a_scheme` is the test that caught it.
///
/// Two signals settle it, in order:
///
/// 1. **`//` after the colon means a scheme**, always. That is the hierarchical
///    form and no `host:port` produces it.
/// 2. Otherwise, **a body that is a bare port is a port**: all digits up to the
///    first `/`, `?` or `#`. `5173` and `8443/x` are ports; `alert(1)`,
///    `text/html,…` and `blank` are not.
///
/// # The one thing this gets wrong, and why it is safe
///
/// An opaque scheme whose body happens to be entirely numeric — `tel:12345` —
/// is read as a host and a port. It is then refused as
/// [`UrlError::NotAnAddress`] rather than [`UrlError::RefusedScheme`], because
/// `tel` is not a hostname. So the *message* is less precise; the outcome is
/// still a refusal, nothing is loaded, and
/// [`super::origin::navigation_verdict`] refuses the scheme a second time
/// regardless. Pinned by `an_opaque_scheme_with_a_numeric_body_is_still_refused`.
fn split_scheme(text: &str) -> Option<(&str, &str)> {
    let at = text.find(':')?;
    let (scheme, rest) = (&text[..at], &text[at + 1..]);
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }
    if !rest.starts_with("//") && is_bare_port(rest) {
        return None;
    }
    Some((scheme, rest))
}

/// Whether what follows the colon is a port rather than a scheme's body.
///
/// An empty body is **not** a port: `https:` is a scheme with nothing behind it
/// and must reach the `NoHost` refusal rather than being read as a host called
/// `https`.
fn is_bare_port(rest: &str) -> bool {
    let port = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !port.is_empty() && port.chars().all(|c| c.is_ascii_digit())
}

/// Why a scheme is refused, in the user's terms. One sentence each, because
/// this reaches a tooltip and an agent's error string.
fn refusal_reason(scheme: &str) -> &'static str {
    match scheme {
        "javascript" => {
            "it would run script in whatever page is currently loaded, which is how a pasted address becomes an attack"
        }
        "data" => "it renders attacker-controlled content under an opaque origin",
        "file" => "this browser does not read the local filesystem; open files in the editor",
        "blob" => "a blob URL only exists inside the page that created it",
        "vbscript" => "it would run script in whatever page is currently loaded",
        "tauri" | "asset" | "http+tauri" | "https+tauri" => {
            "it is one of this application's own internal schemes"
        }
        _ => "only http and https pages are loaded here",
    }
}

/// Whether this host is the machine itself.
///
/// Deliberately a small exact list rather than a subnet test. `0.0.0.0` is not
/// here — it is a bind address, not an address to fetch — and neither is any
/// private range, which is ordinary `https` infrastructure.
fn is_loopback_host(host: &str) -> bool {
    let bare = host.rsplit_once(':').map_or(host, |(h, _)| h);
    matches!(bare, "localhost" | "127.0.0.1" | "[::1]" | "::1") || bare.ends_with(".localhost")
}

/// Whether a bare string looks like a hostname worth prefixing with `https`.
///
/// Requires a dot with something either side, or a bracketed IPv6 literal. That
/// is what separates `example.com` from `budget`, and refusing the second is the
/// whole point of the module — an unqualified single label is far more often a
/// search phrase than an intranet host, and a user who means the intranet host
/// can type `http://budget/`.
fn looks_like_a_hostname(host: &str) -> bool {
    // A bracketed IPv6 literal is unambiguous, so it needs none of the label
    // reasoning below. A *bare* one (`::1`, `fe80::1`) is deliberately refused:
    // it is indistinguishable from a host with a port, and guessing which
    // would produce a request to somewhere the user did not name.
    if host.starts_with('[') {
        return host.contains(']');
    }
    let bare = host.rsplit_once(':').map_or(host, |(h, _)| h);
    if bare.is_empty() || bare.starts_with('.') || bare.ends_with('.') {
        return false;
    }
    let Some((label, tld)) = bare.rsplit_once('.') else {
        return false;
    };
    if label.is_empty() || tld.is_empty() {
        return false;
    }
    bare.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        && tld.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[cfg(test)]
#[path = "url_tests.rs"]
mod tests;
