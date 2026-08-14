//! Knowing what a workspace declares, and finding it fast.
//!
//! The Changes tab already had to answer a small version of this question — "what
//! function does this hunk sit in?" — and answered it with a word scan over a
//! single line. This module is that scan promoted to a first-class concern, plus
//! the machinery around it: an index over every source file the workspace scan
//! can see, a cache so opening a workspace does not re-read it all, a fuzzy
//! matcher, and the query layer a symbol palette sits on.
//!
//! The layering is deliberate and one-directional:
//!
//! * [`declarations`] is pure text. One line in, an optional name and kind out.
//!   No IO, no repository, no workspace.
//! * [`fuzzy`] is pure arithmetic. A query and a candidate in, a score out.
//! * [`index`] does the walking, over [`crate::workspace::source_walker`] so
//!   that what the index knows about and what the project list knows about can
//!   never disagree.
//! * [`cache`] persists what the index built.
//! * [`search`] is the only layer that combines them, and the only one a
//!   *query* goes through. One file talks to this module at all —
//!   `src-tauri/src/commands/symbols.rs` — and it reaches four entry points
//!   across three of the layers, one per thing a palette needs: answering a
//!   query is [`search::search`], reporting whether there is an index to
//!   answer from is [`index::SymbolIndexStatus::of`], and building one is
//!   [`cache::build_cached`] (the ordinary open, fingerprints trusted) or
//!   [`cache::rebuild`] (the user's escape hatch, cache ignored). What that
//!   command layer does *not* do is decide anything: it resolves state, calls
//!   one of those four, and returns what came back — it never takes a
//!   `SearchHit` apart, re-derives a query, or walks a file itself. Everything
//!   below was written and tested against that boundary a phase before the
//!   boundary existed, which is why it needed nothing added to it.
//!
//! Nothing here depends on `git`. The arrow points the other way — `git`'s hunk
//! grouping uses [`declarations`] — and it must stay that way: a declaration is
//! a property of source text, and needing a repository to recognise one would
//! be an accident of where the code first happened to live.
//!
//! # The abstain rule applies here too
//!
//! This is a heuristic over text, not a compiler front end, and a symbol
//! palette that jumps to the wrong place is worse than one that admits it does
//! not know. Every threshold below prefers no answer: an unrecognised line
//! yields no symbol, an unplaceable declaration yields
//! [`declarations::SymbolKind::Other`] and therefore no badge, and a weak fuzzy
//! match is dropped rather than ranked last.

pub mod cache;
pub mod declarations;
pub mod fuzzy;
pub mod index;
pub mod search;
