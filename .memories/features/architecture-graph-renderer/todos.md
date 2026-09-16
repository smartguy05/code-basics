# Architecture graph renderer — todos

## Done
- [x] graphStyleLogic.ts (+test) — kind→color, edge styles, legend rule
- [x] layoutLogic.ts (+test) — top-down layered layout
- [x] GraphCanvas.tsx — SVG renderer shell
- [x] ArchitectureView.tsx — pick GraphCanvas for structured graphs
- [x] pnpm typecheck clean; pnpm test 2155 pass

## Pending / follow-up
- [ ] Visual confirmation in the running app (user to reload Architecture tab;
      switch away+back remounts it). Not yet observed by the model.
- [ ] Possible polish after seeing it live:
      - node radius / row+col gaps (layout constants) for density
      - label wrapping/truncation for long names (currently single <text> line)
      - edge routing when many edges overlap (only one barycentre pass today)
      - optional arrowheads on edges (currently plain curves)
- [ ] Consider whether saved diagrams should also drop the yellow (Mermaid
      dark-theme restyle) — deferred; out of current scope.
