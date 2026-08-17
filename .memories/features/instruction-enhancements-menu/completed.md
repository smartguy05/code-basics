# Completed — Enhancements → Instructions menu

## What shipped
A titlebar **Enhancements → Instructions** dropdown that lists `.md` instruction
templates auto-discovered from a per-user directory and, on one click, splices a
template's marker-bounded section into **both** `CLAUDE.md` and `AGENTS.md` at a
placement the template declares in front matter. Installed items show an `added`
badge + ✕ to remove. First bundled template: the "CRITICAL: Memory Files" block
(placement `after-first-heading`).

## Root design
Generalizes the existing intent-block writer
(`intents/providers/instructions.rs`): marker-bounded, append-only, idempotent,
`.bak`-backed writes. Reused `providers::apply_writes` (extracted from
`apply_plan` in this change) so backups come for free.

## Files
- **core:** `crates/core/src/enhancements/mod.rs` (+ `enhancements_tests.rs`, 21 tests);
  `mod enhancements;` in `crates/core/src/lib.rs`; `apply_writes` added to
  `crates/core/src/intents/providers/mod.rs`.
- **tauri:** `src-tauri/src/commands/enhancements.rs` (list/add/remove_enhancement);
  registered in `src-tauri/src/lib.rs`. `bundled_dir` resolves `instructions`
  Resource dir (mirrors inspect.rs).
- **bundled seed:** `src-tauri/resources/instructions/memory.md`; mapping added to
  `src-tauri/tauri.conf.json` `bundle.resources` (committed, NOT gitignored —
  only the inspector resources are).
- **frontend:** `src/components/EnhancementsMenu.tsx`, `enhancementsLogic.ts`
  (+ `.test.ts`, 4 tests); mounted in `src/App.tsx` titlebar (keyed by root);
  `EnhancementInfo` in `src/ipc/types.ts`; wrappers in `src/ipc/api.ts`.
- **docs:** `docs/guides/instruction-enhancements.md` (+ README link);
  `docs/reference/commands.md` new section; `docs/INDEX.md` regenerated.

## Key facts
- Templates dir: `%APPDATA%\code-basics\instructions\` (else `$XDG_CONFIG_HOME`
  / `~/.config`); `CB_INSTRUCTIONS_PATH` overrides (dev). `seed()` copies bundled
  defaults, never overwrites user edits.
- Section markers: `<!-- code-basics: enhancement:<id> -->` … `<!-- /... -->`
  (namespaced to avoid colliding with the intent block's marker).
- Placements: top / after-first-heading / end / before-marker / after-marker;
  unresolvable anchors fall back to end (after-first-heading → top).
- CRLF preserved; blank lines normalized around insert/remove.

## Revision 2 — menu bar + prompts + confirm
- **Menu bar** (`src/components/MenuBar.tsx`, CSS `.menubar`/`.menu-button`/
  `.dropdown-submenu` in styles.css): **File** (Open/Rescan/Exit — Exit via
  `getCurrentWindow().close()`) and **Enhancements** with fly-out submenus
  **Instructions** + **Prompts**. Replaces the standalone `EnhancementsMenu.tsx`
  (deleted); titlebar Open…/Rescan buttons kept. Mounted in `src/App.tsx`.
- **Prompts**: core `PromptInfo`/`prompts_dir()`/`list_prompts()` (reuse
  `discover`/`parse_template`); command `list_prompts` (no state); bundled
  `src-tauri/resources/prompts/{code-review,write-tests}.md`; tauri.conf resource
  mapping; `CB_PROMPTS_PATH` dev override. Clicking copies body via
  `navigator.clipboard.writeText` (no plugin/capability change).
- **Confirm before add**: inline confirm row in the Instructions submenu
  (`confirmAddMessage`); remove stays one-click.
- **Open folder clears editor tabs**: `<RunView key={workspace.root}>` in App.tsx.
- Logic helpers + tests in `enhancementsLogic.ts(.test.ts)`.

## Gate
`pnpm typecheck` ✓, `pnpm test` (651) ✓, `cargo check -p cb-app`/`-p cb-core` ✓,
clippy clean on new code, `cargo test -p cb-core enhancements` 23/23 ✓,
`pnpm docs:check` ✓ (INDEX regenerated). Full `cb-core` suite (1957) ✓ from Rev 1.
Running via `pnpm tauri dev` with CB_INSTRUCTIONS_PATH + CB_PROMPTS_PATH set.
