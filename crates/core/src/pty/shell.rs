//! Pure decisions for the PTY layer: which shell to launch, and clamping the
//! terminal dimensions. Extracted so they are testable without spawning
//! anything — the same split `process/resolve.rs` uses for its PATHEXT walk.

use std::path::{Path, PathBuf};

use super::model::{DetectedShells, ShellInfo};

/// Floor terminal dimensions to at least 1×1.
///
/// xterm's fit addon can momentarily report a 0 dimension (a hidden or
/// zero-height pane), and ConPTY/forkpty both reject a zero size. A terminal
/// that briefly measures 0 should keep its last usable size rather than error,
/// so the floor is 1 rather than a hard refusal.
pub fn clamp_size(cols: u16, rows: u16) -> (u16, u16) {
    (cols.max(1), rows.max(1))
}

/// Pick the first candidate the predicate reports available, falling back to
/// the **last** candidate even when none are available.
///
/// The fallback is deliberate: returning something to spawn lets the spawn
/// error name what was tried, which is more useful than an empty string. On
/// every platform the last candidate is the one that effectively always exists
/// (`cmd` on Windows, `/bin/sh` on Unix), so the fallback is also the safe
/// default.
///
/// **[`detected_shells`] does the opposite and must keep doing the opposite.**
/// This answers "what do I spawn", where returning nothing is useless; that
/// answers "what can the user pick", where offering a shell that is not there
/// is worse than offering nothing. Do not make either one imitate the other.
pub fn pick_shell(candidates: &[&str], available: impl Fn(&str) -> bool) -> String {
    for candidate in candidates {
        if available(candidate) {
            return (*candidate).to_string();
        }
    }
    candidates
        .last()
        .map(|s| (*s).to_string())
        .unwrap_or_default()
}

/// The interactive shell to launch for a new terminal.
///
/// Windows prefers PowerShell 7 (`pwsh`), then Windows PowerShell
/// (`powershell`), then `cmd`, resolving each on PATH the same way process
/// spawning does. Unix honours `$SHELL` when set, else `/bin/bash`, else
/// `/bin/sh`.
pub fn default_shell() -> String {
    #[cfg(windows)]
    {
        pick_shell(&["pwsh", "powershell", "cmd"], |candidate| {
            crate::process::resolve_program(candidate) != std::path::Path::new(candidate)
        })
    }
    #[cfg(unix)]
    {
        if let Some(shell) = std::env::var_os("SHELL") {
            let shell = shell.to_string_lossy().into_owned();
            if !shell.is_empty() {
                return shell;
            }
        }
        pick_shell(&["/bin/bash", "/bin/sh"], |candidate| {
            std::path::Path::new(candidate).exists()
        })
    }
}

/// Whether an inherited environment variable marks this process as running
/// *inside* a Claude Code session, and so must be stripped before spawning a
/// terminal.
///
/// The app is often launched from within a Claude Code session (that is how it
/// is developed), and a PTY inherits the whole parent environment. Left in
/// place, these markers make a `claude` the user runs in the terminal believe
/// it is a nested child session: it turns transcript saving off (the
/// `CLAUDE_CODE_CHILD_SESSION` marker does exactly that) and would even reuse
/// the parent session's IPC socket. A terminal the user opens deliberately
/// should behave like a fresh top-level shell, so every one of these is removed
/// and the nested session starts clean.
///
/// Matched by Claude Code's whole `CLAUDE_CODE_*` namespace — so a marker added
/// in a future release is stripped without a code change here — plus the four
/// bare markers that live outside that prefix. A user's own unrelated
/// `CLAUDE_*` variable (say `CLAUDE_API_KEY`) is deliberately **not** matched:
/// only the exact injected names and the reserved namespace are.
pub fn is_session_marker(name: &str) -> bool {
    name.starts_with("CLAUDE_CODE_")
        || matches!(
            name,
            "CLAUDECODE" | "CLAUDE_PID" | "CLAUDE_EFFORT" | "AI_AGENT"
        )
}

/// One shell the app knows how to look for, before it is known whether it is
/// here.
///
/// Separate from [`ShellInfo`] on purpose: a candidate is a *question* (does
/// this exist?) and a `ShellInfo` is an *answer* about a file that was found,
/// so the two can never be confused for one another. `probe` is what gets
/// asked, `program` is what came back.
#[derive(Debug, Clone)]
pub struct ShellCandidate {
    /// The stable id the answer carries, and the frontend persists. Several
    /// candidates may share one (`/bin/zsh` and `/usr/bin/zsh` are both `zsh`):
    /// whichever is found first is *the* zsh, and the rest are dropped by
    /// [`detected_shells`].
    pub id: String,
    /// The label the answer carries. Fixed text, never derived from a version
    /// nobody read.
    pub label: String,
    /// What to hand `locate`: a bare name to resolve on PATH (Windows, where
    /// installers put shells in wildly different directories), or an absolute
    /// path to test (Unix, where they do not).
    pub probe: String,
    /// Arguments the shell needs. Empty for every candidate today — but they
    /// are part of the identity of a launch, so they travel with the candidate
    /// rather than being bolted on later.
    pub args: Vec<String>,
}

