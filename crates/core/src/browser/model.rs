//! The browser types that cross IPC, and the distinctions they exist to keep.
//!
//! `src/ipc/types.ts` mirrors these by hand, like every other wire type in this
//! crate — there is no codegen — and the key-pinning tests at the bottom are
//! what stops the two drifting. See `docs/architecture/ipc-contract.md`.
//!
//! There is **no `skip_serializing_if` anywhere in this file**, the same rule as
//! [`crate::dap::model`] and [`crate::lsp::model`]. An absent key and a key
//! holding `null` are different things to a TypeScript reader, and every field
//! here is read by the UI or by an agent to decide what to do next.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Where the embedded browser is — **six answers, never collapsed into one**.
///
/// They are six because each licenses something different, and because an agent
/// asking `browser_status` acts on the difference:
///
/// * [`Self::PluginDisabled`] — the user switched the browser plugin off. There
///   is no WebView2 process, no cookie jar and no connection. Nothing to do
///   except say so; the fix is a setting.
/// * [`Self::PanelClosed`] — the plugin is on and the panel is not up. One click
///   away, which is a materially different answer from the one above and the
///   reason collapsing the two would be a real loss.
/// * [`Self::Blank`] — a panel with no page (`about:blank`). Not an error, and
///   not something a read can be attempted against.
/// * [`Self::Loading`] — a page whose text would be **wrong, not absent**. This
///   is the variant that exists so a half-rendered read is never served as a
///   finished one, exactly like [`crate::lsp::model::Availability::Loading`].
/// * [`Self::Ready`] — the only outcome that licenses a page read.
/// * [`Self::Failed`] — a navigation that did not land. Distinct from `Blank`
///   because the user asked for something and did not get it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BrowserAvailability {
    PluginDisabled,
    PanelClosed,
    Blank,
    Loading,
    Ready,
    Failed,
}

impl BrowserAvailability {
    /// Whether a page read could possibly be correct right now.
    ///
    /// Only [`Self::Ready`]. `Loading` deliberately answers `false`: the text is
    /// obtainable and would be wrong, which is the worse of the two failures.
    pub fn licenses_a_read(self) -> bool {
        matches!(self, Self::Ready)
    }

    /// A phrase for the user, or for a refusal an agent will read. Names the
    /// state's own cause; never a generic "unavailable".
    pub fn reason(self) -> &'static str {
        match self {
            Self::PluginDisabled => {
                "the browser plugin is switched off, so no page is loaded and no browser process is running"
            }
            Self::PanelClosed => {
                "the browser plugin is on but its panel is closed, so there is no page"
            }
            Self::Blank => "the browser panel is open with no page loaded",
            Self::Loading => {
                "the page is still loading, so anything read from it now would be incomplete rather than missing"
            }
            Self::Ready => "the page has finished loading",
            Self::Failed => {
                "the last navigation failed, so the panel is not showing the requested page"
            }
        }
    }
}

/// How a captured console message was ranked.
///
/// [`Self::Other`] is the abstention and it is load-bearing: an unrecognised
/// `console` method (`console.table`, `console.group`, something a framework
/// monkey-patched on) is **not** an error, and ranking it as one would put a red
/// row in front of the user for a call that printed a table. See
/// [`super::console::classify_level`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ConsoleLevel {
    Debug,
    Log,
    Info,
    Warn,
    Error,
    Other,
}

/// One captured console message.
///
/// `seq` is the ring's monotonic sequence number, not an index: it is what a
/// reader passes back as a cursor, and what makes a dropped entry detectable
/// (see [`super::ring`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleEntry {
    pub seq: u64,
    pub level: ConsoleLevel,
    /// The `console` method as the page spelled it (`"warn"`, `"table"`), or
    /// `"onerror"` / `"unhandledrejection"` for the two window events that are
    /// captured but are not console calls at all.
    ///
    /// Kept beside [`Self::level`] rather than replaced by it, because the level
    /// is this app's ranking and the method is the page's own word. An `Other`
    /// row is only readable at all because this field says what it was.
    pub method: String,
    pub text: String,
}

/// How one network row was observed.
///
/// This exists because the rows are **not** DevTools' network panel and the
/// difference has to survive as far as the reader. wry exposes no CDP, so a row
/// comes either from a monkey-patched `fetch`/`XMLHttpRequest` (which can report
/// a status) or from a `PerformanceObserver` over `resource` entries (which
/// cannot, and never sees headers or a body).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum NetworkSource {
    Fetch,
    Xhr,
    Resource,
}

/// What the instrumentation can see at all, stated to the reader verbatim.
///
/// Every network answer carries this. A row list that looks like DevTools' and
/// is silently missing every request made before the init script ran, every
/// header and every body is worse than one that says what it is.
pub const NETWORK_COVERAGE_NOTE: &str = "instrumented: rows come from a patched fetch/XMLHttpRequest plus a PerformanceObserver over resource timings. There are no headers and no bodies, statuses exist only for fetch/XHR rows, and a request issued before the page's init script ran is not here at all.";

