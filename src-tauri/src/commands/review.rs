//! Launching an in-app adversarial review.
//!
//! A thin bridge, in the spirit of the rest of this crate. Every *decision* —
//! which agents are installed, which models each allows, and how the command
//! line is ordered — lives in [`cb_core::review`] and is tested there. This
//! command resolves the chosen prompt, builds an ad-hoc invocation **directly**
//! (a review is not a project `RunConfig`, so it never goes through
//! `invocation::build`), and streams the output to the console through the
//! shared [`forward`] helper.

use cb_core::enhancements;
use cb_core::model::Invocation;
use cb_core::process::ProcessEvent;
use cb_core::review::{self, ReviewAgent};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tokio::sync::mpsc;

use crate::commands::enhancements::seeded_prompts_dir;
use crate::commands::run::forward;
use crate::state::AppState;

/// The supervisor id for the review process. One slot: a second review replaces
/// the first, exactly as re-running a configuration does.
const REVIEW_ID: &str = "review:current";

/// An installed review agent, with the models its picker may offer.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewAgentInfo {
    pub id: String,
    pub label: String,
    /// Empty when the agent runs with its own configured default (no picker).
    pub models: Vec<String>,
}

/// The review agents whose CLI is installed, in preference order.
///
/// Drives the picker: with one agent it is used directly, with two the user
/// chooses. An empty list means neither `claude` nor `codex` is on PATH.
#[tauri::command]
pub async fn review_agents() -> Result<Vec<ReviewAgentInfo>, String> {
    Ok(review::detect_agents()
        .into_iter()
        .map(|a| ReviewAgentInfo {
            id: a.id().to_string(),
            label: a.label().to_string(),
            models: a.models().iter().map(|m| m.to_string()).collect(),
        })
        .collect())
}

/// Run a chosen review prompt against the open workspace with the chosen agent,
/// streaming its output live into the review console.
#[tauri::command]
pub async fn start_review(
    app: AppHandle,
    state: State<'_, AppState>,
    prompt_id: String,
    agent_id: String,
    model: Option<String>,
    channel: Channel<ProcessEvent>,
) -> Result<(), String> {
    let root = state.workspace_root()?;

    let agent = ReviewAgent::from_id(&agent_id)?;
    // Refuse an unknown model rather than silently substituting one.
    let model = review::resolve_model(agent, model.as_deref())?;

    // The prompt library is the same one the Enhancements menu lists, so a
    // user-authored review prompt dropped into the prompts dir is runnable with
    // no recompile.
    let prompt = enhancements::list_prompts(&seeded_prompts_dir(&app))
        .into_iter()
        .find(|p| p.id == prompt_id)
        .ok_or_else(|| format!("no review prompt named {prompt_id}"))?;

    let invocation = Invocation {
        program: agent.program().to_string(),
        args: review::agent_args(agent, model, &prompt.body),
        cwd: root,
        env: Default::default(),
        report: None,
        warnings: Vec::new(),
    };

    let (tx, rx) = mpsc::channel(512);
    forward(rx, channel);

    // A missing CLI on PATH surfaces to the console as a `Failed` event rather
    // than an error here, which is what the pane already renders.
    state
        .supervisor
        .run(REVIEW_ID, &invocation, tx)
        .await
        .map(|_| ())
        .map_err(|e| format!("{e:#}"))
}

/// Cancel a running review, if any.
#[tauri::command]
pub async fn cancel_review(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.supervisor.cancel(REVIEW_ID).await)
}
