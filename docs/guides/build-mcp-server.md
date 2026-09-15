# The Build & Diagnostics MCP server

The app already runs and parses builds for the human in the editor: a build
streams its raw output to the console live, and the structured view —
`{file, line, column, code, severity, message, project}` per diagnostic, plus a
status — is built afterwards from the MSBuild file-logger artifacts. The Build &
Diagnostics MCP server is the other consumer of that same machinery: it lets a
coding agent — Claude Code, Codex, anything that speaks the Model Context
Protocol — build one repository and read the build's diagnostics, over MCP.

It runs as `cb-app mcp-build`: the **same executable**, self-dispatched over
stdio, exactly as `record-intent`, `quality-gate`, `mcp-sql`, `mcp-browser`,
`mcp-tasks`, `mcp-roslyn` and `mcp-editor` are. There is no second artifact to
keep in step and no interpreter to depend on being installed. The server name is
`code-basics-build`.

## Why it forwards to the running app instead of building on its own

An MCP server the agent starts fresh would have to resolve the projects, run the
build and parse the artifacts itself, duplicating what the app already does — and
it could not reuse the app's `Supervisor`, its per-workspace project scan, or the
`build_project` command that already exists. So this server owns none of that. It
is a **shim**: it forwards each tool call over a per-process named pipe to the
running application and lets it run the build and answer from its cached
`BuildReport`.

This is the Roslyn and Editor-context pattern exactly (a shim that decides
nothing, an application that owns everything); the line framing, routing and
config writers are reused from `mcp::` rather than copied. It is deliberately
**not** the self-contained SQL/Tasks pattern, because a build is workspace-scoped
and runs through the app's process supervisor.

The pipe is `\\.\pipe\code-basics.build.<pid>`, derived from a process id that
has first been checked against the executable path recorded for it — a registry
entry naming a different program is refused as tampering rather than corrected,
because a stranger's process answering an agent as if it were this application is
exactly what that check exists to catch.

The app is running by construction — the agent runs inside one of the app's own
floating terminals — so the "requires the app to be running" constraint costs
nothing.

## Why the file logger, not console scraping

The build is run with MSBuild's file logger — `-flp1:errorsOnly;logfile=…` and
`-flp2:warningsOnly;logfile=…` under `/p:GenerateFullPaths=true` and forced
invariant culture (`DOTNET_CLI_UI_LANGUAGE=en-US`) — rather than by scraping the
console text or reading a binlog. MSBuild writes those artifacts in a defined
canonical shape, `PATH(line,col): severity CODE: message [project]`, so the
parser reads *what the tool emitted* — the same model the `testing/` module uses
for `.trx`/JUnit — and stays entirely in Rust with no nuget or sidecar
dependency.

A line that does not match the canonical shape is **never dropped and never
guessed into a diagnostic**: an indented continuation line extends the previous
diagnostic's message, and anything else is preserved verbatim in
`BuildReport.warnings` (which are the parser's own abstentions, *not* build
warnings — those are diagnostics with severity `warning`).

## The tools

Four tools. `build_solution` runs a build; the other three read the most recent
one from cache.

| Tool | Takes | Answers |
| --- | --- | --- |
| `build_solution` | optional `configuration` (e.g. `Debug`, `Release`) | runs a build and reports the outcome and any diagnostics |
| `get_errors` | — | the errors from the most recent build |
| `get_warnings` | — | the warnings from the most recent build |
| `get_build_status` | — | the status of the most recent build |

`build_solution` is the only tool that runs a process; the read tools answer from
the cached `BuildReport` the last build left, so an agent can build once and then
ask about errors and warnings separately without rebuilding.

## Six statuses, never collapsed

The abstain philosophy applies to the build status exactly as it does to the LSP
six-state `Availability` and the DAP `DebugState`. `get_build_status` reports one
of six distinct answers, and they are kept apart on purpose:

| Status | Means |
| --- | --- |
| `neverBuilt` | no build has run for this workspace yet |
| `building` | a build is in progress right now |
| `succeededClean` | the build succeeded with no warnings and no errors |
| `succeededWithWarnings` | the build succeeded but produced warnings |
| `failed` | the build reported errors, or the process exited non-zero |
| `couldNotStart` | the build process could not be started at all (e.g. `dotnet` missing) |

A pure parse of the artifacts can only ever produce the middle three; the app
supplies the other three from state the artifacts cannot describe — that no build
has run, that one is in progress, or that the process never started. Collapsing
any pair — treating *could not start* as *failed*, or *never built* as *clean* —
would be the guess this feature refuses, because a model reads the answer,
believes it, and acts on it.

The read tools follow the same rule. `get_errors` and `get_warnings` **refuse**
with `neverBuilt` before any build has run (rather than reporting an empty
success that reads as a clean build), note that a build is still in progress
rather than answering from a stale cache, and report "no errors"/"no warnings"
only when a build genuinely finished with none. Any lines the parser could not
interpret are noted alongside the answer, never silently dropped.

## Every refusal is its own answer

The refusals that are not data are kept apart, one code each, and none forwards
any internal or driver error text to the agent:

