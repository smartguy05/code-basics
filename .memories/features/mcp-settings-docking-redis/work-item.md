# Work item: MCP settings + panel docking + Redis plugin

Three enhancements requested together (2026-09-11). Plan approved:
`~/.claude/plans/frolicking-soaring-bumblebee.md`.

## Feature 1 — MCP settings page (per-server + per-tool)
Settings page toggling each MCP server AND each individual tool. Per-server already
exists via feature flags; **per-tool gating is new** — a `tool_gate` store every
server filters against at `tools/list` and `tools/call`.
- Per-tool disable: confirmed by user.

## Feature 2 — Docking for SQL console + web browser
Both fully dockable (user chose "Both, fully"). SQL is DOM (StablePortal like
editors); browser is native WebView2 surface (rect-driven via existing
`browser_set_bounds`, no StablePortal).

## Feature 3 — Redis plugin (mirrors SQL console)
Discovery from appsettings/secrets, key browser + edit for 6 types, MCP server that
reads AND writes. Writes behind per-connection consent (`expose_to_agents` +
separate `allow_writes`, both off by default). "Core browser + edit" scope.

## Build order: 1 → 2 → 3. Each independently shippable + version-bumpable.
