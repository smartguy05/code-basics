//! Rejecting a change: undoing it and leaving the reason where it was.
//!
//! Reverting a hunk is silent. The code disappears and the agent that wrote it
//! learns nothing, so next turn it writes the same thing again — the reviewer
//! pays for the same mistake twice. Rejecting is reverting plus a note left at
//! the exact spot, saying what was wrong and asking for it to be done properly.
//!
//! The note is a comment in the file rather than an entry in a sidecar log for
//! one reason: the agent reads the file. It does not read `.code-basics/`, and
//! nothing can make it. A comment three lines above the code it concerns is
//! seen at the moment it matters.
//!
//! # Why line comments only
//!
//! A block comment would cover every file type, and a `/*` that fails to close
//! silently comments out the rest of the file. Line comments cannot do that,
//! and no reason text — however mangled — can escape one. So a file with no
//! line-comment syntax is reverted and left unmarked, and the caller says so;
//! see [`comment_prefix`].
//!
//! # Why the marker has to be removable
//!
//! A note nobody deletes is noise within a week. The block therefore states
//! its own exit condition (fix it, then delete these lines), the instruction
//! file installed alongside capture repeats it, and
//! [`super::guard`] blocks a commit that still carries one. The marker is a
//! work item that cannot be silently abandoned.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::git::patch::{FileDiff, LineOrigin};
use crate::git::{ComparisonMode, Repo};

/// The token every marker carries, and what the commit guard greps for.
pub const MARKER: &str = "AI-REJECTED";

/// Longest reason kept verbatim. A reason is a sentence, not an essay; past
/// this it stops being readable in a gutter and starts burying the code.
pub const MAX_REASON: usize = 400;

/// Characters of reason text per line, before the prefix and indent are added.
pub const WRAP: usize = 72;

/// Introduces the reason itself.
const REASON_LABEL: &str = "Reason:";

/// Introduces — and terminates — the block. Finding this line is how a
/// previously written marker's extent is recognised, so it is always last.
const NEXT_LABEL: &str = "Next:";

/// A hand-edited marker must not send the scan running through the whole file.
const MAX_BLOCK_LINES: usize = 12;

/// The line-comment syntax for a path, or `None` when it has none.
///
/// `None` is a real answer, not a failure: JSON, CSS and Markdown have no line
/// comment that is safe to inject, so they are reverted without a marker and
/// the caller reports which files went unmarked.
pub fn comment_prefix(path: &str) -> Option<&'static str> {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();

    // Some files are identified by name rather than extension.
    match name.as_str() {
        "dockerfile" | "makefile" | "gnumakefile" | ".gitignore" | ".gitattributes"
        | ".dockerignore" | ".env" => return Some("#"),
        _ => {}
    }

    let extension = name.rsplit_once('.').map(|(_, e)| e)?;

    Some(match extension {
        "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "c" | "h" | "cc" | "cpp" | "hpp"
        | "cs" | "go" | "java" | "kt" | "kts" | "swift" | "scala" | "php" | "dart" | "proto"
        | "scss" | "less" | "zig" => "//",

        "py" | "rb" | "sh" | "bash" | "zsh" | "fish" | "yaml" | "yml" | "toml" | "ps1" | "psm1"
        | "pl" | "pm" | "r" | "nix" | "tf" | "ini" | "cfg" | "conf" | "gitignore" => "#",

        "sql" | "lua" | "hs" | "elm" | "adb" | "ads" => "--",

        _ => return None,
    })
}

/// Reduce a reason typed into a text box to something that fits in a comment.
///
/// Newlines are the point: the reason arrives from a multi-line input, and one
/// raw newline would leave the tail of it sitting in the file as live code.
pub fn sanitise_reason(reason: &str) -> String {
    let flat = reason.split_whitespace().collect::<Vec<_>>().join(" ");

    if flat.len() <= MAX_REASON {
        return flat;
    }

    // Truncate on a character boundary, and make the truncation visible rather
    // than letting the reason appear to end mid-thought.
    let budget = MAX_REASON - '…'.len_utf8();
    let mut cut = budget;
    while cut > 0 && !flat.is_char_boundary(cut) {
        cut -= 1;
    }

    let mut out = flat[..cut].trim_end().to_string();
    out.push('…');
    out
}

