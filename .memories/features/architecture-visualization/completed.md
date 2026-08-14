# Completed

## Phase 1 — The symbol index in `cb-core` (2026-08-10)

Built by four agents in parallel plus an integration gate. The full gate is green
on the joined tree: `cargo fmt --check`, `cargo clippy -p cb-core --all-targets`,
`cargo test -p cb-core` (1121 + 44 + 2 + 11 passing, 82 of them `symbols::`),
`cargo check --workspace --all-targets`, `pnpm typecheck`, `pnpm test`
(11 files / 229 tests), `pnpm docs:index`, `pnpm docs:check`. No test was
weakened or deleted; the integration pass had nothing to fix.

### Files added — `crates/core/src/symbols/` (3648 lines incl. tests)

| File | What it does |
|---|---|
| `mod.rs` | The layering doc: `declarations` and `fuzzy` are pure, `index` walks, `cache` persists, `search` combines. Nothing here depends on `git`. |
| `declarations.rs` / `_tests.rs` | `declaration(line) -> Option<Declared>` + `SymbolKind`; `declaration_name` kept as a thin `Option<String>` wrapper so `grouping_tests.rs` remains a free regression net. |
| `fuzzy.rs` / `_tests.rs` | `score(query, candidate) -> Option<Match>` with char-indexed positions; smart case, subsequence pre-pass, bounded DP, deterministic ranking. |
| `index.rs` / `_tests.rs` | `Symbol` / `SymbolIndex`; extension allowlist for parsing while every walked file still appears in `files`; size, count and symbol caps that set `truncated`; `project_id` by longest-prefix match. |
| `cache.rs` / `_tests.rs` | `.code-basics/symbols.json`, entries keyed on `(mtime, len)`, wholesale discard on a `heuristic_version` bump, corrupt file ignored. Exports `CACHE_FILE`. |
| `search.rs` / `_tests.rs` | `search(index, configs, query) -> Vec<SearchHit>`, `SearchScope { All \| Files \| Symbols \| Actions }`, trailing `:123` parsed in Rust, `search_hit_serialises_with_the_keys_the_ui_reads`. |

### Files modified

- `crates/core/src/lib.rs` — `pub mod symbols;`.
- `crates/core/src/workspace.rs` — the `WalkDir` + `filter_entry` block extracted
  from `scan_with` into `pub(crate) fn source_walker(root) -> SourceWalker`, with
  `the_source_walker_skips_build_output_and_nested_checkouts`. **Signature
  deviates from the plan**: it cannot return `walkdir::IntoIter`, because
  `filter_entry` returns `walkdir::FilterEntry<IntoIter, P>`; a named
  `SourceWalker` alias stands in. `index.rs` and `cache.rs` both walk through it,
  so the index and the project list can never disagree about what exists.
- `crates/core/src/git/grouping.rs` — 102 lines removed. `DECLARING`,
  `declaration_name`, `is_identifier` and `NOT_A_SYMBOL` are gone; it now imports
  `declaration_name` and `NOT_A_SYMBOL` from `symbols::declarations`.
  `symbol_from_header` / `header_can_name_a_symbol` stayed, being about hunk
  headers, which are a git concept. The arrow is `git → symbols`, permanently.
