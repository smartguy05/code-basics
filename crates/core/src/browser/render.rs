//! Turning what the application knows into the words an agent reads.
//!
//! The application composes every answer (see [`super::wire`] for why the shim
//! is given prose rather than state), and this is where that prose is decided —
//! in the core crate, with no webview and no pipe, so all of it is provable.
//!
//! # The rule that shapes every function here
//!
//! **An answer must never let an absence pass for a fact about the page.** The
//! same rule as [`crate::lsp::model`]'s six availabilities, and it bites in five
//! specific places:
//!
//! * A read that returned nothing says *nothing was captured*, and separately
//!   says whether entries were **evicted** — a full ring and an empty one are
//!   different claims about the page.
//! * Truncated text reports the page's real length, so `returned == total` is
//!   the only thing that means "all of it".
//! * Every network answer repeats [`super::model::NETWORK_COVERAGE_NOTE`],
//!   because a list that looks like DevTools' and silently has no headers, no
//!   bodies and nothing from before the init script is worse than one that says
//!   what it is.
//! * [`status`] states the consent position **explicitly**, including when
//!   nothing is granted, so an agent knows to ask rather than inferring from a
//!   refusal it has not hit yet.
//! * `browser_back` and `browser_forward` report what they *asked for*, never
//!   that it happened: wry 0.55 has no `go_back`, so this goes through
//!   `history.back()` and a page with nothing behind it does nothing and reports
//!   success. Saying "went back" there would be a confident wrong answer.

use serde_json::Value;

use super::model::{
    BrowserAvailability, ConsoleEntry, ConsoleLevel, NetworkEntry, NetworkSource, PageText,
    NETWORK_COVERAGE_NOTE,
};

/// What the application knows about its browser, as a value.
///
/// A projection of the host's own state, built by the bridge exactly as
/// `shared::state_for` builds a [`super::consent::BrowserState`] — so this
/// module needs no webview and the bridge needs no prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentStatus {
    pub availability: BrowserAvailability,
    pub url: Option<String>,
    pub title: Option<String>,
    pub origin: Option<String>,
    pub reads_allowed: bool,
    pub writes_allowed: bool,
    /// The origin consent was granted against, which is **not** necessarily
    /// [`Self::origin`]: a grant for another page is reported as what it is.
    pub consent_origin: Option<String>,
    pub refused_navigations: u64,
    pub rejected_messages: u64,
    pub last_refusal: Option<String>,
    /// The process id of the application answering, so an agent that hit an
    /// ambiguity refusal can name this window with `--instance`.
    pub pid: u32,
}

/// The `browser_status` answer.
///
/// Leads with the state's own sentence rather than a code, because the six
/// states are the whole content of this tool: each licenses something different
/// and each has a different fix.
pub fn status(status: &AgentStatus) -> String {
    let mut lines = vec![format!(
        "code-basics (pid {}) — {}.",
        status.pid,
        status.availability.reason()
    )];

    match (&status.url, &status.title) {
        (Some(url), Some(title)) if !title.is_empty() => {
            lines.push(format!("Page: {title} — {url}"));
        }
        (Some(url), _) => lines.push(format!("Page: {url}")),
        (None, _) => lines.push(
            "There is no page address, so nothing can be read and nothing can be granted \
             permission: permission is per page."
                .to_string(),
        ),
    }

    lines.push(consent_line(status));

    if status.refused_navigations > 0 {
        lines.push(format!(
            "{} navigation(s) have been refused in this panel — the page tried to reach somewhere \
             this browser does not go.",
            status.refused_navigations
        ));
    }
    if status.rejected_messages > 0 {
        lines.push(format!(
            "{} message(s) from the page were parsed and not applied, which is a page probing \
             this application's own message kinds.",
            status.rejected_messages
        ));
    }
    if let Some(refusal) = &status.last_refusal {
        lines.push(format!("Most recent refusal: {refusal}"));
    }

    lines.join("\n")
}

