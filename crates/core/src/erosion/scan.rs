//! Running the rules over a diff.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::erosion::rules::{compile, CompiledRule, ErosionCategory, ErosionRule, RuleSide};
use crate::git::patch::{FileDiff, LineOrigin};

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
        if applicable.is_empty() {
            continue;
        }

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

                for c in &applicable {
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

    ErosionReport { flags, warnings }
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
