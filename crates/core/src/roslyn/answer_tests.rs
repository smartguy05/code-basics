use super::*;

#[test]
fn every_refusal_has_a_distinct_code_and_a_distinct_sentence() {
    let refusals = [
        RoslynRefusal::NoWorkspace,
        RoslynRefusal::NoSession,
        RoslynRefusal::NotConfigured,
        RoslynRefusal::Starting,
        RoslynRefusal::Loading,
        RoslynRefusal::Failed,
        RoslynRefusal::Unsupported,
        RoslynRefusal::BadPath,
    ];
    let codes: std::collections::BTreeSet<&str> = refusals.iter().map(|r| r.code()).collect();
    assert_eq!(codes.len(), 8, "a shared code would pass every other test");
    let sentences: std::collections::BTreeSet<String> =
        refusals.iter().map(|r| r.sentence()).collect();
    assert_eq!(sentences.len(), 8);
    for refusal in refusals {
        assert!(!refusal.sentence().is_empty(), "{refusal:?}");
    }
}

#[test]
fn no_workspace_and_no_session_are_not_the_same_answer() {
    // One is fixed by reinstalling the server scoped to a repo, the other by
    // reopening the repo. Collapsing them sends the reader to the wrong fix.
    assert_ne!(
        RoslynRefusal::NoWorkspace.code(),
        RoslynRefusal::NoSession.code()
    );
    assert_ne!(
        RoslynRefusal::NoWorkspace.sentence(),
        RoslynRefusal::NoSession.sentence()
    );
}

#[test]
fn ready_is_not_a_refusal() {
    assert_eq!(RoslynRefusal::from_availability(Availability::Ready), None);
}

#[test]
fn each_non_ready_availability_maps_to_its_own_refusal() {
    let cases = [
        (Availability::NotConfigured, RoslynRefusal::NotConfigured),
        (Availability::Starting, RoslynRefusal::Starting),
        (Availability::Loading, RoslynRefusal::Loading),
        (Availability::Failed, RoslynRefusal::Failed),
        (Availability::Unsupported, RoslynRefusal::Unsupported),
    ];
    for (availability, expected) in cases {
        assert_eq!(
            RoslynRefusal::from_availability(availability),
            Some(expected),
            "{availability:?}"
        );
    }
    // And the five derived codes are all distinct.
    let derived: std::collections::BTreeSet<&str> =
        cases.iter().map(|(_, refusal)| refusal.code()).collect();
    assert_eq!(derived.len(), 5);
}

#[test]
fn unsupported_is_not_a_failure_and_says_so() {
    // The sharpest collapse this subsystem refuses: a capability the server does
    // not offer is not a broken server and not a genuine empty.
    let sentence = RoslynRefusal::Unsupported.sentence();
    assert!(sentence.contains("not a failure"), "{sentence}");
    assert_ne!(
        RoslynRefusal::Unsupported.code(),
        RoslynRefusal::Failed.code()
    );
}

#[test]
fn no_sentence_forwards_internal_error_text() {
    // Generic by design — the specific reason is the result's own message,
    // appended by render, never fabricated here.
    for refusal in [
        RoslynRefusal::NoWorkspace,
        RoslynRefusal::NoSession,
        RoslynRefusal::NotConfigured,
        RoslynRefusal::Starting,
        RoslynRefusal::Loading,
        RoslynRefusal::Failed,
        RoslynRefusal::Unsupported,
        RoslynRefusal::BadPath,
    ] {
        let sentence = refusal.sentence();
        assert!(!sentence.contains(".rs"), "{sentence}");
        assert!(!sentence.contains("os error"), "{sentence}");
        assert!(!sentence.contains(r"\\"), "{sentence}");
    }
}

#[test]
fn a_path_that_could_escape_the_workspace_root_is_refused() {
    // The `--workspace` scope is this server's whole consent boundary, so a tool
    // path that is absolute, prefixed, or climbs with `..` must be refused before
    // any file is joined onto the root — an agent scoped to repo X must not reach
    // files outside X.
    for ok in ["src/App.cs", "src/sub/App.cs", "./src/App.cs", "App.cs"] {
        assert!(path_within_root(ok), "{ok} should be allowed");
    }
    for bad in [
        "../outside.cs",
        "src/../../outside.cs",
        r"..\outside.cs",
        r"C:\Windows\System32\x.cs",
        "C:/Windows/x.cs",
        r"\\server\share\x.cs",
        "/etc/passwd",
    ] {
        assert!(!path_within_root(bad), "{bad} should be refused");
    }
}
