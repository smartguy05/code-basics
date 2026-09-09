//! The embedded browser **host**: the one place a real web page lives inside
//! this application.
//!
//! Every decision is in [`cb_core::browser`] (what the URL bar accepts, where a
//! navigation may go, what a bounded log lost, whether an agent may read the
//! page) or in [`shared`] (what the panel and an agent read back). What is left
//! here is the machinery those decisions are made *for*, and it is machinery
//! nothing can test: a wry `WebView` cannot be constructed without a window.
//!
//! # The engine is raw `wry`, and it is not spelled `tauri::wry`
//!
//! `tauri-2.11.5/src/lib.rs:179-184` re-exports `wry` under
//! `#[cfg(all(feature = "wry", target_os = "android"))]`, so on Windows that
//! path does not exist. This crate depends on `wry` directly, pinned to the
//! version already in `Cargo.lock` — the one `tauri-runtime-wry` resolves — so
//! there is exactly one wry in the tree.
//!
//! # No Tauri-created webview may ever host remote content in this app
//!
//! This is the reason for the whole approach and it is a real vulnerability, not
//! a preference. `tauri-2.11.5/src/manager/webview.rs::prepare_pending_webview`
//! appends `__TAURI_INTERNALS__`, the invoke bootstrap and the invoke key as
//! initialization scripts into **every** webview Tauri creates, remote content
//! included, and a builder cannot opt out because the scripts are appended. The
//! ACL rejects remote-origin invokes with one exception —
//! `plugin:__TAURI_CHANNEL__|fetch`, excluded from the origin check for *all*
//! origins at `src/webview/mod.rs:1820-1850` — whose handler
//! (`src/ipc/channel.rs:318-337`) reads a **process-global** queue keyed by a
//! monotonic `u32` and never checks that the requesting webview owns that
//! channel, then `remove`s the entry. In this app those channels carry PTY
//! terminal output (where the user runs a coding agent and pastes tokens),
//! process output, debug-adapter output and SQL rows. A remote page in a
//! Tauri-created child webview could poll ids and both read and steal them.
//!
//! A wry-created child gets none of it: the Phase 4 spike observed
//! `__TAURI_INTERNALS__` as `undefined` on every page load, so the single
//! [`with_ipc_handler`][wry::WebViewBuilder::with_ipc_handler] installed below
//! is the *only* bridge from the page, and everything crossing it is parsed by
//! [`cb_core::browser::ipc::parse_page_message`] — whose whole capability is
//! appending to a bounded ring or setting a title.
//!
//! # The main window's CSP is not involved
//!
//! `manager/webview.rs:485-493` are the only two `csp` references and both sit
//! inside the `WebviewUrl` data-URL branch, so the configured CSP never applies
//! to a separate webview. `tauri.conf.json` needs no `frame-src`, no
//! `connect-src` and no `dangerousRemoteDomainIpcAccess`, and none was added. If
//! a page fails to load, **do not "fix" a CSP error — there is none.**
//!
//! # Why the webview is in a `thread_local!` and not in `AppState`
//!
//! A wry `WebView` is `!Send` and main-thread-affine. `AppState` is `Sync`,
//! shared by every command, and `set_workspace` clears caches *while holding a
//! `std::sync::Mutex` guard* — so nothing in it may be main-thread-bound and
//! nothing in it may need to `.await`. Putting the webview there does not
//! compile, and forcing it to (an `unsafe impl Send`, a wrapper) trades a
//! compile error for a deadlock or a cross-thread COM call.
//!
//! So the resource lives in [`HOST`], a `thread_local!` on the main thread, and
//! `AppState` holds a [`BrowserHandle`] — clone-cheap, `Send + Sync`, carrying
//! only an `AppHandle` and the `Arc<Mutex<`[`BrowserShared`]`>>`. Every
//! operation crosses to the main thread through
//! [`AppHandle::run_on_main_thread`] and returns on a `tokio::sync::oneshot`.
//! This is the [`cb_core::lsp::session::LspHandle`] reasoning: **never an
//! `Arc<Mutex<the resource>>`, always a handle to data plus a way to reach the
//! thread that owns the resource.**
//!
//! Consequence, and it is not optional: **every browser command is `async`**.
//! Tauri warns at `src/webview/mod.rs:1290` that webview creation deadlocks when
//! called from a synchronous command or from an event handler on Windows. The
//! spike only ever created the webview via `run_on_main_thread`, so the
//! synchronous path is untested as well as forbidden.
//!
//! # The accepted cost: an OS webview is not a DOM layer
//!
//! A WebView2 child HWND composites **above** the DOM. It ignores `--z-panel`,
//! `--z-notes` and `--z-overlay` in `src/styles.css` entirely, and `hidden` on a
//! React div does not hide it — the spike confirmed by image that both Notes and
//! Search Everywhere are clipped by the page. **This is accepted**; there is
//! deliberately no occlusion mechanism. Three things still hide the page, and
//! all three are frontend decisions in `src/components/browserPanelLogic.ts`
//! applied through [`BrowserHandle::set_visible`] and
//! [`BrowserHandle::close`]: the panel is minimized (hidden, kept alive), the
//! plugin was switched off (**dropped** — a disabled browser must not keep a
//! WebView2 process, a cookie jar or a network connection alive), and the
//! measured rect is unusable.
//!
//! # Defence in depth
//!
//! Each of these is one line below with its reason beside it, and together they
//! are the reason this panel is not simply "a browser in the app":
//!
//! * navigation refused for everything [`navigation_verdict`] refuses,
//! * a **separate** WebView2 user-data directory,
//! * popups denied,
//! * downloads refused,
//! * clipboard access, devtools, autoplay and back/forward gestures off,
//! * one non-configurable page-side global, defined by
//!   [`cb_core::browser::script::init_script`].

