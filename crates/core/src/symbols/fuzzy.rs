//! Scoring how well a query matches a candidate symbol name.
//!
//! Pure arithmetic over two strings: subsequence matching with a bonus for hits
//! that land on a camel-hump, a word boundary or the start of the name, so that
//! `dwq` reaches `DoWorkQuietly` ahead of a name that merely happens to contain
//! those letters. No IO, no filesystem, no knowledge of what a symbol is — it
//! is given two strings and returns a number, which is what makes the ranking
//! testable without building an index first.
//!
//! Like everything under [`crate::symbols`], it abstains: a match too weak to
//! be meaningful is reported as no match rather than as a low score, because a
//! palette that always shows ten rows teaches users to ignore the last eight.
//!
//! # Two passes, because almost everything is a miss
//!
//! A palette runs this function against the *entire* index on every keystroke.
//! In a workspace of any size that is tens of thousands of calls per character
//! typed, and the overwhelming majority of them are rejections: `gds` is not a
//! subsequence of nearly anything. So the work is split in two.
//!
//! The first pass is a greedy left-to-right subsequence check. It is O(n) over
//! the candidate, touches each char once, allocates nothing at all, and returns
//! `None` the moment it can prove the query cannot be found. This is the pass
//! that has to be fast, because it is the pass that runs ~100% of the time and
//! decides ~99% of the answers.
//!
//! Only what survives is scored, and scoring is where the expensive, quality
//! part lives: a small dynamic program that considers *every* alignment rather
//! than the greedy one, because the greedy alignment is frequently not the one
//! a human means. Greedy matching of `gds` against `GitDiffService` happens to
//! be right; greedy matching of `test` against `latest_tests` is not.
//!
//! # Why the dynamic program is bounded
//!
//! The DP is O(query × candidate) in both time and memory, and it is applied
//! only when the query is at most [`MAX_QUERY_CHARS`] chars and the candidate
//! at most [`MAX_CANDIDATE_CHARS`]. Beyond those bounds the greedy alignment's
//! score is used instead.
//!
//! The bound is not a performance micro-optimisation, it is a guarantee. An
//! index is built by walking files, and a walked file can contain a minified
//! bundle, a generated table, or a single 100 000-char line. Without a ceiling
//! one pathological candidate would stall the whole palette while the user is
//! still typing. Losing a little ranking quality on a name longer than 128
//! characters costs nothing — nobody is hunting for that symbol by typing an
//! acronym — whereas a hang is fatal. Worst-case cost stays flat and linear.
//!
//! Both alignments are scored and the better one wins. The DP maximises its own
//! objective, which omits the floors applied to the leading and gap penalties
//! (a floor is not decomposable into per-step decisions), so in rare cases the
//! greedy alignment is worth more after flooring. Taking the max of the two is
//! cheaper and more honest than pretending the DP objective is the final score.
//!
//! # Positions are char indices, deliberately
//!
//! [`Match::positions`] holds **char** indices, not byte offsets, and that is
//! the single most important thing to know about this module.
//!
//! The positions exist for one reason: the palette highlights the matched
//! characters in the row it draws. Highlighting means slicing the label, and a
//! label is frequently not ASCII — `café_handler`, a Cyrillic identifier, a
//! path with an accented directory in it. A byte offset handed to a highlighter
//! that counts characters silently paints the wrong letters; handed to Rust's
//! own slicing it panics mid-codepoint. Both failures happen only on the
//! non-English names, which is precisely where they will not be caught.
//!
//! Char indices are a little more expensive to produce and cost the caller a
//! `chars()` walk to consume. That is the correct trade: the alternative is a
//! crash or a lie, on a data set the author of the caller does not have.
//!
//! # Smart case
//!
//! An all-lowercase query is case-insensitive. Any uppercase char in the query
//! is a statement of intent — `GDS` means the humps — and matches only an
//! uppercase char in the candidate. This is the behaviour every editor's search
//! box has, and it is the reason `GDS` finds `GitDiffService` without also
//! finding `goods`.

