# The Roslyn MCP server

The app already keeps a **warm per-workspace language-server session** — Roslyn
for C#, plus `typescript-language-server` and `rust-analyzer` — and answers
find-usages, go-to-definition, type hierarchy, signature help and pull
diagnostics for the human in the editor (see [Language servers](language-servers.md)).
The Roslyn MCP server is the other consumer of that same session: it lets a
coding agent — Claude Code, Codex, anything that speaks the Model Context
Protocol — ask the same semantic questions, over MCP.

It runs as `cb-app mcp-roslyn`: the **same executable**, self-dispatched over
stdio, exactly as `record-intent`, `quality-gate`, `mcp-sql`, `mcp-browser` and
`mcp-tasks` are — the sixth self-dispatch mode. There is no second artifact to
keep in step and no interpreter to depend on being installed. The server name is
`code-basics-roslyn`.

Despite the name it serves **every language the LSP serves** — C#, TypeScript,
Rust — not just C#. "Roslyn" is what the warm session is mostly there for; the
tools forward to whichever server owns the file.

## Why it forwards to the running app instead of loading its own Roslyn

An MCP server the agent starts fresh each session would have to load the
solution itself, and a cold Roslyn/MSBuild project load is **30–180 seconds** —
paid on every agent session, over a solution the app already has open and warm.
So this server owns none of the semantic model. It is a **shim**: it forwards
each tool call over a per-process named pipe to the running application and lets
it answer from its existing `LspHandle`.

The pipe is `\\.\pipe\code-basics.roslyn.<pid>`, derived from a process id that
has first been checked against the executable path recorded for it — a registry
entry naming a different program is refused as tampering rather than corrected,
because a stranger's process answering an agent as if it were this application is
exactly what that check exists to catch.

The app is running by construction — the agent runs inside one of the app's own
floating terminals — so the "requires the app to be running" constraint costs
nothing. This is the browser server's pattern exactly (a shim that decides
nothing, an application that owns everything); the line framing, routing and
config writers are reused from `mcp::` rather than copied.

## The tools

Four tools, each gated by the language server's **advertised capability** and
each abstaining rather than failing when it is not offered.

| Tool | Takes | Answers |
| --- | --- | --- |
| `find_references` | `path` + position-or-symbol | every use site of the symbol, the declaration included |
| `get_type_hierarchy` | `path` + position-or-symbol | the type's supertypes and subtypes |
| `resolve_overloads` | `path` + position-or-symbol | the callable signatures at a call site, and the active one |
| `get_diagnostics` | `path` | every diagnostic (error, warning, hint) on one file |

`find_references` is the editor's "find usages" answer, not a text match —
`Order.Total` and `Invoice.Total` are different symbols. `get_type_hierarchy` is
`prepareTypeHierarchy` followed by supertypes/subtypes; `resolve_overloads` is
`signatureHelp`; `get_diagnostics` is a **pull** on `textDocument/diagnostic`,
requested fresh rather than read from a stale cache.

### There is no caret, so a location is a position *or* a symbol

A human points a caret; an agent has neither caret nor open file. So the three
position-taking tools accept **either**:

- an exact position `{ line, character }` — `line` is **1-based** (matching the
  editor gutter), `character` is **0-based UTF-16 code units**; or
- `{ symbol: "Name" }`, which the application resolves to the identifier anchor
  via its own document-symbol index.

A complete position **wins** when both are given; an incomplete position (only
one of the two) is a shape error; and neither a position nor a symbol is a shape
error naming both ways in. An **ambiguous** symbol name — one matching more than
one declaration — with no position to disambiguate is **listed, never guessed
at**: the application refuses and reports the candidates, because guessing which
`Total` you meant is the one thing this feature will not do.

`get_diagnostics` is whole-file — pull diagnostics are document-scoped — so it
takes **only** `path`. Demanding a position it would ignore would be a lie in the
schema.

Every `path` is a **workspace-relative** path with forward slashes (e.g.
`src/App.cs`). A path that is absolute, carries a drive or UNC prefix, or climbs
out with `..` is refused (`badPath`) before any file is touched — `--workspace`
is the whole consent boundary, so a path may not be able to reach outside it.

### Capability-gated, and abstention is the point

Each tool is gated by whether the server advertises the matching capability, and
when it does not the answer is `Availability::Unsupported` — **not** a failure and
**not** an empty result. The worked example is C#: **Roslyn 2.140.9 does not
advertise `diagnosticProvider`**, so `get_diagnostics` on a `.cs` file reports
*unsupported* — the capability simply is not there — rather than "no diagnostics",
which would read as a clean file.

## Every abstention is its own answer

The governing rule is the same abstain-rather-than-guess rule the SQL and Tasks
servers apply, and it is sharper here than anywhere: a model reads the sentence,
believes it, and acts, so the distinctions a human would have recovered from
unaided are kept apart. Eight distinct refusal codes, each its own answer:

