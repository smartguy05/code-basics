# TODOs

## Feature 1 — MCP settings page — ✅ DONE (all gates green). See completed.md.

## Feature 2 — Docking — ✅ DONE (2084 frontend tests pass, typecheck clean)
- DockableKind += "sql"|"browser"; pruneLayout(layout, valid-map); dockedIdsOfKind
  /isPanelDocked; RegionProvider gains panelIds; dockedPanels in API.
- SqlPanel: SqlView via StablePortal into region slot; dock button; .sql-docked
  CSS scopes overlays; region strip meta.
- BrowserPanel: page-lifecycle effects stay top-level; stateless shell via
  createPortal into slot; sync gains isDocked (minimized=false, occludedByPanels
  raise-unaware); re-sync on [isDocked,slotNode]; .browser-docked CSS. NO Rust
  changes (browser_set_bounds reused). docs/architecture/split-docking.md updated.

## Feature 3 — Redis plugin — ✅ DONE (all gates green)
- redis/ (dsn, store [shares sql SecretSource], discover, model, ops [WritePlan
  gate], driver [redis-rs rustls]) + redis/mcp/ (argv/answer/expose/tools/execute/
  serve/render). mcp_redis.rs runner + lib.rs mcp-redis branch. commands/redis.rs
  bridge + registrations. redis crate added (0.27, tokio-rustls-comp).
- Frontend: RedisPanel + redisLogic/redisPanelLogic (+tests), IPC types + api,
  plugin row (always-on like Roslyn, feature:null — no FeatureId, no installer
  edits), WorkspaceTab host + openRedis handle, App action. CSS.
- docs/guides/redis-plugin.md + commands.md. INDEX regenerated.

### Deliberate scope decisions (Feature 3)
- **No FeatureId** for Redis (always-on like Roslyn). Rationale: adding one forces
  a 6th NSIS installer checkbox (layout overflow risk) + contract-test updates for
  marginal value. No in-app on/off toggle, matching Roslyn.
- **No one-click MCP install UI** yet — the mcp-redis server works; add it to
  .mcp.json/config.toml by hand. Wiring mcp::install for redis is a clean
  follow-up (mirror sql/tasks/roslyn install commands + a plugin.redisMcp row).
- Discovery covers ConnectionStrings:* + .env only (not custom sections like
  Redis:Configuration). Values carried lossy-UTF8; caps reported.
- Redis panel is floating only (not dockable) — docking was Feature 2's SQL+browser.
