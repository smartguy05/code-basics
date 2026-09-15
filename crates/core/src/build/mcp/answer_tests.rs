use super::*;

#[test]
fn the_five_refusals_have_five_codes_and_five_sentences() {
    let refusals = [
        BuildRefusal::NoWorkspace,
        BuildRefusal::Ambiguous,
        BuildRefusal::NeverBuilt,
        BuildRefusal::BuildFailedToStart,
        BuildRefusal::Disabled,
    ];
    let codes: std::collections::BTreeSet<&str> = refusals.iter().map(|r| r.code()).collect();
    assert_eq!(codes.len(), 5, "a shared code would pass every other test");
    let sentences: std::collections::BTreeSet<String> =
        refusals.iter().map(|r| r.sentence()).collect();
    assert_eq!(sentences.len(), 5);
    for refusal in &refusals {
        assert!(
            !refusal.sentence().is_empty(),
            "{refusal:?} must explain itself"
        );
    }
}

#[test]
fn never_built_and_failed_to_start_are_different_answers() {
    // One means "run a build first"; the other means "the build could not even
    // start". Collapsing them is the failure this codebase refuses.
    assert_ne!(
        BuildRefusal::NeverBuilt.code(),
        BuildRefusal::BuildFailedToStart.code()
    );
    assert_ne!(
        BuildRefusal::NeverBuilt.sentence(),
        BuildRefusal::BuildFailedToStart.sentence()
    );
}

#[test]
fn a_read_tool_refuses_never_built_and_could_not_start_but_not_the_running_states() {
    assert_eq!(
        BuildRefusal::from_status(BuildStatus::NeverBuilt),
        Some(BuildRefusal::NeverBuilt)
    );
    assert_eq!(
        BuildRefusal::from_status(BuildStatus::CouldNotStart),
        Some(BuildRefusal::BuildFailedToStart)
    );
    // A build in progress is a real state, not a refusal.
    assert_eq!(BuildRefusal::from_status(BuildStatus::Building), None);
    assert_eq!(BuildRefusal::from_status(BuildStatus::SucceededClean), None);
    assert_eq!(
        BuildRefusal::from_status(BuildStatus::SucceededWithWarnings),
        None
    );
    assert_eq!(BuildRefusal::from_status(BuildStatus::Failed), None);
}

#[test]
fn the_disabled_refusal_uses_the_shared_tool_gate_code() {
    assert_eq!(
        BuildRefusal::Disabled.code(),
        crate::tool_gate::DISABLED_CODE
    );
}

#[test]
fn no_sentence_leaks_an_internal_reason() {
    // The build-failed-to-start sentence must not pretend to know why.
    let sentence = BuildRefusal::BuildFailedToStart.sentence();
    assert!(sentence.contains("could not be started"), "{sentence}");
    assert!(sentence.contains("not forwarded"), "{sentence}");
}
