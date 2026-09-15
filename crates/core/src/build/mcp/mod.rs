//! The Build & Diagnostics MCP server: the app's `dotnet build` and its parsed
//! diagnostics, exposed to a coding agent over a named pipe.
//!
//! The app already runs and parses builds for the human in the editor: a build
//! streams raw output to the console and the structured [`crate::build::BuildReport`]
//! is built from the MSBuild file-logger artifacts afterwards. This module is the
//! other consumer: a **separate process** (`cb-app mcp-build`, self-dispatched
//! exactly as `record-intent`, `quality-gate`, `mcp-sql`, `mcp-browser`,
//! `mcp-tasks`, `mcp-roslyn` and `mcp-editor` are — see [`argv`]) with no window
//! and no `AppState`, which **forwards each tool call over a named pipe to the
//! running application** and lets it run the build and answer from its cached
//! [`crate::build::BuildReport`].
//!
//! This is the Roslyn/Editor-context pipe-forwarding pattern exactly — a shim that
//! decides nothing and an application that owns the build — chosen over a
//! self-contained server because a build runs through the app's `Supervisor` and is
//! workspace-scoped, and the app already has a `build_project` command. The app is
//! running by construction (the agent runs inside its floating terminal), so the
//! "requires the app running" constraint is free.
//!
//! # `--workspace` *is* the boundary
//!
//! Like the Roslyn and Tasks servers, `--workspace <root>` names the one
//! repository this server may reach, regardless of which window is focused. The
//! install step bakes it in, so it is a real consent boundary. An optional
//! `--instance <pid>` disambiguates two windows with the same repo open.
//!
//! # Layout — copied in shape from [`crate::roslyn`]
//!
//! | Module | What it decides | Touches the world |
//! |---|---|---|
//! | [`argv`] | Is this process the server or the application | no |
//! | [`wire`] | What travels on the control pipe, both directions | no |
//! | [`instances`] | Which running application has the requested workspace open | no |
//! | [`tools`] | The four tools, their schemas, and how a call is read | no |
//! | [`answer`] | Every refusal that is not data, kept apart | no |
//! | [`render`] | Turning a [`crate::build::BuildReport`] into words an agent reads | no |
//! | [`serve`] | The handshake and the answer envelopes | no |
//! | [`install`] | Writing this server into an agent's configuration | no |
//!
//! The line framing ([`crate::mcp::ndjson`]), the routing and envelopes
//! ([`crate::mcp::serve`]) and the config writers ([`crate::mcp::install`]) are
//! **reused directly** rather than copied.
//!
//! # The governing rule
//!
//! The same abstain-rather-than-guess rule as [`crate::build`]: *never built*,
//! *building*, *succeeded clean*, *succeeded with warnings*, *failed* and *build
//! could not start* are six distinct answers and must never collapse. A tool that
//! asks for errors before any build ran refuses with
//! [`answer::BuildRefusal::NeverBuilt`] rather than reporting an empty success.

pub mod answer;
pub mod argv;
pub mod install;
pub mod instances;
pub mod render;
pub mod serve;
pub mod tools;
pub mod wire;
