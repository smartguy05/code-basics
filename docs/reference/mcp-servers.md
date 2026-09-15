# MCP server installation commands

The install/uninstall command bridges for the MCP servers this app self-dispatches
into (`cb-app mcp-sql`, `mcp-browser`, `mcp-roslyn`, `mcp-editor`, `mcp-build`). Each is
previewed exactly as the agent hooks are: a plan command touches nothing and is
what the confirmation dialog renders, and the apply commands write through
`providers::apply_writes_atomically` (temp file + rename) before re-reading the
status from disk. Split out of [the Tauri command reference](commands.md) so
neither file outgrows its size budget.

Common to all of them: nothing new crosses IPC — a status is exactly
`InstallScope | null`, and a plan is the same `InstallPlan`/`PlannedWrite` the
hook installers use. The entry is `command` = this executable + an `args` array as
**separate fields**, never a quoted command string. Codex has no project scope.
See also the per-server guides: [SQL](../guides/sql-mcp-server.md),
[Roslyn](../guides/roslyn-mcp-server.md),
[Editor context](../guides/editor-context-mcp.md),
[Build & Diagnostics](../guides/build-mcp-server.md); the Tasks server's install
bridge is documented with [the Tasks plugin](commands.md#tasks-plugin).

## SQL MCP server

`src-tauri/src/commands/mcp.rs` — installing the read-only SQL MCP server (and the browser MCP server) into an agent's configuration. Decisions live in `cb_core::mcp::install`. Writes go through `providers::apply_writes_atomically`, because `~/.claude.json` is large and is rewritten continuously by a running Claude Code.

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `mcp_server_status` | `provider: ProviderId` | `InstallScope \| null` | Where the server is installed for this workspace and provider (project wins over user), or `null` |
| `mcp_server_install_plan` | `provider: ProviderId, scope: InstallScope` | `InstallPlan` | The exact final contents of the write — Claude Code `<root>/.mcp.json` (project) or `~/.claude.json` (user), Codex `$CODEX_HOME/config.toml` (user only). **Touches nothing** — what the preview renders, caveats included |
| `install_mcp_server` | `provider: ProviderId, scope: InstallScope` | `InstallScope \| null` | Perform a confirmed install. The entry is `command` = this executable + `args` = `["mcp-sql"]` (plus `--workspace <root>` at project scope), so no path is ever quoted into a string. Returns the new status |
| `mcp_server_uninstall_plan` | `provider: ProviderId, scope: InstallScope` | `InstallPlan` | The exact change removing the server would make. **Touches nothing.** An empty `writes` means that configuration holds no entry of ours — the panel says so rather than disabling a button. Every other configured server survives |
| `uninstall_mcp_server` | `provider: ProviderId, scope: InstallScope` | `InstallScope \| null` | Perform a confirmed removal, backing the file up first. Returns the new status (`null` once removed) |
| `browser_mcp_status` | `provider: ProviderId` | `InstallScope \| null` | Where the **browser** MCP server is installed for this workspace and provider (project wins over user), or `null`. A different server name from the SQL one, so the two are reported separately |
| `browser_mcp_install_plan` | `provider`, `scope` | `InstallPlan` | Exactly what installing the browser server would write. Touches nothing. The caveats are the browser feature own ones, not the SQL server ones: what an agent gains here is the page the user is looking at, in their own logged-in session |
| `install_browser_mcp` | `provider`, `scope` | `InstallScope \| null` | Applies a confirmed browser-server install through `apply_writes_atomically`, then re-reads the status from disk |
| `browser_mcp_uninstall_plan` | `provider`, `scope` | `InstallPlan` | What removing it would rewrite. Zero writes means no entry of ours was there |
| `uninstall_browser_mcp` | `provider`, `scope` | `InstallScope \| null` | Applies a confirmed removal, then re-reads the status |

Codex has no project scope: `mcp_server_install_plan(codex, project)` is an error naming `$CODEX_HOME/config.toml`, rather than inventing a `<root>/.codex/config.toml` that would look installed and never be read.

## Roslyn MCP server

`src-tauri/src/commands/roslyn_mcp.rs` — installing the **Roslyn / LSP** MCP server (which exposes the app's warm per-workspace language-server session to an agent) into an agent's configuration, previewed exactly as the SQL and browser servers are. Decisions live in `cb_core::roslyn::install`, which reuses `mcp::install`'s merge wholesale. The server name is `code-basics-roslyn`. The entry's `args` = `["mcp-roslyn"]` (plus `--workspace <root>` at project scope) — `--workspace` **is** the consent boundary, so an agent configured for one repository cannot reach another's semantic model. See [the Roslyn MCP server guide](../guides/roslyn-mcp-server.md).

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `roslyn_mcp_server_status` | `provider: ProviderId` | `InstallScope \| null` | Where the Roslyn server is installed for this workspace and provider (project wins over user), or `null` |
| `roslyn_mcp_server_install_plan` | `provider: ProviderId, scope: InstallScope` | `InstallPlan` | The exact final contents of the write — Claude Code `<root>/.mcp.json` (project) or `~/.claude.json` (user), Codex `$CODEX_HOME/config.toml` (user only). **Touches nothing** — what the preview renders, read-only caveats included |
| `install_roslyn_mcp_server` | `provider: ProviderId, scope: InstallScope` | `InstallScope \| null` | Perform a confirmed install, then re-read the status from disk. A user-scope entry names no workspace, so it answers `noWorkspace` until scoped to a repository |
| `roslyn_mcp_server_uninstall_plan` | `provider: ProviderId, scope: InstallScope` | `InstallPlan` | The exact change removing the server would make. **Touches nothing.** An empty `writes` means that configuration holds no entry of ours. Every other configured server survives |
| `uninstall_roslyn_mcp_server` | `provider: ProviderId, scope: InstallScope` | `InstallScope \| null` | Perform a confirmed removal, backing the file up first, then re-read the status |

## Editor context MCP server

`src-tauri/src/commands/editor_context_mcp.rs` — installing the **Editor context** MCP server (which exposes the user's live editor state — active file, cursor, selection, open and recently edited files — to a coding agent) into an agent's configuration, previewed exactly as the SQL, browser and Roslyn servers are, plus the push that feeds it. Decisions live in `cb_core::editor_context::install`, which reuses `mcp::install`'s merge wholesale. The server name is `code-basics-editor`. The entry's `args` = `["mcp-editor"]` (plus `--workspace <root>` at project scope) — `--workspace` **is** the consent boundary. The caveats' access note states, last so it is not scrolled past, that the grant hands an agent the contents of what the user is looking at (including selected text and open paths), read-only. See [the Editor context MCP server guide](../guides/editor-context-mcp.md).

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `editor_mcp_server_status` | `provider: ProviderId` | `InstallScope \| null` | Where the editor-context server is installed for this workspace and provider (project wins over user), or `null` |
| `editor_mcp_server_install_plan` | `provider: ProviderId, scope: InstallScope` | `InstallPlan` | The exact final contents of the write — Claude Code `<root>/.mcp.json` (project) or `~/.claude.json` (user), Codex `$CODEX_HOME/config.toml` (user only). **Touches nothing** — what the preview renders, read-only caveats included |
| `install_editor_mcp_server` | `provider: ProviderId, scope: InstallScope` | `InstallScope \| null` | Perform a confirmed install, then re-read the status from disk. A user-scope entry names no workspace, so it answers `noWorkspace` until scoped to a repository |
| `editor_mcp_server_uninstall_plan` | `provider: ProviderId, scope: InstallScope` | `InstallPlan` | The exact change removing the server would make. **Touches nothing.** An empty `writes` means that configuration holds no entry of ours. Every other configured server survives |
| `uninstall_editor_mcp_server` | `provider: ProviderId, scope: InstallScope` | `InstallScope \| null` | Perform a confirmed removal, backing the file up first, then re-read the status |
| `set_editor_context` | `root: String, ctx: EditorContext` | `()` | **The push.** Editor state lives in the React frontend, so the frontend feeds it into `AppState` (debounced) for the pipe host to read back — the twin of the browser panel's automation-consent push. Called **only while the `editorContextMcp` feature is on**; this command only records. The feature-off gate is two-fold — the frontend stops calling, and the pipe host re-checks the feature before serving — so a context left here by a feature switched off after a push is never exposed. A push whose workspace has since closed is a harmless no-op |

## Build & Diagnostics MCP server

`src-tauri/src/commands/build_mcp.rs` — installing the **Build & Diagnostics** MCP server (which lets a coding agent build one repository and read the build's structured diagnostics from the same build the editor runs) into an agent's configuration, previewed exactly as the SQL, browser, Roslyn and Editor context servers are. Decisions live in `cb_core::build::mcp::install`, which reuses `mcp::install`'s merge wholesale. The server name is `code-basics-build`. The entry's `args` = `["mcp-build"]` (plus `--workspace <root>` at project scope) — `--workspace` **is** the consent boundary, so an agent configured for one repository cannot build another. The caveats' access note states, last so it is not scrolled past, that a build writes compiler output (`bin/`, `obj/`) but changes no source file and there is no tool here that edits code. Gated by the `buildMcp` feature. See [the Build MCP server guide](../guides/build-mcp-server.md).

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `build_mcp_server_status` | `provider: ProviderId` | `InstallScope \| null` | Where the build server is installed for this workspace and provider (project wins over user), or `null` |
| `build_mcp_server_install_plan` | `provider: ProviderId, scope: InstallScope` | `InstallPlan` | The exact final contents of the write — Claude Code `<root>/.mcp.json` (project) or `~/.claude.json` (user), Codex `$CODEX_HOME/config.toml` (user only). **Touches nothing** — what the preview renders, caveats included |
| `install_build_mcp_server` | `provider: ProviderId, scope: InstallScope` | `InstallScope \| null` | Perform a confirmed install, then re-read the status from disk. A user-scope entry names no workspace, so it answers `noWorkspace` until scoped to a repository |
| `build_mcp_server_uninstall_plan` | `provider: ProviderId, scope: InstallScope` | `InstallPlan` | The exact change removing the server would make. **Touches nothing.** An empty `writes` means that configuration holds no entry of ours. Every other configured server survives |
| `uninstall_build_mcp_server` | `provider: ProviderId, scope: InstallScope` | `InstallScope \| null` | Perform a confirmed removal, backing the file up first, then re-read the status |
