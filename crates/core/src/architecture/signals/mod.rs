//! Components that are not projects: the databases, caches, queues and HTTP
//! services a workspace talks to, and the grading rule that decides which of
//! them are allowed to exist.
//!
//! [`super::graph`] derives everything it draws from literal strings in
//! manifests. A `<ProjectReference Include="../Lib/Lib.csproj" />` either
//! resolves to a scanned project or is reportably unresolvable; there is no
//! third answer and no judgement involved. This module is different in kind.
//! "This service talks to that database", read out of a package reference, and
//! "this API calls that one", read out of an `AddHttpClient` registration, are
//! *inferences* — and once drawn they carry exactly the same visual weight as
//! the facts next to them. A reader cannot tell an arrow that was declared from
//! an arrow that was deduced, so the deduction has to be disciplined here,
//! before anything reaches the renderer.
//!
//! # The grading rule
//!
//! Every candidate this module's producers find is a [`framework::Signal`] with
//! a [`framework::Strength`], and the strength decides what it is permitted to
//! do:
//!
//! * **HIGH may create.** A HIGH signal is a declared fact — something the
//!   author wrote down in a manifest, a configuration file or a filename, that
//!   means what it says. A `<PackageReference Include="Npgsql" />` is the
//!   author stating that this project speaks PostgreSQL. Only HIGH signals
//!   bring a component or an edge into existence.
//! * **MEDIUM may only enrich.** A MEDIUM signal may add a label, a route list
//!   or a name to a component that a HIGH signal has *already* created. It may
//!   never create one. A MEDIUM signal with no HIGH counterpart is discarded.
//! * **Everything else is discarded, and counted.** Nothing is dropped
//!   silently. Every candidate that was seen and refused is recorded in
//!   [`framework::Admitted::discarded`] and rendered into
//!   [`super::graph::ArchGraph::warnings`], so "we looked at this and declined
//!   to name it" is visible to the user rather than invisible.
//!
//! The rule is enforced in exactly one place — [`framework::admit`] — rather
//! than trusted to each producer. The producers below are separate bodies of
//! per-ecosystem knowledge written at different times; if each of them decided
//! for itself what was admissible, the discipline would last exactly as long as
//! the most permissive one. They emit candidates and have no way to bypass the
//! gate.
//!
//! # The standing prohibitions
//!
//! Three things this module must never do, also enforced centrally in
//! [`framework`] rather than per producer:
//!
//! * **No connection-string *value* ever reaches the graph** — not the host,
//!   not the port, not the database name, not a credential. The *key* (say,
//!   `"Orders"`) is a label the author chose and is fair game; everything to
//!   the right of the `=` is not. A diagram is exported and shared, so this is
//!   the most dangerous thing the module could do and it is screened first, on
//!   every field of every signal including the evidence excerpt.
//! * **No component is created from a name similarity.** `Orders.Api` and
//!   `Orders.Worker` share a prefix. A shared prefix is a naming convention,
//!   not a dependency, and nothing here compares labels for resemblance.
//! * **Nothing is inferred from a `using`/`import` line or from a comment.** An
//!   import says a compiler could resolve a namespace. It does not say the
//!   program uses it at runtime, and a commented-out one says the opposite of
//!   what it appears to say.
//!
//! # Layout
//!
//! * [`framework`] — the signal types and [`framework::admit`], the gate.
//! * [`dotnet`] — signals read out of .NET manifests.
//! * [`node`] — signals read out of `package.json`.
//! * [`routes`] — signals about the routes a service exposes.

pub mod dotnet;
pub mod framework;
pub mod node;
pub mod routes;

#[cfg(test)]
#[path = "framework_tests.rs"]
mod framework_tests;

#[cfg(test)]
#[path = "dotnet_tests.rs"]
mod dotnet_tests;

#[cfg(test)]
#[path = "node_tests.rs"]
mod node_tests;

#[cfg(test)]
#[path = "routes_tests.rs"]
mod routes_tests;
