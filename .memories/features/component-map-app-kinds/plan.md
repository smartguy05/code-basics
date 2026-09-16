# Component map: surface .NET app components (plan)

Full plan: `~/.claude/plans/snazzy-sparking-stallman.md`. Being implemented via
Workflow `wf_69698da3-03d` (Opus agents, tests-first), started 2026-09-16.

## Goal
The component map draws only ~6 nodes + ~191 refusals on ONEflight. Surface the
real apps (backend services, Blazor web apps, MAUI mobile apps) and collapse the
warning noise, without breaking the "draw only what the author declared" gate.

## Phases
- **A Model** — new `ComponentKind::{WebApp,MobileApp}` (framework.rs) + new
  `ArchKind::{WebApp,MobileApp}` (graph.rs), IPC lockstep (mermaid.rs shapes/Key/
  legend, types.ts union, graph_tests.rs spellings), bump `SCANNER_VERSION` 3→4.
- **B+C Detect+Assemble** — parse `<FrameworkReference>` (adapters/dotnet.rs);
  `classify(read)->Option<ComponentKind>` from .csproj (BlazorWASM→WebApp,
  UseMaui→MobileApp, Web/Aspire/AspNetCore.App/Worker→HttpService); promote a
  `launchSettings.json` applicationUrl to HIGH when unclassified (double-node
  avoidance: classified project's launch profile enriches with its own kind);
  components.rs pass-1 generalized via `app_arch_kind()` + `app_ids`.
- **D Warnings** — routes.rs: collapse per-line `DECLINED_MAPS` (MapGrpcService…)
  warnings into one summary-with-count per (project, map-name).
- **E Render** — graphStyleLogic.ts: Service `--pass`, Web app `--syntax-property`
  (cyan), Mobile app `--skip` (amber), + legend order; GraphCanvas READ_VARS.

## Key facts / gotchas
- HIGH signal must cite a declaration file (`is_declaration_file`: csproj/json).
  launchSettings.json IS one → promotion needs no gate change. `.cs`/`.proto` are not.
- Node kind decided by pass ordering + first-wins Builder in `component_graph`.
- ONEflight has NO standalone gRPC projects — all MapGrpcService are inside
  Server.Api (already a Service). So gRPC = warning-collapse only, no new node kind.
- IPC lockstep for a new ArchKind: graph.rs + mermaid.rs + types.ts + graph_tests.rs
  (`an_arch_graph_serialises_with_the_keys_the_ui_reads`) + frontend graphStyleLogic.
- Tests assert on the ArchGraph (components_tests.rs `mapped()` helper), not signals.
- Verify: `cargo test -p cb-core` (Git Bash, sh on PATH), `pnpm test`, `pnpm typecheck`.
  Then release build for the intent/quality-gate hooks (Access-denied ⇒ app running).
