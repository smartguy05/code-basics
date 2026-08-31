//! The SQL console: connect to a database the workspace already talks to, run a
//! read-only query, and read the values back.
//!
//! This subsystem reads *data*, which nothing else in `cb-core` does, so it is
//! governed by three rules that are stricter than the crate's usual
//! abstain-rather-than-guess discipline. They are stated here because every
//! module below inherits them.
//!
//! 1. **The value-reading path lives outside `architecture/`.** The project and
//!    component graphs refuse to surface a connection-string *value* — the key
//!    is the author's label, everything right of the `=` is refused whole rather
//!    than redacted (`architecture::signals::framework::admit`). Nothing here
//!    changes that. This subsystem obtains a connection string by a separate,
//!    explicit route (the user typing it, or `dotenv` reading a file the user
//!    named) and never feeds one back into a graph, a signal or a warning.
//!
//! 2. **The read-only guard is a text heuristic, and its refusal message says
//!    so.** There is no way to prove a statement is read-only without the
//!    server's own permission model, so `guard` classifies parsed statements and
//!    refuses everything it does not positively recognise as a read. A refusal
//!    names the heuristic as a heuristic: the user must not read "blocked" as
//!    "the database is protected". Distinct outcomes stay distinct — *allowed*,
//!    *refused because it is a known write*, *refused because it could not be
//!    parsed*, and *refused because it parsed into something unrecognised* are
//!    four answers, never one.
//!
//! 3. **A connection string crosses IPC in one direction only** — frontend to
//!    backend, per request — and is never persisted or echoed. It never enters
//!    `.code-basics/`, git, an architecture graph, a `tracing` log, or an error
//!    message. `store` therefore persists connection *profiles* without their
//!    secrets, and any error raised while parsing or connecting is built from
//!    the redacted description in `dsn`, never from the input text.
//!
//! Layout:
//!
//! - [`model`] — the wire types that cross IPC.
//! - [`dsn`] — parsing and redacting a connection string.
//! - [`guard`] — the read-only statement classifier.
//! - [`format`] — rendering result values for display.
//! - [`dotenv`] — reading connection strings out of environment files.
//! - [`discover`] — finding the connections a workspace already mentions.
//! - [`store`] — persisted, secret-free connection profiles.
//! - [`driver`] — the **only** module here that opens a database handle.
//! - [`session`] — the registry of in-flight statements and their stop handles.

pub mod discover;
pub mod dotenv;
pub mod driver;
pub mod dsn;
pub mod format;
pub mod guard;
pub mod model;
pub mod session;
pub mod store;
