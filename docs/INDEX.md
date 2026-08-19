# Code index

> **Generated** by [`scripts/generate-index.mjs`](../scripts/generate-index.mjs) — do not edit by hand.
> Regenerate with `pnpm docs:index` after adding files, commands, or public APIs.

Use this file to locate things fast: every first-party source file with its one-line purpose, the full Tauri command surface, the frontend IPC wrappers, and the public API of each `cb-core` module.

## Source files

| File | Lines | Purpose |
|------|------:|---------|
| `crates/core/Cargo.toml` | 44 |  |
| `crates/core/build.rs` | 9 |  |
| `crates/core/src/adapters/cargo.rs` | 254 | The Cargo (Rust) ecosystem adapter — manifest reading only. |
| `crates/core/src/adapters/cargo_tests.rs` | 437 | Tests for the Cargo adapter. |
| `crates/core/src/adapters/dotnet.rs` | 1104 | The .NET ecosystem adapter. |
| `crates/core/src/adapters/dotnet_tests.rs` | 1580 | Tests for the .NET adapter. |
| `crates/core/src/adapters/manifest.rs` | 598 | Declarative adapters: adding an ecosystem without writing Rust. |
| `crates/core/src/adapters/mod.rs` | 28 | Ecosystem adapters. |
| `crates/core/src/adapters/msbuild.rs` | 154 | Optional, accurate MSBuild evaluation. |
| `crates/core/src/adapters/msbuild_tests.rs` | 213 | Tests for optional MSBuild evaluation. |
| `crates/core/src/adapters/node.rs` | 393 | The JavaScript / TypeScript ecosystem adapter. |
| `crates/core/src/adapters/node_tests.rs` | 428 | Tests for the JS/TS adapter. |
| `crates/core/src/adapters/solution.rs` | 316 | Reading .NET solution files. |
| `crates/core/src/adapters/solution_tests.rs` | 139 | Tests for solution parsing. Included by `solution.rs` under `#[cfg(test)]`. |
| `crates/core/src/architecture/components.rs` | 427 | The component map: which services this workspace runs, which data stores |
| `crates/core/src/architecture/components_tests.rs` | 996 | Tests for [`super::components`]. |
| `crates/core/src/architecture/graph.rs` | 1672 | The derived project graph: nodes, edges, and the rules that produce them. |
| `crates/core/src/architecture/graph_tests.rs` | 1986 | Tests for [`super::graph`]. |
| `crates/core/src/architecture/mermaid.rs` | 1315 | Rendering an [`ArchGraph`](super::graph::ArchGraph) to Mermaid source, and |
| `crates/core/src/architecture/mermaid_tests.rs` | 1386 | Tests for [`super::mermaid`]. |
| `crates/core/src/architecture/mod.rs` | 77 | The derived project graph: what this workspace is made of, and what points |
| `crates/core/src/architecture/signals/dotnet.rs` | 1342 | Component signals read out of .NET manifests, configuration and source. |
| `crates/core/src/architecture/signals/dotnet_tests.rs` | 1087 | Tests for [`super::dotnet`]. |
| `crates/core/src/architecture/signals/framework.rs` | 824 | The signal types and [`admit`] — the one gate every candidate component in |
| `crates/core/src/architecture/signals/framework_tests.rs` | 775 | Tests for [`super::framework`]. |
| `crates/core/src/architecture/signals/mod.rs` | 88 | Components that are not projects: the databases, caches, queues and HTTP |
| `crates/core/src/architecture/signals/node.rs` | 1379 | Component signals read out of `package.json`, out of the framework |
| `crates/core/src/architecture/signals/node_tests.rs` | 1357 | Tests for [`super::node`]. |
| `crates/core/src/architecture/signals/routes.rs` | 1613 | Signals about the routes an ASP.NET service exposes. |
| `crates/core/src/architecture/signals/routes_tests.rs` | 886 | Tests for [`super::routes`]. |
| `crates/core/src/architecture/store.rs` | 596 | Diagrams on disk: where they live, how they say where they came from, and |
| `crates/core/src/architecture/store_tests.rs` | 631 | Tests for [`super::store`]. |
| `crates/core/src/behavioral/attribute.rs` | 127 | Mapping observable deltas back to the intent card that plausibly caused them. |
| `crates/core/src/behavioral/attribute_tests.rs` | 131 |  |
| `crates/core/src/behavioral/compare.rs` | 144 | Diffing two [`TestRunResult`]s by test-case outcome. |
| `crates/core/src/behavioral/compare_tests.rs` | 119 |  |
| `crates/core/src/behavioral/console.rs` | 223 | Diffing the console output of the two runs. |
| `crates/core/src/behavioral/console_tests.rs` | 80 |  |
| `crates/core/src/behavioral/http.rs` | 214 | Diffing two HTTP responses for one replayed request. |
| `crates/core/src/behavioral/http_tests.rs` | 101 |  |
| `crates/core/src/behavioral/httpfile.rs` | 317 | Parsing `.http` / `.rest` scenario files (VS Code REST Client / JetBrains |
| `crates/core/src/behavioral/httpfile_tests.rs` | 137 |  |
| `crates/core/src/behavioral/mod.rs` | 159 | Behavioral before/after testing — the *runtime* counterpart to the static |
| `crates/core/src/behavioral/mod_tests.rs` | 180 | Key-pinning for the behavioral wire types. |
| `crates/core/src/behavioral/prepare.rs` | 121 | The testable decision seam between the raw deltas and the wire report. |
| `crates/core/src/behavioral/prepare_tests.rs` | 115 | Tests for the pure report-assembly seam. |
| `crates/core/src/behavioral/replay.rs` | 151 | The one impure corner of behavioral testing: firing HTTP requests. |
| `crates/core/src/behavioral/replay_tests.rs` | 55 | Socket-free tests for the two decisions replay makes without the network. |
| `crates/core/src/behavioral/scenario.rs` | 199 | The pure decision seam for serverful HTTP replay. |
| `crates/core/src/behavioral/scenario_tests.rs` | 196 |  |
| `crates/core/src/behavioral/worktree.rs` | 279 | An on-demand, isolated checkout of a commit to build and test against. |
| `crates/core/src/behavioral/worktree_tests.rs` | 81 |  |
| `crates/core/src/bin/fake_lsp.rs` | 752 | A scripted stand-in for a language server, used only by the transport tests. |
| `crates/core/src/changelists.rs` | 203 | Change groups: named buckets for the files in a working tree. |
| `crates/core/src/changelists_tests.rs` | 344 | Tests for change groups. Included by `changelists.rs` under `#[cfg(test)]`. |
| `crates/core/src/config.rs` | 930 | The workspace configuration file, `.code-basics/config.json`. |
| `crates/core/src/enhancements/enhancements_tests.rs` | 404 | Tests for the instruction-template library. |
| `crates/core/src/enhancements/mod.rs` | 516 | A library of reusable instruction snippets for `CLAUDE.md` / `AGENTS.md`. |
| `crates/core/src/enhancements/runs.rs` | 82 | Per-workspace record of which run-once prompts have already been run. |
| `crates/core/src/enhancements/runs_tests.rs` | 69 | Tests for the per-workspace run-once record. Pure filesystem work over a |
| `crates/core/src/erosion/mod.rs` | 40 | Flagging the changes that quietly weaken a codebase. |
| `crates/core/src/erosion/rules.rs` | 247 | The erosion rule model, its TOML loader, and the built-in set. |
| `crates/core/src/erosion/rules_tests.rs` | 183 | Tests for erosion rule loading and the built-in set. |
| `crates/core/src/erosion/scan.rs` | 135 | Running the rules over a diff. |
| `crates/core/src/erosion/scan_tests.rs` | 304 | Tests for the erosion diff scan. |
| `crates/core/src/files.rs` | 218 | Workspace file access for the directory tree and file editor. |
| `crates/core/src/git/attribution.rs` | 844 | Deciding which recorded edit produced which line of a diff. |
| `crates/core/src/git/attribution_tests.rs` | 1044 | Tests for attributing diff lines to recorded edits. |
| `crates/core/src/git/coverage.rs` | 345 | Auditing the agent's own story about its work. |
| `crates/core/src/git/coverage_tests.rs` | 812 | Tests for the intent-coverage reverse pass and scorecard. |
| `crates/core/src/git/grouping.rs` | 614 | Turning hunks into a handful of decisions. |
| `crates/core/src/git/grouping_tests.rs` | 1040 | Tests for collapsing hunks into cards. |
| `crates/core/src/git/mod.rs` | 34 | Git operations. |
| `crates/core/src/git/patch.rs` | 487 | Building unified diff patches restricted to a selection of lines. |
| `crates/core/src/git/repo.rs` | 1779 | Repository reads and mutations. |
| `crates/core/src/git/why.rs` | 263 | Intent that survives the commit. |
| `crates/core/src/git/why_tests.rs` | 294 | Tests for the pure durable-why build and resolve logic. |
| `crates/core/src/importers/mod.rs` | 7 | Importing configurations from other tools. |
| `crates/core/src/importers/rider.rs` | 516 | Importing JetBrains Rider run configurations. |
| `crates/core/src/importers/rider_tests.rs` | 625 | Tests for the Rider importer. |
| `crates/core/src/inspect/dumps.rs` | 376 | Crash dumps on disk: arming them, recognising them, matching them, pruning |
| `crates/core/src/inspect/dumps_tests.rs` | 297 |  |
| `crates/core/src/inspect/graph.rs` | 225 | Reading what the sidecar wrote, without believing it. |
| `crates/core/src/inspect/graph_tests.rs` | 285 |  |
| `crates/core/src/inspect/inspect_tests.rs` | 472 |  |
| `crates/core/src/inspect/mod.rs` | 124 | Reading the real objects out of a real .NET process. |
| `crates/core/src/inspect/model.rs` | 585 | Types the inspector shares with the frontend. |
| `crates/core/src/inspect/model_tests.rs` | 517 |  |
| `crates/core/src/inspect/session.rs` | 1057 | The decisions that surround one capture. |
| `crates/core/src/inspect/session_tests.rs` | 1525 |  |
| `crates/core/src/inspect/sidecar.rs` | 389 | Deciding how to call the inspector, without calling it. |
| `crates/core/src/inspect/sidecar_tests.rs` | 484 |  |
| `crates/core/src/inspect/tree.rs` | 204 | Shaping the sidecar's flat node list into the tree the UI renders. |
| `crates/core/src/inspect/tree_tests.rs` | 247 |  |
| `crates/core/src/intents/guard.rs` | 220 | The pre-commit guard that keeps rejection notes out of history. |
| `crates/core/src/intents/guard_tests.rs` | 131 | Tests for the pre-commit guard that keeps rejection notes out of history. |
| `crates/core/src/intents/hook.rs` | 772 | Turning a hook payload into a record. |
| `crates/core/src/intents/hook_tests.rs` | 1284 | Tests for ingesting hook payloads. Included by `hook.rs` under `#[cfg(test)]`. |
| `crates/core/src/intents/intents_tests.rs` | 839 | Tests for recorded agent intent. Included by `mod.rs` under `#[cfg(test)]`. |
| `crates/core/src/intents/mod.rs` | 543 | What a coding agent said it was doing, and where it wrote it down. |
| `crates/core/src/intents/patchfmt.rs` | 224 | Reading Codex's patch format. |
| `crates/core/src/intents/patchfmt_tests.rs` | 223 | Tests for Codex patch parsing. Included by `patchfmt.rs` under `#[cfg(test)]`. |
| `crates/core/src/intents/providers/claude_code.rs` | 557 | Claude Code: hooks in `settings.json`, history in per-project transcripts. |
| `crates/core/src/intents/providers/claude_code_tests.rs` | 938 | Tests for the Claude Code transcript reader. |
| `crates/core/src/intents/providers/codex.rs` | 557 | Codex: hooks in `hooks.json`, history in dated rollout files. |
| `crates/core/src/intents/providers/codex_tests.rs` | 1098 | Tests for reading Codex's rollout files and reporting its configuration. |
| `crates/core/src/intents/providers/hooks_json.rs` | 309 | Merging our hooks into a configuration file the user already owns. |
| `crates/core/src/intents/providers/instructions.rs` | 153 | Asking the agent for a reason. |
| `crates/core/src/intents/providers/instructions_tests.rs` | 204 | Tests for the label request appended to an agent's instruction file. |
| `crates/core/src/intents/providers/mod.rs` | 272 | Per-agent knowledge: where it keeps its history, and how to ask it to |
| `crates/core/src/intents/providers/providers_tests.rs` | 835 | Tests for provider detection and hook installation. |
| `crates/core/src/intents/reject.rs` | 457 | Rejecting a change: undoing it and leaving the reason where it was. |
| `crates/core/src/intents/reject_tests.rs` | 371 | Tests for rejecting a change — the marker comment left where code was. |
| `crates/core/src/intents/whyhook.rs` | 166 | The `post-commit` hook that persists durable intent for external commits. |
| `crates/core/src/intents/whyhook_tests.rs` | 83 | Tests for the durable-why post-commit hook installer. |
| `crates/core/src/invocation.rs` | 267 | Turning a run configuration into a command line. |
| `crates/core/src/invocation_tests.rs` | 455 | Tests for dispatching a configuration to the adapter that owns it. |
| `crates/core/src/lib.rs` | 39 | Core logic for `code-basics`. |
| `crates/core/src/lsp/client.rs` | 1233 | One language server, from handshake to shutdown, as questions and answers. |
| `crates/core/src/lsp/client_tests.rs` | 658 | Unit tests for the parts of the client that need no server. |
| `crates/core/src/lsp/documents.rs` | 370 | What each server has been told about each open document. |
| `crates/core/src/lsp/documents_tests.rs` | 598 | Every ordering rule this module claims to obey, one test each. |
| `crates/core/src/lsp/framing.rs` | 202 | The `Content-Length` framing that carries every LSP message. |
| `crates/core/src/lsp/framing_tests.rs` | 234 |  |
| `crates/core/src/lsp/jsonrpc.rs` | 212 | JSON-RPC 2.0, and nothing about LSP. |
| `crates/core/src/lsp/jsonrpc_tests.rs` | 198 |  |
| `crates/core/src/lsp/mod.rs` | 150 | Talking to real language servers, so "find usages" can be right. |
| `crates/core/src/lsp/model.rs` | 300 | The types that cross the IPC boundary for this feature. |
| `crates/core/src/lsp/model_tests.rs` | 428 | The wire shape of everything this subsystem sends the frontend. |
| `crates/core/src/lsp/positions.rs` | 178 | Counting characters and lines the way LSP does, and the way this app does. |
| `crates/core/src/lsp/positions_tests.rs` | 242 |  |
| `crates/core/src/lsp/protocol.rs` | 834 | The LSP messages this app actually uses, and nothing else. |
| `crates/core/src/lsp/protocol_tests.rs` | 1029 |  |
| `crates/core/src/lsp/registry.rs` | 1030 | Which server serves which file, where it is, and what to say when it is absent. |
| `crates/core/src/lsp/registry_tests.rs` | 1270 |  |
| `crates/core/src/lsp/results.rs` | 398 | LSP payloads in, the types the frontend reads out. |
| `crates/core/src/lsp/results_tests.rs` | 670 | Every rule this module claims to obey, one test each. |
| `crates/core/src/lsp/session.rs` | 1623 | Every language server for one workspace, behind one actor. |
| `crates/core/src/lsp/session_tests.rs` | 490 | The session's pure decisions, headlessly. |
| `crates/core/src/lsp/settings.rs` | 159 | The per-workspace language-server configuration block. |
| `crates/core/src/lsp/settings_tests.rs` | 172 |  |
| `crates/core/src/lsp/transport.rs` | 1075 | One live server process, and the bytes going in and out of it. |
| `crates/core/src/lsp/transport_tests.rs` | 752 | Unit tests for the parts of the transport that need no process. |
| `crates/core/src/lsp/uri.rs` | 186 | Paths in, `file:` URIs out, and back again. |
| `crates/core/src/lsp/uri_tests.rs` | 254 |  |
| `crates/core/src/model.rs` | 636 | Types shared between the Rust core and the TypeScript frontend. |
| `crates/core/src/process/chunker.rs` | 188 | Incremental UTF-8 decoding for streamed process output. |
| `crates/core/src/process/kill.rs` | 110 | Platform-specific process *tree* termination. |
| `crates/core/src/process/mod.rs` | 615 | Process supervision: spawn, stream, cancel. |
| `crates/core/src/process/resolve.rs` | 200 | Windows program-name resolution. |
| `crates/core/src/review.rs` | 319 | Assembling the command line for an in-app adversarial review. |
| `crates/core/src/review_tests.rs` | 323 | Tests for [`crate::review`] — the pure, agent-aware command-line assembly for |
| `crates/core/src/secrets.rs` | 574 | .NET user secrets: per-project secrets stored *outside* the repository. |
| `crates/core/src/symbols/cache.rs` | 456 | Persisting a built index so opening a workspace does not re-read every file. |
| `crates/core/src/symbols/cache_tests.rs` | 578 | Tests for persisting a built index and reusing what it can still vouch for. |
| `crates/core/src/symbols/declarations.rs` | 603 | Reading one line of source and deciding whether it declares something. |
| `crates/core/src/symbols/declarations_tests.rs` | 491 | Tests for the declaration heuristic. |
| `crates/core/src/symbols/fuzzy.rs` | 558 | Scoring how well a query matches a candidate symbol name. |
| `crates/core/src/symbols/fuzzy_tests.rs` | 227 | Tests for scoring a query against a candidate name. |
| `crates/core/src/symbols/index.rs` | 755 | Building the symbol index: walking a workspace's source files and recording |
| `crates/core/src/symbols/index_tests.rs` | 813 | Tests for building the index of what a workspace declares. |
| `crates/core/src/symbols/mod.rs` | 51 | Knowing what a workspace declares, and finding it fast. |
| `crates/core/src/symbols/search.rs` | 425 | Querying the index: the layer a symbol palette actually talks to. |
| `crates/core/src/symbols/search_tests.rs` | 362 | Tests for querying the index behind a symbol palette. |
| `crates/core/src/testing/jest_like.rs` | 327 | Parser for the JSON report shared by Jest and Vitest. |
| `crates/core/src/testing/junit.rs` | 349 | Parser for JUnit-style XML test reports. |
| `crates/core/src/testing/mod.rs` | 121 | Test report parsing and result shaping. |
| `crates/core/src/testing/tree.rs` | 296 | Turning a flat list of test cases into the hierarchy the UI renders. |
| `crates/core/src/testing/trx.rs` | 575 | Parser for Visual Studio `.trx` test reports. |
| `crates/core/src/workspace.rs` | 2047 | Scanning a workspace for projects and building the configurations that can |
| `crates/core/tests/behavioral_replay.rs` | 104 | A real-network smoke test for [`cb_core::behavioral::replay`]. |
| `crates/core/tests/durable_why.rs` | 193 | End-to-end tests for durable intent: writing a git note at commit and |
| `crates/core/tests/git_operations.rs` | 1085 | End-to-end git tests against real repositories on disk. |
| `crates/core/tests/intent_attribution.rs` | 280 | Attribution measured against a real repository, rather than a fixture. |
| `crates/core/tests/lsp_client.rs` | 1325 | One language server, from handshake to shutdown, against `cb-fake-lsp`. |
| `crates/core/tests/lsp_oracle.rs` | 838 | The answers a **real** language server gives, for the languages this machine |
| `crates/core/tests/lsp_session.rs` | 962 | Every language server for one workspace, behind one actor, against `cb-fake-lsp`. |
| `crates/core/tests/lsp_transport.rs` | 935 | The transport against a live process, one test per way a server misbehaves. |
| `crates/core/tests/reject_markers.rs` | 359 | Rejection end to end: against a real repository, with the real `git`. |
| `src/App.tsx` | 406 |  |
| `src/components/BranchMenu.tsx` | 386 |  |
| `src/components/ConfigEditor.tsx` | 313 |  |
| `src/components/DiffView.tsx` | 761 |  |
| `src/components/EnvironmentPicker.tsx` | 107 |  |
| `src/components/ErosionPanel.tsx` | 101 |  |
| `src/components/ErrorBoundary.tsx` | 65 |  |
| `src/components/FileEditor.tsx` | 946 |  |
| `src/components/FileTree.tsx` | 118 |  |
| `src/components/IntentPanel.tsx` | 853 |  |
| `src/components/LspStatus.tsx` | 125 |  |
| `src/components/MenuBar.tsx` | 381 |  |
| `src/components/ObjectTree.tsx` | 351 |  |
| `src/components/OutputConsole.tsx` | 428 |  |
| `src/components/ReviewPanel.tsx` | 377 |  |
| `src/components/RiderImportDialog.tsx` | 135 |  |
| `src/components/RunConfigMenu.tsx` | 172 |  |
| `src/components/SearchEverywhere.tsx` | 525 |  |
| `src/components/SecretsEditor.tsx` | 99 |  |
| `src/components/Sidebar.tsx` | 50 |  |
| `src/components/StashPanel.tsx` | 288 |  |
| `src/components/TestTree.tsx` | 125 |  |
| `src/components/behavioralPanelLogic.test.ts` | 200 |  |
| `src/components/behavioralPanelLogic.ts` | 217 |  |
| `src/components/configLogic.test.ts` | 111 |  |
| `src/components/configLogic.ts` | 48 |  |
| `src/components/consoleLogic.test.ts` | 173 |  |
| `src/components/consoleLogic.ts` | 92 |  |
| `src/components/diffLogic.test.ts` | 472 |  |
| `src/components/diffLogic.ts` | 379 |  |
| `src/components/enhancementsLogic.test.ts` | 118 |  |
| `src/components/enhancementsLogic.ts` | 100 |  |
| `src/components/erosionLogic.test.ts` | 78 |  |
| `src/components/erosionLogic.ts` | 65 |  |
| `src/components/intentPanelLogic.test.ts` | 200 |  |
| `src/components/intentPanelLogic.ts` | 405 |  |
| `src/components/language.test.ts` | 114 |  |
| `src/components/language.ts` | 113 |  |
| `src/components/lspStatusLogic.test.ts` | 306 |  |
| `src/components/lspStatusLogic.ts` | 226 |  |
| `src/components/reviewLayoutLogic.test.ts` | 93 |  |
| `src/components/reviewLayoutLogic.ts` | 87 | Pure decisions for positioning the floating agent panel — the clamp |
| `src/components/reviewLogic.test.ts` | 182 |  |
| `src/components/reviewLogic.ts` | 141 | Pure decisions for the adversarial-review panel — extracted so they are |
| `src/components/reviewStreamLogic.test.ts` | 147 |  |
| `src/components/reviewStreamLogic.ts` | 145 | Rendering Claude Code's `--output-format stream-json` NDJSON into readable |
| `src/components/searchLogic.test.ts` | 439 |  |
| `src/components/searchLogic.ts` | 439 |  |
| `src/components/stashLogic.test.ts` | 33 |  |
| `src/components/stashLogic.ts` | 21 |  |
| `src/components/treeLogic.test.ts` | 383 |  |
| `src/components/treeLogic.ts` | 148 |  |
| `src/components/usagesExtension.ts` | 469 |  |
| `src/components/usagesLogic.test.ts` | 1238 |  |
| `src/components/usagesLogic.ts` | 922 |  |
| `src/editorFontSize.ts` | 52 |  |
| `src/editorFontSizeLogic.test.ts` | 114 |  |
| `src/editorFontSizeLogic.ts` | 94 |  |
| `src/ipc/api.ts` | 712 | Typed wrappers over the Tauri command surface. |
| `src/ipc/types.ts` | 1555 |  |
| `src/main.tsx` | 24 |  |
| `src/recentsLogic.test.ts` | 88 |  |
| `src/recentsLogic.ts` | 19 | Workspaces the user has opened before, so reopening is one click. |
| `src/views/ArchitectureView.tsx` | 677 |  |
| `src/views/ChangesView.tsx` | 1060 |  |
| `src/views/HistoryView.tsx` | 556 |  |
| `src/views/InspectView.tsx` | 1062 |  |
| `src/views/RunView.tsx` | 1469 |  |
| `src/views/TestsView.tsx` | 402 |  |
| `src/views/architecture/DiagramCanvas.tsx` | 760 |  |
| `src/views/architecture/DiagramEditor.tsx` | 322 |  |
| `src/views/architecture/architectureLogic.test.ts` | 176 |  |
| `src/views/architecture/architectureLogic.ts` | 258 |  |
| `src/views/architecture/copyLogic.test.ts` | 65 |  |
| `src/views/architecture/copyLogic.ts` | 79 |  |
| `src/views/architecture/emptyStateLogic.test.ts` | 41 |  |
| `src/views/architecture/emptyStateLogic.ts` | 43 |  |
| `src/views/architecture/frontMatterLogic.test.ts` | 86 |  |
| `src/views/architecture/frontMatterLogic.ts` | 91 |  |
| `src/views/architecture/nodeTargets.test.ts` | 390 |  |
| `src/views/architecture/nodeTargets.ts` | 460 |  |
| `src/views/architecture/panZoomLogic.test.ts` | 213 |  |
| `src/views/architecture/panZoomLogic.ts` | 210 |  |
| `src/views/architecture/viewportLogic.test.ts` | 147 |  |
| `src/views/architecture/viewportLogic.ts` | 117 |  |
| `src/views/changesLogic.test.ts` | 157 |  |
| `src/views/changesLogic.ts` | 87 |  |
| `src/views/consolePanelLogic.test.ts` | 178 |  |
| `src/views/consolePanelLogic.ts` | 156 |  |
| `src/views/editorNavLogic.test.ts` | 160 |  |
| `src/views/editorNavLogic.ts` | 120 |  |
| `src/views/historyLogic.test.ts` | 182 |  |
| `src/views/historyLogic.ts` | 109 |  |
| `src/views/inspectLogic.test.ts` | 368 |  |
| `src/views/inspectLogic.ts` | 226 |  |
| `src/views/testsLogic.test.ts` | 279 |  |
| `src/views/testsLogic.ts` | 89 |  |
| `src-tauri/src/commands/architecture.rs` | 219 | Architecture-diagram commands. |
| `src-tauri/src/commands/behavioral.rs` | 484 | Behavioral before/after testing — running the same configuration against |
| `src-tauri/src/commands/changelists.rs` | 57 | Change-group commands. |
| `src-tauri/src/commands/enhancements.rs` | 117 | Instruction-template commands. |
| `src-tauri/src/commands/erosion.rs` | 32 | Erosion-detector command. |
| `src-tauri/src/commands/files.rs` | 245 | Workspace file commands, for the Run tab's directory tree and file editor. |
| `src-tauri/src/commands/git.rs` | 362 | Git commands. |
| `src-tauri/src/commands/inspect.rs` | 350 | Object-inspection commands. |
| `src-tauri/src/commands/intents.rs` | 298 | Agent-intent commands. |
| `src-tauri/src/commands/lsp.rs` | 434 | Asking this workspace's language servers something, and saying what they do. |
| `src-tauri/src/commands/review.rs` | 113 | Launching an in-app adversarial review. |
| `src-tauri/src/commands/run.rs` | 293 | Running applications and tests. |
| `src-tauri/src/commands/secrets.rs` | 43 | .NET user secrets commands. |
| `src-tauri/src/commands/symbols.rs` | 234 | The command palette's surface: searching the workspace, and asking after |
| `src-tauri/src/commands/workspace.rs` | 217 | Workspace and configuration commands. |
| `src-tauri/src/lib.rs` | 191 | The Tauri shell. |
| `src-tauri/src/main.rs` | 6 | Suppress the extra console window on Windows in release builds. |
| `src-tauri/src/recorder.rs` | 136 | The `record-intent` mode, which is what the installed hooks actually run. |
| `src-tauri/src/state.rs` | 1881 | Shared application state. |
| `scripts/build-sidecar.mjs` | 110 |  |
| `scripts/check-docs.mjs` | 66 |  |
| `scripts/generate-index.mjs` | 162 |  |
| `examples/adapters/cargo-nextest.toml` | 34 | Rust tests via cargo-nextest, which can emit JUnit XML. |
| `examples/adapters/pytest.toml` | 35 | A worked example of a declarative adapter. |
| `examples/erosion/custom.toml` | 42 | Example erosion rules. |

## Tauri command surface

Registered in `src-tauri/src/lib.rs`; documented with parameters in [reference/commands.md](reference/commands.md).

- **workspace** (`src-tauri/src/commands/workspace.rs`): `open_workspace`, `current_workspace`, `rescan_workspace`, `save_config`, `delete_config`, `preview_rider_import`, `apply_rider_import`, `launch_profiles`, `set_favorite`, `set_config_order`
- **enhancements** (`src-tauri/src/commands/enhancements.rs`): `list_enhancements`, `add_enhancement`, `remove_enhancement`, `list_prompts`, `agent_runs`, `mark_agent_run`
- **files** (`src-tauri/src/commands/files.rs`): `fs_list_dir`, `fs_read_file`, `fs_write_file`
- **secrets** (`src-tauri/src/commands/secrets.rs`): `read_project_secrets`, `write_project_secrets`
- **run** (`src-tauri/src/commands/run.rs`): `start_run`, `build_project`, `cancel_run`, `running_ids`, `run_tests`, `last_test_run`
- **review** (`src-tauri/src/commands/review.rs`): `start_review`, `cancel_review`, `review_agents`
- **git** (`src-tauri/src/commands/git.rs`): `git_status`, `git_file_diff`, `git_file_contents`, `git_write_file`, `git_stage_file`, `git_unstage_file`, `git_stage_lines`, `git_unstage_lines`, `git_revert_lines`, `git_discard_file`, `git_commit`, `git_branches`, `git_create_branch`, `git_checkout_branch`, `git_checkout_remote_branch`, `git_delete_branch`, `git_merge_branch`, `git_abort_merge`, `git_history`, `git_commit_diff`, `git_commit_file_contents`, `git_commit_file_why`, `git_stash_save`, `git_stash_list`, `git_stash_pop`, `git_stash_apply`, `git_stash_drop`, `git_stash_clear`, `git_network`
- **changelists** (`src-tauri/src/commands/changelists.rs`): `git_changelists`, `git_create_changelist`, `git_delete_changelist`, `git_rename_changelist`, `git_assign_to_changelist`
- **intents** (`src-tauri/src/commands/intents.rs`): `intent_groups`, `stage_intent_group`, `revert_intent_group`, `reject_intent_group`, `intent_capture_status`, `intent_install_plan`, `enable_intent_capture`, `import_intent_history`, `clear_intent_history`
- **behavioral** (`src-tauri/src/commands/behavioral.rs`): `behavioral_diff`, `behavioral_clear`
- **erosion** (`src-tauri/src/commands/erosion.rs`): `erosion_scan`
- **inspect** (`src-tauri/src/commands/inspect.rs`): `inspect_status`, `inspect_attachable`, `inspect_run_dump`, `inspect_capture`, `inspect_last`, `inspect_clear`
- **architecture** (`src-tauri/src/commands/architecture.rs`): `arch_project_graph`, `arch_render_graph`, `arch_component_graph`, `arch_render_component_graph`, `arch_list_diagrams`, `arch_read_diagram`, `arch_write_diagram`, `arch_validate`
- **symbols** (`src-tauri/src/commands/symbols.rs`): `search_everywhere`, `symbol_index_status`, `rebuild_symbol_index`
- **lsp** (`src-tauri/src/commands/lsp.rs`): `lsp_status`, `lsp_open_document`, `lsp_change_document`, `lsp_close_document`, `lsp_find_usages`, `lsp_goto_definition`, `lsp_declaration_anchors`

## Frontend IPC wrappers (`src/ipc/api.ts`)

`openWorkspace`, `currentWorkspace`, `rescanWorkspace`, `saveConfig`, `deleteConfig`, `launchProfiles`, `setFavorite`, `setConfigOrder`, `readProjectSecrets`, `writeProjectSecrets`, `previewRiderImport`, `applyRiderImport`, `fsListDir`, `fsReadFile`, `fsWriteFile`, `listEnhancements`, `addEnhancement`, `removeEnhancement`, `listPrompts`, `agentRuns`, `markAgentRun`, `startRun`, `buildProject`, `cancelRun`, `runningIds`, `runTests`, `lastTestRun`, `startReview`, `cancelReview`, `reviewAgents`, `gitStatus`, `gitFileDiff`, `gitFileContents`, `gitWriteFile`, `gitStageFile`, `gitUnstageFile`, `gitStageLines`, `gitUnstageLines`, `gitRevertLines`, `gitDiscardFile`, `gitCommit`, `gitBranches`, `gitCreateBranch`, `gitCheckoutBranch`, `gitCheckoutRemoteBranch`, `gitDeleteBranch`, `gitMergeBranch`, `gitAbortMerge`, `gitChangelists`, `gitCreateChangelist`, `gitDeleteChangelist`, `gitRenameChangelist`, `gitAssignToChangelist`, `gitHistory`, `gitCommitDiff`, `gitCommitFileContents`, `gitCommitFileWhy`, `gitStashSave`, `gitStashList`, `gitStashPop`, `gitStashApply`, `gitStashDrop`, `gitStashClear`, `gitNetwork`, `intentGroups`, `erosionScan`, `stageIntentGroup`, `revertIntentGroup`, `rejectIntentGroup`, `intentCaptureStatus`, `intentInstallPlan`, `enableIntentCapture`, `importIntentHistory`, `clearIntentHistory`, `behavioralDiff`, `behavioralClear`, `inspectStatus`, `inspectCapture`, `inspectAttachable`, `inspectRunDump`, `inspectLast`, `inspectClear`, `searchEverywhere`, `symbolIndexStatus`, `rebuildSymbolIndex`, `archProjectGraph`, `archRenderGraph`, `archComponentGraph`, `archRenderComponentGraph`, `archListDiagrams`, `archReadDiagram`, `archWriteDiagram`, `archValidate`, `lspStatus`, `lspOpenDocument`, `lspChangeDocument`, `lspCloseDocument`, `lspFindUsages`, `lspGotoDefinition`, `lspDeclarationAnchors`, `errorMessage`

## Public core API (`cb-core`)

- `crates/core/src/adapters/cargo.rs`: `DependencyKind`, `PathDependency`, `CargoManifest`, `is_virtual_manifest()`, `parse()`
- `crates/core/src/adapters/dotnet.rs`: `ProjectFile`, `references()`, `references_prefix()`, `parse_project_file()`, `ConfiguredRunner`, `parse_dotnet_config()`, `is_test_project()`, `classify_runner()`, `has_trx_extension()`, `project_kind()`, `configurations()`, `LaunchProfile`, `is_launchable()`, `parse_launch_settings()`, `split_args()`, `BuildContext`, `test_invocation()`, `run_invocation()`, `BuildAction`, `build_action_invocation()`, `configs_for_project()`
- `crates/core/src/adapters/manifest.rs`: `AdapterManifest`, `CommandTemplate`, `parse()`, `load_dir()`, `matches()`, `matched_file()`, `build_invocation()`, `configs_for_project()`, `manifest_dir()`
- `crates/core/src/adapters/msbuild.rs`: `command_args()`, `parse_output()`, `apply()`, `evaluate()`
- `crates/core/src/adapters/node.rs`: `PackageJson`, `depends_on()`, `parse_package_json()`, `PackageManager`, `program()`, `run_script_args()`, `exec_args()`, `script_arg_separator()`, `detect_package_manager()`, `detect_runner()`, `is_workspace_root()`, `workspace_globs()`, `project_kind()`, `test_invocation()`, `script_invocation()`, `configs_for_project()`, `project_dir()`
- `crates/core/src/adapters/solution.rs`: `Solution`, `SolutionProject`, `is_solution_file()`, `parse()`
- `crates/core/src/architecture/components.rs`: `component_graph()`
- `crates/core/src/architecture/graph.rs`: `ArchKind`, `EdgeKind`, `Derivation`, `ArchNode`, `ArchEdge`, `ArchGraph`, `project_graph()`
- `crates/core/src/architecture/mermaid.rs`: `render()`, `ValidationRule`, `ValidationError`, `validate()`
- `crates/core/src/architecture/signals/dotnet.rs`: `DotnetSignals`, `signals()`, `aspire_class_name()`
- `crates/core/src/architecture/signals/framework.rs`: `Strength`, `ComponentKind`, `slug()`, `Evidence`, `new()`, `elided_value()`, `Signal`, `high()`, `medium()`, `with_detail()`, `Component`, `Usage`, `Detail`, `AdmittedEdge`, `DiscardReason`, `Discarded`, `Admitted`, `edges()`, `warnings()`, `warnings_named()`, `admit()`
- `crates/core/src/architecture/signals/node.rs`: `NodeSignals`, `signals()`
- `crates/core/src/architecture/signals/routes.rs`: `RouteScan`, `route_signals()`
- `crates/core/src/architecture/store.rs`: `DiagramDerivation`, `FrontMatter`, `DiagramFile`, `ParsedDiagram`, `dir()`, `derived_dir()`, `prompts_dir()`, `path()`, `parse()`, `render()`, `list()`, `read()`, `write_authored()`, `write()`
- `crates/core/src/behavioral/attribute.rs`: `attribute_behavioral()`
- `crates/core/src/behavioral/compare.rs`: `CaseTransition`, `CaseDelta`, `TestDelta`, `diff_tests()`
- `crates/core/src/behavioral/console.rs`: `ConsoleNormalization`, `mask_timestamps_and_ids()`, `multiset_minus()`, `diff_console()`, `is_change()`
- `crates/core/src/behavioral/http.rs`: `RecordedResponse`, `is_change()`, `diff_http()`
- `crates/core/src/behavioral/httpfile.rs`: `discover_http_files()`, `HttpRequestSpec`, `Readiness`, `HttpScenario`, `parse_http_file()`
- `crates/core/src/behavioral/mod.rs`: `BehavioralDelta`, `ConsoleDelta`, `HeaderChange`, `BodyDelta`, `HttpDelta`, `CardBehavior`, `BehavioralScorecard`, `BehavioralReport`
- `crates/core/src/behavioral/prepare.rs`: `scan_baseline()`, `find_config()`, `assemble_report()`
- `crates/core/src/behavioral/replay.rs`: `record_from_parts()`, `ready_backoff()`, `send()`, `await_ready()`
- `crates/core/src/behavioral/scenario.rs`: `SideResult`, `unready()`, `ReplayPlan`, `plan_replay()`, `LaunchChoice`, `choose_launch_config()`, `pair_and_diff()`
- `crates/core/src/behavioral/worktree.rs`: `WorktreeOptions`, `BaselineWorktree`, `create()`, `path()`, `head_oid()`, `adopted()`, `keep_for_reuse()`, `finish()`, `teardown()`, `clear_all()`
- `crates/core/src/changelists.rs`: `Changelist`, `Changelists`, `group_of()`, `changelists_path()`, `load()`, `save()`, `create()`, `remove()`, `rename()`, `assign()`
- `crates/core/src/config.rs`: `WorkspaceConfig`, `dump_capture_enabled()`, `inspector_caps()`, `keep_dumps()`, `max_dump_megabytes()`, `config_dir()`, `config_path()`, `results_dir()`, `lsp_log_dir()`, `load()`, `ensure_gitignore()`, `save()`, `merge()`, `apply()`, `sort_configs()`, `set_favorite()`, `set_order()`, `upsert()`, `remove()`
- `crates/core/src/enhancements/mod.rs`: `Placement`, `Template`, `EnhancementInfo`, `PromptInfo`, `begin_marker()`, `end_marker()`, `is_present()`, `parse_template()`, `insert()`, `remove()`, `discover()`, `seed()`, `list()`, `list_prompts()`, `add()`, `remove_from_agents()`, `templates_dir()`, `prompts_dir()`
- `crates/core/src/enhancements/runs.rs`: `PromptRun`, `runs_path()`, `read_runs()`, `record_run()`
- `crates/core/src/erosion/rules.rs`: `ErosionCategory`, `RuleSide`, `ErosionRule`, `CompiledRule`, `parse()`, `load_dir()`, `rules_dir()`, `all_rules()`, `compile()`, `builtin_rules()`
- `crates/core/src/erosion/scan.rs`: `ErosionFlag`, `ErosionReport`, `scan_diffs()`
- `crates/core/src/files.rs`: `DirEntry`, `list_dir()`, `read_file()`, `write_file()`
- `crates/core/src/git/attribution.rs`: `MatchLevel`, `Confidence`, `AttributedSpan`, `HunkAttribution`, `FileAttribution`, `is_empty()`, `Options`, `anchor_key()`, `attribute_file()`, `attribute()`
- `crates/core/src/git/coverage.rs`: `UnfulfilledClaim`, `Scorecard`, `IntentReview`, `review()`
- `crates/core/src/git/grouping.rs`: `GroupKind`, `GroupFile`, `IntentGroup`, `hunk_count()`, `is_formatting_only()`, `enclosing_symbol()`, `group()`
- `crates/core/src/git/patch.rs`: `LineOrigin`, `DiffLine`, `Hunk`, `FileDiff`, `changed_line_indices()`, `hunk_line_indices()`, `Direction`, `build_patch()`
- `crates/core/src/git/repo.rs`: `clear_readonly_directories()`, `clear_readonly_directories()`, `ComparisonMode`, `ChangeKind`, `FileContents`, `FileChange`, `is_conflicted()`, `WorkingStatus`, `Branch`, `Commit`, `StashEntry`, `MergeOutcome`, `MergeReport`, `StageTarget`, `Repo`, `open()`, `workdir()`, `head_oid()`, `hooks_dir()`, `status()`, `file_diff()`, `diff_all()`, `baseline_content()`, `working_content()`, `write_working_file()`, `stage_file()`, `unstage_file()`, `stage_lines()`, `unstage_lines()`, `revert_lines()`, `discard_file()`, `commit()`, `merge_branch()`, `abort_merge()`, `branches()`, `create_branch()`, `checkout_branch()`, `create_branch_from()`, `checkout_remote_branch()`, `delete_branch()`, `history()`, `commit_diff()`, `commit_file_contents()`, `write_why_note()`, `read_why_note()`, `why_for_file()`, `stash_save()`, `stash_list()`, `stash_pop()`, `stash_apply()`, `stash_drop()`, `stash_clear()`, `network_command()`, `NetworkOperation`, `NetworkKind`, `resolve_network()`
- `crates/core/src/git/why.rs`: `WhyEntry`, `FileWhy`, `DurableWhy`, `is_empty()`, `LineIntent`, `content_hash()`, `build_for_commit()`, `resolve_lines()`, `file_in()`, `record_note()`
- `crates/core/src/importers/rider.rs`: `RiderConfiguration`, `parse()`, `expand_macros()`, `convert()`, `resolve_compounds()`, `ImportResult`, `import()`
- `crates/core/src/inspect/dumps.rs`: `dumps_dir()`, `ParsedDumpName`, `dump_env()`, `parse_dump_name()`, `list()`, `newest_for()`, `prune()`, `prune_unnamed()`
- `crates/core/src/inspect/graph.rs`: `RawResult`, `RawNode`, `parse()`, `classify()`, `display_label()`
- `crates/core/src/inspect/mod.rs`: `parse_result_file()`, `parse_result()`
- `crates/core/src/inspect/model.rs`: `Bitness`, `InspectTarget`, `TargetSummary`, `Caps`, `widened()`, `ElidedReason`, `ObjectValue`, `is_expandable()`, `InspectNode`, `InspectGraph`, `RootSpec`, `InspectRequest`, `new()`, `DumpFile`, `DotnetProcess`, `Attribution`, `AttachableProcess`, `AttachableList`, `RunDump`, `InspectStatus`, `InspectorConfig`
- `crates/core/src/inspect/session.rs`: `new_session_id()`, `request_for()`, `attribute()`, `launcher_caveat()`, `live_target_reason()`, `dump_for_run()`, `unsupported_reason()`, `attach_caveats()`, `first_bitness()`, `AttemptOutcome`, `attempt_outcome()`, `other_bitness()`, `retry_bitness()`, `enumeration_outcome()`, `missing_sidecar_reason()`, `status()`, `arm_dumps()`, `ArmedDumps`, `prune()`
- `crates/core/src/inspect/sidecar.rs`: `sessions_dir()`, `session_dir()`, `request_path()`, `result_path()`, `command_args()`, `list_command_args()`, `process_list_path()`, `ProcessList`, `parse_process_list()`, `parse_process_list_file()`, `write_request()`, `sidecar_file_name()`, `FailureCode`, `SidecarFailure`, `failure_of()`, `next_attempt()`, `resolve()`, `retain_newest()`
- `crates/core/src/inspect/tree.rs`: `Built`, `build()`
- `crates/core/src/intents/guard.rs`: `block()`, `hook_path()`, `plan_for()`, `planned_write()`, `is_installed()`, `plan_removal()`, `ensure_executable()`
- `crates/core/src/intents/hook.rs`: `is_record_invocation()`, `RecorderInvocation`, `parse_recorder_args()`, `HookEvent`, `parse()`, `ingest()`, `ask_for_intent()`, `parse_labels_with_source()`, `parse_declared_labels()`, `parse_labels()`, `looks_like_narration()`, `is_usable_inferred_label()`, `is_enabled()`, `resolve_root()`, `resolve_enabled_root()`
- `crates/core/src/intents/mod.rs`: `ProviderId`, `as_str()`, `IntentEdit`, `is_empty()`, `IntentRecord`, `LabelSource`, `IntentLabel`, `IntentPrompt`, `Intents`, `is_empty()`, `label_for()`, `effective_scoped_label()`, `for_path()`, `normalise_path()`, `scope_covers()`, `relative_to()`, `intents_dir()`, `edits_path()`, `labels_path()`, `prompts_path()`, `LoadOptions`, `load()`, `load_prompts()`, `prompt_for()`, `append_edit()`, `append_label()`, `append_prompt()`, `next_seq()`, `rebase_seqs()`, `clear()`
- `crates/core/src/intents/patchfmt.rs`: `PatchedFile`, `parse_envelope()`, `parse_unified_diff()`, `envelope_from_value()`
- `crates/core/src/intents/providers/claude_code.rs`: `ClaudeCode`, `new()`, `with_home()`, `encode_project_dir()`
- `crates/core/src/intents/providers/codex.rs`: `Codex`, `new()`, `codex_home()`, `detected_in()`, `status_in()`, `install_plan_in()`, `history_in()`, `planned_entries()`
- `crates/core/src/intents/providers/hooks_json.rs`: `is_installed()`, `commands_for()`, `pinned_workspace()`, `pinned_elsewhere()`, `pinned_caveat()`, `plan_merge()`, `plan_removal()`
- `crates/core/src/intents/providers/instructions.rs`: `path_for()`, `is_present()`, `planned_write()`
- `crates/core/src/intents/providers/mod.rs`: `InstallScope`, `ProviderStatus`, `absent()`, `PlannedWrite`, `InstallPlan`, `SessionFile`, `HistoryMined`, `apply_plan()`, `apply_writes()`, `all()`, `statuses()`, `history()`, `guard_write()`, `whyhook_write()`, `home_dir()`
- `crates/core/src/intents/reject.rs`: `comment_prefix()`, `sanitise_reason()`, `marker_block()`, `is_marker_line()`, `anchors()`, `insert_markers()`, `iso_date()`, `FileRejection`, `RejectSummary`, `record()`, `reject_file()`
- `crates/core/src/intents/whyhook.rs`: `block()`, `hook_path()`, `plan_for()`, `planned_write()`, `is_installed()`, `ensure_executable()`, `plan_removal()`
- `crates/core/src/invocation.rs`: `build()`, `rerun_filter()`, `plan_compound()`
- `crates/core/src/lsp/client.rs`: `ReadyState`, `is_ready()`, `StartFailure`, `RequestError`, `Client`, `start()`, `start_with_ceiling()`, `spec()`, `root()`, `root_uri()`, `capabilities()`, `pid()`, `death()`, `stderr_tail()`, `readiness()`, `wait_ready()`, `shutdown()`, `did_open()`, `did_change()`, `did_close()`, `document_version()`, `references()`, `definition()`, `implementation()`, `type_definition()`, `document_symbols()`
- `crates/core/src/lsp/documents.rs`: `SyncAction`, `server()`, `path()`, `Documents`, `new()`, `open()`, `change()`, `close()`, `replay_for()`, `text()`, `version()`, `is_open()`, `open_paths()`, `servers_for()`
- `crates/core/src/lsp/framing.rs`: `FramingError`, `encode()`, `Decoder`, `new()`, `with_limits()`, `buffered()`, `push()`
- `crates/core/src/lsp/jsonrpc.rs`: `RequestId`, `RpcError`, `Incoming`, `ProtocolError`, `classify()`, `Outgoing`, `request()`, `notification()`, `reply()`, `error_reply()`
- `crates/core/src/lsp/model.rs`: `Availability`, `UsageResult`, `unavailable()`, `with_server()`, `Usage`, `Highlight`, `DefinitionResult`, `unavailable()`, `Target`, `AnchorResult`, `unavailable()`, `DeclarationAnchor`, `LspStatus`, `ServerStatus`
- `crates/core/src/lsp/positions.rs`: `utf16_to_byte()`, `byte_to_utf16()`, `to_editor_line()`, `to_lsp_line()`, `Snippet`, `snippet()`
- `crates/core/src/lsp/protocol.rs`: `Position`, `Range`, `Location`, `LocationLink`, `DecodeError`, `decode_goto()`, `Symbol`, `decode_document_symbols()`, `symbol_kind()`, `SyncKind`, `ServerCapabilities`, `from_initialize_result()`, `encoding_is_utf16()`, `initialize_params()`, `TextDocumentIdentifier`, `VersionedTextDocumentIdentifier`, `TextDocumentItem`, `DidOpenTextDocumentParams`, `new()`, `TextDocumentContentChangeEvent`, `DidChangeTextDocumentParams`, `whole_document()`, `document_end()`, `DidCloseTextDocumentParams`, `new()`, `TextDocumentPositionParams`, `new()`, `ReferenceContext`, `ReferenceParams`, `new()`, `DocumentSymbolParams`, `new()`
- `crates/core/src/lsp/registry.rs`: `RealProbe`, `Language`, `id()`, `Readiness`, `Timeouts`, `ServerSpec`, `language_id_for()`, `Resolution`, `language_for_extension()`, `resolve()`, `Candidate`, `caller_args()`, `takes_caller_args()`
- `crates/core/src/lsp/results.rs`: `usages()`, `targets()`, `anchors()`
- `crates/core/src/lsp/session.rs`: `LspHandle`, `root()`, `generation()`, `status()`, `request_teardown()`, `open_document()`, `change_document()`, `close_document()`, `find_usages()`, `goto_definition()`, `declaration_anchors()`, `start()`, `start_with_probe()`
- `crates/core/src/lsp/settings.rs`: `LspConfig`, `server()`, `ServerOverride`, `is_disabled()`, `UriStyleSetting`, `style()`
- `crates/core/src/lsp/transport.rs`: `DeathReason`, `Death`, `RequestFailure`, `Launch`, `Notification`, `Transport`, `spawn()`, `pid()`, `death()`, `watch_death()`, `notifications()`, `signal()`, `stderr_tail()`, `allocate_id()`, `is_pending()`, `request()`, `request_with_id()`, `notify()`, `cancel()`, `shutdown()`, `answer_for()`, `RestartDecision`, `RestartPolicy`, `new()`, `with_window()`, `is_failed()`, `on_death()`
- `crates/core/src/lsp/uri.rs`: `UriStyle`, `to_file_uri()`, `from_file_uri()`
- `crates/core/src/model.rs`: `Project`, `ProjectKind`, `TestRunner`, `RunKind`, `ConfigSource`, `RunConfig`, `new()`, `ReportSpec`, `ReportFormat`, `Invocation`, `TestOutcome`, `TestCase`, `TestSummary`, `from_cases()`, `TestRunResult`, `TestNode`
- `crates/core/src/process/chunker.rs`: `Utf8Chunker`, `new()`, `push()`, `finish()`, `LineSplitter`, `new()`, `push()`, `finish()`
- `crates/core/src/process/kill.rs`: `configure_process_group()`, `kill_tree()`
- `crates/core/src/process/mod.rs`: `Stream`, `ProcessEvent`, `Supervisor`, `new()`, `run()`, `cancel()`, `running_ids()`, `pid()`, `running()`, `is_running()`
- `crates/core/src/process/resolve.rs`: `resolve_program()`
- `crates/core/src/review.rs`: `ReviewAgent`, `AgentMode`, `id()`, `from_id()`, `id()`, `label()`, `program()`, `models()`, `default_model()`, `from_id()`, `models_for()`, `codex_models()`, `parse_codex_models()`, `resolve_model()`, `agent_args()`, `detect_agents()`
- `crates/core/src/secrets.rs`: `ProjectSecrets`, `resolve_project_path()`, `secrets_path()`, `user_secrets_id()`, `read()`, `ensure_id()`, `write()`
- `crates/core/src/symbols/cache.rs`: `SymbolCache`, `CachedProject`, `CacheEntry`, `cache_path()`, `load()`, `save()`, `fingerprint()`, `build_cached()`, `rebuild()`
- `crates/core/src/symbols/declarations.rs`: `SymbolKind`, `Declared`, `declaration_name()`, `declaration()`, `is_identifier()`
- `crates/core/src/symbols/fuzzy.rs`: `Match`, `score()`, `rank()`
- `crates/core/src/symbols/index.rs`: `Symbol`, `SymbolIndex`, `SymbolIndexStatus`, `of()`, `Limits`, `build()`, `build_with()`, `index_file()`, `replace_file()`, `relative_to_root()`, `walk()`, `normalise()`
- `crates/core/src/symbols/search.rs`: `SearchScope`, `Query`, `HitKind`, `SearchHit`, `search()`
- `crates/core/src/testing/jest_like.rs`: `parse()`
- `crates/core/src/testing/junit.rs`: `parse()`
- `crates/core/src/testing/mod.rs`: `parse()`, `parse_file()`
- `crates/core/src/testing/tree.rs`: `build()`, `failed_names()`
- `crates/core/src/testing/trx.rs`: `parse()`
- `crates/core/src/workspace.rs`: `Workspace`, `should_skip()`, `source_walker()`, `launch_profiles()`, `ScanOptions`, `scan()`, `workspace_from_dir()`, `scan_with()`, `find_project()`, `dotnet_test_context()`, `configs_by_project()`
