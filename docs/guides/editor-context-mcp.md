# The Editor context MCP server

A coding agent running inside one of the app's floating terminals is otherwise
**blind to what the user is looking at** in the editor: which file has focus,
what is selected, which tabs are open, what was edited last. This server exposes
that live editor state over MCP, so an agent can anchor to the user's current
focus without them pasting a path or a snippet.

It runs as `cb-app mcp-editor`: the **same executable**, self-dispatched over
stdio, exactly as `record-intent`, `quality-gate`, `mcp-sql`, `mcp-browser`,
`mcp-tasks` and `mcp-roslyn` are — the eighth self-dispatch mode. There is no
second artifact to keep in step and no interpreter to depend on being installed.
The server name is `code-basics-editor`.

It is the **twin of [the Roslyn MCP server](roslyn-mcp-server.md)**: a shim that
forwards each tool call over a per-process named pipe to the running application,
which owns the answer. The line framing, routing and config writers are reused
from `mcp::` rather than copied. There is one difference that shapes the whole
feature, described next.

## The state lives in the frontend, so it is *pushed* into the backend

Every other MCP server answers from data the backend already holds — a SQL
connection store, a warm language-server session, a task file. **Editor state is
different: it lives in the React frontend.** Which tab is active, where the caret
is, what is selected, which lines are visible — none of that exists in Rust until
the frontend tells it.

So the frontend *pushes* the editor context into `AppState` (the same shape the
browser panel uses for its automation consent), through the `set_editor_context`
command, debounced, and **only while the feature is on**. The pipe host then
reads it back to answer an agent. Two consequences:

- The push carries a workspace `root`; a push whose workspace has since closed is
  a harmless no-op (the slot is gone, the data is dropped).
- Because the truth is a pushed snapshot rather than a live query, an agent's
  answer is only ever as fresh as the last push — which for a debounced push is
  effectively "the moment the user last moved". This is fine for anchoring an
  agent to focus; it is not a live cursor stream.

## Privacy: a feature flag, default on, and a two-fold gate

This server hands an agent **the contents of what the user is looking at** — the
active file path, the open and recently edited paths, and *the text they have
selected*. That is more intimate than the other read-only servers, so it is
gated by an optional feature, `EditorContextMcp` (id `editorContextMcp`),
**default on** but switchable off in Settings → Plugins / Features.

The gate is **two-fold on purpose**, so neither half alone can leak:

1. **The frontend stops pushing** when the feature is off — nothing new reaches
   the backend.
2. **The pipe host re-checks the feature on every call** — so a context left in
   `AppState` by a push that happened *before* the feature was switched off is
   never served. A call while the feature is off answers
   `featureDisabled` (see the refusals below), which tells the agent to ask the
   user to switch it back on rather than reporting an empty editor.

Installing the server into an agent stays a **separate, previewed consent step**
(see below), independent of the feature flag.

## The tools

Four tools. **None takes any arguments** — unlike the Roslyn tools (which take a
`path` plus a position-or-symbol), every tool here reports the live state of the
*one workspace this server was installed for*. There is nothing to point at: the
tools report what the user is already looking at.

| Tool | Answers |
| --- | --- |
| `get_active_file` | the active tab's workspace-relative path, cursor (line/character), visible line range, and whether the tab has unsaved changes or is pinned |
| `get_selection` | the current selection's start and end (line/character) and the selected text itself, length-capped |
| `get_open_files` | every open tab: path, and whether it is active, has unsaved changes, or is pinned |
| `get_recent_files` | the files edited most recently, in edit-recency order (most recent first) |

Because the tools take no arguments, a client that sends spurious arguments is
**not** refused for them — inventing a shape error over arguments a tool ignores
would be a lie about what the tool needs. The only protocol error is a tool name
this server does not have.

### Positions

