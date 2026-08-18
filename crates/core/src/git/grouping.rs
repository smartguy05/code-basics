//! Turning hunks into a handful of decisions.
//!
//! Twelve hunks scattered across five files are twelve things to read even
//! when they are really four: a new method, a rename, two new tests, and a
//! formatter run. This module does the collapsing, in three passes of
//! decreasing confidence.
//!
//! 1. **What the agent said.** If [`crate::git::attribution`] matched a hunk
//!    to a recorded edit with a label, that label is the group. Nothing here
//!    can beat being told.
//! 2. **Formatting.** A hunk whose removed and added lines are the same text
//!    with different whitespace changed no code. This is decidable rather than
//!    guessed, so it is worth doing regardless of whether anything was
//!    recorded.
//! 3. **The enclosing symbol.** Failing both, group by the function or type
//!    the hunk sits in, which git already worked out when it wrote the hunk
//!    header.
//!
//! # On the third pass being approximate
//!
//! Git derives the hunk header from a per-language regex, and the fallback
//! here is a bracket-and-indent scan. Neither is a parser, and both will
//! sometimes name the wrong symbol or none at all. That is acceptable *because
//! of where it sits*: a hunk only reaches this pass when nothing better is
//! available, and a card labelled with the wrong function name is still
//! grouped with the hunks around it. What is never done is inventing an
//! *intent* — a symbol name is presented as a location, not as a reason.
//!
//! Rename detection is deliberately not attempted. Git's own `-M` is
//! similarity-based rather than exact, and a wrongly claimed rename reads as a
//! much stronger statement than "these two hunks are near each other".

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::git::attribution::{Confidence, FileAttribution};
use crate::git::patch::{FileDiff, Hunk, LineOrigin};
use crate::intents::LabelSource;
// The declaration heuristic lives in `symbols` because it is a property of
// source text, not of a repository; what stays here is the hunk-header half.
use crate::symbols::declarations::{declaration_name, NOT_A_SYMBOL};

/// Why a set of hunks belongs together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GroupKind {
    /// The agent said what it was doing.
    Intent,
    /// One turn made these hunks, but never said why. Grouped because they
    /// really did change together; titled from the changes, not from a reason.
    SameTurn,
    /// Whitespace only — no code changed.
    Formatting,
    /// A symbol that does not exist in the baseline.
    NewSymbol,
    /// An existing symbol whose body changed.
    ModifiedSymbol,
    /// Nothing could be determined; grouped by file so it is still reviewable.
    Other,
}

/// The lines of one file that belong to a group.
///
/// Line indices are only meaningful for the comparison mode the diff was
/// produced in, which is why [`IntentGroup`] carries no indices of its own and
/// callers re-derive them per action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupFile {
    pub path: String,
    /// `DiffLine::index` values, ascending.
    pub line_indices: Vec<u32>,
    /// Indices into `FileDiff::hunks`.
    pub hunks: Vec<usize>,
}

/// One card in the Changes tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IntentGroup {
    /// Stable within one computation, for React keys and for naming a group
    /// in a command.
    pub id: String,
    pub kind: GroupKind,
    /// What to show on the card.
    pub label: String,
    /// The symbol this group sits in, when one was identified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub files: Vec<GroupFile>,
    /// Total changed lines across every file.
    pub line_count: u32,
    /// The weakest confidence of any hunk in the group, so a card never looks
    /// more certain than its shakiest member.
    pub confidence: Confidence,
}

impl IntentGroup {
    pub fn hunk_count(&self) -> usize {
        self.files.iter().map(|f| f.hunks.len()).sum()
    }
}

