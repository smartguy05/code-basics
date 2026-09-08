//! The SQL MCP server: what an agent may ask this app's databases, and what it
//! is told back.
//!
//! The SQL console already reaches three engines, classifies a statement as
//! read-only and caps a result set — and only a human at the panel could use
//! any of it. This module is the other consumer. It is a **separate process**
//! (`cb-app mcp-sql`, self-dispatched exactly as `record-intent` and
//! `quality-gate` are — see [`argv`]) with no `AppState` and no window, so
//! everything it decides has to live in `cb-core` where both it and the console
//! can reach it. That is why [`crate::sql::plan`] and [`crate::sql::catalog`]
//! exist as core modules rather than as bridge code.
//!
//! # The four rules
//!
//! 1. **Consent is per connection and off by default.**
//!    [`crate::sql::store::SqlConnection::expose_to_agents`] is the only thing
//!    that makes a connection reachable, an absent key is consent *withheld*,
//!    and it is re-read on every call so revoking it takes effect immediately
//!    ([`expose`]).
//! 2. **Read-only is forced, not requested.** [`execute::agent_plan`] has no
//!    `writes_allowed` parameter for a caller to pass, the guard runs with
//!    `false`, and the handle is opened with `false` so the engine's own
//!    mechanism fires. The connection's `allow_writes` setting is not consulted
//!    on this path.
//! 3. **No driver text is forwarded.** A connect failure crosses as a variant
//!    and a statement failure as a stage ([`answer`]). `dsn::redact` is a
//!    keyword heuristic, and the party on the other end of this transport will
//!    paste what it is told into a transcript, a commit and a provider's logs.
//! 4. **Every abstention is its own answer.** *Nothing exposed* is not *nothing
//!    saved*; *not exposed* is not *not found*; an ambiguous selector is
//!    refused rather than resolved; a capped result is incomplete rather than
//!    short. See [`answer`] and [`render`].
//!
//! # The hole that cannot be closed here, stated rather than softened
//!
//! An agent with read access can run `pg_stat_activity`, `SHOW ALL`,
//! `sys.dm_exec_connections`, or read a foreign-data-wrapper user mapping. That
//! reads credentials **the database itself stores** — not this app's connection
//! string, which nothing here can reach, but credentials all the same. It is
//! inherent in "an agent has read access to your database" and no amount of
//! care in this module removes it. So the exposure flag's own documentation and
//! every install caveat carry the same sentence: *expose only connections whose
//! login you would give a colleague read access to*.
//!
//! [`crate::sql::guard`]'s denylist already refuses `pg_read_file`, `pg_ls_dir`,
//! `lo_import`, `readfile`, `xp_cmdshell`, `sp_executesql`, `OPENROWSET` and
//! `OPENDATASOURCE`, which closes the filesystem and linked-server escapes —
//! and its own docs correctly say an exhaustive list is not achievable.
//!
//! Layout:
//!
//! - [`argv`] — is this process the server or the application.
//! - [`ndjson`] — the line-delimited framing this transport uses.
//! - [`expose`] — which connections an agent may see, and what it is told.
//! - [`execute`] — forcing read-only.
//! - [`answer`] — every refusal, kept apart.
//! - [`render`] — turning an answer into the words an agent reads.
//! - [`tools`] — the six tools, their schemas, and how a call is read.
//! - [`serve`] — the handshake, the routing, and the answers needing no driver.
//! - [`install`] — writing the server into an agent's configuration, previewed
//!   first, in the three places MCP servers actually live.

pub mod answer;
pub mod argv;
pub mod execute;
pub mod expose;
pub mod install;
pub mod ndjson;
pub mod render;
pub mod serve;
pub mod tools;
