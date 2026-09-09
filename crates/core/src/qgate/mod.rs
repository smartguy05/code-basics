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
//!
//! # A non-zero exit is not always a verdict
//!
//! The sharpest form of that rule lives in [`read_gate_output`]. A gate can fail
//! for reasons that have nothing to do with the change set — an unreadable pnpm
//! junction, a running executable the linker cannot replace — and its output
//! then describes *this machine*, not the code. Blocking a turn on that is the
//! same guess as blocking on a check that never spawned, and it is worse than
//! useless: the turn cannot be unblocked by any edit.
//!
//! So a failed run resolves to one of **three** answers, not two, and the third
//! neither blocks nor passes silently — it reports that nothing was checked.
//! Two rules keep it honest. It never sifts a broken run for the "real" errors
//! among the cascade, because once the dependency tree is unreachable the
//! `any`-poisoned diagnostics are indistinguishable from genuine ones and
//! choosing between them would be the guess this crate refuses. And a missing
//! package is environmental *only* if `package.json` already declares it —
//! importing something never installed stays the ordinary failure it looks like.

pub mod install;

use serde_json::Value;

use crate::erosion::ErosionReport;

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

/// How many individual erosion flags the reminder lists before it summarises
/// the remainder as a count, so a large diff cannot flood the turn-end output.
const EROSION_FLAG_CAP: usize = 20;

/// A **non-blocking** reminder of the erosion signals in the change set, or
/// `None` when the scan found nothing (empty `flags`).
///
/// Advisory only, and deliberately so: the erosion detector ranks nothing and
/// carries no severity, so this states no verdict — it lists what was located
/// (`path:line  category  message` per flag, capped with an "and N more" tail)
/// and leaves the judgement to the reader. It sits with the memory reminder,
/// never with the blocking gates.
pub fn erosion_reminder(report: &ErosionReport) -> Option<String> {
    if report.flags.is_empty() {
        return None;
    }
    let total = report.flags.len();
    let mut lines = vec![format!(
        "{total} potential erosion signal{}:",
        if total == 1 { "" } else { "s" }
    )];
    for flag in report.flags.iter().take(EROSION_FLAG_CAP) {
        lines.push(format!(
            "  {}:{}  {:?}  {}",
            flag.path, flag.line, flag.category, flag.message
        ));
    }
    if total > EROSION_FLAG_CAP {
        lines.push(format!("  ...and {} more", total - EROSION_FLAG_CAP));
    }
    Some(lines.join("\n"))
}

// ---------------------------------------------------------------------------
// Reading a failed gate's output
// ---------------------------------------------------------------------------

/// What a gate's output means.
///
/// The distinction that matters is the third variant. A gate that exits non-zero
/// has said *something*, but not always about the code: if it could not reach
/// its own dependencies, its diagnostics describe the machine. Blocking a turn
/// on those is the same guess as blocking on a check that never spawned — which
/// the runner already refuses to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateVerdict {
    /// The check ran and found nothing.
    Passed,
    /// The check ran and found problems. Carries its output **whole**.
    Failed(String),
    /// The check ran but could not reach what it needed to judge the code, so
    /// it rendered no verdict at all. Never blocks; always says so out loud.
    Unrunnable {
        /// What happened and what it does *not* mean, for the model to read.
        summary: String,
        /// The output that led here, capped — it is noise by construction.
        detail: String,
    },
}

/// How many lines of an [`GateVerdict::Unrunnable`] output are shown before the
/// rest is summarised as a count.
///
/// A broken dependency tree cascades: the incident this cap exists for produced
/// roughly five thousand diagnostics from one unreadable directory. Handing all
/// of them back is its own failure mode, and none of them were verdicts.
/// A genuine [`GateVerdict::Failed`] is deliberately **not** capped — the one
/// line that matters could be anywhere in it.
pub const UNRUNNABLE_DETAIL_LINES: usize = 20;

/// Quoted-specifier prefixes that mean "a module could not be resolved". Both
/// TypeScript's wordings and Node's `ERR_MODULE_NOT_FOUND`, since a `typecheck`
/// script may be either.
const UNRESOLVED_PREFIXES: &[&str] = &[
    "Cannot find module '",
    "Could not find a declaration file for module '",
    "Cannot find type definition file for '",
    "Cannot find package '",
];

/// Substrings that mean the toolchain itself could not be reached, whatever the
/// gate. Each is a message from the operating system rather than from a
/// compiler, and each is a documented local hazard rather than a code defect:
/// an unreadable pnpm junction, and a running executable that Windows will not
/// let the linker replace.
const TOOLCHAIN_BLOCKED: &[&str] = &[
    "untrusted mount point",
    "os error 448",
    "Access is denied. (os error 5)",
    "Permission denied (os error 13)",
];

