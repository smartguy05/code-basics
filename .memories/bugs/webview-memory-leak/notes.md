# Notes / gotchas — webview memory leak

- **A floating panel or diff editor that reconstructs a CodeMirror `EditorView`
  (or `MergeView`) on content change leaks *native* WebView2 memory** even when
  the JS side destroys the old view correctly. WebView2 does not promptly reclaim
  compositor/GPU/DOM-layer memory. Prefer dispatching a doc replacement into the
  existing view over `new EditorView`/`new MergeView`. Rebuild only on structural
  change (file/layout/extensions), never on content.

- **`@codemirror/merge` (v6.10) in-place update API:** `MergeView` — dispatch a
  whole-doc change into `merge.a` (baseline) / `merge.b` (working); it re-diffs.
  Unified `unifiedMergeView` — update the working doc with an ordinary dispatch,
  and the merge *original* with `originalDocChangeEffect(view.state, changeSet)`
  (read current length via `getOriginalDoc(view.state)`).

- **Tauri v2 `Channel.onmessage` is never released** — it lives in
  `__TAURI_INTERNALS__` for the page's life. A fresh `Channel` + closure per call
  is a per-call heap leak. Swap `onmessage` for a no-op (not `null` — the type
  forbids it) on the stream's terminal event. Terminal markers must be a *single
  unambiguous* end event, or a multi-message stream is cut short (SQL: whole-query
  `finished`, not per-statement `completed`).

- **The intent log (`edits.jsonl`) is append-only and unbounded** and was
  re-parsed on every 2s poll tick. `IntentCache` keys on the mtime+len of ALL
  files `intents::load` reads (edits, labels, AND `user::user_intents_path`) — miss
  one and a card move serves stale data. Compaction must stamp the key AFTER it
  rewrites, or its own rewrite looks like a change and it misses forever.

- **Cache miss ≠ every tick.** A miss only happens when a log actually changed
  (a new agent edit). Idle review (staging/committing, no new intent writes) is
  all hits → zero file reads. That's why compaction-on-miss is cheap: it only
  runs during active agent editing.

- **Do not hand-edit the live intent log while the old app runs** — it appends
  concurrently; a rewrite races. Let the rebuilt app self-compact.
