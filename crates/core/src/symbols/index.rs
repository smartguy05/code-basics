//! Building the symbol index: walking a workspace's source files and recording
//! what each one declares.
//!
//! The walk goes through [`crate::workspace::source_walker`] rather than a
//! filter of its own, so the index and the project list are looking at exactly
//! the same tree — build output, nested checkouts and everything else in
//! `SKIP_DIRS` are excluded once, in one place.
//!
//! Each line is offered to [`crate::symbols::declarations`], which is a word
//! scan and not a parser. The index therefore holds a best effort, and says so:
//! a file it could not read contributes nothing rather than an empty entry that
//! would read as "this file declares nothing".
//!
//! # One walk, two answers
//!
//! [`SymbolIndex::files`] is filled by the same pass that fills
//! [`SymbolIndex::symbols`], and that is deliberate rather than incidental. A
//! palette wants to answer two questions — "open the file called X" and "jump
//! to the thing called X" — and the obvious implementation gives each its own
//! walk. Two walks can disagree, and they disagree in the way that is hardest
//! to notice: go-to-file offers a path that go-to-symbol has never heard of, or
//! the reverse, and nothing reports an error. Sharing the pass makes that
//! impossible by construction, and costs a `Vec<PathBuf>` that was already
//! materialised in order to hand chunks of it to the parsing threads.
//!
//! It also means a file that contributes no symbols is still *listed*. A README
//! is not a parse failure, and neither is a 4 MB generated file; both are
//! openable, and refusing to list them would be a worse answer than listing
//! them with nothing attached.
//!
//! # What is deliberately not done here
//!
//! No language is parsed, no `using`/`import` graph is resolved, and no attempt
//! is made to qualify a name with its namespace or enclosing type. All three
//! would need a real front end per ecosystem. The index answers "where is the
//! line that appears to declare this name", and the abstain rule that governs
//! [`crate::git::attribution`] and [`crate::inspect`] governs it too: every
//! threshold below drops a candidate rather than inventing one, and every cap
//! sets [`SymbolIndex::truncated`] rather than returning a short list that
//! looks complete.

use std::path::{Path, PathBuf};
use std::thread;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::model::Project;
use crate::symbols::declarations::{declaration, SymbolKind};

/// One declaration, and where to find it.
///
/// [`Symbol::path`] is workspace-relative with forward slashes on every
/// platform: it is a key that is persisted in [`crate::symbols::cache`] and is
/// meant to be compared against paths from the git and file-tree layers and
/// handed to the frontend once a command returns one, so a Windows-shaped
/// separator here would leak into all three and only fail on somebody else's
/// machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// Workspace-relative, forward slashes.
    pub path: PathBuf,
    /// 1-based, matching what an editor puts in its gutter.
    pub line: u32,
    /// The innermost project containing the file, when there is one. `None` is
    /// a real answer: loose scripts, workspace-level config and anything above
    /// the projects belong to no project, and naming one for them would be a
    /// guess.
    pub project_id: Option<String>,
}

/// Everything one walk of a workspace found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SymbolIndex {
    pub root: PathBuf,
    /// Every file the walk yielded, workspace-relative with forward slashes,
    /// sorted. Not filtered by extension — see the module note above.
    pub files: Vec<PathBuf>,
    /// Sorted by `(path, line, name)`.
    pub symbols: Vec<Symbol>,
    /// True when a cap was reached and the lists below are therefore
    /// incomplete. The UI is expected to say so; silently showing a clipped
    /// list as if it were the whole workspace is the wrong answer this crate
    /// refuses to give.
    pub truncated: bool,
}