/// The consent sentence, stated in all three positions rather than only when it
/// blocks something.
fn consent_line(status: &AgentStatus) -> String {
    match (
        status.reads_allowed,
        status.writes_allowed,
        status.consent_origin.as_deref(),
        status.origin.as_deref(),
    ) {
        (false, _, _, _) => format!(
            "Permission: none. Nothing on this page can be read or changed until the user clicks \
             \"{}\" in the browser panel. Ask them; do not retry.",
            super::consent::READ_CONSENT_ACTION
        ),
        // The case that would otherwise read as a plain grant: consent exists,
        // for a page that is no longer the one on screen.
        (true, _, Some(granted), Some(showing)) if granted != showing => format!(
            "Permission: granted for {granted}, but the panel is now showing {showing}. \
             Permission is per page and did not carry over, so reads and writes here are \
             refused until it is granted again."
        ),
        (true, true, Some(granted), _) => format!(
            "Permission: read and control, for {granted} only. Both reads and the state-changing \
             tools will run against this page."
        ),
        (true, false, Some(granted), _) => format!(
            "Permission: read only, for {granted}. Reads will run; the state-changing tools \
             (navigate, click, type, press key, back, forward, reload) are refused until the user \
             clicks \"{}\".",
            super::consent::WRITE_CONSENT_ACTION
        ),
        // Read consent with no origin is unrepresentable in
        // `AutomationConsent`; reported rather than assumed away, because if it
        // ever arrives, treating it as a grant would make one grant cover every
        // page.
        (true, _, None, _) => "Permission: recorded as granted but with no page named, which \
             cannot be checked against the page on screen. Reads and writes are refused. This is \
             a bug worth reporting."
            .to_string(),
    }
}

/// The `browser_current_url` answer.
pub fn current_url(url: Option<&str>, title: Option<&str>) -> String {
    match (url, title) {
        (Some(url), Some(title)) if !title.is_empty() => format!("{url}\nTitle: {title}"),
        (Some(url), _) => format!("{url}\nTitle: the page has not set one."),
        (None, _) => "The panel has no page address.".to_string(),
    }
}

/// The `browser_page_text` answer.
///
/// The counts come first, so a truncated read cannot be quoted as the whole
/// page by something that stopped reading at the text.
pub fn page_text(text: &PageText) -> String {
    let header = if text.truncated {
        format!(
            "The top document holds {} characters and this is the first {}. It is CUT: \
             do not treat what follows as the whole page. {FRAME_SCOPE_NOTE}",
            text.total_chars, text.returned_chars
        )
    } else {
        format!(
            "The top document's rendered text in full, {} characters (nothing was cut). \
             {FRAME_SCOPE_NOTE}",
            text.total_chars
        )
    };
    if text.text.trim().is_empty() {
        return format!(
            "{header}\n\nThe page rendered no text at all. That is what the page shows, not a \
             failure to read it — a page can be entirely images, canvas or an empty frame."
        );
    }
    format!("{header}\n\n{}", text.text)
}

/// The `browser_read_page` answer, from the outline script's envelope.
///
/// Refuses rather than invents when the envelope is not the declared shape: an
/// outline is what `browser_click` addresses elements by, so a fabricated one
/// would produce clicks on nothing.
pub fn outline(envelope: &Value) -> Result<String, String> {
    let rows = envelope
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            "the page's outline did not arrive as a list of elements, so no element references \
             can be offered"
                .to_string()
        })?;
    let total = envelope.get("total").and_then(Value::as_u64);
    let truncated = envelope
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if rows.is_empty() {
        return Ok(
            "No interactive or landmark elements were found in the page. A page can \
             legitimately have none — everything drawn on a canvas, or inside a cross-origin \
             frame this browser cannot see into."
                .to_string(),
        );
    }

    let mut lines = Vec::with_capacity(rows.len() + 3);
    // Three answers, never two. `truncated` is the cap alone (see
    // `script::read_page_script`); a shorter list with the cap *not* reached is
    // the visibility filter, which is a fact about the page rather than about
    // this read, and printing "the list is CUT ... narrow the page" over a
    // complete outline is how an honesty signal becomes noise.
    let skipped = total.map_or(0, |total| total.saturating_sub(rows.len() as u64));
    match (total, truncated) {
        (Some(total), true) => lines.push(format!(
            "{} of {total} elements. The list is CUT: the rest are not here, and an element you \
             need may be among them — narrow the page rather than assuming these are all.",
            rows.len()
        )),
        (Some(_), false) if skipped > 0 => lines.push(format!(
            "{} elements. {skipped} more matched but are not rendered (a zero-sized box: \
             `display:none`, a collapsed menu, a hidden input), so they are not listed and \
             cannot be clicked.",
            rows.len()
        )),
        (Some(total), false) => lines.push(format!("{total} elements.")),
        (None, _) => lines.push(format!("{} elements.", rows.len())),
    }
    // The scope, on every outline rather than only on an empty one:
    // `querySelectorAll` is the **top document's**.
    lines.push(FRAME_SCOPE_NOTE.to_string());

    for row in rows {
        let element = row.get("ref").and_then(Value::as_str).unwrap_or("?");
        let role = row.get("role").and_then(Value::as_str).unwrap_or("unknown");
        let name = row.get("name").and_then(Value::as_str).unwrap_or("");
        let kind = row.get("type").and_then(Value::as_str);
        let disabled = row
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut line = format!("{element}  {role}");
        if let Some(kind) = kind {
            line.push_str(&format!("[{kind}]"));
        }
        if name.is_empty() {
            // Said rather than left blank: an unnamed control is a real finding
            // — it is what a screen reader would also fail on.
            line.push_str("  (no accessible name)");
        } else {
            line.push_str(&format!("  {name:?}"));
        }
        if disabled {
            line.push_str("  DISABLED");
        }
        lines.push(line);
    }
    Ok(lines.join("\n"))
}