- `crates/core/src/config.rs` — `symbols::cache::CACHE_FILE` added to `IGNORED`
  (the index is fingerprinted against one machine's file timestamps and is
  misleading in anyone else's checkout), and the pinning test updated.
- `package.json` / `pnpm-lock.yaml` — `mermaid` pinned exact at `11.16.1`,
  installed but **imported nowhere yet**; it is there for the Phase 0 spike.
- `docs/INDEX.md` — regenerated: 151 files, 64 commands, 65 IPC wrappers,
  48 core modules.

### The bug fixed while lifting

`declaration_name` named the generic return type of
`public async Task<int> DoWork(int a)` as `Task`. Fixed via
`name_before_parameter_list`, guarded so it only applies when the `(` precedes
any `=`. Details and the two further traps are in `notes.md`.

### No new Rust dependency was added

`crates/core/Cargo.toml` is unchanged.

## The fix round (2026-08-10, same day)

Five agents re-read their own `symbols/` modules adversarially; four real
defects came out, each one previously masked by a test that shared the bug's
assumption. Full gate re-run green on the joined tree — see the counts below.
Nothing was weakened or deleted to get there, and the integration pass again
found **nothing to fix**: the only two clippy warnings are the known
pre-existing pair (`importers/rider.rs:65` while-let, `workspace.rs:1028`
cmp_owned), neither of which is in this branch's diff.

### Gate, before → after

| Gate | Phase 1 | After fix round |
|---|---|---|
| `cargo test -p cb-core` (lib) | 1121 passed | **1133 passed / 0 failed** |
| — of which `symbols::` | 83 | **94** |

(The Phase 1 section above says 82 `symbols::` tests; the handoff baseline for
this round said 83. I could not re-measure the pre-round tree, so treat the
`symbols::` sub-count as ±1. The 1121 → 1133 total is the reliable figure.)

| `tests/git_operations.rs` | 44 | 44 passed |
| `tests/intent_attribution.rs` | 2 (1 ignored) | 2 passed, 1 ignored |
| `tests/reject_markers.rs` | 11 | 11 passed |
| `pnpm test` | 229 | 229 passed (11 files) |
| `cargo fmt --check` | clean | clean |
| `cargo check --workspace --all-targets` | clean | clean |
| `pnpm typecheck` | clean | clean |
| `pnpm docs:index` / `docs:check` | clean | 151 files / 20 docs, all links resolve |

Test count went **up by 12**, fully accounted for: +11 in `symbols::`, +1 the
`workspace::source_walker` test. No test disappeared.

### Files touched in the fix round

- `symbols/declarations.rs` + `_tests.rs` — balanced `<…>` skipping (the C#
  generic property/field bug); `is_comment` widened to `//`, `#`, `/*`, `*`,
  `--`, `'` so comment prose stops being indexed as declarations.
- `symbols/cache.rs` + `_tests.rs` — `HEURISTIC_VERSION 1 → 2` (discards every
  index built with the old namer); `project_id` re-derived from the current
  project list instead of being trusted from the cache entry; the invariant test
  `a_cold_cached_build_agrees_with_an_uncached_one` given a real two-project
  fixture, because with an empty list it compared `None` to `None`.
- `symbols/index.rs` + `_tests.rs` — the byte cap now actually sets `truncated`.
- `symbols/search.rs` + `_tests.rs` — the `:123` overflow case degrades to
  `(head, None)`, matching the documented behaviour.
- `symbols/{mod.rs, fuzzy_tests.rs}` — test-module headers calibrated against
  the 19 existing ones; no assertion changed.
- `docs/INDEX.md` — regenerated.

Root causes and the reasoning behind each fix are in `notes.md` under
"The fix round".

## Phase 2 — The Search Everywhere palette (2026-08-10)

Four agents in parallel — backend commands + `AppState`, the IPC mirror, the
pure frontend logic, the palette component — plus this integration gate. The
palette is reachable from every tab, ranks files, symbols and run configurations
in one list, and hands the choice to the Run tab.

### Gate, before → after

| Gate | Phase 1 fix round | Phase 2 |
|---|---|---|
| `cargo test -p cb-core` (lib) | 1133 passed | **1151 passed / 0 failed** |
| `tests/git_operations.rs` | 44 | 44 passed |
| `tests/intent_attribution.rs` | 2 (1 ignored) | 2 passed, 1 ignored |
| `tests/reject_markers.rs` | 11 | 11 passed |
| `cargo test -p cb-app` | 2 (`state.rs`) | **8 passed** |
| `pnpm test` | 229 (11 files) | **268 passed (12 files)** |
| `pnpm coverage` | ≥70% gate | 99.62% lines (264 lines over the logic modules) |
| `cargo clippy --workspace --all-targets` | 2 warnings | the same 2, unchanged |
| `cargo fmt --check` / `cargo check` / `pnpm typecheck` | clean | clean |
| `pnpm docs:index` | 151 files / 64 cmds / 65 wrappers | 155 files / **67** cmds / **68** wrappers / 48 core modules |
| `pnpm docs:check` | 20 files | 20 files, all <500 lines, all links resolve |

Every count went **up**; nothing disappeared. All 67 registered commands are
covered by `docs/reference/commands.md` (checked mechanically against
`generate_handler!`, not by eye).

**One test failed on the first full run and is flaky, not a regression:**
`process::tests::cancel_stops_a_long_running_process`, `assertion failed:
sup.cancel("long").await` at `process/mod.rs:487`. It passed 3/3 in isolation
and the whole suite passed clean on re-run (1151/0). Nothing in this phase goes
near `process/`. See `notes.md`.

### Files added

| File | What it does |
|---|---|
| `src-tauri/src/commands/symbols.rs` (140 lines) | `search_everywhere`, `symbol_index_status`, `rebuild_symbol_index`, plus `spawn_build` / `Rebuild` shared with `workspace.rs` and the `setup` hook. Thin: the search command is a state lookup and one `cb_core` call. |
| `src/components/searchLogic.ts` + `.test.ts` | `recogniseShortcut` (the whole keybinding table as one expression, `ShortcutEvent` structural so it tests with no jsdom), `groupHits`, `nextIndex`, `highlightSpans`, `lineToPos`. |
| `src/components/SearchEverywhere.tsx` (407 lines) | The overlay. Rendering shell only. |

### Files modified

- `src-tauri/src/lib.rs` — `mod symbols`, the three `generate_handler!` entries,
  and a `.setup()` hook that starts the index for a workspace given on the
  command line.
- `src-tauri/src/state.rs` — `symbols: Mutex<Option<Arc<SymbolIndex>>>` +
  `symbols_building: AtomicBool`; `begin_symbols_build`, `record_symbols`
  (stale-build guard), `symbols()`, `update_symbols`, `clear_symbols`. Six new
  tests.
- `src-tauri/src/commands/workspace.rs` — `open_workspace` / `rescan_workspace`
  take an `AppHandle` and spawn the build. Other config-mutating commands
  deliberately do **not** re-index.
- `src-tauri/src/commands/files.rs` — `reindex_saved_file`, called by
  `fs_write_file` and (via import) `git_write_file`.
- `src/ipc/types.ts` — `SymbolKind`, `SearchScope`, `HitKind`, `SearchHit`,
  `SymbolIndexStatus`.
- `src/ipc/api.ts` — `searchEverywhere`, `symbolIndexStatus`,
  `rebuildSymbolIndex`.
- `src/App.tsx` — `OpenFileRequest` / `SelectConfigRequest`, a monotonic
  `requestToken` ref, and the palette mounted inside the workspace branch.
- `src/views/RunView.tsx` — `pendingOpen` / `pendingSelect`, consumed by object
  identity; the reveal state passed down to the right editor.
- `src/components/FileEditor.tsx` — `revealLine` / `revealToken`, applied from
  both the effect and the async `build`.
- `src/styles.css` — the palette (+168 lines).

### Docs updated at the gate

`docs/reference/commands.md` (written by the backend agent; verified against
`generate_handler!`), `docs/architecture/frontend.md` (the component list, and
"The one piece of cross-view state" → **two**, with the reasoning for allowing
the second), `docs/architecture/core-crate.md` (a `symbols` entry), 
`docs/architecture/ipc-contract.md` (the new pinning tests, and the two
neighbours in `symbols/` that only look like pinning tests),
`docs/getting-started/using-the-app.md` (a Search everywhere section with the
shortcut table), `CLAUDE.md` (`symbols/` under `crates/core`, the five things
`AppState` now holds, and the tab-adjacent overlay).

### What the gate had to fix

Two doc comments written in Phase 1 that Phase 2 made false — see `notes.md`.
No test was weakened, deleted or skipped.

---

## Phase 2 fix round — four parallel agents, joined and gated (2026-08-10)

The round that followed the Phase 2 gate. Four agents worked concurrently on
disjoint file sets: **backend/state** (`state.rs`, `commands/symbols.rs`),
**save path** (`commands/git.rs`, `commands/files.rs`,
`crates/core/src/symbols/index.rs`), **palette** (`searchLogic.ts` + tests,
`SearchEverywhere.tsx`, `RunView.tsx`), and **docs**. This entry records the
join, not the individual reports.

### Defects fixed

| # | Where | What was wrong | Fix |
|---|---|---|---|
| 1 | `commands/symbols.rs` | `search_everywhere` early-returned on a missing index, so the Actions scope was dead for the whole of a 637 ms warm / 9.4 s cold build — and `SearchEverywhere.tsx:397` told the user the opposite. | Early return replaced by `hits_for`, which substitutes an **empty** `SymbolIndex`. Confirmed by reading `search.rs:172-246` that `search` never touches `index.root`. |
| 2 | `state.rs` | `record_symbols` released the workspace lock between the root check and the store, so workspace B could end up holding A's index reporting `ready: true` — passing the guard honestly. | Workspace guard held across the store; lock order fixed at workspace then symbols. Driven by a test **seam**, `record_symbols_interleaved`. |
| 3 | `commands/git.rs` + `symbols/index.rs` | Git-relative paths (relative to the **repository**, which may sit above the workspace) were written into `SymbolIndex::files` verbatim, producing palette rows that opened nothing. | `git_write_file` resolves against `repo.workdir()` and passes an absolute path; `replace_file` settles file-list membership with an `is_file` stat rather than accepting any well-formed relative string. |
| 4 | `searchLogic.ts` / `SearchEverywhere.tsx` | Palette held one hit list across scope changes, so rows from a previous scope could be drawn or acted on; unactionable Action rows were selectable; keyboard selection did not scroll. | Four new pure decisions — `actionableIds`, `dropUnactionable`, `searchKey`, `resultsState` (the `pending` vs `empty` distinction) — plus row refs and a scroll effect in the component. |
| 5 | `searchLogic.ts` | `lineToPos`'s doc claimed it sends "a line that is not a number at all" to line 1. Only `NaN`; `undefined` and `"abc"` come back out as `NaN`. Reported by the docs agent, not actioned by the owner, **applied at this gate**. | Doc narrowed to what executes, with the real guarantee attributed to the `number` type and `FileEditor`'s narrowing; a characterisation test pins the escape. No behaviour change. |

### Gate results — every count up, nothing vanished

| Gate | Baseline (Phase 2) | This round |
|---|---|---|
| `cargo test -p cb-core` (lib) | 1151 | **1161 passed / 0 failed** |
| `tests/git_operations.rs` | 44 | 44 passed |
| `tests/intent_attribution.rs` | 2 (1 ignored) | 2 passed, 1 ignored |
| `tests/reject_markers.rs` | 11 | 11 passed |
| `cargo test -p cb-app` | 8 | **16 passed / 0 failed** |
| `pnpm test` | 268 (12 files) | **287 passed (12 files)** |
| `pnpm coverage` | ≥70% gate | 99.63% lines, 100% functions |
| `cargo clippy --workspace --all-targets` | exactly 2 warnings | the same 2 (`importers/rider.rs:65`, `workspace.rs:1028`), untouched |
| `cargo fmt --check` / `cargo check --workspace --all-targets` / `pnpm typecheck` | clean | clean |
| `pnpm docs:index` | 155 files / 67 cmds / 68 wrappers / 48 modules | unchanged — no source file, command or public API added |
| `pnpm docs:check` | 20 files | 20 files, all under 500 lines, all links resolve |

`process::tests::cancel_stops_a_long_running_process`, flaky at the previous
gate, passed this time in the full parallel run. No test was weakened, skipped
or deleted; +19 frontend, +10 `cb-core`, +8 `cb-app`.

### Cross-agent consistency, checked by reading both sides

1. **"…so only run configurations can match"** — now **true**. `hits_for(None, …)`
   returns Action hits, pinned by
   `a_configuration_still_matches_while_the_index_is_still_being_built`.
2. **The docs agent's `lineToPos` correction** — the palette agent had **not**
   applied it. Verified the claim by executing it in node (see `notes.md`), then
   applied the doc change plus a characterisation test at the gate.
3. **`replace_file`'s doc versus its behaviour** — agree. The doc's four claims
   (lexical gate rejects build output and paths above the root; an incoming
   symbol whose `path` is not the file is dropped; file-list membership is
   settled by a `stat`; `truncated` is left alone) each match the body.

### Files changed at the gate itself

- `src/components/searchLogic.ts` — `lineToPos` doc corrected.
- `src/components/searchLogic.test.ts` — one existing test renamed to describe
  what it checks, one characterisation test added (287th).

Nothing else needed integration repair: the four agents' file sets did not
overlap and the two Rust crates compiled and tested together on the first try.

### Left open, deliberately

- `SearchScope` / `HitKind` are still declared twice on the frontend
  (`ipc/types.ts` and `searchLogic.ts`). Reported at two consecutive gates now.
  It is a decision about which convention wins, not an integration failure, and
  picking one silently is what the round's rules forbid.
- Nobody has still run the application. Whether WebView2 lets Ctrl+N reach the
  document remains unverified by any gate — see `notes.md`.

---

## Phase 3 — The derived project graph in `cb-core` (2026-08-11)

Six agents in parallel — **dotnet** (`<ProjectReference>` extraction), **node**
(`workspace_globs`), **graph** (module skeleton + `graph.rs`), **mermaid**
(`render` + `validate`), **store** (`.code-basics/diagrams/`), **bridge** (the
six Tauri commands and the IPC mirror) — plus this integration gate. The graph
derives itself from manifests on disk; nothing renders it yet.

### Gate, before → after

| Gate | Baseline (Phase 2 fix round) | Phase 3 |
|---|---|---|
| `cargo test -p cb-core` (lib) | 1161 | **1271 passed / 0 failed** |
| `tests/git_operations.rs` | 44 | 44 passed |
| `tests/intent_attribution.rs` | 2 (1 ignored) | 2 passed, 1 ignored |
| `tests/reject_markers.rs` | 11 | 11 passed |
| **`cb-core` total** | **1161** | **1328 passed / 0 failed (+167)** |
| `cargo test -p cb-app` | 27 | 27 passed — **flat, see below** |
| `pnpm test` | 292 (12 files) | 292 passed — **flat, see below** |
| `pnpm coverage` | ≥70% gate | 99.64% lines, 100% functions (283/284) |
| `cargo clippy --workspace --all-targets` | exactly 2 warnings | the same 2 (`importers/rider.rs:65` while-let, `workspace.rs:1028` cmp_owned), untouched |
| `cargo fmt --check` / `cargo build --workspace` / `pnpm typecheck` | clean | clean |
| `pnpm docs:index` | — | 163 files / 73 cmds / 74 wrappers / **51** core modules |
| `pnpm docs:check` | 20 files | 20 files, all under 500 lines, all links resolve |

**No test vanished.** Checked mechanically rather than by eye: `git diff -U0`
over the tracked files yields **0** deleted `#[test]`/`#[tokio::test]`
attributes and 34 added, on top of the three wholly new `architecture/*_tests.rs`
files. The two flat counts are both explained and neither is a loss:

- `cb-app` stayed at 27 because the bridge agent's six commands are the thin
  kind the house rules ask for — a state lookup and one `cb_core` call — and all
  the decisions they wrap are tested in `cb-core`. `src-tauri` still only carries
  the `state.rs` tests.
- `pnpm test` stayed at 292 because **no Phase 3 agent touched frontend logic**.
  `types.ts` and `api.ts` gained mirrors and wrappers, which are declarations and
  `invoke` one-liners, not `*Logic.ts` decisions. There is no Architecture tab
  yet (Phase 5), so nothing calls the wrappers.

`process::tests::cancel_stops_a_long_running_process` failed on the first full
run (`assertion failed: sup.cancel("long").await`, `process/mod.rs:487`) and
passed alone immediately after, then passed in a clean full re-run. Known flake,
recorded at two previous gates; nothing in this phase goes near `process/`.

### Files added — `crates/core/src/architecture/` (4254 lines incl. tests)

| File | What it does |
|---|---|
| `mod.rs` | Why the graph is a separate artifact rather than a `Project` field, and the sharpened abstain rule (an arrow is a stronger claim than a hunk label). |
| `graph.rs` / `_tests.rs` | `project_graph(&Workspace) -> ArchGraph`; `ArchKind`, `EdgeKind`, `Derivation`, `ArchNode`, `ArchEdge`; `SCANNER_VERSION`. Ordered, de-duplicating `Builder`. |
| `mermaid.rs` / `_tests.rs` | `render(&ArchGraph) -> String`, `validate(&str)`, `ValidationRule`, `ValidationError`. |
| `store.rs` / `_tests.rs` | `.code-basics/diagrams/`; `dir`/`derived_dir`/`prompts_dir`/`path`, `parse`/`render`, `list`/`read`/`write`/`write_authored`; `FrontMatter`, `DiagramFile`, `DiagramDerivation`, `FORMAT_VERSION`, `KNOWN_KEYS`. |

### Files modified

- `crates/core/src/lib.rs` — `pub mod architecture;`.
- `crates/core/src/adapters/dotnet.rs` + `_tests.rs` — `project_references` on
  the parse result, handled in **both** XML arms (`Event::Start` line 143,
  `Event::Empty` line 166), as the plan required.
- `crates/core/src/adapters/node.rs` + `_tests.rs` — `workspace_globs(&PackageJson)`,
  reading npm/pnpm's array form and Yarn's `{"packages":[…]}` object form.
- `crates/core/src/config.rs` — `diagrams/derived/` and `diagrams/.prompts/`
  added to `IGNORED`, pinning test updated.
- `src-tauri/src/commands/architecture.rs` (new) + `lib.rs` — six commands:
  `arch_project_graph`, `arch_render_graph`, `arch_list_diagrams`,
  `arch_read_diagram`, `arch_write_diagram`, `arch_validate`. All six verified
  present in `generate_handler!` **and** in `docs/reference/commands.md`,
  checked mechanically.
- `src/ipc/types.ts` / `api.ts` — the mirrors and the six wrappers.

### Docs written at this gate

All four were genuinely undone when the gate started — no agent had touched them.

- `docs/architecture/core-crate.md` — an `architecture` entry in the voice of
  the surrounding ones.
- `docs/architecture/ipc-contract.md` — the five new pinning tests, plus two
  conventions bullets: the externally-tagged `Derivation`/`DiagramDerivation`
  (the exception to the internal-`kind`-tag rule) and the deliberate absence of
  `skip_serializing_if` across every architecture type.
- `docs/reference/configuration.md` — a `Diagrams (diagrams/)` section, the
  three-way directory split and which parts are gitignored; the top-level tree
  and the `.gitignore` sentence also gained `symbols.json`, which had been
  missing since Phase 1.
- `CLAUDE.md` — an `architecture/` entry under `crates/core`, and `architecture`
  added to the `commands/{…}.rs` list.

### What the gate had to fix

**Nothing in the code.** The six agents' file sets did not overlap, both crates
compiled and tested together on the first try, and every claim spot-checked
against the source held up. Doc-only changes at this gate.

## Phase 3 fix round — five defects, then the full gate (2026-08-11)

Five agents fixed five defects in the Phase 3 code, each driven by a test that
failed first for the right reason. The gate then ran the whole suite over the
joined tree.

### What was fixed

- **Mermaid directive/click injection.** `validate` scanned for `%%{` on the
  *quote-stripped* line, so a directive or a `click` callback smuggled in behind
  an unbalanced quote passed. `forbidden_directives` now scans the **raw** line;
  `outside_quotes` folded into `code_of`.
- **`Project::id` collision reaching the graph.** `workspace::project_id`
  replaces both separators with `-`, so `src/a/App.csproj` and
  `src-a/App.csproj` share an id; the builder deduped on it and handed one
  project the other's arrows. Fixed at the consumer (`graph.rs::NodeIds`) — see
  `notes.md` for why `workspace.rs` was **not** changed.
- **Rooted `<ProjectReference Include>`** forging an in-workspace path and
  drawing a confident wrong arrow. Now `is_rooted` → `External` node. Full
  reasoning in `notes.md`.
- Plus two further defects in the same round, all covered by the +18 new
  `cb-core` tests.

### Gate results (joined tree, verified — not reported)

| Check | Result |
|---|---|
| `cargo fmt --check` | clean (whole workspace) |
| `cargo clippy --workspace --all-targets` | **2** warnings — `importers/rider.rs:65`, `workspace.rs:1028`, both pre-existing and untouched |
| `cargo test -p cb-core` | **1289** passed / 0 failed (baseline 1271, **+18**) — plus 44 + 2 + 11 integration, 0 failed |
| `cargo test -p cb-app` | **27** passed / 0 failed (flat, as expected — commands stay thin) |
| `cargo build --workspace` | clean |
| `pnpm typecheck` | clean |
| `pnpm test` | **292** passed / 0 failed |
| `pnpm coverage` | 99.64% lines (gate 70%) |
| `pnpm docs:index` | regenerated — 163 files, 73 commands, 74 wrappers, 51 core modules |
| `pnpm docs:check` | passed — 20 files, all under 500 lines, all links resolve |

`process::tests::cancel_stops_a_long_running_process` did **not** flake this run.

### Two flat counts, both checked rather than assumed

- **`pnpm test` 292 = baseline 292.** Not a sign that tests vanished:
  `src/components/searchLogic.test.ts` is untracked but was already in the
  baseline snapshot. Confirmed by `vitest list` — 12 files, 63 of the 292 tests
  are searchLogic's. All 12 `*Logic.ts` modules appear in
  `coverage-final.json`, including `searchLogic.ts`; the text reporter only
  elides fully-covered files, which is why the printed table lists four.
- **`cb-app` 27 = baseline 27.** Phase 3 added `commands/architecture.rs` and
  `commands/symbols.rs`, which are thin bridges with the decisions in `cb-core`.

### One reported contradiction, resolved as none

An agent recorded that `workspace.rs` was "deliberately untouched", yet it shows
as modified. Both are true: the modification is the *symbols* agent's
`source_walker` extraction (one walker shared by the project scan and the symbol
index). `git diff -U0 … | grep project_id` returns nothing — `project_id` itself
is unchanged, so the architecture agent's claim holds and the two changes are
orthogonal.

### What the gate had to fix

**Nothing.** No integration breakage. The only write was the regenerated
`docs/INDEX.md`, which was stale against the new `architecture/` and `symbols/`
modules.

## Phase 3 second fix round — three defects, then the full gate (2026-08-11)

Two agents in parallel over `architecture/graph.rs` and `architecture/mermaid.rs`,
then this integration gate. Every fix arrived with a test that had been run and
watched fail for the right reason first.

### The three defects

1. **A `workspaces` key that pnpm ignores was drawn as containment.**
   With a `pnpm-workspace.yaml` present, pnpm does not read `workspaces` in
   `package.json` — it prints *"The \"workspaces\" field in package.json is not
   supported by pnpm"* (verified against pnpm 10.14.0 on this machine via
   `pnpm list -r`). The graph read it anyway, so the diagram drew a container
   labelled from a file the package manager ignores, put the real members
   *outside* it, and left `tools/*` members with no container at all. Now: no
   `Contains` edge and no `workspace:package.json` node from the ignored key,
   and one warning naming **both** files plus the patterns that were not drawn.
   Tests: `a_workspaces_key_pnpm_ignores_is_never_drawn_as_containment`,
   `a_pnpm_workspace_file_is_reported_because_its_members_are_not_read`
   (`graph_tests.rs`).

