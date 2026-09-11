//! The write-capable Redis MCP server, self-dispatched as `cb-app.exe mcp-redis`.
//!
//! Mirrors [`crate::mcp`] (the read-only SQL server) and [`crate::tasks::mcp`]
//! (the write-capable Tasks server). Reads are gated by `expose_to_agents`
//! ([`expose::find_exposed`]); writes are additionally gated by `allow_writes`
//! ([`execute::plan`] via [`crate::redis::ops::plan_write`]) — two separate
//! consents, both off by default. Framing, routing and config install are reused
//! from [`crate::mcp`]; the runner (`src-tauri/src/mcp_redis.rs`) is I/O only.

pub mod answer;
pub mod argv;
pub mod execute;
pub mod expose;
pub mod render;
pub mod serve;
pub mod tools;
