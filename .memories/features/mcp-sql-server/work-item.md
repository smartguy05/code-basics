# Work item — SQL MCP server (read-only) + previewed MCP installation

Phases 2 and 3 of `~/.claude/plans/major-enchancements-add-witty-hopper.md`.
Phase 1 (F2 rename) is done; Phase 4 (browser) is a later workflow.

## Why

An agent cannot query the databases the user is already connected to. The SQL
console reaches three engines, classifies a statement as read-only and caps a
result set — but only a human at the panel can use any of it. Expose that to
Claude Code and Codex over an MCP stdio server, read-only, per-connection
opt-in, and install it through the same previewed/additive/backed-up rails the
intent and quality-gate hooks already use.

## Acceptance criteria

- **AC1** Every decision the MCP server needs is reachable from `cb-core`.
  The server is a *separate process with no `AppState`* and cannot call a
  `#[tauri::command]`.
- **AC2** A connection is invisible to agents unless the user opted it in.
  Absent key = consent withheld. No save/rename round-trip can raise it.
- **AC3** The agent path is read-only by construction — not by default, by the
  absence of any parameter that could say otherwise.
- **AC4** No connection string, driver text, file path or secret reaches an
  agent through any tool or refusal.
- **AC5** Every abstention stays a distinct answer (not-exposed vs not-found,
  ambiguous refuses rather than picks, a cap renders first).
- **AC6** Installation is previewed before it writes, additive, backed up, and
  states the caveats that are true (project `.mcp.json` needs user approval;
  `~/.claude.json` is rewritten live by Claude Code; Codex has no project
  scope).
- **AC7** Un-exposing a connection takes effect on the **next call**, with no
  restart.

## Corrections to earlier assumptions (verified on this machine)

1. `mcpServers` is **not** in `.claude/settings.json` (that holds only `hooks`).
   Claude project scope = `<root>/.mcp.json`; Claude user scope = top-level
   `mcpServers` in `~/.claude.json`; Codex = `$CODEX_HOME/config.toml`
   `[mcp_servers.<name>]`.
2. Workspace MSRV floor is **1.88**, not 1.82.
3. A project `.mcp.json` does not load until the user approves it
   (`enabledMcpjsonServers`/`disabledMcpjsonServers` in `~/.claude.json`).
4. `toml_edit` 0.25.13 is already in `Cargo.lock` transitively — zero new crates.
5. MCP stdio framing is **newline-delimited JSON**, so `lsp/framing.rs`
   (Content-Length) is the wrong layer. `lsp/jsonrpc.rs` is reusable.
6. `rmcp` 3.2.0 declares `rust-version = 1.88`. Gate it with
   `cargo add rmcp && cargo check -p cb-core` before committing to it.
