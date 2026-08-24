# Completed — editor nav history + pinned tabs

Status: **implemented, typecheck + 690 tests + coverage + vite build all green.**
Not yet verified in the running Tauri app (see notes.md — WebView2 button risk).

## Files touched
- **NEW `src/views/editorNavLogic.ts`** — pure logic: `NavEntry`, `NavHistory`,
  `pushNav` (truncate-forward + dedupe + front-eviction cap `NAV_HISTORY_CAP=50`),
  `navBack`/`navForward` (return `{entry, history}` or null), `navMouseAction`
  (3→back, 4→forward), `partitionTabs<T extends {path}>` (order-preserving split),
  `togglePin` (immutable Set).
- **NEW `src/views/editorNavLogic.test.ts`** — 20 tests (written first). `src/views`
  now 100% lines in coverage.
- **`src/views/RunView.tsx`**:
  - Added `active: boolean` prop (Run tab visible).
  - Added `baseName()` module helper (didn't exist here before).
  - `pinnedFiles: Set<string>` state (in-memory, matches openFiles). `closeFile`
    also clears the pin.
  - `revealSeq` ref → `nextRevealToken()`: single monotonic reveal-token source.
    Replaced `pendingOpen.token` usage in the reveal with `nextRevealToken()`.
  - History: `navHistoryRef` + `writeNav` (ref-mirror idiom, like
    `inspectInfoRef`/`writeInspect`), `recordNav`, `applyEntry`, `goBack`,
    `goForward`, `nav` ref for the window listener.
  - Window `mousedown` (capture, preventDefault on button 3/4) + `auxclick`
    listener, gated on `active`.
  - `recordNav` called at 3 sites: FileTree `onOpenFile`, file-tab `onClick`,
    `pendingOpen` consume effect.
  - Extracted `renderFileTab(file)`; tab strip split into `.console-tabs.pinned-tabs`
    (top row, only when something pinned) + `.console-tabs` (normal, only when
    non-empty). Each tab has a 📌 `.row-action.pin` before the ×.
- **`src/App.tsx`**: pass `active={tab === "run"}` to `<RunView>`.
- **`src/styles.css`**: `.console-tabs.pinned-tabs { background: var(--bg-raised) }`
  and `.row-action.pin` (hidden until tab hover; full opacity when `.pinned`).

## Docs
- `pnpm docs:index` regenerated (`docs/INDEX.md`) — picks up `editorNavLogic.ts`.
- `docs/getting-started/using-the-app.md` (Run section): new paragraph on
  back/forward file navigation + pinned tabs.
- `docs/architecture/frontend.md`: new subsection "Editor navigation history and
  pinned tabs" after the openRequest/selectRequest section; RunView tree line +
  editorNavLogic.ts line added.
- `pnpm docs:check` passes.

## Design rationale
- History kept inside RunView (NOT lifted to App) — code comments explicitly
  reject lifting editor state up. App only learns `active`.
- All decisions in the `*Logic.ts`; RunView stays a rendering shell.
