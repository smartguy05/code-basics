use super::*;

#[test]
fn every_refusal_has_a_distinct_code() {
    let codes = [
        McpRefusal::NoWorkspace.code(),
        McpRefusal::TaskNotFound { id: "t1".into() }.code(),
        McpRefusal::StoreWriteFailed.code(),
    ];
    let mut unique = codes.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), codes.len(), "codes must be distinct");
}

#[test]
fn not_found_names_the_id_it_was_given() {
    let sentence = McpRefusal::TaskNotFound {
        id: "abc-123".into(),
    }
    .sentence();
    assert!(sentence.contains("abc-123"));
}

#[test]
fn a_write_failure_leaks_no_filesystem_detail() {
    // The whole point of the variant: it carries nothing, so nothing can leak.
    let sentence = McpRefusal::StoreWriteFailed.sentence();
    assert!(!sentence.contains('/'));
    assert!(!sentence.contains('\\'));
    assert!(!sentence.to_lowercase().contains("os error"));
}

#[test]
fn no_workspace_and_not_found_are_different_answers() {
    assert_ne!(
        McpRefusal::NoWorkspace.code(),
        McpRefusal::TaskNotFound { id: "x".into() }.code()
    );
}