pub mod agent;
#[cfg(windows)]
pub mod pipe;
pub mod registry;
pub mod shared;

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cb_core::browser::model::{BrowserAvailability, PageText};
use cb_core::browser::origin::{navigation_verdict, NavigationVerdict, APP_ORIGINS};
use cb_core::browser::script::{self, DEFAULT_MESSAGE_LIMIT};
use cb_core::browser::text::{truncate_page_text, DEFAULT_TEXT_LIMIT};
use cb_core::browser::{ipc, url as browser_url};
use tauri::{AppHandle, Manager};
use tokio::sync::oneshot;
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder};

pub use shared::{BrowserRect, BrowserShared, BrowserSnapshot, ConsoleBatch, NetworkBatch};

thread_local! {
    /// The live webview, on the main thread and reachable from nowhere else.
    ///
    /// A `thread_local!` rather than a field because the resource is
    /// main-thread-affine and `!Send` — see the module doc. Access is always
    /// `HOST.with(|cell| cell.borrow_mut())` inside a `run_on_main_thread`
    /// closure, so there is never a second borrow: the main thread runs one
    /// closure at a time.
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

/// The webview and the context it was built from.
///
/// Field order is load-bearing: fields drop in declaration order, so the
/// `WebView` is torn down **before** the `WebContext` that owns its WebView2
/// environment. The context is boxed so that moving this struct never moves the
/// `WebContext` itself — the builder borrowed it during `build_as_child`, and
/// while the returned `WebView` carries no lifetime, keeping the address stable
/// costs one allocation and removes the question entirely.
struct Host {
    webview: WebView,
    #[allow(
        dead_code,
        reason = "held so the WebView2 environment outlives the webview"
    )]
    context: Box<WebContext>,
}

/// The `Send + Sync` half: what `AppState` holds.
#[derive(Clone)]
pub struct BrowserHandle {
    app: AppHandle,
    shared: Arc<Mutex<BrowserShared>>,
}

impl BrowserHandle {
    /// Pair an app handle with the state `AppState` owns.
    ///
    /// The data outlives any one handle — a handle is minted per command — which
    /// is why the `Arc` comes in rather than being created here.
    pub fn from_parts(app: AppHandle, shared: Arc<Mutex<BrowserShared>>) -> Self {
        Self { app, shared }
    }

    /// Read the host's data. Takes the mutex for the duration of `read` only —
    /// never across an `.await`, and never while the main thread is being waited
    /// on.
    pub fn read<T>(&self, read: impl FnOnce(&BrowserShared) -> T) -> T {
        let guard = self.shared.lock().expect("browser state poisoned");
        read(&guard)
    }

