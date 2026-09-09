# Completed

## Step 1 — the shareable decisions and agent consent (2026-09-04)

### New in `cb-core`

- `crates/core/src/sql/plan.rs` + `plan_tests.rs` (10 tests) — `Plan`,
  `Refusal`, `Refusal::sentence()`, `engine_name`, `guard_engine`,
  `execution_plan`, `refusal_reason`. Moved verbatim from
  `src-tauri/src/commands/sql.rs`, then the `EngineUnsupported` sentence was
  fixed.
- `crates/core/src/sql/catalog.rs` + `catalog_tests.rs` (11 tests) —
  `SqlObject`, `SqlColumn`, `SqlObjectKind`, `object_catalog_query`,
  `column_catalog_query`, `sql_literal`, `table_objects`, `table_columns`
  (moved, with their three tests), plus the new `identifier`,
  `IdentifierRefusal` (7 variants) and `MAX_IDENTIFIER_BYTES = 128`.
- `sql/mod.rs` — both modules registered and documented in the layout list.
- `sql/store.rs` — `SqlConnection.expose_to_agents`, `#[serde(default)]`, with
  the "an agent can read anything that login can read" caveat in its doc
  comment. `store_tests.rs` gained `expose_to_agents_defaults_to_false` and
  `expose_to_agents_survives_a_round_trip_when_granted`, and
  `serialisation_shape_pins_the_wire_keys` gained `exposeToAgents`.

### The defect that started it

`Refusal::EngineUnsupported`'s sentence said *"SQLite is the only engine
currently supported"* for Postgres and SQL Server, both of which have shipped
drivers. Reproduced by
`engine_unsupported_names_the_engine_and_not_a_stale_supported_list` (watched it
fail naming the stale phrase), then re-phrased from `engine_name` and
`driver::ENGINES` filtered by `driver::for_engine`.

### Changed in `src-tauri`

- `commands/sql.rs` — the moved items deleted; `SqlObjectView`/`SqlColumnView`/
  `SqlObjectKind` re-exported from `cb_core::sql::catalog` so the JSON is
  byte-identical; `Refusal::to_event()` became the free function
  `refusal_event(&Refusal)` (orphan rule); new `column_query_for` runs
  `catalog::identifier` before building a column query; `SqlConnectionView`,
  `redact_connection`, `upsert`'s `None` arm and the new `set_expose_to_agents`
  carry exposure; new `sql_set_expose_to_agents` command; module docs rewritten
  for "what moved and why" and "consent is its own verb — twice". Four moved
  tests removed, eight added.
- `lib.rs` — `sql_set_expose_to_agents` registered.

### Changed in `src/`

- `ipc/types.ts` — `exposeToAgents` on `SqlConnectionProfile` (sent and ignored)
  and `SqlConnectionView`. Added **after** both Rust key pins.
- `ipc/api.ts` — `sqlSetExposeToAgents`.
- `views/sqlViewLogic.ts` and three test fixture files updated.
- `docs/reference/commands.md` + regenerated `docs/INDEX.md`.

### Gate, all run and seen green

- `cargo test -p cb-core` — 3123 lib + all integration suites, 0 failed.
- `cargo test -p cb-app` — 137 passed.
- `cargo fmt --check` — clean. `cargo clippy -p cb-core --all-targets` and
  `-p cb-app --all-targets` — no warnings.
- `pnpm typecheck` — clean. `pnpm test` — 71 files, 1856 tests, 0 failed.
- `pnpm docs:index`, `pnpm docs:check` — passed.

Four mutation checks run and reverted; each failed the test named for it. See
`notes.md`.

### Not done in this step (in `todos.md`)

The `SqlConnectionPicker.tsx` toggle and badge for `exposeToAgents`. The command
and the wire type exist; no UI moves the flag yet.


---

# Step 2 — the MCP server's decisions (2026-09-04)

## The dependency gate: PASSED, no fallback

`cargo add rmcp` resolved **3.2.0**, which declares `rust-version = "1.88"` —
exactly this workspace's floor. `cargo check -p cb-core` completed clean with no
error at all, so the hand-rolled legacy-only fallback was **not** needed.

