//! Running the rules over a diff.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::erosion::rules::{compile, CompiledRule, ErosionCategory, ErosionRule, RuleSide};
use crate::git::patch::{DiffLine, FileDiff, LineOrigin};

/// One located weakening the scan found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ErosionFlag {
    pub path: String,
    /// The source line number — `newLineno` for an added line, `oldLineno` for
    /// a removed one — for display as `path:line`.
    pub line: u32,
    /// The `DiffLine::index` of the offending line, so the panel can highlight
    /// it in the diff pane the same way an intent card does.
    pub index: u32,
    pub origin: LineOrigin,
    pub category: ErosionCategory,
    pub rule_id: String,
    pub message: String,
    /// The offending line, trimmed, for display.
    pub content: String,
}

/// Everything the scan found, plus any rule that could not run.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ErosionReport {
    pub flags: Vec<ErosionFlag>,
    /// Rules whose regex would not compile — surfaced, never silently dropped.
    pub warnings: Vec<String>,
}

/// Scan a working tree's diff for erosion.
pub fn scan_diffs(files: &[FileDiff], rules: &[ErosionRule]) -> ErosionReport {
    let (compiled, warnings) = compile(rules);
    let mut flags = Vec::new();

    for file in files {
        if file.is_binary {
            continue;
        }

        let applicable: Vec<&CompiledRule> = compiled
            .iter()
            .filter(|c| rule_applies_to(&c.rule, &file.path))
            .collect();

        // The single-line regex pass. Skipped only for the hunk loop when no
        // rule applies to this file; the paired downgrade pass below still runs.
        if !applicable.is_empty() {
            run_regex_pass(file, &applicable, &mut flags);
        }

        // The paired log-downgrade pass is built in and independent of `rules`,
        // so it runs for every source file regardless of the rule set.
        flags.append(&mut detect_log_downgrades(file));
    }

    ErosionReport { flags, warnings }
}

/// The single-line regex pass: every applicable rule against each changed line,
/// at most one flag per category per line.
fn run_regex_pass(file: &FileDiff, applicable: &[&CompiledRule], flags: &mut Vec<ErosionFlag>) {
    for hunk in &file.hunks {
        for line in &hunk.lines {
            // Only changed lines, and only the side each rule names.
            let side = match line.origin {
                LineOrigin::Addition => RuleSide::Added,
                LineOrigin::Deletion => RuleSide::Removed,
                LineOrigin::Context => continue,
            };

            // A line number is required to cite the flag; without one we
            // abstain rather than guess.
            let line_no = match line.origin {
                LineOrigin::Addition => line.new_lineno,
                LineOrigin::Deletion => line.old_lineno,
                LineOrigin::Context => None,
            };
            let Some(line_no) = line_no else {
                continue;
            };

            // At most one flag per category per line, so two rules of the
            // same category do not double-report.
            let mut seen: BTreeSet<ErosionCategory> = BTreeSet::new();

            for c in applicable {
                if c.rule.side != side {
                    continue;
                }
                if seen.contains(&c.rule.category) {
                    continue;
                }
                if !c.re.is_match(&line.content) {
                    continue;
                }
                seen.insert(c.rule.category);
                flags.push(ErosionFlag {
                    path: file.path.clone(),
                    line: line_no,
                    index: line.index,
                    origin: line.origin,
                    category: c.rule.category,
                    rule_id: c.rule.id.clone(),
                    message: c.rule.message.clone(),
                    content: line.content.trim().to_string(),
                });
            }
        }
    }
}

/// The severity of a log statement paired with a key identifying the statement,
/// so a removed and an added line can be matched as the same log call.
///
/// `rank` is 3 for error/critical, 2 for warning, 1 for info/debug/trace.
/// A line with no recognised log call yields `None`.
fn log_signal(content: &str, kind: LogEcosystem) -> Option<(u8, String)> {
    let rank = log_rank(content, kind)?;
    Some((rank, message_key(content, kind)))
}

/// Which ecosystem's log vocabulary applies to a file, or `None` for a file
/// this detector does not understand.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LogEcosystem {
    Cs,
    TsJs,
    Rs,
}

/// Classify a path by extension, reusing the same suffix logic as
/// [`rule_applies_to`].
fn log_ecosystem(path: &str) -> Option<LogEcosystem> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".cs") {
        Some(LogEcosystem::Cs)
    } else if [".ts", ".tsx", ".js", ".jsx"]
        .iter()
        .any(|e| lower.ends_with(e))
    {
        Some(LogEcosystem::TsJs)
    } else if lower.ends_with(".rs") {
        Some(LogEcosystem::Rs)
    } else {
        None
    }
}

/// The severity rank of a log call on this line, or `None` when the line holds
/// no recognised log call. Higher is more severe. Checked most-severe first so
/// a line is scored by its strongest token.
fn log_rank(content: &str, kind: LogEcosystem) -> Option<u8> {
    let has = |needles: &[&str]| needles.iter().any(|n| content.contains(n));
    match kind {
        LogEcosystem::Cs => {
            if has(&["LogError", "LogCritical", "Log.Error", "Log.Critical"]) {
                Some(3)
            } else if has(&["LogWarning", "Log.Warn"]) {
                Some(2)
            } else if has(&[
                "LogInformation",
                "LogDebug",
                "LogTrace",
                "Log.Info",
                "Log.Debug",
            ]) {
                Some(1)
            } else {
                None
            }
        }
        LogEcosystem::TsJs => {
            if has(&[".error(", ".fatal("]) {
                Some(3)
            } else if has(&[".warn("]) {
                Some(2)
            } else if has(&[".log(", ".info(", ".debug(", ".trace("]) {
                Some(1)
            } else {
                None
            }
        }
        LogEcosystem::Rs => {
            if has(&["error!"]) {
                Some(3)
            } else if has(&["warn!"]) {
                Some(2)
            } else if has(&["info!", "debug!", "trace!"]) {
                Some(1)
            } else {
                None
            }
        }
    }
}