use std::cmp::{min, Ordering, Reverse};
use std::collections::BinaryHeap;

/// The longest query the dynamic program will consider. Past this the greedy
/// alignment is used; nobody types a 33-char acronym.
pub const MAX_QUERY_CHARS: usize = 32;

/// The longest candidate the dynamic program will consider. See the module
/// docs — this is the ceiling that keeps a generated one-line file from
/// stalling the palette.
pub const MAX_CANDIDATE_CHARS: usize = 128;

/// The whole candidate is the query, ignoring case.
const EXACT_BONUS: i32 = 200;
/// The query is a prefix of the candidate.
const PREFIX_BONUS: i32 = 60;
/// A hit on the very first char.
const START_BONUS: i32 = 40;
/// A hit at the start of a word (see [`starts_a_word`]).
const BOUNDARY_BONUS: i32 = 30;
/// A hit on a camel hump.
const HUMP_BONUS: i32 = 30;
/// A hit immediately after the previous hit.
const ADJACENT_BONUS: i32 = 20;
/// The candidate char has the same case as the query char.
const CASE_BONUS: i32 = 10;
/// Charged per candidate char skipped before the first hit...
const LEADING_PENALTY: i32 = 2;
/// ...but never more than this in total, so a late hit in a long name is not
/// buried beyond recovery.
const LEADING_FLOOR: i32 = 30;
/// Charged per candidate char skipped between two hits...
const GAP_PENALTY: i32 = 1;
/// ...also floored, for the same reason.
const GAP_FLOOR: i32 = 40;
/// One point of length penalty per this many candidate chars: a gentle,
/// monotone preference for the shorter of two otherwise equal names.
const LENGTH_DIVISOR: usize = 8;

/// A successful match: how good it was, and where it landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Higher is better. Comparable only against other scores for the *same*
    /// query — the length penalty and the bonuses are not normalised, and no
    /// meaning attaches to any absolute value.
    pub score: i32,
    /// **Char** indices into the candidate, strictly increasing, one per char
    /// of the query. Empty for an empty query. See the module docs for why
    /// these are not byte offsets.
    pub positions: Vec<usize>,
}

/// Scores `query` against `candidate`, or returns `None` when the query is not
/// a subsequence of the candidate under the smart-case rule.
///
/// An empty query matches everything with score 0 and no positions, so that a
/// palette showing "everything" before the user types is the same code path as
/// a palette showing results.
pub fn score(query: &str, candidate: &str) -> Option<Match> {
    if query.is_empty() {
        return Some(Match {
            score: 0,
            positions: Vec::new(),
        });
    }

    // The rejection pass. Nothing is allocated before it says yes.
    if !is_subsequence(query, candidate) {
        return None;
    }

    let greedy = greedy_hits(query, candidate)?;
    let cand_chars = candidate.chars().count();
    let query_chars = query.chars().count();

    let mut best_score = score_hits(&greedy, cand_chars);
    let mut best_positions: Vec<usize> = greedy.iter().map(|h| h.pos).collect();

    if query_chars <= MAX_QUERY_CHARS && cand_chars <= MAX_CANDIDATE_CHARS {
        let q: Vec<char> = query.chars().collect();
        let c: Vec<char> = candidate.chars().collect();
        if let Some(positions) = best_alignment(&q, &c) {
            let hits = hits_from_positions(&q, &c, &positions);
            let dp_score = score_hits(&hits, cand_chars);
            if dp_score > best_score {
                best_score = dp_score;
                best_positions = positions;
            }
        }
    }

    Some(Match {
        score: best_score,
        positions: best_positions,
    })
}

