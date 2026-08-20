# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

---

## CRITICAL: Memory Files

**ALWAYS update the per-work-item memory files when relevant.** Memory is scoped **per feature/bug** under `.memories/features/{feature-name}/` or `.memories/bugs/{bug-name}/`, not at the `.memories/` root. These files track work item state across sessions:

| File | Path | Purpose | When to Update |
|------|------|---------|----------------|
| `work-item.md` | `.memories/features/{feature-name}/work-item.md` or `.memories/bugs/{bug-name}/work-item.md` | The feature work item details, ACs, description | When loading or refreshing work item context |
| `plan.md` | `.memories/features/{feature-name}/plan.md` or `.memories/bugs/{bug-name}/plan.md` | Implementation plan for the feature or bug fix | When planning or revising the approach |
| `related-docs.md` | `.memories/features/{feature-name}/related-docs.md` or `.memories/bugs/{bug-name}/related-docs.md` | Pointers to relevant documentation | When discovering docs that inform the work |
| `notes.md` | `.memories/features/{feature-name}/notes.md` or `.memories/bugs/{bug-name}/notes.md` | Issues, gotchas, lessons learned **for this work item** | When debugging/solving something others might hit on this WI |
| `todos.md` | `.memories/features/{feature-name}/todos.md` or `.memories/bugs/{bug-name}/todos.md` | Remaining tasks and tech debt **for this work item** | When adding, completing, or deprioritizing tasks |
| `completed.md` | `.memories/features/{feature-name}/completed.md` or `.memories/bugs/{bug-name}/completed.md` | Completed work record (files touched, root cause, fix) | When finishing the work item (or a major phase of it) |

**Rules:**
1. Update these files **AT ALL TIMES** under the active work item folder — they are that work item's memory.
2. Update `completed.md` immediately after finishing a task (not at end of session).
3. Update `todos.md` to check off completed items and add new discovered tasks.
4. Update `notes.md` with any issue you debug/solve that others might hit.
5. Keep entries concise but descriptive — future you needs to understand.
6. Periodically prune `todos.md` to remove old completed items.
7. Periodically summarize and prune `completed.md` to keep the file size small.
8. **Cross-work-item patterns** (gotchas that recur across multiple work items) belong in `CLAUDE.md` (root or the relevant per-project `CLAUDE.md`), not in any single work item's `notes.md`.

## What this is

`code-basics` is a lightweight Rider/IDE replacement: a Tauri 2 desktop app for running projects, running tests, and doing git work (staging, line-level revert, history) across .NET and JS/TS workspaces.

## Commands

pnpm is the package manager (`pnpm-lock.yaml` is the tracked lockfile; `tauri.conf.json` runs `pnpm dev`/`pnpm build`). Don't introduce `package-lock.json`.

- `pnpm tauri dev` — run the full app (starts Vite on port 1420 + the Rust shell)
- `pnpm dev` — frontend only in a browser (Tauri `invoke` calls will fail)
- `pnpm typecheck` — TypeScript check (`tsc --noEmit`); `pnpm build` typechecks then builds
- `pnpm sidecar:build` — publishes the C# inspector into `src-tauri/resources/inspector/` (see the third gotcha below — nothing else runs this for you)
- `pnpm test` — frontend unit tests (vitest, node environment, over the pure `*Logic.ts` modules)
- `pnpm coverage` — frontend tests with coverage; enforces ≥70% lines over the logic modules
- `cargo test -p cb-core` — all core Rust tests (unit tests + `crates/core/tests/{git_operations,intent_attribution}.rs`)
- `cargo test -p cb-core <name>` — a single test by substring
- `cargo llvm-cov --workspace --summary-only --fail-under-lines 70 --ignore-filename-regex "src.tauri.src.main\.rs|process.kill\.rs|bin.fake_lsp\.rs"` — the Rust coverage gate (needs `cargo install cargo-llvm-cov` + `rustup component add llvm-tools-preview`; first run rebuilds into its own target dir). **Run all cargo commands from a shell with `sh` on PATH (Git Bash)** — the `process::` tests spawn `sh` and fail (or, before their timeout guards, hung) without it
- `cargo build` / `cargo clippy` / `cargo fmt` — workspace-wide (both crates). There is no `rustfmt.toml` or `clippy.toml`; stock defaults, and the tree was formatted wholesale in `6b481d7` — keep it that way. Toolchain floor is `rust-version = "1.82"`, edition 2021.

**These commands are the entire quality gate.** There is no CI — no `.github/` directory exists at all. There is no ESLint, no Prettier, and no `.editorconfig`; the frontend is checked by a strict `tsc --noEmit` plus the vitest suite (`pnpm test`), which covers only the extracted `*Logic.ts` modules — rendering code has no tests. Nothing runs after you push, so run them before you claim done.

