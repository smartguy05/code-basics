//! Querying the index: the layer a symbol palette actually talks to.
//!
//! The only module here that combines the others — it takes a query, asks
//! [`crate::symbols::index`] for candidates, ranks them with
//! [`crate::symbols::fuzzy`], and returns a bounded, ordered result. Keeping
//! the combination in one place is what lets the walking, the scoring and the
//! declaration heuristic each be tested on their own terms.
//!
//! # Three kinds of answer, one ranked list
//!
//! A palette answers "open the file called X", "jump to the thing called X" and
//! "run the configuration called X" from the same box, and the user does not
//! tell it which one they meant. So all three are scored against the same
//! query and merged into a single ordering, with [`SearchScope`] there for the
//! case where the user *does* say — a scoped query filters, it does not
//! reweight.
//!
//! Merging three populations of wildly different size (tens of thousands of
//! files and symbols, a dozen configurations) into one list only works because
//! [`crate::symbols::fuzzy`] rejects non-matches outright rather than ranking
//! them badly. A configuration named `Run Api` does not appear underneath four
//! hundred weak file matches; it does not appear at all unless the query is
//! actually a subsequence of its name.
//!
//! # Files are scored on their name, and only then on their path
//!
//! `treelogic` should find `src/components/treeLogic.ts`. It should not find
//! `src/components/tree/logic.ts` first, even though that path also spells the
//! query out — one of those files is *named* what was typed and the other
//! merely contains the letters across a directory boundary.
//!
//! So a file is scored against its file name, and the whole relative path is
//! consulted **only when the name does not match at all**, at a flat
//! [`PATH_FALLBACK_PENALTY`] discount. Ordering "name matches, then path
//! matches" rather than "best score wins over both strings" is deliberate: it
//! makes the rule a property of each candidate on its own, so a file can never
//! be pushed off the end of the list by another file's path match, and it means
//! the discount does not have to be tuned to out-weigh every combination of
//! bonuses the scorer can award.
//!
//! The fallback exists because a name-only search cannot find
//! `src/components/logic.ts` by typing `components` — a real way people look
//! for files, and refusing to answer it would be worse than answering it
//! second.
//!
//! # `positions` index into `label`, or they are empty
//!
//! [`SearchHit::positions`] are char indices into [`SearchHit::label`] and
//! nothing else, because the label is the only string the row is guaranteed to
//! draw. A path-fallback match therefore carries **no** positions rather than
//! positions into a string the label is a suffix of: highlighting the wrong
//! characters is a worse answer than highlighting none, which is the same
//! abstain rule that governs the rest of [`crate::symbols`].
//!
//! # The trailing `:123` is parsed here
//!
//! `Foo:123` means "Foo, line 123", and that convention is implemented once,
//! here, so that the palette's front end has no reason to re-derive it — two
//! implementations of a parse always disagree eventually, and this one decides
//! where the editor jumps. The front end now exists and holds to that: the
//! command passes the query text through verbatim and
//! `src/components/searchLogic.ts` says in its own header that neither the
//! ranking nor the suffix lives there. See [`split_line_suffix`] for exactly
//! what counts.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::model::RunConfig;
use crate::symbols::declarations::SymbolKind;
use crate::symbols::fuzzy;
use crate::symbols::index::SymbolIndex;

/// What a path match costs relative to a name match.
///
/// Small on purpose. It is not trying to out-weigh the scorer — the ordering
/// above already guarantees a named file beats a path-only one, because a file
/// whose name matches is never scored against its path in the first place. This
/// number only separates two *path* matches from two files whose names both
/// missed, and keeps a path match from tying with a name match on some third
/// file at exactly the same score.
pub const PATH_FALLBACK_PENALTY: i32 = 20;

/// Which populations a query is allowed to match.
///
/// A scope filters and never reweights. "Show me only files" is a statement
/// about what the user wants to see, not about how good any particular file
/// is, and mixing the two would make a scoped list order differently from the
/// same rows inside an unscoped one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SearchScope {
    All,
    Files,
    Symbols,
    Actions,
}

/// One request from the palette.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    /// Exactly what the user typed, trailing `:123` and all. Splitting it is
    /// this module's job, not the caller's.
    pub text: String,
    pub scope: SearchScope,
    /// How many hits to return. A screenful; the ranking is bounded to it, so
    /// asking for more is not free.
    pub limit: usize,
}

