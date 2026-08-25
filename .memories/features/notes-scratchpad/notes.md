# Notes / gotchas — Notes-scratchpad WI

- **Pre-existing test failure, not mine:** `architecture::mermaid_tests::mermaid_id_matches_committed_fixture`
  fails on this Windows checkout with a CRLF-vs-LF diff (left `\r\n` = committed fixture,
  right `\n` = generated). I did not touch mermaid code/fixtures. Do NOT regenerate the
  fixture as part of this feature — it's a git autocrlf line-ending artifact. Confirmed via
  `git diff --name-only` (no mermaid file in the diff).
- **`NotesFile::default()` needs a manual `impl Default`** to get `version: 1`; the serde
  `#[serde(default = ...)]` only fires on deserialize, not on `Default::default()`. Deriving
  `Default` gives `version: 0`. (A test caught this.)
- **`start_review` param shape changed** — now `prompt_id: Option<String>` +
  `prompt_body: Option<String>` (was `prompt_id: String`). Any new caller must send both keys;
  the TS wrapper passes `promptBody` as the 7th positional arg.
- **Tauri command param naming:** `write_notes(file: NotesFile)` — JS sends `{ file }`.
  Named `file` (not `notes`) to avoid shadowing the imported `notes` module.
- **vitest `noUncheckedIndexedAccess` is on** — index into arrays with `[i]!` in tests
  (project idiom, see erosionLogic.test.ts / treeLogic.test.ts).
- Notes are **user-global** (`%APPDATA%/code-basics/notes.json`), so no `.code-basics/`
  gitignore entry and no `AppState` in the commands — same as `enhancements::list_prompts`.
