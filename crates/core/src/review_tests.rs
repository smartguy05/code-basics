//! Tests for [`crate::review`] — the pure, agent-aware command-line assembly for
//! an in-app adversarial review.

use crate::review::{
    agent_args, detect_agents, resolve_model, AgentMode, ReviewAgent, CLAUDE_DEFAULT_MODEL,
    CLAUDE_EDIT_PERMISSION_MODE, CLAUDE_PERMISSION_MODE, CODEX_EDIT_SANDBOX, CODEX_SANDBOX,
};

// --- Agent identity ------------------------------------------------------

#[test]
fn agents_round_trip_through_their_ids() {
    for agent in ReviewAgent::ALL {
        assert_eq!(ReviewAgent::from_id(agent.id()), Ok(agent));
    }
}

#[test]
fn an_unknown_agent_id_is_refused() {
    assert!(ReviewAgent::from_id("cursor").is_err());
}

#[test]
fn claude_code_leads_the_preference_order() {
    assert_eq!(ReviewAgent::ALL[0], ReviewAgent::ClaudeCode);
}

#[test]
fn only_claude_offers_models() {
    assert!(ReviewAgent::ClaudeCode.models().contains(&"opus"));
    assert!(ReviewAgent::Codex.models().is_empty());
}

// --- Model resolution ----------------------------------------------------

#[test]
fn claude_no_model_takes_its_default() {
    assert_eq!(
        resolve_model(ReviewAgent::ClaudeCode, None),
        Ok(Some(CLAUDE_DEFAULT_MODEL))
    );
    assert_eq!(
        resolve_model(ReviewAgent::ClaudeCode, Some("  ")),
        Ok(Some(CLAUDE_DEFAULT_MODEL))
    );
}

#[test]
fn claude_a_known_alias_resolves_and_tolerates_whitespace() {
    assert_eq!(
        resolve_model(ReviewAgent::ClaudeCode, Some(" sonnet ")),
        Ok(Some("sonnet"))
    );
}

#[test]
fn claude_an_unknown_model_is_refused_not_defaulted() {
    let err = resolve_model(ReviewAgent::ClaudeCode, Some("gpt-4")).unwrap_err();
    assert!(err.contains("gpt-4"), "names the bad model: {err}");
    assert!(err.contains("opus"), "lists the choices: {err}");
}

#[test]
fn claude_casing_is_not_matched() {
    assert!(resolve_model(ReviewAgent::ClaudeCode, Some("Opus")).is_err());
}

#[test]
fn codex_has_no_models_so_any_request_yields_its_own_default() {
    // Codex offers no aliases; it is always run with its configured default,
    // and naming one is ignored rather than an error (the picker never sends
    // one, but a stray value must not break the run).
    assert_eq!(resolve_model(ReviewAgent::Codex, None), Ok(None));
    assert_eq!(resolve_model(ReviewAgent::Codex, Some("o3")), Ok(None));
}

// --- Argument assembly ---------------------------------------------------

#[test]
fn claude_args_run_headless_read_only_with_model_and_prompt() {
    let args = agent_args(
        ReviewAgent::ClaudeCode,
        AgentMode::ReadOnly,
        Some("opus"),
        "Review the diff.",
    );

    assert!(args.contains(&"-p".to_string()), "headless: {args:?}");

    let model_at = args.iter().position(|a| a == "--model").expect("--model");
    assert_eq!(args.get(model_at + 1), Some(&"opus".to_string()));

    let mode_at = args
        .iter()
        .position(|a| a == "--permission-mode")
        .expect("--permission-mode");
    assert_eq!(
        args.get(mode_at + 1),
        Some(&CLAUDE_PERMISSION_MODE.to_string())
    );

    assert_eq!(
        args.iter().filter(|a| *a == "Review the diff.").count(),
        1,
        "prompt appears verbatim once: {args:?}"
    );
}

#[test]
fn claude_edit_mode_bypasses_permission_prompts() {
    // A headless editing run must never prompt (stdin is closed): bypass, not
    // acceptEdits, which still prompts for non-file Bash commands.
    let args = agent_args(
        ReviewAgent::ClaudeCode,
        AgentMode::Edit,
        None,
        "Build the graph.",
    );
    let mode_at = args
        .iter()
        .position(|a| a == "--permission-mode")
        .expect("--permission-mode");
    assert_eq!(
        args.get(mode_at + 1),
        Some(&CLAUDE_EDIT_PERMISSION_MODE.to_string())
    );
    // Editing runs still stream so the console shows progress.
    assert!(args.contains(&"stream-json".to_string()));
    assert!(args.contains(&"--verbose".to_string()));
}