Feature scope was then trimmed, and it matters: under rmcp's default features
(`base64`, `macros`, `server`) `Cargo.lock` gained **ten** packages; with
`default-features = false` it gains **two** — `rmcp` and the `futures` facade.
`cb-core` uses only `rmcp::model`, which is behind no feature. `server` would
pull in the service runtime and a transport, and neither belongs in a crate with
no I/O; the runner in `src-tauri` is what turns those on. Recorded in
`Cargo.toml` in the style of the `sqlx`/`sqlparser` pins.

## New modules (all pure, all tested headlessly)

- `mcp/ndjson.rs` — line-delimited framing. The design note is the **contrast**
  with `lsp/framing.rs`: a newline is a resynchronisation point, so a malformed
  line is one bad message and the decoder does **not** poison; `LineTooLarge`
  **does**, because the terminator was never seen.
- `mcp/argv.rs` — the third self-dispatch mode, either-or acceptance like the
  other two, plus the disjointness drift alarm.
- `mcp/expose.rs` — `AgentConnection { id, name, engine }`, `listing`,
  `exposed_now`, `find_exposed`, `empty_listing_refusal`.
- `mcp/answer.rs` — `McpRefusal` (15 variants), `ConnectionStatusKind` (11),
  `AGENT_NOTE`, `restate_for_agent`.
- `mcp/execute.rs` — `agent_plan` (no `writes_allowed` parameter),
  `agent_connect_spec`, `agent_enforcement`.
- `mcp/render.rs` — cap first, `NO_ROWS`, the enforcement honesty surface.
- `mcp/tools.rs` — six tools as `rmcp::model::Tool`, `parse_call`,
  `refusal_result` (tool error, never protocol error).

## Bridge

- `commands/sql.rs::connection_status_kind` — the exhaustive, message-**dropping**
  map from `SqlTestOutcome`. It lives in the bridge because `SqlTestOutcome`
  does. The plan said "all thirteen"; the enum actually has **eleven** variants,
  and the test asserts the two enums are the same length rather than a literal.
- `src-tauri/src/lib.rs::run()` — the third predicate, before the Tauri builder.
  Placeholder body (stderr + exit 1) until the runner lands.

## Gate, run and seen

- `cargo test -p cb-core --lib` — **3211 passed, 0 failed**.
- `cargo test -p cb-app --lib` — **139 passed, 0 failed**.
- `cargo fmt --check` clean; `cargo clippy -p cb-core --all-targets` and
  `-p cb-app --all-targets` — **zero** warnings.
- `pnpm docs:index`, `pnpm docs:check` — passed (24 docs, links resolve).
- The bare `cargo test -p cb-core` was **not** run in this step, per the
  workflow's test policy; the gate step owns it.

## Docs

New `docs/guides/sql-mcp-server.md`, linked from `docs/README.md`.

---

# Step 3 — the stdio runner, the picker toggle, and a real bug it found

**Green:** `cargo test -p cb-core --lib` 3229 passed; `--test sql_sqlite` 25 passed;
`cargo clippy --workspace --all-targets` clean; `cargo fmt --check` clean;
`npx tsc --noEmit` clean; `pnpm test` 1861 passed (71 files).

## What landed

| File | What |
|---|---|
| `crates/core/src/mcp/serve.rs` (+ `serve_tests.rs`, 18 tests) | **new**: the decisions the runner would otherwise have made — protocol negotiation, `initialize`, routing, JSON-RPC envelopes, `list_connections_answer`, `catalog_objects`/`catalog_columns` |
| `src-tauri/src/mcp_sql.rs` | **new**: I/O only. current-thread tokio, stderr-only logging, per-stage deadlines, store reloaded per call |
| `src-tauri/src/lib.rs` | the placeholder arm replaced by `mcp_sql::run()` |
| `src-tauri/src/commands/sql.rs` | `run_discarding_rows` made `pub(crate)`; `connection_status_kind`'s `#[allow(dead_code)]` removed |
| `src-tauri/Cargo.toml`, `Cargo.toml` | `rmcp.workspace = true` — the **same** trimmed `default-features = false` line; the transport is hand-rolled, so `server`/`transport-io` were never needed. `Cargo.lock` gains exactly two packages (`rmcp`, `futures`) |
| `src/views/sqlViewLogic.ts` (+ 5 tests) | `exposureConfirm`, mirroring `writesConfirm` |
| `src/views/SqlView.tsx` | `requestExposeToAgents`/`applyExposeToAgents`; the confirm state gained a `kind` so one modal serves two consents |
| `src/components/SqlConnectionPicker.tsx`, `src/styles.css` | the `exposed to agents` badge and the Expose/Hide menu row |
| `crates/core/src/sql/catalog.rs` + `tests/sql_sqlite.rs` (2 tests) | **bug fix**, below |

