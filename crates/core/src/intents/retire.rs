//! Retiring the intents a commit has absorbed.
//!
//! # The problem this exists for
//!
//! An [`IntentRecord`](crate::intents::IntentRecord) carries no timestamp and no
//! commit, and [`crate::git::attribution`] matches it against the current diff by
//! **content alone**. Nothing ever removed one. So a reason recorded for a change
//! that was committed weeks ago stayed in the store and re-titled a card the
//! moment its text reappeared — a confidently wrong label on work it had nothing
//! to do with, which is the one outcome the intent feature must avoid.
//!
//! # The rule: full absorption, judged against HEAD
//!
//! A record retires only when HEAD accounts for **every** anchorable line of its
//! evidence — an addition's lines are all present in the HEAD blob, a pure
//! deletion's lines are all absent from it. Anything less keeps it whole.
//!
//! That "every" is what makes line-level partial staging safe. Commit half a
//! card and the other half is still missing from HEAD, so the record survives
//! entire and keeps titling the hunks that are left; it retires at the next
//! commit, when HEAD finally accounts for all of it.
//!
//! # Why the working diff cannot be part of the test
//!
//! The tempting second condition — "and none of its lines are still in the
//! working diff" — defeats the bug it is meant to fix. The reported symptom *is*
//! a committed record's text reappearing as an addition somewhere else, so the
//! stale record always looks like it has a live remainder, and the check would
//! keep exactly the records that need retiring. Matching is content-only, so
//! "this text is uncommitted" and "this text was committed and typed again" are
//! indistinguishable in the diff and only HEAD can tell them apart.
//!
//! The working diff is still read, but only to *explain* a keep — see
//! [`KeepReason`] — never to decide one.
//!
//! # Abstaining
//!
//! Every uncertainty keeps the record. Evidence is judged only on lines
//! [`anchor_key`] accepts, so a line the matcher would never anchor on cannot
//! decide a retirement either; a record of nothing but braces decides nothing at
//! all; an unreadable file decides nothing; and a whole-file write — already
//! weak evidence by construction, because it "matches everything" — is never
//! retired on content, only when its path has gone quiet entirely.
//!
//! # Compaction
//!
//! Rewriting `edits.jsonl` without the retired records is the deliberate side
//! effect. The file grows without bound (16 MB in the workspace this was written
//! for) and [`crate::intents::next_seq`] re-parses all of it on every hook fire,
//! so shrinking it is worth as much as the correctness fix.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::git::attribution::anchor_key;
use crate::git::patch::LineOrigin;
use crate::git::repo::{ComparisonMode, Repo};
use crate::git::why::content_hash;
use crate::intents::{self, normalise_path, IntentLabel, IntentRecord, Intents, LoadOptions};

/// Retired records, kept rather than deleted: this is the only copy of why the
/// code says what it says.
pub const ARCHIVE_FILE: &str = "edits-archive.jsonl";
/// Retired labels, alongside the records they titled.
pub const LABEL_ARCHIVE_FILE: &str = "labels-archive.jsonl";
/// The identities of everything retired, so a re-import cannot resurrect it.
pub const TOMBSTONE_FILE: &str = "tombstones.jsonl";
/// The last HEAD this workspace was pruned against, and the sequence high-water
/// mark that must outlive the records it came from.
pub const STATE_FILE: &str = "prune-state.json";
/// Held for the duration of a rewrite so two prunes cannot interleave.
pub const LOCK_FILE: &str = "prune.lock";

/// A lock older than this is assumed to belong to a process that died.
const STALE_LOCK_SECS: u64 = 300;

pub fn archive_path(root: &Path) -> PathBuf {
    intents::intents_dir(root).join(ARCHIVE_FILE)
}

pub fn label_archive_path(root: &Path) -> PathBuf {
    intents::intents_dir(root).join(LABEL_ARCHIVE_FILE)
}

pub fn tombstone_path(root: &Path) -> PathBuf {
    intents::intents_dir(root).join(TOMBSTONE_FILE)
}

