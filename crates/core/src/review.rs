//! Assembling the command line for an in-app adversarial review.
//!
//! A review runs a coding-agent CLI — Claude Code (`claude`) or Codex (`codex`)
//! — headlessly against the open workspace in a **read-only** posture (Claude
//! plans, Codex sandboxes read-only), so it explores and reports but never edits
//! what it reviews and never blocks on an approval prompt. This module is the
//! feature's only *decision* — which agents exist, which models each allows, and
//! how the arguments are ordered — so it lives here in `cb-core`, pure and
//! tested, rather than in the untestable Tauri command that spawns the process.

use std::path::Path;

/// A coding-agent CLI that can run a review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewAgent {
    ClaudeCode,
    Codex,
}

/// How much the agent is allowed to do to the workspace.
///
/// A review runs [`ReadOnly`]; a "Run Agent" prompt may run [`Edit`] so a setup
/// or knowledge-graph prompt can actually write files. Both postures must never
/// block on an approval prompt — the supervisor closes stdin, so a prompt hangs
/// the process forever.
///
/// [`ReadOnly`]: AgentMode::ReadOnly
/// [`Edit`]: AgentMode::Edit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    ReadOnly,
    Edit,
}

impl AgentMode {
    /// Stable id used across IPC and in the panel toggle.
    pub fn id(self) -> &'static str {
        match self {
            AgentMode::ReadOnly => "read-only",
            AgentMode::Edit => "edit",
        }
    }

    /// Resolve a mode from its stable id. `None`/blank is the safe default
    /// ([`ReadOnly`]); an unknown value is refused rather than silently allowing
    /// edits.
    ///
    /// [`ReadOnly`]: AgentMode::ReadOnly
    pub fn from_id(id: Option<&str>) -> Result<Self, String> {
        match id.map(str::trim) {
            None | Some("") | Some("read-only") => Ok(AgentMode::ReadOnly),
            Some("edit") => Ok(AgentMode::Edit),
            Some(other) => Err(format!("unknown agent mode {other:?}")),
        }
    }
}

/// The model aliases Claude Code offers. Kept to the CLI's stable aliases
/// (`claude --model <alias>`) rather than pinned full ids, so a newer latest
/// model is picked up without a code change.
pub const CLAUDE_MODELS: &[&str] = &["opus", "sonnet", "haiku"];

/// Claude's default when the caller names no model — a review is thorough work,
/// so the most capable alias leads.
pub const CLAUDE_DEFAULT_MODEL: &str = "opus";

/// Claude's read-only posture: `plan` reads and reasons but never edits, and a
/// headless (`-p`) run never blocks on a permission prompt.
pub const CLAUDE_PERMISSION_MODE: &str = "plan";

/// Claude's editing posture: `bypassPermissions` executes tool calls without any
/// prompt, which is required for a headless editing run — `acceptEdits` still
/// prompts for non-file Bash commands, and any prompt hangs with stdin closed.
pub const CLAUDE_EDIT_PERMISSION_MODE: &str = "bypassPermissions";

/// Codex's read-only posture: the `read-only` sandbox lets model-generated shell
/// commands read the tree but never write it, and needs no approval.
pub const CODEX_SANDBOX: &str = "read-only";

/// Codex's editing posture: the `workspace-write` sandbox lets commands write
/// within the workspace. `codex exec` is non-interactive and never prompts for
/// approval (escalations outside the sandbox are auto-denied), so it cannot hang.
pub const CODEX_EDIT_SANDBOX: &str = "workspace-write";

impl ReviewAgent {
    /// All known agents, in preference order (the first installed one leads the
    /// picker and is the default).
    pub const ALL: [ReviewAgent; 2] = [ReviewAgent::ClaudeCode, ReviewAgent::Codex];

