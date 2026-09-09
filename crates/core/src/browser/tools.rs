//! The thirteen browser tools, their schemas, and how a call is read.
//!
//! Two categories, and the line between them is [`super::consent::Category`]
//! rather than anything decided here — [`BrowserToolCall::tool`] maps every
//! call onto a [`super::consent::BrowserTool`], so a tool cannot be added
//! without deciding which side of the line it is on.
//!
//! | tool | needs | answers |
//! | --- | --- | --- |
//! | `browser_status` | nothing | which of the six states the browser is in |
//! | `browser_current_url` | read consent | the address and title of the page |
//! | `browser_page_text` | read consent | the rendered text, and what was cut |
//! | `browser_read_page` | read consent | a role/name/`ref` outline of the elements |
//! | `browser_console_messages` | read consent | captured console output |
//! | `browser_network_requests` | read consent | observed requests, with their coverage |
//! | `browser_navigate` | **write** consent | move the page |
//! | `browser_click` | write consent | click one element by `ref` |
//! | `browser_type` | write consent | type into one element by `ref` |
//! | `browser_press_key` | write consent | send one key to the focused element |
//! | `browser_back` / `_forward` / `_reload` | write consent | move in history |
//!
//! # `browser_status` always answers, and that is the load-bearing part
//!
//! It is the one tool that runs with no consent and in every state, because its
//! entire job is reporting which state this is. Six states, plus *no
//! application running*, plus *several windows open* — and an agent acts on the
//! difference every time: one is a setting to switch on, one is a click, one is
//! a question for the user, one is a page that has not finished loading. A
//! surface that collapsed them into "unavailable" would make every other tool's
//! refusal unactionable.
//!
//! # There is deliberately no `browser_evaluate`
//!
//! **A decision, not an omission.** Arbitrary JavaScript in a live,
//! authenticated browser session is a general-purpose credential-exfiltration
//! and account-mutation tool: it can read every cookie the page can read, fetch
//! any same-origin endpoint with the user's session, and post anything anywhere
//! the user is logged in. No consent wording makes that a proportionate default,
//! and "the user clicked Allow once, on this page" cannot possibly mean "and
//! also authorised arbitrary code against every credential this session holds".
//!
//! The click/type/press-key tools are the narrow alternative and they are narrow
//! on purpose: each addresses **one element by `ref`** from
//! [`BrowserToolCall::ReadPage`]'s own outline, the reference is encoded by
//! [`super::script::json_string`] so it cannot escape into the script body, and
//! the script bodies themselves are `const` in [`super::script`] rather than
//! composed from anything an agent sent.
//!
//! # The honesty notes are in the descriptions, not only in these docs
//!
//! A model reads the tool description and nothing else, so anything it would
//! otherwise get wrong has to be *there*. Two matter:
//!
//! * `browser_network_requests` is **not** DevTools' network panel. wry exposes
//!   no CDP, so rows come from a patched `fetch`/`XMLHttpRequest` plus a
//!   `PerformanceObserver` over resource timings:  no headers, no bodies,
//!   statuses only for fetch/XHR, and nothing at all for a request issued before
//!   the init script ran. [`super::model::NETWORK_COVERAGE_NOTE`] says so
//!   verbatim in the description *and* travels on every answer as data.
//! * `browser_console_messages` captures `console.*`, `onerror` and
//!   `unhandledrejection`. Anything the engine itself emits — a CSP violation, a
//!   failed subresource, a deprecation notice — is **not** there, because
//!   nothing short of CDP can see it.

use rmcp::model::{ErrorCode, ErrorData, JsonObject, Tool};
use serde_json::{json, Map, Value};

use super::consent::BrowserTool;
use super::model::NETWORK_COVERAGE_NOTE;

pub const STATUS: &str = "browser_status";
pub const CURRENT_URL: &str = "browser_current_url";
pub const PAGE_TEXT: &str = "browser_page_text";
pub const READ_PAGE: &str = "browser_read_page";
pub const CONSOLE_MESSAGES: &str = "browser_console_messages";
pub const NETWORK_REQUESTS: &str = "browser_network_requests";
pub const NAVIGATE: &str = "browser_navigate";
pub const CLICK: &str = "browser_click";
pub const TYPE: &str = "browser_type";
pub const PRESS_KEY: &str = "browser_press_key";
pub const BACK: &str = "browser_back";
pub const FORWARD: &str = "browser_forward";
pub const RELOAD: &str = "browser_reload";

