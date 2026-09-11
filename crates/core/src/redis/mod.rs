//! The Redis plugin's decision layers, modelled on [`crate::sql`].
//!
//! Every rule this subsystem follows is one the SQL console already follows, and
//! the module split is the same:
//!
//! - [`dsn`] parses and redacts a connection string; the only view that leaves
//!   is the password-free [`dsn::RedisConnectionDisplay`].
//! - [`store`] persists saved profiles **by reference, not value** (it shares
//!   [`crate::sql::store::SecretSource`]), user-global and outside any repo.
//! - [`discover`] finds connections in appsettings/user-secrets/.env, reading a
//!   value only to redact it and then dropping it. Filesystem only; the
//!   architecture graph still never reads a value.
//! - [`model`] is the wire model the panel and the MCP server share.
//! - [`ops`] is the read/write split and the **one** place write consent is
//!   enforced — a [`ops::WriteOp`] runs only once [`ops::plan_write`] has minted
//!   a [`ops::WritePlan`], which needs `allow_writes`.
//! - [`driver`] is the only socket-opening layer.
//! - [`mcp`] is the write-capable MCP server: reads gated by `expose_to_agents`,
//!   writes additionally by `allow_writes`, both off by default.
//!
//! Redis has one engine, so unlike SQL there is no engine to determine and no
//! server-side read-only mode: `allow_writes` at the planning tier is the whole
//! write gate, which is why [`ops`] makes it structural.

pub mod discover;
pub mod driver;
pub mod dsn;
pub mod mcp;
pub mod model;
pub mod ops;
pub mod store;
