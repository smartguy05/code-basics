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
use crate::intents::{Intents, LabelSource, SelfConfidence};
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
    /// What to show on the card. Empty for an ambiguous intent card, where the
    /// reasons live in `candidates` instead.
    pub label: String,
    /// When several declared reasons scope this file and none could be bound
    /// uniquely, every candidate reason — so the card shows them rather than
    /// silently dropping the author's intent. Empty in the normal single-reason
    /// case (the one reason is in `label`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
    /// The symbol this group sits in, when one was identified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub files: Vec<GroupFile>,
    /// Total changed lines across every file.
    pub line_count: u32,
    /// The weakest confidence of any hunk in the group, so a card never looks
    /// more certain than its shakiest member.
    pub confidence: Confidence,
    /// How sure the agent said it was that this change is correct, when it
    /// appended a `[confidence: …]` token to the declared `Intent:` line.
    ///
    /// **Distinct from `confidence` above.** That measures how well the matcher
    /// tied a recorded edit onto the diff — the tooling's trust in the match.
    /// This is the agent's own stated belief about the code, offered
    /// voluntarily and therefore usually absent. Only a *declared* reason can
    /// carry one; an inferred or derived title never does. Omitted from the
    /// wire when absent, so the UI reads it only when the agent spoke.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_confidence: Option<SelfConfidence>,
    /// The label is a note the *user* wrote (see [`crate::intents::user`]),
    /// rather than a recorded or inferred agent reason. The UI uses this to
    /// distinguish editing your own note from overwriting an agent's intent.
    /// Omitted from the wire when false, like the other optional card fields.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub user_authored: bool,
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

    // For each changed line, keep both the raw characters and the
    // whitespace-normalised form. The raw form is what tells a line that only
    // *moved* — identical characters in a new position — apart from one whose
    // whitespace genuinely changed. Reordering statements changes what the
    // program does, so only the second is formatting.
    let mut removed: Vec<(String, String)> = Vec::new();
    let mut added: Vec<(String, String)> = Vec::new();

    for line in &hunk.lines {
        let normalised = ignore_whitespace(&line.content, significant);
        // The raw line, verbatim: any real whitespace change (a reindent, a
        // collapsed run of spaces, a stripped `\r`) alters it, while a pure
        // relocation leaves it byte-for-byte identical.
        let raw = line.content.clone();
        match line.origin {
            LineOrigin::Deletion => removed.push((raw, normalised)),
            LineOrigin::Addition => added.push((raw, normalised)),
            LineOrigin::Context => {}
        }
    }

    // A pure insertion or deletion is a real change, however it is spaced.
    if removed.is_empty() || added.is_empty() {
        return false;
    }

    // Blank-line churn alone is still formatting.
    removed.retain(|(_, n)| !n.trim().is_empty());
    added.retain(|(_, n)| !n.trim().is_empty());
    if removed.is_empty() && added.is_empty() {
        return true;
    }

    // The normalised content must match as a multiset, or real code changed.
    let mut removed_norm: Vec<&String> = removed.iter().map(|(_, n)| n).collect();
    let mut added_norm: Vec<&String> = added.iter().map(|(_, n)| n).collect();
    removed_norm.sort();
    added_norm.sort();
    if removed_norm != added_norm {
        return false;
    }

    // The normalised content matches — but so it would for a pure reorder,
    // where identical lines merely swapped places. Tell the two apart by the
    // raw characters: if the raw multiset is unchanged too, nothing's
    // whitespace actually changed and the lines only moved. A relocation is a
    // logic change (an assignment now runs before an await, an early return
    // moves), not formatting, so refuse the label.
    let mut removed_raw: Vec<&String> = removed.iter().map(|(r, _)| r).collect();
    let mut added_raw: Vec<&String> = added.iter().map(|(r, _)| r).collect();
    removed_raw.sort();
    added_raw.sort();
    removed_raw != added_raw
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

