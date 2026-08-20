//! The quality-gate Stop hook: deterministic checks when an agent turn ends.
//!
//! This is the runtime counterpart of the intent recorder ([`crate::intents`]):
//! an agent `Stop` hook runs this application — `cb-app quality-gate` — rather
//! than a shipped script, so there is no second artifact to keep in step and no
//! interpreter to depend on being present. It is installed the same way the
//! intent hooks are (see [`install`]).
//!
//! **Every decision lives here, and is pure.** Which checks a change set
//! warrants, whether a file still carries an unresolved rejection note, whether
//! a turn deserves a memory-update reminder, and whether this is even a
//! quality-gate invocation are all decided by the functions below and unit
//! tested headlessly. The thin runner in `src-tauri` does only the I/O it must:
//! read the payload, run `git`, spawn `pnpm`/`cargo`, and turn the verdict into
//! an exit code (2 blocks a Claude Code stop and shows the model the reason).
//!
//! The governing rule is the same abstain-rather-than-guess discipline as the
//! rest of the crate: a check only runs when the change set and the repo's
//! tooling both call for it, so a user-scope gate that fires in every
//! repository stays silent where it does not apply.

pub mod install;

use serde_json::Value;

/// A blocking check the gate can run. Each maps to one command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// `pnpm typecheck` — `tsc --noEmit`.
    Typecheck,
    /// `cargo fmt --check` — fast, never relinks.
    Rustfmt,
    /// `cargo clippy` — opt-in only (`CB_GATE_FULL`), because it relinks and
    /// can hit the "app is running ⇒ Access denied" lock.
    Clippy,
}

impl Gate {
    /// A short label for the blocked-turn message.
    pub fn label(self) -> &'static str {
        match self {
            Gate::Typecheck => "pnpm typecheck",
            Gate::Rustfmt => "cargo fmt --check",
            Gate::Clippy => "cargo clippy",
        }
    }

    /// The program and arguments to spawn. The program is a bare name resolved
    /// on `PATH` by the caller (via `process::resolve_program`), which finds the
    /// `.cmd` shim on Windows.
    pub fn command(self) -> (&'static str, Vec<&'static str>) {
        match self {
            Gate::Typecheck => ("pnpm", vec!["typecheck"]),
            Gate::Rustfmt => ("cargo", vec!["fmt", "--check"]),
            Gate::Clippy => (
                "cargo",
                vec![
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ],
            ),
        }
    }
}