/// What the palette needs to know about the index behind it, without being
/// handed the index itself.
///
/// A palette has to distinguish three states and word them differently: no
/// index at all, an index being built, and an index that is complete or was
/// clipped by a cap. Returning the whole [`SymbolIndex`] to answer that would
/// send tens of thousands of symbols across IPC to compute two integers, so the
/// counts cross instead.
///
/// [`SymbolIndexStatus::ready`] means "there is an index to search", not "the
/// index is finished". Those come apart during a rebuild, when a perfectly
/// usable index is in place and a fresh one is being computed over it, and
/// conflating them would blank a working palette for the duration of every
/// rebuild. `building` is the separate flag that says a rebuild is under way.
///
/// It is deliberately *not* a discriminated enum. The states are not exclusive
/// — ready-and-building is the normal case on a rescan — and an enum would
/// force whoever wrote it to pick one, which is how a rebuild would come to
/// look like an empty workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SymbolIndexStatus {
    /// Whether a search can be answered at all right now.
    pub ready: bool,
    /// Whether a build is running. Orthogonal to `ready`.
    pub building: bool,
    pub files: usize,
    pub symbols: usize,
    /// Mirrors [`SymbolIndex::truncated`]. Zero when there is no index — which
    /// is not a claim that nothing was clipped, only that nothing was looked
    /// at, and `ready: false` is what says so.
    pub truncated: bool,
}

impl SymbolIndexStatus {
    /// Describe whatever index there is, or the absence of one.
    ///
    /// Takes the `Option` rather than being called only when there is an index,
    /// so the "no index yet" wording is decided once here instead of being
    /// assembled at each caller — that assembly is where a missing index would
    /// otherwise get reported as an empty workspace with `ready: true`.
    pub fn of(index: Option<&SymbolIndex>, building: bool) -> Self {
        match index {
            Some(index) => Self {
                ready: true,
                building,
                files: index.files.len(),
                symbols: index.symbols.len(),
                truncated: index.truncated,
            },
            None => Self {
                ready: false,
                building,
                files: 0,
                symbols: 0,
                truncated: false,
            },
        }
    }
}

/// The largest file that is worth *parsing*, as opposed to listing.
///
/// One mebibyte, and deliberately not the 5 MiB the editor will open. The two
/// limits protect against different things. The editor's limit is about
/// whether a human can usefully look at a file; this one is about whether the
/// result is worth putting in a palette. A 4 MB generated `.cs` file — an EF
/// migration snapshot, a service reference, a resource designer — parses fine
/// and yields something like forty thousand symbols, none of which anyone has
/// ever wanted to jump to, and all of which sit between the user and the
/// handful of symbols they did want. Raising this limit does not make the
/// palette more capable, it makes it less usable.
pub const MAX_INDEXED_BYTES: u64 = 1024 * 1024;

/// How much of a file is sniffed for a NUL byte before it is believed to be
/// text. Enough to catch every format that puts a header at the front, small
/// enough to be a single read that has already happened.
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

/// Extensions whose contents are offered to the declaration scan.
///
/// This is an allowlist rather than a denylist because the failure modes are
/// not symmetric. An unknown extension that is really source costs one missing
/// entry in a palette; an unknown extension that is really data — a lockfile, a
/// minified bundle, a CSV, an SVG — costs thousands of nonsense entries that
/// bury the real ones. Adding an extension without also teaching the scan that
/// language would just add noise.
///
/// The list is not, however, exactly the set of languages the scan has rules
/// for: `vb` is listed and has none. `declarations::DECLARING` matches
/// lowercase keywords exactly, so `Public Class Customer` matches nothing and
/// the line abstains — a `.vb` file is read, scanned, and indexes as empty.
/// The reason that gap is left open rather than closed by matching
/// case-insensitively is set out on `DECLARING` itself. Listing the extension
/// costs a wasted read per VB file and nothing else, which is the cheaper half
/// of the trade: dropping it would mean silently re-adding it the day VB gets
/// rules, while leaving it means only the rules have to be written.
///
/// `razor` and `cshtml` are the ASP.NET view formats: HTML markup interleaved
/// with C# in `@code` / `@functions` blocks and `@{ … }` expressions. They are
/// listed because that embedded C# is real, jump-worthy source, and the C#
/// rules in `declarations` scan it line by line and extract it. The surrounding
/// markup and the `@page` / `@inject` / `@using` directives carry no lowercase
/// declaring keyword, so — exactly like a `.vb` line — they abstain rather than
/// fabricate a symbol, which is why listing the extension adds signal and not
/// noise.
const PARSABLE_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "cs", "fs", "vb", "py", "go", "java", "kt", "rb",
    "php", "c", "h", "cpp", "hpp", "swift", "scala", "sql", "razor", "cshtml",
];