/// The changed-line *content* of one file, restricted to a set of diff line
/// indices — the geometry a user annotation stores so it can rebind to the diff
/// by content after the lines move (see [`crate::intents::user`]).
///
/// Returns `(removed, added)` in diff order. Context lines are skipped: only the
/// lines the card actually claims are the annotation's evidence.
pub fn changed_content(
    diff: &FileDiff,
    line_indices: &std::collections::BTreeSet<u32>,
) -> (Vec<String>, Vec<String>) {
    let mut removed = Vec::new();
    let mut added = Vec::new();
    for hunk in &diff.hunks {
        for line in &hunk.lines {
            if !line_indices.contains(&line.index) {
                continue;
            }
            match line.origin {
                LineOrigin::Deletion => removed.push(line.content.clone()),
                LineOrigin::Addition => added.push(line.content.clone()),
                LineOrigin::Context => {}
            }
        }
    }
    (removed, added)
}

/// Lower is more cautious, so a group gathering several stated confidences
/// keeps the one that most asks for review.
fn caution_rank(confidence: SelfConfidence) -> u8 {
    match confidence {
        SelfConfidence::Low => 0,
        SelfConfidence::Medium => 1,
        SelfConfidence::High => 2,
    }
}

/// The more cautious of two stated confidences, treating absence as "no
/// statement" rather than as a value: a claim that named a level always wins
/// over one that named none, and between two levels the lower (more
/// review-worthy) wins. Per declared claim this is 1:1, so it only ever
/// combines identical values; the rule exists for the defensive case.
fn more_cautious(a: Option<SelfConfidence>, b: Option<SelfConfidence>) -> Option<SelfConfidence> {
    match (a, b) {
        (Some(x), Some(y)) if caution_rank(y) < caution_rank(x) => Some(y),
        (Some(x), _) => Some(x),
        (None, b) => b,
    }
}

