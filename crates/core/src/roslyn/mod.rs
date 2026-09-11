//! The Roslyn/LSP MCP server: the app's warm semantic model, exposed to a coding
//! agent over a named pipe.
//!
//! The app already runs a **warm per-workspace LSP session** ([`crate::lsp`],
//! driving Roslyn for C#, plus the TypeScript and Rust servers) and answers
//! find-references, go-to-definition, type hierarchy, signature help and pull
//! diagnostics end-to-end for the human in the editor. This module is the other
//! consumer: a **separate process** (`cb-app mcp-roslyn`, self-dispatched exactly
//! as `record-intent`, `quality-gate`, `mcp-sql`, `mcp-browser` and `mcp-tasks`
//! are — see [`argv`]) with no window and no `AppState`, which **forwards each
//! tool call over a named pipe to the running application** and lets it answer
//! from its existing `LspHandle`.
//!
//! This is the browser-MCP pattern exactly — a shim that decides nothing about
//! the semantic model and an application that owns all of it — chosen over a
//! standalone Roslyn/MSBuild sidecar because that would pay a 30–180 s cold
//! project load on every agent session and duplicate the LSP the app already
//! keeps warm. The app is running by construction (the agent runs inside its
//! floating terminal), so the "requires the app running" constraint is free.
//!
//! # `--workspace` *is* the boundary
//!
//! Unlike the browser server (which resolves the *active* window), this server
//! adopts the Tasks pattern: `--workspace <root>` names the repository an agent
//! configured for repo X must reach, regardless of which window is focused. The
//! install step bakes it in, so it is a real consent boundary. An optional
//! `--instance <pid>` disambiguates two windows with the same repo open.
//!
//! # Layout — copied in shape from [`crate::browser`]
//!
//! | Module | What it decides | Touches the world |
//! |---|---|---|
//! | [`argv`] | Is this process the server or the application | no |
//! | [`wire`] | What travels on the control pipe, both directions | no |
//! | [`instances`] | Which running application has the requested workspace open | no |
//! | [`liveness`] | Is a registry entry's process still that executable | one probe |
//! | [`tools`] | The four tools, their schemas, and how a call is read | no |
//! | [`answer`] | Every refusal that is not data, kept apart | no |
//! | [`render`] | Turning an LSP result into the words an agent reads | no |
//! | [`serve`] | The handshake and the answer envelopes | no |
//! | [`install`] | Writing this server into an agent's configuration | no |
//!
//! The line framing ([`crate::mcp::ndjson`]), the routing and envelopes
//! ([`crate::mcp::serve`]) and the config writers ([`crate::mcp::install`]) are
//! **reused directly** rather than copied — they are generic to any stdio MCP
//! server this binary hosts.
//!
//! # The governing rule
//!
//! The same abstain-rather-than-guess rule as [`crate::lsp`], and it is sharper
//! here than anywhere: *unsupported*, *starting*, *loading*, *dead-or-timeout*,
//! *torn-down* and *a genuine empty* are six different answers and must never
//! collapse into one. [`crate::lsp::model::Availability`] has six variants for
//! exactly that, and [`answer::RoslynRefusal`] gives each non-`Ready` one its own
//! code so an agent branches on the real cause rather than a shared "unavailable".

pub mod answer;
pub mod argv;
pub mod install;
pub mod instances;
pub mod liveness;
pub mod render;
pub mod serve;
pub mod symbol;
pub mod tools;
pub mod wire;