/// A key identifying the log statement, so the same call at two severities can
/// be matched. The first string-literal argument is the strongest signal; when
/// there is none, fall back to the line with severity tokens and logger
/// prefixes stripped.
fn message_key(content: &str, kind: LogEcosystem) -> String {
    if let Some(literal) = first_string_literal(content) {
        return literal;
    }
    let mut stripped = content.trim().to_string();
    let tokens: &[&str] = match kind {
        LogEcosystem::Cs => &[
            "LogError",
            "LogCritical",
            "LogWarning",
            "LogInformation",
            "LogDebug",
            "LogTrace",
            "Log.Error",
            "Log.Critical",
            "Log.Warn",
            "Log.Info",
            "Log.Debug",
            "_logger.",
            "logger.",
            "ILogger",
        ],
        LogEcosystem::TsJs => &[
            "console.", "logger.", ".error", ".fatal", ".warn", ".info", ".debug", ".trace", ".log",
        ],
        LogEcosystem::Rs => &[
            "tracing::",
            "log::",
            "error!",
            "warn!",
            "info!",
            "debug!",
            "trace!",
        ],
    };
    for token in tokens {
        stripped = stripped.replace(token, "");
    }
    stripped.trim().to_string()
}

/// The first single/double/backtick-quoted string literal on a line, without
/// its quotes. Escapes are not interpreted — this is a matching key, not a
/// parser. `None` when the line holds no literal.
fn first_string_literal(content: &str) -> Option<String> {
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' || c == b'\'' || c == b'`' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() {
                if bytes[j] == c {
                    return Some(content[start..j].to_string());
                }
                j += 1;
            }
            return None;
        }
        i += 1;
    }
    None
}

/// The paired log-downgrade pass: within one hunk, match a removed log line to
/// an added log line by exact message key and emit a flag on the added line
/// when its severity is strictly lower. Ambiguity — a key carried by more than
/// one removed or more than one added line — abstains rather than guesses.
fn detect_log_downgrades(file: &FileDiff) -> Vec<ErosionFlag> {
    let Some(kind) = log_ecosystem(&file.path) else {
        return Vec::new();
    };

    let mut flags = Vec::new();

    for hunk in &file.hunks {
        let mut removed: Vec<(u8, String, &DiffLine)> = Vec::new();
        let mut added: Vec<(u8, String, &DiffLine)> = Vec::new();

        for line in &hunk.lines {
            match line.origin {
                LineOrigin::Deletion => {
                    if let Some((rank, key)) = log_signal(&line.content, kind) {
                        removed.push((rank, key, line));
                    }
                }
                LineOrigin::Addition => {
                    if let Some((rank, key)) = log_signal(&line.content, kind) {
                        added.push((rank, key, line));
                    }
                }
                LineOrigin::Context => {}
            }
        }

        for (added_rank, key, line) in &added {
            // The pair must be unique on both sides, or we cannot know which
            // removed line became which added line.
            let removed_matches: Vec<&(u8, String, &DiffLine)> =
                removed.iter().filter(|(_, k, _)| k == key).collect();
            let added_matches = added.iter().filter(|(_, k, _)| k == key).count();
            if removed_matches.len() != 1 || added_matches != 1 {
                continue;
            }

            let removed_rank = removed_matches[0].0;
            if *added_rank >= removed_rank {
                continue; // Same severity or an upgrade is not a downgrade.
            }

            // The added line must carry a line number to be cited.
            let Some(line_no) = line.new_lineno else {
                continue;
            };

            flags.push(ErosionFlag {
                path: file.path.clone(),
                line: line_no,
                index: line.index,
                origin: LineOrigin::Addition,
                category: ErosionCategory::LogDowngrade,
                rule_id: "log-downgrade".to_string(),
                message: "A log statement's severity was lowered.".to_string(),
                content: line.content.trim().to_string(),
            });
        }
    }

    flags
}

/// Does a rule apply to this file? Empty `extensions` means every file; a test
/// path is skipped for `prod_only` rules.
fn rule_applies_to(rule: &ErosionRule, path: &str) -> bool {
    if rule.prod_only && is_test_path(path) {
        return false;
    }
    if rule.extensions.is_empty() {
        return true;
    }
    let lower = path.to_ascii_lowercase();
    rule.extensions
        .iter()
        .any(|ext| lower.ends_with(&ext.to_ascii_lowercase()))
}

/// A path that looks like it holds tests rather than production code.
fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "/test", "test/", "tests/", "/spec/", ".test.", ".spec.", "_test.", "_tests.", "_spec.",
    ];
    MARKERS.iter().any(|m| lower.contains(m))
}

#[cfg(test)]
#[path = "scan_tests.rs"]
mod tests;