2. **A `package.json` broken before the scan was invisible, not reported.**
   The pre-existing unparseable/unreadable tests broke the file *after* a valid
   scan, so a project already existed and the warning came from
   `node_dependencies`. The case a user actually hits — a manifest already
   broken when the workspace is opened — produced no project, no box, no edge
   and no warning. `project_graph` now re-reads manifests the scan dropped and
   reports them, quoting the parse error's line. The valid script-less monorepo
   root is explicitly *not* reported, so a monorepo does not open complaining
   about itself. Tests:
   `a_package_json_that_was_already_broken_before_the_scan_is_reported_not_invisible`,
   `a_broken_root_package_json_is_reported_rather_than_leaving_a_silent_workspace`,
   `a_workspace_root_that_declares_no_scripts_is_not_reported_as_broken`.
   **The underlying app-level bug is still open — see `notes.md` and `todos.md`.**

3. **A one-row legend was drawn.** `write_legend` now returns early when
   `key.entries() < 2`. The rule chosen is the **count**, not "no arrow styles
   present": those two rules differ on exactly one case — a project plus an
   unconnected external, one plain box and one stadium, no arrows — and that
   case is *kept*, because nothing about a stadium says "outside this
   workspace" and the row naming it is the only thing that decodes the shape.
   Tests: `a_legend_with_only_one_entry_is_left_out_because_a_single_row_teaches_nothing`,
   `a_legend_telling_two_shapes_apart_is_kept_even_though_the_diagram_has_no_arrows`,
   `a_diagram_with_nothing_in_it_gets_no_legend` (`mermaid_tests.rs`).