    /// Change the host's data.
    pub fn write<T>(&self, write: impl FnOnce(&mut BrowserShared) -> T) -> T {
        let mut guard = self.shared.lock().expect("browser state poisoned");
        write(&mut guard)
    }

    /// Run `work` on the main thread and await its answer.
    ///
    /// The one seam every operation goes through. `work` receives the host slot
    /// and the shared data, and runs to completion with the main thread's
    /// message loop paused — so it must not block, and must not itself await.
    async fn on_main<T, F>(&self, work: F) -> Result<T, String>
    where
        F: FnOnce(&AppHandle, &Arc<Mutex<BrowserShared>>, &mut Option<Host>) -> Result<T, String>
            + Send
            + 'static,
        T: Send + 'static,
    {
        let (reply, answer) = oneshot::channel();
        let app = self.app.clone();
        let shared = self.shared.clone();
        self.app
            .run_on_main_thread(move || {
                let outcome = HOST.with(|cell| {
                    let mut slot = cell.borrow_mut();
                    work(&app, &shared, &mut slot)
                });
                // The receiver is gone only if the command was cancelled; the
                // work has already happened either way.
                let _ = reply.send(outcome);
            })
            .map_err(|e| format!("the browser could not reach the main thread: {e}"))?;
        answer.await.map_err(|_| {
            "the main thread dropped the browser request without answering".to_owned()
        })?
    }

    /// Open the panel's page, creating the webview if it is not there.
    ///
    /// `url` is the address the user asked for, already normalised by
    /// [`cb_core::browser::url::normalize_input`], or `None` for a blank panel.
    pub async fn open(&self, rect: BrowserRect, url: Option<String>) -> Result<(), String> {
        let bounds = bounds_for(rect)?;
        self.write(shared::note_opened);
        self.on_main(move |app, shared, slot| {
            if let Some(host) = slot.as_ref() {
                // Already up: this is a re-open of a panel that was only
                // minimized. Re-place and re-show it rather than recreating it —
                // recreating would throw away the page, its session and its
                // scroll position for what the user experiences as un-minimizing.
                host.webview.set_bounds(bounds).map_err(describe)?;
                host.webview.set_visible(true).map_err(describe)?;
                if let Some(url) = url {
                    return load(host, shared, &url);
                }
                return Ok(());
            }
            // `note_opened` above set `Blank` optimistically, and `Blank`'s own
            // words are a positive claim: "the panel is open with no page
            // loaded". If creation then fails, a bare `?` would leave that claim
            // standing — the durable state would say the panel is sitting there
            // empty when in fact there is no webview at all, and an agent asking
            // `browser_status` would be told something untrue about a thing the
            // user asked for and did not get. `Failed` exists for exactly this,
            // and the `load` path below already records it the same way.
            let host = match create(app, shared, bounds) {
                Ok(host) => host,
                Err(reason) => {
                    let mut state = shared.lock().expect("browser state");
                    shared::note_failed(&mut state, &reason);
                    return Err(reason);
                }
            };
            let outcome = match &url {
                Some(url) => load(&host, shared, url),
                None => Ok(()),
            };
            *slot = Some(host);
            outcome
        })
        .await
    }

    /// Drop the webview.
    ///
    /// **Not** `set_visible(false)`. Called when the panel is closed or the
    /// plugin is switched off, and the spike verified that dropping really does
    /// take the whole WebView2 process tree with it — the child's browser
    /// process and all five of its descendants were gone afterwards. A hidden
    /// webview keeps the page running, the cookie jar warm and the connection
    /// open, which is exactly what a user switching the feature off is asking
    /// not to have.
    pub async fn close(&self, availability: BrowserAvailability) -> Result<(), String> {
        self.write(|shared| shared::note_closed(shared, availability));
        self.on_main(move |_, _, slot| {
            // Dropped inside the main-thread closure: the destructor is a COM
            // teardown and must run on the thread that created the webview.
            drop(slot.take());
            Ok(())
        })
        .await
    }

    /// Move and resize the page to follow the panel.
    ///
    /// A no-op when there is no webview: the panel measures itself before the
    /// host exists on the first frame, and treating that as an error would put a
    /// red message in front of the user for the ordinary startup sequence.
    pub async fn set_bounds(&self, rect: BrowserRect) -> Result<(), String> {
        let bounds = bounds_for(rect)?;
        self.on_main(move |_, _, slot| match slot.as_ref() {
            Some(host) => host.webview.set_bounds(bounds).map_err(describe),
            None => Ok(()),
        })
        .await
    }

