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
use cb_core::review::{self, AgentMode, ReviewAgent};
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
            models: review::models_for(a),
        })
        .collect())
}

/// Run a chosen prompt against the open workspace with the chosen agent,
/// streaming its output live into the console.
///
/// Serves both the adversarial **Review** (read-only) and the Enhancements
/// **Run Agent** action (read-only or edit, via `mode`).
#[tauri::command]
pub async fn start_review(
    app: AppHandle,
    state: State<'_, AppState>,
    prompt_id: Option<String>,
    prompt_body: Option<String>,
    agent_id: String,
    model: Option<String>,
    mode: Option<String>,
    context: Option<String>,
    channel: Channel<ProcessEvent>,
) -> Result<(), String> {
    let root = state.workspace_root()?;

    let agent = ReviewAgent::from_id(&agent_id)?;
    // Refuse an unknown model rather than silently substituting one, validating
    // against the models the agent actually offers (Codex's are read from disk).
    let available = review::models_for(agent);
    let model = review::resolve_model(agent, &available, model.as_deref())?;
    // Absent/blank ⇒ read-only; an unknown value is refused, not defaulted.
    let mode = AgentMode::from_id(mode.as_deref())?;

    // A run is driven either by an inline body (a note sent to the agent) or a
    // library prompt id. The library — the same one the Enhancements menu lists,
    // so a user-authored review prompt dropped into the prompts dir runs with no
    // recompile — is only consulted when there is no inline body.
    let library_body = match prompt_id.as_deref() {
        Some(id) => Some(
            enhancements::list_prompts(&seeded_prompts_dir(&app))
                .into_iter()
                .find(|p| p.id == id)
                .ok_or_else(|| format!("no review prompt named {id}"))?
                .body,
        ),
        None => None,
    };
    let body = review::resolve_prompt_body(prompt_body.as_deref(), library_body.as_deref())?;

    // Injected context — evidence, business rules — leads the prompt so the
    // agent reads it before the instruction that acts on it. Blank/absent
    // context leaves the body untouched.
    let full_prompt = review::compose_prompt(context.as_deref(), &body);

    let invocation = Invocation {
        program: agent.program().to_string(),
        args: review::agent_args(agent, mode, model.as_deref(), &full_prompt),
        cwd: root,
        env: Default::default(),
        report: None,
        coverage: None,
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
