//! Diffing the console output of the two runs.
//!
//! Console output is the noisiest of the three signals: every line can carry a
//! timestamp, a temp path, a process id, an address, and async logs interleave
//! differently run to run. Comparing it raw would report a "behavioral change"
//! on every run. So both sides are **normalised** first — ANSI stripped,
//! timestamps / hex ids / the two run roots masked to fixed tokens — and only
//! then compared, as multisets of lines (interleave order is not signal).
//!
//! The abstain rule, sharpened for how weak this evidence is:
//!
//! * Equal after normalising ⇒ **no delta** — the run said the same thing, the
//!   noise just wore different clothes.
//! * A surviving difference is reported at [`Confidence::Medium`] at best, and
//!   drops to [`Confidence::Low`] when masking had to touch a large share of
//!   the lines or ordering was deliberately ignored — at that point the diff is
//!   a hint, not proof.

use std::sync::OnceLock;

use regex::Regex;

use super::ConsoleDelta;
use crate::git::attribution::Confidence;

/// What to mask before comparing, and when to distrust the result.
pub struct ConsoleNormalization {
    pub strip_ansi: bool,
    pub mask_timestamps: bool,
    pub mask_hex_ids: bool,
    /// Absolute paths (both run roots, temp dirs) collapsed to `<root>`, so the
    /// mere fact that the two sides ran in different directories is never itself
    /// a difference. Longest first is applied by [`diff_console`].
    pub roots: Vec<String>,
    /// Treat output as order-independent (for known-nondeterministic logs).
    /// Only affects confidence here — comparison is always by multiset.
    pub ignore_ordering: bool,
    /// If masking changed more than this fraction of all lines, a surviving
    /// delta is only [`Confidence::Low`].
    pub heavy_mask_fraction: f64,
}

impl Default for ConsoleNormalization {
    fn default() -> Self {
        Self {
            strip_ansi: true,
            mask_timestamps: true,
            mask_hex_ids: true,
            roots: Vec::new(),
            ignore_ordering: false,
            heavy_mask_fraction: 0.5,
        }
    }
}

fn ansi() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]").unwrap())
}

fn iso_ts() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?")
            .unwrap()
    })
}

fn clock_ts() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b\d{2}:\d{2}:\d{2}(?:\.\d+)?\b").unwrap())
}

fn epoch_ms() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b\d{13}\b").unwrap())
}

fn guid() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b")
            .unwrap()
    })
}

fn long_hex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(?:0x)?[0-9a-fA-F]{16,}\b").unwrap())
}

/// Mask everything except ANSI (that is stripped separately, up front).
fn mask_content(line: &str, norm: &ConsoleNormalization, roots_sorted: &[String]) -> String {
    let mut s = line.to_string();
    // Paths first: a temp path can contain digits a timestamp regex would eat.
    for root in roots_sorted {
        s = s.replace(root, "<root>");
        // Also collapse the forward-slash spelling of the same root.
        let fwd = root.replace('\\', "/");
        if fwd != *root {
            s = s.replace(&fwd, "<root>");
        }
    }
    if norm.mask_timestamps {
        s = iso_ts().replace_all(&s, "<ts>").into_owned();
        s = clock_ts().replace_all(&s, "<ts>").into_owned();
        s = epoch_ms().replace_all(&s, "<ts>").into_owned();
    }
    if norm.mask_hex_ids {
        s = guid().replace_all(&s, "<id>").into_owned();
        s = long_hex().replace_all(&s, "<id>").into_owned();
    }
    s
}

/// Mask timestamps and hex ids in one line — the value-level noise shared with
/// HTTP body comparison (which has no ANSI or run roots to strip).
pub(crate) fn mask_timestamps_and_ids(s: &str) -> String {
    let mut s = iso_ts().replace_all(s, "<ts>").into_owned();
    s = clock_ts().replace_all(&s, "<ts>").into_owned();
    s = epoch_ms().replace_all(&s, "<ts>").into_owned();
    s = guid().replace_all(&s, "<id>").into_owned();
    s = long_hex().replace_all(&s, "<id>").into_owned();
    s
}

/// Multiset difference: lines in `from` not covered by `against`, in `from`
/// order.
pub(crate) fn multiset_minus(from: &[String], against: &[String]) -> Vec<String> {
    let mut remaining: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    for line in against {
        *remaining.entry(line.as_str()).or_insert(0) += 1;
    }
    let mut out = Vec::new();
    for line in from {
        let count = remaining.entry(line.as_str()).or_insert(0);
        if *count > 0 {
            *count -= 1;
        } else {
            out.push(line.clone());
        }
    }
    out
}

/// Compare the two runs' console output after normalisation.
pub fn diff_console(base: &str, work: &str, norm: &ConsoleNormalization) -> ConsoleDelta {
    // Longest roots first so a nested root does not pre-empt its parent.
    let mut roots_sorted = norm.roots.clone();
    roots_sorted.sort_by_key(|r| std::cmp::Reverse(r.len()));

    let mut masked_lines = 0usize;
    let mut total_lines = 0usize;
    let mut any_masking = false;

    let normalize = |text: &str,
                     masked: &mut usize,
                     total: &mut usize,
                     any: &mut bool|
     -> Vec<String> {
        text.lines()
            .map(|raw| {
                *total += 1;
                let stripped = if norm.strip_ansi {
                    let s = ansi().replace_all(raw, "").into_owned();
                    if s != raw {
                        *any = true;
                    }
                    s
                } else {
                    raw.to_string()
                };
                let content = mask_content(&stripped, norm, &roots_sorted);
                if content != stripped {
                    *masked += 1;
                    *any = true;
                }
                content
            })
            .collect()
    };

    let base_lines = normalize(base, &mut masked_lines, &mut total_lines, &mut any_masking);
    let work_lines = normalize(work, &mut masked_lines, &mut total_lines, &mut any_masking);

    let removed = multiset_minus(&base_lines, &work_lines);
    let added = multiset_minus(&work_lines, &base_lines);

    let confidence = if added.is_empty() && removed.is_empty() {
        // Same output once the noise is masked — a confident "no change".
        Confidence::High
    } else {
        let fraction = if total_lines == 0 {
            0.0
        } else {
            masked_lines as f64 / total_lines as f64
        };
        if norm.ignore_ordering || fraction > norm.heavy_mask_fraction {
            Confidence::Low
        } else {
            // Console output is weak evidence even at its best.
            Confidence::Medium
        }
    };

    ConsoleDelta {
        added_lines: added,
        removed_lines: removed,
        normalized: any_masking,
        confidence,
    }
}

impl ConsoleDelta {
    /// Did the two runs' output actually differ after normalisation?
    pub fn is_change(&self) -> bool {
        !self.added_lines.is_empty() || !self.removed_lines.is_empty()
    }
}

#[cfg(test)]
#[path = "console_tests.rs"]
mod tests;
