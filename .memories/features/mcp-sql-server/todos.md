# Todos

## Durable fix, recorded rather than half-built

- [ ] **`run_statement_with_params` on `sql::driver::SqlConnection`.** The trait
      takes no parameters at all today, so parameter binding is not reachable
      and `catalog::identifier` is a refusal layer, not a substitute. Once it
      exists, the catalog queries bind `schema`/`table` and `sql_literal` stops
      being load-bearing. Do not let anything in the meantime imply that
      validation equals binding — `catalog.rs`'s module docs say so.

## Next steps (Phase 2/3, in order)

- [x] Gate the dependency: `cargo add rmcp && cargo check -p cb-core`, read the
      *first* error. Fall back to hand-rolled legacy-only over `lsp/jsonrpc.rs`
      if it will not build on 1.88, and say so loudly.
- [x] `crates/core/src/mcp/ndjson.rs` — newline-delimited framing. A malformed
      line does **not** poison (a newline is a resync point); `LineTooLarge`
      does. `encoded_output_contains_exactly_one_newline_and_it_is_last`.
- [x] `crates/core/src/mcp/argv.rs` — `is_mcp_sql_invocation`,
      `parse_mcp_sql_args`, `the_three_self_dispatch_markers_are_disjoint`,
      `a_directory_argument_is_not_an_invocation`.
- [x] `mcp/expose.rs::AgentConnection` + the sentinel test
      `an_agent_connection_has_no_field_that_could_carry_a_secret_or_a_path`.
- [x] `mcp/execute.rs::agent_plan` with **no** `writes_allowed` parameter, and
      `a_write_refusal_says_that_enabling_writes_would_not_help_here`
      (`guard::HEURISTIC_NOTE` advises enabling writes, which is false on the
      agent path — reusing it would be the bug).
- [x] `mcp/answer.rs::McpRefusal` — every abstention distinct.
- [x] `mcp/render.rs`/`mcp/tools.rs` **done**, and `src-tauri/src/mcp_sql.rs` with
      them (step 3): every log line to stderr, stdout carries only MCP bytes.
- [x] Phase 3: `mcp/install/`, `toml_edit` for Codex,
      `providers::apply_writes_atomically`, `commands/mcp.rs`,
      `McpServerPanel.tsx` + `mcpServerLogic.ts`, feature flag (step 5).

## UI still owed for step 1

- [x] `SqlConnectionPicker.tsx`: the sibling context-menu row and the
      `exposed to agents` badge, wired through `SqlView.requestExposeToAgents`
      → `sqlSetExposeToAgents`, confirmed by the tested
      `sqlViewLogic.exposureConfirm` (granting direction only).

## Docs

- [x] `pnpm docs:index` run after `mcp/` + `mcp_sql.rs` landed (543 files).
- [ ] CLAUDE.md's `crates/core` list gains `sql/` + `mcp/` — **blocked, and the
      block is the point**: `pnpm docs:check` counts CLAUDE.md at exactly 500
      lines today, its hard limit, so *no* line can be added to it. The entry
      was written and reverted. It needs a prune first (the `dap/` and
      `architecture/signals/` bullets are the longest candidates), and that is
      an editorial task of its own, not a line to smuggle into a feature step.
      Draft text is in this work item's `completed.md` step-3 section.
- [x] `docs/reference/commands.md` now carries the whole `commands/mcp.rs`
      surface (a "SQL MCP server" section) and `pnpm docs:index` was
      regenerated; `pnpm docs:check` passes (24 files).

## Step 3 is done — what remains

- [x] `src-tauri/src/mcp_sql.rs` (≈330 lines with docs, deciding nothing).
- [x] `src-tauri/src/lib.rs::run()` placeholder replaced by the real runner.
- [x] `connection_status_kind`'s `#[allow(dead_code)]` removed.
- [ ] Tool names use dots (`sql.query`). Claude Code and Codex have **not** been
      driven against this yet — only a hand-written stdio client. If a client
      rejects the dots, the rename is one `const` each in `mcp/tools.rs`.
- [x] Phase 3 (step 5): `commands/mcp.rs`, `McpServerPanel.tsx` +
      `mcpServerLogic.ts`, the feature flag.
- [ ] `docs/` still has no **guide** page for the MCP server (the command
      reference now covers the surface). Write one, and state the `.mcp.json`
      approval caveat: Claude Code records it per project in `~/.claude.json`
      as `enabledMcpjsonServers`/`disabledMcpjsonServers`, so a project install
      is correct-but-inert until a prompt is accepted.

## After step 4 (installer machinery) — what is left for phase 3

