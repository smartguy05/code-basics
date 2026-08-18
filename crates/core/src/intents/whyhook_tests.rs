//! Tests for the durable-why post-commit hook installer.
//! Included by `whyhook.rs` under `#[cfg(test)]`.

use super::*;
use std::path::Path;

const EXE: &str = "/apps/cb-app";

fn root() -> &'static Path {
    Path::new("/repo")
}

#[test]
fn the_block_invokes_the_recorder_for_post_commit() {
    let text = block(EXE, root());
    assert!(text.contains(BEGIN) && text.contains(END));
    assert!(text.contains("record-intent"));
    assert!(text.contains("--event PostCommit"));
    assert!(text.contains(&format!("--{MARKER}")));
    assert!(text.contains("--workspace \"/repo\""));
    // It must never fail the commit.
    assert!(text.contains("|| true"));
}

#[test]
fn a_missing_hook_is_created_with_a_shebang() {
    let dir = tempfile::tempdir().unwrap();
    let hook = dir.path().join("post-commit");

    let write = plan_for(&hook, EXE, root()).expect("a write");

    assert!(!write.merges_existing);
    assert!(write.content.starts_with("#!/bin/sh"));
    assert!(write.content.contains(BEGIN));
}

#[test]
fn an_existing_hook_is_appended_to_not_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let hook = dir.path().join("post-commit");
    std::fs::write(&hook, "#!/bin/sh\necho existing\n").unwrap();

    let write = plan_for(&hook, EXE, root()).expect("a write");

    assert!(write.merges_existing);
    assert!(write.content.contains("echo existing"), "existing script kept");
    assert!(write.content.contains(BEGIN));
}

#[test]
fn re_installing_an_unchanged_block_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let hook = dir.path().join("post-commit");
    let first = plan_for(&hook, EXE, root()).expect("a write");
    std::fs::write(&hook, &first.content).unwrap();

    assert!(is_installed(&hook));
    assert!(plan_for(&hook, EXE, root()).is_none());
}

#[test]
fn removal_takes_only_our_block_out() {
    let dir = tempfile::tempdir().unwrap();
    let hook = dir.path().join("post-commit");
    std::fs::write(&hook, "#!/bin/sh\necho existing\n").unwrap();
    let write = plan_for(&hook, EXE, root()).expect("a write");
    std::fs::write(&hook, &write.content).unwrap();

    let removed = plan_removal(&hook).unwrap().expect("something removed");

    assert!(removed.contains("echo existing"));
    assert!(!removed.contains(BEGIN));
}

#[test]
fn a_block_with_no_end_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let hook = dir.path().join("post-commit");
    std::fs::write(&hook, format!("#!/bin/sh\n{BEGIN}\nhalf a block, no end\n")).unwrap();

    // Nothing to safely rewrite, so the plan abstains.
    assert!(plan_for(&hook, EXE, root()).is_none());
}
