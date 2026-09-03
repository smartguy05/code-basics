# Notes

## The junction block bit hard, and `pnpm add` is what triggered it

The session started with `node_modules` junctions traversable (`node -e
"require('react/package.json')"` worked). Running `pnpm add` recreated every
junction, and from that point on **every** shell in the session — Bash and
PowerShell alike — got `Permission denied` / `ERR_MODULE_NOT_FOUND` on
`node_modules/<pkg>`, while `node_modules/.pnpm/<pkg>@<ver>/node_modules/<pkg>`
stayed readable. It did not recover.

So: if you need the frontend gate in a session, **run it before you install
anything**, or expect to hand it to the user afterwards.

## The junction block CAN be routed around for typechecking — map `paths` at `.pnpm`

This is the important find of the session, and it beats the `tsc` + shim recipe
because it covers **`.tsx` files too**, which CLAUDE.md previously recorded as
simply unverifiable from a blocked session.

The packages are all really on disk under
`node_modules/.pnpm/<name>@<version>/node_modules/<name>` — only the junctions at
`node_modules/<name>` are unreadable. So generate a scratch tsconfig that extends
the real one with `baseUrl: "."` plus a `paths` entry pointing every package at
its **own** `.pnpm` directory, and typecheck with `npx tsc -p` against that.

Three details are load-bearing; each one on its own leaves hundreds of fake errors:

1. **Prefer the package's own `.pnpm` directory**, i.e. the one whose name is
   `<name with "/" replaced by "+">@<version>`. A naive first-match scan picks a
   *nested* copy (`@codemirror+autocomplete@…/node_modules/@codemirror/state`),
   which is itself a junction and fails exactly like the original.
2. **Map bare specifiers straight at their `@types` package.** Once `react`
   resolves through `paths` to untyped JS, TypeScript's `@types` fallback walks
   `node_modules/@types` — the very junction that is unreadable — so it never
   finds `@types/react`. Setting `typeRoots` does **not** fix this: typeRoots
   governs automatic *global* type inclusion, not the per-module fallback.
   Overwrite `paths["react"]` with the `@types/react` directory instead. This is
   safe because the check is `--noEmit`.
3. Add vite's `client.d.ts` to `include` for `?raw` imports.

**Result, measured: 1 error across the whole frontend** — and that one is
`src/reexportGuards.test.ts`'s `/// <reference types="vite/client" />`, an
untouched pre-existing file, because *type references* resolve through the same
`@types` walk and `paths` cannot redirect them.

The generator lives only in the scratchpad (`mkconfig.cjs`) and the tsconfig is
deleted after use — it hardcodes pinned version directories and would rot. It
also does **not** satisfy the `Stop` quality-gate hook, which runs the fixed
`pnpm typecheck` command; it is a verification tool for the agent, not a fix.

## The `tsc` + shim recipe does work, and it caught nothing but the known error

`npx tsc` resolves fine through the block (only the project's own `vitest`
import fails). Compiling the four pure logic modules plus their tests with
`--strict --noUncheckedIndexedAccess --noUnusedLocals` produced **only** the
four expected `Cannot find module 'vitest'` errors, and the compiled tests ran
**51 passed, 0 failed** through a hand-written shim.

Two shim details are load-bearing and were needed again here:

- `toEqual` must ignore keys whose value is `undefined` (vitest does).
- `expect(x, "message")`'s second argument is a **message**, never a negation
  flag. A shim that treats it as one inverts every such assertion.

Scan **stderr per file**: a test file that dies at `require` never reaches the
exit handler and prints `0 passed, 0 failed`, which reads as an empty file
rather than a crash. All four files here reported non-zero counts.

## `Record<LiteralUnion, string>` is not affected by noUncheckedIndexedAccess

`FileIcon` indexes `SOURCES: Record<IconKey, string>` with an `IconKey`. That
yields `string`, not `string | undefined`, because a Record over a finite
literal union has those exact properties rather than an index signature. This
is what lets the result go straight into `dangerouslySetInnerHTML.__html`.

