//! The browser host's **data**: what the panel and an agent read, and every
//! rule for changing it.
//!
//! This is the half of the host that is not a webview. Nothing here touches
//! `wry`, a window or the main thread, which is the whole reason it exists as
//! its own file: the wry `WebView` is `!Send` and main-thread-affine, so it can
//! never be reached from a test, and every mistake the host could make about
//! *state* — a consent flag surviving a navigation, a page's own fabricated
//! message being applied, a stale url reported after a refused one, `Ready`
//! reported while a page is still loading — is a mistake about the contents of
//! this struct.
//!
//! So [`BrowserShared`] is a plain data struct behind an `Arc<Mutex<..>>`, every
//! change to it is a free function in this file, and `shared_tests.rs` covers
//! them with no webview in existence. See [`super`] for the host that owns the
//! webview and calls these.

use cb_core::browser::consent::{BrowserState, READ_CONSENT_ACTION, WRITE_CONSENT_ACTION};
use cb_core::browser::ipc::{PageMessage, PageMessageProblem};
use cb_core::browser::model::{
    AutomationConsent, BrowserAvailability, ConsoleEntry, ConsoleLevel, NetworkEntry,
    NETWORK_COVERAGE_NOTE,
};
use cb_core::browser::origin::{origin_of, Refusal};
use cb_core::browser::ring::{Ring, DEFAULT_CAPACITY};
use cb_core::browser::{console, ipc};
use serde::{Deserialize, Serialize};
use specta::Type;

/// A panel rect in **logical** pixels, relative to the window's client area.
///
/// Arrives from `browserPanelLogic.pageRect`, which has already rounded it and
/// refused the degenerate and scale-mismatched cases. It is validated again by
/// [`super::bounds_for`] regardless: the frontend is not a trusted validator of
/// numbers that become an OS surface's position, and a `NaN` here would place a
/// real window somewhere unpredictable.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserRect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// Everything the panel renders and an agent asks about, in one read.
///
/// Defined here rather than in `cb_core::browser::model` on the
/// `crate::commands::about::AboutInfo` precedent: it carries no decision the
/// core crate needs to make — it is a projection of this host's own state — and
/// the key-pinning test travels with it in `shared_tests.rs`. `src/ipc/types.ts`
/// mirrors it by hand like every other wire type; see
/// `docs/architecture/ipc-contract.md`.
///
/// No `skip_serializing_if`, the same rule as `cb_core::browser::model`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSnapshot {
    pub availability: BrowserAvailability,
    /// The address of the current page, from the navigation handler rather than
    /// `WebView::url()` — the spike measured `url()` returning an **empty
    /// string** while a document was loaded, so it is not a source of truth.
    pub url: Option<String>,
    pub title: Option<String>,
    /// The page's origin as `cb_core::browser::origin::origin_of` computes it;
    /// `None` for every opaque origin, which is what stops consent ever
    /// attaching to `about:blank`.
    pub origin: Option<String>,
    pub consent: AutomationConsent,
    /// How many navigations `navigation_verdict` refused. Surfaced because a
    /// page trying repeatedly to reach `tauri://localhost` is worth seeing.
    pub refused_navigations: u64,
    /// How many messages from the page were parsed and **not applied**. A
    /// rising count is a page probing this app's own message kinds.
    pub rejected_messages: u64,
    /// The most recent refusal — a navigation or a message — in words. `None`
    /// when nothing has been refused.
    pub last_refusal: Option<String>,
    /// The last program that asked an agent tool of this page. Drives the
    /// consent banner's wording; `None` when nothing has asked.
    pub last_agent_request: Option<AgentRequest>,
}

/// The last program that asked to read or drive this page, and what it asked
/// for.
///
/// This exists so the consent banner can say *who is asking* — "codex.cmd
/// (pid 12345) wants to read app.example.com" rather than "an agent wants to
/// read this page". A user granting access to their own logged-in session is
/// entitled to know which program will get it, and the alternative is a prompt
/// that is indistinguishable whichever local process triggered it.
///
/// The name comes from the connected pipe client's own process image
/// (`GetNamedPipeClientProcessId` then `QueryFullProcessImageNameW`), never
/// from anything the caller sent: a self-reported name would be the one field
/// a rogue caller would lie about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentRequest {
    pub pid: u32,
    /// The client executable's file name, e.g. `codex.cmd`. The file name
    /// rather than the full path: the path is long enough to push the rest of
    /// the banner off screen, and the name is what identifies the program.
    pub program: String,
    /// The MCP tool it called, e.g. `browser_page_text`.
    pub tool: String,
    /// Whether it asked for something that would **change** the page. The
    /// banner offers a different, stronger control for these.
    pub needs_writes: bool,
    /// Whether the request was refused for want of consent. `false` means it
    /// ran — so the banner shows recent activity rather than a prompt.
    pub refused: bool,
}

