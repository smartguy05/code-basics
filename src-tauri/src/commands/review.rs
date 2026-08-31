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
// Tauri exposes command parameters as top-level IPC keys; wrapping these in a
// context struct would change the frontend contract solely to satisfy a lint.
#[allow(clippy::too_many_arguments)]
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
    let meta = cb_core::running::RunMeta {
        root: invocation.cwd.display().to_string(),
        label: "Adversarial review".into(),
        kind: cb_core::running::RunKind::Review,
    };
    state
        .supervisor
        .run_tracked(REVIEW_ID, &invocation, tx, meta)
        .await
        .map(|_| ())
        .map_err(|e| format!("{e:#}"))
}

/// Cancel a running review, if any.
#[tauri::command]
pub async fn cancel_review(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.supervisor.cancel(REVIEW_ID).await)
}

// ---------------------------------------------------------------------------
// The interactive command line behind "Ask the codebase"
// ---------------------------------------------------------------------------

/// A program and its argument vector, ready for `terminal_open` to spawn.
///
/// Deliberately **not** a command *string*: `PtyManager` spawns through
/// `CommandBuilder` with the arguments given, so nothing here joins or
/// re-splits them.
///
/// That is *most* of the safety, not all of it, and the remainder is
/// Windows-specific. `CommandBuilder` applies MSVC argv quoting, which is
/// correct for a real executable — through `claude.exe` a question containing a
/// quote, a newline or a `&` crosses verbatim. It is **not** correct for a
/// `.cmd`/`.bat` shim, which `resolve_program`'s PATHEXT walk can select (npm
/// installs `codex` as `codex.cmd`): there `cmd.exe` re-parses the command
/// line, so `&`, `|`, `<`, `>`, `^`, `"` and `%` change its meaning. So there
/// *is* a shell in that one case, and `cb_core::pty::argv` refuses such an
/// argument before the spawn rather than letting it run as something else.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// Build the interactive command line for asking `prompt` of `agent_id`.
///
/// Extracted from the command body because every line of it is a decision, and
/// a `#[tauri::command]` cannot be tested through — see the module rules in
/// `CLAUDE.md`. It refuses rather than guesses in all three ways it can fail:
/// an agent id this build does not know, a model the agent does not offer
/// (delegated to [`review::resolve_model`], which is where that judgement
/// already lives), and an empty question — which would otherwise start the
/// agent sitting at its own prompt, indistinguishable from the app having
/// failed to send anything.
pub(crate) fn interactive_command(
    agent_id: &str,
    model: Option<&str>,
    prompt: &str,
) -> Result<AgentCommand, String> {
    if prompt.trim().is_empty() {
        return Err("there is no question to ask".to_string());
    }
    let agent = ReviewAgent::from_id(agent_id)?;
    let available = review::models_for(agent);
    let model = review::resolve_model(agent, &available, model)?;
    Ok(AgentCommand {
        program: agent.program().to_string(),
        args: review::agent_args_interactive(agent, model.as_deref(), prompt),
    })
}

/// Refuse an agent whose CLI is not installed, naming the program that was
/// looked for.
///
/// The **installed set is a parameter, not a PATH lookup**: `detect_agents` is
/// PATH-dependent and so untestable, while the judgement it feeds -- what to do
/// when the chosen agent is not in it -- is the part that can be wrong, so it
/// lives here where a test can pin it.
///
/// This duplicates a check the picker already makes, on purpose. `AskPanel` only
/// ever lists installed agents, so `launchBlockedReason`'s not-installed branch
/// is unreachable *through the picker* -- which means a stale remembered id,
/// another caller, or a `claude` uninstalled while the app was running would
/// otherwise reach `terminal_open` and spawn nothing while looking like it
/// worked. A frontend guard is a convenience; this is the one that has to hold.
///
/// The message names the **program**, not just the agent, because "Codex is not
/// installed" leaves the user with nowhere to go and "`codex` was not found on
/// your PATH" tells them exactly what to look for. It mirrors the wording of
/// `askLogic.launchBlockedReason` so the two surfaces cannot say different
/// things about the same fact.
pub(crate) fn ensure_installed(
    agent: ReviewAgent,
    installed: &[ReviewAgent],
) -> Result<(), String> {
    if installed.contains(&agent) {
        return Ok(());
    }
    Err(format!(
        "{} is not installed \u{2014} `{}` was not found on your PATH.",
        agent.label(),
        agent.program()
    ))
}