- [x] `mcp/install/{mod,mcp_merge,codex_toml,plan}.rs` — the whole previewed
      installer, 32 tests, all mutation-checked.
- [x] `providers::apply_writes_atomically` beside `apply_writes`.
- [x] `qgate::install::SHARED_EXE_PATH_NOTE` made `pub` and shared, not copied.
- [x] **Tauri commands** — `src-tauri/src/commands/mcp.rs`: `mcp_server_status`,
      `mcp_server_install_plan`, `install_mcp_server`,
      `mcp_server_uninstall_plan`, `uninstall_mcp_server`, registered in
      `lib.rs`. Both apply commands call `apply_writes_atomically`, not
      `apply_writes` — the whole reason the atomic variant exists is
      `~/.claude.json`. `types.ts` needed nothing: a status is
      `InstallScope | null`, so only `ipc/api.ts` gained wrappers.
- [x] **The install panel** — `McpServerPanel.tsx` over the shared
      `PlanPreview`, with every decision in the tested `mcpServerLogic.ts`.
- [x] **Surface the Codex project-scope refusal as a disabled row with a
      reason**, not a hidden one — `config_target` already returns the sentence.
- [x] `pnpm docs:index` regenerated and `docs/reference/commands.md` gained a
      "SQL MCP server" section.
- [ ] The panel is opened from the **Plugins** menu / `plugin.mcp`; it has not
      been driven in the real app yet (no `pnpm tauri dev` this step). The
      logic and the types are verified; the rendering is not.

## After step 5 (commands, panel, feature flag)

- [ ] **Per-scope status.** `mcp_server_status` answers `Option<InstallScope>`
      with project precedence, which is `plan::status_for`'s API. So the panel
      cannot say "installed for this repository AND for every repository" - a
      user-scope entry is invisible while a project one exists. Nobody has
      needed that yet; if it is wanted it is a new `cb-core` function
      (`installed_at(provider, root, scope, home) -> bool`), not a UI change.
- [ ] The feature flag turned out to touch **nine** places, not seven: the NSIS
      installer page (`src-tauri/installer/windows/installer.nsi`) and the
      `every_choice()` combination generator in `features/store_tests.rs` are
      both enforced by tests that fail by name the moment `FeatureId::ALL`
      grows. Both were updated here; see `notes.md`.
- [ ] Drive a real agent against an installed entry. Neither Claude Code nor
      Codex has been pointed at a written `.mcp.json`/`config.toml` yet, so the
      dotted tool names (`sql.query`) remain unproven end to end.

## From review round 1 (2026-09-08)

- [ ] `plan::status_for` folds "unreadable" into "not installed" (it calls
      `is_installed`, not the new `installed_state`). The destructive write is
      already refused, so this is now a reporting gap rather than a hazard —
      but a user with a locked `~/.claude.json` is told the server is not
      installed when the truth is unknown. Fixing it properly changes what
      `status_for` returns and so the IPC surface; do it when that type is next
      touched.
- [ ] The Postgres and SQL Server `column_catalog_query` strings are **not
      executed by any test** — both integration suites are `#[ignore]`d without
      a live server. The omitted-schema fix is unit-tested as a string only.
      `tests/sql_sqlite.rs` is what caught the `"notnull"` quoting hazard on the
      SQLite arm; the other two engines have no equivalent. Run them against a
      real Postgres and SQL Server before trusting `describe_table` there.
- [ ] `run_statement_with_params` on the driver trait — the durable fix for
      identifier interpolation. `catalog::identifier` refuses hostile input, and
      the module docs say plainly that refusing is not the same as binding.
- [x] The `SqlConnectionPicker.tsx` "Expose to agents" toggle and badge — it
      DOES exist (a later step added it; the note claiming otherwise was stale). The
      command, store field and wire type all exist; nothing in the UI moves the
      flag yet, so consent can currently only be granted by editing
      `sql-connections.json` by hand.
- [x] Drive the built server over stdio end to end — DONE 2026-09-08, see
      `completed.md`. stdout hygiene, forced read-only and the opaque handle all
      confirmed against the release binary. The gate
      agent that was going to do this died on the spend limit, so **no MCP
      client has ever spoken to this server**. Everything is unit and
      integration level. In particular: confirm nothing but MCP frames reaches
      stdout (`tracing`'s default subscriber writes there, which is the most
      likely way to ship a server that appears to hang).
- [x] Secret-sentinel sweep against the running server — DONE, clean on both
      stdout and stderr. Original item: not just the
      unit tests: a DSN with a unique sentinel, every tool, every failure path,
      then grep everything the process wrote.
