# Related docs

## The plan

- `~/.claude/plans/major-enchancements-add-witty-hopper.md` — Phases 2 and 3 are
  this work item. Phase 1 (F2 rename) is done; Phase 4 (browser) is later.

## In-repo

- `CLAUDE.md` — "a `#[tauri::command]` body is untestable here, so it must not
  decide anything"; the Rust↔TS type contract; the abstain rule.
- `docs/architecture/ipc-contract.md` — the key-pinning obligation that governs
  `exposeToAgents`.
- `docs/reference/commands.md` — must track `generate_handler!` in
  `src-tauri/src/lib.rs`; `sql_set_expose_to_agents` added.
- `docs/INDEX.md` — generated; consult first when locating code.

## Sibling work items to copy mechanism from, not just shape

- `.memories/features/sql-console/` — the subsystem this extends; its three
  governing rules are in `crates/core/src/sql/mod.rs`'s module docs.
- `.memories/features/sql-connection-rename/` — why `rename` is its own verb and
  why `upsert` ignores what the payload claims. `expose_to_agents` copies that
  mechanism exactly.
- `.memories/features/installable-quality-gate-hook/` — `qgate/install.rs`,
  `settings_merge.rs`, `SHARED_EXE_PATH_NOTE`, and the self-invoking-subcommand
  dispatch in `qgate/mod.rs` that `mcp/argv.rs` is modelled on.
- `.memories/features/lsp-usages/` — `lsp/jsonrpc.rs` (transport-agnostic, and
  the fallback if `rmcp` will not build) and `lsp/framing.rs` (Content-Length —
  the **wrong** layer for MCP, which is newline-delimited).

## External, verified on this machine

- MCP stdio: messages are newline-delimited and MUST NOT contain embedded
  newlines. Tool failures are `isError: true` tool results, not JSON-RPC errors
  — clients SHOULD hand those to the model for self-correction. JSON-RPC errors
  are for an unknown tool name and malformed arguments.
- Claude Code config: project `<root>/.mcp.json`; user `~/.claude.json`
  (top-level `mcpServers`, plus `enabledMcpjsonServers` /
  `disabledMcpjsonServers` recording project approval).
- Codex config: `$CODEX_HOME/config.toml`, `[mcp_servers.<name>]` with `command`
  and `args`. No project scope.
