# Editor navigation history + pinned file tabs

## Request
User feature request (two parts), both scoped to the **Run tab editor pane**
(`src/views/RunView.tsx` — the only real editor in the app):

1. **Forward/Back mouse buttons** (browser side buttons, `MouseEvent.button === 3`
   back / `4` forward) navigate between the files the user has looked at, like
   browser history. Open A then B → Back returns to A, Forward to B. Includes
   **middle-click go-to-definition** jumps (those route through
   `App.requestOpenFile` → `pendingOpen`, so they feed the same history).

2. **Pinned tabs on a separate row.** A file tab can be pinned via an inline 📌
   control beside the existing ×; pinned tabs render in their own row above the
   normal file-tab strip.

## Confirmed decisions (AskUserQuestion)
- Nav scope: **Run-tab editor only** (active only while the Run tab shows; not
  whole-app / not across Changes/History).
- Pin affordance: **inline pin button** (not a right-click context menu).

## Acceptance
- Back/Forward mouse buttons step through opened files + goto jumps in the Run
  editor; only while the Run tab is visible.
- Middle-click goto is reachable by Back.
- Pin toggles a tab into a separate top row; unpin returns it; closing clears pin.
