//! The `quality-gate` mode: deterministic checks when an agent turn ends.
//!
//! Like the recorder ([`crate::recorder`]), an agent `Stop` hook runs this
//! executable rather than a shipped script. **Every decision lives in
//! [`cb_core::qgate`]** and is unit tested there; this file does only the I/O it
//! must — read the payload, run `git`, spawn `pnpm`/`cargo`, and turn the
//! verdict into an exit code. Exit 2 blocks a Claude Code stop and shows the
//! model the reason; every other path exits 0 so a session can always end.
//!
//! It abstains rather than guesses, sharpened for a hook that a user-scope
//! install fires in every repository: a gate only runs when the change set
//! *and* the repo's tooling both call for it, and a check we could not even
//! spawn is treated as "nothing to say", never as a failure.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use cb_core::erosion;
use cb_core::git::{ComparisonMode, Repo};
use cb_core::process::resolve_program;
use cb_core::qgate::{self, Gate};
use serde_json::Value;

/// Exit code that makes a Claude Code `Stop` hook block the stop and show the
/// hook's stderr to the model.
const BLOCK_STOP: i32 = 2;

/// Did the command line ask for the quality gate rather than the application?
pub fn is_quality_gate_invocation() -> bool {
    qgate::is_quality_gate_invocation(&std::env::args().collect::<Vec<_>>())
}

/// Read the Stop payload, run the gate, and exit.
pub fn run() {
    match gate() {
        Ok(Some(message)) => {
            eprintln!("{message}");
            std::process::exit(BLOCK_STOP);
        }
        Ok(None) => {}
        Err(error) => {
            if std::env::var_os("CODE_BASICS_DEBUG_HOOKS").is_some() {
                eprintln!("code-basics: quality gate error: {error:#}");
            }
        }
    }
}

/// Returns the block message when the turn must not stop, `None` to allow it.
fn gate() -> anyhow::Result<Option<String>> {
    let args: Vec<String> = std::env::args().collect();
    let Some(inv) = qgate::parse_qgate_args(&args) else {
        return Ok(None);
    };

    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw)?;
    let payload: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);

    // Never re-block a turn a prior Stop hook already handled.
    if qgate::should_skip_for_loop(&payload) {
        return Ok(None);
    }

    let Some(root) = resolve_root(inv.workspace.as_deref(), &payload) else {
        return Ok(None);
    };

    let changed = changed_paths(&root);
    if changed.is_empty() {
        return Ok(None);
    }

    // Blocking language gates, in the order cb_core decided.
    for gate in qgate::gates_for_changes(&changed, inv.full) {
        if !applicable(gate, &root) {
            continue;
        }
        if let Some(output) = failing_output(gate, &root) {
            return Ok(Some(format!(
                "Quality gate failed: {}\n\
                 Fix the reported problems before finishing this turn.\n\n{}",
                gate.label(),
                output
            )));
        }
    }

    // AI-REJECTED detector: surface the pre-commit refusal at turn end.
    let flagged: Vec<&String> = changed
        .iter()
        .filter(|rel| {
            std::fs::read_to_string(root.join(rel))
                .map(|t| qgate::has_unresolved_rejection(&t))
                .unwrap_or(false)
        })
        .collect();
    if !flagged.is_empty() {
        let list = flagged
            .iter()
            .map(|f| format!("  {f}"))
            .collect::<Vec<_>>()
            .join("\n");
        return Ok(Some(format!(
            "Unresolved {token} note(s) in changed files:\n{list}\n\n\
             Implement a correct fix that addresses the stated reason, then delete \
             the whole note block in the same edit (a commit that still carries one \
             is refused by the pre-commit hook).",
            token = concat!("AI-", "REJECTED"),
        )));
    }

    // Erosion advisory — non-blocking (stderr, exit 0). The detector ranks
    // nothing and carries no severity, so it never gates; it sits with the
    // memory reminder. Reached only after the blocking checks short-circuited.
    // Abstain on any failure: a diff we could not compute says nothing.
    if let Some(msg) = erosion_reminder(&root) {
        eprintln!("{msg}");
    }

    // Memory advisory — non-blocking (stderr, exit 0).
    if qgate::should_remind_memories(&changed) {
        eprintln!(
            "Reminder: this turn edited source but touched no .memories/ file. If this \
             work item's state changed, update its work-item memory (notes.md / \
             todos.md / completed.md)."
        );
    }

    Ok(None)
}

/// The workspace to gate: the explicitly named one, else the payload's `cwd`.
/// Must be a real directory.
fn resolve_root(explicit: Option<&str>, payload: &Value) -> Option<PathBuf> {
    let candidate = explicit.map(PathBuf::from).or_else(|| {
        payload
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from)
    })?;
    candidate.is_dir().then_some(candidate)
}

/// Working-tree change set vs HEAD: tracked modifications plus untracked files.
fn changed_paths(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let runs: [&[&str]; 2] = [
        &["diff", "--name-only", "HEAD"],
        &["ls-files", "--others", "--exclude-standard"],
    ];
    for args in runs {
        if let Ok(output) = Command::new("git").args(args).current_dir(root).output() {
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    let path = line.trim();
                    if !path.is_empty() {
                        out.push(path.to_string());
                    }
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Does this repo have the tooling a gate needs? Keeps a user-scope hook from
/// failing a turn in a repository that has no `typecheck` script or no cargo.
fn applicable(gate: Gate, root: &Path) -> bool {
    match gate {
        Gate::Typecheck => std::fs::read_to_string(root.join("package.json"))
            .map(|t| qgate::has_typecheck_script(&t))
            .unwrap_or(false),
        Gate::Rustfmt | Gate::Clippy => root.join("Cargo.toml").exists(),
    }
}

/// The non-blocking erosion advisory for the working tree, or `None` to say
/// nothing. Delegates the wording and the empty-vs-something decision to
/// [`qgate::erosion_reminder`]; this does only the I/O (open the repo, compute
/// the diff, run the rules) and **abstains on any error** — a scan we could not
/// run must never speak, exactly as the blocking gates abstain on a check they
/// could not spawn.
fn erosion_reminder(root: &Path) -> Option<String> {
    let repo = Repo::open(root).ok()?;
    let diffs = repo.diff_all(ComparisonMode::WorkingToHead).ok()?;
    let (rules, _warnings) = erosion::all_rules(root);
    let report = erosion::scan_diffs(&diffs, &rules);
    qgate::erosion_reminder(&report)
}

/// Run a gate. `Some(output)` means it failed (and carries the combined
/// stdout+stderr); `None` means it passed *or could not be spawned* — the
/// abstain-safe reading, since blocking on a check we could not run would be a
/// guess.
fn failing_output(gate: Gate, root: &Path) -> Option<String> {
    let (program, args) = gate.command();
    let output = Command::new(resolve_program(program))
        .args(&args)
        .current_dir(root)
        .output()
        .ok()?;
    if output.status.success() {
        return None;
    }
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    Some(combined.trim().to_string())
}