## `?raw` imports need `src/vite-env.d.ts`

`tsconfig.json` declares no `types` entry and no `vite-env.d.ts` existed, so
Vite's ambient client types were reaching nothing. Without
`/// <reference types="vite/client" />` every `?raw` import fails to typecheck.

## Icon bundling: name each SVG, never glob

`material-icon-theme` ships **1251** SVGs. `import.meta.glob` over its icons
directory would pull all of them in to use twenty-nine. Each icon is imported
explicitly and the closed `IconKey` union makes a missing import a type error
rather than a blank square.

## `executeCommand` fallthrough is real, and is what makes per-codebase commands work

`src/shortcuts.ts::executeCommand` walks registrations **newest-first** and
treats any return other than `false` as handled, then falls back to clicking a
`[data-command]` element. So a component mounted once per open codebase can
register the same command and return `false` when it is not the visible one
(`rootRef.current.offsetParent === null` — the guard `OutputConsole` already
uses for `console.find`). Relied on by `tree.reveal`.

Caveat on the DOM fallback: `document.querySelector` takes the **first** match
in document order, which with several codebases open may be a hidden one. This
is pre-existing and affects every `data-command` without a registered handler.

## An icon-only button loses its tooltip if the title was `?? undefined`

The Run button was `title={selectedUnreadable ?? undefined}` — fine while the
label read "Run", useless once it is a glyph. Any button converted to an icon
needs its `title` fallback checked, not just kept.

## Parallel agents must not edit `styles.css` concurrently

Four Phase 1 agents all had CSS to add to one 4831-line file. They returned it
as a string in their structured output instead, and the orchestrator appended
the blocks sequentially in a fixed order. Concurrent edits to that file lose
updates silently.

## The scratch-tsconfig generator: split the .pnpm dir name on the FIRST `@`

Rebuilt the CLAUDE.md `paths` workaround this session and hit a silent hole in
it. A `.pnpm` directory name carries its peer suffix inline:

    vitest@3.2.4_@types+node@22.18.0_jiti@2.6.1

Splitting on `lastIndexOf("@")` lands **inside the suffix**, so the package
parses as something like `vitest@3.2.4_@types+node` with version `22.18.0`, the
existence check fails, and the package is dropped from `paths` **without any
error**. The symptom is a typecheck that still reports `Cannot find module
'vitest'` / `'lucide-react'` for a package that is installed and mapped for
everything else — which reads as a real missing dependency.

Use `indexOf("@", 1)` instead: the separator is the first `@` after index 0
(index 0 being a scope's own `@`). That took the map from 524 entries to 550,
and the run from 40+ errors to the documented **1** (`reexportGuards.test.ts`'s
`/// <reference types="vite/client" />`, which `paths` cannot redirect).

Generator kept at `scratchpad/mkconfig.cjs`; it is deliberately not checked in,
because it hardcodes resolved version directories and would rot.

## The vitest shim's `toHaveProperty` cannot use `arguments`

Writing the matchers as arrow functions in an object literal means
`arguments.length` silently reads the *enclosing* `expect(actual)` call's
arguments, so `toHaveProperty(key)` starts also asserting the value is
`undefined`. Take rest args and test `args.length` explicitly.

## The shim's exit hook decides whether a crash is visible

`process.on("exit")` still fires when a suite dies at `require` — so a file that
crashed printed `0 passed, 0 failed` and read as an empty suite rather than a
broken one. `viewportLogic.test.ts` was doing exactly that (it uses `it.each`,
which the shim lacked): six real tests were silently missing from the total.

Report from **`beforeExit`** instead. It does not fire on a crash, so the runner
loop sees no summary line and reports a crash — which is what CLAUDE.md's
"scan stderr per file" warning exists for, now enforced rather than remembered.
`beforeExit` also lets async tests settle, so `it` can chain promises instead of
refusing them.

Shim gaps found this way, all now filled: `it.each`, `toHaveBeenCalled(Times|With)`,
and async test bodies. With those, the sweep runs **1486 tests, 0 failed**.
