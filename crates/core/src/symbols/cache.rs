//! Persisting a built index so opening a workspace does not re-read every file.
//!
//! Lives under `.code-basics/` with the rest of the per-workspace state. The
//! cache is a pure optimisation and is treated as such: anything about it that
//! cannot be trusted — a missing file, a version it does not recognise, a
//! mismatched fingerprint — means rebuilding from source, never guessing that
//! stale contents are close enough. A palette that jumps to a line that moved
//! is exactly the wrong answer this crate refuses to give.
//!
//! # Why this exists at all
//!
//! It was gated on a measurement rather than assumed, because a cache that
//! saves nothing is pure liability: another file to invalidate, another way to
//! be wrong. Timed over this repository — 315 files, ~3300 symbols — a full
//! [`crate::symbols::index::build`] takes about 20 ms in a release build, and a
//! cache there would buy nothing anyone could perceive. Timed over the .NET
//! solution this application was written for — 2864 files, ~16600 symbols — the
//! same build takes 680–750 ms warm and over nine seconds against a cold
//! filesystem cache. That is the number that justifies the file below: three
//! quarters of a second of dead UI every time a workspace is opened, on the
//! kind of repository the tool is actually pointed at.
//!
//! # The walk stays the authority
//!
//! [`build_cached`] runs [`crate::symbols::index::walk`] — the same walk a cold
//! build runs, called rather than reimplemented, so the two cannot come to
//! differ about which files a workspace consists of — and consults the cache
//! only for files that pass through it. A cache entry is
//! never a source of truth about *which* files exist — only about what a file
//! that the walk already yielded was found to declare. That ordering matters:
//! the alternative, trusting the entry list, would let a file that has since
//! been deleted, gitignored or moved into build output go on appearing in the
//! palette until something else happened to invalidate it, and a palette entry
//! that opens nothing is precisely the wrong answer.
//!
//! # What the fingerprint can and cannot see
//!
//! An entry survives only when the file is still there and *both* its
//! modification time and its length match what was recorded. Content is not
//! hashed. Hashing would mean reading every file, which is the cost the cache
//! exists to avoid — the fingerprint has to be answerable from `stat` alone or
//! it is not a fingerprint, it is a parse.
//!
//! That leaves a real hole, and it is worth naming rather than burying:
//!
//! * **`mtime` is second-granularity here, and it trusts the clock.** A file
//!   rewritten twice inside the same second, to the same length, by something
//!   other than a human typing — a code generator, a `git checkout` of a branch
//!   that differs by a same-length line, a formatter run in a loop — can keep a
//!   fingerprint that no longer describes its contents, and its symbols will
//!   stay stale until it is next touched. A clock stepped backwards (a VM
//!   resume, an NTP correction) can do the same.
//! * The escape hatch is [`rebuild`], which ignores whatever is on disk and
//!   re-reads the workspace from source. Anything that suspects the index —
//!   a user asking for it, or a caller that has just done something wholesale
//!   like switching branches — should call that rather than trying to reason
//!   about which entries might have gone bad.
//!
//! Second-granularity was chosen over the nanosecond precision the platform
//! sometimes offers because that precision is not portable and, worse, is not
//! stable across a copy: restoring a workspace from a backup, or moving it
//! between filesystems, rewrites the sub-second part and would invalidate
//! everything for no reason. A cache that is discarded whenever it would have
//! helped most is not better than the narrow staleness window above.
//!
//! # What the fingerprint deliberately does not describe: ownership
//!
//! [`crate::symbols::index::Symbol::project_id`] is the one field on a stored
//! symbol that is **not** a property of the file it came from. It is derived
//! from the project list passed to [`build_cached`], which is an input the
//! per-file fingerprint above cannot see and which changes for reasons that
//! never touch a single source file — a `.csproj` added, a project folder
//! renamed, a workspace rescan finding something it had missed. A file whose
//! mtime and length are entirely honest can therefore still be carrying an
//! answer that was correct for a different question, and nothing short of
//! [`rebuild`] would ever have corrected it.
//!
//! So the project list is fingerprinted too, at the top of the file, and any
//! difference discards the whole cache. See [`SymbolCache::projects`] for why
//! that rather than re-attributing entries as they are loaded.
//!
//! # Versioning
//!
//! Two numbers, because two different things can invalidate a cache and
//! conflating them would mean bumping one for the other's reasons.
//! [`CACHE_VERSION`] covers the shape of this file. [`HEURISTIC_VERSION`]
//! covers the *meaning* of what is stored: anyone who edits the declaring-word
//! table or the kind mapping in [`crate::symbols::declarations`] changes what
//! a given line yields, and every symbol recorded by the previous rules is now
//! a claim nobody has verified. Either mismatch discards the whole file rather
//! than trying to salvage entries from it — a partially-reinterpreted index is
//! a worse artefact than no index.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::thread;