/// Scores every item and keeps the best `limit` of them, best first.
///
/// `key` names the text each item is matched and ranked on. The ordering is
/// score descending, then a true acronym match (every hit on a word start or a
/// camel hump) ahead of one that is merely a subsequence, then the shorter
/// candidate, then the key ascending. That last step is what makes the result
/// **totally** ordered: two items that tie on everything measurable still come
/// out in the same order on every run, which matters because the palette
/// re-ranks on each keystroke and a list that reshuffles under an unchanged
/// prefix is unusable. A caller wanting "name, then path" as the final
/// tiebreak supplies a key that spells that out.
///
/// Selection is by bounded heap rather than a full sort: the index is large,
/// `limit` is a screenful, and sorting tens of thousands of misses to throw all
/// but twenty away is work nobody asked for.
pub fn rank<T>(
    items: impl Iterator<Item = T>,
    key: impl Fn(&T) -> &str,
    query: &str,
    limit: usize,
) -> Vec<(T, Match)> {
    if limit == 0 {
        return Vec::new();
    }

    let mut heap: BinaryHeap<Reverse<Ranked<T>>> = BinaryHeap::with_capacity(limit + 1);
    for item in items {
        let candidate = key(&item);
        let Some(m) = score(query, candidate) else {
            continue;
        };
        let ranked = Ranked {
            acronym: all_hits_start_a_word(candidate, &m.positions),
            cand_chars: candidate.chars().count(),
            key: candidate.to_string(),
            m,
            item,
        };
        heap.push(Reverse(ranked));
        if heap.len() > limit {
            // The heap is ordered worst-first, so this drops the weakest.
            heap.pop();
        }
    }

    let mut out: Vec<Ranked<T>> = heap.into_iter().map(|r| r.0).collect();
    out.sort_by(|a, b| b.cmp(a));
    out.into_iter().map(|r| (r.item, r.m)).collect()
}

/// An item plus everything the ordering needs, so that comparison never has to
/// re-derive anything from the item.
struct Ranked<T> {
    item: T,
    m: Match,
    acronym: bool,
    cand_chars: usize,
    key: String,
}

impl<T> Ord for Ranked<T> {
    /// `Greater` means "ranks higher". The reversed comparisons are the fields
    /// where smaller is better.
    fn cmp(&self, other: &Self) -> Ordering {
        self.m
            .score
            .cmp(&other.m.score)
            .then_with(|| self.acronym.cmp(&other.acronym))
            .then_with(|| other.cand_chars.cmp(&self.cand_chars))
            .then_with(|| other.key.cmp(&self.key))
    }
}

impl<T> PartialOrd for Ranked<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> PartialEq for Ranked<T> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl<T> Eq for Ranked<T> {}

/// Does this query char accept this candidate char?
///
/// The smart-case rule in one place: an uppercase query char is an explicit
/// demand for an uppercase candidate char, anything else compares case-folded.
/// Folding goes through [`char::to_lowercase`] rather than an ASCII shortcut,
/// because identifiers are not all ASCII and `İ` should not match `i` by
/// accident of byte arithmetic.
fn char_matches(q: char, c: char) -> bool {
    if q.is_uppercase() {
        q == c
    } else {
        q.to_lowercase().eq(c.to_lowercase())
    }
}

/// The rejection pass: is `query` a subsequence of `candidate`?
///
/// Greedy and left-to-right, which is sound for this question — taking the
/// earliest possible match for each query char can never make a later char
/// unmatchable. It is only the *scoring* that greedy gets wrong, not the
/// yes/no. Allocates nothing.
fn is_subsequence(query: &str, candidate: &str) -> bool {
    let mut q = query.chars();
    let mut next = q.next();
    for c in candidate.chars() {
        match next {
            None => return true,
            Some(qc) if char_matches(qc, c) => next = q.next(),
            Some(_) => {}
        }
    }
    next.is_none()
}

/// One matched char, carrying the little context the scorer needs.
///
/// `prev` is the candidate char immediately before the hit, kept here so that
/// scoring never has to index back into the candidate. That is what lets the
/// out-of-bounds path score a 100 000-char candidate without materialising it
/// as a `Vec<char>`.
struct Hit {
    pos: usize,
    ch: char,
    prev: Option<char>,
    q: char,
}

