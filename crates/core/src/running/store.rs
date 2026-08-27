//! The registry itself: a clone-cheap handle over the live process set and the
//! orphan set, write-through-persisted to a user-global `running.json`.
//!
//! Clone-cheap and shareable like `Supervisor`/`PtyManager`, so one instance is
//! injected into every per-slot supervisor, the global supervisor and the PTY
//! manager, and they all record into the same table. Persistence mirrors
//! `notes.rs`: user-global path, atomic temp+rename, tolerant load — the panel
//! spans every open codebase, so the file is not `.code-basics/` per-workspace.
//!
//! The file holds live and orphan records together; the split is only in memory.
//! On the next launch every record is a candidate orphan to be re-probed, so the
//! distinction does not need to survive to disk.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::classify::{classify_orphans, ProcInfo};
use super::record::{RunningFile, RunningRecord};

/// Handle to the running-process registry. Cheap to clone; clones share state.
#[derive(Clone)]
pub struct RunningStore {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    path: PathBuf,
    /// Currently-running processes this session, keyed by `(root, key)`.
    live: HashMap<String, RunningRecord>,
    /// Records reloaded from a previous session whose pid is still alive and whose
    /// identity matched — offered for a kill until resolved.
    orphans: Vec<RunningRecord>,
    /// Notes from the last orphan classification (a pid reused by a different
    /// process), surfaced in the Running panel.
    warnings: Vec<String>,
}

/// The composite table key. A config id is only unique within a workspace, so the
/// root disambiguates two open codebases with the same layout. A NUL separator
/// can never appear in a path or a config id.
fn entry_key(root: &str, key: &str) -> String {
    format!("{root}\u{0}{key}")
}

impl RunningStore {
    /// An empty store persisting to `path`, with **no** reload and no disk write —
    /// so constructing one (including in every `AppState::default`) has no side
    /// effect. Orphan detection is a separate, explicit [`RunningStore::load_orphans`]
    /// step run once at app startup, which keeps this handle shareable: the same
    /// store is injected into every supervisor and the PTY manager before startup
    /// populates its orphan set in place.
    pub fn new(path: PathBuf) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                path,
                live: HashMap::new(),
                orphans: Vec::new(),
                warnings: Vec::new(),
            })),
        }
    }

    /// Reload the previous session's records, classify them into orphans with
    /// `probe`, and rewrite the file to the survivors. Mutates this shared handle
    /// in place (rather than returning a new store) so every supervisor already
    /// holding a clone sees the result. Run once at startup, before anything is
    /// spawned, so a record still present is genuinely from a prior session.
    ///
    /// `probe` is injected so the store is testable without sysinfo; the real
    /// caller passes [`super::probe::probe`].
    pub fn load_orphans(&self, probe: impl Fn(u32) -> Option<ProcInfo>) {
        let file = load(&self.lock().path);
        let probed = file
            .records
            .into_iter()
            .map(|r| {
                let info = probe(r.pid);
                (r, info)
            })
            .collect();
        let classified = classify_orphans(probed);
        {
            let mut inner = self.lock();
            inner.orphans = classified.orphans;
            inner.warnings = classified.warnings;
        }
        self.persist();
    }

    /// Record a freshly spawned process. Replacing an existing `(root, key)` is
    /// fine — a restart reuses the id and the newer pid wins.
    pub fn record(&self, record: RunningRecord) {
        {
            let mut inner = self.lock();
            inner
                .live
                .insert(entry_key(&record.root, &record.key), record);
        }
        self.persist();
    }

    /// Drop a live process by its `(root, key)` — called when it is reaped.
    pub fn remove(&self, root: &str, key: &str) {
        {
            let mut inner = self.lock();
            inner.live.remove(&entry_key(root, key));
        }
        self.persist();
    }

    /// Update a live process's label (a terminal renamed after it opened).
    pub fn update_label(&self, root: &str, key: &str, label: &str) {
        {
            let mut inner = self.lock();
            if let Some(record) = inner.live.get_mut(&entry_key(root, key)) {
                record.label = label.to_string();
            }
        }
        self.persist();
    }

    /// Drop an orphan by pid — called once it has been killed or has exited.
    pub fn resolve_orphan(&self, pid: u32) {
        {
            let mut inner = self.lock();
            inner.orphans.retain(|r| r.pid != pid);
        }
        self.persist();
    }

    /// The live processes, sorted by root then label for a stable listing.
    pub fn live(&self) -> Vec<RunningRecord> {
        let inner = self.lock();
        let mut out: Vec<RunningRecord> = inner.live.values().cloned().collect();
        out.sort_by(|a, b| a.root.cmp(&b.root).then(a.label.cmp(&b.label)));
        out
    }

    /// The orphan candidates from a previous session.
    pub fn orphans(&self) -> Vec<RunningRecord> {
        self.lock().orphans.clone()
    }

    /// Notes from the last orphan classification, for the panel to show.
    pub fn warnings(&self) -> Vec<String> {
        self.lock().warnings.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Write live ∪ orphans to disk. Best-effort: a failed write only means a
    /// staler crash-recovery snapshot, never a failed operation.
    fn persist(&self) {
        let inner = self.lock();
        let mut records: Vec<RunningRecord> = inner.live.values().cloned().collect();
        records.extend(inner.orphans.iter().cloned());
        let file = RunningFile {
            version: 1,
            records,
        };
        let _ = save(&inner.path, &file);
    }
}

/// Read the registry file. A missing or corrupt file yields an empty set rather
/// than an error — the registry is a convenience and must never block the app.
pub fn load(path: &Path) -> RunningFile {
    let Ok(text) = std::fs::read_to_string(path) else {
        return RunningFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// Atomic write (temp + rename), mirroring `notes::save`, so a crash mid-write
/// cannot leave a truncated file that would read back empty.
pub fn save(path: &Path, file: &RunningFile) -> anyhow::Result<()> {
    use anyhow::Context;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(file).context("failed to serialise running set")?;
    let tmp = sibling(path, ".tmp");
    std::fs::write(&tmp, format!("{json}\n"))
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

/// Where the registry file lives: `<config>/code-basics/running.json`, beside the
/// notes store. `CB_RUNNING_PATH` overrides the whole path (used under
/// `pnpm tauri dev` and by tests), the same escape hatch `notes_path` offers.
pub fn running_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CB_RUNNING_PATH") {
        return PathBuf::from(path);
    }
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("code-basics").join("running.json")
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
