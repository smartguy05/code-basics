# Todos

## Decide: the hook now points at this repo's build, not the installed app

`cargo build --release` **did** succeed — the running app was
`C:\Program Files\code-basics\cb-app.exe`, so `target/release/cb-app.exe` was
never locked. (The earlier assumption that the lock would bite was wrong; the
lock is on the *running* file, not on every copy.)

`.claude/settings.json` was then repointed from the Program Files install to
`target/release/cb-app.exe`, so the fix is live now. Two consequences:

- [ ] **`.claude/settings.json` is tracked**, so this shows up in the diff. It
      already held a machine-specific absolute path (the very thing
      `SHARED_EXE_PATH_NOTE` now warns about), so this is not a new problem —
      but decide before committing whether to keep it, revert it, or take the
      path out of version control entirely.
- [ ] **The hook now follows dev builds, not the installed app.** That is
      usually what you want while working in this repo, but a stale
      `target/release` will now silently be what gates your turns. Reinstall the
      app and revert the path if you would rather it track the product.
- [ ] The three **intent recorder** hooks were left pointing at Program Files
      and are unaffected.

## Follow-ups

- [x] The two pre-existing `cb-core` test failures are **fixed** (2026-09-02).
      Both were defects in the tests, not the code — see
      `.memories/bugs/two-red-cb-core-tests/`.
- [ ] Consider whether `pnpm test` deserves the same treatment — the gate does
      not run it today, but a vitest run behind a broken junction would produce
      the same class of unreadable failure.
- [ ] `TOOLCHAIN_BLOCKED` is a substring list. If it grows much past four
      entries it wants a test per marker rather than the two it has now.