`SCANNER_VERSION` was deliberately left at **1**: `crates/core/src/architecture/`
is still untracked, so no version-1 diagram exists anywhere to be distinguished
from. It gets bumped the first time these rules change *after* the feature ships.

### Files touched

`crates/core/src/architecture/graph.rs`, `graph_tests.rs`, `mermaid.rs`,
`mermaid_tests.rs`. Nothing else — no Tauri command, no frontend, no docs prose.

### Gate results (joined tree, run here — not reported by an agent)

| Check | Result |
|---|---|
| `cargo fmt --check` | clean (whole workspace) |
| `cargo clippy --workspace --all-targets` | **2** warnings — `importers/rider.rs:65`, `workspace.rs:1028`, both pre-existing and untouched |
| `cargo test -p cb-core` | **1296** passed / 0 failed (baseline 1289, **+7**) — plus 44 + 2 + 11 integration, 0 failed |
| `cargo test -p cb-app` | **27** passed / 0 failed (flat — nothing outside `cb-core` changed) |
| `cargo build --workspace` | clean |
| `pnpm typecheck` | clean |
| `pnpm test` | **292** passed / 0 failed, 12 files (flat — Phase 5 is still entirely unstarted) |
| `pnpm docs:index` | regenerated — 163 files, 73 commands, 74 wrappers, 51 core modules |
| `pnpm docs:check` | passed — 20 files, all under 500 lines, all links resolve |

