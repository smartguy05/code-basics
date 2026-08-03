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

## Troubleshooting

- **`failed to run 'cargo metadata' ... program not found`** — Rust is not installed or not on PATH. Install rustup, then open a fresh terminal.
- **Link errors mentioning `link.exe`** — the MSVC C++ Build Tools are missing; install them as above.
- **Port 1420 already in use** — another dev instance is running; Vite is pinned to that port by `vite.config.ts` for Tauri's benefit.
- **`` `icons/icon.ico` not found `` during `pnpm tauri build`** — Windows builds embed an icon resource and need the full icon set, not just a PNG. Regenerate it with `pnpm tauri icon src-tauri/icons/icon.png` and make sure `bundle.icon` in `src-tauri/tauri.conf.json` lists `icons/icon.ico`.

Next: [Using the app](using-the-app.md).
