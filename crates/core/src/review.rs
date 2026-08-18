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

/// Resolve a requested model for `agent`.
///
/// `None`/blank takes the agent's default. A named model must be one the agent
/// offers; an unknown one is refused rather than passed through. An agent that
/// offers no models ignores any request and yields its own default.
pub fn resolve_model(
    agent: ReviewAgent,
    requested: Option<&str>,
) -> Result<Option<&'static str>, String> {
    let models = agent.models();
    match requested.map(str::trim) {
        // No request, or an agent with nothing to offer: its own default.
        None | Some("") => Ok(agent.default_model()),
        Some(_) if models.is_empty() => Ok(agent.default_model()),
        Some(name) => models
            .iter()
            .copied()
            .find(|candidate| *candidate == name)
            .map(Some)
            .ok_or_else(|| {
                format!(
                    "unknown {} model {name:?}; choose one of {}",
                    agent.label(),
                    models.join(", ")
                )
            }),
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
