//! Tests for mapping coverage onto the diff.

use super::*;
use crate::git::{DiffLine, FileDiff, Hunk, LineOrigin};
use crate::testing::coverage::FileCoverage;
use std::collections::BTreeMap;

/// An added new-side line at `new_lineno`, carrying `index` as its diff anchor.
fn added(index: u32, new_lineno: u32, content: &str) -> DiffLine {
    DiffLine {
        index,
        origin: LineOrigin::Addition,
        content: content.into(),
        old_lineno: None,
        new_lineno: Some(new_lineno),
        no_newline: false,
    }
}

fn context(index: u32, old_lineno: u32, new_lineno: u32, content: &str) -> DiffLine {
    DiffLine {
        index,
        origin: LineOrigin::Context,
        content: content.into(),
        old_lineno: Some(old_lineno),
        new_lineno: Some(new_lineno),
        no_newline: false,
    }
}

fn diff(path: &str, lines: Vec<DiffLine>) -> FileDiff {
    FileDiff {
        path: path.into(),
        old_path: None,
        hunks: vec![Hunk {
            old_start: 1,
            old_lines: 0,
            new_start: 1,
            new_lines: lines.len() as u32,
            header: String::new(),
            lines,
        }],
        is_binary: false,
    }
}

fn cov(path: &str, entries: &[(u32, u32)]) -> FileCoverage {
    FileCoverage {
        path: path.into(),
        lines: entries.iter().copied().collect::<BTreeMap<u32, u32>>(),
    }
}

#[test]
fn classifies_changed_lines_as_covered_or_uncovered() {
    let diffs = vec![diff(
        "src/Api/Foo.cs",
        vec![
            context(0, 9, 9, "// header"),
            added(1, 10, "var x = compute();"),
            added(2, 11, "return x;"),
        ],
    )];
    // The coverage report is rooted differently (an absolute-ish prefix); it
    // still matches by whole-segment suffix. Line 10 covered, line 11 not.
    let coverage = vec![cov(
        "/build/repo/src/Api/Foo.cs",
        &[(9, 1), (10, 4), (11, 0)],
    )];

    let result = map_change_coverage(&diffs, &coverage);

    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    assert_eq!(result.changed_lines, 2);
    assert_eq!(result.covered_lines, 1);
    assert_eq!(result.uncovered_lines, 1);

    assert_eq!(result.files.len(), 1);
    let file = &result.files[0];
    assert_eq!(file.covered_changed, 1);
    assert_eq!(file.uncovered_changed, 1);
    assert_eq!(file.uncovered.len(), 1);
    // The uncovered line carries its source line AND its diff index for the UI.
    assert_eq!(file.uncovered[0].line, 11);
    assert_eq!(file.uncovered[0].index, 2);
}

#[test]
fn abstains_on_a_changed_line_the_report_never_mentioned() {
    // Line 11 is absent from the coverage map — non-executable / unknown. It is
    // neither covered nor uncovered, so it is counted in neither tally and no
    // uncovered line is fabricated for it.
    let diffs = vec![diff(
        "src/util/math.ts",
        vec![
            added(0, 10, "const y = 1;"),
            added(1, 11, "// a blank comment the tool ignores"),
        ],
    )];
    let coverage = vec![cov("src/util/math.ts", &[(10, 2)])];

    let result = map_change_coverage(&diffs, &coverage);

    assert!(result.warnings.is_empty());
    assert_eq!(result.changed_lines, 1, "only line 10 is classifiable");
    assert_eq!(result.covered_lines, 1);
    assert_eq!(result.uncovered_lines, 0);
    assert_eq!(result.files[0].uncovered.len(), 0);
}

#[test]
fn an_unmatched_file_warns_and_fabricates_nothing() {
    let diffs = vec![diff("src/New.cs", vec![added(0, 1, "int a = 0;")])];
    // No coverage file ends with this path.
    let coverage = vec![cov("src/Other.cs", &[(1, 0)])];

    let result = map_change_coverage(&diffs, &coverage);

    assert!(result.files.is_empty(), "no file should be reported");
    assert_eq!(result.uncovered_lines, 0);
    assert_eq!(result.changed_lines, 0);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("no coverage matched"));
    assert!(result.warnings[0].contains("src/New.cs"));
}

#[test]
fn an_ambiguous_match_warns_rather_than_guessing() {
    // Two coverage files both end with `Foo.cs`; matching by basename would
    // cross-report, so the file is skipped with a warning instead.
    let diffs = vec![diff("Foo.cs", vec![added(0, 1, "x")])];
    let coverage = vec![cov("a/Foo.cs", &[(1, 1)]), cov("b/Foo.cs", &[(1, 0)])];

    let result = map_change_coverage(&diffs, &coverage);

    assert!(result.files.is_empty());
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("ambiguous coverage"));
}
