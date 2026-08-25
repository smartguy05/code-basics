# Notes — Secrets as a file

- **`FileEditor` is the sole client of the LSP surface, all keyed on `path`.** To
  host a non-workspace file (secrets live outside the workspace, backed by the
  secrets commands, not `fs_*`), the clean seam was an `EditorSource` prop + an
  `lspEnabled` gate — not a second editor. When adding any other external/virtual
  editor source, gate the same three spots: (1) don't include `usagesExtension`,
  (2) skip the `lspOpenDocument` block (guarded so no `didClose` is owed —
  `openSent` stays false), (3) don't arm the change→flush timer, and early-return
  in `flushChange` so its "not opened yet, retry" reschedule can't spin forever.
- **Tab identity is `file.id`, not a path.** Secrets id = `secrets:<project>`.
  Anything keyed on the old `.path` had to move to `.id` (including
  `partitionTabs`'s generic constraint). Nav back/forward stack stays
  workspace-path-only — never push a secrets id or Back will try `fsReadFile`
  on `secrets:...csproj`.
- **FileEditor had no visible caret.** Added `drawSelection()`+`dropCursor()`.
  Selection then showed but the caret still didn't. Root cause: `drawSelection`
  force-hides the native caret (`caret-color: transparent !important`), and the
  *drawn* caret `.cm-cursor` is `display:none` until CM's base rule
  `&.cm-focused > .cm-scroller > .cm-cursorLayer .cm-cursor { display:block }`
  matches — that strict child-combinator+focus chain does not hold in this WebView.
  Fix: in `FileEditor`'s `EditorView.theme`, `&.cm-focused .cm-cursor { display:
  block }` (a 2-class descendant selector that outranks the 1-class base hide),
  coloured `var(--text)` at 2px. DiffView/DiagramEditor share the latent gap
  (not fixed).
- **Ctrl+/ comment toggle**: `defaultKeymap` already binds `Mod-/`→`toggleComment`,
  but WebView2 can swallow the chord first, so an explicit
  `{ key: "Mod-/", run: toggleComment, preventDefault: true }` is listed ahead of
  it. No-ops on JSON (no comment tokens) — expected.
- Backend (`read/write_project_secrets`, `cb_core::secrets`) needed **no** change:
  it already resolves the external file, does `ensure_id` on first write, and
  validates JSON. This was purely an editing-surface change.
