# Todos — Editor Context MCP

## Done
- [x] Stage 1 — contracts (model.rs, types.ts, tool_gate, features, installers)
- [x] Stage 2 — cb-core pure module `editor_context/`
- [x] Stage 3 — src-tauri backend (state, pipe host, runner, commands, self-dispatch)
- [x] Stage 5 — frontend
  - [x] `editorContextLogic.ts` (+ test): assemble/dedupe/cap/equality
  - [x] `FileEditor.tsx` `onEditorContextChange` (debounced updateListener)
  - [x] `RunView.tsx` build + push (feature-gated, debounced, dedup)
  - [x] `api.ts` `setEditorContext` + 5 install wrappers
  - [x] Plugins menu + `EditorMcpPanel` install UI + Settings MCP-tools toggle
- [x] **Stage 6 — docs**
  - [x] `pnpm docs:index` (regenerated INDEX.md; NOT hand-edited)
  - [x] Document the 6 new commands — placed in the NEW
        `docs/reference/mcp-servers.md` (extracted from commands.md, which was at
        the 500-line `docs:check` limit) with a pointer left in commands.md
  - [x] New guide `docs/guides/editor-context-mcp.md` (mirrors
        `roslyn-mcp-server.md`)
  - [x] `pnpm docs:check` → passed (31 files, all <500 lines, links resolve)

## Remaining
- (none for docs) — feature docs complete.

## Known limitations (documented, not bugs)
- Only the **active** workspace tab's editor contributes cursor/selection/
  viewport. Under split docking, a docked non-active editor's caret is not
  reported. See notes.md (Stage 5). Revisit only if multi-visible is wanted.

## Not verifiable in this environment
- End-to-end manual run (build release + `pnpm tauri dev` + drive an MCP client
  against `cb-app.exe mcp-editor --workspace <root>`): needs a live Tauri window.
