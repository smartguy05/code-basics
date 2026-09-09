# Completed

## Phase 1 — icons, run controls, branch search, tree action bar (2026-09-02)

Dependencies added: `material-icon-theme@5.38.1` (MIT — brand SVGs) and
`lucide-react` (ISC — UI chrome glyphs). Two packages because neither covers
both jobs. All 30 material icon filenames and all 6 lucide export names were
verified present on disk before acceptance.

New files:

- `src/vite-env.d.ts` — brings in Vite's ambient client types for `?raw`.
- `src/components/fileIconLogic.ts` + test (17 tests) — `iconFor(name, isDir,
  expanded)`. Exact filenames beat extensions; an unknown extension returns the
  generic document key.
- `src/components/FileIcon.tsx` — render shell, explicit `?raw` imports,
  inlined via `dangerouslySetInnerHTML` (the CSP forbids an external `<img>`).
- `src/components/fileTreeRevealLogic.ts` + test (14 tests) — `ancestorsOf`,
  `revealPlan`. A cached directory is still *expanded*; only the *fetch* list
  consults the cache.
- `src/components/branchFilterLogic.ts` + test (16 tests) — `hasQuery`,
  `branchMatches`, `filterBranches`, `expansionForQuery`. An empty query
  returns the same array/Set reference.

Edited:

- `src/components/FileTree.tsx` — file-type icons on both row kinds; a
  `.tree-actions` bar with Select-opened-file (`Alt+F1`) and Collapse All;
  sequential lazy reveal with `scrollIntoView({block:"nearest"})` fired from an
  effect keyed on a reveal token (the row does not exist until React commits
  the expansion).
- `src/views/ChangesView.tsx` — the same icon beside the git status letter on
  file rows and on folder rows. The status letter is kept: it says what git
  did, which no icon carries.
- `src/views/RunView.tsx` — Run→Play, Stop→Square, 🔨→Hammer, each keeping its
  `data-command` and `title` and gaining an `aria-label`. Run's title fallback
  was changed from `undefined` to a real sentence.
- `src/components/BranchMenu.tsx` — a filter box above Fetch/Pull/Push, behind
  a Search icon and a separator so it cannot be confused with the create-branch
  box; Escape clears then closes; matches force-expand their folders while a
  query is active without touching the stored expansion; an explicit
  "No branches match" row.
- `src/shortcutLogic.ts` (+ test) — `tree.reveal` (Alt+F1) and `tree.collapse`.
- `src/styles.css` — one appended Phase 1 block (4831 → 4993 lines).

Verified in-session:

- **The whole frontend typechecks with 1 error, and that error is pre-existing
  and untouched** (`reexportGuards.test.ts`'s `vite/client` type reference).
  Done by mapping `paths` at the real `.pnpm` directories to route around the
  junction block — see notes.md. This covers every `.tsx` change.
- 51/51 new logic tests pass (17 icons, 14 reveal, 16 branch filter, 4
  shortcuts) via the `tsc` + shim recipe.
- All 30 material-icon-theme filenames and all 6 lucide export names exist on
  disk; `executeCommand`'s fallthrough contract re-read and confirmed.

**Not verified in-session:** `pnpm typecheck` / `pnpm test` as the gate actually
runs them (junction block — the `Stop` hook cannot be satisfied here), the rest
of the vitest suite, and all runtime behaviour.

---

# Phase 2 — titlebar, status bar, tab rename (2026-09-02)

## Files

| File | Change |
|---|---|
| `src/App.tsx` | titlebar → three grid zones; LSP indicator mounted in `.statusbar`; `lspPollKeyByRoot` + `setLspPollKeyForRoot`; rename state, handlers, inline editor, tab `ContextMenu` |
| `src/styles.css` | `.titlebar` grid + three zone rules; `.statusbar .lsp-status` block; `.ws-tab-rename` |
| `src/components/lspStatusLogic.ts` (+test) | `LSP_POLL_KEY_SEP`, `lspPollKeyFor`, `activeLspPollKey`, `lspWatching` |
| `src/components/LspStatus.tsx` | `watching` prop; clears status on key change; `.lsp-status` hook |
| `src/components/WorkspaceTab.tsx` | reports the poll key up; `null` on unmount |
| `src/views/RunView.tsx` | toolbar mount removed; reports the key, filtered by `sourceEnablesLsp` |
| `src/components/workspaceRenameLogic.ts` (+test) | new — the `cb.workspaceTabs.labels` store |
| `src/components/workspaceTabsLogic.ts` (+test) | `tabLabels` takes overrides; `disambiguate` walks up |

## Six defects the adversarial pass caught, and the rule each one broke

1. **The poll key conflated identity with "keep polling".** `activeLspPollKey`
   returned `""` when the active codebase had no file open, so two file-less
   codebases produced the same key: the effect never re-armed and the previous
   codebase's server list stayed on screen under the new one's name. Split into
   `activeLspPollKey` (identity, always names the root) and `lspWatching`
   (whether to keep asking). **Two questions, two functions** — collapsing them
   into "empty means both" is what produced the bug.
2. **Stale status across a codebase switch.** One shared indicator now, so the
   effect clears `status` on entry rather than showing the previous codebase's
   servers for a round trip.
3. **Unmount blanked the entry instead of deleting it**, accumulating one dead
   key per codebase ever opened. `null` on unmount, `App` deletes — matching
   what `closeWorkspace` already does for attention.
4. **A `secrets:` tab turned the poll on forever.** It runs with the language
   server off and sends no `didOpen`, so it can never start a server. Now
   filtered by `sourceEnablesLsp`. Inherited from the pre-move code, but the
   hoist made it app-global instead of per-tab.
5. **Renaming one tab relabelled a different one.** Counting derived names over
   only the un-renamed tabs dropped `api` from 2 to 1 and un-prefixed
   `/two/api` from `two/api` back to `api` — a tab the user never touched
   changing because of a rename applied elsewhere, which is the exact harm the
   doc comment claimed to prevent. Count **all** derived names.
6. **`normalizeLabel` cut UTF-16 units and passed control characters through.**
   A cap landing mid-surrogate stored a lone surrogate that survived the JSON
   round trip and came back corrupt on every launch; U+202E reversed the rest
   of the tab. Now caps by code point and strips the unrenderable class.

Also fixed while in there: parent-segment disambiguation returned two identical
labels whenever the colliding roots shared a parent (the same repo on `C:` and
`D:`). `disambiguate` now walks up to the first depth that is actually unique.

## Two tests were corrected, not weakened

Both **pinned the defective behaviour** and would have kept it:

- `workspaceTabsLogic.test.ts` "never prefixes a renamed tab" asserted the
  untouched tab became `api`. That assertion *was* defect 5.
- `lspStatusLogic.test.ts` "says nothing to watch when the active codebase has
  no file open" asserted `activeLspPollKey("/a", {}) === ""`. That assertion
  *was* defect 1. The case moved to `lspWatching`, where it belongs.

Also made "still disambiguates two tabs that collide" discriminating — its
renamed tab's derived name did not collide, so it passed either way.

## Verification

- Frontend typecheck through the scratch `paths` config: **1 error**, the
  pre-existing `vite/client` reference in `reexportGuards.test.ts`.
- Logic suites via the shim: **1486 passed, 0 failed, 0 crashed** (58 suites).
- Not run: `pnpm test` (real vitest), `.tsx` rendering, and the manual pass.
