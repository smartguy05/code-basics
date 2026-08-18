use super::*;
use std::fs;
use std::path::Path;

/// A fresh repository with a single commit; returns (tempdir, root, HEAD oid).
fn init_repo() -> (tempfile::TempDir, PathBuf, String) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let repo = git2::Repository::init(&root).unwrap();
    fs::write(root.join("a.txt"), "hello\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let oid = repo
        .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();
    (tmp, root, oid.to_string())
}

#[test]
fn teardown_removes_dir() {
    let (_tmp, root, oid) = init_repo();
    let wt = BaselineWorktree::create(&root, &oid, &WorktreeOptions::default()).unwrap();
    let path = wt.path().to_path_buf();
    assert!(path.exists() && is_valid_worktree(&path));
    assert!(!wt.adopted(), "a fresh create is not a cache hit");

    let warnings = wt.finish();
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert!(!path.exists(), "the checkout should be gone after finish()");
}

#[test]
fn teardown_is_idempotent() {
    let (_tmp, root, oid) = init_repo();
    let wt = BaselineWorktree::create(&root, &oid, &WorktreeOptions::default()).unwrap();
    let path = wt.path().to_path_buf();
    let _ = wt.finish();

    // Tearing down an already-removed checkout must not error or panic.
    assert!(teardown(&root, &path).is_empty());
    assert!(teardown(&root, &path).is_empty());
    assert!(!path.exists());
}

#[test]
fn drop_removes_an_unkept_checkout() {
    let (_tmp, root, oid) = init_repo();
    let path = {
        let wt = BaselineWorktree::create(&root, &oid, &WorktreeOptions::default()).unwrap();
        wt.path().to_path_buf()
        // wt dropped here without keep_for_reuse
    };
    assert!(!path.exists(), "drop should clean up a checkout nobody kept");
}

#[test]
fn create_is_cache_hit_at_same_oid() {
    let (_tmp, root, oid) = init_repo();

    let mut wt = BaselineWorktree::create(&root, &oid, &WorktreeOptions::default()).unwrap();
    assert!(!wt.adopted());
    wt.keep_for_reuse();
    let path = wt.path().to_path_buf();
    drop(wt); // kept — must survive
    assert!(path.exists(), "a kept baseline should survive drop");

    // A second create at the same oid reuses the cached checkout.
    let wt2 = BaselineWorktree::create(&root, &oid, &WorktreeOptions::default()).unwrap();
    assert!(wt2.adopted(), "second create at the same oid should be a cache hit");
    assert_eq!(wt2.path(), path);
    // An adopted checkout is kept, so finish() leaves it in place for clear_all.
    assert!(wt2.finish().is_empty());
    assert!(path.exists(), "an adopted (kept) checkout is not torn down by finish()");

    let warnings = clear_all(&root);
    assert!(warnings.is_empty(), "clear_all warnings: {warnings:?}");
    assert!(!path.exists(), "clear_all should remove the whole cache");
}
