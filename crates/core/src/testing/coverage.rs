//! Parsing code-coverage reports into a per-file line→hit map.
//!
//! This is the raw-report layer of the "coverage of change" feature: it turns a
//! Cobertura XML document (what coverlet's `XPlat Code Coverage` collector emits
//! for .NET) or an LCOV text file (what Vitest's `lcov` reporter emits for
//! JS/TS) into a list of [`FileCoverage`]. The mapping onto the current diff —
//! the part that must abstain rather than guess — lives in
//! [`crate::testing::changecov`].
//!
//! # Only coverable lines appear
//!
//! A [`FileCoverage`] records **only the lines the tool considered executable**.
//! A blank line, a brace, a comment — anything the coverage tool did not emit a
//! `<line>` / `DA:` entry for — is simply absent from the map. That absence is
//! load-bearing: it is what lets the mapper abstain on a changed line the tool
//! said nothing about, rather than misreport a non-executable line as
//! "uncovered".

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::model::{CoverageFormat, CoverageSpec};

/// The coverable lines of one source file and how many times each was hit.
///
/// `lines` maps a 1-based source line number to its hit count. A line present
/// with `0` was coverable but never executed; a line **absent** from the map
/// was not considered executable and must not be classified either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCoverage {
    /// The path the coverage tool recorded, verbatim. Matching it onto a diff
    /// path (which may be relative to a different root) is the mapper's job.
    pub path: String,
    /// Source line number → hit count, for coverable lines only.
    pub lines: BTreeMap<u32, u32>,
}

/// Parse a Cobertura XML document.
///
/// Reads every `<class filename="...">` and the `<line number=".." hits=".."/>`
/// entries nested under its `<lines>`. Multiple `<class>` elements can name the
/// same `filename` (partial classes, or a file compiled into more than one
/// target framework); their line maps are **merged**, a line's hit counts
/// summed, so a line covered by any one of them counts as covered.
pub fn parse_cobertura(xml: &str) -> Result<Vec<FileCoverage>> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    // Preserve first-seen order of filenames while merging duplicates.
    let mut order: Vec<String> = Vec::new();
    let mut by_file: BTreeMap<String, BTreeMap<u32, u32>> = BTreeMap::new();
    let mut current_file: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_name(&e);
                if name.eq_ignore_ascii_case("class") {
                    if let Some(file) = attr(&e, "filename") {
                        if !order.iter().any(|f| f == &file) {
                            order.push(file.clone());
                        }
                        by_file.entry(file.clone()).or_default();
                        current_file = Some(file);
                    }
                } else if name.eq_ignore_ascii_case("line") {
                    record_line(&mut by_file, current_file.as_deref(), &e);
                }
            }
            Ok(Event::Empty(e)) => {
                let name = local_name(&e);
                if name.eq_ignore_ascii_case("line") {
                    record_line(&mut by_file, current_file.as_deref(), &e);
                } else if name.eq_ignore_ascii_case("class") {
                    // A class with no lines still names a file.
                    if let Some(file) = attr(&e, "filename") {
                        if !order.iter().any(|f| f == &file) {
                            order.push(file.clone());
                        }
                        by_file.entry(file).or_default();
                    }
                }
            }
            Ok(Event::End(e)) => {
                if local_name_end(&e).eq_ignore_ascii_case("class") {
                    current_file = None;
                }
            }
            Ok(Event::Eof) => break,
            // A malformed document yields what was read so far rather than an
            // error: partial coverage is still useful, and the mapper abstains
            // on anything it cannot place.
            Err(_) => break,
            _ => {}
        }
    }

    Ok(order
        .into_iter()
        .map(|path| {
            let lines = by_file.remove(&path).unwrap_or_default();
            FileCoverage { path, lines }
        })
        .collect())
}