/// Is this hunk purely a change of whitespace?
///
/// Compares the removed lines against the added ones with whitespace ignored.
/// Equal multisets mean the tokens are identical and only their layout moved:
/// a reindent, a reflow, a line-ending change, an import sort, a formatter
/// run. This is the same comparison `git diff --ignore-all-space` makes.
///
/// # Two things deliberately not done
///
/// **Comments are not stripped.** Doing so needs a real lexer per language,
/// and the naive version — treating anything after `//` or `#` as a comment —
/// misreads those characters inside a string literal. That would file a
/// genuine change under "reformatted", hiding it behind a card the reviewer
/// has every reason to skim. A missed formatting hunk costs a few seconds; a
/// hidden real change costs correctness.
///
/// **Indentation is not ignored everywhere.** In Python, YAML and friends,
/// leading whitespace *is* the syntax — moving a line in or out of a block
/// changes what the program does while leaving its tokens identical. For those
/// files the indentation must match exactly before a hunk counts as
/// formatting, which is why this needs the path and not just the hunk.
pub fn is_formatting_only(hunk: &Hunk, path: &str) -> bool {
    let significant = indentation_is_significant(path);

    let mut removed: Vec<String> = Vec::new();
    let mut added: Vec<String> = Vec::new();

    for line in &hunk.lines {
        let normalised = ignore_whitespace(&line.content, significant);
        match line.origin {
            LineOrigin::Deletion => removed.push(normalised),
            LineOrigin::Addition => added.push(normalised),
            LineOrigin::Context => {}
        }
    }

    // A pure insertion or deletion is a real change, however it is spaced.
    if removed.is_empty() || added.is_empty() {
        return false;
    }

    // Blank-line churn alone is still formatting.
    removed.retain(|l| !l.trim().is_empty());
    added.retain(|l| !l.trim().is_empty());
    if removed.is_empty() && added.is_empty() {
        return true;
    }

    removed.sort();
    added.sort();
    removed == added
}

/// Languages where leading whitespace carries meaning.
fn indentation_is_significant(path: &str) -> bool {
    let extension = path.rsplit('.').next().unwrap_or_default().to_lowercase();
    matches!(
        extension.as_str(),
        "py" | "pyi" | "yaml" | "yml" | "sass" | "haml" | "slim" | "coffee" | "nim" | "fs" | "fsx"
    )
}

/// Strip whitespace for comparison, keeping the indent where it is syntax.
fn ignore_whitespace(line: &str, indentation_is_significant: bool) -> String {
    let content = line.trim_end_matches('\r');
    let body: String = content.chars().filter(|c| !c.is_whitespace()).collect();

    if !indentation_is_significant {
        return body;
    }

    let indent = content.len() - content.trim_start().len();
    format!("{indent}:{body}")
}

/// The symbol a hunk sits inside.
///
/// Git writes this into the hunk header itself using its per-language
/// `funcname` patterns, so the common case costs nothing. When the header is
/// empty — an unconfigured language, or a hunk at file scope — the hunk's own
/// lines are scanned for something declaration-shaped.
pub fn enclosing_symbol(hunk: &Hunk) -> Option<String> {
    if let Some(name) = symbol_from_header(&hunk.header) {
        return Some(name);
    }

    hunk.lines
        .iter()
        .filter(|l| l.origin == LineOrigin::Addition)
        .find_map(|l| declaration_name(&l.content))
}

/// Reduce a hunk header to a symbol name.
///
/// The header is a whole line of source — `pub fn thing(a: u32) -> bool {` —
/// so the name is whatever follows the last declaring keyword, before the
/// parameter list.
fn symbol_from_header(header: &str) -> Option<String> {
    let header = header.trim();
    if header.is_empty() {
        return None;
    }
    declaration_name(header).or_else(|| {
        // Not declaration-shaped, but git thought it was the enclosing
        // context, so it is still better than nothing — unless it is one of
        // the lines that names no symbol at all. An import block or a bare
        // statement produces titles like "New import", which is noise on a
        // card; letting the hunk fall through to its file says more.
        if !header_can_name_a_symbol(header) {
            return None;
        }
        let cleaned = header.trim_end_matches(['{', ':']).trim();
        (!cleaned.is_empty() && cleaned.len() <= 80).then(|| cleaned.to_string())
    })
}

/// Can this non-declaration header stand in as a name?
fn header_can_name_a_symbol(header: &str) -> bool {
    if header.contains(';') || header.contains('"') || header.contains('\'') {
        return false;
    }

    let first = header.split_whitespace().next().unwrap_or_default();
    !NOT_A_SYMBOL.contains(&first)
}