**A local `Stop` hook enforces part of this gate automatically.** `.claude/settings.json` registers `cb-app.exe quality-gate` — a self-invoking subcommand, exactly like `record-intent`, not a shipped script. It runs when an agent turn ends and, deterministically (no model reasoning involved): runs `pnpm typecheck` if any `*.ts`/`*.tsx` changed and `cargo fmt --check` if any `*.rs` changed, **blocking the turn** (exit 2, feeding the failure back) until they pass; blocks on any changed file still carrying an unresolved `AI-REJECTED YYYY-MM-DD` note (the same thing the git `pre-commit` hook refuses, surfaced earlier); and prints a **non-blocking** reminder when a turn edited source but touched no `.memories/` file. Heavier Rust checks (`cargo clippy`) are opt-in via `CB_GATE_FULL=1` — off by default because they relink and can hit the "app is running ⇒ Access denied" lock. All decisions live in `crates/core/src/qgate/` and are unit-tested there (`cargo test -p cb-core qgate::`); the thin runner is `src-tauri/src/qgate_run.rs`. It is **installable from the app** like the intent hooks (Changes → Intent → *Set up agent intent capture* → **Quality gate**), so it depends on a built `target/release/cb-app.exe` — rebuild release after changing `qgate/`. The hook complements, and does not replace, running the full commands yourself before claiming done.

### Three build failures that are not code failures

All three look like broken code, all three have wasted real time, and the obvious reaction to each is wrong.

- **`cargo build` fails on `cb-app.exe` with `Access is denied. (os error 5)` and no compiler diagnostic.** The app is running and Windows will not let a running exe be replaced. Compilation is fine. Prove it with `cargo check --workspace --all-targets` or a build into a scratch `CARGO_TARGET_DIR`, and say the app is holding the file — do not kill it, since that discards whatever the user had open.
- **`cargo test -p cb-core` appears to hang in the `process::` tests.** Concurrent cargo invocations block on the shared `target/` lock, and those tests spawn real child processes late in the run, so the stall lands somewhere plausible. The suite finishes in about a minute. Re-run before reporting the suite as broken — a subagent's build report is a claim, not evidence. If you give a parallel agent its own `CARGO_TARGET_DIR` to dodge the lock, **read the disk-space rule below first**.

### A private `CARGO_TARGET_DIR` costs 2–6 GB. Budget it.

This has bitten twice, and both times nobody noticed until the disk did.

A target directory for this workspace measures **2–6 GB** (≈4 GB typical, and `debug/deps` alone reaches 39 GB once it has accumulated). One per parallel agent is therefore *tens of gigabytes per workflow*, and they do not clean themselves up. Two real incidents: 48 abandoned target dirs under the user's temp directory totalling well over 100 GB, and 13 more — ~40 GB — left **inside this repository's own `target/`** as `target/wf-*` by an earlier session.

The rules, in order of preference:

1. **Prefer one shared target dir.** The lock serialises builds; it does not break them. Serialised cargo is usually cheaper than 40 GB of parallel cargo.
2. **If you do split, reuse a small fixed set of paths** (one per agent *role*, reused across runs) rather than a fresh directory per agent per workflow. A warm reused dir is also far faster than a cold new one.
3. **Never point `CARGO_TARGET_DIR` inside the repository.** `target/wf-*` is invisible to `du` habits, survives `git clean` (`/target` is gitignored wholesale), and is what produced the 40 GB above.
4. **Delete them when the work finishes.** Removing a target dir costs a rebuild and nothing else — there is no state in there worth keeping. Treat cleanup as part of the task, not as tidying.
5. Read-only work (`cargo check`, reading code, running an existing binary) needs no private target dir at all.

Before starting a workflow that will build in parallel, do the arithmetic out loud: *agents × 4 GB*. If the answer is bigger than a few tens of gigabytes, use fewer agents or share the directory.

### `target/` grows without bound, and it is `deps/` that does it

Third incident, and the largest: the repository reached **62 GB**, of which `target/debug` was 53 GB and `target/debug/deps` alone was **45 GB**. A further **34 GB** sat in four private agent target dirs under the temp directory (`p4-gate`, `p5-a`, `p5-b`, `p5-c`) — created despite the rule above, which is why the rule now has a verification step.

**Why it grows.** `cargo` never garbage-collects `deps/`. Every compilation leaves its artifacts behind under a content hash, and nothing prunes the old ones. This workspace now has **eight** test targets (six integration files plus the lib and the `cb-fake-lsp` bin), each linking its own binary with debuginfo on every run. A session with thirty full `cargo test` runs therefore leaves thirty generations of them.

**Measured, on this workspace, by building all test binaries from a clean `target/debug`:**

| `[profile.dev] debug` | one clean build of every test binary |
|---|---|
| `true` (the cargo default) | **6.1 GB** |
| `"line-tables-only"` (now set) | **3.12 GB** |

So `Cargo.toml` sets `[profile.dev] debug = "line-tables-only"`, which halves the per-build cost. Verified that it keeps what this repository actually reads: a panic still reports `panicked at crates\core\src\lsp\framing_tests.rs:238:5` and the backtrace still carries symbol names. Only interactive-debugger information is lost.