/// The comment block for one rejection, one line per element.
///
/// Three parts, in the order a reader needs them: that this is a rejection and
/// when, why, and what to do about it. The last line is what
/// [`insert_markers`] later recognises as the end of the block.
pub fn marker_block(prefix: &str, indent: &str, reason: &str, date: &str) -> Vec<String> {
    let head = format!("{MARKER} {date} — reverted during review.");
    let next = format!("{NEXT_LABEL} fix this properly, then delete these {MARKER} lines.");

    let mut body = vec![head];
    body.extend(wrap(&format!("{REASON_LABEL} {reason}"), WRAP));
    body.extend(wrap(&next, WRAP));

    body.into_iter()
        .map(|line| format!("{indent}{prefix} {line}"))
        .collect()
}

/// Is this the first line of a marker block?
///
/// Only the head line matches. The block mentions [`MARKER`] again in its
/// closing instruction, and counting that as a second marker would make one
/// rejection look like two.
pub fn is_marker_line(line: &str) -> bool {
    comment_text(line).is_some_and(|text| text.starts_with(MARKER))
}

/// The text of a line comment, whatever syntax introduced it.
fn comment_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let stripped = trimmed
        .strip_prefix("//")
        .or_else(|| trimmed.strip_prefix("--"))
        .or_else(|| trimmed.strip_prefix('#'))?;

    Some(stripped.trim_start())
}

/// Greedy word wrap. A word longer than `width` keeps its own line intact —
/// breaking an identifier or a path mid-way corrupts the one part of the
/// message that has to be copied exactly.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Where each rejected hunk's marker belongs, as a 1-based line number in the
/// file *as it will be once the rejected hunks are reverted*.
///
/// Two translations happen here, and skipping either puts the marker somewhere
/// misleading. The baseline position of a hunk is not where it sits in the
/// working copy; and a hunk that is being *kept* still has its added lines in
/// the file, pushing everything below it down.
pub fn anchors(diff: &FileDiff, rejected: &[usize]) -> Vec<u32> {
    let mut wanted: Vec<usize> = rejected
        .iter()
        .copied()
        .filter(|i| *i < diff.hunks.len())
        .collect();
    wanted.sort_unstable();
    wanted.dedup();

    let mut out = Vec::with_capacity(wanted.len());

    for &index in &wanted {
        let hunk = &diff.hunks[index];

        // An addition-only hunk removed nothing, so there is no restored line
        // to sit above: the marker goes where the additions were, immediately
        // after the baseline line they followed.
        let baseline = if hunk.old_lines == 0 {
            hunk.old_start + 1
        } else {
            hunk.lines
                .iter()
                .filter(|l| l.origin == LineOrigin::Deletion)
                .filter_map(|l| l.old_lineno)
                .min()
                .unwrap_or(hunk.old_start)
        };

        let shift: i64 = diff
            .hunks
            .iter()
            .enumerate()
            .filter(|(other, _)| !wanted.contains(other))
            .filter(|(_, h)| h.old_start < baseline)
            .map(|(_, h)| i64::from(h.new_lines) - i64::from(h.old_lines))
            .sum();

        out.push((i64::from(baseline) + shift).max(1) as u32);
    }

    out
}