/// The greedy alignment, as hits. Only called once [`is_subsequence`] has said
/// yes, so it cannot fail in practice; the `Option` is there so a future caller
/// cannot skip that check silently.
fn greedy_hits(query: &str, candidate: &str) -> Option<Vec<Hit>> {
    let mut q = query.chars();
    let mut next = q.next();
    let mut hits = Vec::with_capacity(query.chars().count());
    let mut prev: Option<char> = None;

    for (pos, c) in candidate.chars().enumerate() {
        match next {
            None => break,
            Some(qc) if char_matches(qc, c) => {
                hits.push(Hit {
                    pos,
                    ch: c,
                    prev,
                    q: qc,
                });
                next = q.next();
            }
            Some(_) => {}
        }
        prev = Some(c);
    }

    if next.is_some() {
        None
    } else {
        Some(hits)
    }
}

fn hits_from_positions(q: &[char], c: &[char], positions: &[usize]) -> Vec<Hit> {
    positions
        .iter()
        .enumerate()
        .map(|(i, &pos)| Hit {
            pos,
            ch: c[pos],
            prev: pos.checked_sub(1).map(|p| c[p]),
            q: q[i],
        })
        .collect()
}

/// Is a hit here the start of a word?
///
/// Index 0 always is. Otherwise it is the char after one of the separators
/// identifiers are actually built from — `_`, `-`, `.`, `/`, `\` — which covers
/// `snake_case`, `kebab-case`, namespaces and both flavours of path separator,
/// since candidates are frequently `path/to/file.rs` rather than a bare name.
/// A digit followed by a letter counts too (`utf8Decode`, `v2Handler`).
///
/// Space is deliberately absent: a symbol name does not contain one, and a
/// candidate that does is a display string where the palette should not be
/// awarding structural bonuses.
fn starts_a_word(prev: Option<char>, ch: char) -> bool {
    match prev {
        None => true,
        Some(p) => {
            matches!(p, '_' | '-' | '.' | '/' | '\\') || (p.is_ascii_digit() && ch.is_alphabetic())
        }
    }
}

/// Is this an interior capital — the `D` of `GitDiffService`?
fn is_camel_hump(prev: Option<char>, ch: char) -> bool {
    match prev {
        None => false,
        Some(p) => p.is_lowercase() && ch.is_uppercase(),
    }
}

/// The part of a hit's value that does not depend on what came before it in the
/// *query*. Split out so the dynamic program can add it once per cell.
fn base_bonus(hit_pos: usize, prev: Option<char>, ch: char, q: char) -> i32 {
    let mut total = 0;
    if hit_pos == 0 {
        total += START_BONUS;
    }
    if starts_a_word(prev, ch) {
        total += BOUNDARY_BONUS;
    }
    if is_camel_hump(prev, ch) {
        total += HUMP_BONUS;
    }
    if ch == q {
        total += CASE_BONUS;
    }
    total
}

/// The real score of an alignment, floors and all. Both the greedy and the DP
/// alignments come through here, which is what makes them comparable.
fn score_hits(hits: &[Hit], cand_chars: usize) -> i32 {
    let mut total: i32 = 0;
    let mut gap_chars: usize = 0;
    let mut prev_pos: Option<usize> = None;

    for hit in hits {
        total += base_bonus(hit.pos, hit.prev, hit.ch, hit.q);
        match prev_pos {
            None => {
                total -= min(LEADING_PENALTY * hit.pos as i32, LEADING_FLOOR);
            }
            Some(p) if hit.pos == p + 1 => total += ADJACENT_BONUS,
            Some(p) => gap_chars += hit.pos - p - 1,
        }
        prev_pos = Some(hit.pos);
    }

    total -= min(gap_chars as i32 * GAP_PENALTY, GAP_FLOOR);
    total -= (cand_chars / LENGTH_DIVISOR) as i32;

    // Positions are strictly increasing, so a last hit at index len-1 means the
    // whole query sits at the front of the candidate.
    if let Some(last) = hits.last() {
        if last.pos == hits.len() - 1 {
            total += PREFIX_BONUS;
            if cand_chars == hits.len() {
                total += EXACT_BONUS;
            }
        }
    }

    total
}

