//! The derived project graph: what this workspace is made of, and what points
//! at what.
//!
//! Every other view in the app answers a question about *one* project — run
//! it, test it, read its diff. This module answers the question that has no
//! single owner: how the projects the scan found relate to each other. The
//! answer is assembled entirely from evidence already on disk — `.csproj`
//! `<ProjectReference>` items, `package.json` dependency names, `.sln`
//! grouping — and never from anything the tool was told.
//!
//! # Why the graph is a separate artifact
//!
//! The obvious implementation is to hang a `references: Vec<String>` field off
//! [`crate::model::Project`] during the scan. That was rejected. `Project` is a
//! wire type: it crosses IPC on every workspace open, it is held by the run
//! sidebar, the tests tree, the changes view and the config editor, and every
//! one of those pays for a field none of them read. Worse, a reference list
//! recorded at scan time is a *cached* answer to a question whose inputs — the
//! manifest files — the user edits constantly while the workspace stays open,
//! so it would go stale silently and there is no cheap way to notice.
//!
//! The graph is therefore computed on demand from the manifests as they are
//! *now*, using the scan only for the set of projects that exist and where
//! they live. That costs a re-read of every manifest each time a diagram is
//! asked for, which is a handful of small files and is the right trade for an
//! artifact a user looks at deliberately rather than continuously.
//!
//! # The abstention rule, sharpened
//!
//! The rest of the crate abstains rather than guesses. Here the stakes are
//! higher than usual, because **an arrow between two boxes is a much stronger
//! claim than a label on a hunk**. A user reading a diagram treats an edge as
//! fact: this thing depends on that thing. A wrong edge is therefore not a
//! cosmetic defect, it is a false statement about the system that the user
//! will act on. Every rule in [`graph`] is written to produce nothing rather
//! than something plausible, and every reference that could not be turned into
//! an edge is pushed onto [`graph::ArchGraph::warnings`] rather than dropped —
//! because the *other* failure mode, a diagram that looks complete and is
//! quietly missing an arrow, is just as misleading and much harder to notice.
//!
//! # Layout
//!
//! * [`graph`] — the derived project graph and the types that cross IPC.
//! * [`components`] — the *component* map: services and the data stores they
//!   declare, assembled from [`signals`] and rendered through the same types.
//!   A separate entry point rather than an option on [`graph::project_graph`],
//!   because "what is in this repository" and "what does this system consist
//!   of" are different questions and a caller has to say which it is asking.
//! * [`mermaid`] — rendering a graph to Mermaid source.
//! * [`store`] — persisting a graph, and the user's edits to one.
//! * [`signals`] — components that are not projects (databases, caches,
//!   queues, services), and the grading rule that decides which of them are
//!   allowed to exist. Everything above derives edges from literal strings in
//!   manifests; that module deals in inference, so it carries a stricter gate
//!   of its own.

pub mod components;
pub mod graph;
pub mod mermaid;
pub mod signals;
pub mod store;

#[cfg(test)]
#[path = "components_tests.rs"]
mod components_tests;

#[cfg(test)]
#[path = "graph_tests.rs"]
mod graph_tests;

#[cfg(test)]
#[path = "mermaid_tests.rs"]
mod mermaid_tests;

#[cfg(test)]
#[path = "store_tests.rs"]
mod store_tests;