/// Which of the three questions a hit answers. The UI renders a different row
/// and takes a different action for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum HitKind {
    File,
    Symbol,
    Action,
}

/// One row of the palette.
///
/// Every field is serialised on every hit, including the ones a given kind can
/// never have — an action has no `line` and it crosses as `null`. The
/// alternative, `skip_serializing_if`, makes "this kind has no line" and "the
/// backend did not send a line" the same `undefined` on the TypeScript side,
/// and the second of those is a bug that would then be invisible.
///
/// Both counterparties now exist: `search_everywhere`
/// (`src-tauri/src/commands/symbols.rs`) returns this type, and `SearchHit` in
/// `src/ipc/types.ts` mirrors it by hand. The keys were pinned by
/// `search_hit_serialises_with_the_keys_the_ui_reads` a phase *before* that
/// mirror was written — which is why the mirror could be written against a
/// shape that had not drifted underneath it — and that test now guards a live
/// contract rather than a planned one. A field renamed here and not carried
/// into `types.ts` surfaces as `undefined` in a palette row, not as an error,
/// so the pinning test is the only thing that catches it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub kind: HitKind,
    /// What the row shows, and the *only* string [`SearchHit::positions`]
    /// refer to: a file's name, a symbol's name, a configuration's name.
    pub label: String,
    /// The secondary line: the workspace-relative path for a file or symbol,
    /// the configuration's project for an action.
    pub detail: String,
    /// Workspace-relative, forward slashes, as [`crate::symbols::index`]
    /// records it. `None` for an action, which opens nothing.
    pub path: Option<PathBuf>,
    /// 1-based. A symbol's declaration line, or the line the query named
    /// explicitly, which wins — see [`split_line_suffix`].
    pub line: Option<u32>,
    /// Present only on a symbol hit; the badge the UI draws.
    pub symbol_kind: Option<SymbolKind>,
    /// The `RunConfig` id to launch, present only on an action hit.
    pub action_id: Option<String>,
    /// **Char** indices into [`SearchHit::label`], for highlighting. Empty when
    /// the match was found somewhere the label does not show — see the module
    /// docs.
    pub positions: Vec<u32>,
    /// Comparable only against other hits for the same query.
    pub score: i32,
}

/// Rank everything the query could mean, best first, at most `limit` of them.
///
/// Takes `&[RunConfig]` rather than a `&Workspace` so this module stays free of
/// workspace state — the same reason [`crate::symbols::index::build`] takes
/// `&[Project]`. A query that matches nothing returns an empty vector: there is
/// no error condition here, because "nothing is called that" is an answer.
pub fn search(index: &SymbolIndex, configs: &[RunConfig], query: &Query) -> Vec<SearchHit> {
    if query.limit == 0 {
        return Vec::new();
    }

    // Search All is the palette's default. Give every population a fair first
    // pass so a symbol-heavy workspace cannot push matching files (including
    // Razor views) completely out of the bounded result set.
    if query.scope == SearchScope::All {
        let scopes = [
            SearchScope::Files,
            SearchScope::Symbols,
            SearchScope::Actions,
        ];
        let mut populations: Vec<Vec<SearchHit>> = scopes
            .into_iter()
            .map(|scope| {
                search(
                    index,
                    configs,
                    &Query {
                        text: query.text.clone(),
                        scope,
                        limit: query.limit,
                    },
                )
            })
            .filter(|hits| !hits.is_empty())
            .collect();

        let reserve = query.limit / populations.len().max(1);
        let mut selected = Vec::with_capacity(query.limit);
        let mut remainder = Vec::new();
        for hits in &mut populations {
            let rest = hits.split_off(reserve.min(hits.len()));
            selected.append(hits);
            remainder.extend(rest);
        }
        remainder.sort_by(|a, b| ranked(b).cmp(&ranked(a)));
        selected.extend(remainder.into_iter().take(query.limit - selected.len()));
        selected.sort_by(|a, b| ranked(b).cmp(&ranked(a)));
        return selected;
    }

    let (text, line) = split_line_suffix(&query.text);
    let mut heap: BinaryHeap<Reverse<Ranked>> = BinaryHeap::with_capacity(query.limit + 1);

    if matches!(query.scope, SearchScope::All | SearchScope::Files) {
        for path in &index.files {
            if let Some(hit) = file_hit(path, text, line) {
                offer(&mut heap, hit, query.limit);
            }
        }
    }

    if matches!(query.scope, SearchScope::All | SearchScope::Symbols) {
        for symbol in &index.symbols {
            let Some(m) = fuzzy::score(text, &symbol.name) else {
                continue;
            };
            offer(
                &mut heap,
                SearchHit {
                    kind: HitKind::Symbol,
                    label: symbol.name.clone(),
                    detail: symbol.path.to_string_lossy().into_owned(),
                    path: Some(symbol.path.clone()),
                    // An explicit line is an instruction, not a guess, so it
                    // overrides where the declaration was found.
                    line: Some(line.unwrap_or(symbol.line)),
                    symbol_kind: Some(symbol.kind),
                    action_id: None,
                    positions: positions(&m),
                    score: m.score,
                },
                query.limit,
            );
        }
    }

    if matches!(query.scope, SearchScope::All | SearchScope::Actions) {
        for config in configs {
            let Some(m) = fuzzy::score(text, &config.name) else {
                continue;
            };
            offer(
                &mut heap,
                SearchHit {
                    kind: HitKind::Action,
                    label: config.name.clone(),
                    detail: config
                        .project
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    path: None,
                    // A configuration is not a place in a file, so a line the
                    // user typed has nothing to apply to and is dropped rather
                    // than attached to a row that cannot honour it.
                    line: None,
                    symbol_kind: None,
                    action_id: Some(config.id.clone()),
                    positions: positions(&m),
                    score: m.score,
                },
                query.limit,
            );
        }
    }

    let mut out: Vec<Ranked> = heap.into_iter().map(|r| r.0).collect();
    out.sort_by(|a, b| b.cmp(a));
    out.into_iter().map(|r| r.hit).collect()
}

