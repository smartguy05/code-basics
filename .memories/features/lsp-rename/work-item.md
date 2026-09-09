# Work item — F2 rename symbol

Rider-style rename: put the caret on a class, method or variable, press F2, type
a new name, and every reference across the workspace is updated.

## Acceptance

- F2 opens an inline field over the symbol, prefilled and preselected.
- Enter applies the rename across every file the language server names; Escape
  and blur cancel.
- **No text-heuristic fallback.** A server with no `renameProvider`, or no server
  at all, refuses and says what it looked for. `symbols/` is not a fallback for
  an LSP fact — a wrong rename is unrecoverable across files, whereas a wrong
  near-miss in the search palette costs a keystroke.
- The six `Availability` answers stay six answers. "No server", "starting",
  "loading", "does not support rename", "failed", and "renamed nothing" are all
  distinct, and `total: Some(0)` never reads as a refusal.
- Open buffers are edited in place (CodeMirror `dispatch`); closed files are
  written by Rust. Never a disk write behind an open tab — there is no file
  watcher, so the next Ctrl+S would clobber it.
- A rename is refused whole, having touched nothing, if any edit overlaps,
  any range names a line the document does not have, any uri falls outside the
  workspace, or the server asks for a file create/rename/delete.

## Decisions taken with the user

| Question | Decision |
| --- | --- |
| No `renameProvider` | refuse and say why; no heuristic fallback |
| UX | inline field, applies directly — no confirmation preview |

Plan: `C:\Users\AnthonyJames\.claude\plans\major-enchancements-add-witty-hopper.md`
(this is Phase 1 of four; the other three are the SQL MCP server, the previewed
MCP installer, and the browser plugin).