    /// Stable id used across IPC and in the picker.
    pub fn id(self) -> &'static str {
        match self {
            ReviewAgent::ClaudeCode => "claude-code",
            ReviewAgent::Codex => "codex",
        }
    }

    /// Human label for the picker.
    pub fn label(self) -> &'static str {
        match self {
            ReviewAgent::ClaudeCode => "Claude Code",
            ReviewAgent::Codex => "Codex",
        }
    }

    /// The program name to spawn (resolved through PATHEXT at run time).
    pub fn program(self) -> &'static str {
        match self {
            ReviewAgent::ClaudeCode => "claude",
            ReviewAgent::Codex => "codex",
        }
    }

    /// The model aliases this agent's picker offers. Empty means the agent runs
    /// with its own configured default and the picker is hidden — we do not
    /// invent model names we cannot verify (Codex's models are user-configured).
    pub fn models(self) -> &'static [&'static str] {
        match self {
            ReviewAgent::ClaudeCode => CLAUDE_MODELS,
            ReviewAgent::Codex => &[],
        }
    }

    /// The model to use when the caller names none.
    pub fn default_model(self) -> Option<&'static str> {
        match self {
            ReviewAgent::ClaudeCode => Some(CLAUDE_DEFAULT_MODEL),
            ReviewAgent::Codex => None,
        }
    }

    /// Resolve an agent by its stable id.
    pub fn from_id(id: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|a| a.id() == id)
            .ok_or_else(|| format!("unknown review agent {id:?}"))
    }
}

/// The models the picker offers for `agent`.
///
/// Claude's are the static [`CLAUDE_MODELS`] aliases; Codex's are whatever the
/// user has configured on disk (see [`codex_models`]). This is the single
/// dispatch point the command layer uses to fill a picker and to validate a
/// request through [`resolve_model`].
pub fn models_for(agent: ReviewAgent) -> Vec<String> {
    match agent {
        ReviewAgent::ClaudeCode => CLAUDE_MODELS.iter().map(|m| m.to_string()).collect(),
        ReviewAgent::Codex => codex_models(),
    }
}

/// The Codex models the user has configured in `~/.codex/config.toml`.
///
/// Reads the config off disk, so — like [`detect_agents`] — it is not
/// unit-tested; the pure parse it delegates to ([`parse_codex_models`]) is. Any
/// failure (no `CODEX_HOME`/home, an unreadable or malformed config, no `model`)
/// yields an empty list, which hides the picker exactly as before.
pub fn codex_models() -> Vec<String> {
    let Some(home) = crate::intents::providers::codex::codex_home() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(home.join("config.toml")) else {
        return Vec::new();
    };
    parse_codex_models(&text)
}

/// Parse the Codex model names from a `config.toml`'s text.
///
/// Codex's effective default is the top-level `model`, so it leads; then each
/// `[profiles.<name>] model`, de-duplicated preserving first-seen order. A
/// missing/blank `model`, a non-string value, or a parse error yields an empty
/// `Vec` — abstain, no picker, exactly as when Codex reported no models. The
/// config is genuinely parsed (unlike `codex.rs::is_trusted_in`, which is fuzzy
/// path-matching), so nested tables and quoting cannot fool it.
pub fn parse_codex_models(config_toml: &str) -> Vec<String> {
    let Ok(value) = toml::from_str::<toml::Value>(config_toml) else {
        return Vec::new();
    };

    let mut models: Vec<String> = Vec::new();
    let add = |models: &mut Vec<String>, raw: Option<&str>| {
        if let Some(name) = raw.map(str::trim) {
            if !name.is_empty() && !models.iter().any(|m| m == name) {
                models.push(name.to_string());
            }
        }
    };

    add(
        &mut models,
        value.get("model").and_then(toml::Value::as_str),
    );
    if let Some(profiles) = value.get("profiles").and_then(toml::Value::as_table) {
        for profile in profiles.values() {
            add(
                &mut models,
                profile.get("model").and_then(toml::Value::as_str),
            );
        }
    }

    models
}

/// Resolve a requested model for `agent` against the models it actually offers.
///
/// `None`/blank takes the first available model, or the agent's own default when
/// nothing is available. A named model must exactly match (trimmed,
/// case-sensitive) an available one; an unknown one is refused rather than passed
/// through. An agent with no available models ignores any request and yields its
/// own default.
pub fn resolve_model(
    agent: ReviewAgent,
    available: &[String],
    requested: Option<&str>,
) -> Result<Option<String>, String> {
    match requested.map(str::trim) {
        // No request: the first available model, else the agent's own default.
        None | Some("") => Ok(available
            .first()
            .cloned()
            .or_else(|| agent.default_model().map(str::to_string))),
        // Nothing to offer: run the agent's default, ignoring a stray request.
        Some(_) if available.is_empty() => Ok(agent.default_model().map(str::to_string)),
        Some(name) => available
            .iter()
            .find(|candidate| candidate.as_str() == name)
            .cloned()
            .map(Some)
            .ok_or_else(|| {
                format!(
                    "unknown {} model {name:?}; choose one of {}",
                    agent.label(),
                    available.join(", ")
                )
            }),
    }
}