/// One observed network request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEntry {
    pub seq: u64,
    pub url: String,
    /// `GET` for a `resource`-timing row, where the real method is not knowable
    /// — which is why [`Self::source`] travels beside it.
    pub method: String,
    /// **Read this against [`Self::source`]; alone it names two different
    /// facts.** `None` on a `resource` row is resource timing having no status
    /// to give. `None` on a `fetch`/`xhr` row is a request that **failed
    /// before a response**: the init script's fetch rejection handler posts
    /// `null`, and an errored XHR's zero status becomes `null` too
    /// (`self.status || null`). `Some(0)` is reachable only from the fetch
    /// *success* path (`(response && response.status) || 0`), so it is a
    /// resolved **opaque** response - `no-cors` or an opaque redirect - which
    /// completed. An earlier version of this comment claimed the opposite, and
    /// `super::render::network` labelled the rows accordingly, so a working
    /// beacon read as a failure and a failure read as a resource-timing
    /// limitation.
    pub status: Option<u16>,
    pub duration_ms: Option<f64>,
    pub transfer_size: Option<u64>,
    pub source: NetworkSource,
}

/// Text extracted from the page, and **what was left out**.
///
/// [`Self::total_chars`] is the whole page's length, never the returned slice's.
/// A cap that also caps the number is a lie about the page, and an agent reading
/// `returnedChars == totalChars` is the only way it can know it has all of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageText {
    pub text: String,
    /// The full page's length in `char`s. Unaffected by truncation.
    pub total_chars: usize,
    /// The length of [`Self::text`] in `char`s.
    pub returned_chars: usize,
    pub truncated: bool,
}

/// What an agent may do with the page the user is looking at.
///
/// # Why the fields are private
///
/// `writes && !reads` is **unrepresentable**, not merely unlikely. There are
/// exactly three constructors and none of them produces it, and deserialisation
/// goes through [`ConsentWire`] so the shape cannot arrive over a wire either.
/// A validating setter would have left every future call site one forgotten
/// check away from a browser an agent can click but not read — which is the
/// worst combination available, since a blind click is the one action whose
/// effect nobody can predict.
///
/// # Why there is an origin, and why it is not persisted
///
/// "The agent may read this page" is a statement about a *page*. So consent is
/// scoped to an origin and reset the moment the origin changes — navigating
/// never grants and never renews it.
///
/// It is not persisted at all. A browser session's identity is a cookie jar, not
/// a named connection the user created (which is what makes
/// `sql::store::SqlConnection::expose_to_agents` a reasonable thing to
/// remember), so there is nothing stable to remember consent *about*, and
/// remembering it against a bare origin would silently re-grant it weeks later
/// against a different session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", try_from = "ConsentWire")]
pub struct AutomationConsent {
    reads: bool,
    writes: bool,
    /// The origin the flags were granted against, `None` when nothing is
    /// granted. Compared by [`super::consent::decide`] against the page's
    /// current origin.
    origin: Option<String>,
}

/// The deserialisation shadow for [`AutomationConsent`].
///
/// Public because `serde(try_from)` names it in generated code. Nothing should
/// construct one deliberately — use the three constructors.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsentWire {
    #[serde(default)]
    pub reads: bool,
    #[serde(default)]
    pub writes: bool,
    #[serde(default)]
    pub origin: Option<String>,
}

impl TryFrom<ConsentWire> for AutomationConsent {
    type Error = &'static str;

    fn try_from(wire: ConsentWire) -> Result<Self, Self::Error> {
        if wire.writes && !wire.reads {
            return Err("write consent without read consent is not a state this browser has: a blind click is the one action whose effect nobody can predict");
        }
        match (wire.reads, wire.origin) {
            (false, _) => Ok(Self::none()),
            (true, Some(origin)) if wire.writes => Ok(Self::reads_and_writes(origin)),
            (true, Some(origin)) => Ok(Self::reads_only(origin)),
            (true, None) => Err("read consent must name the origin it was granted against"),
        }
    }
}

impl Default for AutomationConsent {
    fn default() -> Self {
        Self::none()
    }
}

impl AutomationConsent {
    /// Nothing granted. **The default**, and deliberately so: reads are not
    /// free, because the panel may be showing an authenticated admin page.
    pub fn none() -> Self {
        Self {
            reads: false,
            writes: false,
            origin: None,
        }
    }

    /// The user allowed this agent to *read* `origin`. The ordinary grant.
    pub fn reads_only(origin: impl Into<String>) -> Self {
        Self {
            reads: true,
            writes: false,
            origin: Some(origin.into()),
        }
    }

    /// The user allowed reading **and** driving `origin`. There is no
    /// writes-without-reads constructor, on purpose.
    pub fn reads_and_writes(origin: impl Into<String>) -> Self {
        Self {
            reads: true,
            writes: true,
            origin: Some(origin.into()),
        }
    }

    pub fn reads(&self) -> bool {
        self.reads
    }

    pub fn writes(&self) -> bool {
        self.writes
    }

    pub fn origin(&self) -> Option<&str> {
        self.origin.as_deref()
    }

    /// The consent that survives the page becoming `new_origin`.
    ///
    /// Nothing, unless the origin is unchanged. Navigation is not a grant and is
    /// not a *renewal* either: the page the user was looking at when they
    /// clicked Allow is gone.
    pub fn after_origin_change(&self, new_origin: Option<&str>) -> Self {
        match (self.origin.as_deref(), new_origin) {
            (Some(held), Some(now)) if held == now => self.clone(),
            _ => Self::none(),
        }
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