use serde::{Deserialize, Serialize};

use crate::model::Project;
use crate::symbols::index::{Limits, Symbol, SymbolIndex};

/// The cache file, inside `.code-basics/` with the rest of the per-workspace
/// state. Listed in `config::IGNORED` — it is one machine's derived data and
/// has no business in a shared history.
pub const CACHE_FILE: &str = "symbols.json";

/// The layout of the file below. Bump when the shape changes.
///
/// Version 2 added [`SymbolCache::projects`]. A version 1 file would in fact be
/// rejected anyway — it has no `projects` key, so it fails to deserialise and
/// [`load`] collapses that to "no cache" — but relying on a parse failure to
/// enforce a shape change would mean the next field that happens to be
/// `#[serde(default)]`-able slipped through silently.
pub const CACHE_VERSION: u32 = 2;

/// The version of the declaration heuristic whose output is stored here.
///
/// **Bump this when you change `declarations::DECLARING` or the kind table.**
/// Nothing enforces that, and nothing can: the connection between a word list
/// and a cached result is semantic. Forgetting means users keep symbols the
/// current rules would never produce, indefinitely, with no error to point at.
pub const HEURISTIC_VERSION: u32 = 5;

/// A whole workspace's worth of remembered parses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SymbolCache {
    pub version: u32,
    pub heuristic_version: u32,
    /// The project list the entries below were attributed against, in the
    /// order it was given. Any difference discards the whole cache.
    ///
    /// # Why invalidate rather than re-attribute
    ///
    /// The alternative is to store symbols without a `project_id` and recompute
    /// it on load, which would make the equivalence with a cold build true by
    /// construction instead of merely defended, and would keep every entry
    /// across a project being added. It was not taken, for two reasons.
    ///
    /// The first is that it depends on a property nothing enforces: that
    /// ownership is a pure function of `(path, projects)` and of nothing else.
    /// That is true of today's longest-component-prefix rule, but it is a rule
    /// living in another module which is free to start consulting the file — an
    /// `<AssemblyName>`, a `package.json` `workspaces` entry — at which point
    /// re-attribution on load would be recomputing an answer from half its
    /// inputs and would be wrong in exactly the silent way this module exists to
    /// avoid. Discarding the cache depends on nothing.
    ///
    /// The second is cost. The list changes only when a manifest is added,
    /// removed or moved, which is already an expensive event — it means a
    /// workspace rescan — and it is not something a user does in a loop. The
    /// price of being wrong here is a mislabelled palette that never corrects
    /// itself; the price of being conservative is one rebuild, on the order of
    /// the three quarters of a second quoted in the module note, on an action
    /// that happens a handful of times in a session.
    ///
    /// Order is preserved rather than normalised because ownership can depend
    /// on it: two projects declaring the same directory are separated only by
    /// the stable sort in `index`'s owner table, so reordering the input can
    /// reorder the answer. Sorting here would hide that. The cost is a spurious
    /// rebuild if the scan ever returns the same projects in a different order,
    /// which is an over-invalidation — the safe direction.
    pub projects: Vec<CachedProject>,
    pub entries: Vec<CacheEntry>,
}

/// The part of a [`Project`] that can change what a symbol is attributed to.
///
/// Only the identity and the directory, because those are the only two fields
/// the owner table is built from. A project's frameworks, configurations or
/// test runner changing cannot move a single symbol, and folding them in would
/// throw the index away for edits that could not possibly have affected it.
///
/// `dir` is stored exactly as the [`Project`] carried it — absolute, in the
/// platform's own spelling — and compared verbatim. That means moving a
/// workspace on disk discards the cache; that is an over-invalidation costing
/// one rebuild, and it is preferable to relativising here and thereby encoding
/// a second copy of the rule about which projects the owner table keeps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedProject {
    pub id: String,
    pub dir: PathBuf,
}

/// The ownership fingerprint of a project list.
///
/// A plain projection rather than a hash: the list is a few dozen short strings
/// at most, comparing it directly cannot collide, and a human looking at
/// `symbols.json` to work out why their cache keeps being discarded can read
/// the answer instead of a number.
fn project_fingerprint(projects: &[Project]) -> Vec<CachedProject> {
    projects
        .iter()
        .map(|p| CachedProject {
            id: p.id.clone(),
            dir: p.dir.clone(),
        })
        .collect()
}