| Code | Means |
| --- | --- |
| `noWorkspace` | the server was started with no `--workspace`, so there is no repository to build — reinstall it scoped to a repository |
| `ambiguous` | more than one running window has this repository open and no `--instance` was given — the server refuses rather than building in a window you are not looking at |
| `neverBuilt` | a read tool was asked before any build has run |
| `buildFailedToStart` | the build process could not be launched (the `couldNotStart` status, as a refusal for the tool that ran it) |
| `disabled` | the `buildMcp` feature is switched off (shares `tool_gate`'s disabled code) |

`ambiguous` **refuses rather than picks**: driving a build in a window the user
is not looking at is worse than asking. No refusal names a path, an OS error or a
build log line — a failed build's own words stay in the application.

## `--workspace` *is* the boundary

Like the Roslyn and Tasks servers, `cb-app mcp-build --workspace <root>` names
the **one repository** this server may build, whatever window is focused. An
unscoped invocation reaches no repository and every tool answers `noWorkspace`
rather than guessing a directory. The install step bakes the workspace in, so it
is a real consent boundary: an agent configured for repo X cannot build repo Y.

An optional `--instance <pid>` disambiguates two windows with the **same**
repository open. It is a disambiguator and grants nothing — it narrows the choice
of process before anything is decided, and when several windows have the same
repo open and no `--instance` was given the server refuses (`ambiguous`) rather
than picking.

## The feature gate, checked two ways

Unlike the always-on Roslyn server, the Build server is gated by the `buildMcp`
feature (default on). It is enforced in both directions, the Editor-context
precedent: the Plugins-menu installer row is omitted while the feature is off,
**and** the pipe host re-reads the feature per call and refuses `disabled` — so a
server installed while the feature was on cannot answer once it is switched off.
Per-tool on/off across the built-in servers is a finer grain still, in
`tool_gate` (Settings → MCP tools), under `ServerId::Build`.

## What the grant actually is

This server lets an agent **build one repository and read the build's
diagnostics** — its errors, warnings and status, from the same build the editor
runs. A build writes compiler output such as `bin/` and `obj/`, but it changes no
source file, and there is deliberately **no tool here that edits code**. The
trust level is the SQL/Tasks/Roslyn model: the grant is narrow, so there is **no
per-call consent gate** — the consent decision is made once, when you install the
server scoped to a repository.

## Installing it into an agent

Wiring the server into an agent is a separate, previewed opt-in, surfaced from
the titlebar **Plugins** menu as a **Build MCP server** panel
(`BuildMcpPanel.tsx`). It needs an open codebase (the project-scope install
writes `.mcp.json` at a repository root and the status is read per repository)
and it is present only while the `buildMcp` feature is on. The install action
shows the exact final contents of every file it would write (`PlanPreview`)
before anything is written.

The install bridge reuses `mcp::install`'s config-writer merge wholesale, so the
target files are the same three as the other servers:

| Provider / scope | File |
| --- | --- |
| Claude Code, project | `<root>/.mcp.json` |
| Claude Code, user | `~/.claude.json` |
| Codex, user | `$CODEX_HOME/config.toml` |

The entry is this executable plus `args` — `["mcp-build", "--workspace", <root>]`
at project scope, or just `["mcp-build"]` at user scope — as **separate fields**,
never a quoted command string, so an executable path containing a space, a quote
or a `&` is safe. Codex has no project scope. A **project**-scope install bakes in
the workspace (the consent boundary); a **user**-scope install names no workspace,
fires everywhere, and therefore answers `noWorkspace` until you install it scoped
to a repository instead.

Writes go through `providers::apply_writes_atomically` (temp file + rename)
because `~/.claude.json` is large (~122 KB) and is rewritten continuously by a
running Claude Code, so a truncate-then-write could lose someone's whole
configuration. The caveats shown before writing are the server's own — that a
committed `.mcp.json` is shared with everyone who clones the repo and needs the
agent's own approval before it loads, that a full path to this executable will
fail for teammates who installed it elsewhere, that the build is scoped to the one
repository, and, stated last so it is not scrolled past, that a build writes
compiler output but edits no source file.

## Where the code is

| Module | Holds |
| --- | --- |
| `crates/core/src/build/model.rs` | `BuildReport`, `BuildDiagnostic`, `BuildSeverity`, the six-state `BuildStatus` |
| `crates/core/src/build/parse.rs` | the canonical MSBuild file-logger line parser and `parse_report` |
| `crates/core/src/build/mcp/argv.rs` | is this process the server or the application; `--workspace` / `--instance` |
| `crates/core/src/build/mcp/wire.rs` | what travels on the control pipe, both directions |
| `crates/core/src/build/mcp/instances.rs` | which running application has the requested workspace open; the pipe name |
| `crates/core/src/build/mcp/tools.rs` | the four tools, their schemas, and how a call is read |
| `crates/core/src/build/mcp/answer.rs` | every refusal, kept apart |
| `crates/core/src/build/mcp/render.rs` | turning a `BuildReport` into the words an agent reads |
| `crates/core/src/build/mcp/serve.rs` | the handshake and the answer envelopes; `SERVER_NAME` |
| `crates/core/src/build/mcp/install.rs` | the previewed installer over `mcp::install`'s writers |
| `src-tauri/src/build/` | the pipe host, registry and agent-facing forwarder in the app |
| `src-tauri/src/commands/build_mcp.rs` | the install/uninstall command bridge |
| `src-tauri/src/mcp_build.rs` | the `mcp-build` stdio runner |
| `src/components/BuildMcpPanel.tsx` | the Plugins-menu install panel |

Everything in `cb-core` is pure and unit-tested headlessly with no MCP client and
no `dotnet` installed. See also [the Roslyn MCP server](roslyn-mcp-server.md) and
[the Editor context MCP server](editor-context-mcp.md) (whose pipe-forwarding and
`--workspace` boundary this one follows), [the SQL MCP server](sql-mcp-server.md),
and [the Tauri command reference](../reference/mcp-servers.md#build--diagnostics-mcp-server).
