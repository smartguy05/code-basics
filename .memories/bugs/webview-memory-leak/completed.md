# WebView2 memory leak during git actions (up to 12 GB, crashed the machine)

> ⚠️ **These three fixes are real, valid leak fixes and pass all gates — but
> reproduction proved NONE of them is the leak the user actually hit.** The real
> leak is in the **Rust backend `cb-app.exe`** (webview stays flat), still OPEN.
> See `investigation-state.md` for the current diagnosis and Monday's next steps.
> Keep these fixes; they are genuine improvements.

## Symptom
Installed app climbed to ~12 GB, noticed while doing git actions (staging,
reverting, viewing diffs). A WebView2 process above ~4 GB is past V8's heap
ceiling, so most of it is **native** webview memory (compositor/GPU/DOM layers).

## Three independent root causes (all fixed)

### 1. DiffView rebuilt the whole editor on every git mutation (primary — native)
`src/components/DiffView.tsx` build effect depended on `[path, baseline, working,
layout, editable, collapseUnchanged, ignoreWhitespace]`. Every stage/unstage/revert
re-fetched the file → new `working`/`baseline` strings → the effect destroyed and
reconstructed a `MergeView` (two CodeMirror `EditorView`s). JS-correct (old view
`.destroy()`'d) but WebView2 does not promptly reclaim the native layers, so rapid
staging climbed without bound.

**Fix:** split into a **construction effect** (structural deps only:
`[path, layout, editable, collapseUnchanged, ignoreWhitespace, hasBaseline]`, reads
content via `baselineRef`/`workingRef`) and a **content-sync effect**
(`[baseline, working]`) that dispatches whole-doc replacements into the existing
panes — `merge.a`/`merge.b` for side-by-side, `originalDocChangeEffect` +
`getOriginalDoc` for the inline unified original. `syncedRef` skips the redundant
first sync after a construction. `@codemirror/merge` re-diffs on doc change.

### 2. Unbounded intent log re-parsed every 2s (backend CPU/GC)
`edits.jsonl` is append-only, unbounded (12 MB / 8028 lines here, ~2× because the
record-intent hook is double-installed). The 2s Intent poll → `intent_groups` →
`intents::load` → `read_jsonl` read+parsed the whole file every tick.

**Fix:** `cb_core::intents::IntentCache` (in `mod.rs`) memoises the parse keyed on
the mtime+len of all three files `load` reads (`edits`, `labels`, user-move file)
plus branch. Held per `WorkspaceSlot` (`state.rs`), accessed via
`AppState::load_intents`, used by `intent_groups` (`commands/intents.rs`). The
review still recomputes against the live diff every call — only the parse is
cached. A quiet poll tick now does zero file reads (3 metadata stats).

### 3. Tauri Channel.onmessage never released (JS-heap retention)
11 `channel.onmessage = onEvent` sites in `src/ipc/api.ts` never cleared the
handler; Tauri keeps it in `__TAURI_INTERNALS__` for the page's life, so each
run/build/test/terminal/query/**gitNetwork** permanently pinned its closure.

**Fix:** `src/ipc/channelLogic.ts` — pure terminal-event predicates per event type
(`isProcessTerminal`/`isTerminalStreamTerminal`/`isDebugTerminal`/`isSqlTerminal`,
each a single unambiguous end marker: process exited/failed, SQL `finished` (not
per-statement `completed`), debug exited/failed/notInstalled). `api.ts` `streaming()`
helper swaps `onmessage` for a no-op on the terminal event. All 11 sites wired.

### Compaction (Fix 2c, safe subset)
`compact_if_large` / `compact_duplicate_lines` in `mod.rs`: exact-duplicate-line
dedup (the double-install artifact; reader already dedups by tool_use_id so it
loses nothing), atomic temp+rename with `.bak`, gated at 1 MiB, run on a cache
**miss** (free — we're reading anyway), stamp taken **after** compaction so it
doesn't re-trigger. The 12 MB log self-compacts on the next miss once the rebuilt
app runs. Did NOT touch retire/seq/tombstone machinery (max seq is preserved by
keeping the first of each identical line).

## Files touched
- `src/components/DiffView.tsx` (split effect, in-place doc sync)
- `src/ipc/channelLogic.ts` + `.test.ts` (new), `src/ipc/api.ts` (streaming helper, 11 sites)
- `crates/core/src/intents/mod.rs` (IntentCache, compaction) + `intents_tests.rs` (8 tests)
- `src-tauri/src/state.rs` (per-slot IntentCache + `load_intents`)
- `src-tauri/src/commands/intents.rs` (`intent_groups` uses the cache)

## Gates (all green)
`pnpm typecheck`, `pnpm test` (2105), `cargo fmt --check`, `cargo check -p cb-app`,
`cargo test -p cb-core --lib intents::` (449). Channel logic tests 4/4; cache 4/4;
compaction 4/4.

## Rollout
Frontend fixes (1, 3) ship on next app build. Backend (2 + doubling fix) need a
**release rebuild + reinstall** — hooks and the app run the Program Files binary.
The double-install fix is a SEPARATE uncommitted change (see
`.memories/bugs/intent-duplicate-and-stale/`); commit + reinstall to stop the 2×
growth at the source. NOT hand-editing the live 12 MB log now — the old app is
still appending and a concurrent rewrite would race; the app self-compacts on
reinstall.
