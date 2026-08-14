# Plan (condensed)

Full plan: `C:/Users/AnthonyJames/.claude/plans/to-better-understand-the-compressed-pascal.md`

## Phase 0 — Blocking spike: mermaid under the CSP

CSP at `src-tauri/tauri.conf.json:24` is
`default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:` —
no `unsafe-eval`. Whether mermaid renders under it is **still unverified**.

Render one each of `flowchart`, `sequenceDiagram`, `classDiagram`, `erDiagram`,
`stateDiagram-v2` with `securityLevel: "strict"`, `flowchart.htmlLabels: false`.

- Clean → ship mermaid pinned exact, **dynamically imported**.
- Some families need eval → ship it, enforce a diagram-type allowlist in the
  Rust validator, one test per rejected type.
- Eval needed unconditionally → render a subset ourselves
  (`mermaidSubset.ts` + `layeredLayout.ts`, ~400-600 lines, no deps). Mermaid
  text stays the on-disk format either way.

**Do not add `'unsafe-eval'`.**

## Phase 1 — The symbol index (`cb-core`) — DONE

1a. `workspace::source_walker(root) -> SourceWalker` extracted from `scan_with`.
1b. Declaration heuristic lifted to `symbols/declarations.rs`, plus the
    `Task<int>` bug fix and `SymbolKind`. `git → symbols`, never the reverse.
1c. `symbols/fuzzy.rs` — `score(query, candidate) -> Option<Match>`, smart case,
    subsequence pre-pass, bounded DP, deterministic ranking.
1d. `symbols/index.rs` + `symbols/cache.rs` — `Symbol`/`SymbolIndex`, caps that
    set `truncated`, `.code-basics/symbols.json` keyed on `(mtime, len)`.
1e. `symbols/search.rs` — `search(index, configs, query) -> Vec<SearchHit>`,
    `SearchScope { All | Files | Symbols | Actions }`, trailing `:123` parsed in
    Rust.

## Phase 2 — Search Everywhere (palette)

`AppState` gains `symbols: Mutex<Option<Arc<SymbolIndex>>>` +
`symbols_building: AtomicBool`, cleared on the root-changed branch of
`set_workspace` exactly like `last_inspect`. Commands
`search_everywhere` / `symbol_index_status` / `rebuild_symbol_index` in
`src-tauri/src/commands/symbols.rs`. `open_workspace` / `rescan_workspace` spawn
the build and return before it finishes. `fs_write_file` / `git_write_file`
re-index the single file they wrote.

Frontend: `searchLogic.ts` (+ `.test.ts`, test-first) for shortcut recognition,
grouping, wrap math and highlight spans; `SearchEverywhere.tsx` as an untested
rendering shell, debounced ~80 ms with an in-flight guard. Shortcuts registered
on `window` in the **capture phase** — precedent `OutputConsole.tsx:203-214`.

Opening a file needs no state lifting: only the *request* moves to `App`,
copying the `inspectRequest` / `pendingRequest` / `onRequestConsumed` pattern.

## Phase 3 — Derived diagrams (levels 1-2)

New `crates/core/src/architecture/` — `graph.rs`, `mermaid.rs`, `components.rs`,
`prompt.rs`, `store.rs`. **Do not add edges to `Project`.**

Order of failing tests: `<ProjectReference>` (both the `Event::Start` and
`Event::Empty` arms of `parse_project_file`) → project-graph edges incl. the
outside-the-workspace warning → node sibling deps → `workspaces` globs via
`globset` over directories the scan already found → solution containment →
mermaid emission.

Level 2 signals are graded HIGH/MEDIUM as described in `work-item.md`.
`docker-compose.yml` would be HIGH but needs a YAML dep — skipped in v1.

## Phase 4 — Agent-authored diagrams (levels 3-4)

Prompt file by default (`ClaudeCode::detected()` only checks that `~/.claude` is
a directory — it does not prove a runnable CLI). Opt-in `diagrams.agentCommand`
runs through the existing `process::Supervisor`. Validation in
`architecture/mermaid.rs::validate`. Provenance as YAML front matter rendered as
a badge chip above the SVG; front matter wins over any body comment.

## Phase 5 — The Architecture tab

`src/views/ArchitectureView.tsx` + `architecture/{DiagramCanvas,DiagramEditor}.tsx`
+ `architectureLogic.ts`, `panZoomLogic.ts`, `nodeTargets.ts`. Mounted
conditionally with `key={workspace.root}`. Pan/zoom hand-written, wheel listener
attached via a ref effect with `{ passive: false }`. Commands
`arch_list_diagrams` / `arch_read_diagram` / `arch_write_diagram` /
`arch_generate` / `arch_prompt` / `arch_validate` — the tab needs its own listing
command because `.code-basics` is in `SKIP_DIRS`, and `arch_write_diagram` must
`create_dir_all` itself because `files::write_file` does not.
