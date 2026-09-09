# Todos

## OUTSTANDING should-fix from the adversarial review — not resolved

- [ ] **`src-tauri/build.rs:27` — the `-dirty` marker and the sha itself go
      stale in ordinary use, because `cargo::rerun-if-changed=../.git/HEAD` is
      not written by `git commit` and does not cover source edits. So About can
      assert a commit the binary is not built from.**

      Verbatim from the review, verified three ways in that session:

      1. `.git/HEAD` mtime is 2026-09-01 08:30 while HEAD's commit date is
         2026-09-03 11:56 — **committing on a branch writes the ref, not
         `.git/HEAD`**, so the only watch this script adds never fires on a
         commit.
      2. `touch src-tauri/src/lib.rs` then `cargo check -p cb-app --all-targets`:
         cb-app recompiled, but all three `target/debug/build/cb-app-*/output`
         files stayed **byte-identical** (`CB_BUILD_DATE` unchanged at
         1788468239 / …840 / …758) — the build script did not rerun.
      3. `tauri_build::build()` emits `rerun-if-changed` only for
         `tauri.conf.json`, `capabilities` and `resources/…` (read out of the
         same output files), so **nothing covers `src/`**.

      Failure scenario: build now (stamp `20a7dbb-dirty`), commit this branch,
      rebuild → About still reports `20a7dbb-dirty`, i.e. the previous commit
      plus a false dirty claim. Reverse case: build from a clean tree, edit a
      file, rebuild → About reports a **bare clean sha for a modified binary**,
      which `build.rs`'s own doc comment calls "a **wrong** statement about
      which code is running" and the plan ("a dirty tree **must** be marked")
      forbids. `CB_BUILD_DATE` is likewise the instant of the last build-script
      run, not "This build's instant" as its doc comment says.

      **Consequence for the manual checks below:** the plan's own Phase 3 check
      — "Make a scratch edit and rebuild → the sha shows as dirty" — **will not
      pass** as written.

      Directions considered but not taken (record before choosing): watch
      `../.git/HEAD`, the resolved ref file (`../.git/refs/heads/<branch>`) *and*
      `../.git/index` so a commit and a staged change both invalidate; or emit
      `cargo::rerun-if-changed` for the source tree, which relinks the shell on
      every keystroke; or accept `cargo::rerun-if-env-changed` / an always-rerun
      directive and pay the relink. None is free — the honest options trade
      staleness for rebuild cost, and the choice has not been made.

## Not verifiable headlessly — needs `pnpm tauri dev`. NONE of this is done.

`AboutDialog.tsx` and `MenuBar.tsx` are rendering shells the vitest suite
deliberately does not load (node environment, no DOM, and it cannot load `.tsx`
at all), so the tests pin the *intent* and only an app run pins the outcome:

- [ ] Help appears beside Enhancements; clicking opens it, and **hovering it
      while File is open switches to it** (the `onMouseEnter` handler — omitting
      it makes Help feel broken next to the other two).
- [ ] About shows `1.3.0`, a sha matching `git rev-parse --short HEAD`, and the
      OS/arch.
- [ ] **Escape** closes it, and so does **clicking the backdrop**.
- [ ] Commit a clean tree, rebuild → the sha shows **without** `-dirty`; make a
      scratch edit, rebuild → it shows **with** it. Only the dirty half has been
      seen at all (this tree has been dirty throughout), and per the should-fix
      above **this check is expected to fail** until the rerun triggers are
      fixed — it is the manual reproduction for that item.
- [ ] `getVersion()` / `getTauriVersion()` actually resolve under WebView2 at
      this origin. The permission is granted (verified in the ACL manifest — see
      `notes.md`) but **the call itself has never been made in a running app**.
      If either rejects, the dialog must show its error line rather than a
      half-filled table.
- [ ] `aboutLogic.versionDrift` renders nothing today (all four manifests read
      `1.3.0`), so its *displayed* form has never been seen. Bump one manifest
      locally to view it once.

## Deliberately not done

- The `unknown` branch of `git_sha()` was never **executed**: this machine has
  `git` and a `.git`, so that path is reasoned-through, not run. Verifying it
  needs a build from a tarball or with `git` off `PATH`.
- `-unverified` is likewise unreachable in practice (it needs `git rev-parse` to
  succeed and `git status` to fail with the same binary). It exists so the
  fallback cannot be "report the bare sha", which would claim a clean tree
  nobody checked.
- **No Copy-to-clipboard button.** The dialog exists to be pasted into a bug
  report and a copy button is the obvious next step, but the plan did not ask
  for one and it was left out rather than scoped in.
