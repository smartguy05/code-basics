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
//!
//! A gate that *did* run and failed is not automatically a failure either —
//! [`cb_core::qgate::read_gate_output`] decides whether its output judges the
//! code or describes this machine, and this file only routes the three answers:
//! block, stay quiet, or announce on stderr that nothing was checked.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use cb_core::erosion;
use cb_core::git::{ComparisonMode, Repo};
use cb_core::process::resolve_program;
use cb_core::qgate::{self, Gate, GateVerdict};
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
    //
    // `package.json` is read once and handed to the verdict reader: only a
    // package the manifest already declares can be an *environment* problem, so
    // without it an unresolved import stays the failure it looks like.
    let manifest = std::fs::read_to_string(root.join("package.json")).unwrap_or_default();
    let declared = qgate::declared_dependencies(&manifest);

    for gate in qgate::gates_for_changes(&changed, inv.full) {
        if !applicable(gate, &manifest, &root) {
            continue;
        }
        match run_gate(gate, &root, &declared) {
            GateVerdict::Passed => {}
            GateVerdict::Failed(output) => {
                return Ok(Some(format!(
                    "Quality gate failed: {}\n\
                     Fix the reported problems before finishing this turn.\n\n{}",
                    gate.label(),
                    output
                )));
            }
            // Not a verdict on the change set, so it must not block — but it is
            // also not a pass, and saying nothing would let it read as one.
            // Announced on stderr beside the other advisories.
            GateVerdict::Unrunnable { summary, detail } => {
                eprintln!("{summary}\n\n{detail}");
            }
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
        let mut cmd = Command::new("git");
        cmd.args(args).current_dir(root);
        #[cfg(windows)]
        cb_core::process::no_window(&mut cmd);
        if let Ok(output) = cmd.output() {
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
///
/// `manifest` is the already-read `package.json` text, empty when there is none.
fn applicable(gate: Gate, manifest: &str, root: &Path) -> bool {
    match gate {
        Gate::Typecheck => qgate::has_typecheck_script(manifest),
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

/// Run a gate and read what its run meant.
///
/// A gate that could not even be **spawned** is `Passed` — the long-standing
/// abstain-safe reading, since blocking on a check that never ran would be a
/// guess. A gate that ran and exited non-zero is handed to
/// [`qgate::read_gate_output`], which decides whether its output is a verdict on
/// the code or a description of this machine; this function makes no such
/// judgement of its own.
fn run_gate(gate: Gate, root: &Path, declared: &[String]) -> GateVerdict {
    let (program, args) = gate.command();
    let mut cmd = Command::new(resolve_program(program));
    cmd.args(&args).current_dir(root);
    #[cfg(windows)]
    cb_core::process::no_window(&mut cmd);
    let Ok(output) = cmd.output() else {
        return GateVerdict::Passed;
    };
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    qgate::read_gate_output(gate, output.status.success(), &combined, declared)
}
