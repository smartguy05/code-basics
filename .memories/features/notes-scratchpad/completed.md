# Completed — Notes / Scratchpad panel

## Backend (cb-core)
- `crates/core/src/notes.rs` + `notes_tests.rs` — global notes store. `notes_path()`
  resolves `<config>/code-basics/notes.json` (`CB_NOTES_PATH` overrides whole path).
  `load()` tolerant (missing/corrupt → default, version=1 via manual `impl Default`),
  `save()` pretty JSON + trailing newline. Key-pinning test pins camelCase wire keys.
  Registered `pub mod notes;` in `crates/core/src/lib.rs`.
- `crates/core/src/review.rs` — `resolve_prompt_body(inline, library)`: inline body wins,
  blank inline falls back to library, neither = Err. Tests in `review_tests.rs`.
- `crates/core/src/enhancements/mod.rs` — `slugify`, `serialize_template` (round-trips
  through `parse_template`, `placement: end`), `save_template(dir, title, body)` →
  `<slug>.md`, refreshes same slug. Tests in `enhancements_tests.rs`. Added
  `anyhow::Context` import.

## Backend (src-tauri)
- `commands/notes.rs` — `read_notes` / `write_notes` (no AppState; user-global).
  `write_notes` param is `file: NotesFile` (avoid shadowing the `notes` module).
- `commands/enhancements.rs` — `save_note_as_instruction(app, title, body)` → seeded
  templates dir.
- `commands/review.rs` — `start_review` now `prompt_id: Option<String>` +
  `prompt_body: Option<String>`; looks up library only when no inline body, then
  `resolve_prompt_body`.
- `lib.rs` — `pub mod notes;` in commands + 3 commands registered in `generate_handler!`.

## Frontend
- `ipc/types.ts` — `Note` / `NotesFile`.
- `ipc/api.ts` — `readNotes`/`writeNotes`/`saveNoteAsInstruction`; `startReview` gained
  optional `promptBody` (7th arg) and `promptId` now `string | undefined`.
- `components/notesLogic.ts` + `.test.ts` — pure note ops + persistence keys (16 tests).
- `components/NotesPanel.tsx` — floating panel (reuses `reviewLayoutLogic` + `NOTES_LAYOUT_KEY`),
  tab strip, debounced autosave (400ms), flush on unmount, Send to agent + Save as instruction.
- `components/ReviewPanel.tsx` — `initialPromptBody` prop: hides prompt picker, drives run,
  never run-once.
- `App.tsx` — `notesOpen` state, titlebar **Notes** button, `openNoteInAgent` →
  `setAgentPanel({ initialPromptBody })`; `Note` import.
- `styles.css` — `.notes-panel` / `.notes-pill` (thin bar) / tabs / editor / footer.
- Docs: `pnpm docs:index` regenerated; `docs/reference/commands.md` updated.

## Verified
- `cargo test -p cb-core notes:: / review_tests:: / enhancements::` all green.
- Full `cargo test -p cb-core`: 2274 passed. `pnpm test`: 893 (incl. 16 notes). `pnpm typecheck` clean.
- `cargo check --workspace --all-targets` clean; clippy clean on new code.
- NOT yet run in the live app (`pnpm tauri dev`) — left for user manual verification.