    /// Show or hide the OS surface.
    ///
    /// The minimize mechanism, and the only one that works: a React `hidden`
    /// cannot hide a child HWND that composites above the DOM.
    pub async fn set_visible(&self, visible: bool) -> Result<(), String> {
        self.on_main(move |_, _, slot| match slot.as_ref() {
            Some(host) => host.webview.set_visible(visible).map_err(describe),
            None => Ok(()),
        })
        .await
    }

    /// Load `url` — already normalised and already checked by
    /// [`navigation_verdict`], which runs again inside the navigation handler
    /// because a page can navigate itself.
    pub async fn navigate(&self, url: String) -> Result<(), String> {
        self.on_main(move |_, shared, slot| {
            let host = slot.as_ref().ok_or_else(no_page)?;
            load(host, shared, &url)
        })
        .await
    }

    /// Reload the current page.
    pub async fn reload(&self) -> Result<(), String> {
        self.on_main(|_, _, slot| {
            slot.as_ref()
                .ok_or_else(no_page)?
                .webview
                .reload()
                .map_err(describe)
        })
        .await
    }

    /// Go back or forward in the page's own history.
    ///
    /// Through `history.back()`/`history.forward()` because **wry 0.55.1 has no
    /// `go_back`/`go_forward` on `WebView`** — `reload` is the only navigation
    /// verb it exposes. The script route is genuinely weaker and the difference
    /// is stated rather than hidden: a page that has replaced its own `history`
    /// object, or one whose history is empty, does nothing and reports success,
    /// because the DOM gives no answer either way. The navigation handler still
    /// vets wherever it lands, so this cannot reach a refused origin.
    pub async fn go(&self, delta: i32) -> Result<(), String> {
        let script = if delta < 0 {
            "try{history.back()}catch(e){}"
        } else {
            "try{history.forward()}catch(e){}"
        };
        self.on_main(move |_, _, slot| {
            slot.as_ref()
                .ok_or_else(no_page)?
                .webview
                .evaluate_script(script)
                .map_err(describe)
        })
        .await
    }

    /// The page's rendered text, truncated in Rust so the **real total** is
    /// reported.
    ///
    /// The reply arrives through wry's own `evaluate_script_with_callback`, so
    /// there is no pending-id table and nothing the page can answer on its own
    /// behalf — see [`BrowserShared::pending_eval`].
    pub async fn page_text(&self, limit: usize) -> Result<PageRead, String> {
        let raw = self.evaluate(script::page_text_script()).await?;
        let value = envelope_value(&raw)?;
        let text = value
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "the page returned no text".to_owned())?;
        Ok(PageRead {
            text: truncate_page_text(text, limit),
            // The address the script itself saw, which is the only evidence
            // that the document read is the one consent was checked against -
            // see `cb_core::browser::consent::read_ran_on_the_granted_page`.
            // `WebView::url()` is not usable for this: the spike measured it
            // returning an empty string while a `data:` URL was loaded.
            ran_on: value.get("url").and_then(|v| v.as_str()).map(str::to_owned),
        })
    }

    /// An outline of the page's interactive elements, each with a `ref`.
    ///
    /// The refs are what [`Self::click`] and [`Self::type_text`] address, and
    /// they are held **in the page** by the init script's store — so a ref from
    /// before a navigation resolves to nothing and the script says so, rather
    /// than acting on whatever now sits in that position.
    pub async fn read_page(&self, max_elements: usize) -> Result<serde_json::Value, String> {
        let raw = self
            .evaluate(script::read_page_script(max_elements))
            .await?;
        envelope_value(&raw)
    }

    /// Click the element `element_ref`.
    ///
    /// The ref is encoded by [`cb_core::browser::script::json_string`] rather
    /// than pasted, which is what stops a hostile ref escaping into the script
    /// body — pinned in core by
    /// `a_hostile_element_reference_cannot_escape_into_the_script`.
    pub async fn click(&self, element_ref: &str) -> Result<serde_json::Value, String> {
        let script = script::click_script(element_ref).map_err(|e| e.to_string())?;
        let raw = self.evaluate(script).await?;
        envelope_value(&raw)
    }

    /// Type `text` into the element `element_ref`.
    pub async fn type_text(
        &self,
        element_ref: &str,
        text: &str,
    ) -> Result<serde_json::Value, String> {
        let script = script::type_script(element_ref, text).map_err(|e| e.to_string())?;
        let raw = self.evaluate(script).await?;
        envelope_value(&raw)
    }

    /// Send one key to whatever the page has focused.
    pub async fn press_key(&self, key: &str) -> Result<serde_json::Value, String> {
        let script = script::press_key_script(key).map_err(|e| e.to_string())?;
        let raw = self.evaluate(script).await?;
        envelope_value(&raw)
    }

    /// Evaluate one script and await its JSON envelope.
    ///
    /// Every script this app evaluates comes from
    /// [`cb_core::browser::script`] and is wrapped so it cannot throw — which
    /// matters because `evaluate_script_with_callback` **ignores exceptions on
    /// Windows** and fires the callback with the literal string `null`,
    /// indistinguishable from a script that returned null successfully. So a
    /// bare `null` here is reported as its own failure rather than as an empty
    /// page.
    async fn evaluate(&self, script: String) -> Result<String, String> {
        let (reply, answer) = oneshot::channel();
        // The callback is `Fn`, may fire on another thread, and must be able to
        // send exactly once — hence the mutex around the sender rather than a
        // move.
        let sender = Mutex::new(Some(reply));
        self.on_main(move |_, _, slot| {
            let host = slot.as_ref().ok_or_else(no_page)?;
            host.webview
                .evaluate_script_with_callback(&script, move |result| {
                    if let Ok(mut slot) = sender.lock() {
                        if let Some(reply) = slot.take() {
                            let _ = reply.send(result);
                        }
                    }
                })
                .map_err(describe)
        })
        .await?;
        answer.await.map_err(|_| {
            "the page never answered; it was closed or navigated while being read".to_owned()
        })
    }
}

