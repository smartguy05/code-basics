# Notes — optional features

## Verified facts about the installers (do not re-derive these)

Checked against `tauri-apps/tauri@dev`
`crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi` (977 lines), for the
Tauri **2.11.4 CLI / 2.11.5 runtime** this repo pins.

- **NSIS `installerHooks` cannot host a wizard page.** The hooks file is
  `!include`d at **line 35**, before every page declaration (MUI pages start at
  169). NSIS page order is declaration order, so a page declared in a hook lands
  *before* the Welcome page and there is no way to move it. Hooks are only usable
  for the four `NSIS_HOOK_{PRE,POST}{INSTALL,UNINSTALL}` macros.
- **There is no `MUI_PAGE_COMPONENTS`** in the template, so the built-in
  components page is not an option either.
- The only route to a correctly-placed page is **`bundle.windows.nsis.template`**
  (a full fork). The template already uses `nsDialogs` for its own
  `Page custom PageReinstall` (line 187), so the technique needs no new tooling.
  Insert after `!insertmacro MUI_PAGE_DIRECTORY` (line 389), before
  `MUI_PAGE_INSTFILES` (line 402).
- **A `.deb` cannot ask the question.** `bundle.linux.deb` exposes
  `preInstallScript` / `postInstallScript` / `files` / `depends` /
  `desktopTemplate`, and `appimage` exposes only `files`. Nothing reaches the
  Debian **control** archive, so debconf `config` + `templates` are unavailable.
  The `.deb` ships a defaults seed via `files`; the in-app picker is the real
  surface on Linux.
- `targets: "all"` already emits `.deb` / `.AppImage` / `.rpm` **when built on
  Linux**. There is no cross-compilation from Windows.
- **A Linux build has never been attempted here**, and its Objects tab will be
  inert: `scripts/build-sidecar.mjs` publishes only `win-x64` and `win-x86`
  (lines 34-35), so no `cb-inspector` ships. The app reports that as unavailable
  rather than failing — a known gap, not a bug.

## Why `merge_seed` is all-or-nothing

A key-by-key merge would have to decide what an *absent* key in the user's file
means. It means "no opinion, use the default" — which is indistinguishable from
"the seed should fill this in". Taking the user's file whole keeps the two apart.

The cost: a seed never adds a newly-shipped feature to an existing store. That is
fine, because a new feature already arrives enabled through
`FeatureId::default_enabled`.

The bug this prevents: a repair install re-enabling a feature the user turned off.

## Why the defaults are ON

Most launches never see an installer — `cargo run`, a dev checkout, an AppImage.
Defaulting off would make all of those look broken. The installer's job is to let
someone turn a feature *off*, not to be the only thing that can turn it on.

This is also what makes `featureEnabled(null, …)` returning `true` safe: "not
loaded yet" and "loaded with defaults" agree for everyone except a user who has
turned something off, and for them one frame of an extra tab is a much smaller
wrong than a missing tab for everyone else.

## `cancel_stops_a_long_running_process` is flaky, and adding tests makes it worse

While landing this, `process::tests::cancel_stops_a_long_running_process` began
failing intermittently under the **full** lib suite. Measured on identical code:

| Run | Result |
|---|---|
| lib suite, features tests included | FAIL, FAIL, ok, ok |
| lib suite, `-- --skip features::` | ok |
| that test alone | ok |
| lib suite on the pre-change tree | ok |

So it is **not a regression** — this work touches nothing in `process/`. Adding
26 tests raised the concurrency in the window where it runs, and tipped a
pre-existing race.

**The mechanism:** the failing line is `assert!(sup.cancel("long").await)`.
`Supervisor::cancel` returns `kill::kill_tree_async(pid)`, which on Windows is
`taskkill /PID <pid> /T /F` and returns *its exit status*. That status is load
sensitive, so the assertion is really "taskkill succeeded on the first try",
which is an implementation detail rather than the behaviour the test names. The
assertions after it — that the runner completes promptly and a cancelled exit is
observed — are the ones that actually test cancellation.

Left alone deliberately: weakening someone else's test to go green is exactly
what CLAUDE.md forbids. Flagged to the user instead. If it is ever addressed, the
fix is to assert the *observable* outcome (the process stopped) rather than
taskkill's return code.

## The frontend gate could not be run from the agent shell

`pnpm typecheck` produced ~3100 errors, essentially all `Cannot find module
'react'` / `TS7026` cascade. Confirmed as the documented pnpm junction problem,
not a code problem: `node_modules/react/package.json` is unreadable while
`node_modules/.pnpm/react@19.2.8/node_modules/react/package.json` exists.

**One real error did hide in that noise** and was found by filtering the error
*codes* rather than dismissing the run: `TS2532 Object is possibly 'undefined'`
in `featuresLogic.ts` (`visible[0].id` under `noUncheckedIndexedAccess`). The
technique that found it — `tsc --noEmit | grep -oE "TS[0-9]+" | sort | uniq -c`
and look at anything that is not the cascade codes (TS2307/7026/7006/2875) — is
worth reusing.

`pnpm test` / `pnpm coverage` still need to be run by the user.

## The junction block is the process, not the tool sandbox — and `!` does not escape it

Established on 2026-08-30, after `pnpm tauri dev` was tried and failed:

- `pnpm tauri dev` dies before building anything: `failed to run 'cargo metadata'
  ... The path cannot be traversed because it contains an untrusted mount point.
  (os error 448)`. Same restriction as the node side, different victim.
- **Running the same command with the tool sandbox disabled changes nothing** —
  `node_modules/react/package.json` is still `Permission denied` and the
  `~/.cargo/bin/cargo` shim is still `Permission denied`. So this is the Claude
  Code process's own token, not the per-command sandbox. CLAUDE.md already said
  disabling the sandbox does not help; this confirms it for cargo too.