`line` is **1-based** (matching the editor gutter), `character` is **0-based
UTF-16 code units** — the app-wide convention. The frontend reads these straight
off CodeMirror (`Line.number` and the line's `from` offset) with no conversion,
so the wire matches the source. Every position-bearing answer ends with a
one-line reminder of the convention.

### The selection text is capped

A whole-file selection must not push megabytes through the pipe or into an
agent's context, so the selected text is capped at **4096 code points**
(`SELECTION_TEXT_CAP` in `editorContextLogic.ts`), surrogate-safe, **at the
source** in the frontend before it is ever sent. The recent-files list is capped
at 20 (`RECENT_FILES_CAP`).

## Every abstention is its own answer

The same abstain-rather-than-guess rule the SQL, Tasks and Roslyn servers apply,
and it matters here because a model reads the sentence, believes it, and acts. An
absence must never pass for a fact about the editor, and no path or position is
ever fabricated.

**Four refusal codes**, each its own answer:

| Code | Means |
| --- | --- |
| `noWorkspace` | the server was started with no `--workspace`, so there is no session to ask — reinstall it scoped to a repository |
| `featureDisabled` | the workspace is open but the `EditorContextMcp` feature is switched off, so no live state is being shared — ask the user to switch it back on |
| `noContext` | the feature is on but no editor state has been pushed yet (the app just opened, or no file has been touched) — ask again shortly |
| `noActiveFile` | `get_active_file` / `get_selection` asked about the focused file and there is no active tab — an honest refusal, not a fabricated path |

`noWorkspace`, `featureDisabled` and `noContext` come **before** any editor state
is rendered; `noActiveFile` comes from the render layer when there *is* state but
the focused-file tools have nothing to point at.

Three things are **genuine data, not refusals** — an empty selection, no open
tabs, and no recent files are each stated plainly as the complete answer (the way
a `Ready`-but-empty Roslyn result is data), so silence is never read as failure.

Every refusal is an MCP **tool execution error** the model can correct itself on,
never a JSON-RPC protocol error — protocol errors are reserved for an unknown
tool name and arguments of the wrong shape. **No internal error text is ever
forwarded**: a refusal names no path, no OS error and no log line.

## `--workspace` *is* the boundary

`cb-app mcp-editor --workspace <root>` names the **one repository** whose editor
this server may read, whatever window is focused — the Tasks/Roslyn pattern, not
the SQL server's filtering flag. An unscoped invocation reaches no session and
every tool answers `noWorkspace`. The install step bakes the workspace in, so it
is a real consent boundary: an agent configured for repo X cannot read repo Y's
editor.

An optional `--instance <pid>` disambiguates two windows with the **same**
repository open. It grants nothing — it narrows the choice of process before
anything is decided, and when several windows have the same repo open and no
`--instance` was given the server **refuses rather than picks** (`several_windows`),
because answering from a window the user is not looking at is worse than asking.

There are **five** instance-level refusals, kept apart because each is a
different thing for the user to do: `app_not_running`, `workspace_not_open`,
`several_windows`, `protocol_mismatch` (the running app and the configured
executable are different builds), and `editor_registry_tampered` — the registry
names a control pipe the pid does not imply, so something other than
code-basics wrote that entry. That last one is **refused, never corrected**,
because it is the only visible symptom of a local process trying to impersonate
the application to an agent.

## How the shim reaches the app

The application publishes a small user-global registry
(`editor-instances.json` under the user config directory;
`CB_EDITOR_INSTANCES_PATH` overrides) naming its pid, executable path, protocol
version, pipe, token and the workspace roots it has open. The pipe is
`\\.\pipe\code-basics.editor.<pid>`, **derived from a pid that has first been
checked against the recorded executable path** — the shim opens the *derived*
name, never the name the registry merely states, so a forged entry is caught
rather than followed. A per-launch token is presented on every request: a speed
bump that reduces "any local program" to "any program that can read this user's
config directory", granting nothing (the boundary is the baked-in `--workspace`).

Unlike the browser server, the editor pipe is **process-global** — opened once at
application startup and withdrawn at exit — so a running application always
carries a listener and there is no "panel closed" state.

## Read-only, and no per-call consent gate

This server gives an agent **read-only access to live editor state**. It reads
what is on the user's screen; it **never changes a file, and there is no tool
here that does** (no arbitrary "evaluate" or edit tool). Because the grant is
narrow and read-only, the trust level is the SQL/Tasks/Roslyn model: there is
**no per-call consent gate** — the consent decisions are the feature flag and the
one-time scoped install.

## Installing it into an agent

Wiring the server into an agent is a separate, previewed opt-in, surfaced from
the titlebar **Plugins** menu as an **Editor context MCP** panel
(`EditorMcpPanel.tsx`, a twin of `RoslynMcpPanel.tsx` reusing `mcpServerLogic`).

Unlike the always-on Roslyn panel, this row is **feature-gated on
`editorContextMcp`**: with the feature off there is nothing to share and nothing
to install against, so the Plugins row is omitted. The panel's install action
shows the exact final contents of every file it would write (`PlanPreview`)
before anything is written.

The install bridge reuses `mcp::install`'s config-writer merge wholesale, so the
target files are the same three as the other MCP servers:

| Provider / scope | File |
| --- | --- |
| Claude Code, project | `<root>/.mcp.json` |
| Claude Code, user | `~/.claude.json` |
| Codex, user | `$CODEX_HOME/config.toml` |

The entry is this executable plus `args` — `["mcp-editor", "--workspace", <root>]`
at project scope, or just `["mcp-editor"]` at user scope — as **separate fields**,
never a quoted command string, so an executable path containing a space, a quote
or a `&` is safe. Codex has no project scope. A **project**-scope install bakes
in the workspace (the consent boundary); a **user**-scope install names no
workspace, so it answers `noWorkspace` until you install it scoped to a
repository instead.

Writes go through `providers::apply_writes_atomically` (temp file + rename)
because `~/.claude.json` is large (~122 KB) and is rewritten continuously by a
running Claude Code, so a truncate-then-write could lose the whole configuration.

The caveats shown before writing are this server's own, and the **access note is
stated last so it is not scrolled past**: the grant is read-only access to your
live editor state for one repository — the active file and cursor, the open and
recently edited file paths, and *the text you have selected* — read-only, and
scoped to one repository. It also states the two runtime conditions: the
application must be running with that repository open, and the "Editor context
MCP" feature switched on, for the tools to answer.

## A known MVP limitation

Only the **active workspace tab's editor** reports cursor, selection and
viewport. The push model is single-active-editor, so under split docking a docked
*non-active* editor's caret is not reported. The open-files and recent-files
lists are unaffected; this limits only the focused-file position data.

## Where the code is

| Module | Holds |
| --- | --- |
| `crates/core/src/editor_context/argv.rs` | is this process the server or the application; `--workspace` / `--instance` (`SUBCOMMAND` = `mcp-editor`) |
| `crates/core/src/editor_context/wire.rs` | what travels on the control pipe, both directions |
| `crates/core/src/editor_context/instances.rs` | which running application has the requested workspace open; the pipe name |
| `crates/core/src/editor_context/liveness.rs` | is a registry entry's process still that executable |
| `crates/core/src/editor_context/tools.rs` | the four tools, their (argument-free) schemas, and how a call is read |
| `crates/core/src/editor_context/answer.rs` | the four pre-render refusals, kept apart |
| `crates/core/src/editor_context/render.rs` | turning a pushed `EditorContext` into the words an agent reads |
| `crates/core/src/editor_context/serve.rs` | the handshake and the answer envelopes; `SERVER_NAME`, `INSTRUCTIONS` |
| `crates/core/src/editor_context/install.rs` | the previewed installer over `mcp::install`'s writers; `ACCESS_NOTE` |
| `crates/core/src/model.rs` | the `EditorContext` IPC type (`EditorCursor`, `EditorViewport`, `EditorSelection`, `EditorOpenFile`, `EditorRecentFile`), key-pinned |
| `src-tauri/src/editor_context/` | the process-global pipe host, registry and agent-facing forwarder in the app |
| `src-tauri/src/commands/editor_context_mcp.rs` | the five install verbs plus the `set_editor_context` push |
| `src-tauri/src/mcp_editor.rs` | the `mcp-editor` stdio runner |
| `src/components/editorContextLogic.ts` | assembling `EditorContext`, edit-recency, equality (no identical re-push), the selection cap |
| `src/components/EditorMcpPanel.tsx` | the Plugins-menu install panel |

Everything in `cb-core` is pure and unit-tested headlessly with no MCP client and
no editor. See also [the Roslyn MCP server](roslyn-mcp-server.md) (this server's
twin), [the SQL MCP server](sql-mcp-server.md), [the Tasks plugin](tasks-plugin.md)
(whose `--workspace` boundary this one follows), and
[the Tauri command reference](../reference/mcp-servers.md#editor-context-mcp-server).