## Transport: hand-rolled, and why that was the cheap answer

`rmcp`'s `server`/`transport-io` were not taken. `mcp/ndjson.rs` (framing) and
`lsp/jsonrpc.rs` (the request/notification/response split) already exist and are
tested, so the runner is a `read → classify → route → answer → encode` loop over
them. Only `rmcp::model` is used — `Tool`, `CallToolResult`, `ErrorData`,
`InitializeResult`, `ProtocolVersion` — which is exactly the scope `Cargo.toml`'s
comment argued for. **`ServerCapabilities::builder()` lives behind the `server`
feature**, so `initialize_result` fills a `Default` field by field instead; the
struct is `#[non_exhaustive]`, so neither a struct expression nor `..Default::default()`
compiles from outside the crate.

## Proof it really speaks MCP

Not just unit tests. A throwaway SQLite database plus a scratch store
(`CB_SQL_CONNECTIONS_PATH`) with one exposed and one unexposed connection, driven
over stdio by a hand-written client (scratchpad, not the repo). Verified:
`initialize` (version echoed for both `2025-06-18` and `2024-11-05`), `tools/list`
(six names), all six tools answering, `writeRefusedUnconditionally` for
`delete from orders`, `connectionNotExposed` vs `connectionNotFound` staying
distinct, `resources/list` → −32601, a garbage line → −32700 with a null id,
`tools/call` with no name → −32602, and **exit 0 on stdin EOF**.

**Revocation was tested live**: flipping `exposeToAgents` to `false` in the store
file *while the server process was running* made the very next call refuse. That
is the payoff of reloading the store per request, and it is the one property no
unit test in `cb-core` can prove about this process.

## The bug the drive-test found, which every unit test had passed

`catalog::column_catalog_query(Sqlite, ..)` emitted `CASE WHEN notnull = 0 ...`.
**`notnull` is a postfix operator in SQLite**, so that is a syntax error, not a
comparison — `near "notnull": syntax error`. Every existing test only ever read
the *string* the builder returned, and the string is perfectly well formed. So
`sql.describe_table` failed at the execute stage — **and so did the human
explorer's column listing for every SQLite connection.**

Fixed by quoting it (`"notnull"`), and pinned by two new integration tests in
`tests/sql_sqlite.rs` that run both catalog queries against a real engine. The
lesson is worth more than the fix: **a query builder is only tested by an
engine.** Any future engine arm added to `catalog.rs` needs a test like these.

## Step 4 — the installer machinery (2026-09-04)

Previewed MCP installation, all in `cb-core`. Reuses `InstallPlan` /
`PlannedWrite` / `apply_writes*`; mirrors `qgate/install.rs` name for name.

### New: `crates/core/src/mcp/install/`

- `mod.rs` (+ `mod_tests.rs`, 6 tests) — `McpConfigTarget`
  (`ClaudeProject`/`ClaudeUser`/`CodexUser`), `config_target(provider, root,
  scope, home)`, `CLAUDE_POINTER = ["mcpServers"]`, `SERVER_NAME` re-exported
  from `mcp::serve` so an install and the running server cannot disagree.
  Deliberately **not** an extension of `qgate::install::settings_path` — that
  answers *where are the hooks*, this answers *where are the MCP servers*, and
  the answers differ in every case.
  `config_target(Codex, _, Project, _)` returns `Err` naming
  `$CODEX_HOME/config.toml`; a refusal is recoverable, a silent no-op is not.
