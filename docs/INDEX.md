# Code index

> **Generated** by [`scripts/generate-index.mjs`](../scripts/generate-index.mjs) — do not edit by hand.
> Regenerate with `pnpm docs:index` after adding files, commands, or public APIs.

Use this file to locate things fast: every first-party source file with its one-line purpose, the full Tauri command surface, the frontend IPC wrappers, and the public API of each `cb-core` module.

## Source files

| File | Lines | Purpose |
|------|------:|---------|
| `crates/core/Cargo.toml` | 73 |  |
| `crates/core/build.rs` | 9 |  |
| `crates/core/src/adapters/cargo.rs` | 254 | The Cargo (Rust) ecosystem adapter — manifest reading only. |
| `crates/core/src/adapters/cargo_tests.rs` | 437 | Tests for the Cargo adapter. |
| `crates/core/src/adapters/dotnet.rs` | 1152 | The .NET ecosystem adapter. |
| `crates/core/src/adapters/dotnet_tests.rs` | 1687 | Tests for the .NET adapter. |
| `crates/core/src/adapters/manifest.rs` | 600 | Declarative adapters: adding an ecosystem without writing Rust. |
| `crates/core/src/adapters/mod.rs` | 28 | Ecosystem adapters. |
| `crates/core/src/adapters/msbuild.rs` | 157 | Optional, accurate MSBuild evaluation. |
| `crates/core/src/adapters/msbuild_tests.rs` | 213 | Tests for optional MSBuild evaluation. |
| `crates/core/src/adapters/node.rs` | 433 | The JavaScript / TypeScript ecosystem adapter. |
| `crates/core/src/adapters/node_tests.rs` | 489 | Tests for the JS/TS adapter. |
| `crates/core/src/adapters/solution.rs` | 316 | Reading .NET solution files. |
| `crates/core/src/adapters/solution_tests.rs` | 139 | Tests for solution parsing. Included by `solution.rs` under `#[cfg(test)]`. |
| `crates/core/src/architecture/components.rs` | 469 | The component map: which services this workspace runs, which data stores |
| `crates/core/src/architecture/components_tests.rs` | 997 | Tests for [`super::components`]. |
| `crates/core/src/architecture/graph.rs` | 1782 | The derived project graph: nodes, edges, and the rules that produce them. |
| `crates/core/src/architecture/graph_tests.rs` | 2112 | Tests for [`super::graph`]. |
| `crates/core/src/architecture/mermaid.rs` | 1332 | Rendering an [`ArchGraph`](super::graph::ArchGraph) to Mermaid source, and |
| `crates/core/src/architecture/mermaid_tests.rs` | 1534 | Tests for [`super::mermaid`]. |
| `crates/core/src/architecture/mod.rs` | 77 | The derived project graph: what this workspace is made of, and what points |
| `crates/core/src/architecture/signals/dotnet.rs` | 1387 | Component signals read out of .NET manifests, configuration and source. |
| `crates/core/src/architecture/signals/dotnet_tests.rs` | 1202 | Tests for [`super::dotnet`]. |
| `crates/core/src/architecture/signals/framework.rs` | 925 | The signal types and [`admit`] — the one gate every candidate component in |
| `crates/core/src/architecture/signals/framework_tests.rs` | 832 | Tests for [`super::framework`]. |
| `crates/core/src/architecture/signals/mod.rs` | 88 | Components that are not projects: the databases, caches, queues and HTTP |
| `crates/core/src/architecture/signals/node.rs` | 1379 | Component signals read out of `package.json`, out of the framework |
| `crates/core/src/architecture/signals/node_tests.rs` | 1357 | Tests for [`super::node`]. |
| `crates/core/src/architecture/signals/routes.rs` | 1613 | Signals about the routes an ASP.NET service exposes. |
| `crates/core/src/architecture/signals/routes_tests.rs` | 886 | Tests for [`super::routes`]. |
| `crates/core/src/architecture/store.rs` | 596 | Diagrams on disk: where they live, how they say where they came from, and |
| `crates/core/src/architecture/store_tests.rs` | 631 | Tests for [`super::store`]. |
| `crates/core/src/behavioral/attribute.rs` | 127 | Mapping observable deltas back to the intent card that plausibly caused them. |
| `crates/core/src/behavioral/attribute_tests.rs` | 141 |  |
| `crates/core/src/behavioral/compare.rs` | 144 | Diffing two [`TestRunResult`]s by test-case outcome. |
| `crates/core/src/behavioral/compare_tests.rs` | 126 |  |
| `crates/core/src/behavioral/console.rs` | 251 | Diffing the console output of the two runs. |
| `crates/core/src/behavioral/console_tests.rs` | 121 |  |
| `crates/core/src/behavioral/http.rs` | 218 | Diffing two HTTP responses for one replayed request. |
| `crates/core/src/behavioral/http_tests.rs` | 111 |  |
| `crates/core/src/behavioral/httpfile.rs` | 321 | Parsing `.http` / `.rest` scenario files (VS Code REST Client / JetBrains |
| `crates/core/src/behavioral/httpfile_tests.rs` | 145 |  |
| `crates/core/src/behavioral/mod.rs` | 161 | Behavioral before/after testing — the *runtime* counterpart to the static |
| `crates/core/src/behavioral/mod_tests.rs` | 180 | Key-pinning for the behavioral wire types. |
| `crates/core/src/behavioral/prepare.rs` | 120 | The testable decision seam between the raw deltas and the wire report. |
| `crates/core/src/behavioral/prepare_tests.rs` | 121 | Tests for the pure report-assembly seam. |
| `crates/core/src/behavioral/replay.rs` | 151 | The one impure corner of behavioral testing: firing HTTP requests. |
| `crates/core/src/behavioral/replay_tests.rs` | 58 | Socket-free tests for the two decisions replay makes without the network. |
| `crates/core/src/behavioral/scenario.rs` | 205 | The pure decision seam for serverful HTTP replay. |
| `crates/core/src/behavioral/scenario_tests.rs` | 204 |  |
| `crates/core/src/behavioral/worktree.rs` | 329 | An on-demand, isolated checkout of a commit to build and test against. |
| `crates/core/src/behavioral/worktree_tests.rs` | 135 |  |
| `crates/core/src/bin/fake_lsp.rs` | 779 | A scripted stand-in for a language server, used only by the transport tests. |
| `crates/core/src/browser/argv.rs` | 78 | Is this process the browser MCP server, or the application? |
| `crates/core/src/browser/argv_tests.rs` | 98 |  |
| `crates/core/src/browser/consent.rs` | 333 | Whether an agent may read, or move, the page the user is looking at. |
| `crates/core/src/browser/consent_tests.rs` | 422 | Who may read and drive the page. Included by `consent.rs`. |
| `crates/core/src/browser/console.rs` | 67 | Which level a captured console message carries. |
| `crates/core/src/browser/console_tests.rs` | 124 | Console level ranking, and the abstention. Included by `console.rs`. |
| `crates/core/src/browser/framing.rs` | 69 | The wire format of the browser control pipe — **reused, not rewritten**. |
| `crates/core/src/browser/framing_tests.rs` | 95 | That the reuse is real and behaves as the browser pipe needs. Included by |
| `crates/core/src/browser/install.rs` | 247 | Writing the browser MCP server into an agent's configuration. |
| `crates/core/src/browser/install_tests.rs` | 339 |  |
| `crates/core/src/browser/instances.rs` | 451 | Which running application an MCP browser client should talk to — and the |
| `crates/core/src/browser/instances_tests.rs` | 477 |  |
| `crates/core/src/browser/ipc.rs` | 334 | Every byte the page sends back. **This is the attack surface.** |
| `crates/core/src/browser/ipc_tests.rs` | 379 | The hostile-input boundary. Included by `ipc.rs`. |
| `crates/core/src/browser/liveness.rs` | 39 | The one platform seam of the browser instance registry: is this entry's |
| `crates/core/src/browser/liveness_tests.rs` | 95 |  |
| `crates/core/src/browser/mod.rs` | 105 | The embedded browser: every decision it makes, none of the machinery it |
| `crates/core/src/browser/model.rs` | 309 | The browser types that cross IPC, and the distinctions they exist to keep. |
| `crates/core/src/browser/model_tests.rs` | 341 | Key-pinning for the browser wire types, and the distinctions they exist for. |
| `crates/core/src/browser/origin.rs` | 240 | Where the page may never go, and what counts as "the same page". |
| `crates/core/src/browser/origin_tests.rs` | 262 | Navigation refusals and origin comparison. Included by `origin.rs`. |
| `crates/core/src/browser/render.rs` | 453 | Turning what the application knows into the words an agent reads. |
| `crates/core/src/browser/render_tests.rs` | 553 |  |
| `crates/core/src/browser/ring.rs` | 211 | A bounded log that says what it lost. |
| `crates/core/src/browser/ring_tests.rs` | 281 | What a bounded log lost, and how a reader learns it. Included by `ring.rs`. |
| `crates/core/src/browser/script.rs` | 488 | The scripts injected into the page, and how their arguments are encoded. |
| `crates/core/src/browser/script_tests.rs` | 452 | Script assembly: the try/catch wrapper and the encoding. Included by `script.rs`. |
| `crates/core/src/browser/serve.rs` | 170 | The browser MCP server's handshake, and the shape of every answer it sends. |
| `crates/core/src/browser/serve_tests.rs` | 240 |  |
| `crates/core/src/browser/text.rs` | 56 | Cutting a page's text down to a readable size, and admitting it. |
| `crates/core/src/browser/text_tests.rs` | 123 | Truncation that reports the total. Included by `text.rs`. |
| `crates/core/src/browser/tools.rs` | 485 | The thirteen browser tools, their schemas, and how a call is read. |
| `crates/core/src/browser/tools_tests.rs` | 402 |  |
| `crates/core/src/browser/url.rs` | 273 | What the URL bar accepts, and — mostly — what it refuses. |
| `crates/core/src/browser/url_tests.rs` | 276 | What the URL bar accepts and refuses. Included by `url.rs`. |
| `crates/core/src/browser/wire.rs` | 309 | What travels on the browser control pipe, in both directions. |
| `crates/core/src/browser/wire_tests.rs` | 292 |  |
| `crates/core/src/changelists.rs` | 203 | Change groups: named buckets for the files in a working tree. |
| `crates/core/src/changelists_tests.rs` | 344 | Tests for change groups. Included by `changelists.rs` under `#[cfg(test)]`. |
| `crates/core/src/config.rs` | 1075 | The workspace configuration file, `.code-basics/config.json`. |
| `crates/core/src/dap/breakpoints.rs` | 268 | The breakpoint model: what the user asked for, what the adapter agreed to, |
| `crates/core/src/dap/breakpoints_tests.rs` | 292 | Tests for the breakpoint model. Included by `breakpoints.rs`. |
| `crates/core/src/dap/coalesce.rs` | 126 | Merging a burst of debuggee output into as few console messages as possible. |
| `crates/core/src/dap/coalesce_tests.rs` | 131 | Tests for [`super`]: what may be merged into one console message, and what |
| `crates/core/src/dap/mod.rs` | 57 | Debugging: breakpoints, stepping and a call stack, over the Debug Adapter |
| `crates/core/src/dap/model.rs` | 199 | The debug-session types that cross IPC, and the distinctions they exist to |
| `crates/core/src/dap/model_tests.rs` | 247 | Key-pinning for the debug wire types, and the state distinctions. |
| `crates/core/src/dap/positions.rs` | 75 | Line and column conventions, and the one thing this app refuses to let an |
| `crates/core/src/dap/positions_tests.rs` | 82 | Tests for the line/column conventions. Included by `positions.rs`. |
| `crates/core/src/dap/protocol.rs` | 285 | The Debug Adapter Protocol's base messages, and the handful of requests, |
| `crates/core/src/dap/protocol_tests.rs` | 292 | Tests for the DAP base protocol. Included by `protocol.rs` under `#[cfg(test)]`. |
| `crates/core/src/dap/registry.rs` | 311 | Finding a debug adapter on this machine — and saying precisely what was |
| `crates/core/src/dap/registry_tests.rs` | 407 | Tests for adapter discovery. Included by `registry.rs`. |
| `crates/core/src/dap/sequence.rs` | 170 | Sequence numbers and the request/response correlation built on them. |
| `crates/core/src/dap/sequence_tests.rs` | 199 | Tests for sequencing and correlation. Included by `sequence.rs`. |
| `crates/core/src/editor_context/answer.rs` | 107 | Every answer this server can give that is not data — and they are all |
| `crates/core/src/editor_context/answer_tests.rs` | 58 |  |
| `crates/core/src/editor_context/argv.rs` | 89 | Is this process the editor-context MCP server, or the application? |
| `crates/core/src/editor_context/argv_tests.rs` | 131 |  |
| `crates/core/src/editor_context/install.rs` | 239 | Writing the editor-context MCP server into an agent's configuration. |
| `crates/core/src/editor_context/install_tests.rs` | 314 |  |
| `crates/core/src/editor_context/instances.rs` | 424 | Which running application an editor-context MCP client should talk to — and |
| `crates/core/src/editor_context/instances_tests.rs` | 417 |  |
| `crates/core/src/editor_context/liveness.rs` | 40 | The one platform seam of the editor instance registry: is this entry's |
| `crates/core/src/editor_context/liveness_tests.rs` | 75 |  |
| `crates/core/src/editor_context/mod.rs` | 65 | The editor-context MCP server: the app's live editor state, exposed to a |
| `crates/core/src/editor_context/render.rs` | 167 | Turning the pushed editor state into the words an agent reads. |
| `crates/core/src/editor_context/render_tests.rs` | 278 |  |
| `crates/core/src/editor_context/serve.rs` | 127 | The editor-context MCP server's handshake, and the shape of every answer it |
| `crates/core/src/editor_context/serve_tests.rs` | 140 |  |
| `crates/core/src/editor_context/tools.rs` | 157 | The four editor-context tools, their schemas, and how a call is read. |
| `crates/core/src/editor_context/tools_tests.rs` | 148 |  |
| `crates/core/src/editor_context/wire.rs` | 295 | What travels on the editor-context control pipe, in both directions. |
| `crates/core/src/editor_context/wire_tests.rs` | 259 |  |
| `crates/core/src/enhancements/enhancements_tests.rs` | 465 | Tests for the instruction-template library. |
| `crates/core/src/enhancements/mod.rs` | 579 | A library of reusable instruction snippets for `CLAUDE.md` / `AGENTS.md`. |
| `crates/core/src/enhancements/runs.rs` | 82 | Per-workspace record of which run-once prompts have already been run. |
| `crates/core/src/enhancements/runs_tests.rs` | 69 | Tests for the per-workspace run-once record. Pure filesystem work over a |
| `crates/core/src/erosion/mod.rs` | 40 | Flagging the changes that quietly weaken a codebase. |
| `crates/core/src/erosion/rules.rs` | 592 | The erosion rule model, its TOML loader, and the built-in set. |
| `crates/core/src/erosion/rules_tests.rs` | 198 | Tests for erosion rule loading and the built-in set. |
| `crates/core/src/erosion/scan.rs` | 368 | Running the rules over a diff. |
| `crates/core/src/erosion/scan_tests.rs` | 507 | Tests for the erosion diff scan. |
| `crates/core/src/features/features_tests.rs` | 127 |  |
| `crates/core/src/features/mod.rs` | 231 | Optional features: which of the app's non-core capabilities are switched on. |
| `crates/core/src/features/store.rs` | 198 | Reading and writing `code-basics/features.json`, and the installer seed. |
| `crates/core/src/features/store_tests.rs` | 573 |  |
| `crates/core/src/files.rs` | 432 | Workspace file access for the directory tree and file editor. |
| `crates/core/src/git/attribution.rs` | 844 | Deciding which recorded edit produced which line of a diff. |
| `crates/core/src/git/attribution_tests.rs` | 1045 | Tests for attributing diff lines to recorded edits. |
| `crates/core/src/git/coverage.rs` | 355 | Auditing the agent's own story about its work. |
| `crates/core/src/git/coverage_tests.rs` | 962 | Tests for the intent-coverage reverse pass and scorecard. |
| `crates/core/src/git/grouping.rs` | 1036 | Turning hunks into a handful of decisions. |
| `crates/core/src/git/grouping_tests.rs` | 1616 | Tests for collapsing hunks into cards. |
| `crates/core/src/git/mod.rs` | 35 | Git operations. |
| `crates/core/src/git/patch.rs` | 487 | Building unified diff patches restricted to a selection of lines. |
| `crates/core/src/git/repo.rs` | 2285 | Repository reads and mutations. |
| `crates/core/src/git/why.rs` | 260 | Intent that survives the commit. |
| `crates/core/src/git/why_tests.rs` | 324 | Tests for the pure durable-why build and resolve logic. |
| `crates/core/src/git/worktree.rs` | 165 | Creating a **user** git worktree on a new branch, to open in its own project |
| `crates/core/src/git/worktree_tests.rs` | 56 |  |
| `crates/core/src/importers/mod.rs` | 7 | Importing configurations from other tools. |
| `crates/core/src/importers/rider.rs` | 522 | Importing JetBrains Rider run configurations. |
| `crates/core/src/importers/rider_tests.rs` | 679 | Tests for the Rider importer. |
| `crates/core/src/inspect/dumps.rs` | 376 | Crash dumps on disk: arming them, recognising them, matching them, pruning |
| `crates/core/src/inspect/dumps_tests.rs` | 297 |  |
| `crates/core/src/inspect/graph.rs` | 230 | Reading what the sidecar wrote, without believing it. |
| `crates/core/src/inspect/graph_tests.rs` | 294 |  |
| `crates/core/src/inspect/inspect_tests.rs` | 518 |  |
| `crates/core/src/inspect/mod.rs` | 124 | Reading the real objects out of a real .NET process. |
| `crates/core/src/inspect/model.rs` | 596 | Types the inspector shares with the frontend. |
| `crates/core/src/inspect/model_tests.rs` | 524 |  |
| `crates/core/src/inspect/session.rs` | 1127 | The decisions that surround one capture. |
| `crates/core/src/inspect/session_tests.rs` | 1608 |  |
| `crates/core/src/inspect/sidecar.rs` | 392 | Deciding how to call the inspector, without calling it. |
| `crates/core/src/inspect/sidecar_tests.rs` | 484 |  |
| `crates/core/src/inspect/tree.rs` | 204 | Shaping the sidecar's flat node list into the tree the UI renders. |
| `crates/core/src/inspect/tree_tests.rs` | 247 |  |
| `crates/core/src/intents/guard.rs` | 220 | The pre-commit guard that keeps rejection notes out of history. |
| `crates/core/src/intents/guard_tests.rs` | 131 | Tests for the pre-commit guard that keeps rejection notes out of history. |
| `crates/core/src/intents/hook.rs` | 872 | Turning a hook payload into a record. |
| `crates/core/src/intents/hook_tests.rs` | 1441 | Tests for ingesting hook payloads. Included by `hook.rs` under `#[cfg(test)]`. |
| `crates/core/src/intents/intents_tests.rs` | 1099 | Tests for recorded agent intent. Included by `mod.rs` under `#[cfg(test)]`. |
| `crates/core/src/intents/mod.rs` | 804 | What a coding agent said it was doing, and where it wrote it down. |
| `crates/core/src/intents/patchfmt.rs` | 224 | Reading Codex's patch format. |
| `crates/core/src/intents/patchfmt_tests.rs` | 223 | Tests for Codex patch parsing. Included by `patchfmt.rs` under `#[cfg(test)]`. |
| `crates/core/src/intents/providers/claude_code.rs` | 700 | Claude Code: hooks in `settings.json`, history in per-project transcripts. |
| `crates/core/src/intents/providers/claude_code_tests.rs` | 1188 | Tests for the Claude Code transcript reader. |
| `crates/core/src/intents/providers/codex.rs` | 556 | Codex: hooks in `hooks.json`, history in dated rollout files. |
| `crates/core/src/intents/providers/codex_tests.rs` | 1100 | Tests for reading Codex's rollout files and reporting its configuration. |
| `crates/core/src/intents/providers/hooks_json.rs` | 198 | Merging our hooks into a configuration file the user already owns. |
| `crates/core/src/intents/providers/instructions.rs` | 162 | Asking the agent for a reason. |
| `crates/core/src/intents/providers/instructions_tests.rs` | 204 | Tests for the label request appended to an agent's instruction file. |
| `crates/core/src/intents/providers/mod.rs` | 426 | Per-agent knowledge: where it keeps its history, and how to ask it to |
| `crates/core/src/intents/providers/providers_tests.rs` | 1094 | Tests for provider detection and hook installation. |
| `crates/core/src/intents/providers/settings_merge.rs` | 158 | Generic marker-based merging of hook entries into a `settings.json`-shaped |
| `crates/core/src/intents/reject.rs` | 572 | Rejecting a change: undoing it and leaving the reason where it was. |
| `crates/core/src/intents/reject_tests.rs` | 483 | Tests for rejecting a change — the marker comment left where code was. |
| `crates/core/src/intents/retire.rs` | 750 | Retiring the intents a commit has absorbed. |
| `crates/core/src/intents/retire_tests.rs` | 262 | Tests for retiring intents a commit has absorbed. Included by `retire.rs` |
| `crates/core/src/intents/user.rs` | 401 | User-authored intents: a note the user writes on a card when no agent hook |
| `crates/core/src/intents/user_tests.rs` | 497 |  |
| `crates/core/src/intents/whyhook.rs` | 166 | The `post-commit` hook that persists durable intent for external commits. |
| `crates/core/src/intents/whyhook_tests.rs` | 86 | Tests for the durable-why post-commit hook installer. |
| `crates/core/src/invocation.rs` | 371 | Turning a run configuration into a command line. |
| `crates/core/src/invocation_tests.rs` | 531 | Tests for dispatching a configuration to the adapter that owns it. |
| `crates/core/src/launcher/mod.rs` | 41 | The app launcher: arbitrary command lines the user wants to run beside the |
| `crates/core/src/launcher/model.rs` | 109 | What a launchable is, and the shape of the file that stores them. |
| `crates/core/src/launcher/model_tests.rs` | 107 |  |
| `crates/core/src/launcher/parse.rs` | 140 | Turning a typed command line into a program and argv — or refusing to. |
| `crates/core/src/launcher/parse_tests.rs` | 99 |  |
| `crates/core/src/launcher/recents.rs` | 239 | The recents policy: what running a command remembers, what it forgets, and |
| `crates/core/src/launcher/recents_tests.rs` | 343 |  |
| `crates/core/src/launcher/store.rs` | 90 | The launchers file on disk: `<config>/code-basics/launchers.json`. |
| `crates/core/src/launcher/store_tests.rs` | 93 |  |
| `crates/core/src/lib.rs` | 56 | Core logic for `code-basics`. |
| `crates/core/src/lsp/client.rs` | 1404 | One language server, from handshake to shutdown, as questions and answers. |
| `crates/core/src/lsp/client_tests.rs` | 658 | Unit tests for the parts of the client that need no server. |
| `crates/core/src/lsp/documents.rs` | 370 | What each server has been told about each open document. |
| `crates/core/src/lsp/documents_tests.rs` | 598 | Every ordering rule this module claims to obey, one test each. |
| `crates/core/src/lsp/edits.rs` | 279 | Applying a server's [`TextEdit`]s to a document, or refusing to. |
| `crates/core/src/lsp/edits_tests.rs` | 344 |  |
| `crates/core/src/lsp/framing.rs` | 202 | The `Content-Length` framing that carries every LSP message. |
| `crates/core/src/lsp/framing_tests.rs` | 234 |  |
| `crates/core/src/lsp/jsonrpc.rs` | 212 | JSON-RPC 2.0, and nothing about LSP. |
| `crates/core/src/lsp/jsonrpc_tests.rs` | 198 |  |
| `crates/core/src/lsp/mod.rs` | 174 | Talking to real language servers, so "find usages" can be right. |
| `crates/core/src/lsp/model.rs` | 781 | The types that cross the IPC boundary for this feature. |
| `crates/core/src/lsp/model_tests.rs` | 921 | The wire shape of everything this subsystem sends the frontend. |
| `crates/core/src/lsp/positions.rs` | 231 | Counting characters and lines the way LSP does, and the way this app does. |
| `crates/core/src/lsp/positions_tests.rs` | 342 |  |
| `crates/core/src/lsp/protocol.rs` | 1544 | The LSP messages this app actually uses, and nothing else. |
| `crates/core/src/lsp/protocol_tests.rs` | 2035 |  |
| `crates/core/src/lsp/registry.rs` | 1061 | Which server serves which file, where it is, and what to say when it is absent. |
| `crates/core/src/lsp/registry_tests.rs` | 1325 |  |
| `crates/core/src/lsp/rename.rs` | 639 | Turning a server's `WorkspaceEdit` into files on disk, or refusing to. |
| `crates/core/src/lsp/rename_tests.rs` | 929 | The two phases, every refusal, and the rollback. |
| `crates/core/src/lsp/results.rs` | 546 | LSP payloads in, the types the frontend reads out. |
| `crates/core/src/lsp/results_tests.rs` | 822 | Every rule this module claims to obey, one test each. |
| `crates/core/src/lsp/session.rs` | 2358 | Every language server for one workspace, behind one actor. |
| `crates/core/src/lsp/session_tests.rs` | 629 | The session's pure decisions, headlessly. |
| `crates/core/src/lsp/settings.rs` | 159 | The per-workspace language-server configuration block. |
| `crates/core/src/lsp/settings_tests.rs` | 172 |  |
| `crates/core/src/lsp/transport.rs` | 1087 | One live server process, and the bytes going in and out of it. |
| `crates/core/src/lsp/transport_tests.rs` | 752 | Unit tests for the parts of the transport that need no process. |
| `crates/core/src/lsp/uri.rs` | 186 | Paths in, `file:` URIs out, and back again. |
| `crates/core/src/lsp/uri_tests.rs` | 254 |  |
| `crates/core/src/mcp/answer.rs` | 393 | Every answer this server can give that is not data — and they are all |
| `crates/core/src/mcp/answer_tests.rs` | 252 |  |
| `crates/core/src/mcp/argv.rs` | 67 | Is this process an MCP server, or the application? |
| `crates/core/src/mcp/argv_tests.rs` | 113 |  |
| `crates/core/src/mcp/execute.rs` | 103 | Forcing read-only, in the one place a statement becomes a plan. |
| `crates/core/src/mcp/execute_tests.rs` | 145 |  |
| `crates/core/src/mcp/expose.rs` | 272 | Which saved connections an agent may see, and what it is told about them. |
| `crates/core/src/mcp/expose_tests.rs` | 422 |  |
| `crates/core/src/mcp/install/codex_toml.rs` | 176 | Adding one `[mcp_servers.<name>]` entry to `$CODEX_HOME/config.toml`, |
| `crates/core/src/mcp/install/codex_toml_tests.rs` | 140 |  |
| `crates/core/src/mcp/install/mcp_merge.rs` | 202 | Marker-free merging of one named server into a JSON config file's server map. |
| `crates/core/src/mcp/install/mcp_merge_tests.rs` | 290 |  |
| `crates/core/src/mcp/install/mod.rs` | 178 | Installing the SQL MCP server into an agent's configuration — previewed |
| `crates/core/src/mcp/install/mod_tests.rs` | 86 |  |
| `crates/core/src/mcp/install/plan.rs` | 233 | The install, the uninstall, the status and the caveats — mirroring |
| `crates/core/src/mcp/install/plan_tests.rs` | 316 |  |
| `crates/core/src/mcp/liveness.rs` | 70 | Is a registry entry's process still the executable it recorded? |
| `crates/core/src/mcp/liveness_tests.rs` | 58 |  |
| `crates/core/src/mcp/mod.rs` | 72 | The SQL MCP server: what an agent may ask this app's databases, and what it |
| `crates/core/src/mcp/ndjson.rs` | 148 | Line-delimited JSON framing: the MCP stdio wire. |
| `crates/core/src/mcp/ndjson_tests.rs` | 144 |  |
| `crates/core/src/mcp/render.rs` | 283 | Turning an answer into the words an agent reads. |
| `crates/core/src/mcp/render_tests.rs` | 366 |  |
| `crates/core/src/mcp/serve.rs` | 275 | What the server answers, decided without touching a database. |
| `crates/core/src/mcp/serve_tests.rs` | 343 |  |
| `crates/core/src/mcp/tools.rs` | 323 | The six tools, their schemas, and how a call is read. |
| `crates/core/src/mcp/tools_tests.rs` | 254 |  |
| `crates/core/src/model.rs` | 875 | Types shared between the Rust core and the TypeScript frontend. |
| `crates/core/src/notes.rs` | 148 | The global notes / scratchpad store, `code-basics/notes.json`. |
| `crates/core/src/notes_tests.rs` | 183 |  |
| `crates/core/src/process/chunker.rs` | 188 | Incremental UTF-8 decoding for streamed process output. |
| `crates/core/src/process/kill.rs` | 239 | Platform-specific process *tree* termination. |
| `crates/core/src/process/mod.rs` | 802 | Process supervision: spawn, stream, cancel. |
| `crates/core/src/process/resolve.rs` | 200 | Windows program-name resolution. |
| `crates/core/src/pty/argv.rs` | 145 | Refusing arguments a Windows batch shim would re-interpret. |
| `crates/core/src/pty/argv_tests.rs` | 267 |  |
| `crates/core/src/pty/mod.rs` | 328 | Interactive pseudo-terminal sessions. |
| `crates/core/src/pty/model.rs` | 209 | The wire types for an interactive terminal session. |
| `crates/core/src/pty/shell.rs` | 366 | Pure decisions for the PTY layer: which shell to launch, and clamping the |
| `crates/core/src/pty/shell_tests.rs` | 357 |  |
| `crates/core/src/qgate/decide_tests.rs` | 454 | Decision-logic tests for the quality gate — the Rust port of the behaviour |
| `crates/core/src/qgate/install.rs` | 329 | Installing the quality-gate Stop hook — the same method the intent hooks use. |
| `crates/core/src/qgate/install_tests.rs` | 310 | Installer tests for the quality-gate hook. The behaviour that matters most |
| `crates/core/src/qgate/mod.rs` | 502 | The quality-gate Stop hook: deterministic checks when an agent turn ends. |
| `crates/core/src/redis/discover.rs` | 301 | Finding the Redis connections a workspace already talks about. |
| `crates/core/src/redis/discover_tests.rs` | 103 | Tests for [`super::discover`]. Each builds a real temp workspace and runs the |
| `crates/core/src/redis/driver.rs` | 399 | The one module in the `redis/` subtree that opens a socket. |
| `crates/core/src/redis/driver_tests.rs` | 67 | Only the pure helpers are tested here — the connection methods need a live |
| `crates/core/src/redis/dsn.rs` | 272 | Parsing and redacting a Redis connection string. |
| `crates/core/src/redis/dsn_tests.rs` | 95 |  |
| `crates/core/src/redis/mcp/answer.rs` | 193 | Every non-data answer the Redis MCP server can give, each distinct. |
| `crates/core/src/redis/mcp/argv.rs` | 79 | Is this process the Redis MCP server, or the application? |
| `crates/core/src/redis/mcp/execute.rs` | 156 | Turning a parsed tool call into something runnable — and the consent gate. |
| `crates/core/src/redis/mcp/expose.rs` | 161 | Which saved Redis connections an agent may see, and what it is told. |
| `crates/core/src/redis/mcp/expose_tests.rs` | 97 |  |
| `crates/core/src/redis/mcp/mod.rs` | 16 | The write-capable Redis MCP server, self-dispatched as `cb-app.exe mcp-redis`. |
| `crates/core/src/redis/mcp/render.rs` | 218 | Turning an answer into the text an agent reads. No decisions here beyond |
| `crates/core/src/redis/mcp/serve.rs` | 140 | The handshake and the answers that need no connection. |
| `crates/core/src/redis/mcp/tools.rs` | 552 | The Redis tools, their schemas, and how a call is read. |
| `crates/core/src/redis/mcp/tools_tests.rs` | 97 |  |
| `crates/core/src/redis/mod.rs` | 31 | The Redis plugin's decision layers, modelled on [`crate::sql`]. |
| `crates/core/src/redis/model.rs` | 160 | The wire model the Redis panel and the MCP server share. |
| `crates/core/src/redis/model_tests.rs` | 104 |  |
| `crates/core/src/redis/ops.rs` | 148 | The Redis operations, split read from write, and the **one** place the |
| `crates/core/src/redis/ops_tests.rs` | 35 |  |
| `crates/core/src/redis/store.rs` | 226 | Saved Redis connection profiles: `<config>/code-basics/redis-connections.json`. |
| `crates/core/src/redis/store_tests.rs` | 118 |  |
| `crates/core/src/review.rs` | 429 | Assembling the command line for an in-app adversarial review. |
| `crates/core/src/review_tests.rs` | 573 | Tests for [`crate::review`] — the pure, agent-aware command-line assembly for |
| `crates/core/src/roslyn/answer.rs` | 159 | Every answer this server can give that is not data — and they are all |
| `crates/core/src/roslyn/answer_tests.rs` | 119 |  |
| `crates/core/src/roslyn/argv.rs` | 88 | Is this process the Roslyn MCP server, or the application? |
| `crates/core/src/roslyn/argv_tests.rs` | 128 |  |
| `crates/core/src/roslyn/install.rs` | 238 | Writing the Roslyn MCP server into an agent's configuration. |
| `crates/core/src/roslyn/install_tests.rs` | 314 |  |
| `crates/core/src/roslyn/instances.rs` | 425 | Which running application an MCP Roslyn client should talk to — and the five |
| `crates/core/src/roslyn/instances_tests.rs` | 417 |  |
| `crates/core/src/roslyn/liveness.rs` | 37 | The one platform seam of the Roslyn instance registry: is this entry's |
| `crates/core/src/roslyn/liveness_tests.rs` | 75 |  |
| `crates/core/src/roslyn/mod.rs` | 66 | The Roslyn/LSP MCP server: the app's warm semantic model, exposed to a coding |
| `crates/core/src/roslyn/render.rs` | 270 | Turning an LSP result into the words an agent reads. |
| `crates/core/src/roslyn/render_tests.rs` | 346 |  |
| `crates/core/src/roslyn/serve.rs` | 127 | The Roslyn MCP server's handshake, and the shape of every answer it sends. |
| `crates/core/src/roslyn/serve_tests.rs` | 139 |  |
| `crates/core/src/roslyn/symbol.rs` | 131 | Resolving a `{symbol}` target to a position, using a file's declaration |
| `crates/core/src/roslyn/symbol_tests.rs` | 96 | Tests for resolving a `{symbol}` target against a file's declaration anchors. |
| `crates/core/src/roslyn/tools.rs` | 382 | The four Roslyn tools, their schemas, and how a call is read. |
| `crates/core/src/roslyn/tools_tests.rs` | 377 |  |
| `crates/core/src/roslyn/wire.rs` | 297 | What travels on the Roslyn control pipe, in both directions. |
| `crates/core/src/roslyn/wire_tests.rs` | 259 |  |
| `crates/core/src/rules/mod.rs` | 149 | Business-rule invariants, authored as markdown and injected as review |
| `crates/core/src/rules/rules_tests.rs` | 138 | Tests for rule-doc parsing and loading. |
| `crates/core/src/running/classify.rs` | 79 | The pure decision behind orphan detection: given records reloaded from disk |
| `crates/core/src/running/classify_tests.rs` | 89 |  |
| `crates/core/src/running/mod.rs` | 59 | The running-process registry behind the Running panel. |
| `crates/core/src/running/probe.rs` | 30 | The one platform seam of the running registry: ask the OS whether a pid is |
| `crates/core/src/running/record.rs` | 100 | The record of one process the app has spawned, and the on-disk file format. |
| `crates/core/src/running/record_tests.rs` | 92 |  |
| `crates/core/src/running/store.rs` | 251 | The registry itself: a clone-cheap handle over the live process set and the |
| `crates/core/src/running/store_tests.rs` | 139 |  |
| `crates/core/src/secrets.rs` | 755 | .NET user secrets: per-project secrets stored *outside* the repository. |
| `crates/core/src/setup.rs` | 104 | First-open setup: one combined plan that installs the agent hooks a fresh |
| `crates/core/src/setup_tests.rs` | 197 | Tests for the combined first-open setup plan. The point that matters: the |
| `crates/core/src/sql/catalog.rs` | 347 | The object explorer's catalog queries, and the rows they come back as. |
| `crates/core/src/sql/catalog_tests.rs` | 330 |  |
| `crates/core/src/sql/discover.rs` | 938 | Finding the database connections a workspace already talks about. |
| `crates/core/src/sql/discover_tests.rs` | 773 | Tests for [`super::discover`]. |
| `crates/core/src/sql/dotenv.rs` | 447 | Reading connection strings out of environment files (`.env` and friends) and |
| `crates/core/src/sql/dotenv_tests.rs` | 300 | Tests for [`super`]: the shapes a `.env` line comes in, and the two ways one |
| `crates/core/src/sql/driver/caps.rs` | 268 | The two ceilings a result set is read under, and the arithmetic that decides |
| `crates/core/src/sql/driver/caps_tests.rs` | 294 |  |
| `crates/core/src/sql/driver/mod.rs` | 544 | `sql/driver/` is the **only** module in this subtree that opens a socket or a |
| `crates/core/src/sql/driver/mod_tests.rs` | 416 |  |
| `crates/core/src/sql/driver/mssql.rs` | 1122 | The SQL Server driver, over `tiberius`. |
| `crates/core/src/sql/driver/mssql_tests.rs` | 730 |  |
| `crates/core/src/sql/driver/postgres.rs` | 1339 | The PostgreSQL driver, over `sqlx`. |
| `crates/core/src/sql/driver/postgres_tests.rs` | 922 |  |
| `crates/core/src/sql/driver/sqlite.rs` | 607 | The SQLite driver, over `sqlx`. |
| `crates/core/src/sql/driver/sqlite_tests.rs` | 291 |  |
| `crates/core/src/sql/driver/value.rs` | 41 | The one-to-one relabelling from what [`crate::sql::format`] renders to what |
| `crates/core/src/sql/driver/value_tests.rs` | 84 |  |
| `crates/core/src/sql/dsn.rs` | 854 | Parsing a connection string into the parts a driver needs, and — just as |
| `crates/core/src/sql/dsn_tests.rs` | 397 |  |
| `crates/core/src/sql/format.rs` | 259 | Rendering database values as display strings for the results grid. |
| `crates/core/src/sql/format_tests.rs` | 187 | Tests for [`super`]: the six ways a cell can be wrong on the way to the grid. |
| `crates/core/src/sql/guard.rs` | 654 | The read-only guard: parse a statement with `sqlparser` and decide whether it |
| `crates/core/src/sql/guard_tests.rs` | 541 |  |
| `crates/core/src/sql/mod.rs` | 58 | The SQL console: connect to a database the workspace already talks to, run a |
| `crates/core/src/sql/model.rs` | 307 | Wire types for the SQL console: the request to run a query, the result set |
| `crates/core/src/sql/model_tests.rs` | 585 | The wire shape of everything the SQL console sends the frontend. |
| `crates/core/src/sql/plan.rs` | 164 | Whether a statement may be sent, and with what. |
| `crates/core/src/sql/plan_tests.rs` | 175 |  |
| `crates/core/src/sql/session.rs` | 187 | The registry of in-flight statements, and the handles that stop them. |
| `crates/core/src/sql/session_tests.rs` | 200 |  |
| `crates/core/src/sql/store.rs` | 299 | Saved connection profiles: `<config>/code-basics/sql-connections.json`. |
| `crates/core/src/sql/store_tests.rs` | 545 |  |
| `crates/core/src/symbols/cache.rs` | 456 | Persisting a built index so opening a workspace does not re-read every file. |
| `crates/core/src/symbols/cache_tests.rs` | 578 | Tests for persisting a built index and reusing what it can still vouch for. |
| `crates/core/src/symbols/declarations.rs` | 603 | Reading one line of source and deciding whether it declares something. |
| `crates/core/src/symbols/declarations_tests.rs` | 491 | Tests for the declaration heuristic. |
| `crates/core/src/symbols/fuzzy.rs` | 558 | Scoring how well a query matches a candidate symbol name. |
| `crates/core/src/symbols/fuzzy_tests.rs` | 227 | Tests for scoring a query against a candidate name. |
| `crates/core/src/symbols/index.rs` | 792 | Building the symbol index: walking a workspace's source files and recording |
| `crates/core/src/symbols/index_tests.rs` | 936 | Tests for building the index of what a workspace declares. |
| `crates/core/src/symbols/mod.rs` | 51 | Knowing what a workspace declares, and finding it fast. |
| `crates/core/src/symbols/search.rs` | 499 | Querying the index: the layer a symbol palette actually talks to. |
| `crates/core/src/symbols/search_tests.rs` | 381 | Tests for querying the index behind a symbol palette. |
| `crates/core/src/tasks/mcp/answer.rs` | 74 | Every answer this server can give that is not data — and they are all |
| `crates/core/src/tasks/mcp/answer_tests.rs` | 40 |  |
| `crates/core/src/tasks/mcp/argv.rs` | 68 | Is this process the Tasks MCP server, or the application? |
| `crates/core/src/tasks/mcp/argv_tests.rs` | 79 |  |
| `crates/core/src/tasks/mcp/execute.rs` | 113 | Applying one tool call to the task store. |
| `crates/core/src/tasks/mcp/execute_tests.rs` | 268 |  |
| `crates/core/src/tasks/mcp/install.rs` | 235 | Writing the Tasks MCP server into an agent's configuration. |
| `crates/core/src/tasks/mcp/install_tests.rs` | 313 |  |
| `crates/core/src/tasks/mcp/mod.rs` | 62 | The Tasks MCP server: what a coding agent may do to this workspace's task |
| `crates/core/src/tasks/mcp/render.rs` | 77 | Turning an outcome into the words an agent reads. |
| `crates/core/src/tasks/mcp/render_tests.rs` | 89 |  |
| `crates/core/src/tasks/mcp/serve.rs` | 180 | The handshake, the routing, and the response envelopes. |
| `crates/core/src/tasks/mcp/serve_tests.rs` | 118 |  |
| `crates/core/src/tasks/mcp/tools.rs` | 331 | The seven tools, their schemas, and how a call is read. |
| `crates/core/src/tasks/mcp/tools_tests.rs` | 229 |  |
| `crates/core/src/tasks/mod.rs` | 169 | The per-workspace task store, `.code-basics/tasks.json`. |
| `crates/core/src/tasks/store.rs` | 76 | Persistence for the per-workspace task store: path resolution plus the |
| `crates/core/src/tasks/tasks_tests.rs` | 246 |  |
| `crates/core/src/testing/changecov.rs` | 169 | Mapping a coverage report onto the current diff — "coverage of change". |
| `crates/core/src/testing/changecov_tests.rs` | 139 | Tests for mapping coverage onto the diff. |
| `crates/core/src/testing/coverage.rs` | 280 | Parsing code-coverage reports into a per-file line→hit map. |
| `crates/core/src/testing/coverage_tests.rs` | 147 | Tests for the coverage-report parsers. |
| `crates/core/src/testing/jest_like.rs` | 327 | Parser for the JSON report shared by Jest and Vitest. |
| `crates/core/src/testing/junit.rs` | 349 | Parser for JUnit-style XML test reports. |
| `crates/core/src/testing/mod.rs` | 123 | Test report parsing and result shaping. |
| `crates/core/src/testing/tree.rs` | 296 | Turning a flat list of test cases into the hierarchy the UI renders. |
| `crates/core/src/testing/trx.rs` | 575 | Parser for Visual Studio `.trx` test reports. |
| `crates/core/src/tool_gate/mod.rs` | 253 | Per-tool enable/disable for the four MCP servers. |
| `crates/core/src/tool_gate/store.rs` | 97 | Reading and writing `code-basics/mcp-tools.json`. |
| `crates/core/src/tool_gate/store_tests.rs` | 100 |  |
| `crates/core/src/tool_gate/tool_gate_tests.rs` | 162 |  |
| `crates/core/src/workspace.rs` | 2088 | Scanning a workspace for projects and building the configurations that can |
| `crates/core/tests/behavioral_replay.rs` | 104 | A real-network smoke test for [`cb_core::behavioral::replay`]. |
| `crates/core/tests/durable_why.rs` | 202 | End-to-end tests for durable intent: writing a git note at commit and |
| `crates/core/tests/git_operations.rs` | 1481 | End-to-end git tests against real repositories on disk. |
| `crates/core/tests/intent_attribution.rs` | 282 | Attribution measured against a real repository, rather than a fixture. |
| `crates/core/tests/intent_retirement.rs` | 358 | End-to-end tests for retiring intents a commit has absorbed. |
| `crates/core/tests/lsp_client.rs` | 1664 | One language server, from handshake to shutdown, against `cb-fake-lsp`. |
| `crates/core/tests/lsp_oracle.rs` | 838 | The answers a **real** language server gives, for the languages this machine |
| `crates/core/tests/lsp_session.rs` | 1480 | Every language server for one workspace, behind one actor, against `cb-fake-lsp`. |
| `crates/core/tests/lsp_transport.rs` | 935 | The transport against a live process, one test per way a server misbehaves. |
| `crates/core/tests/pty_roundtrip.rs` | 260 | End-to-end PTY test: prove a real pseudo-terminal round-trips stdin to |
| `crates/core/tests/reject_markers.rs` | 422 | Rejection end to end: against a real repository, with the real `git`. |
| `crates/core/tests/sql_mssql.rs` | 733 | The SQL Server half of the SQL console, against a real server. |
| `crates/core/tests/sql_postgres.rs` | 811 | The PostgreSQL half of the SQL subsystem's end-to-end proof. |
| `crates/core/tests/sql_sqlite.rs` | 1073 | The first test in the SQL subsystem that touches a real database. |
| `src/App.tsx` | 1477 |  |
| `src/appearance.ts` | 87 |  |
| `src/appearanceLogic.test.ts` | 89 |  |
| `src/appearanceLogic.ts` | 196 |  |
| `src/components/AboutDialog.tsx` | 138 |  |
| `src/components/AppOutputPanel.tsx` | 237 |  |
| `src/components/AskPanel.tsx` | 265 |  |
| `src/components/AttachConfirm.tsx` | 63 |  |
| `src/components/BehavioralPanel.tsx` | 412 |  |
| `src/components/BranchMenu.tsx` | 500 |  |
| `src/components/BrowserMcpPanel.tsx` | 227 |  |
| `src/components/BrowserPanel.tsx` | 832 |  |
| `src/components/ConfigEditor.tsx` | 313 |  |
| `src/components/ContextMenu.tsx` | 117 |  |
| `src/components/DiffPane.tsx` | 441 |  |
| `src/components/DiffView.tsx` | 1078 |  |
| `src/components/Dock.tsx` | 48 | The shared minimized-window dock: one fixed strip that renders every |
| `src/components/DockContext.tsx` | 75 | The React plumbing for the shared minimized-window dock. All *decisions* live |
| `src/components/DockableEditorSlot.tsx` | 72 |  |
| `src/components/DockableTerminal.tsx` | 84 |  |
| `src/components/EditorMcpPanel.tsx` | 224 |  |
| `src/components/EnvironmentPicker.tsx` | 107 |  |
| `src/components/ErosionPanel.tsx` | 101 |  |
| `src/components/ErrorBoundary.tsx` | 65 |  |
| `src/components/FeaturesPicker.tsx` | 96 |  |
| `src/components/FileEditor.tsx` | 1691 |  |
| `src/components/FileIcon.tsx` | 108 | The file-type icon beside a name in the file tree and the Changes list. |
| `src/components/FileTree.tsx` | 515 |  |
| `src/components/IntentPanel.tsx` | 1530 |  |
| `src/components/LauncherPicker.tsx` | 308 |  |
| `src/components/LspStatus.tsx` | 184 |  |
| `src/components/McpServerPanel.tsx` | 216 |  |
| `src/components/McpToolsPage.tsx` | 171 |  |
| `src/components/MenuBar.tsx` | 437 |  |
| `src/components/NotesPanel.tsx` | 413 |  |
| `src/components/NotificationHost.tsx` | 79 |  |
| `src/components/ObjectTree.tsx` | 357 |  |
| `src/components/OutputConsole.tsx` | 487 |  |
| `src/components/PillColorMenu.tsx` | 84 |  |
| `src/components/PlanPreview.tsx` | 60 |  |
| `src/components/RedisPanel.tsx` | 585 |  |
| `src/components/RegionContext.tsx` | 277 |  |
| `src/components/RegionDropOverlay.tsx` | 34 |  |
| `src/components/RegionHost.tsx` | 154 |  |
| `src/components/RegionSplitter.tsx` | 53 |  |
| `src/components/ReviewPanel.tsx` | 499 |  |
| `src/components/RiderImportDialog.tsx` | 135 |  |
| `src/components/RoslynMcpPanel.tsx` | 223 |  |
| `src/components/RunConfigMenu.tsx` | 183 |  |
| `src/components/RunningPanel.tsx` | 248 |  |
| `src/components/SearchEverywhere.tsx` | 497 |  |
| `src/components/SettingsDialog.tsx` | 251 |  |
| `src/components/SetupPrompt.tsx` | 115 |  |
| `src/components/Sidebar.tsx` | 50 |  |
| `src/components/SqlConnectionPicker.tsx` | 624 |  |
| `src/components/SqlPanel.tsx` | 255 |  |
| `src/components/SqlResultGrid.tsx` | 217 |  |
| `src/components/StablePortal.tsx` | 49 |  |
| `src/components/StashPanel.tsx` | 288 |  |
| `src/components/TasksPanel.tsx` | 370 |  |
| `src/components/TerminalPanel.tsx` | 615 |  |
| `src/components/TerminalView.tsx` | 178 |  |
| `src/components/TestTree.tsx` | 125 |  |
| `src/components/WorkspaceTab.tsx` | 1123 |  |
| `src/components/aboutLogic.test.ts` | 134 |  |
| `src/components/aboutLogic.ts` | 140 | The string composition behind the About dialog. |
| `src/components/appOutputLogic.test.ts` | 279 |  |
| `src/components/appOutputLogic.ts` | 222 | Pure decisions for the launched-apps output panel — the tab list, which tab |
| `src/components/askLogic.test.ts` | 254 |  |
| `src/components/askLogic.ts` | 294 | The "Ask the codebase" modal's decisions, with no React and no DOM in sight. |
| `src/components/behavioralPanelLogic.test.ts` | 425 |  |
| `src/components/behavioralPanelLogic.ts` | 451 |  |
| `src/components/branchFilterLogic.test.ts` | 126 |  |
| `src/components/branchFilterLogic.ts` | 88 |  |
| `src/components/browserPanelLogic.test.ts` | 672 |  |
| `src/components/browserPanelLogic.ts` | 564 | Pure decisions for the floating browser panel: when it is mounted, when a |
| `src/components/claimChecklistLogic.test.ts` | 147 |  |
| `src/components/claimChecklistLogic.ts` | 106 |  |
| `src/components/claimVerifyLogic.test.ts` | 312 |  |
| `src/components/claimVerifyLogic.ts` | 177 |  |
| `src/components/confidenceLogic.test.ts` | 145 |  |
| `src/components/confidenceLogic.ts` | 75 |  |
| `src/components/configLogic.test.ts` | 185 |  |
| `src/components/configLogic.ts` | 99 |  |
| `src/components/consoleLogic.test.ts` | 314 |  |
| `src/components/consoleLogic.ts` | 213 | Pure decisions for the output console: colouring a chunk of process output, |
| `src/components/coverageOfChangeLogic.test.ts` | 134 |  |
| `src/components/coverageOfChangeLogic.ts` | 103 |  |
| `src/components/diffLogic.test.ts` | 501 |  |
| `src/components/diffLogic.ts` | 401 |  |
| `src/components/diffPaneLogic.test.ts` | 253 |  |
| `src/components/diffPaneLogic.ts` | 150 |  |
| `src/components/dockLogic.test.ts` | 97 |  |
| `src/components/dockLogic.ts` | 118 | Pure decisions for the shared minimized-window **dock** — the single strip |
| `src/components/editorContextLogic.test.ts` | 232 |  |
| `src/components/editorContextLogic.ts` | 227 | Decision logic for the **editor context** pushed to the backend, where the |
| `src/components/editorSourceLogic.test.ts` | 199 |  |
| `src/components/editorSourceLogic.ts` | 196 | What backs an open editor tab. |
| `src/components/enhancementsLogic.test.ts` | 118 |  |
| `src/components/enhancementsLogic.ts` | 100 | Pure decisions behind the Enhancements menu, kept out of the rendering shell |
| `src/components/erosionLogic.test.ts` | 78 |  |
| `src/components/erosionLogic.ts` | 69 |  |
| `src/components/featuresLogic.test.ts` | 130 |  |
| `src/components/featuresLogic.ts` | 75 | Decision logic for optional features: what is switched on, and what that hides. |
| `src/components/fileIconLogic.test.ts` | 129 |  |
| `src/components/fileIconLogic.ts` | 159 | Which file-type icon a row shows. |
| `src/components/fileTreeLogic.test.ts` | 159 |  |
| `src/components/fileTreeLogic.ts` | 117 | Pure decisions for the Run tab's file tree — where a new file lands, whether |
| `src/components/fileTreeRevealLogic.test.ts` | 91 |  |
| `src/components/fileTreeRevealLogic.ts` | 76 | Pure decisions behind the file tree's "select opened file" action: which |
| `src/components/focusOrderContext.tsx` | 83 | The React plumbing for the app-wide floating-panel focus order. All |
| `src/components/focusOrderLogic.test.ts` | 90 |  |
| `src/components/focusOrderLogic.ts` | 71 | Pure decisions for the app-wide floating-panel focus order: which panel is in |
| `src/components/intentPanelLogic.test.ts` | 644 |  |
| `src/components/intentPanelLogic.ts` | 559 |  |
| `src/components/language.test.ts` | 122 |  |
| `src/components/language.ts` | 119 |  |
| `src/components/launcherLogic.test.ts` | 199 |  |
| `src/components/launcherLogic.ts` | 163 | Pure decisions for the app launcher's picker — whether a command line needs |
| `src/components/lspStatusLogic.test.ts` | 409 |  |
| `src/components/lspStatusLogic.ts` | 300 |  |
| `src/components/mcpServerLogic.test.ts` | 204 |  |
| `src/components/mcpServerLogic.ts` | 191 | Decision logic for the SQL MCP server installer panel. |
| `src/components/mcpToolsLogic.test.ts` | 75 |  |
| `src/components/mcpToolsLogic.ts` | 63 | Decision logic for the MCP settings page: which servers and tools exist, and |
| `src/components/notesLogic.test.ts` | 219 |  |
| `src/components/notesLogic.ts` | 178 | Pure decisions for the Notes / scratchpad panel — creating, renaming, |
| `src/components/notificationLogic.test.ts` | 169 |  |
| `src/components/notificationLogic.ts` | 161 | The app's notification service: what deserves a notification, what it says, |
| `src/components/occlusionContext.tsx` | 84 | Tells the embedded browser panel when some DOM surface is on screen that the |
| `src/components/pluginMenuLogic.test.ts` | 298 |  |
| `src/components/pluginMenuLogic.ts` | 224 | The **Plugins** titlebar menu: what the optional features offer, and whether |
| `src/components/projectViewLogic.test.ts` | 171 |  |
| `src/components/projectViewLogic.ts` | 222 | Pure decisions for the merged **Project** tab — the view that puts the old |
| `src/components/redisLogic.test.ts` | 107 |  |
| `src/components/redisLogic.ts` | 98 | Pure decisions for the Redis panel: how a connection is labelled, how scan |
| `src/components/redisPanelLogic.ts` | 32 | Pure decisions for the floating Redis panel: open/close and the restore token. |
| `src/components/regionLayoutLogic.test.ts` | 205 |  |
| `src/components/regionLayoutLogic.ts` | 300 | Pure decisions for **split docking** — dragging an editor, diff or terminal |
| `src/components/renameLogic.test.ts` | 709 |  |
| `src/components/renameLogic.ts` | 756 | Every decision F2 makes, kept out of the editor so it can be tested. |
| `src/components/reviewLayoutLogic.test.ts` | 215 |  |
| `src/components/reviewLayoutLogic.ts` | 197 | Pure decisions for positioning the floating agent panel — the clamp |
| `src/components/reviewLogic.test.ts` | 182 |  |
| `src/components/reviewLogic.ts` | 141 | Pure decisions for the adversarial-review panel — extracted so they are |
| `src/components/reviewStreamLogic.test.ts` | 147 |  |
| `src/components/reviewStreamLogic.ts` | 145 | Rendering Claude Code's `--output-format stream-json` NDJSON into readable |
| `src/components/riskLogic.test.ts` | 216 |  |
| `src/components/riskLogic.ts` | 189 |  |
| `src/components/runningLogic.test.ts` | 239 |  |
| `src/components/runningLogic.ts` | 226 | Pure decisions for the Running panel — the per-kind label/icon, the codebase |
| `src/components/searchLogic.test.ts` | 439 |  |
| `src/components/searchLogic.ts` | 439 | The search palette's decisions, with no React and no DOM in sight. |
| `src/components/setupPromptLogic.test.ts` | 110 |  |
| `src/components/setupPromptLogic.ts` | 62 | Decision logic for the first-open setup prompt. Pure so it can be unit-tested |
| `src/components/sqlPanelLogic.test.ts` | 99 |  |
| `src/components/sqlPanelLogic.ts` | 86 | Pure decisions for the floating SQL console panel: when it is mounted, when a |
| `src/components/sqlPickerLogic.test.ts` | 354 |  |
| `src/components/sqlPickerLogic.ts` | 242 |  |
| `src/components/stashLogic.test.ts` | 33 |  |
| `src/components/stashLogic.ts` | 21 |  |
| `src/components/tasksLogic.test.ts` | 109 |  |
| `src/components/tasksLogic.ts` | 86 | Pure decisions for the Tasks panel — building the agent prompt, toggling a |
| `src/components/tasksPanelLogic.test.ts` | 97 |  |
| `src/components/tasksPanelLogic.ts` | 87 | Pure decisions for the floating Tasks panel: when it is mounted, when a fresh |
| `src/components/terminalLogic.test.ts` | 474 |  |
| `src/components/terminalLogic.ts` | 332 | Pure decisions for the floating terminals — naming, cascade staggering, |
| `src/components/terminalMenuLogic.test.ts` | 350 |  |
| `src/components/terminalMenuLogic.ts` | 343 | Pure decisions for the titlebar's terminal split button — which rows its |
| `src/components/terminalShellLogic.test.ts` | 162 |  |
| `src/components/terminalShellLogic.ts` | 129 | The remembered choice of shell for new terminals: how it is stored, and — |
| `src/components/treeLogic.test.ts` | 383 |  |
| `src/components/treeLogic.ts` | 148 |  |
| `src/components/usagesExtension.ts` | 469 | The CodeMirror half of Find Usages / Go To Definition: decorations, a widget, |
| `src/components/usagesLogic.test.ts` | 1238 |  |
| `src/components/usagesLogic.ts` | 922 | Every decision the usages UI makes, with no React, no DOM and no CodeMirror. |
| `src/components/workspaceRenameLogic.test.ts` | 246 |  |
| `src/components/workspaceRenameLogic.ts` | 222 | Custom, user-chosen labels for the open-codebase tabs. |
| `src/components/workspaceTabsLogic.test.ts` | 324 |  |
| `src/components/workspaceTabsLogic.ts` | 321 | Pure decisions for the top-level workspace tab strip — adding an open |
| `src/editorFontSize.ts` | 52 | Applying the editor font size, and telling live editors it moved. |
| `src/editorFontSizeLogic.test.ts` | 114 |  |
| `src/editorFontSizeLogic.ts` | 94 | The one font size every CodeMirror editor in the app reads. |
| `src/ipc/api.ts` | 1782 | Typed wrappers over the Tauri command surface. |
| `src/ipc/channelLogic.test.ts` | 74 |  |
| `src/ipc/channelLogic.ts` | 53 |  |
| `src/ipc/types.ts` | 3039 | TypeScript mirrors of the `cb-core` model types. |
| `src/main.tsx` | 24 |  |
| `src/recentsLogic.test.ts` | 88 |  |
| `src/recentsLogic.ts` | 19 | Workspaces the user has opened before, so reopening is one click. |
| `src/reexportGuards.test.ts` | 23 | / <reference types="vite/client" /> |
| `src/shortcutLogic.test.ts` | 133 |  |
| `src/shortcutLogic.ts` | 349 |  |
| `src/shortcuts.ts` | 78 |  |
| `src/views/ArchitectureView.tsx` | 679 |  |
| `src/views/ChangesView.tsx` | 1062 |  |
| `src/views/HistoryView.tsx` | 542 |  |
| `src/views/InspectView.tsx` | 1086 |  |
| `src/views/RunView.tsx` | 2462 |  |
| `src/views/SqlView.tsx` | 1069 |  |
| `src/views/TestsView.tsx` | 426 |  |
| `src/views/architecture/DiagramCanvas.tsx` | 760 |  |
| `src/views/architecture/DiagramEditor.tsx` | 324 |  |
| `src/views/architecture/architectureLogic.test.ts` | 176 |  |
| `src/views/architecture/architectureLogic.ts` | 258 | The Architecture tab's list of diagrams, and the two labels that sit beside |
| `src/views/architecture/copyLogic.test.ts` | 65 |  |
| `src/views/architecture/copyLogic.ts` | 79 | The one decision the "save a copy" button makes: what the copy is called, |
| `src/views/architecture/emptyStateLogic.test.ts` | 41 |  |
| `src/views/architecture/emptyStateLogic.ts` | 43 | Which of the two empty answers a derived graph is giving. |
| `src/views/architecture/frontMatterLogic.test.ts` | 86 |  |
| `src/views/architecture/frontMatterLogic.ts` | 91 |  |
| `src/views/architecture/nodeTargets.test.ts` | 407 |  |
| `src/views/architecture/nodeTargets.ts` | 460 | Where clicking a box in a diagram takes you — or, more often than any other |
| `src/views/architecture/panZoomLogic.test.ts` | 213 |  |
| `src/views/architecture/panZoomLogic.ts` | 210 | Panning and zooming a rendered diagram, as arithmetic. |
| `src/views/architecture/viewportLogic.test.ts` | 147 |  |
| `src/views/architecture/viewportLogic.ts` | 117 | Where the user had panned and zoomed each diagram to, kept across a remount. |
| `src/views/changesLogic.test.ts` | 274 |  |
| `src/views/changesLogic.ts` | 155 |  |
| `src/views/changesSelectionLogic.test.ts` | 160 |  |
| `src/views/changesSelectionLogic.ts` | 142 |  |
| `src/views/consolePanelLogic.test.ts` | 228 |  |
| `src/views/consolePanelLogic.ts` | 205 | Whether the Run tab's console panel is put away, and how tall it is when it |
| `src/views/debugLogic.test.ts` | 154 |  |
| `src/views/debugLogic.ts` | 144 | Decision logic for the Run tab's Debug control: whether a configuration can |
| `src/views/editorNavLogic.test.ts` | 160 |  |
| `src/views/editorNavLogic.ts` | 120 | The pure decisions behind the Run tab editor's navigation history and its |
| `src/views/folderTreeLogic.test.ts` | 148 |  |
| `src/views/folderTreeLogic.ts` | 143 |  |
| `src/views/historyLogic.test.ts` | 182 |  |
| `src/views/historyLogic.ts` | 109 |  |
| `src/views/inspectLogic.test.ts` | 415 |  |
| `src/views/inspectLogic.ts` | 267 |  |
| `src/views/runControlLogic.test.ts` | 41 |  |
| `src/views/runControlLogic.ts` | 23 | Decision logic for the bulk stop controls (Run tab "Stop All", Tests tab |
| `src/views/sqlLogic.test.ts` | 655 |  |
| `src/views/sqlLogic.ts` | 671 | Pure decisions for the SQL console — the streaming reducer, the cell |
| `src/views/sqlViewLogic.test.ts` | 464 |  |
| `src/views/sqlViewLogic.ts` | 527 | The decisions `SqlView` would otherwise make inline. |
| `src/views/testsLogic.test.ts` | 279 |  |
| `src/views/testsLogic.ts` | 89 |  |
| `src/views/useChangesModel.ts` | 557 | The working-tree model behind the Project tab's Changes rail. |
| `src/vite-env.d.ts` | 7 | / <reference types="vite/client" /> |
| `src/windowTransparency.ts` | 20 | The DOM half of window transparency — one custom property, written in one |
| `src/windowTransparencyLogic.test.ts` | 121 |  |
| `src/windowTransparencyLogic.ts` | 126 | When the window is translucent, and by how much. |
| `src-tauri/src/browser/agent.rs` | 291 | Answering an agent's browser tool call: the gate, the projection, and the |
| `src-tauri/src/browser/agent_tests.rs` | 341 |  |
| `src-tauri/src/browser/host_tests.rs` | 191 | Tests for the two decisions in the host that are not about the webview. |
| `src-tauri/src/browser/mod.rs` | 778 | The embedded browser **host**: the one place a real web page lives inside |
| `src-tauri/src/browser/pipe.rs` | 511 | The browser control pipe: the one way an agent's MCP server reaches this |
| `src-tauri/src/browser/pipe_tests.rs` | 83 |  |
| `src-tauri/src/browser/registry.rs` | 133 | Publishing what an MCP client needs to find this application. |
| `src-tauri/src/browser/registry_tests.rs` | 284 |  |
| `src-tauri/src/browser/shared.rs` | 575 | The browser host's **data**: what the panel and an agent read, and every |
| `src-tauri/src/browser/shared_tests.rs` | 574 | Tests for the browser host's data layer. |
| `src-tauri/src/commands/about.rs` | 99 | What build is this? — the data behind Help → About. |
| `src-tauri/src/commands/architecture.rs` | 219 | Architecture-diagram commands. |
| `src-tauri/src/commands/behavioral.rs` | 485 | Behavioral before/after testing — running the same configuration against |
| `src-tauri/src/commands/browser.rs` | 461 | Commands behind the embedded browser panel. |
| `src-tauri/src/commands/browser_mcp.rs` | 93 | Commands behind the installable **browser** MCP server. |
| `src-tauri/src/commands/changelists.rs` | 57 | Change-group commands. |
| `src-tauri/src/commands/debug.rs` | 1012 | Launching applications under a Debug Adapter Protocol adapter. |
| `src-tauri/src/commands/editor_context_mcp.rs` | 116 | Commands behind the installable **Editor context** MCP server, plus the push |
| `src-tauri/src/commands/enhancements.rs` | 136 | Instruction-template commands. |
| `src-tauri/src/commands/erosion.rs` | 32 | Erosion-detector command. |
| `src-tauri/src/commands/features.rs` | 53 | Optional-feature commands. |
| `src-tauri/src/commands/files.rs` | 328 | Workspace file commands, for the Run tab's directory tree and file editor. |
| `src-tauri/src/commands/git.rs` | 409 | Git commands. |
| `src-tauri/src/commands/inspect.rs` | 352 | Object-inspection commands. |
| `src-tauri/src/commands/intents.rs` | 837 | Agent-intent commands. |
| `src-tauri/src/commands/launcher.rs` | 380 | The app launcher's bridge: run an arbitrary command line, and manage the |
| `src-tauri/src/commands/lsp.rs` | 545 | Asking this workspace's language servers something, and saying what they do. |
| `src-tauri/src/commands/mcp.rs` | 92 | Commands behind the installable SQL MCP server. |
| `src-tauri/src/commands/mcp_tools.rs` | 35 | Per-tool MCP gating commands. |
| `src-tauri/src/commands/notes.rs` | 21 | Notes / scratchpad commands. |
| `src-tauri/src/commands/qgate.rs` | 78 | Commands behind the installable quality-gate Stop hook. |
| `src-tauri/src/commands/redis.rs` | 425 | The Redis panel's bridge: list/save/discover connection profiles, browse |
| `src-tauri/src/commands/review.rs` | 369 | Launching an in-app adversarial review. |
| `src-tauri/src/commands/roslyn_mcp.rs` | 94 | Commands behind the installable **Roslyn / LSP** MCP server. |
| `src-tauri/src/commands/rules.rs` | 22 | Business-rule-doc command. |
| `src-tauri/src/commands/run.rs` | 473 | Running applications and tests. |
| `src-tauri/src/commands/running.rs` | 102 | The Running panel's bridge: list every process the app has running (across |
| `src-tauri/src/commands/secrets.rs` | 43 | .NET user secrets commands. |
| `src-tauri/src/commands/setup.rs` | 57 | First-open setup: preview and apply the combined agent-hook install (intent |
| `src-tauri/src/commands/sql.rs` | 2411 | The SQL console's bridge: list and save connection profiles, discover the |
| `src-tauri/src/commands/symbols.rs` | 234 | The command palette's surface: searching the workspace, and asking after |
| `src-tauri/src/commands/tasks.rs` | 349 | Commands behind the per-workspace Tasks panel and its installable MCP server. |
| `src-tauri/src/commands/terminal.rs` | 277 | Interactive floating terminals. |
| `src-tauri/src/commands/workspace.rs` | 273 | Workspace and configuration commands. |
| `src-tauri/src/editor_context/agent.rs` | 59 | Answering an agent's editor-context tool call: check the feature, resolve the |
| `src-tauri/src/editor_context/agent_tests.rs` | 74 |  |
| `src-tauri/src/editor_context/mod.rs` | 50 | The application's side of the editor-context MCP server: the process-global |
| `src-tauri/src/editor_context/pipe.rs` | 444 | The editor-context control pipe: the one way an agent's `mcp-editor` server |
| `src-tauri/src/editor_context/pipe_tests.rs` | 62 |  |
| `src-tauri/src/editor_context/registry.rs` | 125 | Publishing what an `mcp-editor` client needs to find this application. |
| `src-tauri/src/lib.rs` | 511 | The Tauri shell. |
| `src-tauri/src/main.rs` | 6 | Suppress the extra console window on Windows in release builds. |
| `src-tauri/src/mcp_browser.rs` | 350 | The `mcp-browser` mode: this executable as an MCP stdio server that talks to |
| `src-tauri/src/mcp_browser_tests.rs` | 189 |  |
| `src-tauri/src/mcp_editor.rs` | 362 | The `mcp-editor` mode: this executable as an MCP stdio server that forwards |
| `src-tauri/src/mcp_editor_tests.rs` | 66 |  |
| `src-tauri/src/mcp_redis.rs` | 257 | The `mcp-redis` mode: this executable as the Redis MCP stdio server. |
| `src-tauri/src/mcp_roslyn.rs` | 361 | The `mcp-roslyn` mode: this executable as an MCP stdio server that forwards |
| `src-tauri/src/mcp_roslyn_tests.rs` | 63 |  |
| `src-tauri/src/mcp_sql.rs` | 384 | The `mcp-sql` mode: this executable as an MCP stdio server. |
| `src-tauri/src/mcp_tasks.rs` | 254 | The `mcp-tasks` mode: this executable as the Tasks MCP stdio server. |
| `src-tauri/src/qgate_run.rs` | 240 | The `quality-gate` mode: deterministic checks when an agent turn ends. |
| `src-tauri/src/recorder.rs` | 151 | The `record-intent` mode, which is what the installed hooks actually run. |
| `src-tauri/src/roslyn/agent.rs` | 135 | Answering an agent's Roslyn tool call: resolve the location, ask the warm |
| `src-tauri/src/roslyn/mod.rs` | 46 | The application's side of the Roslyn MCP server: the process-global control |
| `src-tauri/src/roslyn/pipe.rs` | 476 | The Roslyn control pipe: the one way an agent's `mcp-roslyn` server reaches |
| `src-tauri/src/roslyn/pipe_tests.rs` | 80 |  |
| `src-tauri/src/roslyn/registry.rs` | 124 | Publishing what an `mcp-roslyn` client needs to find this application. |
| `src-tauri/src/state.rs` | 1055 | Shared application state. |
| `src-tauri/src/state_tests.rs` | 774 | Tests for the multi-workspace [`AppState`]. |
| `scripts/build-sidecar.mjs` | 110 | Publish the object-inspector sidecar into the Tauri bundle. |
| `scripts/check-docs.mjs` | 70 | Documentation health check. |
| `scripts/fetch-debuggers.mjs` | 228 | Vendor the debug adapters into the Tauri bundle. |
| `scripts/generate-index.mjs` | 165 | Regenerates docs/INDEX.md — the generated map of source files, Tauri |
| `examples/adapters/cargo-nextest.toml` | 34 | Rust tests via cargo-nextest, which can emit JUnit XML. |
| `examples/adapters/pytest.toml` | 35 | A worked example of a declarative adapter. |
| `examples/erosion/custom.toml` | 43 | Example erosion rules. |

## Tauri command surface

Registered in `src-tauri/src/lib.rs`; documented with parameters in [reference/commands.md](reference/commands.md).

- **workspace** (`src-tauri/src/commands/workspace.rs`): `open_workspace`, `current_workspace`, `list_open_workspaces`, `set_active_workspace`, `close_workspace`, `rescan_workspace`, `save_config`, `delete_config`, `preview_rider_import`, `apply_rider_import`, `launch_profiles`, `set_favorite`, `set_config_order`
- **enhancements** (`src-tauri/src/commands/enhancements.rs`): `list_enhancements`, `add_enhancement`, `remove_enhancement`, `list_prompts`, `agent_runs`, `mark_agent_run`, `save_note_as_instruction`
- **launcher** (`src-tauri/src/commands/launcher.rs`): `list_launchables`, `launch_command`, `stop_command`, `save_launchable`, `delete_launchable`
- **features** (`src-tauri/src/commands/features.rs`): `list_features`, `set_feature`
- **mcp_tools** (`src-tauri/src/commands/mcp_tools.rs`): `list_mcp_tools`, `set_mcp_tool`
- **notes** (`src-tauri/src/commands/notes.rs`): `read_notes`, `write_notes`
- **tasks** (`src-tauri/src/commands/tasks.rs`): `read_tasks`, `create_task`, `update_task`, `assign_task`, `complete_task`, `delete_task`, `tasks_mcp_status`, `tasks_mcp_install_plan`, `install_tasks_mcp_server`, `tasks_mcp_uninstall_plan`, `uninstall_tasks_mcp_server`
- **about** (`src-tauri/src/commands/about.rs`): `about_info`
- **files** (`src-tauri/src/commands/files.rs`): `fs_list_dir`, `fs_read_file`, `fs_write_file`, `fs_create_file`, `fs_create_dir`, `fs_rename`, `fs_delete`
- **secrets** (`src-tauri/src/commands/secrets.rs`): `read_project_secrets`, `write_project_secrets`
- **run** (`src-tauri/src/commands/run.rs`): `start_run`, `build_project`, `cancel_run`, `running_ids`, `run_tests`, `last_test_run`, `coverage_of_change`
- **debug** (`src-tauri/src/commands/debug.rs`): `start_debug`, `stop_debug`, `debug_ids`
- **review** (`src-tauri/src/commands/review.rs`): `start_review`, `cancel_review`, `review_agents`, `agent_interactive_command`
- **git** (`src-tauri/src/commands/git.rs`): `git_status`, `git_file_diff`, `git_file_contents`, `git_write_file`, `git_stage_file`, `git_unstage_file`, `git_stage_lines`, `git_unstage_lines`, `git_revert_lines`, `git_discard_file`, `git_commit`, `git_branches`, `git_create_branch`, `git_add_worktree`, `git_checkout_branch`, `git_checkout_remote_branch`, `git_delete_branch`, `git_merge_branch`, `git_abort_merge`, `git_history`, `git_commit_diff`, `git_commit_file_contents`, `git_commit_file_why`, `git_stash_save`, `git_stash_paths`, `git_stash_list`, `git_stash_pop`, `git_stash_apply`, `git_stash_drop`, `git_stash_clear`, `git_network`
- **changelists** (`src-tauri/src/commands/changelists.rs`): `git_changelists`, `git_create_changelist`, `git_delete_changelist`, `git_rename_changelist`, `git_assign_to_changelist`
- **intents** (`src-tauri/src/commands/intents.rs`): `intent_groups`, `stage_intent_group`, `revert_intent_group`, `reject_intent_group`, `intent_capture_status`, `intent_install_plan`, `enable_intent_capture`, `intent_uninstall_plan`, `disable_intent_capture`, `import_intent_history`, `intent_prune_preview`, `prune_intent_history`, `clear_intent_history`, `set_card_intent`, `clear_card_intent`, `move_card_edits`
- **mcp** (`src-tauri/src/commands/mcp.rs`): `mcp_server_status`, `mcp_server_install_plan`, `install_mcp_server`, `mcp_server_uninstall_plan`, `uninstall_mcp_server`
- **browser_mcp** (`src-tauri/src/commands/browser_mcp.rs`): `browser_mcp_status`, `browser_mcp_install_plan`, `install_browser_mcp`, `browser_mcp_uninstall_plan`, `uninstall_browser_mcp`
- **roslyn_mcp** (`src-tauri/src/commands/roslyn_mcp.rs`): `roslyn_mcp_server_status`, `roslyn_mcp_server_install_plan`, `install_roslyn_mcp_server`, `roslyn_mcp_server_uninstall_plan`, `uninstall_roslyn_mcp_server`
- **editor_context_mcp** (`src-tauri/src/commands/editor_context_mcp.rs`): `editor_mcp_server_status`, `editor_mcp_server_install_plan`, `install_editor_mcp_server`, `editor_mcp_server_uninstall_plan`, `uninstall_editor_mcp_server`, `set_editor_context`
- **qgate** (`src-tauri/src/commands/qgate.rs`): `quality_gate_status`, `quality_gate_install_plan`, `install_quality_gate`, `quality_gate_uninstall_plan`, `uninstall_quality_gate`
- **setup** (`src-tauri/src/commands/setup.rs`): `setup_install_plan`, `install_setup`
- **behavioral** (`src-tauri/src/commands/behavioral.rs`): `behavioral_diff`, `behavioral_clear`
- **erosion** (`src-tauri/src/commands/erosion.rs`): `erosion_scan`
- **rules** (`src-tauri/src/commands/rules.rs`): `list_rules`
- **inspect** (`src-tauri/src/commands/inspect.rs`): `inspect_status`, `inspect_attachable`, `inspect_run_dump`, `inspect_capture`, `inspect_last`, `inspect_clear`
- **architecture** (`src-tauri/src/commands/architecture.rs`): `arch_project_graph`, `arch_render_graph`, `arch_component_graph`, `arch_render_component_graph`, `arch_list_diagrams`, `arch_read_diagram`, `arch_write_diagram`, `arch_validate`
- **symbols** (`src-tauri/src/commands/symbols.rs`): `search_everywhere`, `symbol_index_status`, `rebuild_symbol_index`
- **lsp** (`src-tauri/src/commands/lsp.rs`): `lsp_status`, `lsp_restart`, `lsp_open_document`, `lsp_change_document`, `lsp_close_document`, `lsp_find_usages`, `lsp_goto_definition`, `lsp_declaration_anchors`, `lsp_prepare_rename`, `lsp_rename`
- **browser** (`src-tauri/src/commands/browser.rs`): `browser_open`, `browser_close`, `browser_set_bounds`, `browser_set_visible`, `browser_navigate`, `browser_back`, `browser_forward`, `browser_reload`, `browser_state`, `browser_console`, `browser_network`, `browser_page_text`, `browser_set_automation_consent`
- **terminal** (`src-tauri/src/commands/terminal.rs`): `terminal_open`, `terminal_write`, `terminal_resize`, `terminal_close`, `terminal_list`, `terminal_set_label`, `list_shells`
- **running** (`src-tauri/src/commands/running.rs`): `list_running`, `kill_running`
- **sql** (`src-tauri/src/commands/sql.rs`): `sql_list_connections`, `sql_discover`, `sql_save_connection`, `sql_delete_connection`, `sql_rename_connection`, `sql_set_allow_writes`, `sql_set_expose_to_agents`, `sql_test_connection`, `sql_test_connection_string`, `sql_list_objects`, `sql_list_columns`, `sql_execute`, `sql_cancel`
- **redis** (`src-tauri/src/commands/redis.rs`): `redis_list_connections`, `redis_discover`, `redis_save_connection`, `redis_delete_connection`, `redis_rename_connection`, `redis_set_allow_writes`, `redis_set_expose_to_agents`, `redis_test_connection`, `redis_scan_keys`, `redis_get_key`, `redis_key_info`, `redis_set_string`, `redis_hash_set`, `redis_hash_delete`, `redis_list_push`, `redis_list_remove`, `redis_set_add`, `redis_set_remove`, `redis_zset_add`, `redis_zset_remove`, `redis_stream_add`, `redis_delete_key`, `redis_expire`

## Frontend IPC wrappers (`src/ipc/api.ts`)

`openWorkspace`, `currentWorkspace`, `rescanWorkspace`, `listOpenWorkspaces`, `setActiveWorkspace`, `closeWorkspace`, `saveConfig`, `deleteConfig`, `launchProfiles`, `setFavorite`, `setConfigOrder`, `readProjectSecrets`, `writeProjectSecrets`, `previewRiderImport`, `applyRiderImport`, `fsListDir`, `fsReadFile`, `fsWriteFile`, `fsCreateFile`, `fsCreateDir`, `fsRename`, `fsDelete`, `listEnhancements`, `addEnhancement`, `removeEnhancement`, `listPrompts`, `agentRuns`, `markAgentRun`, `saveNoteAsInstruction`, `listFeatures`, `setFeature`, `listMcpTools`, `setMcpTool`, `readNotes`, `writeNotes`, `readTasks`, `createTask`, `updateTask`, `assignTask`, `completeTask`, `deleteTask`, `aboutInfo`, `startRun`, `buildProject`, `cancelRun`, `runningIds`, `startDebug`, `stopDebug`, `debugIds`, `runTests`, `lastTestRun`, `coverageOfChange`, `startReview`, `cancelReview`, `reviewAgents`, `agentInteractiveCommand`, `terminalOpen`, `terminalSetLabel`, `terminalWrite`, `terminalResize`, `terminalClose`, `terminalList`, `listShells`, `listLaunchables`, `launchCommand`, `stopCommand`, `saveLaunchable`, `deleteLaunchable`, `listRunning`, `killRunning`, `gitStatus`, `gitFileDiff`, `gitFileContents`, `gitWriteFile`, `gitStageFile`, `gitUnstageFile`, `gitStageLines`, `gitUnstageLines`, `gitRevertLines`, `gitDiscardFile`, `gitCommit`, `gitBranches`, `gitCreateBranch`, `gitCheckoutBranch`, `gitAddWorktree`, `gitCheckoutRemoteBranch`, `gitDeleteBranch`, `gitMergeBranch`, `gitAbortMerge`, `gitChangelists`, `gitCreateChangelist`, `gitDeleteChangelist`, `gitRenameChangelist`, `gitAssignToChangelist`, `gitHistory`, `gitCommitDiff`, `gitCommitFileContents`, `gitCommitFileWhy`, `gitStashSave`, `gitStashPaths`, `gitStashList`, `gitStashPop`, `gitStashApply`, `gitStashDrop`, `gitStashClear`, `gitNetwork`, `intentGroups`, `erosionScan`, `listRules`, `stageIntentGroup`, `revertIntentGroup`, `rejectIntentGroup`, `intentCaptureStatus`, `intentInstallPlan`, `enableIntentCapture`, `intentUninstallPlan`, `disableIntentCapture`, `importIntentHistory`, `intentPrunePreview`, `pruneIntentHistory`, `clearIntentHistory`, `setCardIntent`, `clearCardIntent`, `moveCardEdits`, `qualityGateStatus`, `qualityGateInstallPlan`, `installQualityGate`, `qualityGateUninstallPlan`, `uninstallQualityGate`, `mcpServerStatus`, `mcpServerInstallPlan`, `installMcpServer`, `mcpServerUninstallPlan`, `uninstallMcpServer`, `tasksMcpStatus`, `tasksMcpInstallPlan`, `installTasksMcpServer`, `tasksMcpUninstallPlan`, `uninstallTasksMcpServer`, `setupInstallPlan`, `installSetup`, `behavioralDiff`, `behavioralClear`, `inspectStatus`, `inspectCapture`, `inspectAttachable`, `inspectRunDump`, `inspectLast`, `inspectClear`, `searchEverywhere`, `symbolIndexStatus`, `rebuildSymbolIndex`, `archProjectGraph`, `archRenderGraph`, `archComponentGraph`, `archRenderComponentGraph`, `archListDiagrams`, `archReadDiagram`, `archWriteDiagram`, `archValidate`, `lspStatus`, `lspRestart`, `lspOpenDocument`, `lspChangeDocument`, `lspCloseDocument`, `lspFindUsages`, `lspGotoDefinition`, `lspDeclarationAnchors`, `lspPrepareRename`, `lspRename`, `sqlListConnections`, `sqlDiscover`, `sqlSaveConnection`, `sqlDeleteConnection`, `sqlRenameConnection`, `sqlSetAllowWrites`, `sqlSetExposeToAgents`, `redisListConnections`, `redisDiscover`, `redisSaveConnection`, `redisDeleteConnection`, `redisRenameConnection`, `redisSetAllowWrites`, `redisSetExposeToAgents`, `redisTestConnection`, `redisScanKeys`, `redisGetKey`, `redisKeyInfo`, `redisSetString`, `redisHashSet`, `redisHashDelete`, `redisListPush`, `redisListRemove`, `redisSetAdd`, `redisSetRemove`, `redisZsetAdd`, `redisZsetRemove`, `redisStreamAdd`, `redisDeleteKey`, `redisExpire`, `sqlTestConnection`, `sqlTestConnectionString`, `sqlListObjects`, `sqlListColumns`, `sqlExecute`, `sqlCancel`, `errorMessage`, `browserOpen`, `browserClose`, `browserSetBounds`, `browserSetVisible`, `browserNavigate`, `browserBack`, `browserForward`, `browserReload`, `browserState`, `browserConsole`, `browserNetwork`, `browserPageText`, `browserSetAutomationConsent`, `browserMcpStatus`, `browserMcpInstallPlan`, `installBrowserMcp`, `browserMcpUninstallPlan`, `uninstallBrowserMcp`, `roslynMcpStatus`, `roslynMcpInstallPlan`, `installRoslynMcp`, `roslynMcpUninstallPlan`, `uninstallRoslynMcp`, `setEditorContext`, `editorMcpStatus`, `editorMcpInstallPlan`, `installEditorMcp`, `editorMcpUninstallPlan`, `uninstallEditorMcp`

## Public core API (`cb-core`)

- `crates/core/src/adapters/cargo.rs`: `DependencyKind`, `PathDependency`, `CargoManifest`, `is_virtual_manifest()`, `parse()`
- `crates/core/src/adapters/dotnet.rs`: `ProjectFile`, `references()`, `references_prefix()`, `parse_project_file()`, `ConfiguredRunner`, `parse_dotnet_config()`, `is_test_project()`, `classify_runner()`, `has_trx_extension()`, `project_kind()`, `configurations()`, `LaunchProfile`, `is_launchable()`, `parse_launch_settings()`, `split_args()`, `BuildContext`, `test_invocation()`, `run_invocation()`, `BuildAction`, `build_action_invocation()`, `configs_for_project()`
- `crates/core/src/adapters/manifest.rs`: `AdapterManifest`, `CommandTemplate`, `parse()`, `load_dir()`, `matches()`, `matched_file()`, `build_invocation()`, `configs_for_project()`, `manifest_dir()`
- `crates/core/src/adapters/msbuild.rs`: `command_args()`, `parse_output()`, `apply()`, `evaluate()`
- `crates/core/src/adapters/node.rs`: `PackageJson`, `depends_on()`, `parse_package_json()`, `PackageManager`, `program()`, `run_script_args()`, `exec_args()`, `script_arg_separator()`, `detect_package_manager()`, `detect_runner()`, `is_workspace_root()`, `workspace_globs()`, `project_kind()`, `TestInvocationContext`, `test_invocation()`, `script_invocation()`, `configs_for_project()`, `project_dir()`
- `crates/core/src/adapters/solution.rs`: `Solution`, `SolutionProject`, `is_solution_file()`, `parse()`
- `crates/core/src/architecture/components.rs`: `component_graph()`
- `crates/core/src/architecture/graph.rs`: `ArchKind`, `EdgeKind`, `Derivation`, `ArchNode`, `ArchEdge`, `ArchGraph`, `project_graph()`
- `crates/core/src/architecture/mermaid.rs`: `render()`, `mermaid_id()`, `ValidationRule`, `ValidationError`, `validate()`
- `crates/core/src/architecture/signals/dotnet.rs`: `DotnetSignals`, `signals()`, `aspire_class_name()`
- `crates/core/src/architecture/signals/framework.rs`: `Strength`, `ComponentKind`, `slug()`, `Evidence`, `new()`, `elided_value()`, `Signal`, `high()`, `medium()`, `call()`, `with_detail()`, `Component`, `Usage`, `Detail`, `AdmittedEdge`, `AdmittedCall`, `DiscardReason`, `Discarded`, `Admitted`, `service_calls()`, `edges()`, `warnings()`, `warnings_named()`, `admit()`
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
- `crates/core/src/browser/argv.rs`: `is_mcp_browser_invocation()`, `McpBrowserInvocation`, `parse_mcp_browser_args()`
- `crates/core/src/browser/consent.rs`: `BrowserTool`, `Category`, `category()`, `name()`, `BrowserState`, `plugin_disabled()`, `ready()`, `Decision`, `allowed()`, `reason()`, `read_ran_on_the_granted_page()`, `decide()`
- `crates/core/src/browser/console.rs`: `classify_level()`, `is_noteworthy()`
- `crates/core/src/browser/framing.rs`: `pipe_decoder()`
- `crates/core/src/browser/install.rs`: `browser_mcp_entry()`, `status_for()`, `install_plan_for()`, `uninstall_plan_for()`, `caveats_for()`
- `crates/core/src/browser/instances.rs`: `Listener`, `pipe_name()`, `BrowserInstance`, `InstancesFile`, `InstanceError`, `code()`, `sentence()`, `instances_path()`, `load()`, `save()`, `upsert()`, `remove()`, `choose_instance()`
- `crates/core/src/browser/ipc.rs`: `PageMessage`, `PageMessageProblem`, `parse_page_message()`, `accept_eval_reply()`
- `crates/core/src/browser/liveness.rs`: `alive()`
- `crates/core/src/browser/model.rs`: `BrowserAvailability`, `licenses_a_read()`, `reason()`, `ConsoleLevel`, `ConsoleEntry`, `NetworkSource`, `NetworkEntry`, `PageText`, `AutomationConsent`, `ConsentWire`, `none()`, `reads_only()`, `reads_and_writes()`, `reads()`, `writes()`, `origin()`, `after_origin_change()`
- `crates/core/src/browser/origin.rs`: `NavigationVerdict`, `allowed()`, `Refusal`, `navigation_verdict()`, `origin_of()`, `same_origin()`
- `crates/core/src/browser/render.rs`: `AgentStatus`, `status()`, `current_url()`, `page_text()`, `outline()`, `console()`, `network()`, `acted()`, `navigated()`, `history_step()`
- `crates/core/src/browser/ring.rs`: `Slice`, `Ring`, `new()`, `push()`, `dropped()`, `len()`, `is_empty()`, `capacity()`, `latest_seq()`, `since()`, `clear()`
- `crates/core/src/browser/script.rs`: `ScriptError`, `json_string()`, `init_script()`, `page_text_script()`, `read_page_script()`, `click_script()`, `type_script()`, `press_key_script()`
- `crates/core/src/browser/serve.rs`: `initialize_result()`, `tools_list_result()`, `disabled_tool_answer()`, `answer_result()`, `unsupported_answer()`, `unknown_tool_answer()`
- `crates/core/src/browser/text.rs`: `truncate_page_text()`
- `crates/core/src/browser/tools.rs`: `descriptors()`, `BrowserToolCall`, `tool()`, `parse_call()`, `is_known()`, `unknown_tool_sentence()`
- `crates/core/src/browser/url.rs`: `UrlError`, `normalize_input()`
- `crates/core/src/browser/wire.rs`: `Request`, `ToolAnswer`, `ok()`, `refused()`, `request_value()`, `answer_value()`, `RequestProblem`, `code()`, `sentence()`, `answer()`, `parse_request()`, `token_matches()`, `PipeFailure`, `code()`, `sentence()`, `answer()`, `parse_answer()`
- `crates/core/src/changelists.rs`: `Changelist`, `Changelists`, `group_of()`, `changelists_path()`, `load()`, `save()`, `create()`, `remove()`, `rename()`, `assign()`
- `crates/core/src/config.rs`: `WorkspaceConfig`, `dump_capture_enabled()`, `inspector_caps()`, `keep_dumps()`, `max_dump_megabytes()`, `config_dir()`, `config_path()`, `results_dir()`, `lsp_log_dir()`, `load()`, `rider_config_id()`, `ensure_gitignore()`, `save()`, `merge()`, `apply()`, `sort_configs()`, `set_favorite()`, `set_order()`, `upsert()`, `remove()`
- `crates/core/src/dap/breakpoints.rs`: `BindState`, `from_body()`, `is_verified()`, `Breakpoint`, `new()`, `apply()`, `effective_line()`, `detach()`, `BreakpointSet`, `toggle()`, `in_file()`, `lines_in_file()`, `files()`, `is_empty()`, `total()`, `apply_response()`, `apply_event()`, `detach_all()`
- `crates/core/src/dap/coalesce.rs`: `OutputBatch`, `Coalescer`, `new()`, `pending()`, `push()`, `take()`
- `crates/core/src/dap/model.rs`: `DebugEvent`, `DebugState`, `is_paused()`, `is_live()`, `StackFrame`, `Variable`, `is_expandable()`, `Scope`, `Thread`, `DebugStatus`, `idle()`
- `crates/core/src/dap/positions.rs`: `line_from_adapter()`, `column_from_adapter()`, `line_to_adapter()`
- `crates/core/src/dap/protocol.rs`: `Message`, `seq()`, `Request`, `Response`, `failure_text()`, `Event`, `Stopped`, `from_body()`, `Output`, `from_body()`, `exited_code()`, `Capabilities`, `initialize_arguments()`, `set_breakpoints_arguments()`
- `crates/core/src/dap/registry.rs`: `Debuggee`, `for_ecosystem()`, `adapter_id()`, `AdapterSpec`, `Resolution`, `resolve()`
- `crates/core/src/dap/sequence.rs`: `Sequencer`, `next_seq()`, `peek()`, `CorrelationError`, `Pending`, `Correlator`, `request()`, `next_seq()`, `resolve()`, `outstanding()`, `outstanding_commands()`, `abandon_all()`, `needs_reply()`
- `crates/core/src/editor_context/answer.rs`: `EditorRefusal`, `code()`, `sentence()`, `answer()`
- `crates/core/src/editor_context/argv.rs`: `is_mcp_editor_invocation()`, `McpEditorInvocation`, `parse_mcp_editor_args()`
- `crates/core/src/editor_context/install.rs`: `editor_mcp_entry()`, `status_for()`, `install_plan_for()`, `uninstall_plan_for()`, `caveats_for()`
- `crates/core/src/editor_context/instances.rs`: `Listener`, `pipe_name()`, `EditorInstance`, `has_workspace()`, `InstancesFile`, `InstanceError`, `code()`, `sentence()`, `instances_path()`, `load()`, `save()`, `upsert()`, `remove()`, `choose_instance()`
- `crates/core/src/editor_context/liveness.rs`: `alive()`
- `crates/core/src/editor_context/render.rs`: `get_active_file()`, `get_selection()`, `get_open_files()`, `get_recent_files()`
- `crates/core/src/editor_context/serve.rs`: `initialize_result()`, `tools_list_result()`, `disabled_tool_answer()`, `answer_result()`, `unknown_tool_answer()`
- `crates/core/src/editor_context/tools.rs`: `descriptors()`, `EditorToolCall`, `name()`, `parse_call()`, `is_known()`, `unknown_tool_sentence()`
- `crates/core/src/editor_context/wire.rs`: `Request`, `ToolAnswer`, `ok()`, `refused()`, `request_value()`, `answer_value()`, `RequestProblem`, `code()`, `sentence()`, `answer()`, `parse_request()`, `token_matches()`, `PipeFailure`, `code()`, `sentence()`, `answer()`, `parse_answer()`
- `crates/core/src/enhancements/mod.rs`: `Placement`, `Template`, `EnhancementInfo`, `PromptInfo`, `begin_marker()`, `end_marker()`, `is_present()`, `parse_template()`, `split_front_matter()`, `insert()`, `remove()`, `discover()`, `seed()`, `slugify()`, `serialize_template()`, `save_template()`, `list()`, `list_prompts()`, `add()`, `remove_from_agents()`, `templates_dir()`, `prompts_dir()`
- `crates/core/src/enhancements/runs.rs`: `PromptRun`, `runs_path()`, `read_runs()`, `record_run()`
- `crates/core/src/erosion/rules.rs`: `ErosionCategory`, `RuleSide`, `ErosionRule`, `CompiledRule`, `parse()`, `load_dir()`, `rules_dir()`, `all_rules()`, `compile()`, `builtin_rules()`
- `crates/core/src/erosion/scan.rs`: `ErosionFlag`, `ErosionReport`, `scan_diffs()`
- `crates/core/src/features/mod.rs`: `FeatureId`, `id()`, `label()`, `description()`, `default_enabled()`, `from_id()`, `FeaturesFile`, `is_enabled()`, `set()`, `list()`, `FeatureInfo`
- `crates/core/src/features/store.rs`: `features_path()`, `Platform`, `current()`, `seed_path_for()`, `seed_path()`, `merge_seed()`, `ensure_seeded()`, `load()`, `load_existing()`, `save()`
- `crates/core/src/files.rs`: `DirEntry`, `list_dir()`, `read_file()`, `write_file()`, `create_file()`, `create_dir()`, `rename()`, `delete()`
- `crates/core/src/git/attribution.rs`: `MatchLevel`, `Confidence`, `AttributedSpan`, `HunkAttribution`, `FileAttribution`, `is_empty()`, `Options`, `anchor_key()`, `attribute_file()`, `attribute()`
- `crates/core/src/git/coverage.rs`: `UnfulfilledClaim`, `Scorecard`, `IntentReview`, `review()`
- `crates/core/src/git/grouping.rs`: `GroupKind`, `GroupFile`, `IntentGroup`, `hunk_count()`, `is_formatting_only()`, `enclosing_symbol()`, `changed_content()`, `group()`
- `crates/core/src/git/patch.rs`: `LineOrigin`, `DiffLine`, `Hunk`, `FileDiff`, `changed_line_indices()`, `hunk_line_indices()`, `Direction`, `build_patch()`
- `crates/core/src/git/repo.rs`: `clear_readonly_directories()`, `clear_readonly_directories()`, `ComparisonMode`, `ChangeKind`, `FileContents`, `FileChange`, `is_conflicted()`, `WorkingStatus`, `Branch`, `Commit`, `StashEntry`, `MergeOutcome`, `MergeReport`, `StageTarget`, `Repo`, `open()`, `workdir()`, `head_oid()`, `hooks_dir()`, `status()`, `file_diff()`, `diff_all()`, `baseline_content()`, `working_content()`, `write_working_file()`, `stage_file()`, `unstage_file()`, `stage_lines()`, `unstage_lines()`, `revert_lines()`, `discard_file()`, `commit()`, `merge_branch()`, `abort_merge()`, `branches()`, `create_branch()`, `checkout_branch()`, `create_branch_from()`, `checkout_remote_branch()`, `delete_branch()`, `history()`, `commit_diff()`, `commit_file_contents()`, `write_why_note()`, `read_why_note()`, `why_for_file()`, `stash_save()`, `stash_paths()`, `stash_list()`, `stash_pop()`, `stash_apply()`, `stash_drop()`, `stash_clear()`, `network_command()`, `NetworkOperation`, `NetworkKind`, `resolve_network()`
- `crates/core/src/git/why.rs`: `WhyEntry`, `FileWhy`, `DurableWhy`, `is_empty()`, `LineIntent`, `content_hash()`, `build_for_commit()`, `resolve_lines()`, `file_in()`, `record_note()`
- `crates/core/src/git/worktree.rs`: `validate_branch_name()`, `sanitize_dir_segment()`, `default_worktree_dir()`, `add_worktree()`
- `crates/core/src/importers/rider.rs`: `RiderConfiguration`, `parse()`, `expand_macros()`, `convert()`, `resolve_compounds()`, `ImportResult`, `import()`
- `crates/core/src/inspect/dumps.rs`: `dumps_dir()`, `ParsedDumpName`, `dump_env()`, `parse_dump_name()`, `list()`, `newest_for()`, `prune()`, `prune_unnamed()`
- `crates/core/src/inspect/graph.rs`: `RawResult`, `RawNode`, `parse()`, `classify()`, `display_label()`
- `crates/core/src/inspect/mod.rs`: `parse_result_file()`, `parse_result()`
- `crates/core/src/inspect/model.rs`: `Bitness`, `InspectTarget`, `TargetSummary`, `Caps`, `widened()`, `ElidedReason`, `ObjectValue`, `is_expandable()`, `InspectNode`, `InspectGraph`, `RootSpec`, `InspectRequest`, `new()`, `DumpFile`, `DotnetProcess`, `Attribution`, `AttachableProcess`, `AttachableList`, `RunDump`, `InspectStatus`, `InspectorConfig`
- `crates/core/src/inspect/session.rs`: `new_session_id()`, `request_for()`, `attribute()`, `launcher_caveat()`, `live_target_reason()`, `dump_for_run()`, `unsupported_reason()`, `attach_caveats()`, `first_bitness()`, `AttemptOutcome`, `attempt_outcome()`, `other_bitness()`, `retry_bitness()`, `enumeration_outcome()`, `missing_sidecar_reason()`, `status()`, `arm_dumps()`, `ArmedDumps`, `prune()`
- `crates/core/src/inspect/sidecar.rs`: `sessions_dir()`, `session_dir()`, `request_path()`, `result_path()`, `command_args()`, `list_command_args()`, `process_list_path()`, `ProcessList`, `parse_process_list()`, `parse_process_list_file()`, `write_request()`, `sidecar_file_name()`, `FailureCode`, `SidecarFailure`, `failure_of()`, `next_attempt()`, `resolve()`, `retain_newest()`
- `crates/core/src/inspect/tree.rs`: `Built`, `build()`
- `crates/core/src/intents/guard.rs`: `block()`, `hook_path()`, `plan_for()`, `planned_write()`, `is_installed()`, `plan_removal()`, `ensure_executable()`
- `crates/core/src/intents/hook.rs`: `is_record_invocation()`, `RecorderInvocation`, `parse_recorder_args()`, `HookEvent`, `parse()`, `ingest()`, `ask_for_intent()`, `parse_labels_with_source()`, `parse_declared_labels()`, `parse_labels()`, `looks_like_narration()`, `is_usable_inferred_label()`, `is_enabled()`, `project_record_hook_present()`, `defers_to_project_hook()`, `resolve_root()`, `resolve_enabled_root()`
- `crates/core/src/intents/mod.rs`: `ProviderId`, `as_str()`, `IntentEdit`, `is_empty()`, `IntentRecord`, `LabelSource`, `SelfConfidence`, `IntentLabel`, `IntentPrompt`, `Intents`, `is_empty()`, `label_for()`, `effective_scoped_label()`, `scoped_labels_for_path()`, `for_path()`, `normalise_path()`, `scope_covers()`, `relative_to()`, `intents_dir()`, `edits_path()`, `labels_path()`, `prompts_path()`, `LoadOptions`, `load()`, `IntentCache`, `load()`, `invalidate()`, `load_prompts()`, `prompt_for()`, `compact_if_large()`, `append_edit()`, `append_label()`, `append_prompt()`, `next_seq()`, `rebase_seqs()`, `clear()`
- `crates/core/src/intents/patchfmt.rs`: `PatchedFile`, `parse_envelope()`, `parse_unified_diff()`, `envelope_from_value()`
- `crates/core/src/intents/providers/claude_code.rs`: `ClaudeCode`, `new()`, `with_home()`, `claude_home()`, `project_settings_path()`, `user_settings_path()`, `encode_project_dir()`
- `crates/core/src/intents/providers/codex.rs`: `Codex`, `new()`, `codex_home()`, `detected_in()`, `status_in()`, `install_plan_in()`, `history_in()`, `is_trusted_in()`, `planned_entries()`
- `crates/core/src/intents/providers/hooks_json.rs`: `is_installed()`, `commands_for()`, `pinned_workspace()`, `pinned_elsewhere()`, `pinned_caveat()`, `plan_merge()`, `plan_removal()`
- `crates/core/src/intents/providers/instructions.rs`: `path_for()`, `is_present()`, `planned_write()`
- `crates/core/src/intents/providers/mod.rs`: `InstallScope`, `ProviderStatus`, `absent()`, `PlannedWrite`, `InstallPlan`, `SessionFile`, `HistoryMined`, `apply_plan()`, `apply_writes()`, `apply_writes_atomically()`, `all()`, `statuses()`, `history()`, `uninstall_plan()`, `guard_write()`, `whyhook_write()`, `home_dir()`
- `crates/core/src/intents/providers/settings_merge.rs`: `contains_marker()`, `is_installed()`, `merged_text()`, `merged_into_text()`, `plan_removal()`
- `crates/core/src/intents/reject.rs`: `CommentSyntax`, `comment_syntax()`, `comment_prefix()`, `sanitise_reason()`, `marker_block_for()`, `is_marker_line()`, `anchors()`, `insert_markers()`, `iso_date()`, `FileRejection`, `RejectSummary`, `record()`, `reject_file()`
- `crates/core/src/intents/retire.rs`: `archive_path()`, `label_archive_path()`, `tombstone_path()`, `state_path()`, `FileSnapshot`, `KeepReason`, `Verdict`, `RetirePlan`, `Tombstone`, `ArchivedRecord`, `PruneState`, `RetireSummary`, `content_key()`, `verdict()`, `plan()`, `tombstones_for()`, `reject_tombstoned()`, `load_state()`, `load_tombstones()`, `snapshot()`, `run_if_head_moved()`, `preview()`, `run_now()`, `clear()`
- `crates/core/src/intents/user.rs`: `UserEdit`, `UserIntent`, `turn_id()`, `is_empty()`, `overlaps()`, `upsert()`, `remove_overlapping()`, `next_id()`, `MoveRequest`, `note_id_of_group()`, `move_edits()`, `to_intents()`, `merge_into()`, `user_intents_path()`, `load()`, `save()`
- `crates/core/src/intents/whyhook.rs`: `block()`, `hook_path()`, `plan_for()`, `planned_write()`, `is_installed()`, `ensure_executable()`, `plan_removal()`
- `crates/core/src/invocation.rs`: `build()`, `build_coverage()`, `rerun_filter()`, `plan_compound()`, `SolutionBuildStep`, `plan_solution_build()`
- `crates/core/src/launcher/model.rs`: `Launchable`, `LauncherFile`, `LauncherGroups`
- `crates/core/src/launcher/parse.rs`: `Tokens`, `tokenise()`, `split_command()`, `shell_flag()`, `shell_args()`, `program_and_args()`
- `crates/core/src/launcher/recents.rs`: `record_run()`, `group()`, `within_root()`, `rename()`, `set_pinned()`, `set_flags()`, `remove()`, `find()`
- `crates/core/src/launcher/store.rs`: `launchers_path()`, `load()`, `save()`
- `crates/core/src/lsp/client.rs`: `ReadyState`, `is_ready()`, `StartFailure`, `RequestError`, `Client`, `start()`, `start_with_ceiling()`, `spec()`, `root()`, `root_uri()`, `capabilities()`, `pid()`, `death()`, `stderr_tail()`, `readiness()`, `wait_ready()`, `shutdown()`, `did_open()`, `did_change()`, `did_close()`, `document_version()`, `references()`, `definition()`, `implementation()`, `type_definition()`, `document_symbols()`, `prepare_rename()`, `rename()`, `prepare_type_hierarchy()`, `type_supertypes()`, `type_subtypes()`, `signature_help()`, `diagnostics()`
- `crates/core/src/lsp/documents.rs`: `SyncAction`, `server()`, `path()`, `Documents`, `new()`, `open()`, `change()`, `close()`, `replay_for()`, `text()`, `version()`, `is_open()`, `open_paths()`, `servers_for()`
- `crates/core/src/lsp/edits.rs`: `EditError`, `plan()`, `apply()`, `replaced_texts()`
- `crates/core/src/lsp/framing.rs`: `FramingError`, `encode()`, `Decoder`, `new()`, `with_limits()`, `buffered()`, `push()`
- `crates/core/src/lsp/jsonrpc.rs`: `RequestId`, `RpcError`, `Incoming`, `ProtocolError`, `classify()`, `Outgoing`, `request()`, `notification()`, `reply()`, `error_reply()`
- `crates/core/src/lsp/model.rs`: `Availability`, `UsageResult`, `unavailable()`, `with_server()`, `Usage`, `Highlight`, `DefinitionResult`, `unavailable()`, `Target`, `AnchorResult`, `unavailable()`, `DeclarationAnchor`, `RenameResult`, `unavailable()`, `with_server()`, `RangeEdit`, `RenamedFile`, `BufferEdits`, `RenameFailure`, `PrepareRenameResult`, `unavailable()`, `not_renameable()`, `with_server()`, `TypeNode`, `TypeHierarchyResult`, `unavailable()`, `with_server()`, `SignatureInfo`, `ParameterInfo`, `OverloadResult`, `unavailable()`, `with_server()`, `DiagnosticSeverity`, `DiagnosticRow`, `DiagnosticsResult`, `unavailable()`, `with_server()`, `LspStatus`, `ServerStatus`
- `crates/core/src/lsp/positions.rs`: `utf16_to_byte()`, `byte_to_utf16()`, `byte_offset()`, `to_editor_line()`, `to_lsp_line()`, `Snippet`, `snippet()`
- `crates/core/src/lsp/protocol.rs`: `Position`, `Range`, `TextEdit`, `Location`, `LocationLink`, `DecodeError`, `decode_goto()`, `DocumentEdits`, `ResourceOperation`, `WorkspaceEdit`, `decode_workspace_edit()`, `PrepareRenameResponse`, `decode_prepare_rename()`, `Symbol`, `decode_document_symbols()`, `symbol_kind()`, `TypeHierarchyItem`, `TypeHierarchyItemParams`, `new()`, `decode_type_hierarchy()`, `SignatureHelp`, `SignatureInformation`, `ParameterInformation`, `decode_signature_help()`, `Diagnostic`, `DocumentDiagnosticParams`, `new()`, `decode_diagnostics()`, `SyncKind`, `ServerCapabilities`, `from_initialize_result()`, `encoding_is_utf16()`, `initialize_params()`, `TextDocumentIdentifier`, `VersionedTextDocumentIdentifier`, `TextDocumentItem`, `DidOpenTextDocumentParams`, `new()`, `TextDocumentContentChangeEvent`, `DidChangeTextDocumentParams`, `whole_document()`, `document_end()`, `DidCloseTextDocumentParams`, `new()`, `TextDocumentPositionParams`, `new()`, `ReferenceContext`, `ReferenceParams`, `new()`, `RenameParams`, `new()`, `DocumentSymbolParams`, `new()`
- `crates/core/src/lsp/registry.rs`: `RealProbe`, `Language`, `id()`, `from_ecosystem()`, `languages_present()`, `Readiness`, `Timeouts`, `ServerSpec`, `language_id_for()`, `Resolution`, `language_for_extension()`, `resolve()`, `Candidate`, `caller_args()`, `takes_caller_args()`, `parse_extension_version()`
- `crates/core/src/lsp/rename.rs`: `RealFiles`, `new()`, `apply_workspace_edit()`, `enclosing_identifier()`
- `crates/core/src/lsp/results.rs`: `usages()`, `targets()`, `anchors()`, `type_hierarchy()`, `overloads()`, `diagnostics()`
- `crates/core/src/lsp/session.rs`: `LspHandle`, `root()`, `generation()`, `status()`, `server_pids()`, `request_teardown()`, `open_document()`, `change_document()`, `close_document()`, `find_usages()`, `goto_definition()`, `declaration_anchors()`, `prepare_rename()`, `rename()`, `find_references()`, `get_type_hierarchy()`, `resolve_overloads()`, `get_diagnostics()`, `start()`, `start_with_probe()`
- `crates/core/src/lsp/settings.rs`: `LspConfig`, `server()`, `ServerOverride`, `is_disabled()`, `UriStyleSetting`, `style()`
- `crates/core/src/lsp/transport.rs`: `DeathReason`, `Death`, `RequestFailure`, `Launch`, `Notification`, `Transport`, `spawn()`, `pid()`, `death()`, `watch_death()`, `notifications()`, `signal()`, `stderr_tail()`, `allocate_id()`, `is_pending()`, `request()`, `request_with_id()`, `notify()`, `cancel()`, `shutdown()`, `answer_for()`, `RestartDecision`, `RestartPolicy`, `new()`, `with_window()`, `is_failed()`, `on_death()`
- `crates/core/src/lsp/uri.rs`: `UriStyle`, `to_file_uri()`, `from_file_uri()`
- `crates/core/src/mcp/answer.rs`: `ConnectionStatusKind`, `name()`, `detail()`, `restate_for_agent()`, `McpRefusal`, `code()`, `sentence()`, `connect_failed()`, `statement_failed()`
- `crates/core/src/mcp/argv.rs`: `is_mcp_sql_invocation()`, `McpSqlInvocation`, `parse_mcp_sql_args()`
- `crates/core/src/mcp/execute.rs`: `agent_plan()`, `agent_connect_spec()`, `agent_enforcement()`
- `crates/core/src/mcp/expose.rs`: `AgentConnection`, `connection_handle()`, `of()`, `ExposedListing`, `listing()`, `exposed_now()`, `empty_listing_refusal()`, `find_exposed()`
- `crates/core/src/mcp/install/codex_toml.rs`: `merged_text()`, `merged_into_text()`, `removal_text()`, `plan_removal()`, `is_installed_in()`, `is_installed()`, `installed_state()`
- `crates/core/src/mcp/install/mcp_merge.rs`: `merged_text()`, `merged_into_text()`, `removal_text()`, `plan_removal()`, `is_installed_in()`, `is_installed()`, `installed_state()`
- `crates/core/src/mcp/install/mod.rs`: `read_existing()`, `McpConfigTarget`, `path()`, `config_target()`
- `crates/core/src/mcp/install/plan.rs`: `status_for()`, `install_plan_for()`, `uninstall_plan_for()`, `caveats_for()`
- `crates/core/src/mcp/liveness.rs`: `alive_by()`, `same_executable()`
- `crates/core/src/mcp/ndjson.rs`: `NdjsonError`, `Line`, `encode()`, `LineDecoder`, `new()`, `with_limit()`, `is_poisoned()`, `push()`
- `crates/core/src/mcp/render.rs`: `cap_notice()`, `result()`, `value()`, `enforcement()`, `strength()`, `connections()`, `tables()`, `columns()`, `status()`
- `crates/core/src/mcp/serve.rs`: `negotiated_version()`, `initialize_result()`, `tools_list_result()`, `Route`, `route()`, `success()`, `failure()`, `parse_failure()`, `list_connections_answer()`, `catalog_objects()`, `catalog_columns()`
- `crates/core/src/mcp/tools.rs`: `descriptors()`, `ToolCall`, `parse_call()`, `text_result()`, `refusal_result()`
- `crates/core/src/model.rs`: `Project`, `ProjectKind`, `TestRunner`, `RunKind`, `ConfigSource`, `RunConfig`, `new()`, `ReportSpec`, `ReportFormat`, `CoverageFormat`, `CoverageSpec`, `Invocation`, `TestOutcome`, `TestCase`, `TestSummary`, `from_cases()`, `TestRunResult`, `TestNode`, `EditorContext`, `EditorCursor`, `EditorViewport`, `EditorSelection`, `EditorOpenFile`, `EditorRecentFile`
- `crates/core/src/notes.rs`: `Note`, `NotesFile`, `notes_path()`, `load()`, `save()`
- `crates/core/src/process/chunker.rs`: `Utf8Chunker`, `new()`, `push()`, `finish()`, `LineSplitter`, `new()`, `push()`, `finish()`
- `crates/core/src/process/kill.rs`: `configure_process_group()`, `windows_creation_flags()`, `no_window()`, `kill_tree()`, `kill_tree_async()`
- `crates/core/src/process/mod.rs`: `Stream`, `ProcessEvent`, `Supervisor`, `new()`, `with_store()`, `run()`, `run_tracked()`, `cancel()`, `running_ids()`, `pid()`, `running()`, `is_running()`
- `crates/core/src/process/resolve.rs`: `resolve_program()`
- `crates/core/src/pty/argv.rs`: `is_batch_target()`, `batch_argv_refusal()`, `batch_program_refusal()`, `check_batch_argv()`
- `crates/core/src/pty/mod.rs`: `PtyManager`, `new()`, `with_store()`, `open()`, `open_tracked()`, `write()`, `resize()`, `close()`, `is_open()`, `list()`
- `crates/core/src/pty/model.rs`: `PtySpec`, `ShellInfo`, `DetectedShells`, `TerminalEvent`
- `crates/core/src/pty/shell.rs`: `clamp_size()`, `pick_shell()`, `default_shell()`, `is_session_marker()`, `ShellCandidate`, `shell_candidates()`, `detected_shells()`, `is_wsl_bash_launcher()`, `default_among()`, `detect_shells()`
- `crates/core/src/qgate/install.rs`: `status()`, `status_for()`, `settings_path()`, `install_plan()`, `install_plan_for()`, `uninstall_plan()`, `uninstall_plan_for()`, `merged_into()`
- `crates/core/src/qgate/mod.rs`: `Gate`, `label()`, `command()`, `gates_for_changes()`, `touched_source()`, `touched_memories()`, `should_remind_memories()`, `erosion_reminder()`, `GateVerdict`, `unresolved_packages()`, `declared_dependencies()`, `read_gate_output()`, `has_unresolved_rejection()`, `should_skip_for_loop()`, `has_typecheck_script()`, `is_quality_gate_invocation()`, `QGateInvocation`, `parse_qgate_args()`
- `crates/core/src/redis/discover.rs`: `CandidateState`, `is_connectable()`, `RedisCandidate`, `Discovery`, `discover()`, `read_value()`
- `crates/core/src/redis/driver.rs`: `DriverError`, `RedisClient`, `connect()`, `ping()`, `read()`, `write()`, `ReadResult`
- `crates/core/src/redis/dsn.rs`: `RedisTarget`, `RedisConnectionDisplay`, `looks_like_redis()`, `parse()`, `display_form()`
- `crates/core/src/redis/mcp/answer.rs`: `McpRefusal`, `code()`, `sentence()`, `text_result()`, `refusal_result()`
- `crates/core/src/redis/mcp/argv.rs`: `is_mcp_redis_invocation()`, `McpRedisInvocation`, `parse_mcp_redis_args()`
- `crates/core/src/redis/mcp/execute.rs`: `Planned`, `plan()`, `resolve_target()`
- `crates/core/src/redis/mcp/expose.rs`: `AgentConnection`, `of()`, `ExposedListing`, `listing()`, `exposed_now()`, `empty_listing_refusal()`, `find_exposed()`
- `crates/core/src/redis/mcp/render.rs`: `connections()`, `scan_page()`, `value()`, `status()`, `write_ok()`
- `crates/core/src/redis/mcp/serve.rs`: `initialize_result()`, `tools_list_result()`, `list_connections_answer()`
- `crates/core/src/redis/mcp/tools.rs`: `descriptors()`, `ToolCall`, `connection()`, `parse_call()`, `is_known()`, `unknown_tool_sentence()`, `is_write()`
- `crates/core/src/redis/model.rs`: `RedisType`, `from_wire()`, `RedisKeyInfo`, `ZMember`, `HashField`, `StreamEntry`, `RedisValue`, `ScanPage`, `RedisStatusKind`
- `crates/core/src/redis/ops.rs`: `ReadOp`, `WriteOp`, `WritePlan`, `into_op()`, `op()`, `Refusal`, `code()`, `sentence()`, `plan_write()`
- `crates/core/src/redis/store.rs`: `RedisConnection`, `RedisConnectionsFile`, `redis_connections_path()`, `load()`, `save()`, `upsert()`, `remove()`, `rename()`, `set_allow_writes()`, `set_expose_to_agents()`
- `crates/core/src/review.rs`: `ReviewAgent`, `AgentMode`, `id()`, `from_id()`, `id()`, `label()`, `program()`, `models()`, `default_model()`, `from_id()`, `models_for()`, `codex_models()`, `parse_codex_models()`, `resolve_model()`, `compose_prompt()`, `resolve_prompt_body()`, `agent_args()`, `detect_agents()`, `agent_args_interactive()`
- `crates/core/src/roslyn/answer.rs`: `RoslynRefusal`, `from_availability()`, `code()`, `sentence()`, `path_within_root()`
- `crates/core/src/roslyn/argv.rs`: `is_mcp_roslyn_invocation()`, `McpRoslynInvocation`, `parse_mcp_roslyn_args()`
- `crates/core/src/roslyn/install.rs`: `roslyn_mcp_entry()`, `status_for()`, `install_plan_for()`, `uninstall_plan_for()`, `caveats_for()`
- `crates/core/src/roslyn/instances.rs`: `Listener`, `pipe_name()`, `RoslynInstance`, `has_workspace()`, `InstancesFile`, `InstanceError`, `code()`, `sentence()`, `instances_path()`, `load()`, `save()`, `upsert()`, `remove()`, `choose_instance()`
- `crates/core/src/roslyn/liveness.rs`: `alive()`
- `crates/core/src/roslyn/render.rs`: `find_references()`, `diagnostics()`, `type_hierarchy()`, `overloads()`
- `crates/core/src/roslyn/serve.rs`: `initialize_result()`, `tools_list_result()`, `disabled_tool_answer()`, `answer_result()`, `unknown_tool_answer()`
- `crates/core/src/roslyn/symbol.rs`: `Located`, `locate_symbol()`
- `crates/core/src/roslyn/tools.rs`: `descriptors()`, `Target`, `RoslynToolCall`, `path()`, `name()`, `parse_call()`, `is_known()`, `unknown_tool_sentence()`
- `crates/core/src/roslyn/wire.rs`: `Request`, `ToolAnswer`, `ok()`, `refused()`, `request_value()`, `answer_value()`, `RequestProblem`, `code()`, `sentence()`, `answer()`, `parse_request()`, `token_matches()`, `PipeFailure`, `code()`, `sentence()`, `answer()`, `parse_answer()`
- `crates/core/src/rules/mod.rs`: `RuleDoc`, `RulesReport`, `rules_dir()`, `parse_rule_doc()`, `load_rules()`
- `crates/core/src/running/classify.rs`: `ProcInfo`, `OrphanClassification`, `classify_orphans()`, `identity_matches()`
- `crates/core/src/running/mod.rs`: `RunMeta`, `observe()`
- `crates/core/src/running/probe.rs`: `probe()`
- `crates/core/src/running/record.rs`: `RunKind`, `RunningRecord`, `RunningReport`, `RunningFile`
- `crates/core/src/running/store.rs`: `RunningStore`, `new()`, `load_orphans()`, `record()`, `remove()`, `remove_if_pid()`, `update_label()`, `resolve_orphan()`, `live()`, `orphans()`, `warnings()`, `load()`, `save()`, `running_path()`
- `crates/core/src/secrets.rs`: `ProjectSecrets`, `resolve_project_path()`, `secrets_path()`, `user_secrets_id()`, `read()`, `read_with_id()`, `ensure_id()`, `strip_jsonc()`, `write()`
- `crates/core/src/setup.rs`: `setup_plan()`
- `crates/core/src/sql/catalog.rs`: `SqlObjectKind`, `SqlObject`, `SqlColumn`, `IdentifierRefusal`, `sentence()`, `identifier()`, `object_catalog_query()`, `sql_literal()`, `column_catalog_query()`, `table_objects()`, `table_columns()`
- `crates/core/src/sql/discover.rs`: `DiscoveryOptions`, `CandidateState`, `is_connectable()`, `SqlCandidate`, `Discovery`, `EngineChoice`, `engine()`, `resolve_engine()`, `engine_from_packages()`, `ConfigEntry`, `SkippedKey`, `ConfigRead`, `read_dotnet_config()`, `discover()`, `read_value()`
- `crates/core/src/sql/dotenv.rs`: `EnvValue`, `is_usable()`, `as_written()`, `EnvEntry`, `EnvProblemKind`, `EnvProblem`, `EnvFile`, `get()`, `parse()`, `classify_value()`
- `crates/core/src/sql/driver/caps.rs`: `Limits`, `Flow`, `CapTracker`, `new()`, `record()`, `is_full()`, `confirm_more()`, `rows()`, `bytes()`, `cap()`, `value_bytes()`, `row_bytes()`, `charged_cell_bytes()`, `charged_row_bytes()`, `clip_text()`
- `crates/core/src/sql/driver/mod.rs`: `stop_channel()`, `ReadOnlyEnforcement`, `EnforcementStrength`, `label()`, `detail()`, `strength()`, `refuses_writes_before_they_run()`, `is_driver_enforced()`, `declared_read_only_enforcement()`, `expected_enforcement()`, `ErrorStage`, `DriverError`, `new()`, `connect()`, `execute()`, `connect_timed_out()`, `to_event()`, `describe_duration()`, `within_connect_deadline()`, `ConnectSpec`, `StatementOutcome`, `result()`, `was_stopped()`, `for_engine()`
- `crates/core/src/sql/driver/mssql.rs`: `Dialect`, `dialect()`, `keyword_pairs()`, `IntegratedAuth`, `integrated_auth()`, `auth_preflight()`, `named_instance()`, `column_type_name()`, `decode_cell()`, `numeric_text()`, `stopped_in_transaction_notice()`, `extra_result_sets_notice()`, `rollback_failed_message()`, `MssqlDriver`, `new()`, `with_connect_timeout()`, `connect_timeout()`
- `crates/core/src/sql/driver/postgres.rs`: `WireFormat`, `PgTargetError`, `message()`, `SslMode`, `to_sqlx()`, `PgKeywords`, `PgTarget`, `connect_target()`, `column_type_name()`, `PgDecoder`, `decoder_for()`, `decode_value()`, `PostgresDriver`, `new()`, `with_connect_timeout()`, `connect_timeout()`
- `crates/core/src/sql/driver/sqlite.rs`: `SqliteTarget`, `TargetError`, `message()`, `connect_target()`, `column_type_name()`, `SqliteDriver`, `new()`, `with_connect_timeout()`, `connect_timeout()`
- `crates/core/src/sql/driver/value.rs`: `to_wire()`
- `crates/core/src/sql/dsn.rs`: `SqlEngine`, `SqlAuthMode`, `SqlDisplayConfidence`, `SqlConnectionDisplay`, `sniff_engine()`, `display_form()`, `redact()`
- `crates/core/src/sql/format.rs`: `Cell`, `SqlValue`, `format_cell()`
- `crates/core/src/sql/guard.rs`: `Engine`, `RefusalReason`, `Verdict`, `Decision`, `classify()`, `guard()`
- `crates/core/src/sql/model.rs`: `SqlValue`, `is_known()`, `SqlColumn`, `RowCapReason`, `RowCap`, `SqlResultSet`, `is_complete()`, `SqlCompletion`, `completion()`, `SqlEvent`
- `crates/core/src/sql/plan.rs`: `Plan`, `Refusal`, `sentence()`, `engine_name()`, `guard_engine()`, `execution_plan()`, `refusal_reason()`
- `crates/core/src/sql/session.rs`: `StopOutcome`, `RegisterError`, `SqlSessions`, `new()`, `register()`, `finish()`, `stop()`, `stop_connection()`, `queries()`, `connections()`, `is_running()`, `len()`, `is_empty()`
- `crates/core/src/sql/store.rs`: `SecretSource`, `holds_a_secret()`, `SqlConnection`, `SqlConnectionsFile`, `sql_connections_path()`, `load()`, `save()`
- `crates/core/src/symbols/cache.rs`: `SymbolCache`, `CachedProject`, `CacheEntry`, `cache_path()`, `load()`, `save()`, `fingerprint()`, `build_cached()`, `rebuild()`
- `crates/core/src/symbols/declarations.rs`: `SymbolKind`, `Declared`, `declaration_name()`, `declaration()`, `is_identifier()`
- `crates/core/src/symbols/fuzzy.rs`: `Match`, `score()`, `rank()`
- `crates/core/src/symbols/index.rs`: `Symbol`, `SymbolIndex`, `SymbolIndexStatus`, `of()`, `Limits`, `build()`, `build_with()`, `index_file()`, `replace_file()`, `remove_file()`, `relative_to_root()`, `walk()`, `normalise()`
- `crates/core/src/symbols/search.rs`: `SearchScope`, `Query`, `HitKind`, `SearchHit`, `search()`
- `crates/core/src/tasks/mcp/answer.rs`: `McpRefusal`, `code()`, `sentence()`
- `crates/core/src/tasks/mcp/argv.rs`: `is_mcp_tasks_invocation()`, `McpTasksInvocation`, `parse_mcp_tasks_args()`
- `crates/core/src/tasks/mcp/execute.rs`: `Outcome`, `mutated()`, `apply()`
- `crates/core/src/tasks/mcp/install.rs`: `tasks_mcp_entry()`, `status_for()`, `install_plan_for()`, `uninstall_plan_for()`, `caveats_for()`
- `crates/core/src/tasks/mcp/render.rs`: `outcome()`, `list()`, `line()`, `detail()`
- `crates/core/src/tasks/mcp/serve.rs`: `negotiated_version()`, `initialize_result()`, `tools_list_result()`, `Route`, `route()`, `success()`, `failure()`, `parse_failure()`
- `crates/core/src/tasks/mcp/tools.rs`: `descriptors()`, `ToolCall`, `parse_call()`, `text_result()`, `refusal_result()`
- `crates/core/src/tasks/mod.rs`: `TaskStatus`, `TaskOwner`, `Task`, `TasksFile`, `default_version()`, `create()`, `update()`, `set_status()`, `set_owner()`, `remove()`
- `crates/core/src/tasks/store.rs`: `tasks_path()`, `load()`, `save()`
- `crates/core/src/testing/changecov.rs`: `UncoveredLine`, `FileChangeCoverage`, `ChangeCoverage`, `map_change_coverage()`
- `crates/core/src/testing/coverage.rs`: `FileCoverage`, `parse_cobertura()`, `parse_lcov()`, `load_report()`, `newest_cobertura()`
- `crates/core/src/testing/jest_like.rs`: `parse()`
- `crates/core/src/testing/junit.rs`: `parse()`
- `crates/core/src/testing/mod.rs`: `parse()`, `parse_file()`
- `crates/core/src/testing/tree.rs`: `build()`, `failed_names()`
- `crates/core/src/testing/trx.rs`: `parse()`
- `crates/core/src/tool_gate/mod.rs`: `ServerId`, `id()`, `label()`, `descriptors()`, `from_id()`, `ToolGateFile`, `is_enabled()`, `set()`, `list()`, `filter_descriptors()`, `disabled_tool_sentence()`, `disabled_tool_result()`, `McpServerToolsInfo`, `McpToolInfo`
- `crates/core/src/tool_gate/store.rs`: `mcp_tools_path()`, `load()`, `load_existing()`, `save()`
- `crates/core/src/workspace.rs`: `Workspace`, `should_skip()`, `source_walker()`, `launch_profiles()`, `ScanOptions`, `scan()`, `workspace_from_dir()`, `scan_with()`, `find_project()`, `dotnet_test_context()`, `configs_by_project()`