/// The caps a build runs under.
///
/// Extracted from the constants purely so the tests can drive the truncation
/// paths with three files instead of fifty thousand. Generating a workspace
/// large enough to hit the real caps would make the suite take minutes and
/// would test the filesystem rather than the logic.
pub(crate) struct Limits {
    /// Beyond this many files the walk stops. Fifty thousand is far past any
    /// repository this application is meant for, so hitting it means something
    /// unexpected — a data directory, a symlinked drive — and continuing would
    /// hang the UI rather than serve it.
    pub max_files: usize,
    /// Beyond this many symbols the list is cut. Two hundred thousand is
    /// already more than a fuzzy match can rank usefully.
    pub max_symbols: usize,
    pub max_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_files: 50_000,
            max_symbols: 200_000,
            max_bytes: MAX_INDEXED_BYTES,
        }
    }
}

/// Walk `root` and index everything under it.
///
/// Synchronous and deterministic: the same tree produces a byte-identical
/// [`SymbolIndex`] every time, which is what lets the caching layer compare
/// results and the tests assert on them. Parsing is fanned out over threads
/// internally, but the fan-out cannot be observed — chunks are joined in order
/// and the result is sorted before it is returned.
///
/// Takes `&[Project]` rather than a `&Workspace` so this module stays free of
/// workspace state: the only thing it wants from a project is where it lives.
pub fn build(root: &Path, projects: &[Project]) -> SymbolIndex {
    build_with(root, projects, Limits::default())
}