- `mcp_merge.rs` (+ 10 tests) — a NEW sibling to
  `intents/providers/settings_merge.rs`, sharing none of its code (that one is
  hard-wired to `hooks[event] = [entries deduped by marker]`; `mcpServers[name]
  = {object}` has no event layer, no array and no marker — the key IS the
  identity). Takes `pointer: &[&str]`, which is the one parameter that serves
  every JSON shape without a second copy.
- `codex_toml.rs` (+ 7 tests) — `toml_edit` over `$CODEX_HOME/config.toml`.
- `plan.rs` (+ 9 tests) — `status_for`, `install_plan_for`,
  `uninstall_plan_for`, `caveats_for`, `READ_ACCESS_NOTE`.

### Changed

- `intents/providers/mod.rs` — **new** `apply_writes_atomically` (`.bak`, temp
  sibling, rename) beside `apply_writes`, plus the private `temp_extension`.
  `apply_writes` itself is unchanged, so no existing caller moves.
- `qgate/install.rs` — `SHARED_EXE_PATH_NOTE` is now `pub` and **shared** with
  the MCP caveats rather than copied.
- `Cargo.toml` (workspace) + `crates/core/Cargo.toml` — `toml_edit = "0.25"`.
  `Cargo.lock` already carried 0.25.13 transitively: zero new packages.

### The entry that is written