/// Insert a marker block above each anchor.
///
/// Anchors are applied bottom-up: inserting at the top first would shift every
/// line number below it, so each later anchor would land further and further
/// from the code it explains.
pub fn insert_markers(
    text: &str,
    anchors: &[u32],
    prefix: &str,
    reason: &str,
    date: &str,
) -> String {
    if anchors.is_empty() {
        return text.to_string();
    }

    // Whether the file ended with a newline is not ours to change.
    let trailing_newline = text.is_empty() || text.ends_with('\n');
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();

    let mut targets: Vec<u32> = anchors.to_vec();
    targets.sort_unstable();
    targets.dedup();

    for anchor in targets.into_iter().rev() {
        let at = (anchor.max(1) as usize - 1).min(lines.len());

        // Rejecting the same place twice happens: the agent tried again and got
        // it wrong again. Replace the old note rather than stacking a second.
        remove_block_at(&mut lines, at);

        let indent = indent_at(&lines, at);
        for (offset, line) in marker_block(prefix, &indent, reason, date)
            .into_iter()
            .enumerate()
        {
            lines.insert(at + offset, line);
        }
    }

    let mut out = lines.join("\n");
    if trailing_newline && !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Drop a marker block already sitting at `at`, if there is one.
///
/// The block's extent is its closing instruction, not "however many comment
/// lines follow" — a marker written above the user's own comment must not take
/// that comment with it.
fn remove_block_at(lines: &mut Vec<String>, at: usize) {
    if !lines.get(at).is_some_and(|l| is_marker_line(l)) {
        return;
    }

    let end = (at..lines.len().min(at + MAX_BLOCK_LINES))
        .find(|i| comment_text(&lines[*i]).is_some_and(|t| t.starts_with(NEXT_LABEL)));

    // No terminator means the block was hand-edited and its extent is unknown.
    // Removing just the head line is the conservative reading.
    let end = end.unwrap_or(at);
    lines.drain(at..=end);
}

/// The indentation to give a marker: that of the line it will precede, or of
/// the nearest code above when that line is blank or past the end of the file.
fn indent_at(lines: &[String], at: usize) -> String {
    let reference = lines.get(at).filter(|l| !l.trim().is_empty()).or_else(|| {
        lines[..at.min(lines.len())]
            .iter()
            .rev()
            .find(|l| !l.trim().is_empty())
    });

    reference
        .map(|line| line[..line.len() - line.trim_start().len()].to_string())
        .unwrap_or_default()
}

/// A date as `YYYY-MM-DD`, in UTC.
///
/// Taken as an argument rather than read from the clock so the result is
/// reproducible in tests. No date crate is pulled in for three lines of
/// arithmetic.
pub fn iso_date(at: SystemTime) -> String {
    let secs = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a calendar
/// date. Shifting the year to start in March is what makes the leap day fall
/// at the end of a year, where it needs no special case.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;

    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;

    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_position = (5 * day_of_year + 2) / 153;

    let day = (day_of_year - (153 * month_position + 2) / 5 + 1) as u32;
    let month = if month_position < 10 {
        month_position + 3
    } else {
        month_position - 9
    } as u32;

    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// What rejecting one file actually did.
///
/// `marked` being false with `reverted` true is a normal outcome, not an error:
/// the file has no line-comment syntax. The caller surfaces it so the reviewer
/// knows their reason went nowhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRejection {
    pub path: String,
    pub reverted: bool,
    pub marked: bool,
}

/// What rejecting a whole group did.
///
/// `unmarked` is the field that earns its place: a reviewer who typed a reason
/// needs to know it was not written anywhere, and silence would let them
/// believe the agent had been told.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RejectSummary {
    pub reverted: usize,
    /// Files that now carry the reason.
    pub marked: Vec<String>,
    /// Files reverted without a note, having no line-comment syntax.
    pub unmarked: Vec<String>,
}

impl RejectSummary {
    pub fn record(&mut self, outcome: FileRejection) {
        if !outcome.reverted {
            return;
        }

        self.reverted += 1;
        if outcome.marked {
            self.marked.push(outcome.path);
        } else {
            self.unmarked.push(outcome.path);
        }
    }
}

/// Revert a file's share of a rejected group and leave the reason behind.
///
/// The diff is read *before* the revert, because that is the only moment the
/// rejected hunks' geometry still exists.
pub fn reject_file(
    repo: &Repo,
    path: &str,
    mode: ComparisonMode,
    selected: &BTreeSet<u32>,
    rejected_hunks: &[usize],
    reason: &str,
    at: SystemTime,
) -> Result<FileRejection> {
    let diff = repo.file_diff(path, mode)?;
    let spots = anchors(&diff, rejected_hunks);

    let reverted = repo.revert_lines(path, mode, selected)?;

    let mut marked = false;
    if reverted && !spots.is_empty() && !diff.is_binary {
        let reason = sanitise_reason(reason);
        if let (Some(prefix), false) = (comment_prefix(path), reason.is_empty()) {
            marked = mark_file(
                &repo.workdir().join(path),
                &spots,
                prefix,
                &reason,
                &iso_date(at),
            )?;
        }
    }

    Ok(FileRejection {
        path: path.to_string(),
        reverted,
        marked,
    })
}

fn mark_file(full: &Path, spots: &[u32], prefix: &str, reason: &str, date: &str) -> Result<bool> {
    let text = std::fs::read_to_string(full)
        .with_context(|| format!("failed to read {} to mark it", full.display()))?;

    let updated = insert_markers(&text, spots, prefix, reason, date);
    if updated == text {
        return Ok(false);
    }

    std::fs::write(full, updated)
        .with_context(|| format!("failed to write the rejection note into {}", full.display()))?;
    Ok(true)
}

#[cfg(test)]
#[path = "reject_tests.rs"]
mod tests;