#[test]
fn codex_edit_mode_uses_the_workspace_write_sandbox() {
    let args = agent_args(ReviewAgent::Codex, AgentMode::Edit, None, "Set up.");
    let sb = args
        .iter()
        .position(|a| a == "--sandbox")
        .expect("--sandbox");
    assert_eq!(args.get(sb + 1), Some(&CODEX_EDIT_SANDBOX.to_string()));
    assert_eq!(args.last(), Some(&"Set up.".to_string()));
}

#[test]
fn a_mode_round_trips_through_its_id() {
    for mode in [AgentMode::ReadOnly, AgentMode::Edit] {
        assert_eq!(AgentMode::from_id(Some(mode.id())), Ok(mode));
    }
    // Absent/blank is the safe read-only default; unknown is refused.
    assert_eq!(AgentMode::from_id(None), Ok(AgentMode::ReadOnly));
    assert_eq!(AgentMode::from_id(Some("  ")), Ok(AgentMode::ReadOnly));
    assert!(AgentMode::from_id(Some("write-everything")).is_err());
}

#[test]
fn claude_args_stream_events_so_the_console_shows_progress() {
    // Text mode buffers the whole answer until the end, which looks hung during a
    // long review. Stream-json (which requires --verbose) emits each step live.
    let args = agent_args(
        ReviewAgent::ClaudeCode,
        AgentMode::ReadOnly,
        None,
        "Review.",
    );
    let fmt_at = args
        .iter()
        .position(|a| a == "--output-format")
        .expect("--output-format");
    assert_eq!(args.get(fmt_at + 1), Some(&"stream-json".to_string()));
    assert!(
        args.contains(&"--verbose".to_string()),
        "stream-json needs --verbose"
    );
}

#[test]
fn claude_args_omit_the_model_flag_when_none() {
    let args = agent_args(
        ReviewAgent::ClaudeCode,
        AgentMode::ReadOnly,
        None,
        "Review.",
    );
    assert!(!args.contains(&"--model".to_string()), "no model: {args:?}");
    assert!(args.contains(&"--permission-mode".to_string()));
}

#[test]
fn codex_args_run_exec_read_only_with_the_prompt_last() {
    let args = agent_args(
        ReviewAgent::Codex,
        AgentMode::ReadOnly,
        None,
        "Review the diff.",
    );

    assert_eq!(args.first(), Some(&"exec".to_string()), "exec subcommand");

    let sb = args
        .iter()
        .position(|a| a == "--sandbox")
        .expect("--sandbox");
    assert_eq!(args.get(sb + 1), Some(&CODEX_SANDBOX.to_string()));

    // Codex takes the prompt as its trailing positional argument.
    assert_eq!(args.last(), Some(&"Review the diff.".to_string()));
    // No model offered, so none is passed.
    assert!(!args.contains(&"-m".to_string()), "no model: {args:?}");
}

#[test]
fn codex_args_pass_a_model_with_lowercase_m_before_the_prompt() {
    let args = agent_args(
        ReviewAgent::Codex,
        AgentMode::ReadOnly,
        Some("o3"),
        "Review.",
    );
    let m = args.iter().position(|a| a == "-m").expect("-m");
    assert_eq!(args.get(m + 1), Some(&"o3".to_string()));
    assert_eq!(args.last(), Some(&"Review.".to_string()));
}

#[test]
fn the_prompt_is_a_single_argument_for_both_agents() {
    let prompt = "line one\nline two with spaces";
    for agent in ReviewAgent::ALL {
        let args = agent_args(agent, AgentMode::ReadOnly, None, prompt);
        assert!(
            args.iter().any(|a| a == prompt),
            "{agent:?}: whole prompt is one argument: {args:?}"
        );
    }
}

// --- Detection -----------------------------------------------------------

#[test]
fn detected_agents_are_a_subset_of_all_with_no_duplicates() {
    let detected = detect_agents();
    for agent in &detected {
        assert!(ReviewAgent::ALL.contains(agent));
    }
    let mut seen = detected.clone();
    seen.dedup();
    assert_eq!(seen.len(), detected.len(), "no duplicates");
}
