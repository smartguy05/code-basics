use super::*;
use std::path::Path;

#[test]
fn accepts_ordinary_and_namespaced_names() {
    assert!(validate_branch_name("feature-x").is_ok());
    assert!(validate_branch_name("feature/login").is_ok());
    assert!(validate_branch_name("fix/issue-123").is_ok());
}

#[test]
fn rejects_empty_and_whitespace() {
    assert!(validate_branch_name("").is_err());
    assert!(validate_branch_name("   ").is_err());
    assert!(validate_branch_name("has space").is_err());
    assert!(validate_branch_name(" leading").is_err());
    assert!(validate_branch_name("trailing ").is_err());
}

#[test]
fn rejects_git_illegal_characters_and_sequences() {
    for bad in ["~", "^", ":", "?", "*", "[", "\\"] {
        assert!(
            validate_branch_name(&format!("a{bad}b")).is_err(),
            "expected {bad} to be rejected"
        );
    }
    assert!(validate_branch_name("-lead").is_err());
    assert!(validate_branch_name("/lead").is_err());
    assert!(validate_branch_name("trail/").is_err());
    assert!(validate_branch_name("a..b").is_err());
    assert!(validate_branch_name("a//b").is_err());
    assert!(validate_branch_name("a@{b").is_err());
    assert!(validate_branch_name("branch.lock").is_err());
}

#[test]
fn sanitizes_separators_into_one_segment() {
    assert_eq!(sanitize_dir_segment("feature/login"), "feature-login");
    assert_eq!(sanitize_dir_segment("a/b/c"), "a-b-c");
    assert_eq!(sanitize_dir_segment("plain"), "plain");
}

#[test]
fn default_dir_is_a_sibling_of_the_repo() {
    let dir = default_worktree_dir(Path::new("/home/me/code/myrepo"), "feature/x");
    assert_eq!(dir, Path::new("/home/me/code/myrepo.worktrees/feature-x"));
}

#[test]
fn default_dir_tolerates_a_repo_at_a_root() {
    // No parent: falls back to a `.worktrees` inside the repo rather than failing.
    let dir = default_worktree_dir(Path::new("/"), "x");
    assert!(dir.ends_with("x"));
    assert!(dir.to_string_lossy().contains(".worktrees"));
}
