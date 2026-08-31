//! Turning a typed command line into a program and argv — or refusing to.
//!
//! Pure, so every way a command line can be misread is provable without
//! spawning anything. The abstain rule matters more here than it looks: this is
//! the one place in the app where the user hands over free text that becomes a
//! process, and every plausible-but-wrong reading is silent. So an empty line,
//! an unbalanced quote, and a line whose meaning depends on a shell are all
//! **errors that name the problem**, never a best guess at what was meant.

/// Characters that only mean something to a shell. A bare argv spawn would pass
/// them to the program as ordinary arguments — `echo hi | findstr hi` would run
/// `echo` with four arguments and print `hi | findstr hi` — which is exactly the
/// kind of quiet wrongness [`split_command`] refuses.
pub const SHELL_SPECIALS: &[char] = &['|', '>', '<', '&', ';'];

/// A tokenised command line, plus any shell metacharacters seen outside quotes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    /// The argv, quotes removed.
    pub tokens: Vec<String>,
    /// Shell metacharacters found outside quotes, in the order encountered and
    /// deduplicated. Empty for a line that means the same with or without a
    /// shell.
    pub specials: Vec<char>,
}

/// Split a command line into whitespace-separated tokens, honouring double
/// quotes.
///
/// Only `"` groups, and only `\"` escapes — deliberately **not** a general
/// backslash escape, because on Windows every other path the user types
/// (`C:\repo\src`) is full of backslashes that must arrive unchanged. An
/// unbalanced quote is an error: the two readings (an implied closing quote, or
/// the quote as literal text) give different argv, and picking one silently is
/// how a launcher runs the wrong thing.
pub fn tokenise(line: &str) -> Result<Tokens, String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut specials: Vec<char> = Vec::new();
    let mut current = String::new();
    let mut has_current = false;
    let mut in_quotes = false;

    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'"') => {
                chars.next();
                current.push('"');
                has_current = true;
            }
            '"' => {
                in_quotes = !in_quotes;
                // An empty pair of quotes is still an argument.
                has_current = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_current {
                    tokens.push(std::mem::take(&mut current));
                    has_current = false;
                }
            }
            c => {
                if !in_quotes && SHELL_SPECIALS.contains(&c) && !specials.contains(&c) {
                    specials.push(c);
                }
                current.push(c);
                has_current = true;
            }
        }
    }

    if in_quotes {
        return Err(format!(
            "unbalanced quote in `{}` — close the quote, or remove it",
            line.trim()
        ));
    }
    if has_current {
        tokens.push(current);
    }

    Ok(Tokens { tokens, specials })
}

/// The program and its arguments for a line to be spawned directly (no shell).
///
/// Refuses an empty line and any line carrying an unquoted shell metacharacter,
/// naming the character and the fix — running it as a bare argv would "work"
/// while doing something else entirely.
pub fn split_command(line: &str) -> Result<(String, Vec<String>), String> {
    let parsed = tokenise(line)?;
    if !parsed.specials.is_empty() {
        let listed: Vec<String> = parsed
            .specials
            .iter()
            .map(|c| format!("`{c}`"))
            .collect::<Vec<_>>();
        return Err(format!(
            "{} only means something to a shell — tick “run through shell” to run this command line, or quote the character to pass it as an argument",
            listed.join(", ")
        ));
    }
    let mut tokens = parsed.tokens.into_iter();
    let program = tokens
        .next()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| "a command is required".to_string())?;
    Ok((program, tokens.collect()))
}

/// The platform's "run this command line" shell flag: `/C` for the Windows
/// shells (`pwsh`, `powershell` and `cmd` all accept it), `-c` elsewhere.
pub fn shell_flag() -> &'static str {
    if cfg!(windows) {
        "/C"
    } else {
        "-c"
    }
}

/// The arguments handing `line` to the default shell verbatim — the shell, not
/// this module, then decides what the metacharacters mean.
pub fn shell_args(line: &str) -> Vec<String> {
    vec![shell_flag().to_string(), line.trim().to_string()]
}

/// Resolve a command line to a program and argv, through the shell or not.
pub fn program_and_args(line: &str, shell: bool) -> Result<(String, Vec<String>), String> {
    if line.trim().is_empty() {
        return Err("a command is required".into());
    }
    if shell {
        return Ok((crate::pty::default_shell(), shell_args(line)));
    }
    split_command(line)
}

#[cfg(test)]
#[path = "parse_tests.rs"]
mod tests;
