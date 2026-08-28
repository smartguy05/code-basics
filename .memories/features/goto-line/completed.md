# Completed — Ctrl+G goto-line (Rider-style)

## What
Bound **Ctrl+G** to CodeMirror's `gotoLine` command in every code editor, so the
user can jump to a line number the way Rider does. `@codemirror/search` (already
present for Ctrl+F) ships `gotoLine`, bound by default only to Alt-g; this adds
the familiar chord.

## Files
- `src/components/FileEditor.tsx` — Run tab / secrets editor.
- `src/components/DiffView.tsx` — the working pane and the read-only baseline
  pane of the side-by-side diff.
- `src/views/architecture/DiagramEditor.tsx` — the Mermaid editor.
- Docs: `docs/getting-started/using-the-app.md`, `docs/architecture/frontend.md`.

## Notes
- Pattern mirrors the existing `Mod-/` (toggleComment) binding: placed **ahead**
  of `...searchKeymap` with `preventDefault: true` so the WebView can't claim
  Ctrl+G first (some browsers use it for find-next).
- No unit test: keymap wiring has no DOM under vitest (node env), same as the
  existing `Mod-/`/searchKeymap bindings — components are untested rendering
  shells here.
- Verified: `pnpm typecheck` (confirms `gotoLine` export + Command type) and
  `pnpm docs:check` clean.