/// Extensions that trigger the TypeScript gate.
fn is_ts(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".ts", ".tsx", ".mts", ".cts"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Source roots that count as "real work" for the memory reminder. A change
/// only under docs/config/memory does not warrant a `.memories/` update.
const SOURCE_ROOTS: &[&str] = &["src/", "src-tauri/", "crates/", "sidecar/"];

/// Normalise a path the way the change set is compared: forward slashes.
fn normalise(path: &str) -> String {
    path.replace('\\', "/")
}

/// Which blocking gates a change set warrants. `full` adds the opt-in clippy
/// pass after rustfmt so a fmt failure surfaces first.
///
/// Extension-driven and order-stable (typecheck, then the cargo gates) so the
/// blocked-turn message is deterministic.
pub fn gates_for_changes(changed: &[String], full: bool) -> Vec<Gate> {
    let paths: Vec<String> = changed.iter().map(|p| normalise(p)).collect();
    let mut gates = Vec::new();
    if paths.iter().any(|p| is_ts(p)) {
        gates.push(Gate::Typecheck);
    }
    if paths.iter().any(|p| p.ends_with(".rs")) {
        gates.push(Gate::Rustfmt);
        if full {
            gates.push(Gate::Clippy);
        }
    }
    gates
}

/// Did the change set touch real source (so an intent/memory note is expected)?
pub fn touched_source(changed: &[String]) -> bool {
    changed
        .iter()
        .map(|p| normalise(p))
        .any(|p| SOURCE_ROOTS.iter().any(|root| p.starts_with(root)))
}

/// Was any `.memories/` file part of the change set?
pub fn touched_memories(changed: &[String]) -> bool {
    changed
        .iter()
        .map(|p| normalise(p))
        .any(|p| p.starts_with(".memories/"))
}

/// Should the (non-blocking) memory reminder fire — source changed but no
/// `.memories/` file touched?
pub fn should_remind_memories(changed: &[String]) -> bool {
    touched_source(changed) && !touched_memories(changed)
}

/// The AI-REJECTED head-line token, assembled so this source file does not
/// itself contain the literal token its regex scans for — matching the git
/// `pre-commit` guard's reasoning (`intents::guard`).
fn reject_token() -> String {
    concat!("AI-", "REJECTED").to_string()
}

/// Does a file's text carry an unresolved rejection note (a date-stamped head
/// line, `AI-REJECTED YYYY-MM-DD`)? The bare token in prose is committable and
/// is deliberately not matched — identical to what the `pre-commit` hook
/// refuses, so detection here matches what the committer will later block.
pub fn has_unresolved_rejection(text: &str) -> bool {
    let token = reject_token();
    // Scan by hand rather than pulling in a regex: find the token, then require
    // " NNNN-NN-NN" immediately after it.
    let mut from = 0;
    while let Some(rel) = text[from..].find(&token) {
        let start = from + rel + token.len();
        if is_dated_suffix(&text[start..]) {
            return true;
        }
        from = start;
    }
    false
}

/// True when `rest` begins with " YYYY-MM-DD".
fn is_dated_suffix(rest: &str) -> bool {
    let bytes = rest.as_bytes();
    if bytes.len() < 11 || bytes[0] != b' ' {
        return false;
    }
    let digit = |i: usize| bytes[i].is_ascii_digit();
    let dash = |i: usize| bytes[i] == b'-';
    digit(1)
        && digit(2)
        && digit(3)
        && digit(4)
        && dash(5)
        && digit(6)
        && digit(7)
        && dash(8)
        && digit(9)
        && digit(10)
}

/// The Stop-hook loop guard. Claude Code sets `stop_hook_active` true once a
/// Stop hook has already fired this turn; re-blocking then would loop forever.
pub fn should_skip_for_loop(payload: &Value) -> bool {
    payload
        .get("stop_hook_active")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Does a `package.json` declare a `typecheck` script? Guards the TypeScript
/// gate so a user-scope hook firing in a repo without one does not fail the
/// turn on a missing script.
pub fn has_typecheck_script(package_json: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(package_json) else {
        return false;
    };
    value
        .get("scripts")
        .and_then(|s| s.get("typecheck"))
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty())
}

/// The subcommand name the installed hook runs.
pub const SUBCOMMAND: &str = "quality-gate";

/// Did the command line ask for the quality gate rather than the application?
///
/// Like the recorder, either the subcommand or the marker flag alone is
/// accepted, because the line lives in a config file the user shares with their
/// team and a hand-edit keeping only one must still work.
pub fn is_quality_gate_invocation(args: &[String]) -> bool {
    let marker = format!("--{}", install::MARKER);
    args.iter().any(|a| a == SUBCOMMAND || *a == marker)
}

/// What a `quality-gate` command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QGateInvocation {
    /// The workspace the hook was installed for, if it named one. `None` is a
    /// user-scope install, leaving the root to the payload's `cwd`.
    pub workspace: Option<String>,
    /// Whether to run the opt-in heavier checks (from `CB_GATE_FULL`).
    pub full: bool,
}

/// Read a `quality-gate` command line. `None` means this is not a gate
/// invocation and there is nothing to do.
pub fn parse_qgate_args(args: &[String]) -> Option<QGateInvocation> {
    if !is_quality_gate_invocation(args) {
        return None;
    }
    Some(QGateInvocation {
        workspace: flag(args, "--workspace").filter(|w| !w.is_empty()),
        full: std::env::var_os("CB_GATE_FULL").is_some_and(|v| v == "1"),
    })
}

/// Read `--name value` from the command line; first occurrence wins.
fn flag(args: &[String], name: &str) -> Option<String> {
    let position = args.iter().position(|a| a == name)?;
    args.get(position + 1).cloned()
}

#[cfg(test)]
#[path = "decide_tests.rs"]
mod tests;
