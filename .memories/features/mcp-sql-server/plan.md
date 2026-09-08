# Plan

Source of truth: `~/.claude/plans/major-enchancements-add-witty-hopper.md`,
Phases 2 and 3. This file tracks the step breakdown and what is done.

## Step 1 — move the shareable decisions out of the bridge, add consent ✅

- `crates/core/src/sql/plan.rs` + `plan_tests.rs`: `Plan`, `Refusal`,
  `Refusal::sentence()`, `engine_name`, `guard_engine`, `execution_plan`,
  `refusal_reason`. `Refusal::to_event()` stayed in the bridge as the free
  function `commands::sql::refusal_event` (orphan rule — it cannot be an `impl`
  on a foreign type, and it needs `SqlEvent` + `failed_event`'s redaction).
- `crates/core/src/sql/catalog.rs` + `catalog_tests.rs`: `SqlObject`,
  `SqlColumn`, `SqlObjectKind`, `object_catalog_query`, `column_catalog_query`,
  `sql_literal`, `table_objects`, `table_columns`, plus the new
  `identifier`/`IdentifierRefusal`/`MAX_IDENTIFIER_BYTES`. The bridge re-exports
  `SqlObject as SqlObjectView` / `SqlColumn as SqlColumnView`, so the emitted
  JSON is byte-identical and `types.ts` needed no edit for them.
- `commands::sql::column_query_for` is the new decision seam: it runs
  `catalog::identifier` over schema and table before building the query, so the
  command body still decides nothing.
- `store::SqlConnection.expose_to_agents` (`#[serde(default)]`),
  `SqlConnectionView.expose_to_agents`, `set_expose_to_agents`,
  `sql_set_expose_to_agents`, registered in `lib.rs`, `sqlSetExposeToAgents` in
  `api.ts`, `exposeToAgents` in both `types.ts` interfaces.

## Step 2 — transport and dispatch (next)

`crates/core/src/mcp/ndjson.rs` (newline-delimited framing; a malformed line
does **not** poison, `LineTooLarge` does) and `mcp/argv.rs`
(`SUBCOMMAND = "mcp-sql"`, `MARKER = "code-basics-mcp-sql"`, modelled on
`qgate/mod.rs`), then a third `if` in `lib.rs::run()`.

## Step 3 — tool surface, answers, forced read-only

`mcp/tools.rs` (six tools), `mcp/answer.rs` (`McpRefusal`, every abstention
distinct), `mcp/expose.rs` (`AgentConnection` — a third, narrower view than
`SqlConnectionView`), `mcp/execute.rs::agent_plan` (**no** `writes_allowed`
parameter at all).

## Step 4 — the runner

`src-tauri/src/mcp_sql.rs`, modelled on `qgate_run.rs`. Every log line to
stderr. One connection per call, closed after; `store::load` re-read per call so
revocation is immediate.

## Step 5 — Phase 3, previewed installation

`mcp/install/{mod,mcp_merge,plan}.rs`, `toml_edit` for Codex,
`providers::apply_writes_atomically`, `commands/mcp.rs`, `McpServerPanel.tsx` +
`mcpServerLogic.ts`, feature flag in seven places.

## The dependency gate

Before writing any of Step 2/3 on top of `rmcp`: `cargo add rmcp` then
`cargo check -p cb-core`, and read the *first* error. Fall back to hand-rolled
legacy-only over `lsp/jsonrpc.rs` if it will not build on 1.88 — everything in
Steps 1 and 3 is transport-independent and does not change either way.