fn candidate(id: &str, label: &str, probe: &str) -> ShellCandidate {
    ShellCandidate {
        id: id.to_string(),
        label: label.to_string(),
        probe: probe.to_string(),
        args: Vec::new(),
    }
}

/// The shells worth looking for on this platform, **in preference order** —
/// which [`detected_shells`] preserves, because it is the order the picker
/// shows and the order that expresses what the app would rather launch.
///
/// Two deliberate absences on Windows, both of which would otherwise be easy
/// to add and wrong:
///
/// - **`wsl`.** `wsl.exe` ships in System32 on every modern Windows install
///   whether or not a distribution exists, so its presence is *not* evidence a
///   shell will start. Deciding honestly needs `wsl.exe -l -q` — a subprocess
///   spawn, its latency, and its hang risk, inside what is meant to be a
///   "what is here" probe. Listing it on file presence would be exactly the
///   guess this codebase forbids, so it is not listed at all.
/// - **`git-bash.exe`.** It launches its own MinTTY window instead of running a
///   shell attached to the PTY we allocated, so picking it would open a
///   terminal pane with nothing in it.
///
/// Labels assert nothing that was not verified: `pwsh` is "PowerShell (pwsh)"
/// and not "PowerShell 7", because `pwsh` 6 exists and no version was read.
/// [`ShellInfo::program`] — the resolved path — is what actually tells two
/// installations of the same shell apart, which is why the UI shows it.
pub fn shell_candidates() -> Vec<ShellCandidate> {
    #[cfg(windows)]
    {
        vec![
            candidate("pwsh", "PowerShell (pwsh)", "pwsh"),
            candidate("powershell", "Windows PowerShell", "powershell"),
            candidate("cmd", "Command Prompt", "cmd"),
            candidate("bash", "Bash", "bash"),
        ]
    }
    #[cfg(unix)]
    {
        // Absolute paths, not bare names: `resolve_program` is an identity
        // function on Unix, so a bare name would never be found. The pairs are
        // the two conventional locations for the same shell under one id.
        let mut candidates = vec![
            candidate("bash", "Bash", "/bin/bash"),
            candidate("zsh", "Zsh", "/bin/zsh"),
            candidate("zsh", "Zsh", "/usr/bin/zsh"),
            candidate("fish", "Fish", "/usr/bin/fish"),
            candidate("fish", "Fish", "/bin/fish"),
            candidate("sh", "sh", "/bin/sh"),
        ];
        // `$SHELL` is appended **last** so the dedupe in `detected_shells`
        // keeps the nicely-labelled known entry when it names one of the above,
        // and it appears in its own right only when it is something else. Its
        // label is its bare basename with no prettifying — inventing a display
        // name for an unknown shell would be a guess.
        //
        // `/etc/shells` is deliberately not read: it lists the login shells the
        // system *permits*, which is not the same fact as what is installed.
        if let Some(shell) = std::env::var_os("SHELL") {
            let shell = shell.to_string_lossy().into_owned();
            if !shell.is_empty() {
                let label = Path::new(&shell)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| shell.clone());
                candidates.push(candidate("login", &label, &shell));
            }
        }
        candidates
    }
}