- **The `!` prefix runs inside this same session**, so telling the user to type
  `! pnpm tauri dev` does *not* work — it fails identically. Anything needing
  `node_modules` or the cargo shim has to be run from a terminal started
  independently of Claude Code.

Root cause is *not* OneDrive, which was the obvious guess and is wrong:
`C:\Users\AnthonyJames\Documents` is a plain directory (`LinkType` empty), even
though the *shell folder* registry entry redirects "Documents" to
`C:\Users\AnthonyJames\OneDrive - ONEflight International\Documents`. The
`node_modules` entries are ordinary local junctions to ordinary local `.pnpm`
paths. Something about this process's integrity level is refusing to traverse
them; that is the open question if anyone wants to fix it for good.

**Lead, not a conclusion:** os error 448 is `ERROR_UNTRUSTED_MOUNT_POINT`, the
error Windows **Redirection Guard** (KB5014019) raises when a process refuses to
follow a junction. That fits the symptom exactly. But
`Get-ProcessMitigation -System` and the per-image entries for `node.exe` /
`claude.exe` / `pnpm.exe` all came back with `RedirectionTrust` **unset**, so the
policy is not explicitly configured and this is *not confirmed*. Anyone picking
it up should start there rather than re-deriving the symptom.

`pnpm install` fails the same way (on `.pnpm/vite@6.4.3/node_modules/esbuild`,
itself a junction inside the store) — but it prints "Lockfile is up to date /
Already up to date" first, which is good evidence the install is **intact** and
only traversal is broken. Do not "fix" node_modules; there is nothing wrong with it.

## Windows installer: why the feature page is a forked NSIS template

The page could not live in `bundle.windows.nsis.installerHooks`. Tauri's
`installer.nsi` (2.11.4) `!include`s the hooks file at line 35, **before every
page declaration** (MUI pages start at line 169), and NSIS page order is
declaration order — a page declared in a hook lands before Welcome and cannot be
moved. `MUI_PAGE_COMPONENTS` is not an option either: the template never inserts
it, and the feature set is not a set of NSIS sections. So
`src-tauri/installer/windows/installer.nsi` is a **fork** of upstream
`crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi` at tag
`tauri-cli-v2.11.4` (sha `8909f221d1515955fc843808032bdc5d62209c96`), pinned in
its own header with the re-sync command. It is purely additive: four hunks, zero
deleted upstream lines, each marked `code-basics:`.

Three things learned while writing it, all verified rather than assumed:

- **`FileWrite` in a Unicode installer writes ANSI (the active codepage), not
  UTF-16** (`FileWriteUTF16LE` is the separate instruction for that). The seed
  payload is pure ASCII, so ANSI output is byte-identical UTF-8 and `serde_json`
  can read it. Writing it with `FileWriteUTF16LE` would produce a file the app
  cannot parse.
- **The seed is written from `Section Install`, not from the page's leave
  function.** `$INSTDIR` is not guaranteed to exist until `SetOutPath` creates
  it, and a **silent** install runs no page callbacks at all — yet must still be
  seeded with the defaults. The leave function only records the checkbox states.
- **The state variables are read as "only an explicit `0` means off".** NSIS
  vars start as `""`, so a never-visited page (passive/silent) cannot read as
  "off" and no `.onInit` initialisation is needed.

The macro is `${NSD_CreateCheckBox}` (capital B) — that is the spelling
`__NSD_DefineControl CheckBox` generates in `nsDialogs.nsh`.

The uninstaller **does** `Delete "$INSTDIR\features.json"`: `ensure_seeded`
writes the seed through to the user's own store on first launch, so it carries
nothing unique afterwards, and the template's closing `RMDir "$INSTDIR"` is not
recursive — leaving the file behind would leave the install directory standing.

## Why the contract needed its own tests (2026-08-30)

`features::store::load` is deliberately tolerant, so **every** way the installer
side can be wrong is silent: a typo'd key, a renamed feature id, a seed written
to the wrong filename, a `.deb` installing to the wrong directory. All four
degrade to "the defaults" with no error anywhere, and the user's installer
choice just vanishes. The tolerance is right; it is precisely why the contract
has to be pinned outside `load`.

The tests **interpret** the NSIS rather than asserting an expected JSON string —
`nsis_seed_json` walks `Function WriteFeaturesSeed`, tracking `${If}/${Else}/
${EndIf}` over the page variables and concatenating the `FileWrite $9 '...'`
literals. A hard-coded expected string would pass while the script emitted
something else; interpreting means a typo in any literal changes the output and
the `serde_json` parse is what catches it.

`nsis_var` derives `$FeatureSqlConsole` from the id `sqlConsole` mechanically
(no lookup table), so adding a third `FeatureId` without a checkbox fails on the
missing `Var` declaration instead of shipping a switch nobody can reach.

**Mutation-checked** (each mutation applied to the real file, run, reverted):
- id typo `sqlConsole` → `sqlConsle` ⇒ 3 tests fail, naming the missing id.
- `FileOpen "$INSTDIR\features.json"` → `feature-seed.json` ⇒ path test fails.
- deb destination `/usr/share/...` → `/usr/lib/...` ⇒ deb mapping test fails.
- Linux seed with `sqlConsole: false` ⇒ the shipped-seed test fails.

One unexplained observation, recorded rather than guessed at: during the first
mutation run the pre-existing `save_leaves_no_temp_file_behind` and
`save_creates_the_parent_directory` also failed once. They did not reproduce —
six subsequent isolated runs and three full-suite runs are green — and they use
their own scratch directories, unrelated to the new tests. Treat as a possible
temp-dir flake on Windows, not as a known bug.