/// What one file was found to declare, and the fingerprint that says whether
/// the finding still applies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    /// Workspace-relative with forward slashes, exactly as [`Symbol::path`] is
    /// written — this is the key the walk is matched against, so a
    /// platform-shaped separator here would miss on the machine that wrote it.
    pub path: PathBuf,
    /// Seconds since the Unix epoch. See the module note on what this can miss.
    pub mtime_secs: u64,
    pub len: u64,
    pub symbols: Vec<Symbol>,
}

pub fn cache_path(root: &Path) -> PathBuf {
    crate::config::config_dir(root).join(CACHE_FILE)
}

/// Read the cache, or an empty one.
///
/// Every failure is the same answer. Absent, unreadable, truncated mid-write,
/// hand-edited into invalid JSON, written by a future version — all of it
/// collapses to "there is no usable cache", which costs a full build and
/// nothing else. Returning a `Result` here would push a decision onto callers
/// that has exactly one correct outcome, and would tempt one of them into
/// surfacing an error about a file the user never asked to exist.
pub fn load(root: &Path) -> SymbolCache {
    let Ok(text) = std::fs::read_to_string(cache_path(root)) else {
        return SymbolCache::default();
    };
    let Ok(cache) = serde_json::from_str::<SymbolCache>(&text) else {
        return SymbolCache::default();
    };
    if cache.version != CACHE_VERSION || cache.heuristic_version != HEURISTIC_VERSION {
        return SymbolCache::default();
    }
    cache
}

/// Write the cache, creating `.code-basics/` and its ignore file if needed.
///
/// The ignore file is ensured on the same call that writes the cache rather
/// than left to whoever creates the directory first, because `.code-basics/` is
/// deliberately shared through git and this file is deliberately not: getting
/// the two out of order once is enough to put one machine's derived index into
/// everybody else's checkout.
pub fn save(root: &Path, cache: &SymbolCache) -> anyhow::Result<()> {
    use anyhow::Context;

    let dir = crate::config::config_dir(root);
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    crate::config::ensure_gitignore(&dir)?;

    let path = cache_path(root);
    let json = serde_json::to_string(cache).context("failed to serialise the symbol cache")?;
    std::fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))
}

/// What `stat` can tell us about a file, or nothing.
///
/// `None` for a file that has gone, for one whose modification time the
/// platform will not report, and for one dated before 1970. All three mean the
/// same thing to the caller — this file cannot be fingerprinted, so it must be
/// re-read — and distinguishing them would only offer a chance to get one of
/// them wrong.
pub fn fingerprint(absolute: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(absolute).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some((mtime, meta.len()))
}