pub fn state_path(root: &Path) -> PathBuf {
    intents::intents_dir(root).join(STATE_FILE)
}

fn lock_path(root: &Path) -> PathBuf {
    intents::intents_dir(root).join(LOCK_FILE)
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// What one path looked like when the prune ran: what HEAD holds, and what is
/// still uncommitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSnapshot {
    pub path: String,
    /// The file's contents at HEAD, or `None` when it does not exist there.
    pub head_blob: Option<String>,
    /// Added lines in the `WorkingToHead` diff.
    pub working_added: Vec<String>,
    /// Removed lines in the `WorkingToHead` diff.
    pub working_removed: Vec<String>,
    /// Whether the path appears in the working diff at all. Distinct from the
    /// two line vectors being empty, which a binary change also produces.
    pub in_working_diff: bool,
    /// False for a binary or unreadable file, where nothing can be decided.
    pub readable: bool,
}

/// Why a record was kept. Internal — it never crosses IPC; the UI is told counts
/// so it cannot present an abstention as a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepReason {
    /// Some of its text is still uncommitted.
    StillInWorkingTree,
    /// HEAD does not account for it — a different branch's work.
    NotInHead,
    /// No line of it is distinctive enough to decide on.
    NoEvidence,
    /// A whole-file write, whose path is still being edited.
    WholeFileWrite,
    /// Binary, or the blob could not be read.
    Unreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Retire,
    Keep(KeepReason),
}

/// Which indices of an [`Intents`] a prune would remove.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetirePlan {
    /// Indices into `Intents::records`.
    pub records: Vec<usize>,
    /// Indices into `Intents::labels`.
    pub labels: Vec<usize>,
    /// How many records survived.
    pub kept: usize,
}

/// The identity of something retired. Matched as a **conjunction**: the agent's
/// own call id *and* the content, so a genuinely new edit that happens to repeat
/// an old change is not suppressed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tombstone {
    pub tool_use_id: String,
    pub content_key: String,
    pub path: String,
}

/// One archived record, with the commit that absorbed it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedRecord {
    #[serde(flatten)]
    pub record: IntentRecord,
    /// The HEAD the prune ran against, so the archive can be read back in order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_at_head: Option<String>,
}

/// Where the prune left off. Persisted so a commit made outside the app — in a
/// floating terminal, or an amend or rebase from a shell — is still noticed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PruneState {
    #[serde(default = "default_version")]
    pub version: u32,
    /// The HEAD oid the last prune saw. `None` before the first look.
    #[serde(default)]
    pub last_head: Option<String>,
    /// The highest sequence number ever handed out, which must survive the
    /// records it came from — see [`intents::next_seq`].
    #[serde(default)]
    pub high_seq: u64,
}

fn default_version() -> u32 {
    1
}

/// What a prune did. The one type here that crosses IPC.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RetireSummary {
    pub records_retired: usize,
    pub labels_retired: usize,
    pub kept_records: usize,
    /// The HEAD the prune ran against, when there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    /// False when this was a preview, or when nothing needed doing.
    pub pruned: bool,
}

// ---------------------------------------------------------------------------
// The decision
// ---------------------------------------------------------------------------

/// The identity of a record's content, stable across platforms and releases
/// because it is persisted. Reuses the durable-why hash for the same reason it
/// was chosen there.
pub fn content_key(record: &IntentRecord) -> String {
    let mut material = String::new();
    material.push_str(record.provider.as_str());
    material.push('\0');
    material.push_str(&normalise_path(&record.path));
    for line in &record.edit.old_lines {
        material.push('\0');
        material.push('-');
        material.push_str(line);
    }
    for line in &record.edit.new_lines {
        material.push('\0');
        material.push('+');
        material.push_str(line);
    }
    content_hash(&material)
}

/// The anchorable forms of a set of lines. Lines the matcher would never anchor
/// on are dropped, so they cannot decide a retirement either.
fn anchors(lines: &[String]) -> Vec<String> {
    lines.iter().filter_map(|l| anchor_key(l)).collect()
}

