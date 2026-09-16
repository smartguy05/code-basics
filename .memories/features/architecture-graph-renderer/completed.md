# Architecture graph renderer — completed record

## Goal
Replace the plain/boxy Mermaid look (esp. the yellow "Legend" subgraph) of the
two **derived** architecture maps (Project map, Component map) with a bespoke
SVG renderer: dark canvas, glowing category-coloured circular nodes, curved
edges, chip legend at the top. Saved/agent-authored diagrams keep Mermaid.

## Key enabling fact
Built-in maps already ship the full structured `ArchGraph` to the frontend
(`ArchitectureView.load()` fetches graph + mermaid source in parallel). So the
new renderer needed **zero Rust changes** — it draws straight from `graph`.
Saved diagrams have `graph: null` (mermaid text only) → they stay on
`DiagramCanvas`.

## Files
- **New** `src/views/architecture/graphStyleLogic.ts` (+test) — `ArchKind`→color
  (`categoryOf`), `EdgeKind`→stroke (`edgeStyle`), and `legendEntries` (only
  categories present, fixed order — mirrors `mermaid.rs::Key`). CSS-var names
  only, no literals. Uses `import type` from ipc/types (erased, node-env safe).
- **New** `src/views/architecture/layoutLogic.ts` (+test) — pure top-down layered
  layout. `layout(nodes, edges, opts?) → { positions: Map, bounds: Box }`.
  Longest-path layering via bounded relaxation (cycle-safe), barycentre row
  ordering with id tie-break (deterministic), rows centred on x=0. Reuses `Box`
  from panZoomLogic.
- **New** `src/views/architecture/GraphCanvas.tsx` — untested SVG shell. Reuses
  panZoomLogic (fit/zoom/pan), `targetFor` for clicks, `derivationLabel`/
  `warningSummary`, and the DiagramCanvas pointer-capture click mechanism
  (`pressedBoxRef` read at pointerdown). Glow = CSS `drop-shadow` filter with
  theme-resolved color. Reads live CSS vars via getComputedStyle + re-reads on
  `onAppearanceChange`.
- **Modified** `src/views/ArchitectureView.tsx` — renderer picked by data shape:
  `shown.graph !== null ? <GraphCanvas> : <DiagramCanvas>`.

## Untouched (deliberately)
`mermaid.rs`, all `arch_*` commands, validation, `.mmd` export ("Save a copy…"),
DiagramEditor. No new deps.

## Verification
- `pnpm typecheck` clean; `pnpm test` 2155 passed (incl. new layoutLogic +
  graphStyleLogic tests, 19 cases).
- Visual check in `pnpm tauri dev` on the ONEflight workspace — [pending run].

## Follow-up (2026-09-15): project-map colour + 191-warning triage
- Project map was all-blue because it is (almost) all `ArchKind::Project` and
  every project was `--accent`. Fixed: `categoryOf` now takes the whole node and
  colours a **project by ecosystem** (dotnet→--accent blue, node→--syntax-type
  teal, cargo→--syntax-string orange, other/unknown→--text-dim). Service/dataStore/
  solution/external keep role colours. Legend + READ_VARS updated. A nearly
  all-.NET repo is still mostly blue — honest; the legend now names it.
- Component map "191 things this diagram could not draw" is **pre-existing deriver
  behaviour** (191 in the ORIGINAL screenshot too), not a render bug. Dominated by
  gRPC `MapGrpcService` lines (path lives in a .proto the deriver deliberately does
  not read) + `AddHttpClient` calls with no literal base address + a few
  medium-without-high HTTP signals. Surfacing more components = backend deriver
  work (parse .proto / gRPC), out of the visual scope. Optional UX: collapse
  duplicate warnings in the panel.

## Notes / gotchas
- ArchNode has **no size/bytes field** (the "45 KB" badge in the reference is
  cosmetic and not derivable — omitted).
- SVG filter needs a real color, not `var(...)` — hence resolving theme vars to
  values at render (sanctioned pattern, `appearance.ts::terminalAppearance`).
- Data stores / externals / solution folders carry no path → not clickable
  (`targetFor` returns null), same abstain rule as the Mermaid canvas.