/// The `browser_console_messages` answer.
pub fn console(entries: &[ConsoleEntry], missed: u64, discarded: u64, next_cursor: u64) -> String {
    let mut lines = Vec::with_capacity(entries.len() + 3);
    lines.push(gap_line(
        entries.len(),
        missed,
        discarded,
        "console message",
    ));
    for entry in entries {
        lines.push(format!(
            "[{}] {} {}",
            entry.seq,
            level_word(entry.level, &entry.method),
            entry.text
        ));
    }
    lines.push(cursor_line(next_cursor));
    lines.join("\n")
}

/// The level as a word, keeping the page's **own** method name beside this
/// application's ranking.
///
/// An `Other` row is only readable because the method is carried: `console.table`
/// is not an error and ranking it as one would put a red row in front of the
/// user for a call that printed a table.
fn level_word(level: ConsoleLevel, method: &str) -> String {
    let ranked = match level {
        ConsoleLevel::Debug => "debug",
        ConsoleLevel::Log => "log",
        ConsoleLevel::Info => "info",
        ConsoleLevel::Warn => "warn",
        ConsoleLevel::Error => "error",
        ConsoleLevel::Other => "unranked",
    };
    if ranked == method {
        format!("{ranked}:")
    } else {
        format!("{ranked}({method}):")
    }
}

/// The `browser_network_requests` answer. Always carries the coverage note.
pub fn network(entries: &[NetworkEntry], missed: u64, discarded: u64, next_cursor: u64) -> String {
    let mut lines = Vec::with_capacity(entries.len() + 4);
    lines.push(NETWORK_COVERAGE_NOTE.to_string());
    lines.push(gap_line(entries.len(), missed, discarded, "request"));
    for entry in entries {
        // Read against the **source**, because the two facts are produced by
        // different halves of `script::init_script` and neither label is true
        // of the other half. A rejected fetch posts `status: null`, and an
        // errored XHR's zero status becomes null too; `0` is reachable only
        // from the fetch *success* path (`(response && response.status) || 0`),
        // i.e. a resolved opaque `no-cors` or opaque-redirect response. So the
        // source-blind wording had the two labels effectively swapped: a
        // completed opaque response read as a failure, and a genuine failure
        // read as a resource-timing limitation - in the same line that says it
        // was observed via a patched fetch.
        let status = match (entry.source, entry.status) {
            (NetworkSource::Fetch | NetworkSource::Xhr, None) => {
                "no status (the request failed before a response)".to_string()
            }
            (NetworkSource::Fetch | NetworkSource::Xhr, Some(0)) => {
                "status 0 (an opaque response — `no-cors` or an opaque redirect; the request \
                 completed but its status is not visible to the page)"
                    .to_string()
            }
            (NetworkSource::Resource, None) => {
                "no status (resource timing cannot see one)".to_string()
            }
            (_, Some(code)) => format!("status {code}"),
        };
        let duration = match entry.duration_ms {
            Some(ms) => format!("{ms:.0}ms"),
            None => "duration unknown".to_string(),
        };
        let size = match entry.transfer_size {
            Some(bytes) => format!("{bytes} bytes"),
            None => "size unknown".to_string(),
        };
        lines.push(format!(
            "[{}] {} {} — {status}, {duration}, {size}, observed via {}",
            entry.seq,
            entry.method,
            entry.url,
            source_word(entry.source)
        ));
    }
    lines.push(cursor_line(next_cursor));
    lines.join("\n")
}

