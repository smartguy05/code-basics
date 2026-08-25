# Completed — Secrets as "just another file"

## Goal
User: ".NET user secrets management isn't nice/simple — manage like Rider, as
just another file." Replace the cramped modal (`SecretsEditor.tsx`, 14-row
`<textarea>`) with secrets opening in the app's real CodeMirror `FileEditor` as a
tab: JSON highlighting, line numbers, Ctrl+F find, Ctrl+S save.

## What changed (frontend only — no Rust/IPC change)
- **New** `src/components/editorSourceLogic.ts` (+ `.test.ts`): `EditorSource`
  union (`workspace` | `secrets`), `OpenEditorFile { id, name, source }`, and
  helpers `workspaceFile`/`secretsFile`/`sourceEnablesLsp`/`sourceLanguageHint`/
  `EMPTY_SECRETS`. Identity for a secrets tab is `secrets:<project>` (can't
  collide with a workspace-relative path).
- **`FileEditor.tsx`**: prop `path: string` → `source: EditorSource`. Derives
  `identity`, `lspEnabled`, `readSource()`/`writeSource()`. Reads/writes secrets
  via `read/writeProjectSecrets`; workspace via `fs*`. **LSP surface fully gated
  behind `lspEnabled`**: no `usagesExtension` installed, no `lspOpenDocument`, no
  flush timer for secrets — so a secrets tab shows no "Usages unavailable" badge
  and makes zero server calls. `languageFor(sourceLanguageHint(source))` gives a
  secrets tab JSON highlighting.
- **`RunView.tsx`**: `OpenFile` = `OpenEditorFile`; every per-file map re-keyed
  from `.path` to `.id` (`openFiles`, `activeFile`, `dirtyFiles`, `pinnedFiles`,
  `reveal`, `closeFile`, `renderFileTab`, render map, pollKey). Added
  `openSecrets(project)`; wired the existing "Secrets…" button to it. Removed
  `secretsFor` state + `<SecretsEditor>` render + import. Secrets tabs are kept
  out of the back/forward nav stack (workspace paths only).
- **`editorNavLogic.ts` `partitionTabs`**: generic constraint `{ path }` → `{ id }`
  (+ test updated).
- **Deleted** `src/components/SecretsEditor.tsx`.

## Verification done
- `pnpm test` → 877 pass (incl. new editorSourceLogic suite; test-first, watched
  it fail on missing module first).
- `pnpm typecheck` → clean. `pnpm docs:index` + `pnpm docs:check` → clean.
- **NOT yet run in the live app** (`pnpm tauri dev`) — GUI verification of the
  open/edit/Ctrl+S/persist round-trip and the no-usages-badge check is the
  remaining manual step.

## Deliberate simplification
Modal's "no UserSecretsId yet — saving adds one" banner dropped. Id + file are
created on **first save** (`ensure_id` in the write command); opening mutates
nothing. Rider-like.