`{"command": "<abs path to cb-app.exe>", "args": ["mcp-sql", "--workspace",
"<root>"]}` — command is a *field*, args a *vector*, so nothing is ever quoted
(the gate's single-string command line is the shape this deliberately avoids).
User scope carries no `--workspace`.

### Verified

`cargo test -p cb-core --lib` → **3265 passed, 0 failed** (32 of them
`mcp::install::`, 4 `intents::providers::tests::atomic_writes`).
`--test intent_attribution --test intent_retirement` → 3+9 passed.
`cargo fmt --check` clean, `cargo clippy -p cb-core --all-targets` clean,
`cargo check --workspace --all-targets` clean.

## Step 5 - commands, the panel, and the feature flag (phase 3 complete)

### Added

- `src-tauri/src/commands/mcp.rs` - five commands mirroring `commands/qgate.rs`:
  `mcp_server_status`, `mcp_server_install_plan`, `install_mcp_server`,
  `mcp_server_uninstall_plan`, `uninstall_mcp_server`. Registered in
  `lib.rs::generate_handler!` and in `mod commands`. **No new IPC type**: a
  status is exactly `Option<InstallScope>`, and the plan is the existing
  `InstallPlan`. Both apply commands use `providers::apply_writes_atomically`,
  never `apply_writes` - `~/.claude.json` is 122 KB and live.
- `src/components/mcpServerLogic.ts` + 14 vitest tests - `MCP_PROVIDERS`,
  `providerLabel`, `scopeOptions`, `defaultScope`, `scopeAvailable`,
  `statusText`, `confirmLabel`, `planOutcome`. Mutation-checked: breaking
  `planOutcome`'s zero-write branch and `defaultScope`'s narrower-grant answer
  failed three named tests between them; reverted.
- `src/components/McpServerPanel.tsx` - a `.launcher-overlay` modal (the
  `AskPanel` shape, not a floating panel: there is no live state to preserve)
  rendering the shared `PlanPreview` on both paths. A zero-write uninstall
  renders "Nothing to remove", not a disabled button.
- `src/ipc/api.ts` - the five wrappers.

### The feature flag touched NINE places, not seven

1. `crates/core/src/features/mod.rs` - `FeatureId::McpSqlServer`, `ALL: [_; 3]`,
   id/label/description, and `default_enabled` turned from a blanket `true` into
   an **exhaustive match**, so the next feature whose honest default is `false`
   inherits a decision somebody actually made.
   `every_feature_states_its_own_default` names all three answers in one list.
2. `src/components/featuresLogic.ts` - the `FeatureKey` union.
3. `src/shortcutLogic.ts` - `PLUGIN_LABELS.mcpSqlServer`.
4. `src/shortcutLogic.ts` - a `COMMANDS` entry `plugin.mcp` tagged
   `plugin: "mcpSqlServer"`. Without it `commandSections` files the row in no
   section and `pluginMenuRows` shows a raw id.
5. `src/components/pluginMenuLogic.ts` - a `PLUGINS` row and a `{kind:"mcp"}`
   `PluginAction` variant.
6. `App.tsx` dispatch + `WorkspaceTab.tsx` (the `openMcp` handle member, the
   `plugin.mcp` registration gated on the feature, the conditional mount).
7. `src-tauri/resources/linux/features.json` - the .deb seed.
8. **`src-tauri/installer/windows/installer.nsi`** - a third checkbox, its two
   `Var`s, the `PageLeaveFeatures` read-back and the `WriteFeaturesSeed` branch.
   `features::store`'s tests interpret that NSIS function directly, so they fail
   by name when `FeatureId::ALL` grows; that is how this was found.
9. `crates/core/src/features/store_tests.rs` - `every_choice()` generalised from
   a hand-written list of pairs to every bitmask over `FeatureId::ALL`, so a
   fourth feature widens the installer coverage on its own.

### Verified

`cargo test -p cb-core --lib` -> **3266 passed, 0 failed** (63 s).
`cargo fmt --all --check` clean. `cargo clippy -p cb-app --all-targets` clean.
`npx tsc --noEmit` clean. `npx vitest run` over `mcpServerLogic`,
`pluginMenuLogic`, `featuresLogic` and `shortcutLogic` -> 63 passed.
`pnpm docs:index` regenerated; `pnpm docs:check` passes (24 files).
Not run here, by this workflow's test policy (the gate step runs both in full):
the bare `cargo test -p cb-core` and the whole `pnpm test`.

### Not verified

The panel has never been rendered in a running app - no `pnpm tauri dev` this
step. Its logic and its types are proven; its markup is not.

## Step 6 — full gate + end-to-end verification (this session)

Gate, all run and seen in full (no `tail` truncation):
- `cargo test -p cb-core` — **3499 passed, 0 failed, 49 ignored** (the ignored are
  the Postgres/SQL Server suites needing `CB_SQL_PG_URL`/`CB_SQL_MSSQL_URL`).
  `lsp_transport` alone took 302s.
- `cargo fmt --all --check` — clean. `cargo clippy --workspace --all-targets` — clean, no warnings.
- `pnpm typecheck` — clean. `pnpm test` — 72 files, **1879 tests**, all pass.
- `pnpm docs:index && pnpm docs:check` — 24 files, all links resolve.

End-to-end over real stdio (`target/release/cb-app.exe mcp-sql`, scratch SQLite +
`CB_SQL_CONNECTIONS_PATH` fixture): initialize / tools/list / all six tools
answered correctly. Refusals confirmed live: a write is
`writeRefusedUnconditionally` **even with `allowWrites: true` on the connection**;
an unexposed connection is `connectionNotExposed`, never `connectionNotFound`; a
quote in an identifier is `identifierRefused`; an unknown tool is a **protocol**
error (-32601) while every tool refusal is `isError: true`. Revoking
`exposeToAgents` mid-session bit on the **next** call — the per-call store reload
is real, not just unit-tested.

Injection attempt `table = "widgets; drop table orders --"` returned "no columns"
and `orders` survived: `column_catalog_query` puts the name in a *string literal*
with quote doubling, so it is data, not SQL.

Secret confinement: a connection whose literal DSN carried the sentinel
`SENT1NELPW` (and a SQLite path containing it) was exercised across success and
every failure path (wrong password, missing file, bad SQL). The sentinel appears
**nowhere** in stdout or stderr. stderr was empty for the whole run and all 16
stdout lines parsed as JSON-RPC — no stray bytes on the transport.

One doc fix: `docs/guides/sql-mcp-server.md`'s "Where the code is" table omitted
`mcp/serve.rs` and `mcp/install/`; both rows added.

`cargo build --release` succeeded (7m40s) with the app running — no
"Access is denied". No stray `CARGO_TARGET_DIR`s exist.

## Verified against a real MCP client over stdio (2026-09-08)

Driven with `<scratchpad>/drive-mcp.js` against `target/release/cb-app.exe mcp-sql`,
a throwaway SQLite database, and a store with two connections — one exposed
**with `allowWrites: true`**, one deliberately not exposed — plus a sentinel
password in the exposed connection's literal DSN.

Handshake: `protocolVersion 2025-11-25`, `serverInfo code-basics-sql 1.3.0`,
`capabilities.tools`, and an `instructions` string that tells the model exposure
is per-connection and off by default, that no argument lifts read-only, and to
call `sql.read_only_enforcement` before trusting the words "read-only". All six
tools listed and all six answered.

**The three things unit tests could not establish, all confirmed:**

1. **stdout carries MCP frames only.** Every line parsed as JSON. This was the
   most likely way to ship a server that looks hung, because `tracing`'s default
   subscriber writes to stdout and the spec forbids anything else there.
2. **The sentinel password never appeared** — not on stdout, and not on stderr
   either.
3. **Read-only is forced even with `allowWrites: true` on the connection.**
   `DELETE`, `UPDATE` and `VACUUM` were all refused as
   `writeRefusedUnconditionally`, and the message says so explicitly: *"Turning
   writes on for this connection would not change this answer… there is no
   argument that lifts it."* That is the rule the design required, observed
   rather than argued.

**The opaque handle works end to end.** `sql.list_connections` returned
`conn-ce76fa5e4b4ad817  Widgets  (SQLite)`. The stored id is
`appsettings:src/Api/appsettings.json:ConnectionStrings:Db`, and none of that
path reached the client — the round 1 finding is fixed in the real server, not
just in a test.

Refusals stayed distinct answers rather than collapsing: `connectionNotExposed`
("a saved connection matches this, and it is not exposed") against
`connectionNotFound`, and `identifierRefused` naming which rule the name broke.
Each carries `isError: true` with a `structuredContent.code`, so the model gets
a tool error it can self-correct from rather than a protocol error.

Not covered by this run: Postgres and SQL Server. Both integration suites are
still `#[ignore]`d without a live server, so the omitted-schema fix on those two
engines remains verified as a *string* only.

(Note for whoever reuses the driver script: it prints a spurious "TIMEOUT" at
the end. That is the script's own uncleared watchdog keeping node's event loop
alive after `main()` returns — the server had already exited cleanly.)

## Two UI bugs found by the user in dev, both fixed (2026-09-08)

**No confirmation on install.** `confirm()` cleared the preview and refreshed
the status line, and that was the entire feedback. A preview disappearing is
indistinguishable from a cancel, so a successful install read as "nothing
happened" — and the natural response to that is to install a second time. Added
`mcpServerLogic::writtenNote` (7 tests), which names the files actually written
— taken from the plan the user approved, not from a second read — and states the
two silent-no-op conditions: a project `.mcp.json` needs the agent's approval
before it loads, and an already-running agent will not see the change until it
restarts. A `.success` class was added beside the existing `.warning`/`.error`,
because none existed.

**The preview overflowed its window.** `PlanPreview` renders
`<pre>{write.content}</pre>` — the *entire* final contents of each file — and
had **no CSS at all**. A bare `<pre>` is `white-space: pre` with no height
bound, so 122 KB of single-line `~/.claude.json` pushed out of its
`max-height: 70vh` container in both axes and took the confirm buttons with it.
Added `.plan-preview` (shrinkable, scrollable) and `.plan-preview pre` (capped
at 30vh, own scroll container) per CLAUDE.md's rule that wide content scrolls
inside its own container and the surface around it never does. Content is
deliberately **not** wrapped: this is JSON and TOML a person is checking before
it is written to their machine, and rewrapped braces are harder to verify.

Both fixes benefit every `PlanPreview` consumer — the intent hooks, the quality
gate and the first-open setup prompt all show the same unstyled `<pre>` today.
