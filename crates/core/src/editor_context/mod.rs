//! The editor-context MCP server: the app's live editor state, exposed to a
//! coding agent over a named pipe.
//!
//! The **twin** of [`crate::roslyn`]. Where the Roslyn server exposes the warm
//! semantic model, this one exposes what the human is *looking at* — the active
//! file, the cursor and viewport, the current selection, the open tabs and the
//! recently edited files. It is the same shim pattern: a **separate process**
//! (`cb-app mcp-editor`, self-dispatched exactly as `record-intent`,
//! `quality-gate`, `mcp-sql`, `mcp-browser`, `mcp-tasks` and `mcp-roslyn` are —
//! see [`argv`]) with no window and no `AppState`, which **forwards each tool call
//! over a named pipe to the running application** and lets it answer.
//!
//! # The state lives in the frontend
//!
//! The one real difference from the Roslyn twin: the editor state this server
//! reports is owned by the **React frontend**, not the backend. The frontend
//! pushes it into `AppState` (like the browser's automation-consent slot) while
//! the `EditorContextMcp` feature is enabled, and the pipe host reads it back.
//! That push/read seam is the app crate's job; everything decided about the wire,
//! the registry, the tools and the prose is here so it can be tested headlessly.
//!
//! # `--workspace` *is* the boundary
//!
//! Like the Tasks and Roslyn servers, `--workspace <root>` names the one
//! repository whose editor an agent configured for repo X may read, regardless of
//! which window is focused. The install step bakes it in, so it is a real consent
//! boundary. An optional `--instance <pid>` disambiguates two windows with the
//! same repo open.
//!
//! # Layout — copied in shape from [`crate::roslyn`]
//!
//! | Module | What it decides | Touches the world |
//! |---|---|---|
//! | [`argv`] | Is this process the server or the application | no |
//! | [`wire`] | What travels on the control pipe, both directions | no |
//! | [`instances`] | Which running application has the requested workspace open | no |
//! | [`liveness`] | Is a registry entry's process still that executable | one probe |
//! | [`tools`] | The four tools, their schemas, and how a call is read | no |
//! | [`answer`] | Every refusal that is not data, kept apart | no |
//! | [`render`] | Turning pushed editor state into the words an agent reads | no |
//! | [`serve`] | The handshake and the answer envelopes | no |
//! | [`install`] | Writing this server into an agent's configuration | no |
//!
//! The line framing ([`crate::mcp::ndjson`]), the routing and envelopes
//! ([`crate::mcp::serve`]), the liveness probe ([`crate::mcp::liveness`]) and the
//! config writers ([`crate::mcp::install`]) are **reused directly** rather than
//! copied — they are generic to any stdio MCP server this binary hosts.
//!
//! # The governing rule
//!
//! The same abstain-rather-than-guess rule as everywhere in this codebase: no
//! file active, nothing selected, the feature switched off and nothing pushed yet
//! are distinct answers with distinct codes ([`answer::EditorRefusal`]), and a
//! genuine empty (no open tabs, no recent files, an empty selection) is stated as
//! the complete answer rather than fabricated into a path or a position.

pub mod answer;
pub mod argv;
pub mod install;
pub mod instances;
pub mod liveness;
pub mod render;
pub mod serve;
pub mod tools;
pub mod wire;
