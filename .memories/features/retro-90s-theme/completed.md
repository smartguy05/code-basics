# Completed — 90's Retro Video Games theme (SNES/16-bit)

Added two builtin themes: **90's Retro (Dark)** and **90's Retro (Light)**, plus
a bundled pixel font on UI chrome only.

## Files touched
- `src/appearanceLogic.ts` — `retroDarkColors`, `retroLightColors` (all 37
  COLOR_KEYS), `retroFonts` (Press Start 2P on `ui`/--font, readable mono on
  `code`/--mono), two entries **appended** to `BUILTIN_THEMES` (ids
  `builtin-retro-90s-dark` / `-light`). Did NOT reorder index 0/1 (default +
  test-referenced).
- `src/styles.css` — `@font-face` for "Press Start 2P" (local TTF, `truetype`
  format); replaced stale "single dark theme" top comment.
- `src/assets/fonts/PressStart2P-Regular.ttf` (118 KB) + `OFL.txt` — new bundled
  asset (SIL OFL 1.1). Downloaded from google/fonts.

## Why local font
CSP is `default-src 'self'` with no `font-src` (`src-tauri/tauri.conf.json:25`),
so CDN/Google Fonts is blocked. Same-origin bundled font is covered by 'self' —
no CSP change needed.

## Verified
`pnpm typecheck` clean; `pnpm test` 2159/2159 pass. No test changes needed (no
test enumerates theme count; builtins referenced only by index 0/1).

## Not done (deliberate, per plan §4)
Hardcoded CodeMirror selection bg (`FileEditor.tsx:1282`) and
`tags.angleBracket` (`language.ts:39`) don't follow the palette; terminal ANSI
colors aren't themed (only bg/fg/cursor/selection derive). Left as-is unless
review flags the look. Not yet visually verified in `pnpm tauri dev`.