fn ranked(hit: &SearchHit) -> RankedRef<'_> {
    RankedRef {
        hit,
        label_chars: hit.label.chars().count(),
    }
}

struct RankedRef<'a> {
    hit: &'a SearchHit,
    label_chars: usize,
}

impl Ord for RankedRef<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_rank(self.hit, self.label_chars, other.hit, other.label_chars)
    }
}

impl PartialOrd for RankedRef<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for RankedRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for RankedRef<'_> {}

/// Score one file, name first and path second.
///
/// The two-step is the rule from the module docs in code: the path is only ever
/// looked at when the name did not match, so the two never compete.
fn file_hit(path: &std::path::Path, text: &str, line: Option<u32>) -> Option<SearchHit> {
    let display = path.to_string_lossy().into_owned();
    // A path with no final component cannot come out of the index walk, but it
    // can come out of a deserialised cache, and scoring the whole string is a
    // better answer than skipping the file.
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| display.clone());

    let (score, positions) = match fuzzy::score(text, &name) {
        Some(m) => (m.score, positions(&m)),
        None => {
            let m = fuzzy::score(text, &display)?;
            // Deliberately no positions: they index into the path, and the row
            // shows the name.
            (m.score - PATH_FALLBACK_PENALTY, Vec::new())
        }
    };

    Some(SearchHit {
        kind: HitKind::File,
        label: name,
        detail: display,
        path: Some(path.to_path_buf()),
        line,
        symbol_kind: None,
        action_id: None,
        positions,
        score,
    })
}

fn positions(m: &fuzzy::Match) -> Vec<u32> {
    m.positions.iter().map(|&p| p as u32).collect()
}

/// Push a hit into the bounded heap, dropping the weakest once it is full.
///
/// Selection is by heap rather than by sorting everything, for the reason
/// [`fuzzy::rank`] gives: an empty query matches every candidate in the index,
/// which is the palette's *opening* state, and materialising a sorted list of
/// two hundred thousand symbols to show twenty of them would make the box feel
/// broken before the user has typed anything.
///
/// [`fuzzy::rank`] itself is not reused here because its ordering is over one
/// population with one key, and this list interleaves three with different
/// keys, different penalties and a kind of their own to break ties on.
fn offer(heap: &mut BinaryHeap<Reverse<Ranked>>, hit: SearchHit, limit: usize) {
    heap.push(Reverse(Ranked {
        label_chars: hit.label.chars().count(),
        hit,
    }));
    if heap.len() > limit {
        // Ordered worst-first through `Reverse`, so this drops the weakest.
        heap.pop();
    }
}