pub(crate) fn build_with(root: &Path, projects: &[Project], limits: Limits) -> SymbolIndex {
    let (files, walk_truncated) = walk(root, limits.max_files);
    let owners = Owners::new(root, projects);

    let (mut symbols, byte_cap_hit) = parse_all(root, &files, &owners, limits.max_bytes);
    let mut truncated = walk_truncated || byte_cap_hit;
    symbols.sort_by(|a, b| (&a.path, a.line, &a.name).cmp(&(&b.path, b.line, &b.name)));
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

/// Re-read one file, for the save-and-reindex path the Tauri layer will need.
///
/// Nothing in `src-tauri` calls this yet — the palette's command surface is a
/// later phase of this work — so the exclusion rule below is written against
/// what the walk does, not against any caller that exists to be checked.
///
/// `relative` is interpreted the way [`Symbol::path`] is written, and it is
/// re-checked against the walk's own exclusions rather than trusted. A save
/// into `bin/` or into a nested checkout must produce nothing here, because
/// the next full build would not produce it either — an entry that exists
/// until the next rebuild and then vanishes is exactly the kind of quiet
/// inconsistency the shared walker was introduced to prevent.
pub fn index_file(root: &Path, relative: &Path, projects: &[Project]) -> Vec<Symbol> {
    if !is_indexable_relative_path(relative) {
        return Vec::new();
    }
    let relative = normalise(relative);
    let owners = Owners::new(root, projects);
    let mut symbols = parse_file(root, &relative, &owners, MAX_INDEXED_BYTES).symbols;
    symbols.sort_by(|a, b| (a.line, &a.name).cmp(&(b.line, &b.name)));
    symbols
}

/// Swap one file's symbols into an index that is already built.
///
/// This is the save path: the editor writes a file, [`index_file`] re-reads
/// that one file, and the result has to take the place of whatever the last
/// build recorded for it. Without it the palette goes stale
/// the moment anybody edits anything, and stays stale until a rebuild — a
/// symbol that has been renamed goes on being offered under its old name and
/// jumps to a line that has moved, which is precisely the wrong answer this
/// module refuses to give.
///
/// It lives here rather than in the command that calls it because the splice is
/// the least of what it does: *which* entries count as belonging to the file,
/// what a path that the walk would never have produced means, and whether a
/// file nobody has seen before joins [`SymbolIndex::files`] are all questions
/// with a wrong answer available, and all three are settled by tests beside
/// this function.
///
/// # How the ordering is re-established
///
/// Everything one file declares occupies a single contiguous run of the
/// `(path, line, name)` order [`build`] leaves behind, so the replacement is a
/// splice over that run: two binary searches for its ends, a sort of the
/// handful of incoming entries among themselves, and one `splice`.
///
/// This was a `retain` + `extend` + full re-sort, which is the obvious way to
/// write it and reads as free until the index is real. Measured on a
/// synthesised workspace of 2,865 files and 17,184 symbols — a shape chosen to
/// match a large .NET solution — over four runs of fifty replacements each, the
/// full re-sort took a median of **3.1–5.0 ms** and the splice **0.07–0.20 ms**:
/// between twenty-five and sixty times, the spread being other work on the
/// machine rather than anything about the input. It is not a hot loop, but it
/// runs on every save while holding the mutex the palette searches through, and
/// the re-sort was most of that hold.
///
/// The cost of the splice is the memory move, which is why the incoming entries
/// are sorted and placed rather than inserted one at a time. Sortedness of
/// `index.symbols` on entry is a precondition rather than something re-derived:
/// [`build`] establishes it, and this function is the only thing that mutates
/// the list afterwards, so preserving it here keeps it true for good. The tests
/// beside this function check the order across the splice's neighbours on both
/// sides, not merely within the file that was replaced.
///
/// An incoming symbol whose `path` is not the file being replaced is dropped.
/// [`index_file`] stamps every entry it returns with the path it was asked
/// about, so such an entry can only come from a caller that has already
/// confused two files — and admitting it would file it under a name whose own
/// next save would not remove it, at a position the order does not allow.
///
/// The path is normalised and re-checked through the same
/// [`is_indexable_relative_path`] gate [`index_file`] uses, so a save into
/// build output or above the root changes nothing at all — including the file
/// list. Admitting such a file would put an entry in the palette that the next
/// full build silently removes.
///
/// # Why the file list is checked against the disk
///
/// That lexical gate is necessary and not sufficient. `src/Api/Program.cs` is a
/// perfectly well-formed relative path under any root, and for a while a caller
/// handed one in that was relative to a *repository* sitting above the
/// workspace: nothing about the string said so, the file it named did not
/// exist, and it joined [`SymbolIndex::files`] anyway. The result was a palette
/// row that opened nothing, under a root the user never edited a file in.
///
/// So membership of the file list is settled by a `stat` — `is_file`, not
/// `exists`, because a directory is neither openable nor something the walk
/// ever recorded. It is one syscall on a path that was just written, and it is
/// the only check that can tell a genuinely new file apart from a path that
/// merely reads like one. The decision sits here rather than at the two
/// callers because it is the same question the doc above already answers for
/// build output — *would a full build have produced this entry?* — and
/// answering it in two places is how the two would come to disagree.
///
/// The symbol replacement above deliberately runs either way. A file that has
/// gone from disk declares nothing, and dropping what it used to declare is the
/// right answer; it is only the claim "this file is here to be opened" that
/// needs evidence.
///
/// [`SymbolIndex::truncated`] is deliberately left as it was. This function can
/// only see one file, and one file cannot tell you whether a workspace-wide cap
/// was hit; clearing the flag because this particular save was fine would claim
/// completeness nobody verified, and setting it would invent a clip.
pub fn replace_file(index: &mut SymbolIndex, relative: &Path, symbols: Vec<Symbol>) {
    if !is_indexable_relative_path(relative) {
        return;
    }
    let relative = normalise(relative);

    // Everything one file declares is one contiguous run of the `(path, line,
    // name)` order, so the whole replacement is a splice over that run: find
    // its two ends, sort the handful of incoming entries among themselves, and
    // put them where the old ones were.
    let start = index.symbols.partition_point(|s| s.path < relative);
    let end = index.symbols.partition_point(|s| s.path <= relative);

    let mut incoming: Vec<Symbol> = symbols.into_iter().filter(|s| s.path == relative).collect();
    incoming.sort_by(|a, b| (a.line, &a.name).cmp(&(b.line, &b.name)));
    index.symbols.splice(start..end, incoming);

    // A file saved for the first time is openable now, not after the next full
    // build — but only if it is really there. `files` is sorted, so the
    // insertion point is the search result.
    if !index.root.join(&relative).is_file() {
        return;
    }
    if let Err(position) = index.files.binary_search(&relative) {
        index.files.insert(position, relative);
    }
}

/// Drop a file from the index entirely: its symbols and its own entry.
///
/// [`replace_file`] deliberately cannot do this. It re-checks the path on disk
/// and *keeps* the [`SymbolIndex::files`] entry when the file is not there,
/// because the file being momentarily unreadable — mid-save, locked by another
/// process — is not evidence that it is gone. Deleting or renaming one is
/// evidence, and it is the only thing that is, so the caller who performed the
/// deletion is the only one who can say so.
///
/// Without this, a deleted file goes on being offered by the search palette
/// until the next full rebuild, and opening it fails on a path that no longer
/// names anything.
///
/// Removing something the index never held is a no-op rather than an error:
/// a file below the size or extension cut, or under build output, was never
/// indexed, and its deletion is still a perfectly ordinary thing to report.
pub fn remove_file(index: &mut SymbolIndex, relative: &Path) {
    let relative = normalise(relative);

    let start = index.symbols.partition_point(|s| s.path < relative);
    let end = index.symbols.partition_point(|s| s.path <= relative);
    index.symbols.drain(start..end);

    if let Ok(position) = index.files.binary_search(&relative) {
        index.files.remove(position);
    }
}

/// The key this index would record for an absolute path, or `None` if the path
/// is not under `root`.
///
/// Every path in a [`SymbolIndex`] is relative to the root it was walked from,
/// and the two editors that save a file do not agree on what their paths are
/// relative to: the Run tab's file tree is rooted at the workspace, while the
/// Changes tab's paths come from git and are relative to the *repository*,
/// which [`crate::git::Repo::open`] discovers at or above the opened directory.
/// Resolving each against the root it actually came from and re-keying the
/// absolute result here is what lets one save path serve both without either
/// caller having to know about the other's root.
///
/// `None` is the answer for a file the workspace does not contain, and it is a
/// real answer rather than a failure: a repository wider than the workspace has
/// files above and beside it, and keying one of those relative to a root it is
/// not under would put a path in the index that resolves to a different file
/// entirely. An unindexed save is stale until the next rebuild; a mis-keyed one
/// is wrong until somebody notices.
///
/// The root itself yields `None` too. Stripping it leaves an empty path, which
/// names no file and would sort ahead of every real entry in
/// [`SymbolIndex::files`].
///
/// Matching is component-wise, so a root spelled with a trailing separator — as
/// libgit2 spells a working directory — keys the same file as one without.
/// It is not case-folded and does not resolve symlinks or short names: two
/// spellings of the same directory that differ in case will fail to match on
/// Windows and produce `None`, which is the abstaining half of the trade rather
/// than a guess at which of them the user meant.
pub fn relative_to_root(root: &Path, absolute: &Path) -> Option<PathBuf> {
    let relative = absolute.strip_prefix(root).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(normalise(relative))
}

/// Whether a path handed in from outside is one the walk could have produced.
///
/// Absolute paths and any `..` are refused outright: both would let a caller
/// name a file outside the workspace, and a symbol whose recorded path escapes
/// the root cannot be resolved by anything downstream.
fn is_indexable_relative_path(relative: &Path) -> bool {
    use std::path::Component;
    if relative.is_absolute() {
        return false;
    }
    relative.components().all(|c| match c {
        Component::Normal(name) => !name.to_str().is_some_and(crate::workspace::should_skip),
        Component::CurDir => true,
        _ => false,
    })
}

/// Collect the tree's files, workspace-relative and sorted, plus whether the
/// file cap cut the list short.
///
/// Errors from individual entries are dropped rather than aborting the walk: a
/// directory that cannot be read (a permissions quirk, a path that vanished
/// mid-walk) should cost that subtree, not the whole index.
///
/// # Why this is `pub(crate)`
///
/// [`crate::symbols::cache`] needs exactly this list — a warm rebuild has to
/// decide which files are still present, which is the same question a cold
/// build asks — and for a while it answered that question with its own copy of
/// this function. Two copies of a walk are not two implementations of one rule;
/// they are two rules that happen to agree today. They had in fact already
/// begun to drift in shape (one normalised through [`normalise`], the other
/// inlined the same replacement by hand), and nothing would have failed if one
/// of them had later grown a filter the other did not. Sharing the function
/// makes disagreement impossible rather than merely unlikely, which is the same
/// argument the module note above makes for filling `files` and `symbols` from
/// one pass.
///
/// The cap is a parameter rather than read from [`Limits`] here because the
/// caller already owns its own `Limits` and passing the whole struct would tie
/// this to a caps policy it does not otherwise care about.
pub(crate) fn walk(root: &Path, max_files: usize) -> (Vec<PathBuf>, bool) {
    let mut files = Vec::new();
    let mut truncated = false;

    for entry in crate::workspace::source_walker(root).filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        if files.len() >= max_files {
            truncated = true;
            break;
        }
        files.push(normalise(
            entry.path().strip_prefix(root).unwrap_or(entry.path()),
        ));
    }

    // Sorted rather than left in walk order. `walkdir` yields directory
    // entries in whatever order the platform's `readdir` produces, which is
    // stable within a run but not across machines or filesystems, and a cache
    // that compares two builds must not see a difference that is really just
    // NTFS versus ext4.
    files.sort();
    (files, truncated)
}