fn source_word(source: NetworkSource) -> &'static str {
    match source {
        NetworkSource::Fetch => "a patched fetch",
        NetworkSource::Xhr => "a patched XMLHttpRequest",
        NetworkSource::Resource => "resource timing (no method, status, headers or body)",
    }
}

/// What a read of the page could not see, stated on every read of it.
///
/// `document.body.innerText` and `document.querySelectorAll` are the **top
/// document's**. Content in a nested document is not a descendant of that tree,
/// so it was never in scope - cross-origin or not. Without this clause "the
/// whole page, 214 characters (nothing was cut)" is a completeness claim over
/// the shell chrome of a dashboard whose content is an iframe, and an agent
/// concludes the string it was looking for is not on the page. The module rule
/// applied to a scope rather than to a cap: an absence must never pass for a
/// fact about the page.
const FRAME_SCOPE_NOTE: &str = "Content inside a nested frame is not included and was never in \
     scope for this read; if the page hosts a frame, what you want may be in it.";

/// The count line, and the eviction line when there was one.
///
/// **The two are never merged.** "Nothing was captured" and "some of what was
/// captured has been thrown away" are different claims, and an agent reading
/// the first when the second is true concludes the page is quiet.
fn gap_line(returned: usize, missed: u64, discarded: u64, noun: &str) -> String {
    let mut said = if returned == 0 {
        format!("No {noun}s have been captured since the cursor.")
    } else {
        format!("{returned} {noun}(s).")
    };
    // Stated before the eviction line and never merged with it. A page the
    // panel is no longer showing is not this page's record, and only an
    // eviction from *this* page's log says what is in front of the reader is
    // partial. `Ring::clear` keeps the sequence counter, so these entries do
    // sit above the cursor and cannot simply be left unmentioned either.
    if discarded > 0 {
        said.push_str(&format!(
            " {discarded} earlier {noun}(s) belonged to a page that is no longer open and were \
             discarded with it. They are not this page's record."
        ));
    }
    if missed > 0 {
        said.push_str(&format!(
            " {missed} earlier {noun}(s) were evicted from the buffer before this read and are \
             gone - this is not a quiet page, it is a partial record."
        ));
    }
    said
}

fn cursor_line(next_cursor: u64) -> String {
    format!("Pass since={next_cursor} to read only what arrives after this.")
}

/// The answer for a state-changing tool that reports what it *asked for*.
///
/// `outcome` is the script's own `value`, which names the element it acted on,
/// so a caller can check it was the one it meant.
pub fn acted(what: &str, outcome: &Value) -> String {
    format!("{what}. The page reported: {outcome}")
}

/// The `browser_navigate` answer.
///
/// Says the navigation **started**, because the page has not loaded yet and
/// claiming it has is the mistake `BrowserAvailability::Loading` exists to stop.
/// Also states the consent consequence, which is the part an agent will
/// otherwise be surprised by.
pub fn navigated(url: &str) -> String {
    format!(
        "Navigation to {url} started. The page has not finished loading yet — call browser_status \
         until it reports ready before reading anything.\nThe permission the user granted was for \
         the previous page, so it does not apply here: reads and writes on the new page need it \
         granted again."
    )
}

/// The answer for `browser_back` / `browser_forward`.
pub fn history_step(direction: &str) -> String {
    format!(
        "Asked the page to go {direction}. Whether anything happened is not observable from here: \
         a page with nothing {direction} in its history does nothing and reports no error. Call \
         browser_current_url to see where the panel actually is."
    )
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