fn anchor_set(lines: &[String]) -> HashSet<String> {
    lines.iter().filter_map(|l| anchor_key(l)).collect()
}

/// Decide whether one record has been absorbed by HEAD. See the module docs for
/// the rule and why both halves are required.
pub fn verdict(record: &IntentRecord, snapshot: &FileSnapshot) -> Verdict {
    if !snapshot.readable {
        return Verdict::Keep(KeepReason::Unreadable);
    }

    // A whole-file write matches everything, so its lines being in HEAD proves
    // nothing at all. The only safe reading is "this path has gone quiet".
    if record.edit.whole_file {
        if snapshot.in_working_diff {
            return Verdict::Keep(KeepReason::WholeFileWrite);
        }
        return match snapshot.head_blob {
            Some(_) => Verdict::Retire,
            None => Verdict::Keep(KeepReason::WholeFileWrite),
        };
    }

    let added = anchors(&record.edit.new_lines);
    let removed = anchors(&record.edit.old_lines);
    if added.is_empty() && removed.is_empty() {
        return Verdict::Keep(KeepReason::NoEvidence);
    }

    let Some(blob) = snapshot.head_blob.as_deref() else {
        // The file is not at HEAD at all. If it is not in the working tree
        // either it is simply gone, and there is nothing left for this record to
        // label; otherwise it is new and uncommitted, so the record is live.
        return if snapshot.in_working_diff {
            Verdict::Keep(KeepReason::StillInWorkingTree)
        } else {
            Verdict::Retire
        };
    };

    let in_head = anchor_set(&blob.lines().map(str::to_string).collect::<Vec<_>>());

    // Full absorption: HEAD accounts for every line of the evidence. Judged
    // against HEAD alone — see the module docs for why the working diff must
    // not get a vote.
    let additions_landed = added.iter().all(|a| in_head.contains(a));
    let deletions_landed = removed.iter().all(|r| !in_head.contains(r));
    if additions_landed && deletions_landed {
        return Verdict::Retire;
    }

    // Kept. The working diff decides nothing, but it does say *which* kind of
    // keep this is, which is worth distinguishing when reading a prune.
    let live_added = anchor_set(&snapshot.working_added);
    let live_removed = anchor_set(&snapshot.working_removed);
    let live = added.iter().any(|a| live_added.contains(a))
        || removed.iter().any(|r| live_removed.contains(r));

    if live {
        Verdict::Keep(KeepReason::StillInWorkingTree)
    } else {
        Verdict::Keep(KeepReason::NotInHead)
    }
}

/// Plan a whole prune: which records go, and then which labels are left titling
/// nothing.
///
/// A record whose path has no snapshot is always kept — never decide about a
/// path nobody looked at.
pub fn plan(intents: &Intents, snapshots: &[FileSnapshot]) -> RetirePlan {
    let by_path: HashMap<&str, &FileSnapshot> =
        snapshots.iter().map(|s| (s.path.as_str(), s)).collect();

    let mut records = Vec::new();
    let mut surviving_turns: HashSet<&str> = HashSet::new();

    for (index, record) in intents.records.iter().enumerate() {
        let retire = by_path
            .get(record.path.as_str())
            .map(|s| verdict(record, s) == Verdict::Retire)
            .unwrap_or(false);

        if retire {
            records.push(index);
        } else {
            surviving_turns.insert(record.turn_id.as_str());
        }
    }

    // A label outlives its own records only while some record still carries its
    // turn: a declared, path-scoped label can title an orphan on that path from
    // any turn, so a stale one is a second way the same bug shows up.
    let labels = intents
        .labels
        .iter()
        .enumerate()
        .filter(|(_, l)| !surviving_turns.contains(l.turn_id.as_str()))
        .map(|(i, _)| i)
        .collect();

    let kept = intents.records.len() - records.len();
    RetirePlan {
        records,
        labels,
        kept,
    }
}