/// Parse the collected files, spread over the available cores.
///
/// The walk itself stays single-threaded — it is one tree, and the ordering
/// guarantee above depends on it — but reading and scanning the files is
/// embarrassingly parallel and is where nearly all the wall-clock time goes.
/// `std::thread::scope` is used rather than a thread-pool dependency: the work
/// is a single fixed-size fan-out with a join at the end, which is precisely
/// the shape `scope` exists for, and it has been stable since 1.63 against a
/// floor of 1.82.
///
/// Chunks are joined in spawn order, so the concatenation is deterministic
/// before it is even sorted.
///
/// The second half of the return value is [`ParsedFile::skipped_oversized`]
/// or-ed over every file, which is how the byte cap reaches
/// [`SymbolIndex::truncated`].
fn parse_all(
    root: &Path,
    files: &[PathBuf],
    owners: &Owners,
    max_bytes: u64,
) -> (Vec<Symbol>, bool) {
    if files.is_empty() {
        return (Vec::new(), false);
    }

    let threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(files.len());
    let chunk = files.len().div_ceil(threads).max(1);

    let mut out = Vec::new();
    let mut skipped_any = false;
    thread::scope(|scope| {
        let handles: Vec<_> = files
            .chunks(chunk)
            .map(|batch| {
                scope.spawn(move || {
                    let mut found = ParsedFile::default();
                    for relative in batch {
                        let parsed = parse_file(root, relative, owners, max_bytes);
                        found.symbols.extend(parsed.symbols);
                        found.skipped_oversized |= parsed.skipped_oversized;
                    }
                    found
                })
            })
            .collect();

        for handle in handles {
            // A worker panic is a bug in this module, not a condition to
            // recover from, and swallowing it would silently produce a short
            // index that claims to be complete.
            let parsed = handle.join().expect("symbol parsing thread panicked");
            out.extend(parsed.symbols);
            skipped_any |= parsed.skipped_oversized;
        }
    });
    (out, skipped_any)
}