/// A page read, and the address the reading script itself saw.
///
/// The address is carried rather than discarded because the consent check and
/// the read are not atomic: the gate runs under the shared mutex, the lock is
/// released, and only then does the script hop to the main thread and execute,
/// so the document read need not be the one the decision was made about.
/// `cb_core::browser::consent::read_ran_on_the_granted_page` is what compares
/// them. `None` means the page did not say, which is refused rather than
/// assumed to match.
///
/// Not an IPC type: `browser_page_text` still answers with the `PageText`
/// alone, because the panel's own read is not made under a grant.
#[derive(Debug, Clone)]
pub struct PageRead {
    pub text: PageText,
    pub ran_on: Option<String>,
}

/// Turn a panel rect into wry bounds, refusing what must not become an OS
/// surface's position.
///
/// The frontend has already refused the degenerate and scale-mismatched cases
/// (`browserPanelLogic.pageRect`), and this checks again anyway: a `NaN`
/// crossing IPC would reach `set_bounds` as an unpredictable number and place a
/// real window somewhere nobody chose, and "the frontend already validated it"
/// is not a property this side can verify.
///
/// The rect is **logical** — `wry::dpi` defaults to logical and the panel
/// measures CSS pixels, which are equal only while the window's scale factor
/// and `devicePixelRatio` agree. That comparison is the frontend's, because only
/// it can read `devicePixelRatio`; this side cannot re-derive it and does not
/// guess.
pub fn bounds_for(rect: BrowserRect) -> Result<Rect, String> {
    for value in [rect.left, rect.top, rect.width, rect.height] {
        if !value.is_finite() {
            return Err(format!(
                "the browser panel's rect is not a number ({rect:?}), so the page was not placed"
            ));
        }
    }
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return Err(format!(
            "the browser panel measured {}×{}, so there is nowhere to put the page",
            rect.width, rect.height
        ));
    }
    Ok(Rect {
        position: LogicalPosition::new(rect.left, rect.top).into(),
        size: LogicalSize::new(rect.width, rect.height).into(),
    })
}

fn no_page() -> String {
    "the browser panel is not open, so there is no page".to_owned()
}

fn describe(error: wry::Error) -> String {
    format!("{error}")
}