/// The tombstones for a set of retired records.
pub fn tombstones_for(records: &[&IntentRecord]) -> Vec<Tombstone> {
    records
        .iter()
        .map(|r| Tombstone {
            tool_use_id: r.tool_use_id.clone(),
            content_key: content_key(r),
            path: normalise_path(&r.path),
        })
        .collect()
}

/// Drop mined records that were already retired, returning how many went.
///
/// Applied before sequence numbers are handed out, so a rejected record never
/// consumes one.
pub fn reject_tombstoned(records: &mut Vec<IntentRecord>, tombs: &[Tombstone]) -> usize {
    if tombs.is_empty() {
        return 0;
    }
    let index: HashSet<&Tombstone> = tombs.iter().collect();
    let before = records.len();
    records.retain(|r| {
        let candidate = Tombstone {
            tool_use_id: r.tool_use_id.clone(),
            content_key: content_key(r),
            path: normalise_path(&r.path),
        };
        !index.contains(&candidate)
    });
    before - records.len()
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// Read the prune state. A missing or corrupt file is an empty state, never an
/// error: a bad file must not stop the Changes tab opening.
pub fn load_state(root: &Path) -> PruneState {
    std::fs::read_to_string(state_path(root))
        .ok()
        .and_then(|raw| serde_json::from_str::<PruneState>(&raw).ok())
        .unwrap_or(PruneState {
            version: 1,
            last_head: None,
            high_seq: 0,
        })
}

fn save_state(root: &Path, state: &PruneState) -> Result<()> {
    let dir = intents::intents_dir(root);
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    crate::config::ensure_gitignore(&crate::config::config_dir(root))?;

    let path = state_path(root);
    let json =
        serde_json::to_string_pretty(state).context("failed to serialise the prune state")?;
    write_atomically(&path, &format!("{json}\n"))
}

/// Temp file plus rename, so a crash mid-write can never leave a half-file.
fn write_atomically(path: &Path, contents: &str) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);

    std::fs::write(&tmp, contents).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

/// Read the tombstones, skipping unparseable lines like every other store here.
pub fn load_tombstones(root: &Path) -> Vec<Tombstone> {
    let path = tombstone_path(root);
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Tombstone>(l).ok())
        .collect()
}