/// What one file contributed, and whether a cap stopped it contributing.
///
/// The flag exists because the two ways a file yields nothing are not the same
/// answer and must not be collapsed into one. A file that genuinely declares
/// nothing — a README, a module of pure `impl` bodies — makes the index no less
/// complete, and reporting truncation for it would put a warning in front of
/// almost every workspace. A file skipped by [`Limits::max_bytes`] does make the
/// index incomplete: a user searching for a symbol that lives in a 1.2 MB
/// generated file gets an empty list from an index that claims to be whole, and
/// has no way to tell that it was never looked at.
///
/// Deliberately not inferred from `symbols.is_empty()` at the call site. That
/// would be the same conflation written as a heuristic, and this module's
/// governing rule is that a wrong answer is worse than no answer.
///
/// The other early returns in [`parse_file`] — unreadable, binary, not UTF-8 —
/// keep the flag clear on purpose. They are properties of the file rather than
/// of a cap the user could raise, the doc there already argues that surfacing
/// them helps nobody, and lighting `truncated` for them would say the workspace
/// was clipped when it was fully examined.
#[derive(Default)]
struct ParsedFile {
    symbols: Vec<Symbol>,
    skipped_oversized: bool,
}

/// Read one file and pull every declaration out of it.
///
/// Every early return here is the abstain rule in miniature. A file that is
/// unreadable, binary or not UTF-8 yields an empty vector, which is
/// indistinguishable in the result from a file that genuinely declares
/// nothing — and that is the intended outcome. The alternative, recording that
/// a file *failed*, would put a diagnostic in front of the user for a
/// condition they cannot act on and did not ask about.
///
/// The byte cap is the one exception, and [`ParsedFile`] explains why: skipping
/// a huge generated file is a good decision about *what to index*, but it is
/// still a reason the answer is not complete, and the caller is told so.
fn parse_file(root: &Path, relative: &Path, owners: &Owners, max_bytes: u64) -> ParsedFile {
    let extension = relative
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    let Some(extension) = extension else {
        return ParsedFile::default();
    };
    if !PARSABLE_EXTENSIONS.contains(&extension.as_str()) {
        return ParsedFile::default();
    }

    let absolute = root.join(relative);
    match std::fs::metadata(&absolute) {
        Ok(meta) if meta.len() > max_bytes => {
            return ParsedFile {
                symbols: Vec::new(),
                skipped_oversized: true,
            }
        }
        Ok(_) => {}
        Err(_) => return ParsedFile::default(),
    }

    let Ok(bytes) = std::fs::read(&absolute) else {
        return ParsedFile::default();
    };
    // A NUL in the first few kilobytes is the cheap, conventional test for
    // "this is not text". It is not a proof — a UTF-16 source file would be
    // rejected by it — but the failure is in the safe direction: a file that
    // is wrongly skipped is still listed and still openable.
    if bytes[..bytes.len().min(BINARY_SNIFF_BYTES)].contains(&0) {
        return ParsedFile::default();
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return ParsedFile::default();
    };

    let project_id = owners.owner_of(relative);
    let symbols = text
        .lines()
        .enumerate()
        .filter_map(|(offset, line)| {
            let declared = declaration(line)?;
            if is_local_binding(line, declared.kind) {
                return None;
            }
            Some(Symbol {
                name: declared.name,
                kind: declared.kind,
                path: relative.to_path_buf(),
                // `enumerate` counts from zero; gutters count from one, and
                // this number is handed straight to an editor.
                line: offset as u32 + 1,
                project_id: project_id.clone(),
            })
        })
        .collect();
    ParsedFile {
        symbols,
        skipped_oversized: false,
    }
}

