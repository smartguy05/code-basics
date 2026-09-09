# Three small UX fixes — completed

Grouped because each is a one-sitting change. The two larger items shipped with
them have their own folders (`sql-connection-rename`, `window-transparency`).

## 1. `Edit` moved into the run-configuration dropdown

The toolbar button (`RunView.tsx`) could only ever edit the **selection**, so
reaching another config meant selecting it first — which also changes what F5
runs. It is now a per-row `✎` in `RunConfigMenu`, beside the existing `↑`/`↓`/`★`
and using the same idiom (`.row-action`, `role="button"`, `e.stopPropagation()`
so the pencil does not also select the row). Wired to the `editing` state that
already existed and that `onNew` already set, so `ConfigEditor` needed nothing.

Editing a *detected* config was already permitted, so this adds reach, not
capability.

## 2. "Ask the codebase" in the Plugins menu

It had a `PLUGIN_LABELS` entry and a `plugin: "askCodebase"` tag on its command
but **no row in `PLUGINS`**, so the menu that exists to give optional features a
surface omitted the one feature nothing else advertises. Ctrl+/ still works.

Four edits: the `PLUGINS` row + a `{ kind: "ask" }` `PluginAction`; an
`openSignal?: number` prop on `AskPanel`; `openAsk()` on `WorkspaceTabHandle`;
one dispatch arm in `App.tsx`.

`openSignal` is a **monotonic counter, not a lifted `open` boolean**. The panel
owns its own visibility (and its Ctrl+/ registration, which is what keeps that
chord returning cleanly to CodeMirror's comment toggle when the feature is off),
so an outside caller only ever *asks* it to open — and re-opening a box the user
just closed changes no field a boolean could compare. Same request-and-consume
shape `App` uses for `openRequest`/`selectRequest`.

Unlike the chord, the signal path does **not** call `shouldAbstainForFocus`: a
menu row was clicked deliberately, and where the caret happened to be is not a
reason to refuse it.

Watch out when adding tests: `featureEnabled` returns **true** for a feature id
not present in the list, so a `FeatureInfo[]` fixture that omits `askCodebase`
now yields the Ask row. The existing tests had to name both features
explicitly.

## 3. Branch dropdown draws over the floating panels

`BranchMenu` used the generic `.dropdown` chrome — menu at `z-index: 41`,
backdrop at `40` — both far below `--z-panel: 60` and `--z-notes: 200`. Opened
from the **titlebar**, which is above everything, so a terminal or Notes covered
the menu *and* its click-catching backdrop.

Fixed with new generic classes in the same `styles.css` block as
`context-menu-elevated`: `.dropdown-menu.dropdown-elevated` and
`.dropdown-backdrop.dropdown-elevated-backdrop`. A class, not a number, because
no z-index integer is written in TypeScript.

**They sit two steps *below* the elevated context band on purpose.** BranchMenu
opens a right-click menu over itself, and today that submenu's backdrop (45)
sits above the branch list (41), so a click on a branch row closes the submenu
rather than checking that branch out. Putting both on one band would invert that
and check out a branch with a menu still open. Ordering preserved exactly:
submenu (−1) > submenu backdrop (−2) > branch menu (−4) > branch backdrop (−5),
mirroring the old 46 > 45 > 41 > 40.

The hand-rolled submenu was migrated to the shared `ContextMenu` with `elevated`
in the same change — which `CLAUDE.md` already asked for, and which gains it
Escape-to-close and viewport clamping. `OutputConsole`'s copy is still
hand-rolled; migrate it next time it is touched.

`.titlebar` sets no `transform`/`filter`/`contain`, so nothing was trapping the
menu in a lower stacking context — raising z-index was sufficient, with no
change to positioning.
