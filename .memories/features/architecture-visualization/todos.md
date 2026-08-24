# Todos

## RECONCILED 2026-08-21 — read this first; it supersedes the open boxes below

The round-by-round sections below are kept for provenance, but many `[ ]` items
in them were completed by the Phase 4 UI rounds and never ticked, so the file
contradicted the tree. Verified against the actual code on 2026-08-21:

**DONE (ignore any open box below that claims otherwise):**
- The Architecture tab is fully wired and shipped. `ArchitectureView.tsx` /
  `views/architecture/` call `archProjectGraph`, `archRenderGraph`,
  `componentGraph`, `archListDiagrams`, `archReadDiagram`, `archWriteDiagram`,
  `archValidate` — the "six wrappers with zero callers" / "no UI calls" /
  "component_graph not reachable" / "Phase 5 entirely frontend, unstarted" items
  are all **stale**.
- **Phase 0 CSP spike is settled.** `DiagramCanvas.tsx` sets top-level
  `htmlLabels: false` (plus per-diagram) and mermaid renders under the strict
  production CSP. Ship branch was taken.
- `mermaid` is imported (lazily) — the "installed and imported nowhere" note is stale.
- Level 2 signals / component map (Phase 4) shipped with the centralized
  `signals::framework::admit` gate and the connection-string-value test.

**GENUINELY STILL OPEN (the real remainder):**
- **Agent-authored diagrams (old "Phase 4"): not implemented.**
  `architecture/prompt.rs::prompt_for` does **not** exist and no `diagrams.agentCommand`
  is wired (verified 2026-08-21). `validate` + front-matter provenance did land.
- ~~**Aspire `Projects.` identifier transform** — still unverified.~~ **DONE (WF1,
  2026-08-24, B6):** verified correct; pinned end-to-end by
  `an_aspire_add_project_resolves_a_hyphenated_project_name` + an ambiguity guard.
- ~~**`pnpm-workspace.yaml`** — half-supported on purpose~~ **DONE (WF3, 2026-08-24, B7):**
  added `yaml-rust2`, containment drawn from `pnpm-workspace.yaml` `packages:` globs only
  (package.json `workspaces` stays suppressed, matching pnpm); `SCANNER_VERSION` 2→3; the two
  old abstain-pinning tests inverted.
- ~~**`mermaidIdOf` (TS) mirrors `mermaid_id` (Rust)** with no cross-language drift guard.~~
  **DONE (WF1, 2026-08-24, B8):** cb-core `mermaid_id_matches_committed_fixture`
  generates+pins `fixtures/architecture/mermaid_ids.json`; vitest consumes it via a
  JSON import (no `node:fs` — see notes.md).
- ~~**No project→project arrow** for the MEDIUM `AddHttpClient`/`BaseAddress` evidence~~
  **DONE (WF3, 2026-08-24, B9):** new `EdgeKind::ServiceCall` — the caller's `AddHttpClient`
  `BaseAddress` matched against a callee's `launchSettings.json` `applicationUrl` (HIGH,
  config-file evidence) draws a service→service arrow; never invents a node (guarded at draw
  time in components.rs), evidence elided so no URL/credential leaks, ambiguous → warning.
  Mermaid `--x` glyph + legend row; two "never drawn as an arrow" tests inverted.
  **Rendering in the running app is still a manual-verification item.**
- **Live verification:** a *non-empty component map* has never been produced from
  real code (this repo yields an empty one); clicking a box / opening a stored
  diagram in the built app is still the manual check from the last fix round.
- Doc-comment safety-property sweep in `architecture/` (2-for-2 wrong so far).

---

## Immediate follow-ups from Phase 1

- [x] ~~**Mirror `SearchHit` / `SearchScope` / `HitKind` into `src/ipc/types.ts`.**~~
      Done in Phase 2, together with `SymbolKind` and `SymbolIndexStatus`.
- [ ] Decide whether `mermaid@11.16.1` stays in `package.json` if Phase 0 fails.
      It is currently installed and imported nowhere.
