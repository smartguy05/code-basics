//! Deciding which recorded edit produced which line of a diff.
//!
//! A record says "I replaced this text with that text" in some file. A diff
//! says "these lines differ". Joining the two is what turns a wall of hunks
//! into a handful of labelled decisions.
//!
//! # Position is not evidence
//!
//! Records carry line numbers and they are thrown away. Between the moment an
//! agent wrote a line and the moment anyone reviews it, the file has been
//! edited again, run through a formatter, and partly reverted by hand. Every
//! one of those moves the line without changing what it says. **Only text is
//! evidence**, so matching is entirely content-based, and a record is found
//! wherever its text ended up.
//!
//! # A wrong label is worse than no label
//!
//! This is the rule every threshold below is tuned against. An unlabelled hunk
//! costs the reviewer the same effort they spend today; a *confidently
//! mislabelled* hunk invites them to approve code they never read. So the
//! matcher is built to abstain:
//!
//! * Lines too short or too generic to identify anything are never anchors.
//! * A single matching line is almost never enough — agreement has to be
//!   contiguous before it counts.
//! * The normalisation ladder stops well short of anything that could make two
//!   genuinely different lines compare equal.
//!
//! # Hunks are never split
//!
//! Attribution happens per line, then rolls up. A hunk keeps its identity and
//! may carry several labels, because [`DiffLine::index`] is the contract
//! shared with [`crate::git::patch::build_patch`] and the selection UI:
//! renumbering it here would mean reimplementing patch construction. It is
//! also genuinely ambiguous — with three lines of context, two unrelated edits
//! six lines apart arrive as one hunk, and interleaved edits cannot be cut
//! apart at all. So a hunk reports every record that touched it, and names a
//! `dominant` one only when a single record holds a strict majority.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::git::patch::{FileDiff, LineOrigin};
use crate::intents::{IntentRecord, Intents};

/// How far the text had to be bent before it matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum MatchLevel {
    /// Identical once line endings are normalised.
    Exact,
    /// Identical after trimming and collapsing runs of whitespace.
    Whitespace,
    /// Identical once all whitespace and trailing punctuation are removed.
    Skeleton,
}

impl MatchLevel {
    /// How much a match at this level is worth. Skeleton is aggressive enough
    /// that it cannot clear the acceptance bar without near-unique text.
    fn fidelity(self) -> f32 {
        match self {
            MatchLevel::Exact => 1.00,
            MatchLevel::Whitespace => 0.90,
            MatchLevel::Skeleton => 0.75,
        }
    }
}

/// How much to trust an attribution, for rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// One record's claim over part of a hunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AttributedSpan {
    /// Identifies the record: its turn, so a label can be found again.
    pub turn_id: String,
    /// The label, when the turn had one.
    pub label: Option<String>,
    pub seq: u64,
    /// `DiffLine::index` values, ascending. Never contains a context line.
    pub line_indices: Vec<u32>,
    pub confidence: Confidence,
}

/// Attribution for one hunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HunkAttribution {
    /// Index into `FileDiff::hunks`.
    pub hunk: usize,
    /// Most lines first, newest breaking ties.
    pub spans: Vec<AttributedSpan>,
    /// Changed lines in this hunk that no record claimed.
    pub unattributed_lines: u32,
    /// The record holding a strict majority, if any. `None` means the hunk is
    /// genuinely mixed and the UI must say so rather than pick one.
    pub dominant: Option<String>,
}

/// Attribution for one file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileAttribution {
    pub path: String,
    pub hunks: Vec<HunkAttribution>,
}

impl FileAttribution {
    pub fn is_empty(&self) -> bool {
        self.hunks.iter().all(|h| h.spans.is_empty())
    }
}

/// Thresholds, named so tests can pin them instead of hard-coding numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// Minimum score for a run to be believed.
    pub accept_score: f32,
    /// Shortest skeleton that can anchor at all.
    pub min_anchor_len: usize,
    /// Shortest skeleton that can anchor *alone*, with no neighbours.
    pub lone_line_min_len: usize,
    /// Contiguous lines a whole-file write needs before it may claim anything.
    pub write_min_run: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            // Exactly 0.75 x 0.80: a match that only survived the most
            // aggressive normalisation must be carried by near-unique text.
            accept_score: 0.60,
            min_anchor_len: 4,
            lone_line_min_len: 12,
            write_min_run: 3,
        }
    }
}

