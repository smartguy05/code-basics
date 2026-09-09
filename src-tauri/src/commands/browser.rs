//! Commands behind the embedded browser panel.
//!
//! **Every one is `async`.** Tauri warns at `src/webview/mod.rs:1290` that
//! webview creation deadlocks when called from a synchronous command or from an
//! event handler on Windows, and the host reaches the webview through
//! `AppHandle::run_on_main_thread` plus a `oneshot` for exactly that reason. A
//! synchronous command here would be a hang, not a compile error, so the shape
//! is not optional — see [`crate::browser`] for the whole argument.
//!
//! Per CLAUDE.md a command body may not decide anything, and in this crate that
//! is not a style rule: a command takes `State<'_, AppState>`, which cannot be
//! constructed without a `tauri::App`, so **no function in this file is callable
//! from a test and none can be**. So each body resolves state, clones a handle
//! and awaits — and every mapping that could be wrong is a free function beside
//! it, tested below:
//!
//! * [`close_availability`] — which of the six states a close leaves behind.
//! * [`read_gate`] — whether the panel may be read *now*, and which of the six
//!   answers explains a refusal.
//!
//! Two more decisions the brief for this work asked to be put here turned out to
//! already have a better home, and are tested there rather than duplicated:
//! `state_for` (the availability/origin collapse) and the
//! consent-after-navigation rule are both in [`crate::browser::shared`], which
//! is where the state they act on lives; `bounds_for` is in
//! [`crate::browser`] beside the `set_bounds` call it feeds. A second copy of
//! any of them here would be a second opinion.

use cb_core::browser::model::{BrowserAvailability, PageText};
use cb_core::features::{self, FeatureId};
use tauri::{AppHandle, State};

use cb_core::browser::instances::Listener;

use crate::browser::{
    self, registry, shared, BrowserRect, BrowserShared, BrowserSnapshot, ConsoleBatch, NetworkBatch,
};
use crate::state::AppState;

/// Open the panel's page at `rect`, optionally navigating straight to `url`.
///
/// Creates the webview if there is none, and otherwise re-places and re-shows
/// the one that is already there — a re-open is usually a user un-minimizing,
/// and recreating would throw away the page, its session and its scroll
/// position.
#[tauri::command]
pub async fn browser_open(
    app: AppHandle,
    state: State<'_, AppState>,
    rect: BrowserRect,
    url: Option<String>,
) -> Result<BrowserSnapshot, String> {
    let handle = state.browser(app);
    let target = match url.as_deref().map(str::trim) {
        Some(input) if !input.is_empty() => Some(browser::normalize(input)?),
        _ => None,
    };
    handle.open(rect, target).await?;
    // The control pipe is tied to the panel, not to the process: with no panel
    // open there is no pipe on the machine, and *panel closed* is then an
    // answer a client reads out of the registry without connecting. A failure
    // to listen is **reported to stderr and not to the user** — the panel they
    // just opened works, and agent access is the part that does not.
    announce(&state, start_listener(&state, &handle));
    Ok(handle.read(shared::snapshot))
}

/// Close the panel, dropping the webview and its WebView2 process tree.
///
/// `plugin_disabled` says *why*, and it is a parameter rather than something
/// inferred because the two cases are different answers to an agent asking
/// `browser_status` and only the caller knows which one happened.
#[tauri::command]
pub async fn browser_close(
    app: AppHandle,
    state: State<'_, AppState>,
    plugin_disabled: bool,
) -> Result<BrowserSnapshot, String> {
    let handle = state.browser(app);
    handle.close(close_availability(plugin_disabled)).await?;
    stop_listener(&state);
    // Republished rather than withdrawn, and that is the point of the entry:
    // an absent entry means *no application is running*, while an entry with
    // `browserFeature: false` means *the user switched the browser off*. Those
    // are one setting apart and a client must be able to tell them apart
    // without connecting.
    announce(&state, None);
    Ok(handle.read(shared::snapshot))
}

/// Move and resize the page to follow the panel.
#[tauri::command]
pub async fn browser_set_bounds(
    app: AppHandle,
    state: State<'_, AppState>,
    rect: BrowserRect,
) -> Result<(), String> {
    state.browser(app).set_bounds(rect).await
}

