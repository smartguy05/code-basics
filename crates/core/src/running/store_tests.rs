use super::super::record::RunKind;
use super::*;

fn rec(root: &str, key: &str, pid: u32, label: &str) -> RunningRecord {
    RunningRecord {
        pid,
        kind: RunKind::Terminal,
        label: label.into(),
        root: root.into(),
        key: key.into(),
        program: "node.exe".into(),
        started_at_ms: 1000,
    }
}

fn temp_path() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("running.json");
    (dir, path)
}

#[test]
fn record_then_live_lists_it_and_persists() {
    let (_dir, path) = temp_path();
    let store = RunningStore::new(path.clone());
    store.record(rec("/ws", "sess-1", 10, "Server"));
    assert_eq!(store.live().len(), 1);
    // Persisted to disk.
    assert_eq!(load(&path).records.len(), 1);
}

#[test]
fn remove_drops_the_matching_entry_only() {
    let (_dir, path) = temp_path();
    let store = RunningStore::new(path);
    store.record(rec("/ws", "sess-1", 10, "A"));
    store.record(rec("/ws", "sess-2", 11, "B"));
    store.remove("/ws", "sess-1");
    let live = store.live();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].key, "sess-2");
}

#[test]
fn the_same_key_under_two_roots_does_not_collide() {
    let (_dir, path) = temp_path();
    let store = RunningStore::new(path);
    store.record(rec("/a", "cfg", 10, "A"));
    store.record(rec("/b", "cfg", 11, "B"));
    assert_eq!(
        store.live().len(),
        2,
        "same config id in two codebases stays distinct"
    );
}

#[test]
fn update_label_changes_a_live_record() {
    let (_dir, path) = temp_path();
    let store = RunningStore::new(path);
    store.record(rec("/ws", "sess-1", 10, "Terminal 1"));
    store.update_label("/ws", "sess-1", "Server");
    assert_eq!(store.live()[0].label, "Server");
}

#[test]
fn live_is_sorted_by_root_then_label() {
    let (_dir, path) = temp_path();
    let store = RunningStore::new(path);
    store.record(rec("/b", "k1", 1, "Zed"));
    store.record(rec("/a", "k2", 2, "Yak"));
    store.record(rec("/a", "k3", 3, "Ant"));
    let labels: Vec<_> = store.live().into_iter().map(|r| r.label).collect();
    assert_eq!(labels, ["Ant", "Yak", "Zed"]);
}

#[test]
fn load_orphans_keeps_alive_matching_records_and_prunes_the_rest() {
    let (_dir, path) = temp_path();
    // Seed a file with three records: one alive+matching, one dead, one reused.
    let seed = RunningStore::new(path.clone());
    seed.record(rec("/ws", "alive", 10, "Alive"));
    seed.record(rec("/ws", "dead", 11, "Dead"));
    seed.record(rec("/ws", "reused", 12, "Reused"));

    // Probe: 10 matches, 11 is gone, 12 is a different program.
    let probe = |pid: u32| match pid {
        10 => Some(ProcInfo {
            name: "node.exe".into(),
            started_at_ms: 1000,
        }),
        12 => Some(ProcInfo {
            name: "chrome.exe".into(),
            started_at_ms: 500_000,
        }),
        _ => None,
    };
    // A fresh handle over the same path, as the app does at startup.
    let store = RunningStore::new(path.clone());
    store.load_orphans(probe);

    let orphans = store.orphans();
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].key, "alive");
    assert!(store.live().is_empty(), "live starts empty at load");
    assert_eq!(store.warnings().len(), 1, "the reused pid is reported");
    // The rewritten file holds only the surviving orphan.
    assert_eq!(load(&path).records.len(), 1);
}

#[test]
fn resolve_orphan_removes_it() {
    let (_dir, path) = temp_path();
    let seed = RunningStore::new(path.clone());
    seed.record(rec("/ws", "a", 10, "A"));
    let store = RunningStore::new(path);
    store.load_orphans(|_| {
        Some(ProcInfo {
            name: "node.exe".into(),
            started_at_ms: 1000,
        })
    });
    assert_eq!(store.orphans().len(), 1);
    store.resolve_orphan(10);
    assert!(store.orphans().is_empty());
}