/// A hit plus the one derived value the ordering needs, so comparison never
/// walks a string twice.
struct Ranked {
    hit: SearchHit,
    label_chars: usize,
}

impl Ord for Ranked {
    /// `Greater` means "ranks higher".
    ///
    /// Score first, then the shorter label — `run` should reach `run` before
    /// `runAllTheThings`. Then kind, smallest population first: a workspace has
    /// a dozen configurations, a few thousand symbols and tens of thousands of
    /// files, so on an exact tie the curated thing is the one more likely to
    /// have been meant, and burying it under files it ties with would make it
    /// unreachable.
    ///
    /// The remaining steps exist to make the order **total**. The palette
    /// re-ranks on every keystroke, and a list that reshuffles under an
    /// unchanged query is unusable even when every row in it is right — so
    /// every field that could still differ is compared, and two rows that
    /// survive all of them are equal values whose order cannot be observed.
    fn cmp(&self, other: &Self) -> Ordering {
        compare_rank(&self.hit, self.label_chars, &other.hit, other.label_chars)
    }
}

fn compare_rank(a: &SearchHit, a_chars: usize, b: &SearchHit, b_chars: usize) -> Ordering {
    a.score
        .cmp(&b.score)
        .then_with(|| b_chars.cmp(&a_chars))
        .then_with(|| kind_rank(a.kind).cmp(&kind_rank(b.kind)))
        .then_with(|| b.label.cmp(&a.label))
        .then_with(|| b.detail.cmp(&a.detail))
        .then_with(|| b.line.cmp(&a.line))
        .then_with(|| b.action_id.cmp(&a.action_id))
}

impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Ranked {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Ranked {}

/// Higher sorts higher. See [`Ranked::cmp`].
fn kind_rank(kind: HitKind) -> u8 {
    match kind {
        HitKind::Action => 2,
        HitKind::Symbol => 1,
        HitKind::File => 0,
    }
}

/// Split a trailing line reference off the query.
///
/// A suffix is a line reference when it is a `:` followed by nothing but ASCII
/// digits, with something before the colon. That covers both `Foo:123` and the
/// halfway-typed `Foo:` — treating the bare colon as an unfinished line
/// reference rather than as literal text is what keeps the result list from
/// emptying out and refilling between the `:` and the `1`.
///
/// Everything else stays in the text, and that is the abstain rule rather than
/// laziness. `Foo:abc` is not a line reference and is not on its way to being
/// one, so the colon is searched for literally; silently dropping it would run
/// a query the user did not type and present the results as if they had.
///
/// That leaves a second class, kept deliberately separate from it: a suffix
/// that *is* unmistakably a line reference but names no usable line. `:0` is
/// one — a gutter counts from one, so a zero is not a line — and so is a
/// number too large for a `u32`, which a paste or a stuck key produces easily
/// enough. Both consume the suffix and answer `None` for the line, exactly as
/// the unfinished `Foo:` does. Searching those digits as literal text instead
/// would be the harsher failure rather than the more honest one: nothing is
/// named `Foo:5000000000`, so the palette would empty out and the file would
/// look as though it were gone, when what the user got wrong was only the part
/// this function is entitled to discard. Abstaining is about the line, not
/// about the name — the name is still answerable and is still answered.
///
/// Returns a borrow of the input so the common case allocates nothing.
fn split_line_suffix(text: &str) -> (&str, Option<u32>) {
    let Some(colon) = text.rfind(':') else {
        return (text, None);
    };
    if colon == 0 {
        // `:42` alone names a line in a file the palette has not been told
        // about. There is nothing to apply it to, so it is text.
        return (text, None);
    }

    let (head, digits) = (&text[..colon], &text[colon + 1..]);
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return (text, None);
    }
    match digits.parse::<u32>() {
        Ok(line) if line > 0 => (head, Some(line)),
        // The suffix was digits, so it was a line reference; it just does not
        // name a line. Zero, empty (`Foo:`) and an overflowing number are the
        // only three ways to get here, and all three are the same answer.
        _ => (head, None),
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod search_tests;