/// Show or hide the OS surface — the minimize mechanism.
#[tauri::command]
pub async fn browser_set_visible(
    app: AppHandle,
    state: State<'_, AppState>,
    visible: bool,
) -> Result<(), String> {
    state.browser(app).set_visible(visible).await
}

/// Navigate to whatever the user typed in the URL bar.
#[tauri::command]
pub async fn browser_navigate(
    app: AppHandle,
    state: State<'_, AppState>,
    input: String,
) -> Result<BrowserSnapshot, String> {
    let handle = state.browser(app);
    let url = browser::normalize(&input)?;
    handle.navigate(url).await?;
    Ok(handle.read(shared::snapshot))
}

/// Back one step in the page's own history.
#[tauri::command]
pub async fn browser_back(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.browser(app).go(-1).await
}

/// Forward one step in the page's own history.
#[tauri::command]
pub async fn browser_forward(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.browser(app).go(1).await
}

/// Reload the current page.
#[tauri::command]
pub async fn browser_reload(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.browser(app).reload().await
}

/// Everything the panel renders: availability, url, title, origin, consent and
/// the two refusal counters.
///
/// Takes no `AppHandle` and never touches the main thread — a pure read of the
/// data half, so polling it costs nothing and cannot deadlock.
#[tauri::command]
pub async fn browser_state(state: State<'_, AppState>) -> Result<BrowserSnapshot, String> {
    Ok(state.browser_data(shared::snapshot))
}

/// Captured console messages after `cursor`, and what the cursor missed.
#[tauri::command]
pub async fn browser_console(
    state: State<'_, AppState>,
    cursor: u64,
) -> Result<ConsoleBatch, String> {
    Ok(state.browser_data(|data| shared::console_batch(data, cursor)))
}

/// Observed network rows after `cursor`, what the cursor missed, and what the
/// instrumentation cannot see at all.
#[tauri::command]
pub async fn browser_network(
    state: State<'_, AppState>,
    cursor: u64,
) -> Result<NetworkBatch, String> {
    Ok(state.browser_data(|data| shared::network_batch(data, cursor)))
}

