//! Whether an agent may read, or move, the page the user is looking at.
//!
//! Modelled on `sql::store::SqlConnection::expose_to_agents` and sharper than
//! it in two ways, both forced by what a browser session is.
//!
//! # Both flags default to false, because reads are not free
//!
//! A database connection an agent may read is one the user created and named. A
//! browser page is whatever is on screen, which may be an authenticated admin
//! console, a password manager's vault, or a support ticket full of somebody
//! else's data. So the read flag is not a formality on the way to the
//! interesting one: it is the flag that matters, and it starts off.
//!
//! # Consent dies with the page
//!
//! "The agent may read this page" is a statement about a *page*, so it is
//! scoped to an origin and reset by an origin change — see
//! [`super::model::AutomationConsent::after_origin_change`]. Navigating never
//! grants it and never renews it.
//!
//! It is also never persisted. There is nothing stable to remember it *about*:
//! a browser session's identity is a cookie jar, and remembering a grant
//! against a bare origin would silently re-grant weeks later to a different
//! session with different cookies.
//!
//! # A refusal says what the user would click
//!
//! A tool that answers "not permitted" sends the agent into a loop of retries
//! and the user into the wrong settings page. Every refusal here names the
//! control — [`READ_CONSENT_ACTION`] or [`WRITE_CONSENT_ACTION`], the same
//! constants the banner is labelled with, so the message and the button cannot
//! drift apart.
//!
//! # Status is always allowed
//!
//! [`BrowserTool::Status`] answers even when the plugin is off, and that is
//! deliberate: it is the tool whose entire job is reporting which of the six
//! states this is, and gating it would make "off" indistinguishable from
//! "refused". It reveals only that the panel is or is not open, which is also
//! all a rogue local process gets from the pipe without a user click.

use super::model::{AutomationConsent, BrowserAvailability};

/// The label on the read-consent control, used verbatim in refusals.
pub const READ_CONSENT_ACTION: &str = "Allow agents to read this page";

/// The label on the write-consent control, used verbatim in refusals.
pub const WRITE_CONSENT_ACTION: &str = "Allow agents to read and control this page";

/// Every browser tool, and nothing else. An exhaustive enum rather than a
/// string, so [`category`](BrowserTool::category) is a total function and a new
/// tool cannot be added without deciding which side of the line it is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserTool {
    /// Which of the six states the browser is in. Always allowed.
    Status,
    // --- reads ---
    CurrentUrl,
    PageText,
    ReadPage,
    ConsoleMessages,
    NetworkRequests,
    Screenshot,
    // --- writes ---
    /// **A write, not a read.** Moving the page the user is looking at mutates
    /// state they can see, and does it in a window they may not be watching.
    Navigate,
    Click,
    Type,
    PressKey,
    Back,
    Forward,
    Reload,
}

/// What a tool needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Needs nothing, and answers in every state.
    Status,
    /// Needs read consent for the current origin.
    Read,
    /// Needs write consent for the current origin. Read consent comes with it
    /// structurally — see [`AutomationConsent`].
    Write,
}

impl BrowserTool {
    pub fn category(self) -> Category {
        match self {
            Self::Status => Category::Status,
            Self::CurrentUrl
            | Self::PageText
            | Self::ReadPage
            | Self::ConsoleMessages
            | Self::NetworkRequests
            | Self::Screenshot => Category::Read,
            Self::Navigate
            | Self::Click
            | Self::Type
            | Self::PressKey
            | Self::Back
            | Self::Forward
            | Self::Reload => Category::Write,
        }
    }

    /// The tool's MCP name, for a refusal an agent reads.
    pub fn name(self) -> &'static str {
        match self {
            Self::Status => "browser_status",
            Self::CurrentUrl => "browser_current_url",
            Self::PageText => "browser_page_text",
            Self::ReadPage => "browser_read_page",
            Self::ConsoleMessages => "browser_console_messages",
            Self::NetworkRequests => "browser_network_requests",
            Self::Screenshot => "browser_screenshot",
            Self::Navigate => "browser_navigate",
            Self::Click => "browser_click",
            Self::Type => "browser_type",
            Self::PressKey => "browser_press_key",
            Self::Back => "browser_back",
            Self::Forward => "browser_forward",
            Self::Reload => "browser_reload",
        }
    }
}

/// What the browser currently is, as far as a consent decision is concerned.
///
/// Deliberately tiny: an availability and an origin. It is a *value*, computed
/// by the host and handed in, which is what makes every decision here provable
/// with no webview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserState {
    pub availability: BrowserAvailability,
    /// The current page's origin, as [`super::origin::origin_of`] computes it.
    ///
    /// `None` for every opaque origin — `about:blank`, a failed navigation —
    /// which is why a blank panel can never carry consent.
    pub origin: Option<String>,
}

impl BrowserState {
    /// The resting state: the plugin is off and there is no page.
    pub fn plugin_disabled() -> Self {
        Self {
            availability: BrowserAvailability::PluginDisabled,
            origin: None,
        }
    }

    pub fn ready(origin: impl Into<String>) -> Self {
        Self {
            availability: BrowserAvailability::Ready,
            origin: Some(origin.into()),
        }
    }
}

/// The answer to "may this tool run right now".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Refused, with a sentence naming both the cause and — where the user can
    /// change it — the control they would use.
    Refuse {
        reason: String,
    },
}