/// Whether a binding looks local enough to be noise.
///
/// This is the one filter in the module that throws away things the
/// declaration scan was happy to name, and it earns its place: `declarations`
/// treats `const`, `let` and `var` as declaring keywords — it must, because a
/// hunk sitting on a module-level constant deserves a title — so a whole-file
/// sweep surfaces every loop counter and every intermediate variable. Counted
/// over this repository, indented `let`/`var`/`const`/`static` lines outnumber
/// column-zero ones by roughly 4200 to 180. Left unfiltered the palette would
/// be about ninety-six percent local variables.
///
/// The rule is indentation: a binding written hard against column zero is a
/// module-level or file-level declaration and is kept; a binding indented by
/// anything at all is inside a body and is dropped. Nothing cleverer is
/// available from one line of text without tracking brace depth, and brace
/// depth is a parser.
///
/// # What it gets wrong
///
/// Two things, both knowingly:
///
/// * **A class-level constant is lost.** C#'s `private const int MaxRetries`
///   and Rust's `const LIMIT` inside an `impl` block are indented, so they are
///   dropped along with the locals. In this repository that is about forty
///   real symbols against the ~4200 of noise, and a symbol missing from a
///   palette is a far cheaper failure than a palette nobody can read.
/// * **A C# field is still kept.** `private readonly IGitService _git;` gets
///   [`SymbolKind::Other`] from the scan, not `Variable`, because `readonly`
///   says nothing about what is being declared. `Other` is also what a plain
///   C# method signature gets, and no rule over a single line can separate the
///   two — so fields stay in. That is the abstain rule again: dropping
///   everything unplaceable would take the methods with it.
fn is_local_binding(line: &str, kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::Constant | SymbolKind::Variable) && line.starts_with([' ', '\t'])
}