/// The page's rendered text, with the real total reported alongside.
#[tauri::command]
pub async fn browser_page_text(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PageText, String> {
    state.browser_data(read_gate)?;
    state
        .browser(app)
        .page_text(browser::PAGE_TEXT_LIMIT)
        .await
        .map(|read| read.text)
}

/// Record the consent the user gave (or withdrew) in the panel's banner.
///
/// The **only** thing that moves consent. Navigating never grants it and never
/// renews it; see `cb_core::browser::model::AutomationConsent`.
#[tauri::command]
pub async fn browser_set_automation_consent(
    state: State<'_, AppState>,
    reads: bool,
    writes: bool,
) -> Result<BrowserSnapshot, String> {
    state.browser_data_mut(|data| shared::grant_consent(data, reads, writes))?;
    Ok(state.browser_data(shared::snapshot))
}

/// Publish this application's registry entry.
///
/// Called on every panel open and close, because the registry file is the only
/// channel a client has before it knows where to ask — see
/// [`crate::browser::registry`]. Everything else about the browser is live state
/// a client learns by asking, and putting it here would guarantee a stale
/// answer.
///
/// Failures go to stderr. A user who opened a browser panel did not ask for an
/// MCP server, and a modal about one would be an error for something they are
/// probably not using.
fn announce(state: &AppState, listener: Option<Listener>) {
    let entry = registry::instance_for(
        std::process::id(),
        registry::own_exe(),
        // Read from the store rather than passed in: the feature's state is not
        // this call's to decide, and `browser_close` is reached both by a user
        // closing the panel and by the feature being switched off. The panel
        // being open is not published as a fact of its own — the presence of a
        // listener *is* that fact, and there is nothing else a client could do
        // with it.
        features::load(&features::features_path()).is_enabled(FeatureId::WebBrowser),
        listener,
        state
            .open_workspaces()
            .into_iter()
            .map(|workspace| workspace.root.display().to_string())
            .collect(),
    );
    if let Err(error) = registry::publish(entry) {
        eprintln!("code-basics: the browser instance registry could not be written: {error}");
    }
}

/// Start the control pipe, or report why there is none.
#[cfg(windows)]
fn start_listener(state: &AppState, handle: &crate::browser::BrowserHandle) -> Option<Listener> {
    match crate::browser::pipe::start(handle.clone(), registry::mint_token()) {
        Ok(listener) => {
            let published = listener.published();
            state.set_browser_pipe(Some(listener));
            Some(published)
        }
        Err(error) => {
            eprintln!("code-basics: {error}");
            None
        }
    }
}

#[cfg(not(windows))]
fn start_listener(_state: &AppState, _handle: &crate::browser::BrowserHandle) -> Option<Listener> {
    None
}

#[cfg(windows)]
fn stop_listener(state: &AppState) {
    state.clear_browser_pipe();
}

#[cfg(not(windows))]
fn stop_listener(_state: &AppState) {}

/// Which of the six states a closed panel leaves behind.
///
/// The distinction is the whole reason `BrowserAvailability` has six variants
/// rather than an `Option`: **"the user switched the browser plugin off" and
/// "the plugin is on and the panel is not up" are one click apart**, and an
/// agent told only "there is no page" would report the wrong fix to the user.
/// Collapsing them is the failure this codebase refuses, so the caller says
/// which one it is and this function is the one place it is turned into a state.
pub fn close_availability(plugin_disabled: bool) -> BrowserAvailability {
    if plugin_disabled {
        BrowserAvailability::PluginDisabled
    } else {
        BrowserAvailability::PanelClosed
    }
}

/// Whether the page may be read right now, and why not when it may not.
///
/// Asks `BrowserAvailability::licenses_a_read`, so `Loading` is a **refusal**:
/// a page mid-load yields text that is wrong rather than absent, which is the
/// worse of the two failures. The refusal quotes the availability's own reason,
/// so "the plugin is off", "the panel is closed", "there is no page", "it is
/// still loading" and "the last navigation failed" stay five answers rather than
/// one "unavailable".
pub fn read_gate(data: &BrowserShared) -> Result<(), String> {
    if data.availability.licenses_a_read() {
        return Ok(());
    }
    Err(format!(
        "the page cannot be read: {}",
        data.availability.reason()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use cb_core::browser::model::BrowserAvailability;

    #[test]
    fn closing_the_panel_and_switching_the_plugin_off_are_different_answers() {
        assert_eq!(
            close_availability(true),
            BrowserAvailability::PluginDisabled
        );
        assert_eq!(close_availability(false), BrowserAvailability::PanelClosed);
        assert_ne!(
            close_availability(true),
            close_availability(false),
            "one is a setting to change and the other is one click away; an agent \
             acts on the difference"
        );
    }

    #[test]
    fn only_a_ready_page_may_be_read() {
        let mut data = BrowserShared::new();
        for availability in [
            BrowserAvailability::PluginDisabled,
            BrowserAvailability::PanelClosed,
            BrowserAvailability::Blank,
            BrowserAvailability::Loading,
            BrowserAvailability::Failed,
        ] {
            data.availability = availability;
            let refused = read_gate(&data).unwrap_err();
            assert!(
                refused.contains(availability.reason()),
                "{availability:?} must explain itself, got {refused:?}"
            );
        }
        data.availability = BrowserAvailability::Ready;
        assert!(read_gate(&data).is_ok());
    }

    #[test]
    fn a_loading_page_is_refused_rather_than_read_half_way() {
        // The one refusal that looks like an over-reaction and is not: the text
        // is obtainable and would be wrong.
        let mut data = BrowserShared::new();
        data.availability = BrowserAvailability::Loading;
        let refused = read_gate(&data).unwrap_err();
        assert!(refused.contains("still loading"), "{refused}");
        assert!(
            refused.contains("incomplete rather than missing"),
            "{refused}"
        );
    }

    #[test]
    fn every_refusal_names_a_different_cause() {
        // Five states, five sentences. A shared "unavailable" would pass every
        // other test in this file.
        let reasons: std::collections::BTreeSet<&str> = [
            BrowserAvailability::PluginDisabled,
            BrowserAvailability::PanelClosed,
            BrowserAvailability::Blank,
            BrowserAvailability::Loading,
            BrowserAvailability::Failed,
        ]
        .into_iter()
        .map(|a| a.reason())
        .collect();
        assert_eq!(reasons.len(), 5);
    }
}