/// A page of captured console messages, with what the cursor missed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleBatch {
    pub entries: Vec<ConsoleEntry>,
    /// Entries after the requested cursor that this page's own log lost to the
    /// buffer cap. Never silently zero: the ring counts them.
    pub missed: u64,
    /// Entries after the requested cursor that belonged to a page that is gone
    /// and were discarded with it. A different fact from [`Self::missed`], and
    /// only `missed` says the record in front of the reader is partial.
    pub discarded: u64,
    pub next_cursor: u64,
}

/// A page of observed network rows, with what the instrumentation cannot see.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NetworkBatch {
    pub entries: Vec<NetworkEntry>,
    pub missed: u64,
    /// See [`ConsoleBatch::discarded`].
    pub discarded: u64,
    pub next_cursor: u64,
    /// `cb_core::browser::model::NETWORK_COVERAGE_NOTE`, carried on every
    /// answer. These rows are not DevTools' network panel and the difference
    /// has to reach the reader as data, not only as prose in a doc comment.
    pub coverage: String,
}

/// The host's data. Held as `Arc<Mutex<BrowserShared>>` in `AppState`.
#[derive(Debug)]
pub struct BrowserShared {
    pub availability: BrowserAvailability,
    pub url: Option<String>,
    pub title: Option<String>,
    pub consent: AutomationConsent,
    pub console: Ring<ConsoleEntry>,
    pub network: Ring<NetworkEntry>,
    pub refused_navigations: u64,
    pub rejected_messages: u64,
    pub last_refusal: Option<String>,
    /// The last agent request over the control pipe, for the consent banner.
    ///
    /// Cleared by [`reset_page`] with everything else a page owns: "codex.cmd
    /// wanted to read this page" is a statement about a page that is gone, and
    /// leaving it up would invite granting consent for a request nobody made
    /// about the page now on screen.
    pub last_agent_request: Option<AgentRequest>,
    /// Evaluations this host is waiting for a **page-side** reply to.
    ///
    /// Always empty, and that is the design rather than an omission. Every
    /// evaluation this host performs answers through wry's own
    /// `evaluate_script_with_callback`, which hands the result straight back to
    /// the caller's `oneshot` — there is no id to match and no window in which a
    /// reply could be waited for. So an `evalReply` arriving over the page's IPC
    /// channel is by construction unmatched, and
    /// `cb_core::browser::ipc::accept_eval_reply` refuses it against this empty
    /// slice. A page fabricating one gets a counted rejection.
    pub pending_eval: Vec<u64>,
}

