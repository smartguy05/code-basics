//! Flagging the changes that quietly weaken a codebase.
//!
//! Additions get read; deletions and loosenings get skimmed. This is a
//! rules-based, **no-model** scan over the diff that surfaces the moves an
//! agent makes on its way to green: a deleted assertion, a test marked
//! `[Ignore]`, a widened `catch`, an introduced `.unwrap()`, a `TODO` left in a
//! production path, a removed timeout.
//!
//! # Declarative, like the adapter manifests
//!
//! Each rule is one regex against one *side* of the diff, and the built-in set
//! is extended — never shadowed — by per-workspace TOML under
//! `.code-basics/erosion/*.toml`, exactly as [`crate::adapters::manifest`]
//! loads declarative adapters. A user adds a rule for their own conventions
//! without writing Rust.
//!
//! # The side is the correctness lever
//!
//! A *deleted assertion* is a pattern on a **removed** line; an *introduced
//! unwrap* is a pattern on an **added** line. Getting that backwards is how a
//! rules scan produces noise, so a rule names its side and a match is only ever
//! taken on a line of that origin — never a context line.
//!
//! # A wrong flag is worse than none
//!
//! The rule the whole codebase is built against. Patterns are chosen for high
//! signal rather than coverage; a flag always cites the exact line and the rule
//! that fired; a rule whose regex will not compile is reported in
//! [`scan::ErosionReport::warnings`] rather than silently dropped; and the
//! detector ranks nothing — it is a pure producer of located facts, leaving any
//! future risk-ranking layer to weight them.

pub mod rules;
pub mod scan;

pub use rules::{
    all_rules, builtin_rules, compile, load_dir, parse, rules_dir, ErosionCategory, ErosionRule,
    RuleSide,
};
pub use scan::{scan_diffs, ErosionFlag, ErosionReport};
