# Erosion detector — completed

Feature #5 from the `feature-ideas.html` report. A rules-based, no-model scan
over the diff for changes that quietly weaken the codebase.

## What shipped
- **`crates/core/src/erosion/`** — new top-level module (`mod.rs`, `rules.rs`,
  `scan.rs` + sibling `*_tests.rs`; 19 tests). `pub mod erosion;` in `lib.rs`.
  - `ErosionRule { id, category, side, pattern, message, extensions[], prodOnly }`;
    `ErosionCategory` (8 variants); `RuleSide { Added, Removed }`.
  - `parse` / `load_dir` / `rules_dir(root)` = `.code-basics/erosion/` /
    `all_rules(root)` = `builtin_rules() ++ user rules` (extend, never shadow) —
    mirrors `adapters/manifest.rs`. `compile` returns warnings for bad regex.
  - `scan_diffs(files, rules) -> ErosionReport { flags, warnings }`. **Side
    discipline**: only matches lines whose origin matches the rule side, never
    context. Cites `new_lineno` (added) / `old_lineno` (removed) as `line`, and
    the `DiffLine.index` as `index` for the click highlight. `prodOnly` +
    `is_test_path` skips test files. At most one flag per category per line.
  - `builtin_rules()` covers .cs / ts-js / .rs.
- **`src-tauri/src/commands/erosion.rs`**: `erosion_scan(mode) -> ErosionReport`
  (opens repo, `diff_all`, `all_rules`, `scan_diffs`; concatenates loader +
  compile warnings). Registered in `lib.rs`.
- **IPC**: `ErosionCategory`/`ErosionFlag`/`ErosionReport` in `types.ts`;
  `erosionScan` in `api.ts`.
- **Frontend**: `erosionLogic.ts` (+ `.test.ts`) — `groupByCategory`,
  `badgeCount`, `categoryLabel`, `CATEGORY_ORDER`. `ErosionPanel.tsx` renders
  grouped flags + warnings banner; click → `openErosionFlag` sets highlight to
  `[flag.index]`. `ChangesView.tsx` gained a fourth grouping mode `"erosion"`
  with a flag-count badge and `refreshErosion` (gated to when shown, like intent).
- **Docs**: `erosion_scan` + TOML schema in `docs/reference/commands.md`;
  `examples/erosion/custom.toml`; `pnpm docs:index` regenerated.

## Design note (deviation from plan)
The plan proposed a `flagToLineIndex(flag, diff)` frontend helper to map a
flag's source line back to a `DiffLine.index`. Instead the **backend emits the
`index` directly** on each flag (it already walks the DiffLines) — consistent
with how intent groups carry `lineIndices`, and it avoids an async
click-before-load mapping. The flag carries both `line` (source no., display)
and `index` (diff index, click).

## Verified
`cargo test -p cb-core --lib erosion` (19) green; `cargo check -p cb-app` clean;
`pnpm typecheck` clean; `pnpm test` 705 pass; `docs:index` + `docs:check` clean.

## Composition
Detector is a pure producer of located facts — `ErosionFlag` carries no score.
A future risk-ranking layer can consume `ErosionReport` + attribution + tests.