/// Did this hunk introduce the symbol, or change one that already existed?
fn symbol_is_new(hunk: &Hunk, symbol: &str) -> bool {
    let declared_in = |origin: LineOrigin| {
        hunk.lines
            .iter()
            .filter(|l| l.origin == origin)
            .any(|l| declaration_name(&l.content).as_deref() == Some(symbol))
    };

    declared_in(LineOrigin::Addition) && !declared_in(LineOrigin::Deletion)
}

/// Build the cards for a working tree.
///
/// `diffs` and `attributions` are parallel: both come from the same scan, in
/// the same order.
pub fn group(diffs: &[FileDiff], attributions: &[FileAttribution]) -> Vec<IntentGroup> {
    // Keyed so hunks of the same kind and symbol merge across files.
    let mut buckets: BTreeMap<String, Bucket> = BTreeMap::new();

    for (file_index, diff) in diffs.iter().enumerate() {
        let attribution = attributions.get(file_index);

        for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
            let changed: Vec<u32> = hunk
                .lines
                .iter()
                .filter(|l| l.origin != LineOrigin::Context)
                .map(|l| l.index)
                .collect();
            if changed.is_empty() {
                continue;
            }

            let hunk_attribution = attribution.and_then(|a| a.hunks.get(hunk_index));

            // 1. The turn that made this hunk, when one turn made most of it.
            //
            // Grouping by turn is worth having on its own — the files really
            // did change together — so this branch is taken whether or not the
            // turn came with a reason. What the reason *is* decides the kind:
            // only a label the agent declared may be presented as a stated
            // intent, and a card with no declared reason is titled from its own
            // changes further down rather than borrowing one.
            let turn = hunk_attribution.and_then(|h| {
                let dominant = h.dominant.as_ref()?;
                let span = h.spans.iter().find(|s| &s.turn_id == dominant)?;
                Some((
                    dominant.clone(),
                    span.label.clone(),
                    span.label_source,
                    span.confidence,
                ))
            });

            let (key, kind, label, symbol, confidence) = match turn {
                Some((turn, Some(label), Some(LabelSource::Declared), confidence)) => (
                    format!("intent:{turn}"),
                    GroupKind::Intent,
                    label,
                    None,
                    confidence,
                ),

                // A sentence mined out of prose. Still the best description
                // available, so it is shown — but as a description of a turn,
                // not as a reason the agent gave.
                Some((turn, Some(label), _, confidence)) => (
                    format!("turn:{turn}"),
                    GroupKind::SameTurn,
                    label,
                    None,
                    confidence,
                ),

                // A turn with no reason at all. The grouping still holds; the
                // title is derived from the changes once the bucket is whole
                // (see `Bucket::derived`), because "2 files" is not knowable
                // from one hunk.
                Some((turn, None, _, _)) => (
                    format!("turn:{turn}"),
                    GroupKind::SameTurn,
                    String::new(),
                    enclosing_symbol(hunk),
                    Confidence::Low,
                ),

                // 2. Formatting, which is decidable rather than guessed.
                None if is_formatting_only(hunk, &diff.path) => (
                    "formatting".to_string(),
                    GroupKind::Formatting,
                    "Whitespace only".to_string(),
                    None,
                    Confidence::High,
                ),

                // 3. Where it sits, as a last resort.
                None => match enclosing_symbol(hunk) {
                    Some(symbol) => {
                        let is_new = symbol_is_new(hunk, &symbol);
                        let kind = if is_new {
                            GroupKind::NewSymbol
                        } else {
                            GroupKind::ModifiedSymbol
                        };
                        // The card's badge already renders New/Changed from
                        // the kind; repeating it in the label just makes the
                        // title longer than the name it is showing.
                        (
                            format!("symbol:{}:{symbol}", kind_key(kind)),
                            kind,
                            symbol.clone(),
                            Some(symbol),
                            Confidence::Low,
                        )
                    }
                    None => (
                        format!("other:{}", diff.path),
                        GroupKind::Other,
                        format!("Other changes in {}", file_name(&diff.path)),
                        None,
                        Confidence::Low,
                    ),
                },
            };

            // A same-turn card with no reason has no title yet — one hunk
            // cannot know how many files the turn touched.
            let needs_title = kind == GroupKind::SameTurn && label.is_empty();

            let bucket = buckets.entry(key.clone()).or_insert_with(|| Bucket {
                id: key,
                kind,
                label,
                symbol: symbol.clone(),
                confidence,
                files: BTreeMap::new(),
                needs_title,
                symbols: BTreeSet::new(),
            });

            // Every symbol the card touches, so a derived title names one only
            // when the whole card sits in the same place. A hunk with no
            // identifiable symbol contributes the empty string, which is what
            // stops a card that is half in `foo` and half nowhere from being
            // titled "foo".
            bucket.symbols.insert(symbol.unwrap_or_default());

            // A card is only as trustworthy as its weakest member.
            bucket.confidence = bucket.confidence.min(confidence);

            let entry = bucket
                .files
                .entry(diff.path.clone())
                .or_insert_with(|| (Vec::new(), Vec::new()));
            entry.0.extend(changed);
            entry.1.push(hunk_index);
        }
    }

    let groups: Vec<IntentGroup> = buckets.into_values().map(Bucket::finish).collect();
    let mut groups = collapse_singletons(groups);

    // Most substantial first, so the biggest decision is the first one read.
    groups.sort_by(|a, b| {
        kind_order(a.kind)
            .cmp(&kind_order(b.kind))
            .then(b.line_count.cmp(&a.line_count))
            .then(a.label.cmp(&b.label))
    });

    groups
}

