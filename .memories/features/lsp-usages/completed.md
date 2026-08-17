# Completed — LSP usages

## The follow-up round (2026-08-17) — `plan.md`'s eleven items

Worked in the plan's order. Five real defects, three decisions now recorded in
code or `CLAUDE.md`, three deliberately left with an explicit trigger. Every
finding came from executing something; the captures are at the end of `notes.md`.

### Real defects fixed

| # | Defect | Where | How it was caught |
|---|---|---|---|
| 1 | Two document-version counters; the mirror's was never sent, so `Documents::version()` was an authority nobody consulted | `lsp/client.rs`, `lsp/session.rs` | New `notificationJournal` on the fake made the wire visible: `[1, 2, 1, 2]` across a close and re-open |
| 2 | `.tsx` opened as `typescript`, so `documentSymbol` reported JSX innards (`<unknown>`, `props`) as declarations — and that is what the inline rows are built from | `lsp/registry.rs`, `lsp/client.rs` | Ran typescript-language-server against the same project under both ids |
| 3 | `pid_alive` answered `false` for **every** pid on Unix (`kill` is a shell builtin; `/bin/kill` absent), so the tree-kill test could not fail | `tests/lsp_transport.rs` | First-ever Linux run |
| 4 | `kill_tree` signalled `-pid` only, killing **nothing** for a non-group-leader pid | `process/kill.rs` | `a_write_failure_kills_the_process_it_can_no_longer_reach` on Linux |
| 5 | `a_project_install_shows_up_…` asserted the installed branch on a machine that happens to have Codex | `intents/providers/providers_tests.rs` | First-ever Linux run |

### Wire-type change

`SymbolKind::Property` added. LSP 7 → `Property`; 8 (`Field`) and 13
(`Variable`) stay `Variable` deliberately — every language here that has
properties also has fields and calls them different things. Mirrored in
`src/ipc/types.ts`; `results::admits` treats `Property` under the same container
rule as other named storage. The key-pinning test became an **exhaustive
`match`**, so the next variant fails to compile rather than reaching the wire
unmirrored — the rule is now in `docs/architecture/ipc-contract.md`.

### Evidence captured rather than reasoned

`the_location_link_rust_analyzer_really_sends_is_aimed_at_its_selection_range` —
the verbatim rust-analyzer 1.97.1 payload. `targetRange` starts on the doc
comment, `targetSelectionRange` on the identifier, so `LocationLink::aim`'s
preference is load-bearing. Roslyn never sends this shape.

### Decisions recorded

- **Command modules are untestable by construction**, verified rather than
  assumed: `State<'_, AppState>` needs a `tauri::App` and the `test` feature is
  not enabled, so no command in `cb-app` is called by any test. `CLAUDE.md` now
  carries the rule — a command body may not decide anything, and a decision moves
  to a free function beside it. Applied to `lsp_status` → `status_for`.
- **`codeLens`**, **`Transport::shutdown`'s double `EXIT_GRACE`**, and
  **`kill_tree` blocking the runtime worker** stay open, each with a trigger
  rather than a date. The `kill_tree` fix is pre-diagnosed as `spawn_blocking`.
- The **dirty-dot** item was moved out: it belongs to `FileEditor`, predates this
  feature, and is not this work item's.

### Files touched

`crates/core/src/lsp/{client,session,registry,protocol,documents,results}.rs`
and their `*_tests.rs`; `crates/core/src/symbols/declarations.rs`;
`crates/core/src/process/kill.rs`;
`crates/core/src/intents/providers/providers_tests.rs`;
`crates/core/src/bin/fake_lsp.rs`;
`crates/core/tests/{lsp_client,lsp_session,lsp_transport}.rs`;
`src-tauri/src/commands/lsp.rs`; `src/ipc/types.ts`;
`CLAUDE.md`, `docs/architecture/{ipc-contract,core-crate}.md`, `docs/INDEX.md`.

### Gate at the end of the round

- `cargo test -p cb-core` — **1930** lib, integration 49 / 2 (1 ignored) / 33 /
  1 (5 ignored) / 17 / 26 / 11. `cargo test -p cb-app` — **41**.
- **The same suite on Linux** (Docker `rust:slim` + `git` + `procps`, `src-tauri`
  dropped from the workspace members): all green, **1925** lib — the five-test
  gap is Windows-only tests.
- `cargo clippy --workspace --all-targets` — exactly the two documented baseline
  warnings (`importers/rider.rs:65`, `workspace.rs:1729`). `cargo fmt --check`
  clean.
- `pnpm typecheck` clean, `pnpm test` **631** in 23 files, `pnpm docs:check`
  passed (21 files), `pnpm docs:index` regenerated.
- **All five real-server oracles pass**, 103 s, against Roslyn 2.140.9,
  rust-analyzer 1.97.1, typescript-language-server, basedpyright and pyright.

### Earlier rounds

Phases 0–5 and the oracle round are recorded in `todos.md` and `notes.md`; the
oracle round was committed separately as `75a8d25` and opened as PR #1 so a
bisect can tell it apart from these follow-ups.