`process::tests::cancel_stops_a_long_running_process` did **not** flake this run.

### What the gate had to fix

**Nothing.** No integration breakage between the two agents, and no
contradiction between their reports: they touched disjoint files
(`graph*.rs` vs `mermaid*.rs`) and the counts reconcile — 128 tests under
`cargo test -p cb-core architecture`, `+7` on the crate total. Both agents
independently left `SCANNER_VERSION` at 1 as instructed. The only write this
gate made was the regenerated `docs/INDEX.md`.

---

## Phase 4 — component map assembly (`architecture/components.rs`)

**Files:** `crates/core/src/architecture/components.rs` + `components_tests.rs`
(new, 24 tests); extended `graph.rs` (two `ArchKind` variants, one `EdgeKind`),
`graph_tests.rs` (key-pinning), `mermaid.rs` + `mermaid_tests.rs` (shapes,
arrow, legend, +3 tests), `architecture/mod.rs`, `src/ipc/types.ts`,
`docs/architecture/core-crate.md`, generated `docs/INDEX.md`.

**Types crossing IPC:** `ArchKind` gained `Service` (`"service"`) and
`DataStore` (`"dataStore"`); `EdgeKind` gained `DataAccess` (`"dataAccess"`).
The spellings are pinned variant-by-variant in
`an_arch_graph_serialises_with_the_keys_the_ui_reads` — one-word variants make
`rename_all = "camelCase"` a no-op, so an untested variant is exactly the one
that ships a union the UI cannot narrow.

**Shapes:** data store `[( )]` (cylinder), service `( )` (rounded), project
`[ ]`, external `([ ])`, container `[[ ]]`. Data-access arrow is `==>`. The
legend picks all of them up and `render`'s output still passes `validate` (the
property test gained two component-map cases).

**The decision worth remembering: there is no project → project edge.** The
task asked for one where a literal `BaseAddress` matched an `applicationUrl`,
but that signal is MEDIUM (its evidence is a `.cs` file, which is not a
declaration file), and `admit` returns no edge-shaped MEDIUM result at all —
drawing it would have meant parsing prose out of `Detail::text` to bypass the
one gate the phase is built around. The fact is reported instead: every MEDIUM
detail whose project is not one of the projects that earned the component
becomes a warning naming caller, callee and file, never the address. If that
arrow is wanted, the fix is in `signals/dotnet.rs` — cite the
`launchSettings.json` line, which *is* a declaration file — not in the
assembly.