/// Parse an LCOV text file.
///
/// `SF:<path>` opens a file record, `DA:<line>,<hits>` adds a coverable line,
/// and `end_of_record` closes the current file. Everything else (branch and
/// function records, summary lines) is ignored — only line coverage is needed
/// to classify changed lines.
pub fn parse_lcov(text: &str) -> Result<Vec<FileCoverage>> {
    let mut out: Vec<FileCoverage> = Vec::new();
    let mut current: Option<FileCoverage> = None;

    for raw in text.lines() {
        let line = raw.trim();
        if let Some(path) = line.strip_prefix("SF:") {
            // A new SF without an end_of_record still starts a new file; keep
            // the previous one rather than dropping it.
            if let Some(prev) = current.take() {
                out.push(prev);
            }
            current = Some(FileCoverage {
                path: path.trim().to_string(),
                lines: BTreeMap::new(),
            });
        } else if let Some(rest) = line.strip_prefix("DA:") {
            if let Some(file) = current.as_mut() {
                let mut parts = rest.splitn(2, ',');
                if let (Some(number), Some(hits)) = (parts.next(), parts.next()) {
                    if let (Ok(number), Some(hits)) =
                        (number.trim().parse::<u32>(), parse_hits(hits.trim()))
                    {
                        // A line can be reported more than once; keep the
                        // highest hit count so covered wins over uncovered.
                        let entry = file.lines.entry(number).or_insert(0);
                        *entry = (*entry).max(hits);
                    }
                }
            }
        } else if line.eq_ignore_ascii_case("end_of_record") {
            if let Some(file) = current.take() {
                out.push(file);
            }
        }
    }

    if let Some(file) = current.take() {
        out.push(file);
    }

    Ok(out)
}

/// Record a `<line number=".." hits=".."/>` under the current file, if any.
fn record_line(
    by_file: &mut BTreeMap<String, BTreeMap<u32, u32>>,
    current: Option<&str>,
    e: &quick_xml::events::BytesStart,
) {
    let Some(file) = current else { return };
    if let (Some(number), Some(hits)) = (
        attr(e, "number").and_then(|v| v.parse::<u32>().ok()),
        attr(e, "hits").and_then(|v| parse_hits(&v)),
    ) {
        let entry = by_file.entry(file.to_string()).or_default();
        *entry.entry(number).or_insert(0) += hits;
    }
}

/// Locate and read the coverage report a [`CoverageSpec`] points at.
///
/// For [`CoverageFormat::Lcov`] the spec's path is the `lcov.info` file itself.
/// For [`CoverageFormat::Cobertura`] the spec's path is coverlet's
/// `--results-directory`, under which the collector writes
/// `coverage.cobertura.xml` inside a per-run GUID subfolder; the **newest** such
/// file is read, so a fresh run wins over a stale one left in the directory.
///
/// A missing report is an error the caller turns into a warning — coverage
/// being absent must never fail the test run itself.
pub fn load_report(spec: &CoverageSpec) -> Result<Vec<FileCoverage>> {
    match spec.format {
        CoverageFormat::Lcov => {
            let text = std::fs::read_to_string(&spec.path)
                .with_context(|| format!("failed to read coverage {}", spec.path.display()))?;
            parse_lcov(&text)
        }
        CoverageFormat::Cobertura => {
            let file = newest_cobertura(&spec.path).ok_or_else(|| {
                anyhow::anyhow!(
                    "no coverage.cobertura.xml was found under {}",
                    spec.path.display()
                )
            })?;
            let xml = std::fs::read_to_string(&file)
                .with_context(|| format!("failed to read coverage {}", file.display()))?;
            parse_cobertura(&xml)
        }
    }
}

/// The newest `coverage.cobertura.xml` anywhere under `dir`, by modified time.
///
/// coverlet writes one per run into a fresh GUID subfolder, so a directory
/// accumulates them; the most recently modified is this run's.
pub fn newest_cobertura(dir: &Path) -> Option<PathBuf> {
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.eq_ignore_ascii_case("coverage.cobertura.xml"))
        {
            continue;
        }
        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::UNIX_EPOCH);
        if newest.as_ref().is_none_or(|(t, _)| mtime >= *t) {
            newest = Some((mtime, entry.path().to_path_buf()));
        }
    }
    newest.map(|(_, path)| path)
}

/// LCOV and Cobertura both occasionally emit a hit count with a trailing marker
/// or a huge value; parse leniently and clamp to `u32`.
fn parse_hits(raw: &str) -> Option<u32> {
    let digits: String = raw.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    Some(
        digits
            .parse::<u64>()
            .unwrap_or(u32::MAX as u64)
            .min(u32::MAX as u64) as u32,
    )
}

fn attr(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if a.key
            .local_name()
            .as_ref()
            .eq_ignore_ascii_case(name.as_bytes())
        {
            a.unescape_value().ok().map(|v| v.into_owned())
        } else {
            None
        }
    })
}

fn local_name(e: &quick_xml::events::BytesStart) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).into_owned()
}

fn local_name_end(e: &quick_xml::events::BytesEnd) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).into_owned()
}

#[cfg(test)]
#[path = "coverage_tests.rs"]
mod coverage_tests;