/// Every tool this server has. The listing and the dispatch read the same
/// array, so a tool cannot be advertised and unreachable, or reachable and
/// unadvertised.
pub const ALL: [&str; 13] = [
    STATUS,
    CURRENT_URL,
    PAGE_TEXT,
    READ_PAGE,
    CONSOLE_MESSAGES,
    NETWORK_REQUESTS,
    NAVIGATE,
    CLICK,
    TYPE,
    PRESS_KEY,
    BACK,
    FORWARD,
    RELOAD,
];

/// The sentence every **read** description ends with.
///
/// Repeated per tool rather than stated once in the server instructions,
/// because a client may show a description without them — the
/// `READ_ONLY_SUFFIX` reasoning from [`crate::mcp::tools`].
const READ_SUFFIX: &str = " This needs the user's permission for the page currently open, granted \
     in the browser panel and not remembered between pages; without it nothing is read.";

/// The sentence every **write** description ends with.
const WRITE_SUFFIX: &str = " This changes the page the user is looking at, so it needs the \
     stronger \"read and control\" permission for the page currently open, granted in the browser \
     panel and not remembered between pages; without it nothing is changed.";

/// The tools, as the protocol describes them.
pub fn descriptors() -> Vec<Tool> {
    vec![
        // No suffix at all: this one answers in every state, with no consent,
        // and saying otherwise would stop a model calling the tool that exists
        // to tell it what is going on.
        bare(
            STATUS,
            "Report the state of the code-basics embedded browser. Always answers, and \
             distinguishes: no application running; the browser plugin switched off; the plugin \
             on with no panel open; a panel open with no page; a page still loading; a page \
             loaded and readable; a navigation that failed; and several windows with a panel \
             open. These are different situations with different fixes — call this first, and \
             report its answer rather than guessing why another tool refused.",
            object(&[], &[]),
        ),
        read(
            CURRENT_URL,
            "The address and title of the page in the browser panel.",
            object(&[], &[]),
        ),
        read(
            PAGE_TEXT,
            "The rendered text of the page's top document. Long pages are cut, and the answer \
             states the page's real length alongside the length returned, so a cut is never \
             mistaken for a short page. Content inside a nested frame is not included and never \
             was in scope, so on a page whose content is a frame this returns the shell chrome \
             only - the answer says so, and a cut of zero is not a claim that a frame's content \
             is absent from the page.",
            object(&[], &[]),
        ),
        read(
            READ_PAGE,
            "An outline of the page's interactive elements: each one's role, its accessible name \
             and a short reference. Use those references with browser_click and browser_type — \
             they address an element this application resolved, rather than a CSS selector that \
             may match nothing or several things. Two limits the answer states: an element with \
             a zero-sized box (display:none, a collapsed menu, a hidden input) is left out \
             because it cannot be clicked, and the outline is the top document's - nothing \
             inside a nested frame is in it.",
            object(&[], &[]),
        ),
        read(
            CONSOLE_MESSAGES,
            "Console output captured from the page. What is captured is console.* calls plus the \
             window's error and unhandledrejection events, and nothing else: messages the browser \
             engine itself emits — content-security-policy violations, failed subresources, \
             deprecation notices — are not here and their absence is not evidence they did not \
             happen. Capture starts when the page loads, so anything logged earlier is gone.",
            object(&[cursor_arg()], &[]),
        ),
        read(
            NETWORK_REQUESTS,
            &format!(
                "Network requests observed in the page. This is NOT the browser's network panel \
                 and must not be reported as one. {NETWORK_COVERAGE_NOTE} Every answer repeats \
                 that coverage as a field; quote it rather than implying the list is complete."
            ),
            object(&[cursor_arg()], &[]),
        ),
        write(
            NAVIGATE,
            "Move the browser panel to a URL. This is a change and not a read: it replaces the \
             page the user is looking at, in a window they may not be watching, and it ends the \
             permission they granted for the previous page. A search phrase is refused rather \
             than searched for — pass an address.",
            object(
                &[(
                    "url",
                    json!({
                        "type": "string",
                        "description": "An http or https address. Anything else — a search \
                                        phrase, a file:// path, a javascript: url, the \
                                        application's own origin — is refused with the reason."
                    }),
                )],
                &["url"],
            ),
        ),
        write(
            CLICK,
            "Click one element, addressed by a reference from browser_read_page.",
            object(&[element_arg()], &["ref"]),
        ),
        write(
            TYPE,
            "Type text into one element, addressed by a reference from browser_read_page. The \
             element is focused and its existing value replaced.",
            object(
                &[
                    element_arg(),
                    (
                        "text",
                        json!({ "type": "string", "description": "The text to type." }),
                    ),
                ],
                &["ref", "text"],
            ),
        ),
        write(
            PRESS_KEY,
            "Send one key to whatever the page has focused — Enter, Tab, Escape, ArrowDown. Use \
             the key's own name as the DOM reports it.",
            object(
                &[(
                    "key",
                    json!({
                        "type": "string",
                        "description": "A DOM key name, e.g. \"Enter\", \"Tab\", \"Escape\", \
                                        \"ArrowDown\"."
                    }),
                )],
                &["key"],
            ),
        ),
        write(
            BACK,
            "Go back one step in the page's own history. A page with nothing behind it does \
             nothing, and this is not detectable from here — check browser_current_url after.",
            object(&[], &[]),
        ),
        write(
            FORWARD,
            "Go forward one step in the page's own history. As with back, a page with nothing \
             ahead of it does nothing and that is not detectable from here.",
            object(&[], &[]),
        ),
        write(
            RELOAD,
            "Reload the current page. A reload re-runs whatever the page does on load, which for \
             a page reached by submitting a form may repeat that submission.",
            object(&[], &[]),
        ),
    ]
}