/// Build the index, reusing everything the cache can still vouch for, and
/// leave the refreshed cache behind.
///
/// The result is the same [`SymbolIndex`] a cold
/// [`crate::symbols::index::build`] would produce, for any file whose
/// fingerprint is honest — that equivalence is what the tests hold this to, and
/// it is the only property that makes the cache safe to put in front of the
/// palette. "Fingerprint" there means both halves: the per-file `mtime`/length
/// pair *and* the project list recorded at the top of the file, since a
/// symbol's `project_id` comes from the latter and no amount of the former can
/// vouch for it.
///
/// One gap in that equivalence is known and is not closed here. A file above
/// [`crate::symbols::index::MAX_INDEXED_BYTES`] makes a cold build set
/// [`SymbolIndex::truncated`], because its symbols were never looked at; this
/// path cannot see that, since [`crate::symbols::index::index_file`] returns
/// symbols and not the reason there are none. The symbols agree either way —
/// both paths yield nothing for such a file — so the divergence is confined to
/// the truncation flag being under-reported on the warm path. Closing it needs
/// a change in `index`, which owns the byte cap.
pub fn build_cached(root: &Path, projects: &[Project]) -> SymbolIndex {
    let limits = Limits::default();
    let fingerprint_of_projects = project_fingerprint(projects);

    // Ownership is not a property of any file, so a project list that has moved
    // on invalidates every entry at once rather than any of them individually.
    let stored = load(root);
    let remembered: HashMap<PathBuf, CacheEntry> = if stored.projects == fingerprint_of_projects {
        stored
            .entries
            .into_iter()
            .map(|e| (e.path.clone(), e))
            .collect()
    } else {
        HashMap::new()
    };

    let (files, mut truncated) = crate::symbols::index::walk(root, limits.max_files);

    // Partitioned before anything is parsed, so the fan-out below covers only
    // the work that actually has to happen. A file with no fingerprint is
    // treated as stale rather than skipped: it may still be readable even when
    // its metadata is not.
    let mut entries: Vec<Option<CacheEntry>> = Vec::with_capacity(files.len());
    let mut stale: Vec<(usize, PathBuf, u64, u64)> = Vec::new();
    for (position, relative) in files.iter().enumerate() {
        let stat = fingerprint(&root.join(relative));
        let hit = stat.and_then(|(mtime, len)| {
            let entry = remembered.get(relative)?;
            (entry.mtime_secs == mtime && entry.len == len).then(|| entry.clone())
        });
        match hit {
            Some(entry) => entries.push(Some(entry)),
            None => {
                entries.push(None);
                let (mtime, len) = stat.unwrap_or((0, 0));
                stale.push((position, relative.clone(), mtime, len));
            }
        }
    }

    for (position, relative, mtime_secs, len, symbols) in parse_stale(root, stale, projects) {
        entries[position] = Some(CacheEntry {
            path: relative,
            mtime_secs,
            len,
            symbols,
        });
    }

    // Every position was either a cache hit or a parse, so nothing is left
    // unfilled. `expect` rather than a default: a hole here would mean the two
    // passes above had disagreed about the file list, which is a bug and not
    // something to paper over with an empty entry that reads as "declares
    // nothing".
    let entries: Vec<CacheEntry> = entries
        .into_iter()
        .map(|entry| entry.expect("every walked file is either reused or parsed"))
        .collect();

    let mut symbols: Vec<Symbol> = entries.iter().flat_map(|e| e.symbols.clone()).collect();
    symbols.sort_by(|a, b| (&a.path, a.line, &a.name).cmp(&(&b.path, b.line, &b.name)));

    // The cache is written from the untruncated per-file results: each entry
    // is complete for its own file whatever the total came to, and recording a
    // clipped one would quietly persist the clip.
    let _ = save(
        root,
        &SymbolCache {
            version: CACHE_VERSION,
            heuristic_version: HEURISTIC_VERSION,
            projects: fingerprint_of_projects,
            entries,
        },
    );

    if symbols.len() > limits.max_symbols {
        symbols.truncate(limits.max_symbols);
        truncated = true;
    }

    SymbolIndex {
        root: root.to_path_buf(),
        files,
        symbols,
        truncated,
    }
}

/// Re-read the workspace from source, ignoring anything already cached, and
/// leave the fresh result behind.
///
/// This is the escape hatch the module note points at. It exists because the
/// fingerprint is a heuristic and heuristics need a way out: a user who can see
/// that the palette is wrong needs one action that is guaranteed to fix it,
/// rather than advice about touching files.
pub fn rebuild(root: &Path, projects: &[Project]) -> SymbolIndex {
    let _ = std::fs::remove_file(cache_path(root));
    build_cached(root, projects)
}

/// Parse the files the cache could not vouch for, spread over the cores.
///
/// Mirrors the fan-out in [`crate::symbols::index`] deliberately rather than
/// sharing it: that one owns the cold path and is free to change shape, and
/// coupling the two would make the caching layer a reason not to touch it.
/// Chunks are joined in spawn order so the concatenation is deterministic
/// before it is sorted.
type StaleParse = (usize, PathBuf, u64, u64, Vec<Symbol>);

fn parse_stale(
    root: &Path,
    stale: Vec<(usize, PathBuf, u64, u64)>,
    projects: &[Project],
) -> Vec<StaleParse> {
    if stale.is_empty() {
        return Vec::new();
    }

    let threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(stale.len());
    let chunk = stale.len().div_ceil(threads).max(1);

    let mut out = Vec::with_capacity(stale.len());
    thread::scope(|scope| {
        let handles: Vec<_> = stale
            .chunks(chunk)
            .map(|batch| {
                scope.spawn(move || {
                    batch
                        .iter()
                        .map(|(position, relative, mtime, len)| {
                            let symbols =
                                crate::symbols::index::index_file(root, relative, projects);
                            (*position, relative.clone(), *mtime, *len, symbols)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for handle in handles {
            // A worker panic is a bug here, not a condition to recover from,
            // and swallowing it would cache a file as declaring nothing.
            out.extend(handle.join().expect("symbol parsing thread panicked"));
        }
    });
    out
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod cache_tests;
