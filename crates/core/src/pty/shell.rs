//! Pure decisions for the PTY layer: which shell to launch, and clamping the
//! terminal dimensions. Extracted so they are testable without spawning
//! anything — the same split `process/resolve.rs` uses for its PATHEXT walk.

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

#[cfg(test)]
#[path = "shell_tests.rs"]
mod shell_tests;