/// Fold a file's one-hunk symbol cards together.
///
/// Pass 3 names each hunk after the symbol it sits in, which is right when a
/// symbol collects several hunks and wrong when a file is touched in a dozen
/// unrelated places: the result is one card per hunk, which is the pile the
/// grouping exists to remove. So when a *single file* produced two or more
/// cards that are each one hunk of one symbol, they become one card for the
/// file.
///
/// What is deliberately left alone: a card with several hunks, a card spanning
/// several files (a symbol touched in two places is a real grouping), and a
/// file's lone symbol card — its name is a better title than the file's.
/// Nothing is ever merged across files, and intent and formatting cards are
/// not touched at all.
fn collapse_singletons(groups: Vec<IntentGroup>) -> Vec<IntentGroup> {
    let is_singleton = |group: &IntentGroup| {
        matches!(group.kind, GroupKind::NewSymbol | GroupKind::ModifiedSymbol)
            && group.files.len() == 1
            && group.hunk_count() == 1
    };

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for group in groups.iter().filter(|g| is_singleton(g)) {
        *counts.entry(group.files[0].path.as_str()).or_default() += 1;
    }
    let merging: Vec<String> = counts
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .map(|(path, _)| path.to_string())
        .collect();

    if merging.is_empty() {
        return groups;
    }

    let mut kept: Vec<IntentGroup> = Vec::new();
    // path -> index in `kept`, so an id is never handed out twice: staging and
    // reverting look a group up by id, and two cards sharing one would act on
    // each other's lines.
    let mut merged: BTreeMap<String, usize> = BTreeMap::new();

    for group in groups {
        // The file's existing "Other" bucket, if pass 3 made one, is the same
        // card by another name — merge into it rather than beside it.
        let one_file = group.files.len() == 1
            && (is_singleton(&group) || group.kind == GroupKind::Other)
            && merging.contains(&group.files[0].path);

        if !one_file {
            kept.push(group);
            continue;
        }

        let path = group.files[0].path.clone();
        match merged.get(&path) {
            Some(&index) => absorb(&mut kept[index], group),
            None => {
                merged.insert(path.clone(), kept.len());
                kept.push(IntentGroup {
                    id: format!("other:{path}"),
                    kind: GroupKind::Other,
                    label: format!("Several changes in {}", file_name(&path)),
                    symbol: None,
                    files: group.files,
                    line_count: group.line_count,
                    confidence: Confidence::Low,
                });
            }
        }
    }

    kept
}

