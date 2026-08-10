//! Tests for the pre-commit guard that keeps rejection notes out of history.
//! Included by `guard.rs` under `#[cfg(test)]`.

use super::*;

fn hook_file() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pre-commit");
    (dir, path)
}

#[test]
fn a_repository_with_no_hook_gets_one_created() {
    let (_dir, path) = hook_file();

    let write = plan_for(&path).expect("a write");

    assert!(!write.merges_existing);
    assert!(write.content.starts_with("#!/bin/sh\n"));
    assert!(write.content.contains(BEGIN));
    assert!(write.content.contains(END));
}

/// The hook may be the user's own, doing work of its own.
#[test]
fn an_existing_hook_keeps_everything_it_had() {
    let (_dir, path) = hook_file();
    std::fs::write(&path, "#!/bin/sh\nnpm run lint || exit 1\n").unwrap();

    let write = plan_for(&path).expect("a write");

    assert!(write.merges_existing);
    assert!(write.content.contains("npm run lint || exit 1"));
    assert!(write.content.contains(BEGIN));
    // One shebang, still at the top.
    assert!(write.content.starts_with("#!/bin/sh\n"));
    assert_eq!(write.content.matches("#!/bin/sh").count(), 1);
}

#[test]
fn installing_twice_does_not_add_the_block_twice() {
    let (_dir, path) = hook_file();
    let first = plan_for(&path).expect("a write");
    std::fs::write(&path, &first.content).unwrap();

    assert!(plan_for(&path).is_none(), "the block is already current");
    assert!(is_installed(&path));
}

/// Re-running setup after the guard's wording changed must bring the hook up to
/// date without disturbing the user's own lines around it.
#[test]
fn an_out_of_date_block_is_replaced_in_place() {
    let (_dir, path) = hook_file();
    std::fs::write(
        &path,
        format!("#!/bin/sh\nmy-own-check\n{BEGIN}\nstale guard\n{END}\ntrailing-check\n"),
    )
    .unwrap();

    let write = plan_for(&path).expect("a rewrite");

    assert!(write.merges_existing);
    assert!(write.content.contains("my-own-check"));
    assert!(write.content.contains("trailing-check"));
    assert!(!write.content.contains("stale guard"));
    assert_eq!(write.content.matches(BEGIN).count(), 1);
}

/// A start with no end bounds nothing; rewriting would mean guessing where the
/// user's own script resumes. Same rule as the instruction file.
#[test]
fn a_block_without_its_end_is_left_alone() {
    let (_dir, path) = hook_file();
    std::fs::write(&path, format!("#!/bin/sh\n{BEGIN}\nhalf a block\n")).unwrap();

    assert!(plan_for(&path).is_none());
}

/// The whole point of the guard is to grep staged files for the marker. If the
/// hook itself contained the token literally, then a repository using a
/// *tracked* hooks directory (`core.hooksPath = .githooks`) would block every
/// commit that staged the hook — the guard would flag itself.
#[test]
fn the_guard_script_never_contains_the_token_it_searches_for() {
    assert!(
        !block().contains(crate::intents::reject::MARKER),
        "the guard must assemble the token at runtime, not embed it"
    );
    // It must still tell the user what to look for.
    assert!(block().contains("cb_marker"));
}

#[test]
fn the_guard_names_its_own_escape_hatch() {
    // A guard with no way past it gets deleted wholesale the first time it is
    // wrong, so it has to say how to override it.
    assert!(block().contains("CB_ALLOW_REJECTED"));
}

#[test]
fn removal_takes_the_block_out_and_leaves_the_rest() {
    let (_dir, path) = hook_file();
    std::fs::write(
        &path,
        format!("#!/bin/sh\nmy-own-check\n{BEGIN}\nguard body\n{END}\n"),
    )
    .unwrap();

    let content = plan_removal(&path).unwrap().expect("a removal");

    assert!(content.contains("my-own-check"));
    assert!(!content.contains(BEGIN));
    assert!(!content.contains("guard body"));
}

#[test]
fn there_is_nothing_to_remove_from_a_hook_we_never_touched() {
    let (_dir, path) = hook_file();
    std::fs::write(&path, "#!/bin/sh\nmy-own-check\n").unwrap();

    assert!(plan_removal(&path).unwrap().is_none());
}

#[test]
fn a_missing_hook_is_not_installed_and_needs_no_removal() {
    let (_dir, path) = hook_file();

    assert!(!is_installed(&path));
    assert!(plan_removal(&path).unwrap().is_none());
}
