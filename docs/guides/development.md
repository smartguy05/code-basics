# Development guide

## Commands

| Command | What it does |
|---------|--------------|
| `pnpm install` | Install frontend dependencies |
| `pnpm tauri dev` | Run the full app with hot reload (Vite on port 1420 + Rust shell) |
| `pnpm dev` | Frontend only, in a browser — `invoke` calls fail; layout work only |
| `pnpm typecheck` | `tsc --noEmit` |
| `pnpm build` | Typecheck + Vite production build into `dist/` |
| `pnpm tauri build` | Full release build: executable + installers |
| `cargo test -p cb-core` | All core tests (the entire logic layer) |
| `cargo test -p cb-core <name>` | Tests whose names contain `<name>` |
| `cargo clippy` | Lint the Rust workspace |
| `pnpm docs:index` | Regenerate [docs/INDEX.md](../INDEX.md) from the source tree |
| `pnpm docs:check` | Verify doc line limits and relative links |

There is no JavaScript test suite. Logic that needs tests belongs in `cb-core` — that is the point of the [architecture](../architecture/overview.md).

## Where code goes

The dependency rule, restated as a checklist:

- **Decides something** (what to run, how to parse, what a diff means)? → `crates/core`, with tests.
- **Wires a decision to the UI**? → a thin command in `src-tauri/src/commands/`.
- **Draws something**? → `src/`.

## Adding a Tauri command end-to-end

1. Implement the logic as a plain function in `cb-core`, unit-tested there.
2. Add a `#[tauri::command]` wrapper in the right file under `src-tauri/src/commands/` returning `Result<T, String>` (format errors with `{e:#}`).
3. Register it in the `generate_handler!` block in `src-tauri/src/lib.rs` — forgetting this compiles fine and fails at runtime.
4. If it returns a new type, follow the [IPC contract rules](../architecture/ipc-contract.md#rules-when-changing-a-crossing-type): derive the serde attributes, pin the JSON keys in `model.rs` tests, mirror in `src/ipc/types.ts`.
5. Add a typed wrapper in `src/ipc/api.ts` — views never call `invoke` directly.
6. Update the [command reference](../reference/commands.md) and run `pnpm docs:index`.

## Rust conventions

- Sibling test files for big suites: `dotnet_tests.rs` next to `dotnet.rs`, included with `#[cfg(test)] #[path = "..."]`.
- Errors use `anyhow` with `.context(...)` so messages explain what was being attempted; `thiserror` for typed errors where callers branch.
- Module-level `//!` docs explain *why* a module works the way it does (the VSTest/MTP split, the patch direction table, the two-git-implementations rationale). Keep that habit — the docs in this folder link to them rather than duplicating them.
- Report fixtures live in `crates/core/fixtures/reports/`.

## Frontend conventions

- All IPC goes through `src/ipc/api.ts`; components take data as props.
- Commands that return an updated `Workspace` flow it back up via `onWorkspaceChange`.
- Match the existing plain-React style: local `useState`, no state libraries, no router.

## Documentation conventions

- Docs live in `docs/`, grouped by directory (`getting-started/`, `architecture/`, `guides/`, `reference/`).
- Keep every file under 500 lines — split logically when approaching the limit.
- Cross-link related docs with relative links; `pnpm docs:check` verifies they resolve.
- `docs/INDEX.md` is generated — never edit it by hand; run `pnpm docs:index`.
