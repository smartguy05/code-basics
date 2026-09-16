# Component map: surface .NET app components — completed

Implemented 2026-09-16 via Workflow `wf_69698da3-03d` (4 Opus agents, tests-first),
then verified end-to-end by the orchestrator (agent self-reports + live IDE
diagnostics were NOT trusted — the IDE showed stale mid-run compile errors that
`cargo check` proved false).

## What shipped
- **New node kinds** `ArchKind::{WebApp, MobileApp}` (graph.rs) + `ComponentKind::
  {WebApp, MobileApp}` (framework.rs), in IPC lockstep: mermaid.rs (hexagon /
  parallelogram shapes + Key fields + legend rows), src/ipc/types.ts union,
  graph_tests.rs spelling assertions. `SCANNER_VERSION` 3 → 4.
- **Detection** (signals/dotnet.rs): `classify()`/`classify_with_trigger()` reads
  the .csproj — BlazorWASM SDK → WebApp; `UseMaui` → MobileApp; Web/Aspire/
  IsAspireHost/`Microsoft.AspNetCore.App` FrameworkReference/Worker SDK →
  HttpService(Service). `classified_component()` emits one HIGH signal of that
  kind. `<FrameworkReference>` now parsed in adapters/dotnet.rs (`framework_references`).
- **launchSettings promotion** (Bindings::of): classified project's applicationUrl
  → MEDIUM enrichment at its own kind (avoids medium-without-high/double node);
  unclassified project with applicationUrl → HIGH HttpService citing
  launchSettings.json. Address-book binding recorded both ways.
- **Assembly** (components.rs): `app_arch_kind()` maps app ComponentKinds → ArchKinds;
  pass 1 draws all app nodes and collects `app_ids` (was `service_ids`) so web/mobile
  apps can be ServiceCall endpoints; pass 2 draws data stores.
- **Warning collapse** (routes.rs): per-line DECLINED_MAPS (MapGrpcService…) warnings
  → one summary-with-count per (project, map-name).
- **Renderer** (frontend): graphStyleLogic.ts — Service `--pass`, Web app
  `--syntax-property` (cyan), Mobile app `--skip` (amber); legend order after Service;
  GraphCanvas READ_VARS += `--syntax-property`.

## Verification (orchestrator, not agents)
cargo test -p cb-core = 4315 lib + all integration, 0 failed. cargo check --all-targets,
cargo fmt --check, pnpm typecheck all clean. pnpm test = 2161 pass. Fixed one agent
leftover: `classify` was `pub` over a private `Read` type → made module-private.

## To SEE it in the app
The running `pnpm tauri dev` holds an OLD Rust build. Frontend TS is live via HMR,
but the Rust deriver changes need the Tauri shell rebuilt — restart `pnpm tauri dev`
(a running cb-app.exe locks the exe; a rebuild while it runs hits "Access is denied").
Not done here to avoid killing the user's session.

## Notes
- Tests assert on the ArchGraph (components_tests.rs `mapped()`), never on signals.
- 5 pre-existing tests were intentionally updated (not weakened) because the
  launchSettings promotion inverts the old "applicationUrl never creates a node".
- gRPC stayed warning-collapse only (no standalone gRPC projects in ONEflight).