/// Which project, if any, a file belongs to.
///
/// Longest prefix wins, so a project nested inside another gets its own files
/// rather than losing them to the outer one — the common .NET layout where a
/// test project sits under the directory of the thing it tests would otherwise
/// attribute every test to the wrong project.
///
/// Matching is on whole path components. A prefix compared as a plain string
/// would put `src/App/Program.cs` inside a project rooted at `src/A`, and the
/// mislabelling would be invisible until someone filtered the palette by
/// project and quietly got the wrong list.
struct Owners {
    /// `(project directory relative to root, project id)`, longest directory
    /// first.
    entries: Vec<(String, String)>,
}

impl Owners {
    fn new(root: &Path, projects: &[Project]) -> Self {
        let mut entries: Vec<(String, String)> = projects
            .iter()
            .filter_map(|p| {
                // A project outside the walked root cannot own anything the
                // walk produced, so it is dropped rather than matched against
                // an absolute path that will never appear.
                let relative = p.dir.strip_prefix(root).ok()?;
                Some((
                    normalise(relative).to_string_lossy().into_owned(),
                    p.id.clone(),
                ))
            })
            .collect();
        entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        Self { entries }
    }

    fn owner_of(&self, relative: &Path) -> Option<String> {
        let path = relative.to_string_lossy();
        self.entries
            .iter()
            .find(|(dir, _)| {
                // A project at the workspace root owns everything not claimed
                // by something deeper.
                dir.is_empty()
                    || path
                        .strip_prefix(dir.as_str())
                        .is_some_and(|r| r.starts_with('/'))
            })
            .map(|(_, id)| id.clone())
    }
}

/// A relative path with forward slashes, whatever the platform produced.
///
/// `pub(crate)` for the same reason [`walk`] is: the cache writes the same kind
/// of key and must spell it the same way, and a second inlined copy of this one
/// line is how the two walks started to diverge.
pub(crate) fn normalise(relative: &Path) -> PathBuf {
    PathBuf::from(relative.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
#[path = "index_tests.rs"]
mod index_tests;
