//! Answering an agent's browser tool call: the gate, the projection, and the
//! dispatch.
//!
//! This is the application's half of [`cb_core::browser::wire`] — the shim owns
//! no browser state and phrases nothing, so a call arrives here as a
//! [`BrowserToolCall`] and leaves as a [`ToolAnswer`] whose words came from
//! [`cb_core::browser::render`].
//!
//! # What is decided here, and what is not
//!
//! Almost nothing is decided here, deliberately:
//!
//! * **whether** a call may run is [`cb_core::browser::consent::decide`],
//! * **what the refusal says** is that same function,
//! * **what an answer says** is [`cb_core::browser::render`],
//! * **which category a tool is in** is
//!   [`cb_core::browser::consent::BrowserTool::category`].
//!
//! Three things are genuinely this module's, and all three are free functions
//! with tests below rather than lines inside [`answer`]:
//!
//! * [`status_of`] — the projection from this host's state to the value
//!   `render::status` takes. The same collapse `shared::state_for` performs for
//!   the consent decision, and the same rule: the availability travels through
//!   verbatim, only the origin is derived.
//! * [`refusal_code`] — the machine-matchable code beside the prose. This is
//!   where the six states could quietly become one, so it is where they are
//!   pinned.
//! * [`gate`] — the two together.
//!
//! [`answer`] itself is an `async` mapping from an enum variant onto a handle
//! method and a renderer. It is not reachable from a test (it needs a webview),
//! which is exactly why it contains no `if`.

use cb_core::browser::consent::{self, BrowserTool, Category, Decision};
use cb_core::browser::render::{self, AgentStatus};
use cb_core::browser::tools::BrowserToolCall;
use cb_core::browser::wire::ToolAnswer;

use super::shared::{self, AgentRequest, BrowserShared};
use super::{BrowserHandle, PAGE_TEXT_LIMIT, READ_PAGE_LIMIT};

/// The program on the other end of the pipe.
///
/// Named by the OS, never by the caller — see [`AgentRequest::program`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub pid: u32,
    pub program: String,
}

/// What the application knows about its browser, as the value
/// [`render::status`] takes.
///
/// **The collapse point, and the same one `shared::state_for` guards.** The
/// tempting shortcut is to report `Ready` whenever there is a url, which would
/// describe a still-loading page and the browser's own error page as readable
/// ones. So the availability is carried through unchanged and only the origin
/// is derived.
pub fn status_of(data: &BrowserShared, pid: u32) -> AgentStatus {
    AgentStatus {
        availability: data.availability,
        url: data.url.clone(),
        title: data.title.clone(),
        origin: shared::current_origin(data),
        reads_allowed: data.consent.reads(),
        writes_allowed: data.consent.writes(),
        consent_origin: data.consent.origin().map(str::to_string),
        refused_navigations: data.refused_navigations,
        rejected_messages: data.rejected_messages,
        last_refusal: data.last_refusal.clone(),
        pid,
    }
}

/// The code that travels beside a refusal's prose.
///
/// A model branches on this without parsing sentences, so the distinctions the
/// prose keeps have to survive here too. The two groups are different fixes:
/// a **state** code is answered by doing something to the browser (switch the
/// plugin on, open the panel, load a page, wait), a **consent** code only by
/// the user clicking a control.
///
/// `consent_other_page` is separate from `consent_required` because the user
/// genuinely did click Allow and deserves to be told which page it was for —
/// and because an agent that reads "no permission" will ask for it again, while
/// one that reads "permission was for another page" knows the navigation is
/// what lost it.
pub fn refusal_code(call: &BrowserToolCall, data: &BrowserShared) -> &'static str {
    use cb_core::browser::model::BrowserAvailability as A;

    if !data.availability.licenses_a_read() {
        return match data.availability {
            A::PluginDisabled => "browser_plugin_off",
            A::PanelClosed => "browser_panel_closed",
            A::Blank => "browser_blank",
            A::Loading => "page_loading",
            A::Failed => "navigation_failed",
            // Unreachable: `licenses_a_read` is true for exactly this one.
            A::Ready => "browser_not_ready",
        };
    }
    let page_origin = shared::current_origin(data);
    match (page_origin.as_deref(), data.consent.origin()) {
        (None, _) => "no_page_origin",
        (Some(showing), Some(granted)) if showing != granted => "consent_other_page",
        _ => match call.tool().category() {
            Category::Write => "control_consent_required",
            _ => "consent_required",
        },
    }
}

/// May this call run? [`Err`] is the refusal to send back, already phrased.
///
/// The wording is [`consent::decide`]'s, so the sentence an agent reads and the
/// sentence the panel's own refusals use cannot drift; only the code is added
/// here.
pub fn gate(call: &BrowserToolCall, data: &BrowserShared) -> Result<(), ToolAnswer> {
    match consent::decide(call.tool(), &data.consent, &shared::state_for(data)) {
        Decision::Allow => Ok(()),
        Decision::Refuse { reason } => Err(ToolAnswer::refused(refusal_code(call, data), reason)),
    }
}

/// What to record about this request for the consent banner.
///
/// Recorded for allowed calls too: a banner that appeared only on a refusal
/// would tell the user an agent *wanted* to read their session and never that
/// one *is*.
pub fn request_record(call: &BrowserToolCall, peer: &Peer, refused: bool) -> AgentRequest {
    AgentRequest {
        pid: peer.pid,
        program: peer.program.clone(),
        tool: call.tool().name().to_string(),
        needs_writes: call.tool().category() == Category::Write,
        refused,
    }
}