| Code | Means |
| --- | --- |
| `noWorkspace` | the server was started with no `--workspace`, so there is no session to ask — reinstall it scoped to a repository |
| `noSession` | the workspace is open but its LSP session was torn down, or none started for this language — reopen the repository |
| `notConfigured` | no server is configured for this language, or the configured one was not found on disk |
| `serverStarting` | the server is up but has not finished its handshake — ask again shortly |
| `serverLoading` | the server is handshaken but still loading projects, so any answer now would be incomplete — ask again once loaded |
| `serverFailed` | the server exited, errored, or replied with something unreadable |
| `unsupported` | the server is healthy and does not offer this question for this language (the `get_diagnostics`-on-C# case) |
| `badPath` | the `path` could escape the scoped workspace root |

`noWorkspace` and `noSession` come **before** the server is ever asked; the
middle five are derived from the LSP's own six-variant `Availability`, one code
each so *starting*, *loading*, *failed*, *unsupported* and *not configured* never
collapse into a shared "unavailable". `Ready` is not a refusal — it is data.

**No internal error text is ever forwarded.** A failed server's own words stay in
the application; the refusal names no path, no OS error and no server log line.
Each refusal is an MCP **tool execution error** the model can correct itself on,
not a JSON-RPC protocol error — protocol errors are reserved for the two things
no model reasoning fixes: an unknown tool name and arguments of the wrong shape.

## `--workspace` *is* the boundary

Unlike the SQL server (where `--workspace` only filters a listing) and unlike the
browser server (which resolves the *active* window), this server adopts the Tasks
pattern: `cb-app mcp-roslyn --workspace <root>` names the **one repository** this
server may reach, whatever window is focused. An unscoped invocation reaches no
session and every tool answers `noWorkspace` rather than guessing a directory.
The install step bakes the workspace in, so it is a real consent boundary: an
agent configured for repo X cannot reach repo Y's semantic model.

An optional `--instance <pid>` disambiguates two windows with the **same**
repository open. It is a disambiguator and grants nothing — it narrows the choice
of process before anything is decided, and when several windows have the same
repo open and no `--instance` was given the server **refuses rather than picks**,
because answering from a window the user is not looking at is worse than asking.

## Read-only, and no per-call consent gate

This server gives an agent **read-only code intelligence** — find-usages,
diagnostics, type hierarchy and signature help. It reads code; it never changes a
file, and there is no tool here that does (there is deliberately no arbitrary
"evaluate" or edit tool). Because the grant is narrow and read-only, the trust
level is the SQL/Tasks model: there is **no per-call consent gate** — the consent
decision is made once, when you install the server scoped to a repository.

## Installing it into an agent

Wiring the server into an agent is a separate, previewed opt-in. It is surfaced
from the titlebar **Plugins** menu as a **Roslyn MCP** panel
(`RoslynMcpPanel.tsx`) — always present (it has no optional feature to switch
off, because the warm session it points at is always there), needing only an open
codebase. The panel's install action shows the exact final contents of every file
it would write (`PlanPreview`) before anything is written.

The install bridge reuses `mcp::install`'s config-writer merge wholesale, so the
target files are the same three as the SQL, browser and Tasks servers:

| Provider / scope | File |
| --- | --- |
| Claude Code, project | `<root>/.mcp.json` |
| Claude Code, user | `~/.claude.json` |
| Codex, user | `$CODEX_HOME/config.toml` |

The entry is this executable plus `args` — `["mcp-roslyn", "--workspace", <root>]`
at project scope, or just `["mcp-roslyn"]` at user scope — as **separate fields**,
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
fail for teammates who installed it elsewhere, and, stated last so it is not
scrolled past, that the grant is read-only code intelligence over one repository
and nothing more.

## Where the code is

| Module | Holds |
| --- | --- |
| `crates/core/src/roslyn/argv.rs` | is this process the server or the application; `--workspace` / `--instance` |
| `crates/core/src/roslyn/wire.rs` | what travels on the control pipe, both directions |
| `crates/core/src/roslyn/instances.rs` | which running application has the requested workspace open; the pipe name |
| `crates/core/src/roslyn/liveness.rs` | is a registry entry's process still that executable |
| `crates/core/src/roslyn/tools.rs` | the four tools, their schemas, and how a call is read |
| `crates/core/src/roslyn/answer.rs` | every refusal, kept apart; the path-within-root check |
| `crates/core/src/roslyn/render.rs` | turning an LSP result into the words an agent reads |
| `crates/core/src/roslyn/serve.rs` | the handshake and the answer envelopes; `SERVER_NAME` |
| `crates/core/src/roslyn/install.rs` | the previewed installer over `mcp::install`'s writers |
| `src-tauri/src/roslyn/` | the pipe host, registry and agent-facing forwarder in the app |
| `src-tauri/src/commands/roslyn_mcp.rs` | the install/uninstall command bridge |
| `src-tauri/src/mcp_roslyn.rs` | the `mcp-roslyn` stdio runner |
| `src/components/RoslynMcpPanel.tsx` | the Plugins-menu install panel |

Everything in `cb-core` is pure and unit-tested headlessly with no MCP client and
no language server installed. See also [Language servers](language-servers.md)
(the warm session this server exposes), [the SQL MCP server](sql-mcp-server.md)
and [the Tasks plugin](tasks-plugin.md) (whose `--workspace` boundary this one
follows), and [the Tauri command reference](../reference/mcp-servers.md#roslyn-mcp-server).
