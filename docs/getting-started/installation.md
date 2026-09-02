# Installation

code-basics is a Tauri 2 desktop application: a Rust workspace compiled into a native executable, with a React frontend bundled inside it.

## Prerequisites

| Tool | Why | Install |
|------|-----|---------|
| Rust (stable, ≥ 1.82) | Compiles `cb-core` and the Tauri shell | `winget install Rustlang.Rustup`, then `rustup default stable-msvc` |
| MSVC C++ Build Tools | The linker Rust needs on Windows | `winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"` |
| Node.js + pnpm | Frontend build tooling | [nodejs.org](https://nodejs.org), then `corepack enable` or `npm i -g pnpm` |
| WebView2 | Renders the UI in the native window | Ships with Windows 11; nothing to do |

pnpm is the canonical package manager — `pnpm-lock.yaml` is the tracked lockfile and `src-tauri/tauri.conf.json` invokes `pnpm dev` / `pnpm build`. Do not commit a `package-lock.json`.

On Linux, Tauri additionally needs the WebKitGTK development packages; see the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for the distribution-specific list.

## First run

```sh
pnpm install
pnpm tauri dev
```

`pnpm tauri dev` starts the Vite dev server on port 1420 (configured as `beforeDevCommand`) and then compiles and launches the Rust application pointing at it. The first run compiles the entire Rust workspace and takes several minutes; later runs are incremental. Frontend edits hot-reload; Rust edits trigger a rebuild and relaunch.

To open a workspace immediately, the binary accepts a directory argument (`code-basics <path>`). A bad path is not fatal — the app logs it and shows the welcome screen instead.

`pnpm dev` alone serves only the frontend in a browser. Every `invoke` call to the backend fails there, so it is useful solely for pure layout work.

## Building a distributable

```sh
pnpm tauri build
```

This typechecks and builds the frontend into `dist/`, compiles the Rust workspace in release mode (the workspace profile enables LTO, `opt-level = "s"`, and symbol stripping), and produces the executable plus installers (`bundle.targets` is `"all"`, so MSI and NSIS on Windows). Output lands under `src-tauri/target/release/`.

`beforeBuildCommand` chains **`pnpm debuggers:fetch`**, which vendors the two debug adapters — NetCoreDbg and the standalone js-debug DAP server, both MIT, about 11 MB together — into `src-tauri/resources/debuggers/` so the installer carries them. Versions and SHA-256 hashes are pinned in `scripts/fetch-debuggers.mjs`; a checksum mismatch fails the build deliberately, while an offline machine only warns and produces an app whose Debug button reports no bundled adapter. Re-runs are free: a stamp file skips a download that is already current.

It does **not** chain `pnpm sidecar:build`. Run that first if you want the Objects tab to work in the bundle; see below.

## Optional features and how each installer asks about them

Two capabilities are optional:

| Feature | Id | What it is |
|---------|----|------------|
| SQL console | `sqlConsole` | Connect to a database and run queries. |
| Ask the codebase | `askCodebase` | Ctrl+/ asks a coding agent about this codebase in a live terminal. |

**One binary ships both of them, always.** No installer can produce a
feature-reduced build, so an installer checkbox does not decide what is compiled
in — it only writes a preference file that the app reads at startup. Both
features default to **on**, because most launches (a `cargo run`, a dev checkout,
an AppImage) never see an installer at all; the installer's job is to let someone
turn a feature *off* up front, not to be the only thing that can turn it on.

The preference lives in `<config>/code-basics/features.json` —
`%APPDATA%\code-basics\features.json` on Windows, `$XDG_CONFIG_HOME/code-basics/`
(then `~/.config/code-basics/`) on Linux. `CB_FEATURES_PATH` overrides the whole
path. **The choice is changeable at any time in the app**: File → *Optional
features…*.

An installer's answer is a **seed**, not an override. On first launch the app
looks for the installer's seed file and adopts it *only when the user has no
store yet*, then writes it through — so exactly one launch ever reads it, and a
repair install, a reinstall or an upgrade can never switch a feature back on that
someone turned off.

### Windows (NSIS)

The NSIS installer asks. A custom wizard page appears **after** the directory
page, with a checkbox per feature, and writes the seed next to the installed
executable as `features.json`.

This needs a **vendored copy of Tauri's NSIS template**, pointed at by
`bundle.windows.nsis.template` in `src-tauri/tauri.conf.json`. Tauri's
`installerHooks` mechanism cannot host the page: the hook file is `!include`d at
line 35 of `installer.nsi`, before every page declaration, and NSIS page order is
declaration order — a page declared there lands before the Welcome page and
cannot be moved.

> **Maintenance cost, for whoever upgrades Tauri:** the vendored template is a
> fork of `installer.nsi` from the Tauri version this project builds against. It
> does not update itself. **On any Tauri upgrade, diff the new upstream
> `installer.nsi` against the vendored copy and re-apply the custom page**, or
> the installer silently keeps building from a stale template.

The MSI target (WiX) has no such page and ships no seed, so an MSI install starts
with both features on and the in-app picker as its surface.

### Ubuntu / Debian (`.deb`)

**The `.deb` cannot ask.** Tauri's `bundle.linux.deb` exposes
`preInstallScript` / `postInstallScript` / `files` / `desktopTemplate`, and none
of those reach the Debian *control* archive — so debconf templates are
unavailable and there is no supported way to put a question in front of the
user during `dpkg -i`.

So the `.deb` ships an all-defaults seed instead: `src-tauri/resources/linux/features.json`,
installed via `bundle.linux.deb.files` to `/usr/share/code-basics/features.json`,
which is exactly the path `features::seed_path_for(_, Platform::Linux)` reads.
Both features arrive enabled.

**On Ubuntu the in-app picker is the real surface.** If you want a feature off,
open the app and use File → *Optional features…*. There is no installer page to
hunt for.

### AppImage

`bundle.linux.appimage` supports only `files` — no install scripts, and an
AppImage is not installed in any case. Same story as the `.deb`: everything ships
enabled, and the in-app picker is where the choice is made.

### Two honest caveats about the Linux packages

- **A Linux build has never been attempted for this project.** The `bundle.linux`
  configuration above is written against Tauri's schema, not against a `.deb`
  anyone has produced from this repository. Expect to shake out the WebKitGTK
  prerequisites and the bundle itself on the first real attempt.
- **The Objects tab will be inert on Linux.** `scripts/build-sidecar.mjs`
  publishes the `cb-inspector` sidecar for `win-x64` and `win-x86` only, so no
  inspector binary ships in a Linux bundle and `inspect_status` reports the
  feature unavailable — by design, but worth knowing before filing it as a bug.

## Troubleshooting

- **`failed to run 'cargo metadata' ... program not found`** — Rust is not installed or not on PATH. Install rustup, then open a fresh terminal.
- **Link errors mentioning `link.exe`** — the MSVC C++ Build Tools are missing; install them as above.
- **Port 1420 already in use** — another dev instance is running; Vite is pinned to that port by `vite.config.ts` for Tauri's benefit.
- **`` `icons/icon.ico` not found `` during `pnpm tauri build`** — Windows builds embed an icon resource and need the full icon set, not just a PNG. Regenerate it with `pnpm tauri icon src-tauri/icons/icon.png` and make sure `bundle.icon` in `src-tauri/tauri.conf.json` lists `icons/icon.ico`.

Next: [Using the app](using-the-app.md).
