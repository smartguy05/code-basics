//! Test report parsing and result shaping.
//!
//! Every supported runner streams human-readable output live *and* writes a
//! structured report file when it finishes. That single observation is what
//! makes the adapter layer cheap: the console shows raw output as it arrives,
//! and the tree is built from the report afterwards, so adding an ecosystem
//! only means knowing which command to run and which of these formats it
//! leaves behind.

pub mod changecov;
pub mod coverage;
pub mod jest_like;
pub mod junit;
pub mod tree;
pub mod trx;

use std::path::Path;

use anyhow::{Context, Result};

use crate::model::{ReportFormat, TestRunResult};

/// Parse report *content* in the given format.
pub fn parse(format: ReportFormat, content: &str) -> Result<TestRunResult> {
    match format {
        ReportFormat::Trx => trx::parse(content),
        ReportFormat::JestLike => jest_like::parse(content),
        ReportFormat::JunitXml => junit::parse(content),
    }
}

/// Read and parse a report from disk.
///
/// A missing file is reported as its own error: it almost always means the
/// runner never produced one — the classic symptom of pointing VSTest flags at
/// Microsoft.Testing.Platform, which ignores them silently and exits cleanly.
pub fn parse_file(format: ReportFormat, path: &Path) -> Result<TestRunResult> {
    if !path.exists() {
        anyhow::bail!(
            "the test runner did not produce a report at {}. \
             The run may have failed before any test executed, or the runner \
             may not have understood the reporting arguments it was given.",
            path.display()
        );
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read test report {}", path.display()))?;
    parse(format, &content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TestOutcome;

    #[test]
    fn dispatches_to_the_right_parser() {
        let trx = include_str!("../../fixtures/reports/sample.trx");
        assert_eq!(parse(ReportFormat::Trx, trx).unwrap().summary.total, 5);

        let vitest = include_str!("../../fixtures/reports/vitest.json");
        assert_eq!(
            parse(ReportFormat::JestLike, vitest).unwrap().summary.total,
            4
        );

        let junit = include_str!("../../fixtures/reports/junit.xml");
        assert_eq!(
            parse(ReportFormat::JunitXml, junit).unwrap().summary.total,
            4
        );
    }

    #[test]
    fn every_parser_agrees_on_how_a_failure_looks() {
        // Different formats, same contract: a failing case carries a message
        // the UI can show. This is what lets one failure pane serve all of them.
        let inputs = [
            (
                ReportFormat::Trx,
                include_str!("../../fixtures/reports/sample.trx"),
            ),
            (
                ReportFormat::JestLike,
                include_str!("../../fixtures/reports/vitest.json"),
            ),
            (
                ReportFormat::JunitXml,
                include_str!("../../fixtures/reports/junit.xml"),
            ),
        ];

        for (format, content) in inputs {
            let run = parse(format, content).unwrap();
            let failed: Vec<_> = run
                .cases
                .iter()
                .filter(|c| c.outcome == TestOutcome::Failed)
                .collect();

            assert_eq!(failed.len(), 1, "{format:?} should report one failure");
            assert!(
                failed[0].message.as_ref().is_some_and(|m| !m.is_empty()),
                "{format:?} failure should carry a message"
            );
            assert!(
                !failed[0].full_name.is_empty(),
                "{format:?} needs a name to re-run by"
            );
        }
    }

    #[test]
    fn a_missing_report_explains_the_likely_cause() {
        let err = parse_file(ReportFormat::Trx, Path::new("/nonexistent/report.trx"))
            .expect_err("missing report should error");
        let msg = err.to_string();

        assert!(msg.contains("did not produce a report"));
        // The MTP/VSTest mismatch is the failure this most often signals.
        assert!(msg.contains("reporting arguments"));
    }
}
