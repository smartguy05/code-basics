# The Tasks plugin

Tasks is an optional plugin that gives each codebase a personal to-do list, plus
a second consumer of that list: a Model Context Protocol server so a coding agent
can read and write the same tasks. It mirrors the other floating-panel-plus-MCP
features (Notes for the panel shape, the SQL and browser servers for the MCP), so
the patterns below will look familiar.

The plugin is **on by default**, like the other features. Turn it off in the
Plugins menu and the panel unmounts; turn it on and a **Tasks** row appears in the
titlebar Plugins menu that opens the floating panel for the active codebase.

## The store is per-codebase and gitignored

Each task list lives at `<root>/.code-basics/tasks.json`, and `tasks.json` is added
to the gitignore the app maintains (exactly like `agent-runs.json`), so it stays
local — a task list is a personal working note, not something the team shares. A
missing or corrupt file loads as an empty list rather than an error, and every save
is atomic (`.tmp` + rename, with a `.bak` guard before an empty overwrite), so a
crash mid-write does not lose the list.

A `Task` carries `id`, `title`, `body`, `status` (`open`/`done`),
`owner` (`me`/`ai`), and `createdAtMs`/`updatedAtMs`. The wall clock is stamped at
the command edge (never in `cb-core`), so the pure store helpers stay testable with
a fixed time. Ids are uuids, so the panel and an agent creating a task in the same
millisecond cannot collide.

## Assign to AI, and assign back

Each task has an owner badge and two buttons.

- **Assign to AI** sets the task's `owner = ai` (persisted), then launches the agent
  in an **interactive PTY terminal** — the same path "Ask the codebase" uses:
  `agent_interactive_command` builds the `claude -- <prompt>` / `codex -- <prompt>`
  argv (the question crosses as one argv entry, never as typed keystrokes), and
  `terminal_open` opens a real terminal running it. The prompt is the task's title
  and body, joined with a blank line so the agent reads a heading then the detail
  (`tasksLogic.taskPrompt`).
- **Assign to me** sets `owner = me` and does nothing else — no agent is launched.

Only the owner is recorded by the backend; launching the agent is the frontend's
job, so `assign_task` stays a plain store write.

## The MCP server is write-capable, on purpose

The SQL MCP server forces read-only and has no argument that lifts it. The Tasks
server is the opposite: **creating, updating, completing, assigning and deleting
tasks is the whole point**. Managing a personal, gitignored to-do list is a
low-stakes operation, and the value of letting an agent keep its own list in step
with the user's is exactly the writes. There is no read-only enforcement layer and
no consent gate on execution — a call is applied to the store directly.

It runs as `cb-app mcp-tasks`: the same executable, self-dispatched over stdio,
exactly as `record-intent`, `quality-gate`, `mcp-sql` and `mcp-browser` are. The
server name is `code-basics-tasks`.

### The scope *is* the boundary

`--workspace <root>` is not a listing filter here (as it is for SQL): the store is
per-workspace, so the flag names the one `tasks.json` this server reads and writes.
An unscoped invocation has no store and every tool refuses with *no workspace*
rather than guessing a directory. The install step bakes the workspace in — that is
the consent boundary: an agent configured for one repository cannot reach another's
tasks.

### The tools

| Tool | Does |
| --- | --- |
| `list_tasks` | every task in this workspace |
| `get_task` | one task by id |
| `create_task` | add an open task |
| `update_task` | overwrite a task's title and body |
| `complete_task` | set a task done or back to open |
| `delete_task` | remove a task |
| `assign_task` | set a task's owner (`me`/`ai`) |

`assign_task` records the owner only; unlike the panel button it does **not** launch
an agent, because there is no interactive terminal on the server side to launch one
into.

### The abstain discipline is kept

Every answer that is not data is its own variant: a missing task is *not found*
rather than a silent no-op, an unscoped server refuses with *no workspace*, and no
internal error text (an fs path, a serde message) is ever forwarded — a write that
could not be persisted crosses as a *store-write-failed* refusal with the words
dropped, exactly as the SQL server drops driver text. Each of these is an MCP tool
execution error the model can correct itself on, not a JSON-RPC protocol error;
protocol errors stay reserved for an unknown tool name or arguments of the wrong
shape.

## Installing it into an agent

Wiring the server into an agent is a separate, previewed opt-in — the panel's
**Install for agent…** action shows the exact final contents of every file it would
write (`PlanPreview`) before anything is written. The install bridge reuses
`mcp::install`'s config-writer merge wholesale, so the target files are the same as
the SQL and browser servers: Claude Code `<root>/.mcp.json` (project) or
`~/.claude.json` (user), Codex `$CODEX_HOME/config.toml` (user only, no project
scope). The entry is this executable + `args` = `["mcp-tasks", "--workspace", <root>]`,
so no path is ever quoted into a string, and writes go through
`providers::apply_writes_atomically` because `~/.claude.json` is large and rewritten
continuously by a running Claude Code.

## Where the code is

| Module | Holds |
| --- | --- |
| `crates/core/src/tasks/mod.rs` | the `Task`/`TasksFile` model and the pure CRUD helpers |
| `crates/core/src/tasks/store.rs` | path resolution + tolerant load / atomic save |
| `crates/core/src/tasks/mcp/argv.rs` | is this process the server or the application |
| `crates/core/src/tasks/mcp/tools.rs` | the seven tools, their schemas, and call parsing |
| `crates/core/src/tasks/mcp/execute.rs` | applying one call to the store |
| `crates/core/src/tasks/mcp/answer.rs` | every refusal, kept apart |
| `crates/core/src/tasks/mcp/render.rs` | the words an agent reads |
| `crates/core/src/tasks/mcp/serve.rs` | version negotiation, routing, the envelope |
| `crates/core/src/tasks/mcp/install.rs` | the previewed installer over `mcp::install`'s writers |
| `src-tauri/src/commands/tasks.rs` | the CRUD + MCP-install command bridge |
| `src-tauri/src/mcp_tasks.rs` | the `mcp-tasks` stdio runner |
| `src/components/TasksPanel.tsx`, `tasksLogic.ts` | the floating panel and its pure UI decisions |

Everything in `cb-core` is pure and unit-tested headlessly with no MCP client. See
also [the SQL MCP server](sql-mcp-server.md), whose installer this one follows, and
[the Tauri command reference](../reference/commands.md#tasks-plugin).
