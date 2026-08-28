# Notes

## The secrets.json bug was a BOM, not the comments

Reported as "parsing errors for secrets.json when including comments in json". Every
comment shape already worked. A probe test
(`the_dialect_dotnet_accepts_round_trips_shape_by_shape`) ran 18 shapes .NET's loader
accepts — line comments in five positions, block comments in four, trailing commas in
three, CRLF, nested objects, non-ASCII — and exactly one failed:

    UTF-8 BOM: expected value at line 1 column 1

.NET's reader skips a leading BOM; `serde_json` refuses it. `dotnet user-secrets` and
Rider both write one, and it is invisible in the editor — so the file's *comments* were
the only unusual thing the user could see, and got the blame.

**Lesson worth keeping: when a bug report names a feature, check the feature is actually
implicated before fixing it.** The probe was cheaper than reading `strip_jsonc` closely,
and it disproved the reported cause in one run.

The error message now quotes the offending line, so the next instance of this diagnoses
itself. Only a *leading* mark is stripped — one mid-file is a real error, and one inside
a string is data the user typed.

## Why an xterm scrollbar renders but cannot be grabbed

WebView2's default scrollbar is an **overlay**: it paints over the content and takes no
layout width. `FitAddon` sizes the terminal from the viewport's client width, so with an
overlay bar it measures a scrollbar width of zero and lays `.xterm-screen` — a later,
`position: relative` sibling — right across the strip the bar is drawn in. Every press on
the thumb lands on the screen element and starts a text selection instead.

Fix: ask for a real scrollbar (`::-webkit-scrollbar { width: 10px }` on
`.xterm-viewport`), which takes the width back out of the layout. Applied globally — the
same viewport is in every console and terminal in the app.

**Still to confirm visually.** The diagnosis is from reading the layout, not from
measuring the running app. The check: compare `.xterm-screen` `offsetWidth` against
`.xterm-viewport` `clientWidth` in devtools; equal widths confirm it.

## A resizer over a scrollbar, found but not fixed

`.sidebar-resizer` (`styles.css:301`) has `margin-left: -4px` and `z-index: 5`, so it sits
on top of the Run sidebar's scrollbar, and `Sidebar.startDrag` calls `preventDefault()` on
mousedown — turning a thumb grab into a panel resize. Same class of bug as the one above,
different mechanism. Out of scope; not reported by the user. Worth fixing if it comes up.

## A build's exit callback holds a stale session list

`api.buildProject(id, action, e => handleEvent(session, e))` captures `handleEvent` from
the render where the build *started*, so its `sessions` is whatever the list was then.
Closing a session directly from the `exited` case would drop any session opened while the
build ran. Hence `setPendingClose(id)` and an effect that closes on a fresh render.

## This environment cannot run `pnpm test` or `pnpm typecheck`

`node_modules` is a pnpm store reached through junctions, and this agent shell cannot
traverse them (`Test-Path` through the junction is false; the target itself exists), so
node cannot resolve any dependency — vitest, tsc's module resolution, esbuild's bundler.
`dangerouslyDisableSandbox` does not lift it. Cargo hits the same wall: use
`~/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/{cargo,rustc}.exe` with `RUSTC`
set explicitly, and put Git Bash on `PATH` or the `process::` tests fail.

**The `Stop` quality-gate hook fails for this same reason**, and its failure is not a
code failure: it runs `pnpm typecheck`, which reports `Cannot find module 'react'` in every
file including untouched ones (`src/main.tsx`, `vite.config.ts`). The decisive check —
`Test-Path node_modules/react/package.json` is **false** while `Test-Path` on that
junction's *target* is **true** — proves the traversal, not the install, is what is broken.
Do not "fix" this by editing source or by relaxing the gate; verify with the workarounds
below and say plainly that the hook cannot pass in this sandbox.

Workarounds that **do** work, and are worth rebuilding if needed:

- **Type check**: generate a `tsconfig` whose `paths` map every package name to its real
  `node_modules/.pnpm/<pkg>@<ver>/node_modules/<pkg>` directory, preferring the matching
  `@types/` entry first. tsc then resolves everything without touching a junction. Run it
  with `node node_modules/.pnpm/typescript@<ver>/node_modules/typescript/lib/tsc.js`.
- **Frontend tests**: run `esbuild.exe` straight from
  `node_modules/.pnpm/@esbuild+win32-x64@<ver>/node_modules/@esbuild/win32-x64/` to bundle
  each `*.test.ts` with `--alias:vitest=<a shim>`, and run the bundle under node. A
  ~120-line shim covering `describe`/`it`/`expect` and the matchers this repo uses is
  enough for every suite except the two using async tests and `vi` mocks.