/// The best-scoring alignment, by dynamic programming over every one of them.
///
/// `dp[i][j]` is the best value of an alignment of the first `i+1` query chars
/// whose last hit is at candidate index `j`. The objective here is *not* the
/// final score: the leading and gap penalties are applied unfloored, because a
/// floor is a property of the whole alignment and cannot be charged one step at
/// a time. [`score`] compensates by scoring the result properly and comparing
/// it against the greedy alignment.
///
/// The inner loop is linear, not quadratic, by carrying `best_carry` — the best
/// `dp[i-1][p] + p` seen so far. The gap penalty from `p` to `j` is
/// `-(j - p - 1)`, so `dp[i-1][p] - (j - p - 1)` is `(dp[i-1][p] + p) - (j - 1)`
/// and the term depending on `p` can be maximised independently of `j`.
///
/// Every tie is broken towards the smaller index, so the result is a pure
/// function of its inputs.
fn best_alignment(q: &[char], c: &[char]) -> Option<Vec<usize>> {
    let (m, n) = (q.len(), c.len());
    if m == 0 || n == 0 || m > n {
        return None;
    }

    let mut dp: Vec<Vec<Option<i32>>> = vec![vec![None; n]; m];
    let mut parent: Vec<Vec<usize>> = vec![vec![0; n]; m];

    for i in 0..m {
        // Best `dp[i-1][p] + p` over every p strictly less than the current j.
        let mut best_carry: Option<(i32, usize)> = None;

        for j in 0..n {
            if i > 0 && j > 0 {
                if let Some(v) = dp[i - 1][j - 1] {
                    let candidate_carry = v + (j - 1) as i32;
                    if best_carry.is_none_or(|(best, _)| candidate_carry > best) {
                        best_carry = Some((candidate_carry, j - 1));
                    }
                }
            }

            if !char_matches(q[i], c[j]) {
                continue;
            }

            let base = base_bonus(j, j.checked_sub(1).map(|p| c[p]), c[j], q[i]);

            if i == 0 {
                dp[0][j] = Some(base - LEADING_PENALTY * j as i32);
                continue;
            }

            let mut best: Option<(i32, usize)> = None;
            if let Some((carry, p)) = best_carry {
                best = Some((carry - (j as i32 - 1) + base, p));
            }
            if j > 0 {
                if let Some(v) = dp[i - 1][j - 1] {
                    let adjacent = v + base + ADJACENT_BONUS;
                    if best.is_none_or(|(value, _)| adjacent > value) {
                        best = Some((adjacent, j - 1));
                    }
                }
            }
            if let Some((value, p)) = best {
                dp[i][j] = Some(value);
                parent[i][j] = p;
            }
        }
    }

    let mut end: Option<(i32, usize)> = None;
    for (j, cell) in dp[m - 1].iter().enumerate() {
        if let Some(v) = *cell {
            if end.is_none_or(|(best, _)| v > best) {
                end = Some((v, j));
            }
        }
    }

    let (_, mut j) = end?;
    let mut positions = vec![0usize; m];
    for i in (0..m).rev() {
        positions[i] = j;
        if i > 0 {
            j = parent[i][j];
        }
    }
    Some(positions)
}

/// Does every hit land on a word start or a camel hump? True of `GDS` against
/// `GitDiffService`, false of `git` against `alignment` — the difference
/// between an acronym the user meant and a subsequence they did not.
///
/// Streams the candidate rather than indexing it, so this stays linear and
/// allocation-free even on the pathological candidates the DP refuses.
fn all_hits_start_a_word(candidate: &str, positions: &[usize]) -> bool {
    let mut wanted = positions.iter().copied();
    let mut next = wanted.next();
    let mut prev: Option<char> = None;

    for (pos, ch) in candidate.chars().enumerate() {
        match next {
            None => return true,
            Some(want) if want == pos => {
                if !starts_a_word(prev, ch) && !is_camel_hump(prev, ch) {
                    return false;
                }
                next = wanted.next();
            }
            Some(_) => {}
        }
        prev = Some(ch);
    }

    next.is_none()
}

#[cfg(test)]
#[path = "fuzzy_tests.rs"]
mod fuzzy_tests;