fn cursor_arg() -> (&'static str, Value) {
    (
        "since",
        json!({
            "type": "integer",
            "minimum": 0,
            "description": "Return only entries after this sequence number. Omit it for \
                            everything still held. The answer reports how many entries were \
                            evicted before the cursor, so a gap is never silent."
        }),
    )
}

fn element_arg() -> (&'static str, Value) {
    (
        "ref",
        json!({
            "type": "string",
            "description": "An element reference from browser_read_page. Not a CSS selector: a \
                            selector guessed from page text may match nothing, or several \
                            things, and clicking the wrong one is not undoable."
        }),
    )
}

fn bare(name: &'static str, description: &str, schema: JsonObject) -> Tool {
    Tool::new(name, description.to_string(), schema)
}

fn read(name: &'static str, description: &str, schema: JsonObject) -> Tool {
    Tool::new(name, format!("{description}{READ_SUFFIX}"), schema)
}

fn write(name: &'static str, description: &str, schema: JsonObject) -> Tool {
    Tool::new(name, format!("{description}{WRITE_SUFFIX}"), schema)
}

fn object(properties: &[(&str, Value)], required: &[&str]) -> JsonObject {
    let mut props = Map::new();
    for (name, schema) in properties {
        props.insert((*name).to_string(), schema.clone());
    }
    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), Value::Object(props));
    schema.insert(
        "required".to_string(),
        Value::Array(
            required
                .iter()
                .map(|name| Value::String((*name).to_string()))
                .collect(),
        ),
    );
    schema
}

/// One call, already read. The dispatch matches on this rather than on strings,
/// so a tool that is added to [`ALL`] and forgotten is a compile error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserToolCall {
    Status,
    CurrentUrl,
    PageText,
    ReadPage,
    ConsoleMessages { since: u64 },
    NetworkRequests { since: u64 },
    Navigate { url: String },
    Click { element: String },
    Type { element: String, text: String },
    PressKey { key: String },
    Back,
    Forward,
    Reload,
}

impl BrowserToolCall {
    /// Which consent category this call falls in.
    ///
    /// Goes through [`super::consent::BrowserTool`] rather than answering
    /// directly, so there is exactly one table saying what is a read and what
    /// is a write, and it is the one the refusal wording is built from.
    pub fn tool(&self) -> BrowserTool {
        match self {
            Self::Status => BrowserTool::Status,
            Self::CurrentUrl => BrowserTool::CurrentUrl,
            Self::PageText => BrowserTool::PageText,
            Self::ReadPage => BrowserTool::ReadPage,
            Self::ConsoleMessages { .. } => BrowserTool::ConsoleMessages,
            Self::NetworkRequests { .. } => BrowserTool::NetworkRequests,
            Self::Navigate { .. } => BrowserTool::Navigate,
            Self::Click { .. } => BrowserTool::Click,
            Self::Type { .. } => BrowserTool::Type,
            Self::PressKey { .. } => BrowserTool::PressKey,
            Self::Back => BrowserTool::Back,
            Self::Forward => BrowserTool::Forward,
            Self::Reload => BrowserTool::Reload,
        }
    }
}

