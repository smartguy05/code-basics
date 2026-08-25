//! Mapping a coverage report onto the current diff — "coverage of change".
//!
//! The question this answers is narrow and useful: of the lines you just
//! *added or changed*, which ones did the test run never execute? A whole-file
//! coverage percentage buries that; this reports only the changed lines, each
//! carrying its [`crate::git::DiffLine::index`] so the UI can highlight the
//! exact row in the diff pane.
//!
//! # Abstain before you guess
//!
//! Two places decide, and both refuse rather than fabricate:
//!
//! * **Path matching.** A coverage path and a diff path are relative to
//!   possibly different roots, so they are matched by whole-segment suffix and
//!   only when that match is **unique**. Zero matches or two matches both become
//!   a warning and the file is skipped — never matched by basename, because two
//!   `Foo.cs` in different directories would cross-report each other's coverage.
//! * **Non-executable lines.** A changed line the coverage tool said nothing
//!   about (absent from [`FileCoverage::lines`]) is neither covered nor
//!   uncovered — it is not counted at all. Only a line the tool actually
//!   emitted, with a hit count, is classified.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::git::{FileDiff, LineOrigin};
use crate::testing::coverage::FileCoverage;

/// One changed line the test run never executed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UncoveredLine {
    /// The 1-based new-side source line number.
    pub line: u32,
    /// The [`crate::git::DiffLine::index`] of that line, the UI's highlight anchor.
    pub index: u32,
}

/// Coverage of the changed lines in one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeCoverage {
    /// The diff path (working-copy path) these numbers describe.
    pub path: String,
    /// The changed lines that were coverable but never executed.
    pub uncovered: Vec<UncoveredLine>,
    /// How many changed lines were executed at least once.
    pub covered_changed: u32,
    /// How many changed lines were coverable but never executed.
    pub uncovered_changed: u32,
}

/// Coverage of every changed line across the diff.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChangeCoverage {
    pub files: Vec<FileChangeCoverage>,
    /// Changed lines that could be classified (covered + uncovered). Lines the
    /// tool considered non-executable are abstained on and excluded here.
    pub changed_lines: u32,
    pub covered_lines: u32,
    pub uncovered_lines: u32,
    /// Files whose coverage could not be matched — no match, or an ambiguous
    /// one — reported rather than silently dropped.
    pub warnings: Vec<String>,
}

/// Normalise a path to forward slashes for comparison.
fn normalise(path: &str) -> String {
    path.replace('\\', "/")
}

/// Whether `coverage` ends with `diff` on a whole-segment boundary.
///
/// Either the two are equal, or `coverage` ends with `"/" + diff` — so
/// `a/b/src/Foo.cs` matches `src/Foo.cs` but `x/BarFoo.cs` does not match
/// `Foo.cs`.
fn suffix_matches(coverage: &str, diff: &str) -> bool {
    if coverage == diff {
        return true;
    }
    coverage.len() > diff.len()
        && coverage.ends_with(diff)
        && coverage.as_bytes()[coverage.len() - diff.len() - 1] == b'/'
}

/// Map coverage onto the diff, reporting the uncovered changed lines.
pub fn map_change_coverage(diffs: &[FileDiff], coverage: &[FileCoverage]) -> ChangeCoverage {
    let normalised: Vec<(String, &FileCoverage)> =
        coverage.iter().map(|c| (normalise(&c.path), c)).collect();

    let mut out = ChangeCoverage::default();

    for diff in diffs {
        if diff.is_binary {
            continue;
        }
        let diff_path = normalise(&diff.path);

        // Find the unique coverage file whose path ends with this diff path.
        let mut matches = normalised
            .iter()
            .filter(|(cov_path, _)| suffix_matches(cov_path, &diff_path));
        let first = matches.next();
        let second = matches.next();

        let cov = match (first, second) {
            (None, _) => {
                out.warnings
                    .push(format!("no coverage matched {}", diff.path));
                continue;
            }
            (Some(_), Some(_)) => {
                out.warnings
                    .push(format!("ambiguous coverage for {}", diff.path));
                continue;
            }
            (Some((_, cov)), None) => *cov,
        };

        let mut file = FileChangeCoverage {
            path: diff.path.clone(),
            uncovered: Vec::new(),
            covered_changed: 0,
            uncovered_changed: 0,
        };

        for hunk in &diff.hunks {
            for dl in &hunk.lines {
                if dl.origin != LineOrigin::Addition {
                    continue;
                }
                let Some(lineno) = dl.new_lineno else {
                    continue;
                };
                // Absent from the map: non-executable / unknown — abstain.
                let Some(hits) = cov.lines.get(&lineno) else {
                    continue;
                };
                if *hits > 0 {
                    file.covered_changed += 1;
                } else {
                    file.uncovered_changed += 1;
                    file.uncovered.push(UncoveredLine {
                        line: lineno,
                        index: dl.index,
                    });
                }
            }
        }

        // A file with no classifiable changed lines contributes nothing and is
        // not listed — its presence would imply coverage information it has none of.
        if file.covered_changed + file.uncovered_changed == 0 {
            continue;
        }

        out.covered_lines += file.covered_changed;
        out.uncovered_lines += file.uncovered_changed;
        out.files.push(file);
    }

    out.changed_lines = out.covered_lines + out.uncovered_lines;
    out
}

#[cfg(test)]
#[path = "changecov_tests.rs"]
mod changecov_tests;