/// Fold one single-file group's lines into another's.
fn absorb(into: &mut IntentGroup, other: IntentGroup) {
    let source = other.files.into_iter().next().expect("one file");
    let target = &mut into.files[0];

    target.line_indices.extend(source.line_indices);
    target.line_indices.sort_unstable();
    target.line_indices.dedup();
    target.hunks.extend(source.hunks);
    target.hunks.sort_unstable();
    target.hunks.dedup();

    into.line_count = target.line_indices.len() as u32;
    into.confidence = into.confidence.min(other.confidence);
}

/// Ordered by review risk, highest first. This deliberately reverses the older
/// "intent first" rule: the card that carries the *most* risk is the one
/// nothing accounts for — an `Other` hunk is a change no stated intent explains,
/// which is exactly what a reviewer must not skim past — so it leads. A stated
/// intent, having been explained, comes next, and formatting, which changed no
/// code, trails as the one kind that is safe to skim.
fn kind_order(kind: GroupKind) -> u8 {
    match kind {
        GroupKind::Other => 0,
        GroupKind::Intent => 1,
        GroupKind::SameTurn => 2,
        GroupKind::NewSymbol => 3,
        GroupKind::ModifiedSymbol => 4,
        GroupKind::Formatting => 5,
    }
}

fn kind_key(kind: GroupKind) -> &'static str {
    match kind {
        GroupKind::NewSymbol => "new",
        GroupKind::ModifiedSymbol => "modified",
        GroupKind::Intent => "intent",
        GroupKind::SameTurn => "sameTurn",
        GroupKind::Formatting => "formatting",
        GroupKind::Other => "other",
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

struct Bucket {
    id: String,
    kind: GroupKind,
    label: String,
    symbol: Option<String>,
    confidence: Confidence,
    /// path -> (line indices, hunk indices)
    files: BTreeMap<String, (Vec<u32>, Vec<usize>)>,
    /// A same-turn card with no declared reason: `label` is filled in from the
    /// changes themselves once every hunk has been collected.
    needs_title: bool,
    /// Enclosing symbols across the card's hunks, empty string for "none".
    symbols: BTreeSet<String>,
}

impl Bucket {
    /// Title a card that has no reason to show, from what it actually contains.
    ///
    /// Never a reason — only a description. The card exists because those hunks
    /// came from one turn, and that is all it is allowed to say.
    fn derived_title(&self) -> String {
        if self.files.len() > 1 {
            return format!("{} files changed together", self.files.len());
        }

        let single = (self.symbols.len() == 1)
            .then(|| self.symbols.iter().next())
            .flatten()
            .filter(|name| !name.is_empty());

        match (single, self.files.keys().next()) {
            (Some(symbol), _) => symbol.clone(),
            (None, Some(path)) => format!("Changes in {}", file_name(path)),
            // Unreachable: a bucket only exists once a hunk went into it.
            (None, None) => "Changes in one turn".to_string(),
        }
    }

    fn finish(mut self) -> IntentGroup {
        if self.needs_title {
            self.label = self.derived_title();
            self.symbol = (self.symbols.len() == 1)
                .then(|| self.symbols.iter().next().cloned())
                .flatten()
                .filter(|name| !name.is_empty());
        }

        let mut line_count = 0u32;

        let files: Vec<GroupFile> = self
            .files
            .into_iter()
            .map(|(path, (mut lines, mut hunks))| {
                lines.sort_unstable();
                lines.dedup();
                hunks.sort_unstable();
                hunks.dedup();
                line_count += lines.len() as u32;

                GroupFile {
                    path,
                    line_indices: lines,
                    hunks,
                }
            })
            .collect();

        IntentGroup {
            id: self.id,
            kind: self.kind,
            label: self.label,
            symbol: self.symbol,
            files,
            line_count,
            confidence: self.confidence,
        }
    }
}

#[cfg(test)]
#[path = "grouping_tests.rs"]
mod tests;