/// Read a call, or produce the protocol error saying why it could not be read.
///
/// Only two things are protocol errors, the same two as [`crate::mcp::tools`]:
/// a tool this server does not have, and arguments that are not the declared
/// shape. Neither is something a model can fix by reasoning about the page.
/// Everything else — no consent, no panel, a page still loading — is a tool
/// *refusal*, which the specification asks be handed to the model so it can
/// correct itself, and the corrections here are real.
pub fn parse_call(
    name: &str,
    arguments: Option<&JsonObject>,
) -> Result<BrowserToolCall, ErrorData> {
    let empty = Map::new();
    let args = arguments.unwrap_or(&empty);
    match name {
        STATUS => Ok(BrowserToolCall::Status),
        CURRENT_URL => Ok(BrowserToolCall::CurrentUrl),
        PAGE_TEXT => Ok(BrowserToolCall::PageText),
        READ_PAGE => Ok(BrowserToolCall::ReadPage),
        CONSOLE_MESSAGES => Ok(BrowserToolCall::ConsoleMessages {
            since: cursor(args, name)?,
        }),
        NETWORK_REQUESTS => Ok(BrowserToolCall::NetworkRequests {
            since: cursor(args, name)?,
        }),
        NAVIGATE => Ok(BrowserToolCall::Navigate {
            url: string(args, name, "url")?,
        }),
        CLICK => Ok(BrowserToolCall::Click {
            element: string(args, name, "ref")?,
        }),
        TYPE => Ok(BrowserToolCall::Type {
            element: string(args, name, "ref")?,
            text: string(args, name, "text")?,
        }),
        PRESS_KEY => Ok(BrowserToolCall::PressKey {
            key: string(args, name, "key")?,
        }),
        BACK => Ok(BrowserToolCall::Back),
        FORWARD => Ok(BrowserToolCall::Forward),
        RELOAD => Ok(BrowserToolCall::Reload),
        unknown => Err(ErrorData::new(
            ErrorCode::METHOD_NOT_FOUND,
            unknown_tool_sentence(unknown),
            None,
        )),
    }
}

fn string(args: &JsonObject, tool: &str, field: &str) -> Result<String, ErrorData> {
    match args.get(field) {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(other) => Err(ErrorData::invalid_params(
            format!(
                "{tool} expects {field:?} to be a string; it was {}.",
                type_of(other)
            ),
            None,
        )),
        None => Err(ErrorData::invalid_params(
            format!("{tool} requires {field:?}."),
            None,
        )),
    }
}

/// A `since` cursor. Absent is `0`, which means *everything still held* — the
/// ring reports separately how much it had already evicted, so this default
/// cannot quietly stand for "the beginning of time".
///
/// A negative or fractional number is a shape error rather than being clamped:
/// a clamped cursor answers a question nobody asked, and a caller that sent
/// `-1` has a bug worth reporting.
fn cursor(args: &JsonObject, tool: &str) -> Result<u64, ErrorData> {
    match args.get("since") {
        None | Some(Value::Null) => Ok(0),
        Some(Value::Number(number)) => number.as_u64().ok_or_else(|| {
            ErrorData::invalid_params(
                format!("{tool} expects \"since\" to be a whole number of zero or more; it was {number}."),
                None,
            )
        }),
        Some(other) => Err(ErrorData::invalid_params(
            format!(
                "{tool} expects \"since\" to be a whole number of zero or more; it was {}.",
                type_of(other)
            ),
            None,
        )),
    }
}

fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Is this a tool this server advertised?
///
/// Exists so the **shim** can refuse a name it does not have without first
/// looking for a running application: forwarding an unknown name means a client
/// with no application running is told to start one and retry a tool that will
/// never exist. The application still parses the arguments — this answers only
/// the name, so nothing becomes reachable-but-unadvertised.
pub fn is_known(tool: &str) -> bool {
    ALL.contains(&tool)
}

/// Why a tool name is refused, in the one wording both sides use.
///
/// Both the shim (before it looks for an application) and the application
/// itself ([`parse_call`]) refuse an unknown name, and they must say the same
/// thing — so this is the single source and
/// `the_shim_and_the_application_refuse_an_unknown_tool_in_the_same_words` is
/// the alarm for a reworded copy. It states the `browser_evaluate` decision
/// explicitly, because an agent told only *no* will keep looking for it.
pub fn unknown_tool_sentence(tool: &str) -> String {
    format!(
        "There is no tool called {tool:?}. This server has: {}. There is deliberately no tool that evaluates JavaScript in the page: use {READ_PAGE} to find an element and {CLICK} or {TYPE} to act on it.",
        ALL.join(", ")
    )
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