**Halving it does not stop it.** The growth is accumulation, so the durable fix is periodic deletion, and that is a maintenance task somebody has to actually do:

```sh
# Reclaim everything reclaimable. Costs a rebuild and nothing else.
rm -rf target/debug target/llvm-cov-target target/tmp
```

Three things to know before running it:

1. **Do not use `cargo clean`, and do not delete `target/release`.** The `Stop` hook that records intents runs `target/release/cb-app.exe record-intent`. Wiping the release profile silently breaks intent capture until somebody happens to run a release build — a failure whose cause is nowhere near its symptom.
2. **`target/llvm-cov-target` is a second complete build tree** (6 GB when last measured), created by `pnpm coverage` / `cargo llvm-cov`. It is pure cache; delete it after a coverage run.
3. **Check for stray private target dirs, do not assume there are none.** They are outside the repository, so no amount of looking at `git status` or the repo's size will show them:

```sh
ls -d "$TEMP"/claude/*/target 2>/dev/null   # then delete what you find
ls -d target/wf-* target/agent-* 2>/dev/null
```

Do this check whenever a workflow that ran cargo finishes. Agents have been told not to create these and have created them anyway in three separate sessions; the instruction is not sufficient on its own.
- **The Objects tab is dead in a fresh clone and nothing reports an error.** `pnpm tauri build` runs `beforeBuildCommand: "pnpm build"`, which does not chain `pnpm sidecar:build`, and `src-tauri/resources/inspector/` is gitignored. With no sidecar present `inspect_status` reports the feature unavailable — by design, since missing .NET is not a build failure, but it means a clean checkout ships an inert tab. Run `pnpm sidecar:build` manually before bundling. Under `pnpm tauri dev`, `CB_INSPECTOR_PATH` (a directory or a single binary) overrides the bundled copy.

## Tests first — not optional

Write the failing test before the implementation. `cb-core` exists precisely so that every decision is testable headlessly; if something is hard to test, it is in the wrong layer, not untestable.

- **Every change to `crates/core` starts with a test.** Add it to the sibling `*_tests.rs` file, or to `crates/core/tests/` for a cross-module scenario. Watch it fail for the right reason, then implement. A test that passes the first time you run it has proved nothing.
- **Run it before and after** — `cargo test -p cb-core <name>` for the loop, `cargo test -p cb-core` before you say you are done.
- **Bug fixes begin with a reproduction.** The test must fail on the current code and name the symptom, not the fix.
- **Changing a type that crosses IPC?** Update the key-pinning test in `model.rs` first, then `types.ts` (see the IPC contract section below).
- **Rust decisions are tested in `cb-core`; frontend pure logic in `*Logic.ts` + vitest.** `src-tauri` has only the `state.rs` tests, and there is no CI. A frontend helper that makes a decision (parsing, classification, index math) gets extracted into a co-located `*Logic.ts` module with a `.test.ts` beside it — components stay untested rendering shells. Anything bigger than a display helper still belongs in `cb-core`.
- **Never delete or weaken a failing test to get green.** Either the test is wrong and you say so explicitly, or the code is. If a test encodes surprising behaviour, characterise and document it rather than "fixing" it.
- Report results honestly: paste the failure, do not summarise it away.

## Architecture

Three layers with a strict dependency rule:

1. **`crates/core` (`cb-core`)** — all decision-making logic, deliberately with **no Tauri dependency** so everything is unit-testable headlessly:
   - `workspace.rs` — scans an opened directory for projects. Filesystem-only detection (no MSBuild/npm invocation); skips `SKIP_DIRS`, max depth 10. A manifest that will not parse produces a project carrying `unreadable: Some(reason)` — listed but inert (no configurations, `ProjectKind::Unknown`) — rather than being dropped, because a shorter list is indistinguishable from a correct one.
   - `adapters/` — per-ecosystem knowledge (`dotnet.rs`, `node.rs`, `cargo.rs`): how to detect a project and build the command line to run/test/build it. `manifest.rs` adds declarative TOML adapters loaded from `.code-basics/adapters/*.toml` in a workspace (see `examples/adapters/` for pytest and cargo-nextest); any runner that emits JUnit XML can be added without Rust code. .NET: no launch profile means `dotnet run`'s default profile applies (`ignore_launch_settings` opts out); detected test configs are Debug-only. **Cargo is detection-only**: `cargo.rs` reads a `Cargo.toml` and emits **no** run or test configurations at all, because doing so would change the Run and Tests tabs for every Rust repository on the next scan. It exists so the architecture graph has nodes with `ecosystem == "cargo"`; `examples/adapters/cargo-nextest.toml` remains the way to actually run Rust, and the manifest-adapter merge in `workspace.rs` deliberately lets a declarative adapter supply configurations for a directory cargo already claimed (any other built-in ecosystem still shadows it).
   - `testing/` — parses test report files (`trx`, `junitXml`, `jestLike` formats). The core design: runners stream raw output live to the console, then the test tree is built from the structured report file written at exit.
   - `git/` — libgit2 (`git2`) operations including partial staging and line-level revert via patch manipulation (`patch.rs`). Stash management (`stash_list`/`save`/`apply`/`pop`/`drop`/`clear`, indexed by `stash@{n}`) treats a stash as the commit it is, so `StashEntry.id` is that commit's oid and the UI previews a stash through the ordinary `commit_diff`/`commit_file_contents` path rather than any stash-specific one. `attribution.rs` matches recorded agent edits onto the current diff **by content only** (recorded line numbers are deliberately discarded), and `grouping.rs` collapses hunks into intent cards: stated intent, then formatting-only, then enclosing symbol. Governing rule: a wrong label is much worse than no label, so every threshold abstains rather than guesses. **libgit2 ref writes are not safe to run concurrently:** every branch delete rewrites the shared `packed-refs`, so firing N `git_delete_branch` calls at once makes them race — one blocks on `packed-refs.lock` (`code=Locked`) while another reads a half-rewritten `packed-refs` (`could not read (expected N bytes, read N-k)`). Any bulk ref operation must be **serialised** (the History tab's `historyLogic.bulkDeleteBranches` awaits each delete in turn for exactly this reason); a branch git legitimately refuses — not fully merged, or checked out in a linked worktree — is a separate, real failure to report, not the same bug.
   - `intents/` — what a coding agent said it was doing, from Claude Code and Codex. Neither agent can attach a rationale to a tool call, so a `PostToolUse` hook records the geometry and a `Stop` hook records the reason, joined on the turn id both carry (`.code-basics/intents/`, gitignored). `providers/` also mines each agent's existing session files so the feature works retroactively with no setup. Hook installs are always additive and previewed before writing — they touch files the user shares with their team.
   - `inspect/` — reads the real managed heap of a crash dump or a live process via the `cb-inspector` sidecar: one-shot, request file in, `result.json` out, so `process/` needed no changes. `dumps.rs` arms the runtime's own `DOTNET_Dbg*` crash-dump capture (opt-in per workspace, off by default — a dump is a verbatim copy of process memory) and prunes by count and bytes. Same abstain rule: unreadable becomes `Unavailable`, a cap becomes `Elided`. No method is called and no property evaluated; addresses cross as hex strings. Live attach lists **every** .NET process on the machine (`cb-inspector --list-processes` → `DiagnosticsClient.GetPublishedProcesses()`), because `dotnet run` starts the application as a child and the supervisor's pid is the CLI launcher. `session::attribute` labels each row `launched`/`descendant`/`unrelated` from the parent chain — an unrelated row never carries a configuration name, a missing parent pid is *unknown* rather than "no parent", and a chain leaving the known set stops. Builds, git and the sidecar are dropped entirely. The pid is re-checked against a fresh enumeration immediately before spawning, because a recycled pid would render a stranger's heap under the user's label.
   - `symbols/` — the index behind the search palette. `declarations.rs` is pure text (one line in, a name and `SymbolKind` out) and is what `git/grouping.rs` uses for enclosing symbols — the dependency points that way and must stay that way. `fuzzy.rs` is pure arithmetic and **rejects** non-matches rather than ranking them low, which is what lets files, symbols and configs merge into one list. `index.rs` walks via `workspace::source_walker` so the index and the project scan can never disagree; extensions are an allowlist and files over 1 MiB are listed but not parsed. `cache.rs` persists to `.code-basics/symbols.json`, fingerprinting on mtime+length only (never content — hashing is the cost the cache exists to avoid) plus the project list; `rebuild` is the escape hatch. `search.rs` is the only combining layer and the **only** place the `Foo:123` line suffix is parsed. Same abstain rule: unrecognised yields no symbol, an unplaceable kind is `Other` (so: no badge), and a path-fallback match carries no highlight positions.
   - `architecture/` — the derived project graph behind the diagrams, computed **on demand from the manifests** and deliberately never cached on `Project` (a wire type everything holds, whose reference list would go stale silently while the user edits). `graph.rs` derives nodes and edges from `<ProjectReference>`, `package.json` dependency names, cargo `path` dependencies, `.sln` grouping, npm `workspaces` globs and cargo `[workspace] members`/`exclude` globs — globs are matched against **discovered project directories, never the filesystem**, so membership cannot drift from `source_walker`. Cargo edges resolve by *path* (which is what cargo itself does) rather than by name, and cargo globs resolve against the manifest's own directory rather than the workspace root. `mermaid.rs` renders and validates; `store.rs` owns `.code-basics/diagrams/`, where the directory is a function of the derivation (so derived output cannot be committed, hand-drawn work cannot be overwritten) and provenance lives in hand-parsed front matter rather than a `%%` comment a formatter could eat. The abstain rule is **sharper here than anywhere else — an arrow is a much stronger claim than a hunk label**: lookups never cross ecosystems, a reference resolving to nothing invents no node, a casing-only near miss is reported rather than matched, an ambiguous package name drops the edge, and everything that could not become an edge lands in `ArchGraph::warnings` instead of vanishing. Third-party npm packages are the one deliberate silence.
   - `architecture/signals/` + `components.rs` — the *component* map: services and the data stores they declare, which is inference rather than manifest literals and so carries a stricter gate than anything above it. Every candidate is a `Signal` with a `Strength`, and the strength decides what it may do: **HIGH may create** a node or an edge and is only ever a fact the author wrote down (a manifest, a config file, a filename); **MEDIUM may only enrich** something a HIGH signal already created and may never bring one into existence; everything else is **discarded and counted** into `ArchGraph::warnings`, so a refusal is visible rather than silent. The rule lives in one function — `signals::framework::admit` — not in each producer (`dotnet.rs`, `node.rs`, `routes.rs`), because per-producer discipline lasts only as long as the most permissive producer. Enforced centrally alongside it: no connection-string *value* ever reaches a graph (the key is the author's label, everything right of the `=` is refused whole rather than redacted), no component is created from a name similarity, and nothing is inferred from a `using`/`import` line or a comment. `components.rs::component_graph` assembles the survivors into an ordinary `ArchGraph` so one renderer and one IPC type serve both maps — and a workspace with no HIGH signals yields an **empty** component map, never a fallback to the project map.
   - `lsp/` — real language servers, for find-usages and go-to-definition. This exists **next to** `symbols/`, not instead of it, and the split is the point: a text heuristic is right for a palette where a near miss costs a keystroke, and wrong for a count the user reads as permission to change a method. `symbols/` is not a fallback for this and this is not an upgrade of it. Nothing is bundled — a server is found on disk (Roslyn from the VS Code C# extension's `.roslyn/`, picked by **parsed** version so `2.140.9` beats `2.9.0`) or on `PATH` via `process::resolve_program`, and a missing one is *reported with what was looked for and how to install it*, never worked around. The lower layers are pure so the invisible mistakes are provable without a server installed: `framing` (fails closed — a stream that cannot be resynchronised is not guessed at), `jsonrpc` (a server *request* must be answered or the server hangs forever; `"id": null` is present, so reading it as a notification is that hang), `uri` (per-server colon spelling, and identity is decided on **paths** via `symbols::index::relative_to_root`, never on URI strings — four servers disagree about spelling), `positions` (UTF-16 code units ⇄ bytes, 0-based ⇄ 1-based lines; everything clamps), `protocol`, `registry`, `settings`, `documents`, `results`, `model`. Only `transport`, `client` and `session` touch a process. **The governing rule is sharper here than anywhere else: a timeout, a dead server, a server still loading, an unsupported capability, and a genuine zero are five different answers and must never collapse into one** — `Availability` has six variants for exactly that reason, `total` is an `Option` so `Some(0)` and `None` stay distinct, and there is no `skip_serializing_if` anywhere in `model.rs`. Two things learned the hard way and pinned by tests: a **one-shot** notification (`workspace/projectInitializationComplete`) is recorded as sticky state on the transport as the reader decodes it, because a `broadcast` subscriber that lags loses it and there is no next one — that bug shipped "No usages" for a method with one usage while 1901 tests passed; and shutdown must go through `kill_tree`, since Roslyn spawns `BuildHost-*` children that a parent-only kill orphans.
   - `importers/rider.rs` — converts Rider `.run/*.xml` configurations.
   - `enhancements/` — the file-driven **Enhancements** menu. Reusable snippets are plain `.md` files (front matter `id`/`title`/`placement`) in a user-owned dir (`<config>/code-basics/instructions` and a sibling `prompts/`; `CB_INSTRUCTIONS_PATH`/`CB_PROMPTS_PATH` override, and the app seeds each from a bundled resource dir without overwriting user edits). Generalises `intents/providers/instructions.rs`: an **instruction** is spliced into **both** `CLAUDE.md` and `AGENTS.md` bounded by a `<!-- code-basics: enhancement:<id> -->` marker (namespaced so it never collides with the intent block), idempotent — a re-add refreshes the span in place — and applied through `providers::apply_writes` so it inherits the `.bak` backup. `placement` resolves the insertion point (`top`/`after-first-heading`/`end`/`before-marker`/`after-marker`) and **abstains to a safe fallback** rather than guessing when an anchor is missing (no heading ⇒ top; missing marker ⇒ end). A **prompt** reuses the same parser but is never written — its body is returned for the frontend to **run as an agent** (via `start_review`, shared with the adversarial Review). A prompt may declare `once: true`; a run-once prompt is recorded per workspace in `.code-basics/agent-runs.json` (`enhancements::runs`, pure fs/serde) when it finishes successfully, and the menu badges it and confirms before re-running. All pure string/fs logic, tested headlessly in `enhancements_tests.rs` / `runs_tests.rs`.
   - `config.rs` — `.code-basics/config.json` per workspace: only user-created/imported configs are saved; detected ones are re-derived on every scan. Also holds `favorites`/`order` (config ids; favourites sort first, then saved order, then names).
   - `secrets.rs` — .NET user secrets (`%APPDATA%\Microsoft\UserSecrets\<id>\secrets.json`); validates the comment-tolerant JSON dialect .NET accepts and can add a `<UserSecretsId>` to a project.
   - `process/` — process spawning, output chunking, cross-platform kill. Layers colour-enabling env defaults under the config's own. `pid(id)`/`running()` expose what the supervisor already knew, so the inspector can attach to a live process without a second lifecycle.
   - `invocation.rs` — turns a `RunConfig` into a command line: `build()` (the single dispatch point over the adapters), `plan_compound` (compound member resolution + env layering, resolve-all-before-start-any), `rerun_filter` (the "re-run failed" guard).
   - `erosion/` — a rules-based, **no-model** scan over the diff for changes that quietly weaken the codebase (a deleted assertion, an `[Ignore]`/`.skip` test, a widened `catch`, an introduced `.unwrap()`, a `TODO` in a production path, a removed timeout, a dropped log). Each rule is **one regex against one *side*** of the diff and a match is only taken on a line of that origin, never context (getting the side backwards is how a rules scan makes noise); `rules.rs` ships built-ins per ecosystem (.cs / ts-js / .rs), **extended never shadowed** by `.code-basics/erosion/*.toml` like `adapters/manifest`. `scan.rs::scan_diffs` yields `ErosionReport { flags, warnings }` — each flag cites the exact line plus the `DiffLine::index` for the click highlight, a bad regex lands in `warnings` rather than being dropped, and the detector **ranks nothing** (a pure producer of located facts).
   - `behavioral/` — the **runtime** counterpart to the intent `git/coverage` Scorecard: run a config against `HEAD` (materialised in an isolated `git worktree`, `worktree.rs` — net-new, since every other checkout mutates the single working tree in place; `Drop`-guarded, oid-cached, and reusing the same Windows read-only-directory handling as `checkout_tree_tolerating_locks`) and against the working tree, then diff the *observable* outcomes: test results (`compare.rs`, joined by `full_name`; `Other` is never a pass), console output (`console.rs`, masking timestamps/ids/both run roots and comparing as multisets), and HTTP responses replayed from `.http` files (`httpfile.rs` pure parse + `@readiness`, `replay.rs` the only networking layer, `http.rs` diff with volatile headers ignored and JSON compared structurally). `attribute.rs` pins each delta to the one intent card whose files it points at, or abstains to an unattributed bucket; `scenario.rs`/`prepare.rs` are the pure, tested seams the untestable command delegates to. Same abstain rule, **sharpened for how weak runtime evidence is**: equal-after-masking is *no* delta, a single-run test flip is capped at `Medium`, and a never-ready server / missing-or-ambiguous launch config / no `@readiness` each become a warning rather than a fabricated result. HTTP replay is **strictly sequential** (base then work — same port) and can never hang on a server that will not exit (the detached run task is cancelled with a retry that closes the spawn/registration race, then awaited under a timeout).
2. **`src-tauri`** — thin bridge only: app state (`state.rs`) and the `#[tauri::command]` surface in `commands/{workspace,run,secrets,git,changelists,intents,files,inspect,symbols,architecture,lsp,erosion,behavioral}.rs`, registered in `lib.rs` (`behavioral_diff`/`behavioral_clear` stream a before/after run and return a `BehavioralReport`). New backend functionality goes in `cb-core` first; commands here should stay small. Config-to-adapter dispatch lives in `cb_core::invocation` — its `build()` is the **single** point that maps a `RunConfig` to an ecosystem adapter; do not add a second. `AppState` holds six things: `workspace`, the `Supervisor`, `last_test_run` (keyed by config id), `last_inspect` — one slot, not a map, deliberately, because a capture is a copy of process memory — `symbols` (an `Arc<SymbolIndex>` so a search takes a handle and drops the lock) beside a `symbols_building` flag, and `lsp` (an `LspHandle`). The LSP session is an **actor handle and not an `Arc<Mutex<..>>`**, for a reason worth keeping: `AppState` uses `std::sync::Mutex` and `set_workspace` clears its caches *while holding the guard*, so teardown cannot `.await` — `request_teardown()` is a non-blocking send, which is what keeps check-and-act one atomic step. `record_lsp_session` returns `Result<(), LspHandle>` and is `#[must_use]`, handing the handle **back** on rejection, because unlike the other caches a rejected session is a running process tree and a `bool` would let a caller leak a language server. The symbol index is built on a **background thread** and never awaited: `open_workspace`/`rescan_workspace` spawn it, the `setup` hook covers a workspace named on the command line, and `record_symbols` discards a build whose root is no longer open so opening A then B cannot serve A's paths under B. `recorder.rs` is the one non-window entry point: an agent hook re-invokes this binary as `record-intent`, which reads a payload from stdin and exits without ever creating a window.

   **A `#[tauri::command]` body is untestable here, so it must not decide anything.** Every command takes `State<'_, AppState>`, which cannot be constructed without a `tauri::App`, and the `tauri` dependency does not enable the `test` feature. So **no command function in this crate is called by any test, in any module, and none can be** — which is why a coverage pass keeps re-filing the same finding against whichever command module it looked at last (`arch_*`, then `lsp_*`). Filing it again will not change the constraint. The rule that does:

   - Command bodies resolve state, clone a handle, drop the lock, and await. That much is genuinely covered by the type checker plus the core tests behind it.
   - **The moment a body contains a decision — a `match` that maps one outcome to another, a fallback, a defaulted value — extract it to a plain free function beside the command and test that.** `commands/lsp.rs::status_for` is the worked example: it turns "no workspace open" into a successful *empty server list*, which is the exact shape this codebase refuses everywhere else, and inline it was the one line worth checking and the one line nothing could check.
   - Helpers reached from a command but not taking `State` (`ensure_session`, `ensure_session_interleaved`) are ordinary functions and are tested normally.

   Judge these modules by whether the untested lines decide anything, not by their line percentage. The percentage will stay low and that is the design.
3. **`src/` (React 19 + Vite)** — six tab views, in the order `TABS` in `App.tsx` declares them: Run, Tests, Changes, History, Architecture, Objects. Run, Tests and Objects stay mounted while hidden because they own running processes; Changes, History and Architecture are mounted conditionally, so leaving and returning re-reads from disk — which is what you want after a regeneration. The Objects tab is `views/InspectView.tsx` under the id `inspect` — label and filename differ, so grepping for "Objects" will not find it; **`architecture` deliberately does not repeat that**, its id matching its label. The Architecture tab is `views/ArchitectureView.tsx` over `views/architecture/` (`DiagramCanvas` renders Mermaid and owns pan/zoom + click-to-open, `DiagramEditor` is a CodeMirror editor over a stored diagram). Mermaid is loaded with `await import("mermaid")` and must stay out of the entry chunk; it needs a **top-level** `htmlLabels: false` (not just `flowchart.htmlLabels`) and `securityLevel: "strict"` to satisfy the CSP, and `click … call` is banned because it requires `securityLevel: "loose"`. `ArchGraph.warnings` must reach the UI — the deriver reports every reference it refused to draw there, and a picture that silently omits them looks complete and is not. Also: a titlebar branch widget, a menu bar (`components/MenuBar.tsx` — a **File** menu and an **Enhancements** menu with fly-out **Add Instructions**/**Run Agent** submenus over `cb_core::enhancements`; instructions confirm inline before writing and reuse `components/enhancementsLogic.ts`, and a prompt runs as an agent in the shared `ReviewPanel` — a run-once prompt is badged from `agent_runs` and confirms before re-running), CodeMirror-based diff/config editors and the xterm console (`components/`), and typed IPC wrappers in `ipc/api.ts`. The Changes tab (`views/ChangesView.tsx`) toggles its list between **Files**, **Intent** (`components/IntentPanel.tsx` + `intentPanelLogic.ts` — each card and file carries a staged/partial badge derived from `git status`, since this view has no Staged section of its own) **Stashes** (`components/StashPanel.tsx` + `stashLogic.ts`, the stash manager: a list with a read-only preview reusing the History diff cascade, and create/apply/pop/drop/clear) and **Erosion** (`components/ErosionPanel.tsx` + `erosionLogic.ts`, the `cb_core::erosion` scan grouped by category, each flag clicking through to its diff line). Terminal-hosting panes must be `overflow: hidden` (a scrollbar fights xterm's fit addon). Pure decision helpers live in co-located `*Logic.ts` modules (e.g. `views/testsLogic.ts`, `components/consoleLogic.ts`) with vitest tests beside them — extract into those rather than growing logic inside a component. That rule is load-bearing for the usages UI in particular: **vitest runs in the node environment, so there is no DOM**, and the CodeMirror and React halves cannot be tested at all. So every decision lives in `components/usagesLogic.ts` and `components/lspStatusLogic.ts` — the phrasing for each `Availability`, whether a row is clickable, the grouping, and the jump-versus-picker rule (which counts distinct **destinations** by `(path, line)`, since a static method is its own implementation and both groups answer with one location) — while `components/usagesExtension.ts` holds the `StateField`/block-widget/`domEventHandlers` plumbing and decides nothing. Two CodeMirror specifics: the block widget's `eq()` must compare rendered text and actionability or the row rebuilds its DOM on every keystroke and loses its click target, and middle-click must `preventDefault()` on **mousedown** (not `auxclick`) or Windows starts the autoscroll cursor. Tab-adjacent, not a tab: `components/SearchEverywhere.tsx` is a full-app overlay mounted whenever a workspace is (its window-level, capture-phase key listener is the only way to reach it) — Shift Shift / Ctrl+N / Ctrl+Shift+N / Ctrl+Shift+A, with the whole binding table as one expression in `searchLogic.ts`. It acts through `App`, which now holds a **second** piece of cross-view state beside `inspectRequest`: `openRequest`/`selectRequest`, the same request-and-consume shape, carrying a monotonic `token` because a jump into an already-open file changes no field the consumer could compare. The Intent view also renders the **behavioral before/after evidence** beside the static coverage Scorecard — a per-card badge for the deltas attributed to that card plus an overall panel for the run and the unattributed bucket, every decision in the tested `components/behavioralPanelLogic.ts`. And every CodeMirror editor (file, diff — both panes, diagram) carries `@codemirror/search`, so **Ctrl+F** finds within a file; the console's own capture-phase Ctrl+F is guarded to fire only while the console is visible (`offsetParent`), so the two never contend.
4. **`sidecar/inspector/`** — the one component not written in Rust or TypeScript: a .NET one-shot process (`cb-inspector`) that walks a heap with ClrMD and writes `result.json`, plus a second mode (`--list-processes`) that enumerates the machine's attachable .NET processes. It makes no product decisions — the platform call is here, the attribution is in `cb-core`. `pnpm sidecar:build` publishes x64 and x86 (~4 MB each) into `src-tauri/resources/inspector/`, shipped via `bundle.resources` (which also carries the hand-authored `instructions/` and `prompts/` enhancement defaults); everything else is found on `PATH`. Not built by `cargo build`, not covered by `cargo test`, and **absent from `docs/INDEX.md`** because `generate-index.mjs` does not read C#. Missing .NET is not a build failure: the feature reports itself unavailable.

### The Rust↔TS type contract

`src/ipc/types.ts` hand-mirrors the Rust model types (`crates/core/src/model.rs`, `workspace.rs`, `git/`) — there is no codegen wired up (specta derives exist but are unused for export). Rust structs use `serde(rename_all = "camelCase")`, and the `tests` module at the bottom of `model.rs` pins the exact JSON keys each type produces (the `types.ts` comment calls this `serialisation_shape`; the tests exist under different names). **When changing a Rust type that crosses IPC, update the key-pinning test in `model.rs` and `types.ts` together.** Full rules: `docs/architecture/ipc-contract.md`.

## Documentation and the code index

- `docs/INDEX.md` is a **generated** map of every source file (with one-line purpose), the full Tauri command surface, the `ipc/api.ts` wrappers, and each `cb-core` module's public API. **Consult it first when locating code** — it is usually faster than searching. Never edit it by hand.
- `pnpm docs:index` regenerates it (`scripts/generate-index.mjs`). Run it after adding/removing source files, Tauri commands, or public core APIs.
- `pnpm docs:check` (`scripts/check-docs.mjs`) enforces the docs rules: every markdown file in `docs/` (plus README.md and this file) stays under 500 lines, and all relative links resolve. Run it after editing docs.
- Human docs live in `docs/` grouped by directory (`getting-started/`, `architecture/`, `guides/`, `reference/`), entry point `docs/README.md`. When changing behavior documented there (commands, config format, adapter schema), update the matching doc — `docs/reference/commands.md` in particular must track `generate_handler!` in `src-tauri/src/lib.rs`.

### Other conventions

- Rust tests for adapters live in sibling `*_tests.rs` files included via `#[path]` under `#[cfg(test)]`.
- Streaming process output uses Tauri `Channel<ProcessEvent>` (see `startRun` in `ipc/api.ts` and `commands/run.rs`).
- Per-workspace state (config, declarative adapters, test report output) all lives under `.code-basics/` in the opened workspace.
- The app accepts a directory argument (`code-basics .`); a bad path logs and still opens the welcome screen.
- The release profile sets `panic = "abort"` (plus `lto`, `codegen-units = 1`, `opt-level = "s"`, `strip`). Nothing may rely on `catch_unwind` to recover.

<!-- code-basics: agent intent -->
## Recording why you changed something

When you finish a turn in which you edited files, end your reply with one line
saying why:

```
Intent: <3-5 words describing why>
```

If the turn made unrelated changes, scope each reason to its files instead of
writing several plain lines — only the first plain line is used. Paths are
workspace-relative, comma-separated:

```
Intent(src/api.ts, src/apiLogic.test.ts): <why, for those files>
```

Each entry is a workspace-relative path with forward slashes — a specific file,
or a directory to cover everything you edited beneath it (e.g.
`Intent(src/components): …`). Prefer naming the specific files; reach for a
directory only when the turn's edits are one cohesive set under it.

A scoped line covers the files it names; one plain line may cover the rest.
Keep each label short enough to read at a glance — it titles a group of hunks
in the Changes tab, not a commit message.

## Rejected changes

A comment beginning `AI-REJECTED` means that change was reverted during review
and the reason is on the following line. It is a task, not documentation:

1. Implement a correct version that addresses the stated reason.
2. Delete the whole `AI-REJECTED` comment block in the same edit.

Do not reinstate the reverted code as it was, and do not leave the comment
behind once the code is right — a commit that still carries one is refused by
a `pre-commit` hook.
<!-- /code-basics -->
