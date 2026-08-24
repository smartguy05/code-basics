# Todos — LSP usages

> **`plan.md`'s eleven items are done** (worked 2026-08-17). Five were real
> defects, three were decisions now recorded in code or `CLAUDE.md`, and three
> stay open with an explicit trigger rather than a date. See the follow-up
> section at the end of `notes.md` for the captures behind each one.
>
> The only genuinely open item left is the untestable-by-construction frontend
> (node environment, no DOM), and it has a manual procedure rather than a fix.

## Done

- [x] **Phase 0 — the blocking unknowns, retired by probing rather than reasoning.**
      Roslyn's licence (MIT), `--stdio` + `--autoLoadProjects`, the full
      capability set, and a real cross-file `references` answer. See `notes.md`.
- [x] **Phase 1 — the pure foundations.** `lsp/{framing,jsonrpc,uri,positions}.rs`
      with sibling `*_tests.rs`. **83 tests**, `cb-core` 1575 → 1658. Clippy back
      to exactly the two documented baseline warnings
      (`importers/rider.rs`, `workspace.rs` — pin by lint + file, never line).

Two real bugs were found by the tests rather than by review, both in clamping,
both invisible to a fixture without astral-plane characters:

- `utf16_to_byte` walked *past* a surrogate pair instead of clamping back to the
  character's start, so a `character` landing inside an emoji returned the byte
  after it.
- `byte_to_utf16` counted a character whose span merely *contained* the byte
  offset, so a byte inside a two-byte `é` reported the column after it.