**`component_graph` over this repository is empty**, correctly: three projects
(Crasher, Inspector, code-basics), none serving HTTP and none declaring a data
client, so no HIGH signal and therefore no box and no warning. That is the
"empty map, not a project map" rule firing for real.

| Check | Result |
|---|---|
| `cargo test -p cb-core architecture` | **274** passed / 0 failed (baseline 247, **+27**) |
| `cargo test -p cb-core` | **1442** passed / 0 failed, plus 44 + 2 + 11 integration |
| `cargo test -p cb-app` | **27** passed / 0 failed |
| `cargo clippy -p cb-core --all-targets` | **2** warnings, both pre-existing |
| `rustfmt --check` over `architecture/*.rs` | clean |
| `pnpm typecheck` / `pnpm test` | clean / **292** passed |
| `pnpm docs:index` / `pnpm docs:check` | regenerated (56 core modules) / passed |

`process::tests::cancel_stops_a_long_running_process` flaked once under the
full parallel run and passed alone and on re-run — the documented flake.

---

## Phase 4 gate + docs (2026-08-11)

Full quality gate over the joined tree after the five signal agents landed
(`framework`, `dotnet`, `node`, `routes`, `assemble`). **Everything green, no
integration breakage, nothing needed fixing in anyone's code.**

| Check | Result |
|---|---|
| `cargo fmt --check` | clean (whole workspace) |
| `cargo clippy --workspace --all-targets` | **2** warnings, both pre-existing (`importers/rider.rs:65`, `workspace.rs:1028`); zero from `architecture/` |
| `cargo test -p cb-core` | **1442** passed / 0 failed (+ 44 / 2 (+1 ignored) / 11 integration) |
| `cargo test -p cb-app` | **27** passed / 0 failed |
| `cargo build --workspace` | clean |
| `pnpm typecheck` | clean |
| `pnpm test` | **292** passed, 12 files |
| `pnpm coverage` | 99.64 % lines (gate is 70) |
| `pnpm docs:index` | 174 files, 73 commands, 74 wrappers, 56 core modules |
| `pnpm docs:check` | 20 files, all under 500 lines, all links resolve |

No flake this round — `process::tests::cancel_stops_a_long_running_process`
passed in the full parallel run.

### The count arithmetic, reconciled exactly

Baseline was **1296**. Per-module counts measured at the gate:

    architecture::signals::framework   18
    architecture::signals::dotnet      37
    architecture::signals::node        39
    architecture::signals::routes      25
    architecture::components           24
                                      ---
                                      143  + 3 new mermaid tests = 146

1296 + 146 = **1442**, and `cargo test -p cb-core architecture` is 128 + 146 =
**274**. Every test claimed by an agent is present; nothing vanished.

`pnpm test` (292) and `cb-app` (27) are flat for the third round running and it
is genuine both times: the phase is entirely `cb-core`, the only frontend file
touched was `src/ipc/types.ts` (two union widenings plus comments, no logic and
therefore no vitest), and no Tauri command was added — `component_graph` is
**not** exposed over IPC yet. 12 vitest files before and after.

### Contradictions between agents

One, and it is a labelling error rather than a disagreement. The framework
agent reported `cargo test -p cb-core architecture` → 18; that filter returns
**274** at the gate and returned 128 at the baseline, so what it actually ran
was `architecture::signals::framework` (which is exactly 18) and wrote the
wider filter down. Its 1314 = 1296 + 18 is right for its own point in the
sequence.

The dotnet/node/routes totals (1415 / 1412 / 1412) look inconsistent with each
other but are not contradictions: the three ran concurrently against a shared
tree, so each is a snapshot of however much of the others' work was merged at
the moment it ran. 1415 = 1296 + 18 + 37 + 39 + 25 exactly — the state after all
four signal modules and before `components`.

### Docs written this round

- `CLAUDE.md` — new bullet in the `cb-core` list for `architecture/signals/` +
  `components.rs`: the HIGH-creates / MEDIUM-enriches / else-discarded-and-counted
  rule, `framework::admit` as the single enforcement point (and *why* it is not
  per-producer), the three standing prohibitions, and the empty-map rule.
- `docs/architecture/ipc-contract.md` — new bullet recording that `ArchKind` and
  `EdgeKind` are pinned **variant by variant** rather than by example, and why
  (`rename_all = "camelCase"` is a no-op on a one-word variant, so an untested
  variant is precisely the one that ships an unnarrowable union).
- `docs/architecture/core-crate.md` — already carried the `signals` and
  `components` entries from the assemble agent; verified against the code rather
  than re-written.
- `docs/INDEX.md` regenerated (it was stale against `signals/` and
  `components.rs`).

Every claim written this round was checked against the source first:
`framework::admit` at `signals/framework.rs:398`, `Admitted::discarded` at 303,
the connection-string screen at 250/354/535, `component_graph` at
`components.rs:105`, and the empty-map behaviour at
`components_tests.rs:127`.

## Adversarial round: 3 route defects + 4 credential-leak channels, full gate green

Three agents, all test-first, all in `crates/core/src/architecture/signals/`.

| Area | Files | What changed |
|---|---|---|
| ASP.NET routes | `routes.rs`, `routes_tests.rs` | `MapGroup` prefix no longer leaks across files; a `RouteGroupBuilder` parameter stops the project resolving; abstract controllers declare nothing. 11 new tests, 6 failed first for the right reason. |
| Express mounts | `node.rs`, `node_tests.rs` | `mount_prefix` returns `Option<Mount>` (`Literal` / `Unreadable`) instead of collapsing "no mount" and "unreadable mount" into `None`; new `argument_list_head` depth/quote scanner. |
| Credential leak | `framework.rs`, `components.rs`, `dotnet.rs` (+ `_tests.rs`) | 4 channels closed: `detail.text` → mermaid, the false doc comment, `Discarded::label` printing a `LabelLooksLikeAValue` label, and `nameable` being weaker than `looks_like_a_value`. |

**Gate, executed end to end:**