/// Parse the `{"ok":…}` envelope every injected script returns.
fn envelope_value(raw: &str) -> Result<serde_json::Value, String> {
    if raw.trim() == "null" {
        // Windows swallows a script's exception and reports `null`. The scripts
        // are wrapped so they cannot throw, so reaching this means the wrapper
        // is gone or the page tore down mid-evaluation — which is not the same
        // thing as an empty result and must not be reported as one.
        return Err(
            "the page returned nothing; its script was interrupted or the document was replaced"
                .to_owned(),
        );
    }
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("the page's answer was not JSON: {e}"))?;
    // **The envelope arrives JSON-encoded, and this is measured rather than
    // assumed.** Every injected script ends in `return JSON.stringify({...})`,
    // so the value handed to the callback is a *string*, and WebView2 delivers
    // it as JSON - the Phase 4 spike saw `document.title` come back as
    // `"focus-test"`, quotes included. So the raw text is JSON containing JSON:
    // parsing it once yields a `Value::String`, whose `get("ok")` is `None`,
    // which read as "the page refused without saying why" and made every page
    // read fail against a real document while every unit test passed.
    //
    // Both shapes are accepted: a runtime that hands back the object directly
    // is not something to break over, and the inner parse is attempted only for
    // a string, so nothing else is unwrapped twice.
    let value = match value.as_str() {
        Some(inner) => serde_json::from_str(inner).unwrap_or(value),
        None => value,
    };
    if value.get("ok").and_then(|v| v.as_bool()) == Some(true) {
        return Ok(value);
    }
    let reason = value
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("the page refused without saying why");
    Err(reason.to_owned())
}

/// Ask the webview to load `url`, recording what that means for the state.
fn load(host: &Host, shared: &Arc<Mutex<BrowserShared>>, url: &str) -> Result<(), String> {
    // Checked here as well as in the navigation handler. The handler is the
    // backstop for a navigation the *page* starts; this is the answer for one
    // the *user* asked for, and it must be a refusal they can read rather than a
    // click that silently does nothing.
    if let NavigationVerdict::Refuse(refusal) = navigation_verdict(url, &APP_ORIGINS) {
        with(shared, |data| {
            shared::note_refused_navigation(data, &refusal)
        });
        return Err(refusal.to_string());
    }
    match host.webview.load_url(url) {
        Ok(()) => {
            // `Loading`, not `Ready`. The page-load handler moves it.
            with(shared, |data| shared::note_navigation(data, url));
            Ok(())
        }
        Err(error) => {
            let reason = describe(error);
            with(shared, |data| shared::note_failed(data, &reason));
            Err(reason)
        }
    }
}

fn with<T>(shared: &Arc<Mutex<BrowserShared>>, work: impl FnOnce(&mut BrowserShared) -> T) -> T {
    let mut guard = shared.lock().expect("browser state poisoned");
    work(&mut guard)
}