- [x] ~~`MAX_DEPTH = 10` hides files from the palette~~ **DONE (WF1, 2026-08-24, C5):
      raised to 50** (Anthony's call). SKIP_DIRS + the 50k-file cap keep the deeper
      walk bounded; pinned by `a_source_file_nested_deeper_than_ten_levels_is_indexed`
      (symbols) + `a_project_nested_deeper_than_ten_levels_is_discovered` (workspace).

## Carried out of the fix round (2026-08-10)

Phase 1 + the fix round are **done and green** (1133/44/2+1/11 Rust, 229 vitest).
What the round left behind:

- [ ] **Re-index existing workspaces once.** `HEURISTIC_VERSION` is now `2`, so
      any `.code-basics/symbols.json` written before the fix is discarded and
      rebuilt on next open. Nothing to do in code — but the first open of a large
      solution after this change pays the cold-build cost again (9 s+ on the
      2864-file target), which will look like a regression if it surprises you.
- [ ] **The generic-naming fix also changed `grouping.rs` output.** Intent-card
      titles in the Changes tab were being named after types too. No cache to
      invalidate there — cards are recomputed — but the visible behaviour of an
      already-shipped feature changed, and nobody has looked at it in the running
      app. Worth one manual pass over a branch with C# property changes.
- [ ] **Audit the remaining "two paths must agree" tests for the None-vs-None
      trap.** The stale `project_id` survived because the invariant test's
      fixture could only produce one value for the field under test. That failure
      mode is not specific to `cache.rs`; check any equivalence test whose
      fixture is deliberately minimal.

## Phase 0 — CSP spike (blocks everything visual)

- [ ] **Correct `plan.md` line 12 before running the spike.** It specifies
      `flowchart.htmlLabels: false`; mermaid 11.16.1's own
      `config.type.d.ts` says diagram-specific `htmlLabels` is **deprecated** and
      the **root-level `htmlLabels` takes precedence**. Set top-level
      `htmlLabels: false`. Setting only the flowchart one can be silently
      overridden, and under a strict CSP that is not cosmetic — HTML labels are
      the markup-injecting path.
- [ ] Render `flowchart`, `sequenceDiagram`, `classDiagram`, `erDiagram`,
      `stateDiagram-v2` under the real production CSP — via `pnpm tauri build`
      or a `<meta http-equiv>` copy of the policy, **not** `tauri dev`.
- [ ] Record the outcome and pick the branch (ship / allowlist / own renderer).
      Note `htmlLabels: false` settles only the *labels*; whether mermaid needs
      `eval` at all is the separate open question (`notes.md`, CSP section).

## Phase 2 — Search Everywhere palette — **DONE** (2026-08-10)

Every planned item landed; see `completed.md` for the file list and gate numbers.

- [x] `AppState`: `symbols` (`Arc<SymbolIndex>`) + `symbols_building`, cleared on
      the root-changed branch of `set_workspace`.
- [x] `commands/symbols.rs`: the three commands, registered, documented.
- [x] Background build on `open_workspace` / `rescan_workspace` — **and** in the
      `setup` hook, which was not in the plan and turned out to be required:
      `code-basics .` opens a workspace before any command runs, so without it
      the palette stayed empty until something happened to trigger a rescan.
- [x] `fs_write_file` / `git_write_file` re-index the single file they wrote.
- [x] `searchLogic.ts` + `.test.ts`, `SearchEverywhere.tsx`, `FileEditor`
      reveal, the `lineToPos` clamp.

What Phase 2 left open:

- [ ] **Nobody has run the app.** The gate is static: typecheck, vitest, cargo.
      The one item the plan flagged for manual verification — "verify in the real
      app that Ctrl+N is not swallowed by the webview" — is still **unverified**,
      and it is the binding most likely to be taken by the host. The listener is
      window-level, capture-phase, with `preventDefault` + `stopPropagation`,
      which is the strongest position available from JS, but a WebView2
      accelerator handled above the document would still win. Fallback stated in
      the plan: Ctrl+Alt+N, or double-Shift only.
- [ ] **Also unverified in the app**: that double-Shift does not fire while
      someone types capitals fast, and that the palette's overlay does not fight
      the xterm fit addon underneath it.
- [ ] `SearchScope` and `HitKind` are declared **twice** on the frontend — in
      `src/ipc/types.ts` and again in `src/components/searchLogic.ts`. Both
      spellings are identical string unions so `tsc` cannot tell them apart, and
      `SearchEverywhere.tsx` passes `searchLogic`'s version into an `api.ts`
      wrapper typed with `types.ts`'s. Harmless today, a silent drift hazard the
      moment a scope is added on one side only. Left as-is rather than picked
      arbitrarily at the gate — see `notes.md`.
- [ ] The palette is only reachable with a workspace open, which is correct, but
      it is also rendered *inside* the workspace branch of `App`, so the welcome
      screen swallows the shortcut silently rather than saying anything.

## Phase 3 — Derived diagrams (level 1) — **DONE** (2026-08-11)

Gate green on the joined tree: cb-core **1328** (+167), cb-app 27, pnpm test 292,
the same 2 pre-existing clippy warnings, fmt/build/typecheck/docs clean. See
`completed.md` for the file list and the explanation of the two flat counts.

- [x] `<ProjectReference>` in **both** arms of `parse_project_file`
      (`Event::Start` line 143, `Event::Empty` line 166), plus "no `Include`
      is ignored".
- [x] `architecture/{graph,mermaid,store}.rs` with sibling `*_tests.rs`;
      key-pinning tests + `types.ts` mirrors in the same change.
      **`components.rs` was not created** — `ArchKind` covers the need without a
      fourth module. Not an omission; see `notes.md`.
- [x] Out-of-workspace reference → `External` node **plus** a warning.
- [x] Node sibling deps; `workspaces` globs via `globset` (with
      `literal_separator`) over already-scanned directories; solution containment.

Left open out of Phase 3:

- [ ] **Level 2 HIGH signals** — not started. One per commit, each with its
      warnings surface; MEDIUM signals may only enrich. Test
      `a_connection_string_value_never_reaches_the_graph`.
- [ ] Confirm the `.`/`-`→`_` transform used by the generated `Projects.` class
      before relying on Aspire `AddProject<Projects.Ident>()` — still unverified.
- [ ] **pnpm workspaces are only half-supported, on purpose.** Only the root
      `package.json` is read; `pnpm-workspace.yaml` is ignored because parsing
      YAML would need a dependency this crate does not have. This repository
      itself uses pnpm, so its own diagram will show no workspace membership.
      Documented in `core-crate.md`; needs a product decision, not a bug fix.
- [ ] **Nobody has rendered a graph.** `project_graph` is tested against fixtures
      only; it has never been run over a real solution, and no UI calls it.

## Phase 4 — Agent-authored diagrams

**Two of these already landed in Phase 3** — check before starting.

- [ ] `architecture/prompt.rs::prompt_for` + contract-pinning tests.
- [ ] Opt-in `diagrams.agentCommand` through `process::Supervisor`; disabled
      button states show the reason.
- [x] `architecture/mermaid.rs::validate` + `ValidationRule` / `ValidationError`,
      landed early in Phase 3.
- [x] Front-matter provenance landed early in Phase 3 (hand-parsed, **not** YAML —
      see `store.rs`'s "No YAML" note). Still to do: hand-editing an inferred
      diagram flipping it to `inferred (edited)` on save is modelled
      (`FrontMatter::edited`) but nothing sets it, because nothing edits yet.

## Phase 5 — Architecture tab

**The backend half is already done.** What remains is entirely frontend.

- [ ] `ArchitectureView.tsx`, `DiagramCanvas.tsx`, `DiagramEditor.tsx`,
      `architectureLogic.ts`, `panZoomLogic.ts`, `nodeTargets.ts` (+ tests).
      This is why `pnpm test` did not move in Phase 3.
- [x] All six `arch_*` commands registered and documented;
      `arch_write_diagram` `create_dir_all`s.
- [x] Gitignore split: `diagrams/derived/` and `diagrams/.prompts/` in
      `config.rs::IGNORED` with the pinning test updated, `diagrams/*.md`
      committable.
- [ ] **Wire the wrappers up.** `archProjectGraph`, `archRenderGraph`,
      `archListDiagrams`, `archReadDiagram`, `archWriteDiagram` and `archValidate`
      exist in `ipc/api.ts` with **zero callers**, which `tsc` does not flag.
- [ ] The Phase 0 CSP spike above still blocks anything that renders mermaid, and
      is still not done.

## Phase 3 fix round + gate (2026-08-11)

- [x] All five Phase 3 defects fixed, each with a test that failed first for the
      right reason (+18 `cb-core` tests: 1271 → **1289**).
- [x] Full gate green over the joined tree — fmt, clippy (still exactly the 2
      pre-existing), both cargo suites, build, typecheck, test, coverage,
      docs:index, docs:check. Table in `completed.md`.
- [x] `docs/INDEX.md` regenerated; it was stale against `architecture/` and
      `symbols/`. **Nothing else needed fixing** — no integration breakage.
- [x] Verified the two flat counts (`pnpm test` 292, `cb-app` 27) are genuine
      and not disappeared tests — method recorded in `completed.md`.

**Still open — nothing below was touched by the fix round.** The Phase 3
open items above stand unchanged, and these remain the blockers:

- [ ] **Nobody has rendered a graph.** Still true after this round. The fixes
      were all driven by fixture tests; `project_graph` has still never run over
      a real solution and no UI calls it.
- [ ] **The six `arch_*` `ipc/api.ts` wrappers still have zero callers**, which
      `tsc` will not flag. Phase 5 is entirely frontend and entirely unstarted.
- [ ] **The Phase 0 CSP spike still blocks anything that renders mermaid.**
- [ ] Level 2 HIGH signals; the Aspire `Projects.` identifier transform;
      `pnpm-workspace.yaml` (half-supported on purpose — needs a product
      decision, not a bug fix).

## Phase 3 second fix round + gate (2026-08-11)

- [x] All three defects fixed, each with a test that failed first for the right
      reason (+7 `cb-core` tests: 1289 → **1296**; 128 under `architecture`).
- [x] Full gate green over the joined tree — fmt, clippy (still exactly the 2
      pre-existing), both cargo suites, build, typecheck, test, docs:index,
      docs:check. Table in `completed.md`.
- [x] `docs/INDEX.md` regenerated. **Nothing else needed fixing** — no
      integration breakage, and no contradiction between the two agents.
- [x] `SCANNER_VERSION` left at 1, as decided. Bump it the first time these
      rules change *after* the feature ships.

### New — an app-level bug this round uncovered but did not fix

- [ ] **A broken `package.json` deletes a project from the whole app.**
      `workspace.rs::scan_node_project` returns `None` on a read or parse
      failure (`.ok()?` then `parse_package_json(&content)?`), so the project is
      missing from `Workspace::projects` entirely: no Run entry, no test config,
      no Tests-tab presence, no diagram box, and no message anywhere. A stray
      trailing comma silently deletes a project from the IDE.
      `project_graph` now warns about it, but that is a diagram-layer patch —
      every other consumer is still silently wrong and a user who never opens
      the Architecture tab sees nothing. **This is an app bug, not a diagram
      bug.** The fix wants a `warnings` field on `Workspace` surfaced once at
      the top level; it crosses IPC, so `model.rs`'s key-pinning test and
      `src/ipc/types.ts` change together, plus a place in the UI to show it.
      Full reasoning in `notes.md`.
- [ ] Audit the .NET scan path for the same `Option`-swallowing shape before
      assuming this is Node-only.

**Still open — nothing below was touched by this round.** Every Phase 3, 4 and
5 item above stands unchanged, and these remain the blockers:

- [ ] **Nobody has rendered a graph.** Still true. All fixes were fixture-driven;
      `project_graph` has never run over a real solution and no UI calls it.
- [ ] **The six `arch_*` `ipc/api.ts` wrappers still have zero callers**, which
      `tsc` will not flag. Phase 5 is entirely frontend and entirely unstarted —
      which is why `pnpm test` stayed at 292 again.
- [ ] **The Phase 0 CSP spike still blocks anything that renders mermaid**, and
      `mermaid@11.16.1` is still installed and imported nowhere.
- [ ] Level 2 HIGH signals; the Aspire `Projects.` identifier transform;
      `pnpm-workspace.yaml` still half-supported on purpose (needs a product
      decision and a YAML dependency this crate will not take — the round only
      tightened what happens when both files are present).

## Phase 4 — level 2 signals (component map) — **DONE** (2026-08-11)

- [x] `architecture/signals/{framework,dotnet,node,routes}.rs` + `components.rs`,
      each with a sibling `*_tests.rs` (+146 `cb-core` tests: 1296 → **1442**).
- [x] The grading rule in exactly one function (`framework::admit`), not per
      producer; every refusal counted into `ArchGraph::warnings`.
- [x] `a_connection_string_value_never_reaches_the_graph` — the value is refused
      whole rather than redacted, screened on every field including the evidence
      excerpt.
- [x] `ArchKind::{Service,DataStore}` + `EdgeKind::DataAccess`, mirrored in
      `types.ts` and pinned variant-by-variant.
- [x] Full gate green + docs (`CLAUDE.md`, `ipc-contract.md`, `core-crate.md`,
      `INDEX.md`). Table in `completed.md`.

Left open out of Phase 4:

- [ ] **`component_graph` is not reachable from the app at all.** No Tauri
      command, no `ipc/api.ts` wrapper, no caller — which is why `cb-app` and
      `pnpm test` are flat. It is a `cb-core` function with tests and nothing
      else, and `tsc`/`cargo` cannot tell you that.
- [ ] **No project → project arrow is drawn**, deliberately: the `AddHttpClient`
      `BaseAddress` ↔ `applicationUrl` match is MEDIUM because its evidence is a
      `.cs` file, so it lands in the warnings naming caller, callee and file. If
      that arrow is wanted the fix is in `signals/dotnet.rs` — cite the
      `launchSettings.json` line, which *is* a declaration file — not in the
      assembly.
- [ ] **`component_graph` over this repository is legitimately empty** (no HTTP
      project, no data client), so the one workspace on hand cannot demonstrate
      the feature. Nobody has produced a non-empty component map from real code.
- [ ] The Aspire `Projects.` identifier transform is **still** unverified, third
      round running.

**Still open and untouched by Phase 4:** the Phase 0 CSP spike (still blocking
anything that renders mermaid; `mermaid@11.16.1` still imported nowhere), the
six `arch_*` wrappers with zero callers, Phase 5 (entirely frontend, unstarted),
`pnpm-workspace.yaml`, and the app-level bug from the previous round — **a
broken `package.json` silently deletes a project from the whole app**
(`workspace.rs::scan_node_project` returning `None`), which is still not fixed
and is still the most user-visible item on this list.

## Opened by the adversarial round

- [x] 3 ASP.NET route-fabrication defects (`MapGroup` cross-file leak,
      `RouteGroupBuilder` parameter, abstract controllers).
- [x] 4 credential-leak channels in the component map.
- [x] `mount_prefix` no longer conflates "no mount" with "unreadable mount".
- [ ] **No running-app oracle is checked in.** The route defects were caught by
      comparing against a live ASP.NET application; that comparison exists
      nowhere in this repository, so the next change to `routes.rs` has only
      synthetic fixtures to answer to. Either check in a minimal ASP.NET sample
      whose endpoint list is asserted against, or write down that route changes
      require a manual run before merge.
- [ ] **The leak sweep is not automated either.** `components_tests` pins the
      known channels; nothing re-plants secrets and re-greps every reachable
      string when a *new* producer or a new published surface is added. The next
      producer inherits the guarantee only by remembering to.
- [ ] Doc comments asserting safety properties are now 2-for-2 wrong in this
      feature (`lineToPos`, `cross_project_notes`). Worth a sweep of the
      remaining "this is safe because…" blocks in `architecture/` before Phase 5.

## Cargo round

- [x] **A broken `package.json` silently deletes a project from the whole app.**
      Fixed by `Project.unreadable` — the project is listed inert instead of
      dropped, and the Run tab shows it under "Could not be read". This was the
      most user-visible item on this list for two rounds.
- [x] Cargo is a detected ecosystem, so the diagrams are no longer three
      unconnected boxes on this repository.
- [ ] **Cargo emits no run/test configurations, on purpose.** If that is ever
      revisited, note what blocks it: which workspace member to offer, `cargo
      test` having no report file (so no "re-run failed" filter), and the fact
      that detected configs are re-derived every scan and would therefore appear
      in every Rust user's Run and Tests tabs at once. The reasoning is in
      `adapters/cargo.rs`'s module doc.
- [ ] `cb-app` has **no test for the new `arch_*` / `symbols` commands** — the
      count has been flat at 27 across three rounds while the command surface
      grew from 64 to 75. They are thin delegators, but "thin" is a claim nobody
      is checking.
- [ ] `unreadable` is set for .NET, Node and cargo manifests. Nothing sets it
      for a **declarative-adapter** project whose detection file will not parse;
      worth checking whether that path can even fail that way.
- [ ] The `cmp_owned` clippy warning in `workspace.rs` has now moved twice
      (:1028 → :1581). Any future baseline should name the lint and file, not
      the line.

## Phase 4 (UI) round

- [x] The Architecture tab exists, is registered in `TABS`, and click-to-open
      routes through the existing `openRequest`/token path.
- [x] `ArchGraph.warnings` now reaches a human — counted in the canvas toolbar
      and listed under the picture. This was the outstanding audit item.
- [x] Mermaid confirmed lazy: no static import, and zero mermaid sources in the
      entry chunk's sourcemap.
- [x] ~~**`nodeTargets.ts` is outside the coverage gate.**~~ Fixed at the
      2026-08-12 gate: `vite.config.ts`'s `coverage.include` is now a three-entry
      array, `src/**/*Logic.ts` + `src/components/language.ts` +
      `src/views/architecture/nodeTargets.ts`. Verified by removing the line and
      re-running — 372 lines / 18 files without it, 485 / 19 with it.
- [x] The `vitest` text coverage reporter hides files at 100% — **DOCUMENTED (C6,
      2026-08-24)** in `docs/guides/development.md`: the printed table is not the full
      measured set; read `coverage/coverage-final.json` or `--coverage.reporter=json-summary`
      for the real per-file list (how the `nodeTargets.ts` gap once survived a round).
- [ ] **`nodeTargets.ts` is now a named exception in the include list**, which
      is a second place to remember. The durable fix is the naming convention:
      either rename it `nodeTargetsLogic.ts` so the glob catches it, or accept
      that any future non-`*Logic.ts` decision module needs its own entry. Left
      as-is at the gate rather than renaming a file two agents had just edited.
- [ ] `cb-app` still has **no test for the `arch_*` commands** — 27 tests flat
      across four rounds now while the surface grew to 75 commands.
- [ ] The entry chunk is 1.47 MB and rollup warns about it. Nothing to do with
      this feature (react-dom + CodeMirror + xterm), but if it is ever worth
      addressing, the sourcemap breakdown method is written up in
      `completed.md` under Phase 4.

## Phase 4 (UI) fix round + gate (2026-08-12)

- [x] Click-to-open fixed — `BOX_SELECTOR = "g.node[id], g.cluster[id]"` and
      forward resolution through `mermaidIdOf`, mirroring `mermaid_id` in
      `architecture/mermaid.rs`.
- [x] "Will not render" on every stored diagram fixed — `frontMatterLogic.ts`
      strips the front matter before `arch_validate` and adds the dropped line
      count back to the reported line number.
- [x] `nodeTargets.ts` brought inside the coverage gate.
- [x] Full gate green: cb-core **1542** (+1), cb-app 27, pnpm test **423 / 19
      files** (+32), coverage 99.58% lines, exactly the 2 pre-existing clippy
      warnings, fmt/build/typecheck/docs clean. Table in `completed.md`.
- [x] Mermaid confirmed lazy **again after the fixes** — the entry chunk has
      zero static imports and reaches mermaid only through `import()`.

Opened by this round:

- [ ] **`mermaidIdOf` is a hand-maintained mirror of `mermaid_id`.** Two
      implementations of one encoding in two languages, with nothing that fails
      when they diverge — the same shape as the `SearchScope`/`HitKind` double
      declaration already on this list. A `cb-core` test emitting a table of
      `(id, mermaid_id(id))` pairs that the vitest suite reads would close it;
      nothing does today.
- [ ] **Neither fix has been re-verified in the running app.** Both were driven
      by tests written against captured real output (the frontmatter one quotes
      a real `store::render` + `validate` run in its header), which is stronger
      than fixtures, but clicking a box and opening a stored diagram in the
      built app is still unexecuted.