| Step | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets` | exactly the 2 pre-existing warnings (`importers/rider.rs:65`, `workspace.rs:1028`) — no new ones |
| `cargo test -p cb-core` | **1468** lib (baseline 1442, **+26**), + 44 git_operations, 2 intent_attribution (1 ignored), 11 reject_markers |
| `cargo test -p cb-app` | **27** (flat — no `src-tauri` change) |
| `cargo build --workspace` | clean |
| `pnpm typecheck` | clean |
| `pnpm test` | **292** (flat — no frontend change) |
| `pnpm docs:index` | regenerated: 174 files, 73 commands, 74 IPC wrappers, 56 core modules |
| `pnpm docs:check` | 20 files, all under 500 lines, all links resolve |

`architecture::` alone is 298 passing. The one red run was
`process::tests::cancel_stops_a_long_running_process` under full parallel load —
the known flake; passed alone immediately after, so not a regression.

No test was weakened or deleted, no dependency added, and nothing outside the
three agents' own files was edited to make the gate pass — there was no
integration breakage to fix.

## Cargo ecosystem round: cargo adapter + `Project.unreadable`, full gate green

The audit finding that opened this round: the diagrams were **true but useless
on the repository they ship in** — three unconnected boxes, because
`code-basics` is a cargo workspace and cargo was not a detected ecosystem.

| Area | Files | What landed |
|---|---|---|
| Cargo adapter | `crates/core/src/adapters/cargo.rs`, `cargo_tests.rs` (new), `adapters/mod.rs` | `parse` → `CargoManifest { package_name, workspace_members, workspace_exclude, path_dependencies, is_workspace_root, has_bin, has_lib }` + `is_virtual_manifest()`. `PathDependency` keeps the real `package` name (not the alias) and the `path` string verbatim; `DependencyKind` preserves normal/dev/build. |
| Scan | `crates/core/src/workspace.rs` (`scan_cargo_project`, `unreadable_project`) | `Cargo.toml` → a project with `ecosystem: "cargo"`, kind from `[[bin]]`/`[lib]` **or** `src/main.rs` / `src/lib.rs` on disk; a virtual manifest is not a project. **No run/test configurations at all** — a declarative adapter is therefore merged *on top of* a cargo-claimed directory instead of being shadowed, which is what keeps `examples/adapters/cargo-nextest.toml` working. |
| Broken manifests | `model.rs` (`Project.unreadable`, `skip_serializing_if`), `workspace.rs`, `src/ipc/types.ts`, `src/views/RunView.tsx`, `src/styles.css` | The long-standing app-level bug on `todos.md` is **fixed**: a `package.json` (or `Cargo.toml`, or project file) that will not parse no longer deletes the project silently. It is listed inert — `ProjectKind::Unknown`, no frameworks, no configs — under a **Could not be read** heading, struck through and dim, reason on the row and manifest path in the tooltip; a saved config targeting it has Run/Restart/build disabled with the reason as tooltip. |
| Graph | `architecture/graph.rs` | `cargo_manifests` / `cargo_dependencies` / `cargo_workspace_members`. Cargo edges resolve **by path** (what cargo itself does), never by name; member/exclude globs resolve against the **manifest's own** directory, and are matched against discovered project directories rather than the filesystem. |

**Gate, executed end to end (this agent, one shared target dir):**

| Step | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets` | exactly the 2 pre-existing warnings — `importers/rider.rs:65` and the `cmp_owned` in `workspace.rs`, now at **:1581** (was :1028) purely because the file grew. No new lints. |
| `cargo test -p cb-core` | **1523** lib (baseline 1468, **+55**) + 44 git_operations + 2 intent_attribution (1 ignored) + 11 reject_markers = **1580 passed / 0 failed** |
| `cargo test -p cb-app` | **27 / 0** (flat — the new `arch_*` and `symbols` commands are thin delegators with no tests of their own) |
| `cargo build --workspace` | clean |
| `pnpm typecheck` | clean |
| `pnpm test` | **292 / 0** (flat) |
| `pnpm docs:index` | 176 files, 75 commands, 76 IPC wrappers, 57 core modules |
| `pnpm docs:check` | 20 files, all under 500 lines, all links resolve |

The conflict an earlier agent reported —
`a_package_json_that_was_already_broken_before_the_scan_is_reported_not_invisible`
failing because the fixture asserted the scan **drops** a broken
`package.json` — is **resolved in the tree as it now stands**: the test passes,
and it passes asserting the *new* behaviour (the project is listed carrying
`unreadable`). Nothing was weakened to get there; the fixture's own message
("the fixture only means anything while the scan really does drop it") is gone
from the current file.

Docs updated this round: `CLAUDE.md` (cargo as a detection-only adapter,
`unreadable` on the `workspace.rs` line, cargo sources on the `architecture/`
line), `docs/architecture/core-crate.md` (new `adapters::cargo` section + the
unreadable paragraph under `workspace`), `docs/getting-started/using-the-app.md`
(Rust detection without runs; the greyed-out **Could not be read** group).
`docs/architecture/ipc-contract.md` and `docs/reference/commands.md` were
already correct and in sync — the handler list and the command table match
exactly, both directions, checked with `comm`.

---

## Phase 4 — the UI, and the full gate over it

The tab exists. `src/views/ArchitectureView.tsx` (612) over
`src/views/architecture/`: `DiagramCanvas.tsx` (703, lazy-imports mermaid,
pan/zoom, click-to-open, and the warnings panel), `DiagramEditor.tsx` (275,
CodeMirror over a stored diagram with live validation), and five pure modules —
`architectureLogic.ts` (258), `nodeTargets.ts` (332), `panZoomLogic.ts` (210),
`viewportLogic.ts` (117), `copyLogic.ts` (79) — each with a co-located
`.test.ts`. `App.tsx` gained the `architecture` tab between `history` and
`inspect`; `styles.css` gained an Architecture section.

### Gate, all executed this round