/// Build the cards for a working tree.
///
/// `diffs` and `attributions` are parallel: both come from the same scan, in
/// the same order.
pub fn group(
    diffs: &[FileDiff],
    attributions: &[FileAttribution],
    intents: &Intents,
) -> Vec<IntentGroup> {
    // Keyed so hunks of the same kind and symbol merge across files.
    let mut buckets: BTreeMap<String, Bucket> = BTreeMap::new();

    // The agent's *own* stated confidence for each declared reason, keyed by the
    // reason's identity so an intent card can carry it however the card came to
    // exist (a matched span, a same-turn dominant, or a file the reason merely
    // scopes). Only declared labels contribute — a self-confidence is something
    // the agent wrote, never something mined from prose.
    let mut declared_confidence: BTreeMap<(String, String), Option<SelfConfidence>> =
        BTreeMap::new();
    for label in &intents.labels {
        if label.source != LabelSource::Declared {
            continue;
        }
        let key = (label.turn_id.clone(), label.label.clone());
        let entry = declared_confidence.entry(key).or_insert(None);
        *entry = more_cautious(*entry, label.self_confidence);
    }
    let confidence_for = |turn: &str, label: &str| -> Option<SelfConfidence> {
        declared_confidence
            .get(&(turn.to_string(), label.to_string()))
            .copied()
            .flatten()
    };

    // Declared intents that attribution actually bound to changed lines
    // *somewhere* in this diff, keyed by the reason's identity. A reason that is
    // demonstrably real — it has its own evidenced card — must never also be
    // offered as a mere "candidate" on an ambiguous card for an unbound hunk it
    // happens to scope: that presents a known intent as a guess and duplicates
    // it. Such reasons are filtered out of `covering` below.
    let evidenced_intents: BTreeSet<(String, String)> = attributions
        .iter()
        .flat_map(|fa| &fa.hunks)
        .flat_map(|h| &h.spans)
        .filter_map(|s| {
            if s.label_source != Some(LabelSource::Declared) {
                return None;
            }
            let turn = s.label_turn_id.clone().unwrap_or_else(|| s.turn_id.clone());
            Some((turn, s.label.clone()?))
        })
        .collect();

    for (file_index, diff) in diffs.iter().enumerate() {
        // Declared reasons the author scoped to this file. Used only when the
        // geometry did not already bind one: a single reason titles the file's
        // card, several become its candidates. Computed once per file.
        //
        // Reasons already evidenced elsewhere are dropped: they have real cards,
        // so listing them as a "maybe" here would duplicate a known intent.
        let covering: Vec<(String, String)> = intents
            .scoped_labels_for_path(&diff.path)
            .into_iter()
            .map(|l| (l.turn_id.clone(), l.label.clone()))
            .filter(|pair| !evidenced_intents.contains(pair))
            .collect();

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

            // Evidenced split. When attribution tied lines in this one hunk to
            // two or more *distinct declared intents*, each intent becomes its
            // own card carrying only the lines the matcher actually gave it.
            //
            // This is the one place a hunk's lines are divided across cards, and
            // it is safe precisely because it is driven by per-line evidence:
            // staging and reverting already act on a card's `line_indices`, not
            // on whole hunks. Without it, such a hunk either lands wholly on its
            // dominant intent (absorbing the others' lines) or — with no
            // majority — falls through to a location card while every intent
            // shows as unmatched. A hunk merely *scoped* by several declared
            // reasons with no matched geometry has no per-line evidence and is
            // left to the covering path below, which abstains to one card.
            if let Some(hunk_attr) = hunk_attribution {
                let changed_set: BTreeSet<u32> = changed.iter().copied().collect();
                // Declared intents in this hunk's spans, keyed by the reason's
                // identity (the turn that declared it) so the same intent merges
                // across hunks and files, each with the lines it claimed here.
                let mut per_intent: BTreeMap<(String, String), (Vec<u32>, Confidence)> =
                    BTreeMap::new();
                let mut claimed: BTreeSet<u32> = BTreeSet::new();
                for span in &hunk_attr.spans {
                    let (Some(label), Some(LabelSource::Declared)) =
                        (span.label.as_ref(), span.label_source)
                    else {
                        continue;
                    };
                    let turn = span
                        .label_turn_id
                        .clone()
                        .unwrap_or_else(|| span.turn_id.clone());
                    let lines: Vec<u32> = span
                        .line_indices
                        .iter()
                        .copied()
                        .filter(|i| changed_set.contains(i))
                        .collect();
                    if lines.is_empty() {
                        continue;
                    }
                    claimed.extend(lines.iter().copied());
                    let entry = per_intent
                        .entry((turn, label.clone()))
                        .or_insert_with(|| (Vec::new(), Confidence::High));
                    entry.0.extend(lines);
                    entry.1 = entry.1.min(span.confidence);
                }

                if per_intent.len() >= 2 {
                    for ((turn, label), (mut lines, confidence)) in per_intent {
                        lines.sort_unstable();
                        lines.dedup();
                        let self_confidence = confidence_for(&turn, &label);
                        let key = intent_key(&turn, &label);
                        let bucket = buckets.entry(key.clone()).or_insert_with(|| Bucket {
                            id: key,
                            kind: GroupKind::Intent,
                            label: label.clone(),
                            candidates: Vec::new(),
                            symbol: None,
                            confidence,
                            self_confidence,
                            files: BTreeMap::new(),
                            needs_title: false,
                            symbols: BTreeSet::new(),
                        });
                        bucket.symbols.insert(String::new());
                        bucket.confidence = bucket.confidence.min(confidence);
                        bucket.self_confidence =
                            more_cautious(bucket.self_confidence, self_confidence);
                        let entry = bucket
                            .files
                            .entry(diff.path.clone())
                            .or_insert_with(|| (Vec::new(), Vec::new()));
                        entry.0.extend(lines);
                        if !entry.1.contains(&hunk_index) {
                            entry.1.push(hunk_index);
                        }
                    }

                    // Lines the matcher tied to no declared intent are genuinely
                    // unexplained here: the stated intents are already their own
                    // cards, so re-attaching these to one would claim lines it
                    // has no evidence for. Group them by where they sit instead.
                    let remainder: Vec<u32> = changed
                        .iter()
                        .copied()
                        .filter(|i| !claimed.contains(i))
                        .collect();
                    if !remainder.is_empty() {
                        let (key, kind, label, symbol) = match enclosing_symbol(hunk) {
                            Some(symbol) => {
                                let kind = if symbol_is_new(hunk, &symbol) {
                                    GroupKind::NewSymbol
                                } else {
                                    GroupKind::ModifiedSymbol
                                };
                                (
                                    format!("symbol:{}:{symbol}", kind_key(kind)),
                                    kind,
                                    symbol.clone(),
                                    Some(symbol),
                                )
                            }
                            None => (
                                format!("other:{}", diff.path),
                                GroupKind::Other,
                                format!("Other changes in {}", file_name(&diff.path)),
                                None,
                            ),
                        };
                        let bucket = buckets.entry(key.clone()).or_insert_with(|| Bucket {
                            id: key,
                            kind,
                            label,
                            candidates: Vec::new(),
                            symbol: symbol.clone(),
                            confidence: Confidence::Low,
                            self_confidence: None,
                            files: BTreeMap::new(),
                            needs_title: false,
                            symbols: BTreeSet::new(),
                        });
                        bucket.symbols.insert(symbol.unwrap_or_default());
                        bucket.confidence = bucket.confidence.min(Confidence::Low);
                        let entry = bucket
                            .files
                            .entry(diff.path.clone())
                            .or_insert_with(|| (Vec::new(), Vec::new()));
                        entry.0.extend(remainder);
                        if !entry.1.contains(&hunk_index) {
                            entry.1.push(hunk_index);
                        }
                    }

                    continue;
                }
            }

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
                    span.label_turn_id.clone(),
                    span.confidence,
                ))
            });

            let (key, kind, label, symbol, confidence, self_confidence) = match turn {
                // A declared reason. Keyed by the *label's* identity, not the
                // turn that made the edit: one orphan geometry turn can carry
                // two different declared labels (each scoped to different
                // files), and those are two intents, not one card.
                Some((turn, Some(label), Some(LabelSource::Declared), label_turn, confidence)) => {
                    let declaring_turn = label_turn.unwrap_or(turn);
                    let self_confidence = confidence_for(&declaring_turn, &label);
                    (
                        intent_key(&declaring_turn, &label),
                        GroupKind::Intent,
                        label,
                        None,
                        confidence,
                        self_confidence,
                    )
                }

                // A sentence mined out of prose. Still the best description
                // available, so it is shown — but as a description of a turn,
                // not as a reason the agent gave. A mined sentence never carries
                // a self-confidence, so this abstains.
                Some((turn, Some(label), _, _, confidence)) => (
                    format!("turn:{turn}"),
                    GroupKind::SameTurn,
                    label,
                    None,
                    confidence,
                    None,
                ),

                // A turn with no reason at all. The grouping still holds; the
                // title is derived from the changes once the bucket is whole
                // (see `Bucket::derived`), because "2 files" is not knowable
                // from one hunk.
                Some((turn, None, _, _, _)) => (
                    format!("turn:{turn}"),
                    GroupKind::SameTurn,
                    String::new(),
                    enclosing_symbol(hunk),
                    Confidence::Low,
                    None,
                ),

                // 2. Formatting, which is decidable rather than guessed.
                None if is_formatting_only(hunk, &diff.path) => (
                    "formatting".to_string(),
                    GroupKind::Formatting,
                    "Whitespace only".to_string(),
                    None,
                    Confidence::High,
                    None,
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
                            None,
                        )
                    }
                    None => (
                        format!("other:{}", diff.path),
                        GroupKind::Other,
                        format!("Other changes in {}", file_name(&diff.path)),
                        None,
                        Confidence::Low,
                        None,
                    ),
                },
            };

            // Surface a declared reason the geometry did not bind. When a hunk
            // landed as anything but a bound intent (or decidable formatting)
            // yet the author scoped a declared reason to this file, retitle it:
            // one reason titles the card, several become candidates rather than
            // being dropped to a bare symbol name. Confidence is Low — the
            // reason is stated, not corroborated by matched geometry.
            if !matches!(kind, GroupKind::Intent | GroupKind::Formatting) && covering.len() > 1 {
                // Several real intents can plausibly own the same changed
                // lines. Keep one card per intent and repeat only those
                // genuinely ambiguous lines in each card; a synthetic
                // multi-intent card makes the reviewer decode several goals at
                // once and defeats the intent-first review model.
                for (turn_id, reason) in &covering {
                    let key = intent_key(turn_id, reason);
                    let bucket = buckets.entry(key.clone()).or_insert_with(|| Bucket {
                        id: key,
                        kind: GroupKind::Intent,
                        label: reason.clone(),
                        candidates: Vec::new(),
                        symbol: None,
                        confidence: Confidence::Low,
                        self_confidence: confidence_for(turn_id, reason),
                        files: BTreeMap::new(),
                        needs_title: false,
                        symbols: BTreeSet::new(),
                    });
                    bucket.symbols.insert(String::new());
                    let entry = bucket
                        .files
                        .entry(diff.path.clone())
                        .or_insert_with(|| (Vec::new(), Vec::new()));
                    entry.0.extend(changed.iter().copied());
                    if !entry.1.contains(&hunk_index) {
                        entry.1.push(hunk_index);
                    }
                }
                continue;
            }

            let (key, kind, label, symbol, confidence, candidates, self_confidence) =
                if matches!(kind, GroupKind::Intent | GroupKind::Formatting) || covering.is_empty()
                {
                    (
                        key,
                        kind,
                        label,
                        symbol,
                        confidence,
                        Vec::new(),
                        self_confidence,
                    )
                } else if covering.len() == 1 {
                    let (turn_id, reason) = &covering[0];
                    (
                        intent_key(turn_id, reason),
                        GroupKind::Intent,
                        reason.clone(),
                        None,
                        Confidence::Low,
                        Vec::new(),
                        confidence_for(turn_id, reason),
                    )
                } else {
                    // Several declared reasons scope this file and none bound
                    // uniquely. The card abstains from a single title, so it
                    // abstains from a single confidence too — no one claim owns
                    // it.
                    let reasons: Vec<String> = covering.iter().map(|(_, r)| r.clone()).collect();
                    (
                        format!("intent-ambiguous:{}", reasons.join("\u{1f}")),
                        GroupKind::Intent,
                        String::new(),
                        None,
                        Confidence::Low,
                        reasons,
                        None,
                    )
                };

            // A same-turn card with no reason has no title yet — one hunk
            // cannot know how many files the turn touched.
            let needs_title = kind == GroupKind::SameTurn && label.is_empty();

            let bucket = buckets.entry(key.clone()).or_insert_with(|| Bucket {
                id: key,
                kind,
                label,
                candidates,
                symbol: symbol.clone(),
                confidence,
                self_confidence,
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
            // And keeps the most cautious stated confidence any hunk carried.
            bucket.self_confidence = more_cautious(bucket.self_confidence, self_confidence);

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
                    candidates: Vec::new(),
                    symbol: None,
                    files: group.files,
                    line_count: group.line_count,
                    confidence: Confidence::Low,
                    // A "several changes" merge of location cards is not a stated
                    // intent, so it carries no self-confidence.
                    self_confidence: None,
                    user_authored: false,
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
/// Agent turns that repeat the exact same declared intent describe one review
/// goal, even when the work spans several turns. User notes retain their own
/// stable identity because two deliberate cards may intentionally share a
/// title without being the same group.
fn intent_key(turn: &str, label: &str) -> String {
    if turn.starts_with("usernote:") {
        format!("intent:{turn}:{label}")
    } else {
        format!("intent:label:{label}")
    }
}

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
    /// Candidate reasons for an ambiguous intent card; empty otherwise.
    candidates: Vec<String>,
    symbol: Option<String>,
    confidence: Confidence,
    /// The agent's stated confidence for this card's declared reason, when it
    /// gave one. Combined with [`more_cautious`] as spans merge.
    self_confidence: Option<SelfConfidence>,
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

        // A user note becomes a declared-intent bucket keyed
        // `intent:usernote:{id}:{label}` (see [`crate::intents::user`]); the
        // prefix is distinctive, so recognising it here needs no extra field
        // threaded through every bucket.
        let user_authored = self.id.starts_with("intent:usernote:");

        IntentGroup {
            id: self.id,
            kind: self.kind,
            label: self.label,
            candidates: self.candidates,
            symbol: self.symbol,
            files,
            line_count,
            confidence: self.confidence,
            self_confidence: self.self_confidence,
            user_authored,
        }
    }
}

#[cfg(test)]
#[path = "grouping_tests.rs"]
mod tests;
