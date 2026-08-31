//! Refusing arguments a Windows batch shim would re-interpret.
//!
//! The PTY spawn is the one path in the app that hands **free user prose** to a
//! program as `argv` — "Ask the codebase" passes the typed question as one
//! argument. That is safe for a real executable and unsafe for a batch shim,
//! and the asymmetry is the whole content of this module:
//!
//! - `resolve_program` walks `PATHEXT`, so a bare agent name can resolve to a
//!   `.cmd`/`.bat` shim (on this machine `claude` is a `.exe` but `codex` is
//!   `codex.cmd`, installed by npm).
//! - `portable_pty`'s `CommandBuilder` implements **MSVC argv quoting only**.
//!   It leaves an argument unquoted unless it contains a space, tab, newline,
//!   vertical tab or double-quote, and it has none of the batch-file mitigation
//!   `std::process::Command` gained for CVE-2024-24576.
//! - When the target is a batch file, `cmd.exe` then re-parses that command
//!   line. `&`, `|`, `<`, `>` separate commands; `^` escapes; `%VAR%` is
//!   expanded even inside quotes; and `cmd.exe` does not honour `\"`, so an odd
//!   number of quotes ends the quoting early.
//!
//! So the same argument means one thing through a `.exe` and another through a
//! `.cmd`. Rather than mangle the question or disable the feature, this refuses
//! **precisely** what would be misread, naming the character and the fix. That
//! covers the whole command line, not only the arguments: `CommandBuilder`
//! quotes the resolved **program path** with the very same routine
//! (`src/cmdbuilder.rs:679`), so a shim under a directory named `dev&test` is
//! split at the `&` and `cmd.exe` runs a second command of its own — checked by
//! [`batch_program_refusal`], and reported as its own cause because its fix is
//! to move the program rather than to rephrase a question — the same posture as [`crate::launcher::parse::split_command`],
//! which refuses a shell metacharacter rather than running a bare argv that
//! silently means something else. Ordinary prose, which is nearly every
//! question, is untouched.
//!
//! For a non-batch target the guard does **not** apply, on purpose: MSVC argv
//! quoting is correct for a real executable, nothing re-parses the line, and
//! refusing a valid question there would be a regression rather than a fix.

use std::path::Path;

/// Whether spawning `resolved` means `cmd.exe` will re-parse the command line —
/// i.e. whether it is a `.cmd` or `.bat` file. Case-insensitive, because the
/// filesystem is.
///
/// Judged on the **resolved** path, not the name the caller typed: `codex` is
/// safe-looking and resolves to `codex.cmd`.
pub fn is_batch_target(resolved: &Path) -> bool {
    matches!(
        resolved
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("cmd") | Some("bat")
    )
}

/// Why `cmd.exe` would misread `c`, or `None` if it would not.
fn hazard(c: char) -> Option<&'static str> {
    match c {
        '&' | '|' => Some("cmd.exe reads it as a command separator"),
        '<' | '>' => Some("cmd.exe reads it as a redirection"),
        '^' => Some("cmd.exe reads it as an escape character"),
        '"' => Some("cmd.exe does not honour `\\\"`, so a quote can end the quoting early"),
        '%' => Some("cmd.exe expands it as an environment variable, even inside quotes"),
        _ => None,
    }
}

/// Whether `arg` can be passed unchanged to a batch target, and if not, why.
///
/// `Some(reason)` names the offending character and the fix; `None` means the
/// argument survives `cmd.exe`'s re-parse intact. Only ever consulted for a
/// batch target — see [`is_batch_target`] and [`check_batch_argv`].
pub fn batch_argv_refusal(arg: &str) -> Option<String> {
    for c in arg.chars() {
        if let Some(why) = hazard(c) {
            return Some(format!(
                "`{c}` cannot be passed to a `.cmd`/`.bat` program: {why}, so the argument would not reach it as written — remove the character, or rephrase without it"
            ));
        }
        if c.is_control() {
            return Some(format!(
                "a control character (U+{:04X}) cannot be passed to a `.cmd`/`.bat` program: cmd.exe re-parses the command line and would not pass it through as written — remove it, or rephrase without it",
                c as u32
            ));
        }
    }
    None
}

/// Whether the resolved **program path** can be spawned as a batch target, and
/// if not, why.
///
/// Separate from [`batch_argv_refusal`] because the fault and the fix are
/// different facts: an argument is the user's own prose and can be rephrased,
/// while a path is where the program is installed and can only be moved. The
/// hazard set is identical — `CommandBuilder` applies the same MSVC quoting to
/// the exe path as to every argument (`portable-pty-0.9.0`,
/// `src/cmdbuilder.rs:679`), so the path sits on the same line `cmd.exe`
/// re-parses, and a directory named `dev&test` splits the command before the
/// program is ever reached.
pub fn batch_program_refusal(program: &str) -> Option<String> {
    for c in program.chars() {
        if let Some(why) = hazard(c) {
            return Some(format!(
                "the program path contains `{c}`: {why}, so cmd.exe would re-read the command line before the `.cmd`/`.bat` program is reached — move or reinstall it somewhere without that character in the path"
            ));
        }
        if c.is_control() {
            return Some(format!(
                "the program path contains a control character (U+{:04X}): cmd.exe re-parses the command line and would not reach the `.cmd`/`.bat` program as written — move or reinstall it somewhere without that character in the path",
                c as u32
            ));
        }
    }
    None
}

/// The whole guard, at the spawn seam: `Ok(())` unless `resolved` is a batch
/// target whose own path, or one of whose arguments, `cmd.exe` would misread.
///
/// The program path is checked **first and separately**: it is on the same
/// command line, and a hazard there is a different fault with a different fix
/// than a hazard in a question the user typed, so the two never share a message.
///
/// Returns the refusal **before** anything is spawned, so the caller reports it
/// rather than discovering it from a mangled run.
pub fn check_batch_argv(resolved: &Path, args: &[String]) -> Result<(), String> {
    if !is_batch_target(resolved) {
        return Ok(());
    }
    let program = resolved.display().to_string();
    if let Some(reason) = batch_program_refusal(&program) {
        return Err(format!("cannot start `{program}`: {reason}"));
    }
    for arg in args {
        if let Some(reason) = batch_argv_refusal(arg) {
            return Err(format!("cannot start `{}`: {reason}", resolved.display()));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "argv_tests.rs"]
mod tests;