| Step | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets` | exactly the 2 pre-existing — `while_let_loop` in `importers/rider.rs:65` and `cmp_owned` in `workspace.rs` (now **:1729**). No new lints. |
| `cargo test -p cb-core` | **1541 total**; first pass 1540 passed / 1 failed, the failure being the documented flake `process::tests::cancel_stops_a_long_running_process`, which then passed **3/3 run alone**. Flat vs baseline — this phase added no Rust. |
| `cargo test -p cb-app` | **27 / 0** (still flat; see the todo about the untested `arch_*` commands) |
| `pnpm typecheck` | clean |
| `pnpm test` | **391 / 0, 17 files** (baseline 292 / 12 → **+99 tests, +5 files**) |
| `pnpm coverage` | Lines **99.72%**, Branches 97.94%, Functions 100%. Every architecture module at 100% lines. |
| `pnpm build` | clean, 28.9 s |
| `pnpm docs:index` | 189 files, 75 commands, 76 IPC wrappers, 57 core modules |
| `pnpm docs:check` | 20 files, all under 500 lines, all links resolve |

### The bundle question, answered from the sourcemap rather than the sizes

`pnpm build` prints a **1,467.75 kB entry chunk** (`index-*.js`, the only
`<script>` in `dist/index.html`), which looks alarming next to the Phase 0
spike's "~3 kB entry". It is not a regression and the spike number is not
comparable — that was a bare spike page, this is the whole app.

Verified rather than assumed, two ways:

1. The entry's only reference to mermaid is a dynamic
   `import("./mermaid.core-*.js")` behind a memoised null-check singleton.
   There is no static import.
2. Totalling `sourcesContent` per package out of `index-*.js.map` gives
   react-dom 530 kB, `@codemirror/view` 479, `@xterm/xterm` 282, `src/views`
   252, `src/components` 170, then `@codemirror/state` and the lezer grammars.
   **Zero mermaid sources appear in the entry chunk's sourcemap at all.**

Mermaid lands in its own 623 kB chunk plus per-diagram-family chunks
(sequence 117 kB, architecture 152 kB, …), cytoscape 444 kB and katex 261 kB —
all lazy. The dynamic import is doing its job. Total `dist` js 5.66 MB.

### Docs updated

`CLAUDE.md` (five → **six** tab views, the Architecture entry, the note that
`architecture`'s id matches its label *unlike* `inspect`, the mermaid/CSP
constraints, and that `ArchGraph.warnings` must reach the UI);
`docs/architecture/frontend.md` (six tabs, the `views/architecture/` tree, a
new "The Architecture tab" section, and `openRequest` now having a second
producer); `docs/getting-started/using-the-app.md` (six views, a full
Architecture section, `diagrams/` in the state list).
`docs/reference/configuration.md` needed **no change** — its `## Diagrams
(diagrams/)` section already documented the storage split, the front matter and
the "which directory is not a parameter" rule, and all of that is still true.

---

## Phase 4 (UI) fix round — gate over the two verification defects (2026-08-12)

Two agents fixed the two defects the running-app verification found; this entry
records the gate over the joined tree, plus the one coverage-gate change the
gate itself made.

### What the two agents changed

| Defect | Files | Fix |
|---|---|---|
| Click-to-open resolved nothing — `NODE_SELECTOR = "g.node[data-id]"` matched no element mermaid emits | `src/views/architecture/nodeTargets.ts` + `.test.ts`, `DiagramCanvas.tsx` | `BOX_SELECTOR = "g.node[id], g.cluster[id]"`, and resolution now goes **forward** through `mermaidIdOf(nodeId)` — a mirror of `mermaid_id` in `crates/core/src/architecture/mermaid.rs` — instead of a hand-written decoder. |
| Every stored diagram was labelled "will not render" | `src/views/architecture/frontMatterLogic.ts` + `.test.ts`, `DiagramEditor.tsx` | Front matter is stripped before `arch_validate`, and the dropped line count is added back to the reported line number. The reproduction is quoted verbatim in the test file's header (`WHOLE` → `DiagramType` at line 1, `BODY` → `Ok`). |

The frontmatter fix also landed one `cb-core` test —
`mermaid_tests.rs::line_numbers_are_reported_against_the_source_as_the_user_typed_it`
— which is the whole of the `cb-core` movement this round.

### The one change the gate made

`vite.config.ts` — `src/views/architecture/nodeTargets.ts` added to
`test.coverage.include`. This closes the todo carried out of the Phase 4 UI
round: the file has ~30 tests but was outside the ≥70% gate, so deleting them
would not have failed `pnpm coverage`.

**Verified rather than assumed**, by removing the line, re-running, and putting
it back: without it the report measures **372** lines over **18** files and has
no `nodeTargets.ts` row; with it, **485** lines over **19** files and
`nodeTargets.ts` at 99.11%. The v8 text reporter still elides files at 100% on
every metric, so read `coverage/coverage-final.json` — not the printed table —
when asking whether a module is measured. That is how the gap went unnoticed.

### Gate, every step executed here

| Step | Baseline | Result |
|---|---|---|
| `cargo fmt --check` | clean | clean |
| `cargo clippy --workspace --all-targets` | exactly 2 | exactly **2** — `importers/rider.rs:65` while-let, `workspace.rs:1729` cmp_owned. Both pre-existing, untouched. |
| `cargo test -p cb-core` (lib) | 1541 | **1542 / 0** (+1, the mermaid line-number test) |
| — integration | 44 / 2 (1 ignored) / 11 | 44 / 2 (1 ignored) / 11, all 0 failed |
| `cargo test -p cb-app` | 27 | **27 / 0** (flat — no `src-tauri` change) |
| `pnpm typecheck` | clean | clean |
| `pnpm test` | 391 / 17 files | **423 / 0, 19 files** (+32 tests, +2 files) |
| `pnpm coverage` | ≥70% | Lines **99.58%** (483/485), Branches 97.31%, Functions 100% |
| `pnpm build` | clean | clean, 30.4 s |
| `pnpm docs:index` | 189 files | **193** files, 75 commands, 76 wrappers, 57 core modules |
| `pnpm docs:check` | 20 files | 20 files, all under 500 lines, all links resolve |

No flake this round — `process::tests::cancel_stops_a_long_running_process`
passed in the full run.

**Nothing was weakened or deleted.** Checked mechanically, not by eye:
`git diff -U0` over the tracked files has **0** removed `#[test]`/`#[tokio::test]`
attributes (55 added) and **0** removed vitest `it(`/`test(` calls.

The +2 vitest files are `frontMatterLogic.test.ts` and `emptyStateLogic.test.ts`;
`nodeTargets.test.ts` already existed and was rewritten, not added.

### Mermaid is still lazy — checked in the built output, not inferred

`dist/index.html` loads exactly one script, `assets/index-ySLtRb0z.js`
(1,469.89 kB / 472.92 kB gzip). That chunk has **zero static imports** and
mentions mermaid three times: once as `import("./mermaid.core-DM-gku9C.js")`
behind the memoised null-check singleton, and twice in UI copy
("Mermaid passes through states like this…", "Edit the stored Mermaid source").

Lazy chunk sizes:

    mermaid.core          623.27 kB │ gzip 149.56 kB
    cynefin (diagram)     688.05 kB │ gzip 152.10 kB
    cytoscape.esm         443.74 kB │ gzip 142.40 kB
    katex                 261.36 kB │ gzip  77.61 kB
    architectureDiagram   151.89 kB │ gzip  43.17 kB
    swimlanes             118.87 kB │ gzip  43.69 kB
    sequenceDiagram       117.03 kB │ gzip  31.16 kB

The 1.47 MB entry is react-dom + CodeMirror + xterm, unchanged in kind from the
Phase 4 measurement (1,467.75 → 1,469.89 kB); rollup's >500 kB warning is about
that entry, not about mermaid.

### What the gate had to fix

**Nothing in either agent's code.** The two file sets are disjoint, both crates
compiled and tested together on the first try, and the two reports do not
contradict each other. The only writes this gate made were `vite.config.ts` and
the regenerated `docs/INDEX.md` (stale — +97/-28 lines).
