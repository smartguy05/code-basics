//! Console level ranking, and the abstention. Included by `console.rs`.

use super::*;

#[test]
fn the_known_methods_map_to_their_own_levels() {
    assert_eq!(classify_level("debug"), ConsoleLevel::Debug);
    assert_eq!(classify_level("log"), ConsoleLevel::Log);
    assert_eq!(classify_level("info"), ConsoleLevel::Info);
    assert_eq!(classify_level("warn"), ConsoleLevel::Warn);
    assert_eq!(classify_level("error"), ConsoleLevel::Error);
}

#[test]
fn the_dom_spelling_of_warning_is_also_a_warning() {
    assert_eq!(classify_level("warning"), ConsoleLevel::Warn);
}

#[test]
fn an_unknown_console_method_is_other_not_error() {
    // The governing rule. Ranking these as errors would put a red row in front
    // of the user for a call that printed a table, and would make an agent
    // report "the page logged an error" about `console.group`.
    for method in [
        "table",
        "group",
        "groupEnd",
        "dir",
        "dirxml",
        "count",
        "countReset",
        "time",
        "timeEnd",
        "timeLog",
        "assert",
        "profile",
        "trace",
        "clear",
    ] {
        assert_eq!(
            classify_level(method),
            ConsoleLevel::Other,
            "console.{method} must not be ranked"
        );
    }
}

#[test]
fn an_unknown_method_is_not_ranked_as_log_either() {
    // The opposite guess, which would hide a method that really was severe.
    // `Other` is the only answer that claims nothing.
    assert_ne!(
        classify_level("somethingAFrameworkAdded"),
        ConsoleLevel::Log
    );
    assert_eq!(
        classify_level("somethingAFrameworkAdded"),
        ConsoleLevel::Other
    );
}

#[test]
fn a_method_name_a_page_invented_is_other() {
    // The set arriving here is open: a page may add anything to `console`.
    for method in [
        "",
        "   ",
        "🙂",
        "prototype",
        "constructor",
        "__proto__",
        "0",
    ] {
        assert_eq!(classify_level(method), ConsoleLevel::Other, "{method:?}");
    }
}

#[test]
fn a_method_name_that_merely_contains_error_is_not_an_error() {
    // A substring test would be the tempting shortcut and would rank a page's
    // own `errorBoundaryDebug` helper as a page error.
    for method in ["errorBoundaryDebug", "logError", "reportErrors", "erroring"] {
        assert_eq!(classify_level(method), ConsoleLevel::Other, "{method}");
    }
}

#[test]
fn classification_survives_casing_and_padding_from_the_page() {
    // The name arrives as a string from a page-side shim, so its shape is not
    // guaranteed. Nothing else is inferred from it.
    assert_eq!(classify_level("WARN"), ConsoleLevel::Warn);
    assert_eq!(classify_level(" Error "), ConsoleLevel::Error);
    assert_eq!(classify_level("\tinfo\n"), ConsoleLevel::Info);
}

#[test]
fn the_window_error_events_are_not_console_methods() {
    // They are captured, and their level is `Error` by construction of the
    // message kind in `ipc`, because the page genuinely threw. Routing them
    // through this function would silently downgrade them to `Other`.
    assert_eq!(classify_level("onerror"), ConsoleLevel::Other);
    assert_eq!(classify_level("unhandledrejection"), ConsoleLevel::Other);
}

#[test]
fn only_warnings_and_errors_are_noteworthy() {
    assert!(is_noteworthy(ConsoleLevel::Warn));
    assert!(is_noteworthy(ConsoleLevel::Error));
    for level in [
        ConsoleLevel::Debug,
        ConsoleLevel::Log,
        ConsoleLevel::Info,
        ConsoleLevel::Other,
    ] {
        assert!(!is_noteworthy(level), "{level:?}");
    }
}

#[test]
fn other_is_never_noteworthy_because_that_would_be_the_ranking_again() {
    // Counting `Other` in the header would reintroduce the guess the module
    // refuses, one indirection later.
    assert!(!is_noteworthy(classify_level("table")));
}