/// The program and arguments that start an interactive agent session already
/// asked `prompt`. The frontend hands the result straight to `terminal_open`.
#[tauri::command]
pub async fn agent_interactive_command(
    agent_id: String,
    model: Option<String>,
    prompt: String,
) -> Result<AgentCommand, String> {
    // Two `from_id` calls rather than one: `interactive_command` is the command
    // *line*, this is whether the agent may be spawned at all, and folding the
    // second into the first would put a PATH lookup inside a function whose
    // every test runs without one.
    ensure_installed(ReviewAgent::from_id(&agent_id)?, &review::detect_agents())?;
    interactive_command(&agent_id, model.as_deref(), &prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_takes_the_model_then_the_prompt() {
        let cmd = interactive_command("claude-code", Some("sonnet"), "why is this slow?").unwrap();
        assert_eq!(cmd.program, "claude");
        assert_eq!(
            cmd.args,
            vec!["--model", "sonnet", "--", "why is this slow?"]
        );
    }

    #[test]
    fn no_model_falls_back_to_the_agents_first_offered_alias() {
        let cmd = interactive_command("claude-code", None, "what does this do?").unwrap();
        assert_eq!(
            cmd.args,
            vec!["--model", "opus", "--", "what does this do?"]
        );
    }

    #[test]
    fn a_model_the_agent_does_not_offer_is_refused_not_substituted() {
        let err = interactive_command("claude-code", Some("gpt-9"), "hello").unwrap_err();
        assert!(err.contains("gpt-9"), "{err}");
    }

    #[test]
    fn an_unknown_agent_id_is_refused() {
        // `claudeCode` is the intent-provider spelling, not a review agent id;
        // accepting it would spawn nothing while looking like it worked.
        let err = interactive_command("claudeCode", None, "hello").unwrap_err();
        assert!(err.contains("claudeCode"), "{err}");
    }

    #[test]
    fn codex_runs_the_interactive_form_with_the_question_last() {
        let cmd = interactive_command("codex", None, "where is the retry policy?").unwrap();
        assert_eq!(cmd.program, "codex");
        assert!(
            !cmd.args.iter().any(|a| a == "exec"),
            "exec is the headless subcommand: {:?}",
            cmd.args
        );
        assert_eq!(
            cmd.args.last().map(String::as_str),
            Some("where is the retry policy?")
        );
        // Codex's models come from the user's own `~/.codex/config.toml`, so
        // *which* model resolves here is environment-dependent. What is not:
        // whichever one did must be carried as `-m <model>` ahead of the
        // question, never silently dropped.
        // `--` ends option parsing, so a question beginning with a dash is
        // still a question rather than an unknown flag.
        match cmd.args.as_slice() {
            [sep, _prompt] => assert_eq!(sep, "--", "{:?}", cmd.args),
            [flag, _model, sep, _prompt] => {
                assert_eq!(flag, "-m", "{:?}", cmd.args);
                assert_eq!(sep, "--", "{:?}", cmd.args);
            }
            other => panic!("unexpected codex argv {other:?}"),
        }
    }

    #[test]
    fn a_codex_model_the_user_configured_reaches_the_argv() {
        // Only meaningful on a machine with a Codex config; where there is one,
        // the picker offers exactly these names and a chosen one must survive
        // the trip through `resolve_model` into the command line.
        let available = review::models_for(ReviewAgent::Codex);
        let Some(model) = available.first() else {
            return;
        };
        let cmd = interactive_command("codex", Some(model), "hello").unwrap();
        assert_eq!(
            cmd.args,
            vec![
                "-m".to_string(),
                model.clone(),
                "--".to_string(),
                "hello".to_string()
            ]
        );
    }

    #[test]
    fn a_blank_question_is_refused_rather_than_starting_an_idle_agent() {
        assert!(interactive_command("claude-code", None, "   \n ").is_err());
    }

    #[test]
    fn an_agent_that_is_not_installed_is_refused_naming_the_program() {
        // The picker only lists installed agents, so this can only be reached by
        // a stale remembered id or a caller that is not the picker -- which is
        // exactly why the backend must check rather than trust the frontend.
        let err = ensure_installed(ReviewAgent::Codex, &[ReviewAgent::ClaudeCode]).unwrap_err();
        assert!(err.contains("Codex"), "{err}");
        assert!(err.contains("codex"), "{err}");
    }

    #[test]
    fn an_empty_installed_set_still_names_the_program_it_looked_for() {
        let err = ensure_installed(ReviewAgent::ClaudeCode, &[]).unwrap_err();
        assert!(err.contains("claude"), "{err}");
        assert!(err.contains("PATH"), "{err}");
    }

    #[test]
    fn an_installed_agent_is_allowed() {
        assert!(ensure_installed(
            ReviewAgent::Codex,
            &[ReviewAgent::ClaudeCode, ReviewAgent::Codex]
        )
        .is_ok());
    }

    #[test]
    fn the_question_crosses_verbatim_as_one_argument() {
        // No shell, no quoting: a question with a newline, a quote and a
        // metacharacter must arrive as exactly one argv entry.
        let question = "why does `a && b` fail\nwhen \"x\" is set?";
        let cmd = interactive_command("claude-code", Some("haiku"), question).unwrap();
        assert_eq!(cmd.args.last().map(String::as_str), Some(question));
        assert_eq!(cmd.args.len(), 4, "{:?}", cmd.args);
    }
}