/// A clear divider between injected review context and the prompt that acts on
/// it. Visually unmistakable so neither half bleeds into the other.
pub const CONTEXT_SEPARATOR: &str = "\n\n---\n\n";

/// Prepend review context — evidence, business rules — to a prompt body.
///
/// The context leads so the agent reads the material *before* the instruction
/// that acts on it. Blank or absent context is a no-op: the body is returned
/// unchanged rather than gaining a stray separator with nothing above it. Pure,
/// so the ordering is pinned by a test rather than discovered in a live run.
pub fn compose_prompt(context: Option<&str>, body: &str) -> String {
    match context.map(str::trim).filter(|c| !c.is_empty()) {
        Some(context) => format!("{context}{CONTEXT_SEPARATOR}{body}"),
        None => body.to_string(),
    }
}

/// Resolve which prompt body a review will run.
///
/// A run can be driven by a **library** prompt (the Review / Run Agent entries)
/// or by an **inline** body typed elsewhere (a Notes-panel note sent to the
/// agent). An inline body wins when present — it is the more specific request —
/// and a blank inline body is treated as absent so an accidental empty string
/// does not shadow a chosen library prompt. With neither, there is nothing to
/// run and that is an error, not a silent empty prompt. Pure, so the precedence
/// is pinned by a test.
pub fn resolve_prompt_body(inline: Option<&str>, library: Option<&str>) -> Result<String, String> {
    if let Some(inline) = inline.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(inline.to_string());
    }
    match library {
        Some(body) => Ok(body.to_string()),
        None => Err("no prompt to run: neither an inline body nor a library prompt".into()),
    }
}

/// The argument vector for the agent, given a posture, a resolved model and the
/// prompt body.
///
/// The prompt is one argv entry, never shell-joined, so a multi-line body with
/// spaces cannot be split or reinterpreted; and it is passed as an argument (not
/// stdin), because the supervisor runs every child with stdin closed. The `mode`
/// picks the permission/sandbox posture — neither posture ever prompts, so a
/// closed stdin cannot hang either.
pub fn agent_args(
    agent: ReviewAgent,
    mode: AgentMode,
    model: Option<&str>,
    prompt: &str,
) -> Vec<String> {
    match agent {
        ReviewAgent::ClaudeCode => {
            // claude -p <prompt> [--model <m>] --permission-mode <mode>
            //        --output-format stream-json --verbose
            //
            // Stream-json (which the CLI requires --verbose for) emits each step
            // as it happens; the default text mode buffers the whole answer to
            // the end, which looks hung during a long run. The frontend renders
            // these NDJSON events into readable console output.
            let permission = match mode {
                AgentMode::ReadOnly => CLAUDE_PERMISSION_MODE,
                AgentMode::Edit => CLAUDE_EDIT_PERMISSION_MODE,
            };
            let mut args = vec!["-p".to_string(), prompt.to_string()];
            if let Some(m) = model {
                args.push("--model".to_string());
                args.push(m.to_string());
            }
            args.push("--permission-mode".to_string());
            args.push(permission.to_string());
            args.push("--output-format".to_string());
            args.push("stream-json".to_string());
            args.push("--verbose".to_string());
            args
        }
        ReviewAgent::Codex => {
            // codex exec --sandbox <mode> [-m <m>] <prompt>
            let sandbox = match mode {
                AgentMode::ReadOnly => CODEX_SANDBOX,
                AgentMode::Edit => CODEX_EDIT_SANDBOX,
            };
            let mut args = vec![
                "exec".to_string(),
                "--sandbox".to_string(),
                sandbox.to_string(),
            ];
            if let Some(m) = model {
                args.push("-m".to_string());
                args.push(m.to_string());
            }
            // The prompt is Codex's trailing positional argument.
            args.push(prompt.to_string());
            args
        }
    }
}

/// The agents whose CLI is actually installed, in preference order.
///
/// PATH-dependent, so not unit-tested; the pure pieces above are. On non-Windows
/// `resolve_program` is an identity function, so this reports nothing there —
/// acceptable for a Windows-targeted app.
pub fn detect_agents() -> Vec<ReviewAgent> {
    ReviewAgent::ALL
        .into_iter()
        .filter(|a| is_installed(a.program()))
        .collect()
}

fn is_installed(program: &str) -> bool {
    let resolved = crate::process::resolve_program(program);
    resolved != Path::new(program) && resolved.is_file()
}
