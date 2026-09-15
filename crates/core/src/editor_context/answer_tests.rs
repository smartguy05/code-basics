use super::*;

#[test]
fn every_refusal_has_a_distinct_code_and_a_distinct_sentence() {
    let refusals = [
        EditorRefusal::NoWorkspace,
        EditorRefusal::Disabled,
        EditorRefusal::NoContext,
        EditorRefusal::NoActiveFile,
    ];
    let codes: std::collections::BTreeSet<&str> = refusals.iter().map(|r| r.code()).collect();
    assert_eq!(codes.len(), 4, "a shared code would pass every other test");
    let sentences: std::collections::BTreeSet<String> =
        refusals.iter().map(|r| r.sentence()).collect();
    assert_eq!(sentences.len(), 4);
    for refusal in refusals {
        assert!(!refusal.sentence().is_empty(), "{refusal:?}");
    }
}

#[test]
fn no_workspace_and_disabled_and_no_context_are_three_different_answers() {
    // One is fixed by reinstalling scoped to a repo, one by switching the
    // feature on, one by opening a file. Collapsing any two sends the reader to
    // the wrong fix — the failure this subsystem refuses.
    let three = [
        EditorRefusal::NoWorkspace,
        EditorRefusal::Disabled,
        EditorRefusal::NoContext,
    ];
    let codes: std::collections::BTreeSet<&str> = three.iter().map(|r| r.code()).collect();
    assert_eq!(codes.len(), 3);
    let sentences: std::collections::BTreeSet<String> =
        three.iter().map(|r| r.sentence()).collect();
    assert_eq!(sentences.len(), 3);
}

#[test]
fn no_active_file_states_it_is_the_complete_answer_not_a_truncation() {
    let sentence = EditorRefusal::NoActiveFile.sentence();
    assert!(sentence.contains("complete answer"), "{sentence}");
    assert!(sentence.contains("nothing was fabricated"), "{sentence}");
}

#[test]
fn no_sentence_forwards_internal_error_text() {
    for refusal in [
        EditorRefusal::NoWorkspace,
        EditorRefusal::Disabled,
        EditorRefusal::NoContext,
        EditorRefusal::NoActiveFile,
    ] {
        let sentence = refusal.sentence();
        assert!(!sentence.contains(".rs"), "{sentence}");
        assert!(!sentence.contains("os error"), "{sentence}");
        assert!(!sentence.contains(r"\\"), "{sentence}");
    }
}