impl Default for BrowserShared {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserShared {
    /// The resting state: nothing is open.
    ///
    /// Starts at [`BrowserAvailability::PanelClosed`] rather than
    /// `PluginDisabled`: this process cannot know at construction whether the
    /// user switched the plugin off, and claiming it did would be a confident
    /// wrong answer. `PanelClosed` is the true statement available with no
    /// information — the panel is not up — and `browser_close` sets
    /// `PluginDisabled` explicitly when the *feature* is what closed it.
    pub fn new() -> Self {
        Self {
            availability: BrowserAvailability::PanelClosed,
            url: None,
            title: None,
            consent: AutomationConsent::none(),
            console: Ring::new(DEFAULT_CAPACITY),
            network: Ring::new(DEFAULT_CAPACITY),
            refused_navigations: 0,
            rejected_messages: 0,
            last_refusal: None,
            last_agent_request: None,
            pending_eval: Vec::new(),
        }
    }
}

/// The origin of the current page, or `None`.
///
/// Reads the recorded url rather than asking the webview, for the reason
/// [`BrowserSnapshot::url`] gives.
pub fn current_origin(shared: &BrowserShared) -> Option<String> {
    shared.url.as_deref().and_then(origin_of)
}

/// What `cb_core::browser::consent::decide` is handed.
///
/// **The collapse point.** Six availabilities and an origin become the two
/// facts a consent decision needs, and the temptation is to answer
/// `BrowserState::ready(origin)` whenever there is a url — which would license a
/// read of a page that is still loading, or of the browser's own error page
/// after a failed navigation. So the availability is carried through verbatim
/// and only `origin` is derived.
pub fn state_for(shared: &BrowserShared) -> BrowserState {
    BrowserState {
        availability: shared.availability,
        origin: current_origin(shared),
    }
}

/// The whole readable state, in one value.
pub fn snapshot(shared: &BrowserShared) -> BrowserSnapshot {
    BrowserSnapshot {
        availability: shared.availability,
        url: shared.url.clone(),
        title: shared.title.clone(),
        origin: current_origin(shared),
        consent: shared.consent.clone(),
        refused_navigations: shared.refused_navigations,
        rejected_messages: shared.rejected_messages,
        last_refusal: shared.last_refusal.clone(),
        last_agent_request: shared.last_agent_request.clone(),
    }
}

/// The panel has just opened with no page in it.
///
/// Deliberately **not** a no-op when the availability is already something else:
/// re-opening the panel after it was closed must start from a blank page, not
/// from the last page's title and console.
pub fn note_opened(shared: &mut BrowserShared) {
    shared.availability = BrowserAvailability::Blank;
    reset_page(shared);
}

/// The panel closed, or the plugin was switched off and the webview dropped.
///
/// Clears the page **and the consent**. Consent is a statement about a page the
/// user was looking at; a closed panel is not showing it any more, and a
/// re-opened panel is a new session. The rings go too: they are a log of a page
/// that no longer exists, and serving them afterwards would attribute one
/// page's errors to the next.
pub fn note_closed(shared: &mut BrowserShared, availability: BrowserAvailability) {
    shared.availability = availability;
    reset_page(shared);
}

/// Discard both page logs, marking the page boundary.
///
/// `Ring::clear` keeps its sequence counter and records where the boundary
/// fell, so the discarded rows are reported to the next reader as belonging to
/// a page that is gone rather than as this page's buffer evictions - two facts
/// the renderer states separately.
fn clear_logs(shared: &mut BrowserShared) {
    shared.console.clear();
    shared.network.clear();
}

fn origin_of_current(shared: &BrowserShared) -> Option<String> {
    shared.url.as_deref().and_then(origin_of)
}

fn reset_page(shared: &mut BrowserShared) {
    shared.url = None;
    shared.title = None;
    shared.consent = AutomationConsent::none();
    clear_logs(shared);
    // A statement about a page that is gone. Left up, it would invite granting
    // consent for a request nobody made about the page now on screen.
    shared.last_agent_request = None;
}

/// A navigation this host allowed has started.
///
/// Three things happen and each is a rule:
///
/// * The availability becomes [`BrowserAvailability::Loading`], never `Ready`.
///   A page mid-load yields text that is *wrong rather than absent*, which is
///   the worse of the two failures and the reason `Loading` exists at all.
/// * Consent is re-derived through
///   [`AutomationConsent::after_origin_change`]: unchanged across a same-origin
///   navigation, **dropped** the moment the origin moves. Navigating never
///   grants consent and never renews it.
/// * The title is cleared. It belongs to the document being replaced, and a
///   minimized pill still labelled with the previous page is a lie the user
///   would act on.
pub fn note_navigation(shared: &mut BrowserShared, url: &str) {
    let previous = shared.url.as_deref().and_then(origin_of);
    let next = origin_of(url);
    // The rings belong to the document, so a different document gets empty
    // ones. `note_closed` states the rule for the identical data - "serving
    // them afterwards would attribute one page's errors to the next" - and
    // `Ring::clear`'s own doc says it is "used when the page navigates", but no
    // navigation path called it: a checkout page's `console.error("payment
    // declined")` was answered as the settings page's *complete* console
    // record. Cross-origin it was worse than misattribution: consent granted
    // for the page now on screen served the rows of a page consent was never
    // given for.
    //
    // Keyed on the url changing, not called unconditionally, because one user
    // navigation reaches this function up to three times (`load`, wry's
    // navigation handler, `PageLoadEvent::Started`) and the later fires would
    // discard rows the new document has already logged. A same-url reload
    // therefore keeps its predecessor's rows, which is the one case this does
    // not separate; the sequence numbers still order them, and clearing three
    // times per navigation would lose a real page's real console.
    if shared.url.as_deref() != Some(url) {
        clear_logs(shared);
    }
    shared.consent = shared.consent.after_origin_change(next.as_deref());
    // The banner record dies **exactly when consent dies**, and for the same
    // reason: both are statements about a page. So an origin change clears it
    // and a same-origin navigation does not — leaving "codex.cmd wants to read
    // <somewhere else>" above a control that would grant access to *this* page
    // is the one wording that could get a grant given for the wrong thing,
    // while dropping it on every path change would hide a request the user has
    // not answered yet.
    if previous != next {
        shared.last_agent_request = None;
    }
    shared.url = Some(url.to_owned());
    shared.title = None;
    shared.availability = BrowserAvailability::Loading;
}

/// A navigation `cb_core::browser::origin::navigation_verdict` refused.
///
/// Counts it and records why, and **does not touch the url**: the page the user
/// is looking at has not changed, so reporting the refused address as the
/// current one would be a wrong answer about what is on screen.
pub fn note_refused_navigation(shared: &mut BrowserShared, refusal: &Refusal) {
    shared.refused_navigations += 1;
    shared.last_refusal = Some(refusal.to_string());
}

/// A page finished loading.
///
/// `Ready` is the only availability that licenses a read, and this is the only
/// function that sets it.
///
/// # What this cannot tell you
///
/// It is reached from wry's `PageLoadEvent::Finished`, which fires for the
/// browser's own network-error page exactly as it does for the requested one —
/// wry exposes no `NavigationCompleted`/`IsSuccess`. So a page that failed to
/// resolve arrives here as `Ready` over an error document, and
/// [`BrowserAvailability::Failed`] is reserved for a navigation this app could
/// not even start (see [`note_failed`]). That is stated rather than papered
/// over: guessing failure from a document this host cannot read would be a
/// worse answer than reporting the error page it really is showing.
pub fn note_load_finished(shared: &mut BrowserShared, url: &str) {
    // The load-finished url is the authority on where the page ended up: a
    // redirect means the address that started the navigation is no longer it,
    // and consent must be re-checked against where it *landed*, not where it
    // was aimed.
    let next = origin_of(url);
    // Keyed on the **origin**, unlike `note_navigation`'s url check. At an
    // ordinary load-finish the held rows already belong to the document that
    // just loaded, so clearing them would throw away the current page's own
    // console. An origin that moved without a navigation event (an SSO bounce
    // seen only here) is the case where they do not, and there they are another
    // origin's rows - which must not survive into a grant made for this one.
    if origin_of_current(shared) != next {
        clear_logs(shared);
    }
    shared.consent = shared.consent.after_origin_change(next.as_deref());
    shared.url = Some(url.to_owned());
    // An opaque origin stays `Blank`, and that is not a detail. `about:blank`
    // really does finish loading, so reporting `Ready` for it is *true about
    // the load* and wrong about the panel - it says "the page has finished
    // loading" about a panel with nothing in it, and collapses `Blank` into
    // `Ready`, leaving the six states as five in practice. Found by driving the
    // real application and asking `browser_status`.
    //
    // Keyed on the origin rather than on the literal `about:blank`, because
    // every opaque origin is the same fact: a page nothing can be read from and
    // nothing can be granted for.
    shared.availability = match next {
        Some(_) => BrowserAvailability::Ready,
        None => BrowserAvailability::Blank,
    };
}

/// A navigation this app could not start (`load_url` itself failed).
pub fn note_failed(shared: &mut BrowserShared, reason: &str) {
    shared.availability = BrowserAvailability::Failed;
    shared.consent = AutomationConsent::none();
    shared.last_refusal = Some(reason.to_owned());
}

/// Apply one parsed message from the page.
///
/// Everything the page can say lands in a bounded ring or in a single field.
/// There is deliberately no branch that can reach the webview, the filesystem
/// or a process: this is the only bridge from remote content into this app, and
/// its whole capability is "append to a log, or set a title".
pub fn apply_page_message(shared: &mut BrowserShared, message: PageMessage) {
    match message {
        PageMessage::Console { method, text } => {
            // The page's own spelling is kept beside this app's ranking. An
            // unrecognised method is `Other`, never `Error`.
            let level = console::classify_level(&method);
            push_console(shared, level, method, text);
        }
        PageMessage::PageError { source, text } => {
            // `Error` by construction of the variant: the page genuinely threw,
            // whichever of the two window events reported it.
            push_console(shared, ConsoleLevel::Error, source, text);
        }
        PageMessage::Network {
            url,
            method,
            status,
            duration_ms,
            transfer_size,
            source,
        } => {
            let seq = next_seq(&shared.network);
            let given = shared.network.push(NetworkEntry {
                seq,
                url,
                method,
                status,
                duration_ms,
                transfer_size,
                source,
            });
            debug_assert_eq!(given, seq, "the ring's numbering changed under this file");
        }
        PageMessage::Title { text } => {
            shared.title = Some(text);
        }
        PageMessage::EvalReply { id, .. } => {
            // Refused against an always-empty pending set — see
            // `BrowserShared::pending_eval`. The core function is asked rather
            // than the refusal being written here, so the rule has one home.
            let problem = match ipc::accept_eval_reply(id, &shared.pending_eval) {
                Ok(_) => PageMessageProblem::UnknownEvalId { id },
                Err(problem) => problem,
            };
            note_page_problem(shared, &problem);
        }
    }
}

/// A message from the page that was parsed and not applied.
/// Record that an agent asked something of this page.
///
/// Deliberately records the **allowed** calls too, not only the refused ones.
/// A banner that appeared only on a refusal would tell the user an agent wanted
/// to read the page and never that one *is* reading it, and the second is the
/// more important fact about their own session.
pub fn note_agent_request(shared: &mut BrowserShared, request: AgentRequest) {
    shared.last_agent_request = Some(request);
}

pub fn note_page_problem(shared: &mut BrowserShared, problem: &PageMessageProblem) {
    shared.rejected_messages += 1;
    shared.last_refusal = Some(problem.to_string());
}

/// The sequence the next push into `ring` will take.
///
/// A row carries its own `seq` because `Slice` hands out `&T` and nothing else —
/// the field *is* how a reader learns the number to use as its next cursor. But
/// `Ring::push` mints the sequence and takes the entry by value, so the number
/// has to be known one instruction early. `latest_seq() + 1` is that number, and
/// every caller `debug_assert`s it against what `push` actually returned, so a
/// change to the ring's numbering surfaces here rather than as an off-by-one in
/// somebody's cursor.
fn next_seq<T>(ring: &Ring<T>) -> u64 {
    ring.latest_seq() + 1
}

fn push_console(shared: &mut BrowserShared, level: ConsoleLevel, method: String, text: String) {
    let seq = next_seq(&shared.console);
    let given = shared.console.push(ConsoleEntry {
        seq,
        level,
        method,
        text,
    });
    debug_assert_eq!(given, seq, "the ring's numbering changed under this file");
}

/// The console rows after `cursor`, and what the cursor missed.
pub fn console_batch(shared: &BrowserShared, cursor: u64) -> ConsoleBatch {
    let slice = shared.console.since(cursor);
    ConsoleBatch {
        entries: slice.entries.into_iter().cloned().collect(),
        missed: slice.missed,
        discarded: slice.discarded,
        next_cursor: slice.next_cursor,
    }
}

/// The network rows after `cursor`, what the cursor missed, and what the
/// instrumentation cannot see at all.
pub fn network_batch(shared: &BrowserShared, cursor: u64) -> NetworkBatch {
    let slice = shared.network.since(cursor);
    NetworkBatch {
        entries: slice.entries.into_iter().cloned().collect(),
        missed: slice.missed,
        discarded: slice.discarded,
        next_cursor: slice.next_cursor,
        coverage: NETWORK_COVERAGE_NOTE.to_owned(),
    }
}

/// Record a consent grant the user made in the panel's banner.
///
/// Refuses rather than storing something it cannot scope:
///
/// * **No origin** — a blank panel, anything opaque. There is nothing for the
///   grant to be *about*, and a grant with no origin would never be reset by a
///   navigation because there would be no origin to compare.
/// * **A page that is not `Ready`** — granting against a url that is still
///   loading would attach the grant to whatever the page turns out to be.
///
/// `writes` implies `reads` by construction: there are three constructors and
/// none produces write-without-read.
pub fn grant_consent(
    shared: &mut BrowserShared,
    reads: bool,
    writes: bool,
) -> Result<AutomationConsent, String> {
    if !reads && !writes {
        shared.consent = AutomationConsent::none();
        return Ok(shared.consent.clone());
    }
    if !shared.availability.licenses_a_read() {
        return Err(format!(
            "consent cannot be granted right now: {}",
            shared.availability.reason()
        ));
    }
    let Some(origin) = current_origin(shared) else {
        return Err(format!(
            "there is no origin to grant against, so neither {READ_CONSENT_ACTION:?} nor \
             {WRITE_CONSENT_ACTION:?} can be recorded: the panel is not showing an http or \
             https page"
        ));
    };
    shared.consent = if writes {
        AutomationConsent::reads_and_writes(origin)
    } else {
        AutomationConsent::reads_only(origin)
    };
    Ok(shared.consent.clone())
}

#[cfg(test)]
#[path = "shared_tests.rs"]
mod tests;
