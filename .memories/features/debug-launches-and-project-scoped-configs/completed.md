# Completed

## 2026-09-02 — Codex (ran out of usage mid-verification)

**Configuration identity** (`crates/core/src/config.rs`, `importers/rider.rs`)
- `CURRENT_VERSION = 2`, `rider_config_id`, `migrate` (rewrites ids plus
  `favorites`/`order`/`compound`).
- `rider::convert` stamps the project-scoped id; `id_for` deleted.
- `resolve_compounds` drops ambiguous same-named members.
- `import` dedupes `.run/` against legacy `.idea/` and warns on disagreement.

**Debug runtime** (`src-tauri/src/commands/debug.rs`, new, ~860 lines)
- stdio (.NET/netcoredbg) and loopback-TCP (Node/js-debug) adapters, DAP
  handshake, output forwarding, reverse-request replies, compound preflight.
- `state::DebugSessions`; `running::store::remove_if_pid`.
- `dap::model::DebugEvent`; `dap::registry` no longer discovers vsdbg.

**UI + icon** (`src/views/RunView.tsx`, `src/ipc/*`, `src-tauri/src/lib.rs`)
- Debug button, `startDebug`/`stopDebug`/`debugIds`, `set_icon` in `setup`.

## 2026-09-02 — Claude (finished the handoff)

- Ran Codex's last, never-executed test
  (`an_ambiguous_same_named_compound_member_is_dropped_instead_of_guessed`) —
  passes, along with all 37 Rider importer tests.
- **Extracted the two frontend decisions out of `RunView.tsx`** into
  `src/views/debugLogic.ts` + `debugLogic.test.ts` (17 tests), per the
  "decisions live in a `*Logic.ts`" rule:
  - `debugAvailability` — enablement, and *why not* when disabled. It now
    validates a compound's **members**, which the inline version did not: the
    old rule enabled Debug for any `ecosystem === "compound"`, so a compound
    holding a cargo member built its .NET members and only then refused.
  - `debugEffects` — `DebugEvent` → console `ProcessEvent` + tab status.
- Removed the `run_one` wrapper in `debug.rs` (8 arguments, the one new clippy
  warning) by inlining its two calls at the single call site.
- Wrote `docs/reference/commands.md`'s **Debug launches** section (the file
  must track `generate_handler!`) and regenerated `docs/INDEX.md`.
- Verified the two remaining `cb-core` failures are pre-existing on `main`.

## Gate as of this entry

| Check | Result |
|---|---|
| `cargo test --workspace --no-fail-fast` | 3216 pass, **2 fail** — both pre-existing on `main` |
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets` | no warnings |
| `tsc --noEmit` | clean |
| `vitest run` | 57 files, **1441** pass |
| `node scripts/check-docs.mjs` | pass |

Not verified: a real debug session, and the packaged installer's icon — see
`todos.md`.

## 2026-09-02 — Claude: adapters bundled into the installer

Reverses the plan's "adapters are user-installed dependencies". Both now ship.

- **`scripts/fetch-debuggers.mjs`** (`pnpm debuggers:fetch`) — vendors
  NetCoreDbg 3.2.0-1092 (MIT) and vscode-js-debug v1.117.0 (MIT) into
  `src-tauri/resources/debuggers/`. Pinned versions, **SHA-256 verified**
  (a mismatch is fatal; a network failure is not), idempotent via a
  FORMAT+version+hash stamp, and it fetches netcoredbg's LICENSE separately
  because its release zip ships none.
- **Wiring** — `tauri.conf.json` gains `"resources/debuggers/"` and chains the
  fetch into `beforeBuildCommand`; `.gitignore` excludes the vendored tree.
- **`dap::registry::resolve`** takes `bundled_dir: Option<&Path>`; order is
  pin → bundle → PATH. New `needs_node` refuses a `.js` adapter when Node is
  absent, naming Node rather than letting the spawn fail opaquely.
  8 new tests (mutation-checked against the path constants).
- **`commands/debug.rs`** — `bundled_debuggers(&AppHandle)` mirrors
  `inspect.rs`'s `bundled_dir`; threaded through `prepare_one`.
- Removed the `format!`-with-no-args clippy warning introduced en route.
- Docs: `core-crate.md` (`dap`), `commands.md`, `using-the-app.md`,
  `development.md`, `CLAUDE.md` (both the command list and the stale
  "beforeBuildCommand is just pnpm build" gotcha), `INDEX.md` regenerated.

**Verified empirically, not just by unit test:** resources land in
`target/debug/debuggers/` so `BaseDirectory::Resource` resolves in dev;
`netcoredbg.exe --version` runs from the extracted copy (3.2.0-1); and the
bundled js-debug prints its port from inside the repo — which only works
because of the `{"type":"commonjs"}` fix (see `notes.md`).

### Gate

| Check | Result |
|---|---|
| `cargo test --workspace --no-fail-fast` | 3224 pass, 2 fail — both pre-existing on `main` |
| `cargo fmt --check` / `clippy --all-targets` | clean / 0 warnings |
| `tsc --noEmit` / `vitest run` | clean / 1441 pass |
| `check-docs.mjs` | pass |