impl Decision {
    pub fn allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }

    /// The refusal text, or `None` when allowed.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Allow => None,
            Self::Refuse { reason } => Some(reason),
        }
    }
}

/// Did a read actually run on the page consent was granted for?
///
/// A **second** check, after [`decide`], and it exists because the first one
/// cannot be atomic with the read. The gate is evaluated under the host's
/// shared mutex, the lock is released, and only then does the injected script
/// hop to the main thread and execute - so a `meta refresh` or a scripted
/// `location.replace` can replace the document in between. `note_navigation`
/// correctly drops consent when that happens, but the decision is already
/// behind us, and the answer named no address at all, so neither the agent nor
/// the user could tell the text came from a page nothing was granted for.
///
/// The evidence was already being produced and discarded: `page_text_script`
/// and `read_page_script` both return their own `location.href`.
///
/// Two abstentions, both deliberate:
///
/// * **`observed` is `None`** - the page reported no address (a torn-down
///   document, or `WebView::url()`'s empty string for a `data:` URL). Refused,
///   for [`decide`]'s reason: an unknown address cannot be compared to a grant,
///   and treating it as matching would make one grant cover every page.
/// * **`granted` is `None`** - allowed, because this is not the function that
///   decides whether consent exists. [`decide`] has already refused that case
///   with the right words, and a second refusal here would report the wrong
///   reason for it.
pub fn read_ran_on_the_granted_page(
    tool: BrowserTool,
    granted: Option<&str>,
    observed: Option<&str>,
) -> Result<(), String> {
    let Some(granted) = granted else {
        return Ok(());
    };
    let Some(observed) = observed else {
        return Err(format!(
            "{} was not answered: consent is held for {granted}, and the page could not say what \
             address the read ran on, so there is no way to tell whether it was that page.",
            tool.name()
        ));
    };
    match super::origin::origin_of(observed) {
        Some(origin) if origin == granted => Ok(()),
        Some(origin) => Err(format!(
            "{} was not answered: consent is held for {granted}, but the read ran on {origin} — \
             the page navigated between the permission check and the read. Nothing from it is \
             reported. Call browser_status and grant consent again if {origin} is the page you \
             meant.",
            tool.name()
        )),
        None => Err(format!(
            "{} was not answered: consent is held for {granted}, and the read ran on {observed}, \
             which has no comparable origin.",
            tool.name()
        )),
    }
}

/// Decide whether `tool` may run.
pub fn decide(tool: BrowserTool, consent: &AutomationConsent, state: &BrowserState) -> Decision {
    let category = tool.category();
    if category == Category::Status {
        // The one tool whose job is reporting the state. Gating it would make
        // "off" indistinguishable from "refused".
        return Decision::Allow;
    }

    if !state.availability.licenses_a_read() {
        return Decision::Refuse {
            reason: format!(
                "{} cannot run: {}",
                tool.name(),
                state.availability.reason()
            ),
        };
    }

    // Ready with no origin should not happen, but the two facts arrive
    // separately and this is the one combination where guessing would be
    // dangerous: an unknown origin cannot be compared to a grant, so treating
    // it as matching would make one grant cover every page.
    let Some(page_origin) = state.origin.as_deref() else {
        return Decision::Refuse {
            reason: format!(
                "{} cannot run: the page's origin could not be determined, and consent is granted per origin",
                tool.name()
            ),
        };
    };

    match consent.origin() {
        // Nothing granted at all — the default, and the ordinary case.
        None => {
            return Decision::Refuse {
                reason: needs_consent(tool, category, page_origin),
            };
        }
        // A grant for a different origin. Named explicitly rather than reported
        // as "no consent", because the user *did* click Allow and deserves to
        // be told which page it was for.
        Some(granted) if granted != page_origin => {
            return Decision::Refuse {
                reason: format!(
                    "{} cannot run: consent was granted for {granted}, and the panel is now showing {page_origin}. Consent is per page, so it did not carry over — grant it again with \"{}\" if this is the page you meant.",
                    tool.name(),
                    action_for(category)
                ),
            };
        }
        Some(_) => {}
    }

    let permitted = match category {
        Category::Status => true,
        Category::Read => consent.reads(),
        Category::Write => consent.writes(),
    };
    if permitted {
        Decision::Allow
    } else {
        Decision::Refuse {
            reason: needs_consent(tool, category, page_origin),
        }
    }
}

fn needs_consent(tool: BrowserTool, category: Category, origin: &str) -> String {
    match category {
        Category::Status => String::new(),
        Category::Read => format!(
            "{} needs your permission to read {origin}. Nothing was read. Click \"{READ_CONSENT_ACTION}\" in the browser panel to allow it; the permission applies to this page only and is not remembered.",
            tool.name()
        ),
        Category::Write => format!(
            "{} would change the page you are looking at, so it needs your permission to control {origin}. Nothing was changed. Click \"{WRITE_CONSENT_ACTION}\" in the browser panel to allow it; the permission applies to this page only and is not remembered.",
            tool.name()
        ),
    }
}

fn action_for(category: Category) -> &'static str {
    match category {
        Category::Write => WRITE_CONSENT_ACTION,
        _ => READ_CONSENT_ACTION,
    }
}

#[cfg(test)]
#[path = "consent_tests.rs"]
mod tests;
