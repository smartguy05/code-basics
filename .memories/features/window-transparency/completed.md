# Configurable window transparency — completed

## What it is

Settings → Appearance → **Window** carries an opacity slider (30–100%, default
100 = off). The window's background is painted at that opacity **only while the
active codebase's editor area holds no tab**. Open a file or a diff and it goes
fully opaque.

Floating panels (terminals, Notes, SQL, Review) deliberately do **not** count.
They paint their own opaque surface and stay readable over a translucent
window, which is the point of the feature rather than a gap in it — and they
are not in `openFiles`, so this is true by construction rather than by a filter
somebody has to maintain.

## The layers

1. `src-tauri/tauri.conf.json` — `"transparent": true` on the window. The
   **only** native change; reverting the whole native half is one line.
2. `src/styles.css` — `--app-bg-opacity: 100%` in `:root`, and `body`'s
   background becomes
   `color-mix(in srgb, var(--bg) var(--app-bg-opacity), transparent)`.
3. `src/appearanceLogic.ts` — `windowOpacity` + `clampWindowOpacity`.
4. `src/windowTransparencyLogic.ts` (+ test) — the runtime decision.
5. `src/windowTransparency.ts` — `applyWindowOpacity`, the single DOM writer.
6. `RunView` → `WorkspaceTab` → `App`, mirroring the `lspPollKey` route.

## Things that are load-bearing, not taste

**`color-mix(… 100%, transparent)` computes to exactly the colour.** At the
default the background is byte-identical to the plain `var(--bg)` it replaced,
so "indistinguishable when off" holds *in the CSS* and not only in the
predicate. There is no `data-` attribute to toggle and no second code path to
drift.

**`--bg` itself must not be given alpha.** Four other selectors use it as an
opaque paint (`.ws-tabs`, `.intent-panel pre`, `.intent-edit-prompt`,
`.ws-tab-rename`) and would go see-through with it.

**No schema version bump.** `readAppearance` re-derives every field
individually rather than spreading `parsed`, so an absent `windowOpacity` is
`undefined` → `NaN` → `100`, which is exactly "the user never chose this". A
bump would make the version gate reject every existing blob and fall through to
`DEFAULT_APPEARANCE`, **discarding every user's custom themes, active theme and
UI font size** — and `APPEARANCE_STORAGE_KEY` is the literal
`"code-basics.appearance.v1"`, so it would also force a key that lies about its
version. One test (`reads a blob written before the slider existed…`) pins this;
if someone later bumps the version it fails immediately.

**`clampWindowOpacity`'s non-finite fallback is 100, not the floor** — unlike
its two font-size neighbours, where either answer is harmless. Garbage here can
make the app unreadable, so the fallback is the *safe* value.

**Unknown is opaque.** `editorTabsByRoot` absent ≠ `false`. A tab is mounted
for a render or two before its effect runs, and a closed codebase's entry is
deleted while it may still be `activeRoot` for one render. Guessing "empty" is
what would flash the window translucent on startup and on every codebase
opened.

**The welcome screen is translucent.** It is the emptiest editor area there is
and has no editor to open. Making it the one opaque state would mean the app
starts opaque, flashes translucent when a folder is opened, then opaque on the
first file — three changes to arrive where it should have started.

**Two writers would fight.** `applyAppearance` deliberately does not write
`--app-bg-opacity`: it cannot know the editor state. The preference reaches
`App` through the existing `APPEARANCE_CHANGE_EVENT`, whose listener signature
was widened to receive the settings — which is what makes the Settings dialog's
*unpersisted* `preview()` reachable, so its buffered-draft model needed no
change at all.

**The state travels as a callback, not on `WorkspaceTabHandle`.** The handle is
for one-shot actions `App` invokes through a ref, and a ref is invisible to
rendering. This is a value `App` renders from, so it is a sibling of
`onLspPollKeyChange` — including the `null`-on-unmount rule so the entry is
**deleted**, not blanked.

**The reported value is `openFiles.length > 0`**, not `activeTab !== null`
(briefly `null` mid-close — it would flash) and not `!isDiff` (the *toolbar*
question; a diff is an editor tab and counts). Same term
`consolePanelLogic.shouldForceExpand` already uses.

## Degrades to opaque in five independent ways

The default; the clamp; `transparencySupport` (Windows only — macOS needs the
private `macos-private-api` feature, Linux needs an undetectable compositing
WM); the unknown-root abstain; and two CSS fallbacks (`@property`'s
`initial-value: 100%` and `@supports not (color-mix …)`).

`transparencySupport` is the **single seam** for switching the feature off if
Win11 compositing misbehaves — the CSS and the predicate stay as they are.

## Not verified yet — needs a run of the real app

Nothing here has been seen on screen. Specifically unconfirmed: whether WebView2
on this machine paints the transparent region as the desktop or as **black**
(a known GPU/driver variation); whether maximise/snap/restore leaves stale
regions; and legibility at 30% over a bright desktop. Record what actually
reproduces in `notes.md`.

## Known and out of scope

A custom theme whose `bgInset`/`bgRaised` is an `rgba` with alpha would leak the
desktop into a terminal — `isThemeDefinition` accepts any colour string and
`validThemeColors` only asks `CSS.supports`. If it becomes a complaint the fix
is a check on those two surface colours, not on the slider.