/// Attribute one file's diff.
pub fn attribute_file(diff: &FileDiff, intents: &Intents, options: Options) -> FileAttribution {
    let records = intents.for_path(&diff.path);

    if diff.is_binary || records.is_empty() {
        return empty_attribution(diff);
    }

    let prepared: Vec<Prepared> = records.iter().map(|r| Prepared::new(r)).collect();
    let claims = resolve(diff, &prepared, options);

    rollup(diff, &claims, &prepared, intents)
}

/// Attribute every file in a working tree.
pub fn attribute(
    diffs: &[FileDiff],
    intents: &Intents,
    options: Options,
) -> Vec<FileAttribution> {
    diffs
        .iter()
        .map(|d| attribute_file(d, intents, options))
        .collect()
}

fn empty_attribution(diff: &FileDiff) -> FileAttribution {
    FileAttribution {
        path: diff.path.clone(),
        hunks: diff
            .hunks
            .iter()
            .enumerate()
            .map(|(index, hunk)| HunkAttribution {
                hunk: index,
                spans: Vec::new(),
                unattributed_lines: hunk
                    .lines
                    .iter()
                    .filter(|l| l.origin != LineOrigin::Context)
                    .count() as u32,
                dominant: None,
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Normalisation
// ---------------------------------------------------------------------------

/// The three forms of a line, computed once.
///
/// The ladder stops here deliberately. Case folding, quote unification,
/// comment stripping and identifier normalisation would each let two lines
/// that genuinely differ compare equal, which is the one failure this whole
/// module is built to avoid.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Forms {
    exact: String,
    whitespace: String,
    skeleton: String,
}

impl Forms {
    fn new(line: &str) -> Self {
        // Undoing a representation difference, not fuzzy matching: without it
        // every CRLF repository attributes nothing at all.
        let exact = line.trim_end_matches('\r').to_string();

        let whitespace = exact.split_whitespace().collect::<Vec<_>>().join(" ");

        let skeleton = whitespace
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .trim_end_matches([',', ';'])
            .to_string();

        Self {
            exact,
            whitespace,
            skeleton,
        }
    }

    fn at(&self, level: MatchLevel) -> &str {
        match level {
            MatchLevel::Exact => &self.exact,
            MatchLevel::Whitespace => &self.whitespace,
            MatchLevel::Skeleton => &self.skeleton,
        }
    }

    /// Can this line identify anything on its own?
    ///
    /// A bare `}`, `);` or blank line is the commonest line in any C-family
    /// file. Allowing it to anchor would match every record against every
    /// hunk, so weak lines may only be inherited from neighbours.
    fn is_anchor(&self, options: &Options) -> bool {
        self.skeleton.len() >= options.min_anchor_len
            && self.skeleton.chars().any(|c| c.is_alphanumeric())
    }
}

// ---------------------------------------------------------------------------
// Records, prepared for matching
// ---------------------------------------------------------------------------

struct Prepared<'a> {
    record: &'a IntentRecord,
    /// Lines the record removed, after the shared frame is stripped.
    old: Vec<Forms>,
    /// Lines the record added, after the shared frame is stripped.
    new: Vec<Forms>,
    /// How often each skeleton occurs *within this record*. A record that
    /// contains the same line twice cannot say which of them a diff line is.
    counts: HashMap<String, usize>,
}

impl<'a> Prepared<'a> {
    fn new(record: &'a IntentRecord) -> Self {
        let old: Vec<Forms> = record.edit.old_lines.iter().map(|l| Forms::new(l)).collect();
        let new: Vec<Forms> = record.edit.new_lines.iter().map(|l| Forms::new(l)).collect();

        // An edit's before and after text share the surrounding lines that
        // made the match unique. Those lines are not what the record changed,
        // and treating them as evidence would let a record claim the context
        // around every similar edit in the file.
        let (old, new) = strip_shared_frame(old, new);

        let mut counts: HashMap<String, usize> = HashMap::new();
        for forms in old.iter().chain(new.iter()) {
            *counts.entry(forms.skeleton.clone()).or_default() += 1;
        }

        Self {
            record,
            old,
            new,
            counts,
        }
    }

    fn side(&self, origin: LineOrigin) -> &[Forms] {
        match origin {
            LineOrigin::Deletion => &self.old,
            _ => &self.new,
        }
    }
}

/// Drop the common leading and trailing lines of a replacement.
fn strip_shared_frame(old: Vec<Forms>, new: Vec<Forms>) -> (Vec<Forms>, Vec<Forms>) {
    // A pure insertion or deletion has no frame to share.
    if old.is_empty() || new.is_empty() {
        return (old, new);
    }

    let same = |a: &Forms, b: &Forms| a.whitespace == b.whitespace;

    let mut front = 0;
    while front < old.len() && front < new.len() && same(&old[front], &new[front]) {
        front += 1;
    }

    let mut back = 0;
    while back < old.len() - front && back < new.len() - front
        && same(&old[old.len() - 1 - back], &new[new.len() - 1 - back])
    {
        back += 1;
    }

    (
        old[front..old.len() - back].to_vec(),
        new[front..new.len() - back].to_vec(),
    )
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

/// One diff line matched against one line of one record.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    record: usize,
    /// Position within the record, so a run can require forward progress.
    record_line: usize,
    level: MatchLevel,
}

/// A line's winning claim.
#[derive(Debug, Clone, Copy)]
struct Claim {
    record: usize,
    level: MatchLevel,
    confidence: Confidence,
}

/// How ambiguous each line's text is across the whole file's diff.
///
/// Deliberately *not* counted across records: the same text appearing in two
/// records is an edit that was made twice, which recency resolves. Treating
/// that as ambiguity would reject both and lose the change entirely.
struct Corpus {
    diff_counts: HashMap<String, usize>,
}

fn resolve(diff: &FileDiff, prepared: &[Prepared], options: Options) -> BTreeMap<u32, Claim> {
    // Every changed line of the diff, in index order, with its forms.
    let mut changed: Vec<(usize, u32, LineOrigin, Forms)> = Vec::new();
    for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
        for line in &hunk.lines {
            if line.origin == LineOrigin::Context {
                continue;
            }
            changed.push((hunk_index, line.index, line.origin, Forms::new(&line.content)));
        }
    }

    let corpus = build_corpus(&changed);
    let candidates = find_candidates(prepared, &changed, &options);
    let runs = build_runs(&changed, &candidates, prepared, &corpus, &options);

    let mut claims = apply_runs(runs, prepared, options);
    inherit_weak_lines(&changed, prepared, &mut claims, &options);
    claims
}

fn build_corpus(changed: &[(usize, u32, LineOrigin, Forms)]) -> Corpus {
    let mut diff_counts: HashMap<String, usize> = HashMap::new();

    for (_, _, _, forms) in changed {
        *diff_counts.entry(forms.skeleton.clone()).or_default() += 1;
    }

    Corpus { diff_counts }
}

/// For each changed line, every record line it could correspond to.
///
/// Probing stops at the first level that produces any hit, so an exact truth
/// is never outranked by a skeleton coincidence.
fn find_candidates(
    prepared: &[Prepared],
    changed: &[(usize, u32, LineOrigin, Forms)],
    options: &Options,
) -> Vec<Vec<Candidate>> {
    changed
        .iter()
        .map(|(_, _, origin, forms)| {
            if !forms.is_anchor(options) {
                return Vec::new();
            }

            for level in [MatchLevel::Exact, MatchLevel::Whitespace, MatchLevel::Skeleton] {
                let key = forms.at(level);
                let mut hits = Vec::new();

                for (index, p) in prepared.iter().enumerate() {
                    for (line, candidate) in p.side(*origin).iter().enumerate() {
                        if candidate.at(level) == key {
                            hits.push(Candidate {
                                record: index,
                                record_line: line,
                                level,
                            });
                        }
                    }
                }

                if !hits.is_empty() {
                    return hits;
                }
            }

            Vec::new()
        })
        .collect()
}

/// A contiguous stretch of one record's lines found in the diff.
struct Run {
    record: usize,
    /// `DiffLine::index` values this run covers, ascending.
    indices: Vec<u32>,
    level: MatchLevel,
    score: f32,
}

/// Group candidates into runs and score them.
///
/// Contiguity is the whole point. One line matching proves very little; three
/// consecutive lines of a record appearing consecutively in the diff is strong
/// evidence even when each line alone is unremarkable.
fn build_runs(
    changed: &[(usize, u32, LineOrigin, Forms)],
    candidates: &[Vec<Candidate>],
    prepared: &[Prepared],
    corpus: &Corpus,
    options: &Options,
) -> Vec<Run> {
    let mut runs = Vec::new();

    for record in 0..prepared.len() {
        // This record's matches, in diff order.
        let mut hits: Vec<(usize, Candidate)> = Vec::new();
        for (position, line_candidates) in candidates.iter().enumerate() {
            if let Some(c) = line_candidates.iter().find(|c| c.record == record) {
                hits.push((position, *c));
            }
        }
        if hits.is_empty() {
            continue;
        }

        let mut current: Vec<(usize, Candidate)> = Vec::new();

        for hit in hits {
            let extends = current.last().is_some_and(|(prev_pos, prev)| {
                // Same hunk, near enough, and moving forward through the
                // record. The forward requirement stops a coincidental match
                // against a distant part of the same record gluing itself on.
                changed[*prev_pos].0 == changed[hit.0].0
                    && hit.0 <= prev_pos + 2
                    && hit.1.record_line > prev.record_line
            });

            if !extends && !current.is_empty() {
                runs.extend(finish_run(record, &current, prepared, corpus, changed, options));
                current.clear();
            }
            current.push(hit);
        }
        runs.extend(finish_run(record, &current, prepared, corpus, changed, options));
    }

    runs
}

fn finish_run(
    record: usize,
    hits: &[(usize, Candidate)],
    prepared: &[Prepared],
    corpus: &Corpus,
    changed: &[(usize, u32, LineOrigin, Forms)],
    options: &Options,
) -> Option<Run> {
    if hits.is_empty() {
        return None;
    }

    // The weakest link decides: a run is only as trustworthy as its least
    // convincing line.
    let level = hits.iter().map(|(_, c)| c.level).max().unwrap_or(MatchLevel::Skeleton);
    let fidelity = level.fidelity();

    // A line is distinctive when it could not plausibly have matched by
    // coincidence: it appears once in the diff, and once in the record that
    // claims it.
    let unique = hits
        .iter()
        .filter(|(position, _)| {
            let key = &changed[*position].3.skeleton;
            corpus.diff_counts.get(key).copied().unwrap_or(0) <= 1
                && prepared[record].counts.get(key).copied().unwrap_or(0) <= 1
        })
        .count();
    let distinctness = unique as f32 / hits.len() as f32;

    let bulk = if hits.len() >= 2 { 1.0 } else { 0.6 };
    let score = fidelity * distinctness * bulk;

    // A lone line has to be long, unique and essentially verbatim.
    if hits.len() == 1 {
        let forms = &changed[hits[0].0].3;
        let strong = forms.skeleton.len() >= options.lone_line_min_len
            && distinctness >= 1.0
            && level != MatchLevel::Skeleton;
        if !strong {
            return None;
        }
    }

    if score < options.accept_score {
        return None;
    }

    Some(Run {
        record,
        indices: hits.iter().map(|(p, _)| changed[*p].1).collect(),
        level,
        score,
    })
}

/// Only a verbatim match over distinctive text earns `High`.
///
/// The bar sits above what a whitespace-level match can reach (0.90), so
/// "identical apart from formatting" is reported as `Medium`. That is the
/// honest answer: the text on disk is not what the agent wrote, and something
/// else has been through the file since.
fn confidence_for(score: f32) -> Confidence {
    if score >= 0.95 {
        Confidence::High
    } else if score >= 0.72 {
        Confidence::Medium
    } else {
        Confidence::Low
    }
}

/// Turn accepted runs into per-line claims, resolving overlaps.
///
/// Whole-file writes are settled last and only where nothing else reached: a
/// write's text is the entire file, so unrestricted it would claim every line
/// including ones the user typed themselves.
fn apply_runs(runs: Vec<Run>, prepared: &[Prepared], options: Options) -> BTreeMap<u32, Claim> {
    let mut claims: BTreeMap<u32, Claim> = BTreeMap::new();

    let (writes, replaces): (Vec<Run>, Vec<Run>) = runs
        .into_iter()
        .partition(|r| prepared[r.record].record.edit.whole_file);

    for run in replaces {
        insert_run(&mut claims, &run, prepared, false, options);
    }
    for run in writes {
        insert_run(&mut claims, &run, prepared, true, options);
    }

    claims
}

fn insert_run(
    claims: &mut BTreeMap<u32, Claim>,
    run: &Run,
    prepared: &[Prepared],
    is_write: bool,
    options: Options,
) {
    // A whole-file write is weak evidence by construction, so it has to clear
    // a much higher bar than a targeted edit before claiming anything.
    if is_write && (run.indices.len() < options.write_min_run || run.level != MatchLevel::Exact) {
        return;
    }

    let confidence = confidence_for(run.score);

    for &index in &run.indices {
        match claims.get(&index) {
            // A write never displaces a targeted edit.
            Some(_) if is_write => continue,
            Some(existing) if !beats(run, existing, prepared) => continue,
            _ => {}
        }

        claims.insert(
            index,
            Claim {
                record: run.record,
                level: run.level,
                confidence,
            },
        );
    }
}

/// Later edits win, except where an older one matched more faithfully: an
/// exact match means that text is literally what is on disk now.
fn beats(run: &Run, existing: &Claim, prepared: &[Prepared]) -> bool {
    let new_record = prepared[run.record].record;
    let old_record = prepared[existing.record].record;

    match run.level.cmp(&existing.level) {
        std::cmp::Ordering::Less => true,      // better fidelity (Exact < Skeleton)
        std::cmp::Ordering::Greater => false,
        std::cmp::Ordering::Equal => new_record.seq > old_record.seq,
    }
}

/// Let punctuation-only lines follow their neighbours.
///
/// A closing brace added as part of one record's block should belong to that
/// record, but its text identifies nothing. It is adopted only when both
/// neighbours agree *and* the record actually contains that text — otherwise
/// it stays unattributed.
fn inherit_weak_lines(
    changed: &[(usize, u32, LineOrigin, Forms)],
    prepared: &[Prepared],
    claims: &mut BTreeMap<u32, Claim>,
    options: &Options,
) {
    let mut adopted: Vec<(u32, Claim)> = Vec::new();

    for (position, (hunk, index, origin, forms)) in changed.iter().enumerate() {
        if claims.contains_key(index) || forms.is_anchor(options) {
            continue;
        }

        let before = neighbour(changed, claims, position, *hunk, false);
        let after = neighbour(changed, claims, position, *hunk, true);

        let record = match (before, after) {
            (Some(a), Some(b)) if a.record == b.record => a.record,
            (Some(a), None) => a.record,
            (None, Some(b)) => b.record,
            _ => continue,
        };

        // The record must actually contain this text on the matching side.
        let present = prepared[record]
            .side(*origin)
            .iter()
            .any(|f| f.whitespace == forms.whitespace);
        if !present {
            continue;
        }

        adopted.push((
            *index,
            Claim {
                record,
                level: MatchLevel::Whitespace,
                confidence: Confidence::Medium,
            },
        ));
    }

    for (index, claim) in adopted {
        claims.insert(index, claim);
    }
}

/// The nearest attributed changed line on one side, within the same hunk.
fn neighbour(
    changed: &[(usize, u32, LineOrigin, Forms)],
    claims: &BTreeMap<u32, Claim>,
    from: usize,
    hunk: usize,
    forward: bool,
) -> Option<Claim> {
    let mut position = from;

    loop {
        position = if forward {
            position.checked_add(1)?
        } else {
            position.checked_sub(1)?
        };
        let (line_hunk, index, _, _) = changed.get(position)?;
        if *line_hunk != hunk {
            return None;
        }
        if let Some(claim) = claims.get(index) {
            return Some(*claim);
        }
    }
}

// ---------------------------------------------------------------------------
// Rollup
// ---------------------------------------------------------------------------

fn rollup(
    diff: &FileDiff,
    claims: &BTreeMap<u32, Claim>,
    prepared: &[Prepared],
    intents: &Intents,
) -> FileAttribution {
    let mut hunks = Vec::with_capacity(diff.hunks.len());

    for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
        let changed: Vec<u32> = hunk
            .lines
            .iter()
            .filter(|l| l.origin != LineOrigin::Context)
            .map(|l| l.index)
            .collect();

        let mut by_record: BTreeMap<usize, (Vec<u32>, Confidence)> = BTreeMap::new();
        let mut unattributed = 0u32;

        for index in &changed {
            match claims.get(index) {
                Some(claim) => {
                    let entry = by_record
                        .entry(claim.record)
                        .or_insert_with(|| (Vec::new(), Confidence::High));
                    entry.0.push(*index);
                    entry.1 = entry.1.min(claim.confidence);
                }
                None => unattributed += 1,
            }
        }

        let mut spans: Vec<AttributedSpan> = by_record
            .into_iter()
            .map(|(record, (line_indices, confidence))| {
                let source = prepared[record].record;
                AttributedSpan {
                    turn_id: source.turn_id.clone(),
                    label: intents.label_for(source).map(|l| l.label.clone()),
                    seq: source.seq,
                    line_indices,
                    confidence,
                }
            })
            .collect();

        spans.sort_by(|a, b| {
            b.line_indices
                .len()
                .cmp(&a.line_indices.len())
                .then(b.seq.cmp(&a.seq))
                .then(a.turn_id.cmp(&b.turn_id))
        });

        // Strictly more than half, so an even split reports no dominant record
        // and the UI has to say "mixed".
        let dominant = spans
            .first()
            .filter(|s| s.line_indices.len() * 2 > changed.len())
            .map(|s| s.turn_id.clone());

        hunks.push(HunkAttribution {
            hunk: hunk_index,
            spans,
            unattributed_lines: unattributed,
            dominant,
        });
    }

    FileAttribution {
        path: diff.path.clone(),
        hunks,
    }
}

#[cfg(test)]
#[path = "attribution_tests.rs"]
mod tests;