/// The package a module specifier belongs to, or `None` when the specifier is
/// not a package at all.
///
/// A relative or absolute path is never environmental: a missing sibling file is
/// the author's own doing however the dependency tree is laid out, so it must
/// keep blocking. A scoped name keeps both segments; a subpath and a `?query`
/// (Vite's `?raw`, say) are stripped, because it is the *package* that is either
/// reachable or not.
fn package_of(specifier: &str) -> Option<String> {
    let spec = specifier.trim();
    if spec.is_empty() || spec.starts_with('.') || spec.starts_with('/') || spec.starts_with('\\') {
        return None;
    }
    // A Windows drive prefix ("C:/repo/thing") is a path, not a package.
    if spec.as_bytes().get(1) == Some(&b':') {
        return None;
    }
    let without_query = spec.split('?').next().unwrap_or(spec);
    let mut segments = without_query.split('/');
    let first = segments.next().filter(|s| !s.is_empty())?;
    if let Some(scope) = first.strip_prefix('@') {
        if scope.is_empty() {
            return None;
        }
        let name = segments.next().filter(|s| !s.is_empty())?;
        return Some(format!("{first}/{name}"));
    }
    Some(first.to_string())
}

/// Every package a gate's output said it could not resolve, deduplicated and
/// sorted. Scans by hand rather than pulling in a regex, matching
/// [`has_unresolved_rejection`].
pub fn unresolved_packages(output: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for prefix in UNRESOLVED_PREFIXES {
        let mut from = 0;
        while let Some(rel) = output[from..].find(prefix) {
            let start = from + rel + prefix.len();
            let Some(end) = output[start..].find('\'') else {
                break;
            };
            if let Some(package) = package_of(&output[start..start + end]) {
                if !found.contains(&package) {
                    found.push(package);
                }
            }
            from = start + end;
        }
    }
    found.sort();
    found
}

/// The package names a `package.json` promises are installed — `dependencies`
/// and `devDependencies` together.
///
/// This is what separates "the install is unreachable" from "you imported
/// something that was never added": only a package the manifest already
/// declares can be an environment problem. Abstains to an empty list on a
/// manifest that will not parse, so an unreadable file can never *widen* what
/// the gate forgives.
pub fn declared_dependencies(package_json: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(package_json) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for field in ["dependencies", "devDependencies"] {
        if let Some(map) = value.get(field).and_then(Value::as_object) {
            names.extend(map.keys().cloned());
        }
    }
    names
}

/// Cap an output at [`UNRUNNABLE_DETAIL_LINES`], saying what was dropped.
fn capped(output: &str) -> String {
    let lines: Vec<&str> = output.trim().lines().collect();
    if lines.len() <= UNRUNNABLE_DETAIL_LINES {
        return lines.join("\n");
    }
    let dropped = lines.len() - UNRUNNABLE_DETAIL_LINES;
    let mut shown = lines[..UNRUNNABLE_DETAIL_LINES].join("\n");
    shown.push_str(&format!(
        "\n  ...and {dropped} more line{}",
        if dropped == 1 { "" } else { "s" }
    ));
    shown
}

/// Build the "could not run" verdict, including the warning that must travel
/// with it: this is not a pass, and a genuine problem may be sitting in the
/// noise unread.
fn unrunnable(gate: Gate, reason: &str, output: &str) -> GateVerdict {
    GateVerdict::Unrunnable {
        summary: format!(
            "{} could not run: {reason}.\n\
             Its output describes this machine, not this turn's changes, so the gate is \
             not blocking — but it has also checked nothing. A real problem may be hidden \
             in the output below: read it, and do not report the gate as passing.",
            gate.label()
        ),
        detail: capped(output),
    }
}

/// Read what a gate's run means: a verdict on the code, or on the machine.
///
/// `declared` is the package list from `package.json` (see
/// [`declared_dependencies`]); an empty list simply means no unresolved module
/// can be forgiven, which is the safe direction.
///
/// **The gate never sifts a broken run for real errors.** Once the dependency
/// tree is unreachable, `any` poisoning invents diagnostics that would vanish in
/// a healthy tree, so telling the two apart would be exactly the guess this
/// crate refuses everywhere else. It abstains from the whole run instead, keeps
/// the output, and says a real problem may be hidden in it.
pub fn read_gate_output(
    gate: Gate,
    success: bool,
    output: &str,
    declared: &[String],
) -> GateVerdict {
    if success {
        return GateVerdict::Passed;
    }
    if let Some(marker) = TOOLCHAIN_BLOCKED.iter().find(|m| output.contains(**m)) {
        return unrunnable(
            gate,
            &format!("the toolchain reported \"{marker}\""),
            output,
        );
    }
    let unreachable: Vec<String> = unresolved_packages(output)
        .into_iter()
        .filter(|package| declared.iter().any(|d| d == package))
        .collect();
    if !unreachable.is_empty() {
        let reason = format!(
            "{} installed package{} could not be resolved ({})",
            unreachable.len(),
            if unreachable.len() == 1 { "" } else { "s" },
            unreachable.join(", ")
        );
        return unrunnable(gate, &reason, output);
    }
    GateVerdict::Failed(output.trim().to_string())
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
