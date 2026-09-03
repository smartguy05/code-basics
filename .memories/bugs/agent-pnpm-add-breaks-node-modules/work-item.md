# Running `pnpm add` from an agent shell breaks `pnpm tauri dev` for the user

2026-09-02. **Caused by this session.** Not a pre-existing hazard — the CLAUDE.md
"junction block" note described it as something that afflicts *agent* shells, and
this is worse: it broke the **user's own** dev server.

## Symptom the user saw

    failed to load config from ...\vite.config.ts
    Error [ERR_MODULE_NOT_FOUND]: Cannot find package 'vitest' imported from
      ...\node_modules\.vite-temp\vite.config.ts.timestamp-....mjs

`vite.config.ts` imports `defineConfig` from `vitest/config`, so a `node_modules`
whose links cannot be followed stops the dev server before it starts.

## What is actually wrong

Every top-level entry in `node_modules` is a pnpm **junction**, and traversal
through them yields an **empty directory** rather than an error:

    Get-ChildItem node_modules\vitest -Force            -> 0 entries
    Get-ChildItem node_modules\.pnpm\vitest@4.1.10_...\node_modules\vitest -> 26 entries
    cmd /c "dir node_modules\vitest"                    -> resolves the path, then "File Not Found"
    fsutil reparsepoint query node_modules\vitest       -> valid Mount Point, correct 286-byte target

So the junction is well-formed and its target is intact. Windows is **refusing
to follow it** (Redirection Guard — the "untrusted mount point" / os error 448
in the CLAUDE.md note), and it reports that refusal as emptiness, which is why
it reads as "package not found" rather than "access denied".

## The trigger

`node_modules/.modules.yaml` and the junctions were all rewritten at 12:35 on
2026-09-02 — the moment this session ran

    pnpm add material-icon-theme lucide-react

from the agent shell. Junctions created by that process are untrusted for the
user's own shell afterwards. The session note "the block started right after
`pnpm add`" was the same event, misread at the time as bad luck.

## The rule

**Never run `pnpm add` / `pnpm install` from an agent shell in this repo.**
Ask the user to run it in their terminal (`! pnpm add …`). Editing
`package.json` and having the user install is fine; it is the *install* that
creates the junctions, and whoever creates them is who can traverse them.

## The fix (must be run by the user, in their own terminal)

    rm -rf node_modules
    pnpm install

`pnpm install --force` may not be enough — it can leave junctions it considers
current. Removing the directory guarantees they are recreated by the user's own
process.

## Durable prevention, if it recurs — the user's call, not a drive-by

A checked-in `.npmrc` with `node-linker=hoisted` makes pnpm write real
directories instead of junctions, removing this class of failure entirely. It
changes the install strategy and the disk cost for everyone on the repo, so it
is a decision to take deliberately. There is no `.npmrc` today.
