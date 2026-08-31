//! Tests for [`crate::review`] — the pure, agent-aware command-line assembly for
//! an in-app adversarial review.

use crate::review::{
    agent_args, agent_args_interactive, compose_prompt, detect_agents, models_for,
    parse_codex_models, resolve_model, resolve_prompt_body, AgentMode, ReviewAgent,
    CLAUDE_DEFAULT_MODEL, CLAUDE_EDIT_PERMISSION_MODE, CLAUDE_PERMISSION_MODE, CODEX_EDIT_SANDBOX,
    CODEX_SANDBOX,
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

// --- Codex model parsing -------------------------------------------------

#[test]
fn parse_codex_models_reads_the_top_level_default() {
    // Codex's effective default is the top-level `model`.
    assert_eq!(
        parse_codex_models("model = \"gpt-5-codex\"\n"),
        vec!["gpt-5-codex".to_string()]
    );
}

#[test]
fn parse_codex_models_reads_profile_models() {
    let toml = "[profiles.smart]\nmodel = \"o3\"\n";
    assert_eq!(parse_codex_models(toml), vec!["o3".to_string()]);
}

#[test]
fn parse_codex_models_combines_default_then_profiles_deduped_in_order() {
    let toml = "\
model = \"gpt-5-codex\"

[profiles.fast]
model = \"gpt-5-codex\"

[profiles.smart]
model = \"o3\"
";
    // Top-level default leads; then each profile. The duplicate contributed by
    // `fast` is dropped and order is preserved.
    assert_eq!(
        parse_codex_models(toml),
        vec!["gpt-5-codex".to_string(), "o3".to_string()]
    );
}

#[test]
fn parse_codex_models_empty_string_yields_nothing() {
    assert!(parse_codex_models("").is_empty());
}

#[test]
fn parse_codex_models_malformed_toml_yields_nothing() {
    // A parse error abstains rather than guessing — no picker, as before.
    assert!(parse_codex_models("model = = broken").is_empty());
}

#[test]
fn parse_codex_models_non_string_model_is_ignored() {
    assert!(parse_codex_models("model = 42\n").is_empty());
}

// --- Model resolution ----------------------------------------------------

#[test]
fn claude_no_model_takes_its_default() {
    let claude = models_for(ReviewAgent::ClaudeCode);
    assert_eq!(
        resolve_model(ReviewAgent::ClaudeCode, &claude, None),
        Ok(Some(CLAUDE_DEFAULT_MODEL.to_string()))
    );
    assert_eq!(
        resolve_model(ReviewAgent::ClaudeCode, &claude, Some("  ")),
        Ok(Some(CLAUDE_DEFAULT_MODEL.to_string()))
    );
}

#[test]
fn claude_a_known_alias_resolves_and_tolerates_whitespace() {
    let claude = models_for(ReviewAgent::ClaudeCode);
    assert_eq!(
        resolve_model(ReviewAgent::ClaudeCode, &claude, Some(" sonnet ")),
        Ok(Some("sonnet".to_string()))
    );
}

#[test]
fn claude_an_unknown_model_is_refused_not_defaulted() {
    let claude = models_for(ReviewAgent::ClaudeCode);
    let err = resolve_model(ReviewAgent::ClaudeCode, &claude, Some("gpt-4")).unwrap_err();
    assert!(err.contains("gpt-4"), "names the bad model: {err}");
    assert!(err.contains("opus"), "lists the choices: {err}");
}

#[test]
fn claude_casing_is_not_matched() {
    let claude = models_for(ReviewAgent::ClaudeCode);
    assert!(resolve_model(ReviewAgent::ClaudeCode, &claude, Some("Opus")).is_err());
}

#[test]
fn codex_with_a_configured_list_resolves_a_pick_and_refuses_an_invalid_one() {
    let available = vec!["gpt-5-codex".to_string(), "o3".to_string()];
    assert_eq!(
        resolve_model(ReviewAgent::Codex, &available, Some(" o3 ")),
        Ok(Some("o3".to_string()))
    );
    // No request takes the first configured model (Codex's own default).
    assert_eq!(
        resolve_model(ReviewAgent::Codex, &available, None),
        Ok(Some("gpt-5-codex".to_string()))
    );
    let err = resolve_model(ReviewAgent::Codex, &available, Some("o1")).unwrap_err();
    assert!(err.contains("o1"), "names the bad model: {err}");
    assert!(err.contains("gpt-5-codex"), "lists the choices: {err}");
}

#[test]
fn codex_with_an_empty_list_ignores_any_request_and_yields_none() {
    // No configured models: Codex runs with its own default and a stray request
    // is ignored rather than breaking the run (the picker never sends one).
    let empty: Vec<String> = Vec::new();
    assert_eq!(resolve_model(ReviewAgent::Codex, &empty, None), Ok(None));
    assert_eq!(
        resolve_model(ReviewAgent::Codex, &empty, Some("o3")),
        Ok(None)
    );
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

// --- Prompt composition --------------------------------------------------

#[test]
fn compose_prompt_returns_the_body_unchanged_when_there_is_no_context() {
    assert_eq!(compose_prompt(None, "Review the diff."), "Review the diff.");
}

#[test]
fn compose_prompt_treats_blank_context_as_none() {
    assert_eq!(compose_prompt(Some(""), "Review."), "Review.");
    assert_eq!(compose_prompt(Some("   \n  "), "Review."), "Review.");
}

#[test]
fn compose_prompt_puts_the_context_before_the_body_with_a_separator() {
    let out = compose_prompt(Some("RULE: money is in cents"), "Review the diff.");

    let ctx = out
        .find("RULE: money is in cents")
        .expect("context present");
    let body = out.find("Review the diff.").expect("body present");
    assert!(ctx < body, "context must precede the body: {out:?}");
    assert!(
        out.contains("---"),
        "a visible separator sits between them: {out:?}"
    );
    // The body survives verbatim so nothing the caller wrote is altered.
    assert!(out.ends_with("Review the diff."));
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

// --- Choosing between an inline body and a library prompt ----------------

#[test]
fn an_inline_body_wins_over_a_library_prompt() {
    let got = resolve_prompt_body(Some("run my note"), Some("library prompt")).unwrap();
    assert_eq!(got, "run my note");
}

#[test]
fn a_blank_inline_body_falls_back_to_the_library_prompt() {
    let got = resolve_prompt_body(Some("   \n  "), Some("library prompt")).unwrap();
    assert_eq!(got, "library prompt");
}

#[test]
fn the_library_prompt_is_used_when_there_is_no_inline_body() {
    let got = resolve_prompt_body(None, Some("library prompt")).unwrap();
    assert_eq!(got, "library prompt");
}

#[test]
fn neither_source_is_an_error_rather_than_an_empty_prompt() {
    assert!(resolve_prompt_body(None, None).is_err());
    assert!(resolve_prompt_body(Some("  "), None).is_err());
}

// --- Interactive argument assembly ---------------------------------------

#[test]
fn claude_interactive_argv_passes_the_prompt_positionally() {
    let args = agent_args_interactive(ReviewAgent::ClaudeCode, Some("opus"), "What does this do?");

    // The prompt is a bare positional, not the value of a flag. The one thing
    // that may precede it is `--`, which is what *makes* it positional.
    let at = args
        .iter()
        .position(|a| a == "What does this do?")
        .expect("prompt present");
    assert!(
        at > 0 && args[at - 1] == "--",
        "prompt is positional behind the separator, not a flag value: {args:?}"
    );

    let model_at = args.iter().position(|a| a == "--model").expect("--model");
    assert_eq!(args.get(model_at + 1), Some(&"opus".to_string()));
}

#[test]
fn claude_interactive_argv_is_not_headless() {
    // -p/--print, --output-format and --verbose force the one-shot posture: the
    // session exits with the first answer instead of staying open to talk to.
    let args = agent_args_interactive(ReviewAgent::ClaudeCode, None, "Explain the auth flow.");
    for banned in [
        "-p",
        "--print",
        "--output-format",
        "stream-json",
        "--verbose",
    ] {
        assert!(
            !args.iter().any(|a| a == banned),
            "{banned} forces headless: {args:?}"
        );
    }
    // No model resolved means no flag at all, rather than an empty value.
    assert!(!args.contains(&"--model".to_string()), "no model: {args:?}");
    assert!(args.contains(&"Explain the auth flow.".to_string()));
}

#[test]
fn codex_interactive_argv_is_not_the_exec_form() {
    // `codex exec` is the headless subcommand; the interactive session is plain
    // `codex <prompt>`.
    let args = agent_args_interactive(ReviewAgent::Codex, None, "Where is the retry policy?");
    assert!(
        !args.iter().any(|a| a == "exec"),
        "exec is the headless form: {args:?}"
    );
    assert_eq!(
        args.last(),
        Some(&"Where is the retry policy?".to_string()),
        "prompt is the trailing positional: {args:?}"
    );
}

#[test]
fn codex_interactive_argv_carries_a_resolved_model() {
    // `-m/--model` is a *global* Codex flag, not exec-only, so the interactive
    // form can carry it. The command layer validates the choice against the
    // user's own `config.toml` and even refuses a name that is not in it, so
    // dropping it here would run the default under the label of a chosen model.
    let args = agent_args_interactive(ReviewAgent::Codex, Some("gpt-5-codex"), "Why is this slow?");

    let at = args.iter().position(|a| a == "-m").expect("-m present");
    assert_eq!(args.get(at + 1), Some(&"gpt-5-codex".to_string()));
    assert_eq!(
        args.last(),
        Some(&"Why is this slow?".to_string()),
        "prompt stays the trailing positional: {args:?}"
    );
    assert!(
        !args.iter().any(|a| a == "exec"),
        "still not the headless form: {args:?}"
    );
}

#[test]
fn codex_interactive_argv_omits_the_model_flag_when_none_resolved() {
    // No model resolved means no flag at all, rather than an empty value.
    let args = agent_args_interactive(ReviewAgent::Codex, None, "Where is the retry policy?");
    assert!(!args.iter().any(|a| a == "-m"), "no -m: {args:?}");
    assert!(!args.iter().any(|a| a == "--model"), "no --model: {args:?}");
    // `--` stays: it is what keeps a dash-leading question a question, and it
    // is harmless for ordinary prose.
    assert_eq!(
        args,
        vec!["--".to_string(), "Where is the retry policy?".to_string()]
    );
}

#[test]
fn a_multi_line_question_stays_one_argument() {
    let prompt = "line one\nline two with spaces";
    for agent in ReviewAgent::ALL {
        // With a model present too: a model flag must not swallow the prompt or
        // split it into a second argument.
        for model in [None, Some("a-model")] {
            let args = agent_args_interactive(agent, model, prompt);
            assert_eq!(
                args.iter().filter(|a| *a == prompt).count(),
                1,
                "{agent:?}/{model:?}: whole prompt is exactly one argument: {args:?}"
            );
            assert_eq!(
                args.last(),
                Some(&prompt.to_string()),
                "{agent:?}/{model:?}: the prompt trails everything: {args:?}"
            );
        }
    }
}

#[test]
fn the_headless_argv_is_unchanged() {
    // The Review panel depends on this exact argv; adding the interactive form
    // must not have disturbed it.
    assert_eq!(
        agent_args(
            ReviewAgent::ClaudeCode,
            AgentMode::ReadOnly,
            Some("opus"),
            "Review."
        ),
        vec![
            "-p",
            "Review.",
            "--model",
            "opus",
            "--permission-mode",
            CLAUDE_PERMISSION_MODE,
            "--output-format",
            "stream-json",
            "--verbose",
        ]
    );
    assert_eq!(
        agent_args(
            ReviewAgent::Codex,
            AgentMode::ReadOnly,
            Some("o3"),
            "Review."
        ),
        vec!["exec", "--sandbox", CODEX_SANDBOX, "-m", "o3", "Review."]
    );
}

#[test]
fn a_question_beginning_with_a_dash_is_still_a_question() {
    // A question and a flag are two distinct outcomes and must not collapse
    // into one argv position. Verified against both installed CLIs:
    //   codex "--why-does-the-build-fail"
    //     -> error: unexpected argument '--why-does-the-build-fail' found
    //        tip: to pass ... as a value, use '-- --why-does-the-build-fail'
    //   claude "--why-does-the-build-fail"
    //     -> error: unknown option '--why-does-the-build-fail'
    // With the separator both accept it and treat it as the prompt (checked by
    // round-tripping a sentinel through `codex exec --` and `claude -p --`).
    let prompt = "--why-does-the-build-fail";
    for agent in ReviewAgent::ALL {
        for model in [None, Some("a-model")] {
            let args = agent_args_interactive(agent, model, prompt);
            assert_eq!(
                args.last(),
                Some(&prompt.to_string()),
                "{agent:?}/{model:?}: the question still trails: {args:?}"
            );
            assert_eq!(
                args.len().checked_sub(2).map(|i| args[i].as_str()),
                Some("--"),
                "{agent:?}/{model:?}: `--` must immediately precede the question: {args:?}"
            );
            // The separator ends option parsing, so a model flag has to sit
            // ahead of it or it becomes part of the prompt.
            if let Some(m) = model {
                let flag_at = args
                    .iter()
                    .position(|a| a == "--model" || a == "-m")
                    .expect("model flag present");
                let sep_at = args.iter().position(|a| a == "--").expect("separator");
                assert!(
                    flag_at < sep_at,
                    "{agent:?}: the model flag must precede `--`: {args:?}"
                );
                assert_eq!(args.get(flag_at + 1), Some(&m.to_string()));
            }
        }
    }
}
