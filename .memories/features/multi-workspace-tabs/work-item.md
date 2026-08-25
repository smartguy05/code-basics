# Multiple codebases open at once (workspace tabs)

## Goal
Hold several codebases open simultaneously, each as a **top-level tab**. Opening
another folder ADDS a tab (never evicts the current one).

## Confirmed requirements (from user)
- **Background tabs stay LIVE.** Switching tabs must not stop a background tab's
  processes — running test/run, a Claude Code session in a terminal, and the
  language server all keep running; console scrollback preserved.
- **Terminals belong to a workspace tab.** Only the active tab's terminals show;
  others keep streaming while hidden.
- **Notes stay GLOBAL** — one shared Notes panel across all tabs. Do NOT touch
  notes.rs / NotesPanel data model.
- **UI: new top-level tab strip** above the inner Run/Tests/Changes/History/
  Architecture/Objects tabs, with `+` (open another folder) and per-tab close.

## Chosen approach
- **Backend:** shard `AppState` into `HashMap<canonical root, Arc<WorkspaceSlot>>`
  + an `active: Mutex<Option<PathBuf>>` pointer. Root path IS the workspace id.
  ~104 argument-free commands resolve the ACTIVE slot and compile ~unchanged.
  Switching = pointer move (tears nothing down); teardown on CLOSE.
- **Frontend:** one `<WorkspaceTab>` component per open root, mounted-but-hidden
  when backgrounded (React preserves state/DOM → live processes survive).

Full plan: `C:\Users\AnthonyJames\.claude\plans\imperative-munching-planet.md`

## Key correctness points
- Per-slot `Supervisor` + `last_test_run` (fixes root-relative config_id COLLISION
  across same-layout repos — today B's run silently evicts A's handle).
- Per-slot `lsp_generation` and `symbols_build` (a global counter breaks the
  ordering check across workspaces).
- Active-pointer handshake: await `set_active_workspace(root)` BEFORE revealing a
  tab; prop-ify the self-fetchers (ChangesView, SearchEverywhere); gate background
  polls on `active`.
- Canonicalize root at every insert/lookup (dunce::canonicalize).