fn append_lines<T: Serialize>(path: &Path, values: &[T]) -> Result<()> {
    use std::io::Write;
    if values.is_empty() {
        return Ok(());
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;

    for value in values {
        let json = serde_json::to_string(value).context("failed to serialise a retired record")?;
        writeln!(file, "{json}").with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(())
}

/// A best-effort exclusive lock, released on drop.
struct PruneLock(PathBuf);

impl PruneLock {
    fn acquire(root: &Path) -> Option<PruneLock> {
        let path = lock_path(root);

        // Reclaim a lock left behind by a process that died mid-prune.
        if let Ok(meta) = std::fs::metadata(&path) {
            let stale = meta
                .modified()
                .ok()
                .and_then(|m| m.elapsed().ok())
                .map(|age| age.as_secs() > STALE_LOCK_SECS)
                .unwrap_or(false);
            if stale {
                let _ = std::fs::remove_file(&path);
            }
        }

        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .ok()
            .map(|_| PruneLock(path))
    }
}

impl Drop for PruneLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// ---------------------------------------------------------------------------
// Running it
// ---------------------------------------------------------------------------

/// Build the snapshots the plan needs for a set of paths.
///
/// Always `WorkingToHead`, and deliberately not reusing the diff the caller may
/// already hold: `intent_groups` computes its diff in whichever mode the user is
/// looking at, and deciding a retirement from the staged view would ask the
/// wrong question.
pub fn snapshot(repo: &Repo, _root: &Path, paths: &BTreeSet<String>) -> Result<Vec<FileSnapshot>> {
    let diffs = repo
        .diff_all(ComparisonMode::WorkingToHead)
        .unwrap_or_default();

    let mut out = Vec::with_capacity(paths.len());

    for path in paths {
        let diff = diffs.iter().find(|d| normalise_path(&d.path) == *path);

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut binary = false;
        if let Some(diff) = diff {
            binary = diff.is_binary;
            for hunk in &diff.hunks {
                for line in &hunk.lines {
                    match line.origin {
                        LineOrigin::Addition => added.push(line.content.clone()),
                        LineOrigin::Deletion => removed.push(line.content.clone()),
                        LineOrigin::Context => {}
                    }
                }
            }
        }

        let head_blob = repo
            .baseline_content(path, ComparisonMode::WorkingToHead)
            .ok()
            .flatten();

        out.push(FileSnapshot {
            path: path.clone(),
            head_blob,
            working_added: added,
            working_removed: removed,
            in_working_diff: diff.is_some(),
            readable: !binary,
        });
    }

    Ok(out)
}

/// Prune only if HEAD has moved since the last look.
///
/// The catch-all that makes a commit typed in a floating terminal, an amend, or
/// a rebase behave the same as one made through the app. On the very first run
/// there is no baseline, so "HEAD moved" is unknowable: record it and prune
/// nothing.
pub fn run_if_head_moved(repo: &Repo, root: &Path) -> Result<RetireSummary> {
    // A half-applied tree is not evidence of anything.
    let conflicted = repo
        .status()
        .map(|s| s.files.iter().any(|f| f.is_conflicted()))
        .unwrap_or(false);
    if conflicted {
        return Ok(RetireSummary::default());
    }

    let Ok(head) = repo.head_oid() else {
        // Unborn branch: no baseline yet, and nothing to absorb anything.
        return Ok(RetireSummary::default());
    };

    let state = load_state(root);
    if state.last_head.as_deref() == Some(head.as_str()) {
        return Ok(RetireSummary {
            head: Some(head),
            ..Default::default()
        });
    }

    if state.last_head.is_none() {
        // First look. Establish the baseline and touch nothing: the backlog is
        // cleaned by the explicit, previewed action instead.
        let mut next = state;
        next.last_head = Some(head.clone());
        next.high_seq = next.high_seq.max(intents::next_seq(root).saturating_sub(1));
        save_state(root, &next)?;
        return Ok(RetireSummary {
            head: Some(head),
            ..Default::default()
        });
    }

    execute(repo, root, Some(head))
}

/// Preview the backlog prune: the same decision, run without requiring HEAD to
/// have moved, writing nothing.
pub fn preview(repo: &Repo, root: &Path) -> Result<RetireSummary> {
    let head = repo.head_oid().ok();
    let intents = intents::load(root, &LoadOptions::default())?;
    let paths: BTreeSet<String> = intents
        .records
        .iter()
        .map(|r| normalise_path(&r.path))
        .collect();
    let snapshots = snapshot(repo, root, &paths)?;
    let outcome = plan(&intents, &snapshots);

    Ok(RetireSummary {
        records_retired: outcome.records.len(),
        labels_retired: outcome.labels.len(),
        kept_records: outcome.kept,
        head,
        pruned: false,
    })
}

/// Run the backlog prune now, whether or not HEAD moved. The confirmed half of
/// [`preview`].
pub fn run_now(repo: &Repo, root: &Path) -> Result<RetireSummary> {
    execute(repo, root, repo.head_oid().ok())
}

fn execute(repo: &Repo, root: &Path, head: Option<String>) -> Result<RetireSummary> {
    let Some(_lock) = PruneLock::acquire(root) else {
        // Another prune is mid-rewrite. Skipping is always safe: the same
        // verdicts recur next time.
        return Ok(RetireSummary {
            head,
            ..Default::default()
        });
    };

    let edits = intents::edits_path(root);
    let size_before = std::fs::metadata(&edits).map(|m| m.len()).unwrap_or(0);

    // Load unfiltered by branch: the archive is written from what is on disk, so
    // a branch filter here would rewrite the file without records it never saw.
    let intents_all = intents::load(root, &LoadOptions::default())?;
    let paths: BTreeSet<String> = intents_all
        .records
        .iter()
        .map(|r| normalise_path(&r.path))
        .collect();
    let snapshots = snapshot(repo, root, &paths)?;
    let outcome = plan(&intents_all, &snapshots);

    if outcome.records.is_empty() && outcome.labels.is_empty() {
        let mut state = load_state(root);
        state.last_head = head.clone();
        state.high_seq = state
            .high_seq
            .max(intents::next_seq(root).saturating_sub(1));
        save_state(root, &state)?;
        return Ok(RetireSummary {
            kept_records: outcome.kept,
            head,
            pruned: false,
            ..Default::default()
        });
    }

    let retiring: Vec<&IntentRecord> = outcome
        .records
        .iter()
        .map(|i| &intents_all.records[*i])
        .collect();
    let retiring_labels: Vec<&IntentLabel> = outcome
        .labels
        .iter()
        .map(|i| &intents_all.labels[*i])
        .collect();

    // Order matters: archive, then tombstones, then the rewrite. A crash between
    // steps leaves a superset archived and the original file intact, which is
    // recoverable; the other order loses records.
    let archived: Vec<ArchivedRecord> = retiring
        .iter()
        .map(|r| ArchivedRecord {
            record: (*r).clone(),
            retired_at_head: head.clone(),
        })
        .collect();
    append_lines(&archive_path(root), &archived)?;
    append_lines(&label_archive_path(root), &retiring_labels)?;
    append_lines(&tombstone_path(root), &tombstones_for(&retiring))?;

    // The high-water mark must outlive the records: `next_seq` is max+1 over the
    // file, so a prune that lowered the max would hand out colliding sequence
    // numbers and break "later edits win".
    let mut state = load_state(root);
    state.high_seq = state
        .high_seq
        .max(intents::next_seq(root).saturating_sub(1));

    let retired_ids: HashSet<&str> = retiring.iter().map(|r| r.tool_use_id.as_str()).collect();
    let retired_turns: HashSet<&str> = retiring_labels.iter().map(|l| l.turn_id.as_str()).collect();

    // If the hook appended while we were deciding, abandon: the next run redoes
    // the same work against a file we have actually read.
    let size_now = std::fs::metadata(&edits).map(|m| m.len()).unwrap_or(0);
    if size_now != size_before {
        return Ok(RetireSummary {
            kept_records: outcome.kept,
            head,
            pruned: false,
            ..Default::default()
        });
    }

    rewrite_jsonl(&edits, |line: &IntentRecord| {
        !retired_ids.contains(line.tool_use_id.as_str())
    })?;
    rewrite_jsonl(&intents::labels_path(root), |line: &IntentLabel| {
        !retired_turns.contains(line.turn_id.as_str())
    })?;

    state.last_head = head.clone();
    save_state(root, &state)?;

    Ok(RetireSummary {
        records_retired: retiring.len(),
        labels_retired: retiring_labels.len(),
        kept_records: outcome.kept,
        head,
        pruned: true,
    })
}

/// Rewrite a JSONL file keeping only the lines a predicate accepts. A line that
/// will not parse is kept — it is not ours to discard.
fn rewrite_jsonl<T, F>(path: &Path, keep: F) -> Result<()>
where
    T: for<'de> Deserialize<'de>,
    F: Fn(&T) -> bool,
{
    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let mut kept = String::with_capacity(content.len());
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let drop = serde_json::from_str::<T>(trimmed)
            .ok()
            .map(|parsed| !keep(&parsed))
            .unwrap_or(false);
        if !drop {
            kept.push_str(line);
            kept.push('\n');
        }
    }

    write_atomically(path, &kept)
}

/// Forget the retirement bookkeeping too. Called by [`intents::clear`], because
/// "forget everything" that left tombstones behind would make a clear-then-import
/// silently return nothing.
pub fn clear(root: &Path) -> Result<()> {
    for path in [
        archive_path(root),
        label_archive_path(root),
        tombstone_path(root),
        state_path(root),
    ] {
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "retire_tests.rs"]
mod tests;
