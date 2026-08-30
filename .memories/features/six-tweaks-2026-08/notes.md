# Notes

## The frontend gate could not be run in this session

`node_modules/*` are pnpm junctions the agent shell cannot traverse
(`Permission denied` from `ls`, `ERR_MODULE_NOT_FOUND` from node) — the failure
mode CLAUDE.md already documents. What was tried, and what worked:

- `pnpm test` / `pnpm typecheck` — unrunnable.
- **`node node_modules/.pnpm/typescript@5.9.3/node_modules/typescript/bin/tsc`
  does run** from the store path. Against the whole project it drowns in
  `TS2307 Cannot find module 'react'` and its knock-on
  `TS7026`/`TS2875`/`TS7006`/`TS2503` noise, but two things are still worth
  doing: filter the output to the touched files and discard those codes, and
  **typecheck the pure `*Logic.ts` modules standalone**
  (`tsc --noEmit --strict --target es2022 --moduleResolution bundler --module esnext <files>`),
  which needs no junction and gives a real, clean answer.
- `vitest` from the store path does **not** run — it cannot resolve its own
  `@vitest/utils` through the junctions. So `pnpm test` must be run by the user.

Two pre-existing `TS2345` errors at the `partitionTabs` call site in
`RunView.tsx` are the same noise (`T` degrades to `{ id: string }` once
`OpenFile` fails to resolve). Verified unchanged from `HEAD` before dismissing.

**The noise really does hide real errors — read it, do not dismiss the run.**
The `Stop` quality-gate hook surfaced one this session that the filtered check
had missed: `TS2304: Cannot find name 'report'` in `runningLogic.test.ts`. The
`report()` fixture was a `const` declared *inside* one `describe`, so new
top-level `describe` blocks could not see it. Hoisted to module scope beside
`rec()`. Grepping for `^function ` missed it precisely because it was a `const`
arrow inside a block — search for the *name*, not the declaration shape.

- Cargo works via the documented escape hatch:
  `TC="$USERPROFILE/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin";
  PATH="$TC:$PATH" RUSTC="$TC/rustc.exe" "$TC/cargo.exe" test -p cb-core`.

## Writing source files from a bash heredoc mangles backslashes

Three separate breakages this session: a Rust `"src\a.rs"` came out as `"src\a.rs"`
(an unknown-escape compile error), a JS regex `/\/g` came out as `/\/g`, and a
character class written with real control characters put a **NUL byte** into a
`.ts` file. Backslash-bearing literals go through the `Write`/`Edit` tools, not
through `python - <<'PY'`; and `grep` reporting "Binary file … matches" is the
tell that a control character got in.

## `sort_configs` already tolerates ids that name nothing

The plan budgeted a config-id migration for the collapsed Debug/Release fanout.
It is not needed: `config::sort_configs` looks each `favorites`/`order` id up and
an unmatched one simply never applies. Keeping the surviving entry's id as
`:run:debug` means the common favourite keeps working with no migration at all.

A migration would in fact have been **dangerous**: a launch profile's id is also
`:run:{sanitised name}`, so a rule rewriting `:run:<anything>` would have
destroyed the launch-profile configurations too.
