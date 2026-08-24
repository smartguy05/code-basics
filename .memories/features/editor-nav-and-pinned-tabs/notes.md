# Notes / gotchas — editor nav + pinned tabs

## Must verify in the real Tauri app (couldn't headlessly)
Mouse buttons 3/4 must arrive as DOM `mousedown`/`auxclick` (`event.button` 3/4)
in **WebView2 on Windows**. Chromium delivers them normally and there's no SPA
router for a webview "back" to act on, so `preventDefault` on mousedown should be
enough. If they DON'T arrive, fall back to a `pointerup`/`mouseup` window listener
before reaching for any Tauri-side handling.
Test steps: Run tab → open A, B, C (tree) → Back = C→B→A, Forward = A→B→C;
middle-click a symbol to jump to its def in another file → Back returns to origin
file+line; buttons do nothing while on another tab; pin/unpin moves a tab between
rows; × and middle-click close still work; closing a pinned file clears its pin.

## Implementation gotchas
- `noUncheckedIndexedAccess` is ON → `history.entries[index]` is `T | undefined`.
  navBack/navForward use `!` (guarded valid) — see the two comments there.
- Reveal token: use ONE RunView-owned counter (`revealSeq`) for both palette
  opens and history jumps. Don't mix with App's `requestToken` (would risk a
  collide → editor's reveal effect only fires on a *changed* token).
- `recordNav` at the `pendingOpen` consume site is what wires palette + arch
  diagram + middle-click goto into history — don't move it into `openFile`
  (openFile is also called when reopening on Back, which must NOT record).
- Closing a tab must NOT push history (browser doesn't record a close as forward).
- Pin band uses `--bg-raised` (#1c1f26). `--bg-elevated` does NOT exist in
  styles.css (first draft referenced it by mistake).
