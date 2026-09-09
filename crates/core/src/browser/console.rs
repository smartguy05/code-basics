//! Which level a captured console message carries.
//!
//! The whole module is one abstention: **an unknown `console` method is
//! [`ConsoleLevel::Other`], never [`ConsoleLevel::Error`]**.
//!
//! There are more than a dozen methods on `console` and a page may add its own,
//! so the set arriving here is open. Ranking an unrecognised one as an error
//! puts a red row in front of the user for a call that printed a table, and —
//! worse — makes an agent report "the page logged an error" about
//! `console.group`. Ranking it as `Log` would be the opposite guess and hide a
//! method that really was severe. `Other` is the only answer that claims
//! nothing, and [`crate::browser::model::ConsoleEntry::method`] carries the
//! page's own word beside it so an `Other` row is still readable.
//!
//! # The two captured things that are not console calls
//!
//! `window.onerror` and `unhandledrejection` are captured by the injected
//! script because they are the most useful signals a page emits, and neither is
//! a `console` method. They are **not** classified here: their level is `Error`
//! by construction of the message kind in [`super::ipc`], because the page
//! genuinely threw. Handing their names to this function would make them
//! `Other`, which is why they are not routed through it — pinned by
//! `the_window_error_events_are_not_console_methods`.

use super::model::ConsoleLevel;

/// The console methods this app ranks, and nothing else.
///
/// A deliberately short exact list. `trace`, `assert`, `dir`, `table`, `group`,
/// `count` and `time*` are all absent: they are real methods with no clear
/// severity, and inventing one for them is the guess this module refuses.
/// `"warning"` is here beside `"warn"` because the DOM's own level vocabulary
/// spells it that way and a page-side shim may pass it through.
const KNOWN: [(&str, ConsoleLevel); 6] = [
    ("debug", ConsoleLevel::Debug),
    ("log", ConsoleLevel::Log),
    ("info", ConsoleLevel::Info),
    ("warn", ConsoleLevel::Warn),
    ("warning", ConsoleLevel::Warn),
    ("error", ConsoleLevel::Error),
];

/// Rank one console method.
///
/// Case- and whitespace-insensitive, because the method name arrives as a
/// string from the page and a shim may have changed its shape; nothing else is
/// inferred from it.
pub fn classify_level(method: &str) -> ConsoleLevel {
    let key = method.trim().to_ascii_lowercase();
    KNOWN
        .iter()
        .find(|(name, _)| *name == key)
        .map_or(ConsoleLevel::Other, |(_, level)| *level)
}

/// Whether a level is worth surfacing in the panel header as a count.
///
/// Warnings and errors only. `Other` is excluded precisely because it is an
/// abstention — counting it would reintroduce the ranking the module refuses,
/// one indirection later.
pub fn is_noteworthy(level: ConsoleLevel) -> bool {
    matches!(level, ConsoleLevel::Warn | ConsoleLevel::Error)
}

#[cfg(test)]
#[path = "console_tests.rs"]
mod tests;
