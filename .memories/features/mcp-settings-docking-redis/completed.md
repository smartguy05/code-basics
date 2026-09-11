# Completed

## Feature 1 — MCP settings page (per-server + per-tool) — DONE (all gates green)

New `crates/core/src/tool_gate/` module (mod.rs, store.rs, store_tests.rs,
tool_gate_tests.rs). Mirrors `features/store.rs` idioms; **no seed** (permissive
default: absent/corrupt = all tools enabled — a preference, not a security gate).
- `ToolGateFile { version, enabled: BTreeMap<"server/tool", bool> }`; composite
  key keeps same-named tools independent; unknown keys preserved.
- `ServerId {Sql,Tasks,Roslyn,Browser}` with `descriptors()` pulling each server's
  real `tools::descriptors()`. `is_enabled` (absent=on), `filter_descriptors`
  (order-preserving), `list()`, `disabled_tool_sentence/result`, `DISABLED_CODE`.
- Wire types `McpServerToolsInfo`/`McpToolInfo` (camelCase, pinned by
  `serialisation_shape_pins_the_wire_keys`).

Runtime enforcement (two points per server):
- `serve::tools_list_result(&ToolGateFile)` in all 4 serve.rs → filter_descriptors.
- Dispatch refusal: mcp_sql.rs + mcp_tasks.rs return `disabled_tool_result`;
  mcp_browser.rs + mcp_roslyn.rs return `serve::disabled_tool_answer` after
  `is_known`. Each runner has a `load_gate()` re-read per call.

IPC + frontend:
- `commands/mcp_tools.rs` (list_mcp_tools, set_mcp_tool) registered in lib.rs.
- `ipc/types.ts` + `ipc/api.ts` (listMcpTools/setMcpTool).
- `components/mcpToolsLogic.ts` (+ 12 vitest tests): serverToggleState,
  nextServerValue, enabledCount, toolBlockedByServer, toolKey.
- `components/McpToolsPage.tsx` wired into SettingsDialog as the "MCP tools" page.
  Server rows drive the feature flag (sql→mcpSqlServer, tasks→tasks,
  browser→webBrowser, roslyn→null "always on"); per-tool checkboxes write-through.
- CSS in styles.css (.mcp-tools-page etc.). Docs: commands.md + INDEX regenerated.

Gates: cargo test -p cb-core (all pass), pnpm typecheck, pnpm test mcpToolsLogic
(12), cargo fmt --check, docs:check — all green.

## Feature 2 — Docking (SQL + browser) — DONE (2084 frontend tests, typecheck clean)

- `regionLayoutLogic.ts`: `DockableKind` += "sql"|"browser"; `DOCKABLE_KINDS`;
  `dockedIdsOfKind`/`isPanelDocked`; `pruneLayout(layout, valid-map)` (signature
  changed — absent kind = none valid); `isRegion` accepts new kinds.
- `RegionContext.tsx`: `panelIds` prop; `validFor()` builds the per-kind map;
  `dockedPanels` in API; prune on terminal/panel changes.
- `SqlPanel.tsx`: `SqlView` via `StablePortal` into region slot; header dock
  button; region strip meta; `.sql-docked` CSS scopes the two fixed overlays.
- `BrowserPanel.tsx`: page-lifecycle effects stay top-level; stateless shell via
  `createPortal` into slot when docked; `sync` gains `isDocked` (minimized→false,
  `occludedByPanels` raise-unaware); re-sync effect on `[isDocked,slotNode]`;
  `.browser-docked` CSS. **No Rust changes** — `browser_set_bounds` reused.
- `WorkspaceTab.tsx`: `panelIds` passed to `RegionProvider`.
- Note: NUL separator in `terminalIds.join("\0")` (pre-existing) breaks Edit-tool
  space matching in RegionContext.tsx — use python/sed for edits near it.

## Feature 3 — Redis plugin — DONE (cb-core tests pass, 2096 frontend tests, fmt/typecheck/docs green)

- cb-core `redis/`: dsn (parse/redact, RedisTarget/RedisConnectionDisplay,
  looks_like_redis), store (RedisConnection/File, shares sql SecretSource, upsert
  ignores consent), discover (reuses sql readers; ConnectionStrings:*+.env),
  model (RedisType/KeyInfo/Value/ScanPage/StatusKind, key-pinned), ops
  (ReadOp/WriteOp/WritePlan token; plan_write is the sole write gate), driver
  (redis-rs MultiplexedConnection, rustls; caps reported; errors redacted).
- redis/mcp/: argv (mcp-redis), answer (McpRefusal 10 distinct codes; not-exposed
  ≠ writes-not-allowed), expose (reuses connection_handle; find_exposed), tools
  (12: 4 read + 8 write; is_write), execute (plan: read=no consent, write=plan_write
  gate; resolve_target), serve (initialize/list; reuses mcp::serve route/envelopes),
  render.
- src-tauri: mcp_redis.rs runner (+ lib.rs mcp-redis branch), commands/redis.rs
  (23 commands; RedisConnectionView/redact_connection; UI writes via plan_write(true)).
- Cargo: `redis = 0.27, tokio-rustls-comp + tls-rustls-webpki-roots + streams`.
- Frontend: types.ts + api.ts wrappers; redisLogic.ts (+test), redisPanelLogic.ts;
  RedisPanel.tsx (connection mgmt w/ expose+writes toggles, discover, key browser
  w/ scan+match+load-more, type-aware value editor for all 6 types); plugin row
  (feature:null), WorkspaceTab host + openRedis, App action; CSS.
- docs/guides/redis-plugin.md, commands.md, INDEX.

## NOTE for review
McpToolsPage `toggleServerTools` loops setMcpTool per tool (N IPC calls). Fine for
small tool counts (4-13); could batch later if it matters.
