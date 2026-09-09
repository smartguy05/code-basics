//! The embedded browser: every decision it makes, none of the machinery it
//! makes them for.
//!
//! A floating panel hosting a real web page inside the app, so "does my
//! deployment work" is answerable without leaving it, plus a read-only-by-
//! default surface an agent can ask about the page the user is looking at.
//!
//! Nothing in this module touches a webview, a process, a socket or the
//! filesystem. That is the whole point: the mistakes an embedded browser makes
//! are invisible ones — a URL bar that silently searches, a ring that loses
//! entries without saying so, a consent flag that survives a navigation, a
//! selector that escapes into an injected script — and none of them are
//! reproducible by clicking around. They are all provable here.
//!
//! # Layering
//!
//! | Module | What it decides | Touches the world |
//! |---|---|---|
//! | [`model`] | The wire types, and the six states that must not collapse | no |
//! | [`url`] | What the URL bar accepts, and what it **refuses** | no |
//! | [`origin`] | Where the page may never go, and what "same page" means | no |
//! | [`ring`] | What a bounded log lost, and how a reader learns it | no |
//! | [`text`] | What was cut, and what the real total was | no |
//! | [`console`] | Which level a captured message carries | no |
//! | [`script`] | The injected script bodies, and how arguments are encoded | no |
//! | [`ipc`] | Every byte the page sends back — hostile input | no |
//! | [`consent`] | Whether an agent may read or move this page | no |
//! | [`framing`] | The pipe wire format — **reused**, not rewritten | no |
//!
//! # Three things that will otherwise be rediscovered
//!
//! **The engine is raw `wry`, and it is not spelled `tauri::wry`.** On Windows
//! that re-export does not exist (`tauri-2.11.5/src/lib.rs:179-184` is
//! `#[cfg(all(feature = "wry", target_os = "android"))]`), so the host crate
//! depends on `wry` directly at the version already in `Cargo.lock`.
//!
//! **No Tauri-created webview may ever host remote content in this app.**
//! `tauri-2.11.5/src/manager/webview.rs::prepare_pending_webview` appends
//! `__TAURI_INTERNALS__`, the invoke bootstrap and the invoke key as
//! initialization scripts into *every* webview Tauri creates, remote content
//! included, and a builder cannot opt out because the scripts are appended. The
//! ACL rejects remote-origin invokes with one exception —
//! `plugin:__TAURI_CHANNEL__|fetch`, excluded from the origin check for all
//! origins at `src/webview/mod.rs:1820-1850` — whose handler
//! (`src/ipc/channel.rs:318-337`) reads a **process-global** queue keyed by a
//! monotonic `u32` and never checks that the requesting webview owns that
//! channel. In this app those channels carry PTY terminal output (where the user
//! runs a coding agent and pastes tokens), process output, debug adapter output
//! and SQL rows. A wry-created child gets none of that: the spike confirmed
//! `__TAURI_INTERNALS__` is `undefined` on every page load there, so the single
//! [`ipc`] handler is the *only* bridge from the page, and everything it carries
//! is parsed by [`ipc::parse_page_message`].
//!
//! **The main window's CSP is not involved.** `manager/webview.rs:485-493` are
//! the only two `csp` references and both sit inside the `WebviewUrl` data-URL
//! branch, so it never applies to a separate webview. `tauri.conf.json` needs no
//! `frame-src`, no `connect-src` and no `dangerousRemoteDomainIpcAccess`. If a
//! page fails to load, do not "fix" a CSP error — there is none.
//!
//! # The accepted cost: an OS webview is not a DOM layer
//!
//! A WebView2 child HWND composites **above** the DOM. It ignores `--z-panel`,
//! `--z-notes` and `--z-overlay` in `src/styles.css` entirely, and `hidden` on a
//! React div does not hide it. Confirmed by image in the spike: Notes and Search
//! Everywhere are both clipped by the page. **This is accepted** — the panel is
//! not given an occlusion mechanism. Three things still hide it, and all three
//! are decisions in [`crate::browser`]'s frontend sibling
//! `src/components/browserPanelLogic.ts`: the panel is minimized, the plugin is
//! switched off (in which case the host *drops* the webview — a disabled browser
//! must not keep a WebView2 process, a cookie jar or a network connection
//! alive), and the measured rect is degenerate.
//!
//! # The governing rule
//!
//! The same abstain-rather-than-guess rule as everywhere else in this crate,
//! and it bites in four specific places:
//!
//! * **A search phrase is refused, not searched.** This is not a search box, and
//!   sending what someone typed to a search engine is a guess with a network
//!   request attached. See [`url::normalize_input`].
//! * **A dropped log entry is counted, and a cursor that skipped one is told
//!   so.** See [`ring`].
//! * **Truncation reports the real total.** A cap that also caps the number is a
//!   lie about the page. See [`text`].
//! * **Consent is a statement about a page, so it dies with the page.** Both
//!   flags default false, and an origin change resets them. See [`consent`].

pub mod argv;
pub mod consent;
pub mod console;
pub mod framing;
pub mod install;
pub mod instances;
pub mod ipc;
pub mod liveness;
pub mod model;
pub mod origin;
pub mod render;
pub mod ring;
pub mod script;
pub mod serve;
pub mod text;
pub mod tools;
pub mod url;
pub mod wire;