/// Build the child webview.
///
/// Every builder call that is a *refusal* carries its reason inline. This is the
/// list the module doc calls defence in depth, and none of it is a nicety.
fn create(
    app: &AppHandle,
    shared: &Arc<Mutex<BrowserShared>>,
    bounds: Rect,
) -> Result<Host, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "the application window is gone, so no page can be shown".to_owned())?;

    // A separate WebView2 user-data folder, and it is a hard requirement rather
    // than isolation hygiene: two WebView2 environments sharing a folder with
    // different options **fail to create** (wry warns about this at
    // `lib.rs:1705`). Tauri puts the main webview's in `LocalData/<identifier>`,
    // so this is a distinct directory beneath it. It also means the page's
    // cookies, storage and cache never touch the application's own.
    let mut context = Box::new(WebContext::new(Some(data_directory(app)?)));

    let for_navigation = shared.clone();
    let for_load = shared.clone();
    let for_title = shared.clone();
    let for_ipc = shared.clone();

    let webview = WebViewBuilder::new_with_web_context(&mut context)
        // The instrumentation, from `cb-core` so its content is tested. It
        // defines exactly one non-configurable, non-writable global, so a page
        // cannot replace it and start feeding this host fabricated console lines
        // under the app's own message kinds.
        .with_initialization_script(script::init_script(DEFAULT_MESSAGE_LIMIT))
        // The single bridge from remote content into this application.
        .with_ipc_handler(move |request| {
            let raw = request.into_body();
            match ipc::parse_page_message(&raw) {
                Ok(message) => with(&for_ipc, |data| shared::apply_page_message(data, message)),
                // Counted and named, never applied. A rising count of one
                // unknown kind is a page probing this app.
                Err(problem) => with(&for_ipc, |data| shared::note_page_problem(data, &problem)),
            }
        })
        // Returning `false` cancels the navigation. This is the backstop for a
        // navigation the *page* starts — a redirect, a script, a link — and it
        // is where `tauri://localhost`, the dev server, `file:`, `javascript:`,
        // `data:` and every custom scheme are refused.
        .with_navigation_handler(move |url| match navigation_verdict(&url, &APP_ORIGINS) {
            NavigationVerdict::Allow => {
                with(&for_navigation, |data| {
                    shared::note_navigation(data, &url);
                });
                true
            }
            NavigationVerdict::Refuse(refusal) => {
                with(&for_navigation, |data| {
                    shared::note_refused_navigation(data, &refusal);
                });
                false
            }
        })
        .with_on_page_load_handler(move |event, url| match event {
            PageLoadEvent::Started => with(&for_load, |data| shared::note_navigation(data, &url)),
            PageLoadEvent::Finished => {
                with(&for_load, |data| shared::note_load_finished(data, &url));
            }
        })
        // The pill's label. Taken from the handler as well as from the init
        // script's own `title` message, because the handler works on a page
        // whose scripts never ran.
        .with_document_title_changed_handler(move |title| {
            with(&for_title, |data| data.title = Some(title));
        })
        // Popups denied. A `window.open` would create a webview this app does
        // not own, outside the navigation handler, outside the panel's rect, and
        // with no URL bar to say where it went.
        .with_new_window_req_handler(|_url, _features| NewWindowResponse::Deny)
        // Downloads refused in v1. A browser panel that can write files is a
        // bigger feature than this one, and it needs its own answer to where the
        // file goes and what the user is shown before it lands.
        .with_download_started_handler(|_url, _path| false)
        // No clipboard access: the page must not be able to read what the user
        // copied out of the editor, a terminal or a SQL result.
        .with_clipboard(false)
        // No devtools. They are a second, unvetted navigation and evaluation
        // surface on the same webview.
        .with_devtools(false)
        // No autoplay: a page that starts making noise in a panel behind the
        // editor has no visible control to stop it.
        .with_autoplay(false)
        // No swipe/gesture history navigation: it moves the page with no URL-bar
        // interaction, which is the one place the user's own address is shown.
        .with_back_forward_navigation_gestures(false)
        // Not incognito: `with_incognito` **ignores the WebContext entirely**
        // (wry's own doc note), which would put the page's data back in the
        // default environment — the opposite of what is wanted. Isolation here
        // is the separate data directory, and a session the user can end by
        // closing the panel.
        .with_incognito(false)
        // Not created focused: opening the panel must not take the caret out of
        // the editor the user is typing in.
        .with_focused(false)
        .with_bounds(bounds)
        .with_url("about:blank")
        .build_as_child(&window)
        .map_err(|e| format!("the browser panel's page could not be created: {e}"))?;

    Ok(Host { webview, context })
}

/// Where the page's cookies, storage and cache live.
fn data_directory(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("the browser panel has nowhere to keep its session data: {e}"))?;
    Ok(base.join("browser"))
}

/// The page-text cap this host applies. Re-exported so the command and the
/// frontend name one number.
pub const PAGE_TEXT_LIMIT: usize = DEFAULT_TEXT_LIMIT;

/// How many elements one `browser_read_page` may return.
///
/// A cap rather than the whole document, and the outline **reports the real
/// total** so a cut list is never mistaken for the whole page — the
/// `truncate_page_text` rule applied to elements. 300 is enough for any page a
/// person navigates by hand and small enough that the answer stays readable.
pub const READ_PAGE_LIMIT: usize = 300;

/// Normalise what the user typed in the URL bar.
///
/// A thin pass-through so the command body contains no decision: the rule —
/// a bare host becomes `https`, `localhost:PORT` becomes `http`, a
/// **search phrase is refused rather than searched** — is
/// [`cb_core::browser::url::normalize_input`]'s.
pub fn normalize(input: &str) -> Result<String, String> {
    browser_url::normalize_input(input).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
