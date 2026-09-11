# Redis plugin

A RedisInsight-style panel plus a write-capable MCP server, modelled directly on
the [SQL console](sql-mcp-server.md). It lets you browse and edit a Redis server
the workspace already talks about, and (with two explicit consents) lets a coding
agent read from and write to it over MCP.

## Where it lives

All decisions are in `crates/core/src/redis/`, tested with no server installed:

- `dsn.rs` — parse/redact a connection string (`redis://`, `rediss://`, and the
  StackExchange.Redis `host:port,ssl=true,…` host-list form). The only view that
  leaves the module is the password-free `RedisConnectionDisplay`; passwords are
  redacted through the SQL module's `dsn::redact`.
- `store.rs` — saved profiles under `<config>/code-basics/redis-connections.json`
  (user-global, outside any repo, for the same reason SQL connections are). It
  **shares `sql::store::SecretSource`** — a connection is found by *reference*
  (an `appsettings.json`/user-secrets/`.env` key), not by value, so the app
  usually stores no secret at all. Two consent flags, both `#[serde(default)]`
  off: `expose_to_agents` (may an agent see it) and `allow_writes` (may an agent
  write). Each moves only through its own command; `upsert` ignores both.
- `discover.rs` — finds Redis connections in appsettings/user-secrets/`.env`,
  reusing the SQL readers. Filesystem only; the architecture graph never reads a
  value. **Limitation:** only `ConnectionStrings:*` entries and `.env` keys are
  scanned — a custom section (`Redis:Configuration`) is not found yet.
- `ops.rs` — the read/write split and the **one** write gate: a `WriteOp` runs
  only once `plan_write(allow_writes, op)` has minted a `WritePlan`. Redis has no
  server-side read-only mode, so this planning-tier gate is the whole of it.
- `model.rs` — the wire types (`RedisType`, `RedisKeyInfo`, `RedisValue`,
  `ScanPage`, …), camelCase, keys pinned by tests.
- `driver.rs` — the only socket-opening layer (redis-rs, rustls TLS). Values are
  capped and the cap reported (`truncated`), never silently shortened.
- `mcp/` — the MCP server (`argv`/`answer`/`expose`/`tools`/`execute`/`serve`/
  `render`), self-dispatched as `cb-app.exe mcp-redis`.

The bridge is `src-tauri/src/commands/redis.rs`; the runner is
`src-tauri/src/mcp_redis.rs`; the panel is `src/components/RedisPanel.tsx` with
its decisions in `redisLogic.ts` / `redisPanelLogic.ts`.

## The panel

Open it from the titlebar **Plugins** menu → **Redis** (always-on, like the
Roslyn server — there is no on/off feature flag). **Discover** scans the open
codebase for connections; **Add** saves one. Select a connection, **Scan** the
keyspace (with an optional `MATCH` glob and **Load more** paging), then pick a
key to view and edit its value. The panel itself may always write — `allow_writes`
gates only the agent path.

## The MCP server

Point an agent at `cb-app.exe mcp-redis --workspace <root>`. Its tools:

- **Reads** (always available on an exposed connection): `redis.list_connections`,
  `redis.scan_keys`, `redis.get_key`, `redis.connection_status`.
- **Writes** (refused unless the connection has *both* `expose_to_agents` and
  `allow_writes`): `redis.set_string`, `redis.hash_set`, `redis.list_push`,
  `redis.set_add`, `redis.zset_add`, `redis.stream_add`, `redis.delete_key`,
  `redis.expire`.

Exposure and writes are separate consents, both off by default and both re-read
from the store on every call, so revoking either bites the next request. A
connection you have not exposed is invisible to the agent; a connection you have
exposed but not granted writes to is read-only. No driver message ever crosses to
the agent — a failure is a category, not a string.

> One-click install of this server into an agent's config (the way the SQL,
> Tasks, browser and Roslyn servers install) is not wired up yet — add the
> `mcp-redis` command to your `.mcp.json` / `config.toml` by hand for now.