/// A path in the form two paths can be compared in.
///
/// Case-folded on Windows, where the filesystem is case-insensitive, and left
/// alone on Unix, where it is not. Separators are **not** normalised and the
/// path is **not** canonicalised: every path compared here came out of the same
/// `locate`, so they are already spelled the same way, and canonicalising would
/// touch the disk inside a pure function.
fn path_key(path: &Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

/// Which of `candidates` are actually on this machine, as rows the UI can
/// offer, in candidate order.
///
/// `locate` is injected so every rule below is testable without a shell
/// installed; [`detect_shells`] supplies the real one.
///
/// **A candidate `locate` cannot find is omitted** — never emitted with a
/// broken path, and never emitted as a disabled row. This is the direct
/// opposite of [`pick_shell`] in this same module, which falls back to its
/// *last* candidate even when none are available, and the contrast is
/// deliberate rather than an inconsistency: `pick_shell` must return
/// *something* to spawn so the spawn error can name it, whereas this builds a
/// list the user clicks, and a row that cannot launch is worse than a shorter
/// list. Do not "fix" either one to match the other.
///
/// For the same reason **an empty result is a legitimate answer, not a
/// failure**: `terminal_open` still calls [`default_shell`] when the frontend
/// names no program, so a machine where nothing was detected keeps exactly the
/// behaviour it had before this function existed.
///
/// The rest of the rules:
///
/// - **Order is preserved**, because candidate order is preference order.
/// - **One row per resolved file**, keeping the earliest candidate — so a
///   `$SHELL` pointing at `/bin/bash` does not list bash twice, and the better
///   label wins.
/// - **One row per id**, because the id is what the frontend persists; two rows
///   sharing one would make the stored preference ambiguous.
/// - **A path [`super::check_batch_argv`] refuses is omitted.** A `.cmd`/`.bat`
///   shim under a directory containing `&`, `%`, `^`, `<`, `>`, `|` or `"` is
///   re-read by `cmd.exe` and cannot be spawned as written. The guard at the
///   spawn seam in `PtyManager::open_inner` remains the enforcement; this is
///   only about not advertising a row that would fail the moment it was clicked.
/// - **A `bash` that is really the WSL launcher is omitted** — see
///   [`is_wsl_bash_launcher`].
pub fn detected_shells(
    candidates: &[ShellCandidate],
    locate: impl Fn(&ShellCandidate) -> Option<PathBuf>,
) -> Vec<ShellInfo> {
    let mut found: Vec<ShellInfo> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    for candidate in candidates {
        let Some(resolved) = locate(candidate) else {
            continue;
        };
        if is_wsl_bash_launcher(&resolved) {
            continue;
        }
        if super::check_batch_argv(&resolved, &candidate.args).is_err() {
            continue;
        }
        let key = path_key(&resolved);
        if keys.contains(&key) || found.iter().any(|s| s.id == candidate.id) {
            continue;
        }
        keys.push(key);
        found.push(ShellInfo {
            id: candidate.id.clone(),
            label: candidate.label.clone(),
            // The resolved path, not `candidate.probe`: detection proved *this
            // file* exists, and re-resolving the bare name at spawn time would
            // re-run the PATHEXT walk and could launch a different one.
            program: resolved.display().to_string(),
            args: candidate.args.clone(),
        });
    }
    found
}

/// Whether `resolved` is Windows' legacy WSL `bash.exe` launcher rather than a
/// real bash.
///
/// `System32\bash.exe` (and its `SysWOW64` sibling) is a stub that starts the
/// default WSL distribution, and it fails outright when no distribution is
/// installed — so offering it as a shell offers something that may not start.
///
/// Stated as the heuristic it is: this also omits a genuine bash that somebody
/// copied into `System32`, and omitting is the safe side, because the cost of
/// dropping an unusual install is a shorter list while the cost of keeping the
/// stub is a picker row that opens an error.
pub fn is_wsl_bash_launcher(resolved: &Path) -> bool {
    let named_bash = resolved
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|stem| stem.eq_ignore_ascii_case("bash"));
    if !named_bash {
        return false;
    }
    resolved
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .is_some_and(|dir| {
            dir.eq_ignore_ascii_case("system32") || dir.eq_ignore_ascii_case("syswow64")
        })
}

/// The id of the detected shell that `default_program` names, or `None`.
///
/// `None` when the app's own default resolves to a file that is not in the
/// list — and it must stay `None` rather than falling back to the first entry:
/// naming a row the app would not actually launch tells the user something
/// untrue, and the UI can simply say nothing instead. `default_program` is
/// expected already resolved, so this stays pure and touches no disk.
pub fn default_among(shells: &[ShellInfo], default_program: &Path) -> Option<String> {
    let target = path_key(default_program);
    shells
        .iter()
        .find(|shell| path_key(Path::new(&shell.program)) == target)
        .map(|shell| shell.id.clone())
}

/// Locate one candidate on this machine.
///
/// Windows resolves the bare name through the same PATHEXT walk a spawn uses,
/// and demands a *different* path back — `resolve_program` returns its input
/// unchanged when it finds nothing, so equality means "not found" (the test
/// `review::is_installed` uses, kept identical) — plus that the result is a
/// file. Unix tests the absolute path directly, because `resolve_program` is an
/// identity function there and would prove nothing.
fn locate_on_this_machine(candidate: &ShellCandidate) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let resolved = crate::process::resolve_program(&candidate.probe);
        (resolved != Path::new(&candidate.probe) && resolved.is_file()).then_some(resolved)
    }
    #[cfg(unix)]
    {
        let path = PathBuf::from(&candidate.probe);
        path.is_file().then_some(path)
    }
}

/// What the shell picker is offered: the shells on this machine, and which of
/// them the app launches with no preference set.
///
/// The thin impure wrapper over [`shell_candidates`], [`detected_shells`] and
/// [`default_among`] — every decision is in those, this only touches the disk.
/// `default_id` is resolved through the same [`default_shell`] a terminal
/// actually falls back to, so the answer is a fact about this build rather than
/// a re-derivation of the candidate order.
pub fn detect_shells() -> DetectedShells {
    let candidates = shell_candidates();
    let shells = detected_shells(&candidates, locate_on_this_machine);
    let default_program = crate::process::resolve_program(&default_shell());
    let default_id = default_among(&shells, &default_program);
    DetectedShells { shells, default_id }
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod shell_tests;