/// Was the page the read *ran on* the page consent was held for?
///
/// The second half of the consent check, and it has to be a second half: the
/// gate releases the lock before the script is evaluated, so the document can
/// be replaced in between. The decision itself is
/// `cb_core::browser::consent::read_ran_on_the_granted_page`; this only reads
/// the grant out of the shared state and hands it over.
fn ran_on_the_granted_page(
    handle: &BrowserHandle,
    tool: BrowserTool,
    ran_on: Option<&str>,
) -> Result<(), String> {
    handle.read(|data| consent::read_ran_on_the_granted_page(tool, data.consent.origin(), ran_on))
}

/// Run one call and phrase its answer.
///
/// Contains no decision — every branch is one enum variant onto one handle
/// method onto one renderer — which is the only shape available to it, because
/// nothing in this function is reachable from a test.
pub async fn answer(handle: &BrowserHandle, call: BrowserToolCall, peer: &Peer) -> ToolAnswer {
    // Status is answered before the gate and before anything is recorded: it is
    // the tool whose job is reporting the state, it reveals only what a rogue
    // local process already learns by connecting, and a banner naming a program
    // that only asked "is a panel open" would be noise.
    if matches!(call, BrowserToolCall::Status) {
        let pid = std::process::id();
        return ToolAnswer::ok(render::status(&handle.read(|data| status_of(data, pid))));
    }

    let refusal = handle.write(|data| {
        let refusal = gate(&call, data).err();
        shared::note_agent_request(data, request_record(&call, peer, refusal.is_some()));
        refusal
    });
    if let Some(refusal) = refusal {
        return refusal;
    }

    let tool = call.tool();
    match call {
        // Answered above.
        BrowserToolCall::Status => unreachable!("status is answered before the gate"),
        BrowserToolCall::CurrentUrl => ToolAnswer::ok(
            handle.read(|data| render::current_url(data.url.as_deref(), data.title.as_deref())),
        ),
        BrowserToolCall::PageText => match handle.page_text(PAGE_TEXT_LIMIT).await {
            Ok(read) => match ran_on_the_granted_page(handle, tool, read.ran_on.as_deref()) {
                Ok(()) => ToolAnswer::ok(render::page_text(&read.text)),
                Err(reason) => ToolAnswer::refused("consent_other_page", reason),
            },
            Err(error) => page_failure(error),
        },
        BrowserToolCall::ReadPage => match handle.read_page(READ_PAGE_LIMIT).await {
            Ok(envelope) => {
                let ran_on = envelope.get("url").and_then(|v| v.as_str());
                match ran_on_the_granted_page(handle, tool, ran_on) {
                    Ok(()) => match render::outline(&envelope) {
                        Ok(text) => ToolAnswer::ok(text),
                        Err(error) => page_failure(error),
                    },
                    Err(reason) => ToolAnswer::refused("consent_other_page", reason),
                }
            }
            Err(error) => page_failure(error),
        },
        BrowserToolCall::ConsoleMessages { since } => {
            let batch = handle.read(|data| shared::console_batch(data, since));
            ToolAnswer::ok(render::console(
                &batch.entries,
                batch.missed,
                batch.discarded,
                batch.next_cursor,
            ))
        }
        BrowserToolCall::NetworkRequests { since } => {
            let batch = handle.read(|data| shared::network_batch(data, since));
            ToolAnswer::ok(render::network(
                &batch.entries,
                batch.missed,
                batch.discarded,
                batch.next_cursor,
            ))
        }
        BrowserToolCall::Navigate { url } => match super::normalize(&url) {
            Ok(target) => match handle.navigate(target.clone()).await {
                Ok(()) => ToolAnswer::ok(render::navigated(&target)),
                Err(error) => page_failure(error),
            },
            // The URL bar's own refusal, verbatim: a search phrase is refused
            // rather than searched, and the reason names why.
            Err(refusal) => ToolAnswer::refused("url_refused", refusal),
        },
        BrowserToolCall::Click { element } => match handle.click(&element).await {
            Ok(envelope) => ToolAnswer::ok(render::acted(
                &format!("Clicked {element}"),
                envelope.get("value").unwrap_or(&envelope),
            )),
            Err(error) => page_failure(error),
        },
        BrowserToolCall::Type { element, text } => match handle.type_text(&element, &text).await {
            Ok(envelope) => ToolAnswer::ok(render::acted(
                &format!("Typed into {element}"),
                envelope.get("value").unwrap_or(&envelope),
            )),
            Err(error) => page_failure(error),
        },
        BrowserToolCall::PressKey { key } => match handle.press_key(&key).await {
            Ok(envelope) => ToolAnswer::ok(render::acted(
                &format!("Pressed {key}"),
                envelope.get("value").unwrap_or(&envelope),
            )),
            Err(error) => page_failure(error),
        },
        BrowserToolCall::Back => match handle.go(-1).await {
            Ok(()) => ToolAnswer::ok(render::history_step("back")),
            Err(error) => page_failure(error),
        },
        BrowserToolCall::Forward => match handle.go(1).await {
            Ok(()) => ToolAnswer::ok(render::history_step("forward")),
            Err(error) => page_failure(error),
        },
        BrowserToolCall::Reload => match handle.reload().await {
            Ok(()) => ToolAnswer::ok(
                "Reload started. The page has not finished loading yet — call browser_status \
                 until it reports ready before reading anything."
                    .to_string(),
            ),
            Err(error) => page_failure(error),
        },
    }
}

/// A failure that came from the page or the webview rather than from a decision.
///
/// One code, because from an agent's point of view they are one situation — the
/// browser did not do the thing — and inventing sub-codes for wry's error text
/// would be classifying strings this side does not control.
fn page_failure(error: String) -> ToolAnswer {
    ToolAnswer::refused(
        "page_error",
        format!(
            "The browser could not complete that: {error}. Nothing about the page can be \
             concluded from this; call browser_status to see the current state."
        ),
    )
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