- [x] **Phase 2 — protocol, registry, settings.** `protocol.rs` (55 tests),
      `settings.rs` (12) + `WorkspaceConfig.lsp` + `lsp-logs/` in `IGNORED` (6),
      `registry.rs` (48), `resolve_program` exported, `docs:index` regenerated,
      `core-crate.md` gained an `lsp` section. Lib tests 1658 → 1780 (+122, one
      of them the integration pass's own); integration targets unchanged at
      49 / 2 (1 ignored) / 11. Clippy back to exactly the two documented
      baseline warnings.

## Opened by Phase 2, to settle before or during the phase that needs it

- [x] ~~**`ServerSpec::language_id` is one string per language**~~ — it is a
      property of the **extension**, and that was measured rather than reasoned:
      opening a `.tsx` as `typescript` makes typescript-language-server report
      `<unknown>` and `props` from inside a JSX expression as declarations, plus
      nine diagnostics starting `'>' expected.`, and `documentSymbol` is exactly
      what the inline rows are built from. `ServerSpec::language_id_for` refines
      per extension; an extension belonging to another language falls back to the
      server's own id rather than guessing.
- [x] ~~**`SymbolKind` has no `Property`**~~ — added. LSP 7 → `Property`; **8
      (`Field`) and 13 (`Variable`) stay `Variable` deliberately**, because every
      language here that has properties also has fields and calls them different
      things. The variant is unreachable from the text scan by design. The
      key-pinning test is now an exhaustive `match` rather than a hand-written
      array, so the next added variant fails to compile instead of reaching the
      wire unmirrored.
- [x] ~~`decode_goto`'s error always says `textDocument/definition`~~ — fixed:
      both call sites in `client.rs` wrap it with `malformed(method, error)`., because
      the four goto-shaped requests share one decoder that is not told which one
      it is decoding. `client.rs` must wrap the error with the real method or
      the UI will name the wrong question.
- [x] ~~**The `LocationLink` path is not backed by a captured payload.**~~ —
      captured. rust-analyzer 1.97.1 really does answer `LocationLink[]`, and in
      the real payload `targetRange` starts on the doc comment (line 2) while
      `targetSelectionRange` starts on the identifier (line 3), so `aim()`'s
      preference is load-bearing rather than theoretical. Pasted verbatim into
      `protocol_tests.rs`, named so captured and hand-written fixtures cannot be
      confused.
- [x] ~~Nothing about the three absent servers is verified~~ — all installed and
      run. **Every design-derived claim was wrong**; see `notes.md` for the
      captures. Summary: rust-analyzer's readiness prefix matched a sibling token
      and promoted ~10s early; TypeScript and Python were not `Immediate`;
      `--stdio` was handed to Python programs that reject it; and `pylsp` /
      `jedi-language-server` were removed as candidates because they answer
      zero and one-too-many respectively.

- [x] **Phase 3 — transport, client, and a scripted fake server.**
      `transport.rs` (writer / reader / stderr-pump / reaper, register-before-send,
      `$/cancelRequest` on timeout, the server→client answering table, the restart
      policy), `client.rs` (handshake, capability gates, readiness, the document
      mirror, the five questions), and `src/bin/fake_lsp.rs` with thirteen scripted
      misbehaviours. `process::{kill_tree, configure_process_group}` exported so
      the transport stopped carrying its own single-pid copy. Lib tests 1780 →
      1813; `tests/lsp_transport.rs` 24 and `tests/lsp_client.rs` 30 are new
      targets. `fake_lsp.rs` is in the coverage ignore regex (test-only code,
      recorded in CLAUDE.md **and** `docs/guides/development.md`).

      The adversarial review pass found four real defects, all in the transport
      and none caught by the tests that existed — see `notes.md`.

## Opened by Phase 3

- [ ] **`Transport::shutdown` is `&self` and always publishes `ShutDown`**, so a
      second call re-publishes nothing (first-death-wins) but still burns the
      `EXIT_GRACE` wait. Harmless today because only `Client::shutdown` calls it.
      **Trigger: the session actor gaining a second teardown path.** Reviewed
      2026-08-17 and deliberately left; the reason is in `notes.md`.
- [x] ~~**`kill_tree` blocks the calling runtime worker**~~ **DONE (WF2, 2026-08-24, B14):**
      added `kill_tree_async` = `spawn_blocking(kill_tree).await` and routed the async callers
      (`Supervisor::cancel`, the four `transport.rs` sites) through it; `Transport::drop` uses
      `Handle::try_current` → `spawn_blocking` fire-and-forget in a runtime, inline when none.
      Sync `kill_tree` kept (Drop's no-runtime branch + the offloaded body). Pinned by
      `an_offloaded_kill_does_not_stall_the_current_thread_runtime` (timing-free injected body).
- [x] ~~`ReadyWithCaveat` reaches no consumer~~ — it does now: `session.rs`
      turns it into a `RunningRow` and a caveat `message`, which is exactly what the
      Phase 5 fix round made the UI honour. `ask` returns
      `Result<Vec<Location>, RequestError>`, which has no channel for it; the
      contract is documented on `ask` (callers must read `readiness()` alongside
      the result) and enforced by nothing. Decide in the phase that renders a
      count whether the state should travel *with* the answer.
- [x] ~~**The Unix tree kill is unverified.**~~ — run on Linux (Docker,
      `rust:slim` + `git` + `procps`, `src-tauri` dropped from the workspace
      members so no webkit2gtk is needed). **The whole `cb-core` suite now passes
      there**, and getting it to took two real fixes:
      - `pid_alive` shelled out to `Command::new("kill")`, but `kill` is a POSIX
        *shell builtin* and `/bin/kill` is not in a Debian slim image. It
        therefore answered `false` for **every** pid, which means the two
        `until(|| !pid_alive(..))` waits that *are* this test returned instantly
        and asserted nothing. The item carried for a phase as "unverified" was
        really "could not fail". Goes through `sh` now.
      - `kill_tree` signalled `-pid` only, so for a pid that is not a group
        leader it killed **nothing at all**, not even the named process. Both
        production callers do spawn group leaders, so this was correct purely by
        construction. It signals the group *and* the pid now, matching the
        Windows `taskkill /T` arm.

      Repeat the Docker run whenever a `#[cfg(unix)]` branch changes; the recipe
      is in `notes.md` and costs one container.

## Watch out for

- [x] ~~Capability negotiation is a gate~~ — done and pinned by
      `every_unadvertised_request_names_the_capability_it_needed`. A missing
      `implementationProvider` must produce "this server does not provide
      implementations", never an empty group that reads as "there are none". A
      server whose `textDocumentSync` is `None` is refused outright — we cannot
      keep it in sync, so nothing it says can be trusted.
- [x] ~~Three of the four servers are not installed on this machine~~ — installed
      2026-08-14: `typescript-language-server` + `typescript@5` (npm),
      `basedpyright` (pip), `pyright` (npm), and the `rust-analyzer` rustup
      component, which was a *shim for an uninstalled component* and failed at
      launch rather than reporting itself absent.
- [ ] **`codeLens` is a later optimisation, not the first cut.** Roslyn
      advertises `codeLensProvider: {resolveProvider: true}` and would give the
      count directly, but it is not uniform across four servers and needs its own
      configuration plumbing. `documentSymbol` + `references` is one code path
      for all of them. **Trigger: the two-request cost becoming a *measured*
      problem** — not a suspicion. Accepted again 2026-08-17.
- [x] ~~No real-server oracle is checked in yet~~ — `crates/core/tests/lsp_oracle.rs`.
      Table-driven, one `#[ignore]`d test per language, self-contained fixtures,
      skipping only on `NotConfigured`. Plus `fixtures_say_what_the_oracles_assert`,
      which is *not* ignored and holds the fixtures to the line numbers the
      oracles assert. Run with `--test-threads=1`.
- [x] ~~Roslyn spawns `BuildHost-netcore` children~~ — `transport.rs` calls
      `kill_tree(pid)` when a server ignores `shutdown`. Original note: Roslyn spawns `BuildHost-netcore` / `BuildHost-net472` children. Shutdown
      must go through `process::kill::kill_tree` or they are orphaned.

## Phase 4 — complete, reviewed, gate green

- [x] **Phase 4 — `documents.rs`, `session.rs`, the `src-tauri` seam, the
      `types.ts` mirrors, and the review pass over all of it.** `documents.rs`
      (the per-server sync state machine, with the late-server replay),
      `session.rs` (the actor), `AppState.lsp` + `begin/record/take_lsp_session`,
      the seven `lsp_*` commands, and the ten mirrored types in `types.ts` /
      seven wrappers in `api.ts`. `lsp_session.rs` is a sixth `cb-core` test
      target (**16** tests). Counts after the review pass: `cb-core` lib
      **1901**, integration 49 / 2 (1 ignored) / 30 / 16 / 24 / 11; `cb-app`
      **40**; `pnpm test` 494. Clippy at exactly the two documented baseline
      warnings, coverage **88.48%**.
- [x] Docs: `core-crate.md`'s `lsp` section now covers `documents`, `results`,
      `model`, `transport`, `client` and `session` (Phase 3's entry had been
      deferred to here); `ipc-contract.md` gained the ten LSP types, their
      pinning tests, the no-`skip_serializing_if` rule and the
      1-based-line / 0-based-UTF-16-character convention.

### What the review pass changed (all with a test that failed first)

- `session::worst` — a wholly refused goto answer took `refusals[0]`, so a
      server lacking `definitionProvider` that then died was reported
      `Unsupported`: the one outcome that says retrying is pointless.
- `session::request_position` — extracted so the *request* direction of the
      line convention is pinned at all (see `notes.md`).
- `session::caveat_note` — a count from a server promoted at the 90 s readiness
      ceiling was byte-identical to a primed server's. It now carries the caveat
      in `message`, which is what `client::ask`'s docs already required of its
      caller.
- `Needs::OpenDocument` — `documentSymbol` is a question about an open buffer, so
      a file that could not be read to open it is now `Failed` with that reason
      rather than `Ready` with no anchors.
- `commands::lsp::ensure_session` retries a refusal instead of reading the slot
      once, so the loser of a same-root start race is no longer told the
      workspace was closed.
- `AppState::lsp_slot` recovers a poisoned mutex — the one exception to this
      file's poisoning contract, because the slot owns processes.

## Phase 5 — complete, reviewed, gate green

- [x] **Phase 5 — the frontend feature.** `usagesLogic.ts` (+ tests),
      `usagesExtension.ts` (the first `WidgetType` in this repo),
      `FileEditor.tsx` as the whole LSP client (didOpen/didChange/didClose, the
      inline rows, the usages dropdown, the middle-click picker),
      `lspStatusLogic.ts` + `LspStatus.tsx` in the titlebar, and
      `App`/`RunView` wiring through the **existing** `requestToken`.
- [x] **The review pass over it** — three read-only lenses, then this pass acted
      on them. Counts after: `pnpm test` **607** in 23 files (570 before this
      pass), coverage **99.71%** lines. Rust untouched and verified unchanged:
      clippy at exactly the two baseline warnings, `cb-core`
      1901 / 49 / 2 (1 ignored) / 30 / 16 / 24 / 11, `cb-app` 40.
- [x] Docs: `architecture/frontend.md` (the three-file split and the rules each
      follows), `getting-started/using-the-app.md` (the row, middle-click, and
      what to do about "no language server"), and the new
      `guides/language-servers.md` — the item carried since Phase 2.

### What the review pass changed

Confirmed and fixed: the query/sync ordering (a `syncedVersion` ref, and the
guard moved into `requestVisible` where all three callers pass); an edit made
before `didOpen` resolved being dropped silently; `didClose` owed from *send*
rather than from resolve; the anchor retry limit raised past the backend's 90 s
ceiling with honest wording when it does run out; "None." under a group the
server may have refused; the goto picker discarding `outcome`; rows left on
screen at pre-edit anchor lines after a sync failure; queue pruning and the
unbounded `answers` map; the anchor retry chains multiplying; middle-click on a
row starting the Windows autoscroll cursor; `hint` missing from the collapsed
tooltip; the status poll going silent about a server that dies after settling.
Eight decisions moved out of components into `usagesLogic.ts`
(`usageCountLabel`, `shouldRetryAnchors`, `placeMenu`, `countUsageRows`,
`failedUsageResult`, `emptyGroupNote`, `partialAnswerNote`, `retainUsageJobs`)
plus `toneClass`/`actionDetail` out of `usagesExtension.ts`, which the coverage
glob cannot see.

Rejected: threading a per-row "paused" reason into `usageRowView` — the rows are
placed from pre-edit anchor lines, so an explanatory row above the wrong
declaration is worse than no row; they are cleared and the corner badge explains.

## Still open

- [ ] **`usagesExtension.ts` and every component in the feature are untested by
      construction** (node environment, no DOM). The widget's `toDOM`, both
      `mousedown` paths and the viewport reporter have no coverage of any kind —
      only the app itself exercises them. See `notes.md` for how to look.
Superseded and removed: the stub list that used to sit here (documents.rs,
session.rs, the `src-tauri` seam, the `types.ts` mirrors, the `SearchScope`
dedup) is all delivered — see the Phase 4 and Phase 5 sections above.

Landed and green: **`model.rs`** (the full IPC contract, no `skip_serializing_if`,
keys pinned) and **`results.rs`** (locations to `UsageResult`/`DefinitionResult`,
`relative_to_root` for identity, unresolvable URIs kept with `path: None`).
`cb-core` lib 1813 → **1855**.

Everything that was listed here as a stub has since been delivered and
verified — `documents.rs`, `session.rs`, the `src-tauri` seam, the `types.ts`
mirrors, and the `SearchScope`/`HitKind` deduplication. See the Phase 4 and
Phase 5 sections above. The boxes were left ticked-off-by-prose for a while,
which is exactly how a todo list stops being read; they are gone now.

One scripting gotcha worth keeping: a markdown fence written as three raw
backticks inside a JS template literal **closes the string** and the workflow
fails to parse. Build such prompts as arrays of lines joined with `\n`, and
parse-check with the async-function wrapper before launching.

## Opened by the Phase 4 verification pass

- [x] ~~**`Documents::version()` is a number nobody sends.**~~ — fixed the way
      the notes preferred: `Client::did_open`/`did_change` take the version as an
      argument and `session::dispatch` passes the mirror's through. One counter,
      in the only place whose high-water rule survives a close.

      Getting a *failing* test first needed new machinery, and that is the
      durable part: document sync is the one thing this subsystem does that a
      server never answers, so no test could see the wire at all. `cb-fake-lsp`
      gained a `notificationJournal` script field — every received notification
      appended as one JSON line, in wire order. The defect was then immediate:
      open / change / close / re-open / change produced **`[1, 2, 1, 2]`**.
- [x] ~~**`src-tauri/src/commands/lsp.rs` is 49.7% line-covered**~~ — settled as
      a rule rather than a number, because the cause is structural and was
      verified: every `#[tauri::command]` takes `State<'_, AppState>`, which
      cannot be built without a `tauri::App`, and the `tauri` dependency does not
      enable the `test` feature. **No command in `cb-app` is called by any test,
      in any module, and none can be.** `CLAUDE.md`'s `src-tauri` section now
      says so, and says the thing that actually matters: a command body may not
      *decide* anything, and the moment one does the decision moves to a plain
      free function beside it. Applied to the one that did — `lsp_status` turned
      a failure into a successful empty list — which is `status_for` now, tested
      both ways. Judge these modules by whether the untested lines decide
      anything, not by the percentage.

## The Phase 5 fix round (done)

- [x] The lost one-shot readiness signal — sticky `SignalSink` in `transport.rs`,
      read by `watch_readiness`; `Lagged` still non-fatal.
- [x] The ceiling is no longer a verdict: a late signal withdraws the caveat, a
      death still ends the watch without publishing.
- [x] `ServerStatus.caveat` as its own field, `toneFor` warns on it, the titlebar
      says it, and `spawn_supervisor` compares the whole `ReadyState` so the row is
      recomputed when it is withdrawn.
- [x] `--clientProcessId` / `--extensionLogDirectory` are appended; the log dir is
      created only when it will be used, and pruned by count and bytes.
- [x] The UI no longer renders a caveated answer as authoritative: the row, the
      dropdown heading, the truncation line, the goto jump (which now falls through
      to the picker) and the corner badge (short text, long detail).
- [x] Non-ready count answers are no longer cached as final — `UsageAnswers` with a
      bounded re-ask, which fixes rows frozen after a mid-session server restart.

Still open from earlier passes: `Documents::version()`, and `commands/lsp.rs`
coverage. Neither was touched this round.

## VERIFIED IN THE RUNNING APP (the fix works)

Driven over CDP against a real Roslyn server, on the repo itself:

| Step | Result |
|---|---|
| `csharp` readiness | **`ready` at t+15s**, no caveat in `detail` — the sticky signal works; previously it sat `loading` for 90s and was then promoted with a caveat |
| Row above `TryGetElements` | **"1 usage"** (was "No usages") |
| Dropdown | `sidecar/inspector/Walker.cs`, line **138**, snippet `if (Collections.TryGetElements(obj, out var elements, out var total))` |
| Clicking it | `Walker.cs` opens in a new tab, becomes active, viewport scrolled to 102-999 so line 138 is on screen. `sed -n '138p'` confirms that is the call site |
| `lsp_find_usages` raw | `total: 1`, `message: null`, `highlight {16,30}` = `"TryGetElements"` |
| Middle-click the identifier | picker opens with **Declarations** and **Implementations**, both `Collections.cs:26`; `mousedown.defaultPrevented === true`, so the Windows autoscroll guard is in place |

### The earlier "silent exit" was my launch plumbing, not a crash

Two post-fix launches had the app vanish seconds after a `.cs` file was opened,
with no panic in any log. It was **not** a crash in the new start-up path. Started
detached via PowerShell `Start-Process` with stdout/stderr redirected, the app
stayed up indefinitely with the file open and **stderr was empty**. The earlier
exits came from launching through `nohup`/`tee` inside a backgrounded shell task,
whose process tree was reaped when the task completed.

Lesson for the next person verifying in the app: **do not launch it from a
backgrounded shell**. `Start-Process -RedirectStandardOutput -RedirectStandardError
-PassThru` survives, gives you a pid to poll, and captures a panic if there is one.
Note that `pnpm` is a `.ps1`/`.cmd` shim so `Start-Process pnpm` fails with
"%1 is not a valid Win32 application" — start Vite separately and run
`target/debug/cb-app.exe <abs path>` directly, which also removes the tauri-cli
supervisor from the picture.

### Two small things this verification opened

- [x] **The picker fired when both entries were the same location.** Fixed in
      `usagesLogic::definitionAction`: it now counts distinct *destinations*, keyed
      `(path, line)` — or `(label, line)` for an unopenable target, so two
      `source-generated:` documents are not merged into one. The groups are still
      built from the original lists, because a reader does want to see that a
      symbol is both a declaration and an implementation; deduplication is a
      decision about *where to go*, not about what to show. Four tests, one of
      which failed first for the right reason (`expected pick to equal jump`).
      Original wording of the finding: A static method
      is its own implementation, so `definition` and `implementation` both return
      `Collections.cs:26` and the "more than one target ⇒ pick" rule shows a picker
      with two rows pointing at one destination. Display-wise keeping both groups is
      right; for the **jump-versus-pick decision** two entries at one `(path, line)`
      are one destination and should jump. Fix in `usagesLogic::definitionAction`
      (dedup by location before counting), not in the backend.
- [x] **Verified: editing clears the count and never shows a stale number.**
      Driven with CDP `Input.insertText` (a synthetic DOM event would not go
      through CodeMirror's `beforeinput` path, so it would not have tested this):

      | after the keystroke | row text |
      |---|---|
      | ~120 ms | `Usages` — the number is gone |
      | ~400 ms | `Finding usages…` |
      | ~1.2 s | `1 usage` |

      At no point was a stale count on screen. Undo restored the buffer and the
      file was never written (`git status` on `Collections.cs` clean afterwards).
      Original wording of the finding: Every other step above was checked; this one needs a synthetic
      keystroke (CDP `Input.insertText` after focusing the editor) and was skipped.
      It is the cheapest remaining check and it guards the core promise, so do it
      first next time.

## Noticed while verifying, pre-existing, out of scope

- [ ] **MOVED OUT — this belongs to the editor, not to this work item.** Not
      fixed here and deliberately not tracked here any longer; re-file it against
      `FileEditor` when the editor is next worked on. Kept only so the next
      reader of this file knows it was a decision rather than an oversight.
      **The dirty dot stays lit after undoing back to the saved content.**
      `FileEditor` sets dirty on any `docChanged` and clears it only on save, so
      undoing an edit leaves the buffer identical to disk while the tab still
      claims unsaved changes. Observed with CDP: after insert-then-undo the first
      line matched `head -1` exactly and `git status` was clean, yet one
      `.dirty-dot` remained. Predates this feature — the dirty flag has always
      been "has been edited", not "differs from disk". Fixing it means comparing
      against the loaded text (or CodeMirror's `isClean` checkpoint), which is a
      change to a shipped behaviour and wants its own decision.
