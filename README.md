# code-basics

A lightweight desktop IDE companion: run projects, run tests, and work with git across .NET and JavaScript/TypeScript workspaces — without a full IDE.

Built as a native desktop application (Tauri 2): a compiled Rust core does all the real work, and the interface renders in a native window. There is no web server and nothing is hosted; `pnpm tauri build` produces a regular Windows executable and installers.

## What it does

- **Tests** — discovers test projects (VSTest, Microsoft.Testing.Platform, Vitest, Jest — plus anything added via a [declarative adapter](docs/guides/adding-an-ecosystem.md)), runs them with live output, parses the report into a project → suite → test tree, and can re-run only the failures.
- **Run** — discovers runnable projects and `package.json` scripts, launches them with streamed output, and kills the whole process tree on cancel.
- **Changes** — git working-copy review with staging, unstaging, and revert down to the individual line, plus commit, branch, and stash operations.
- **History** — commit log with per-commit diffs.
- **Rider import** — converts JetBrains Rider `.run/*.xml` configurations, with a review step that surfaces anything that could not be translated.

## Quick start

Prerequisites: [Rust](https://rustup.rs) (stable, MSVC toolchain on Windows), the Visual Studio C++ Build Tools, [Node.js](https://nodejs.org) and [pnpm](https://pnpm.io). Details in [Installation](docs/getting-started/installation.md).

```sh
pnpm install
pnpm tauri dev          # run the app (first build takes several minutes)
pnpm tauri build        # produce a distributable executable + installers
```

Open a workspace from the welcome screen, or pass it on the command line: `code-basics <path>`.

## Documentation

Full documentation lives in [`docs/`](docs/README.md):

- [Getting started](docs/getting-started/installation.md) — install, run, build
- [Using the app](docs/getting-started/using-the-app.md) — the four views
- [Architecture overview](docs/architecture/overview.md) — the three layers and the dependency rule
- [Adding an ecosystem](docs/guides/adding-an-ecosystem.md) — support a new language/test runner with a TOML file, no code
- [Development guide](docs/guides/development.md) — commands, tests, conventions
- [Reference](docs/reference/commands.md) — command surface, configuration format, report formats
- [Code index](docs/INDEX.md) — generated map of files, commands, and public APIs (`pnpm docs:index` to regenerate)

## Repository layout

```
crates/core/     cb-core: all logic (detection, adapters, test parsing, git, processes) — no Tauri dependency
src-tauri/       the Tauri shell: state, adapter dispatch, IPC command surface
src/             React frontend: four views, CodeMirror diff/editor components, typed IPC wrappers
docs/            documentation (see docs/README.md)
scripts/         docs/index maintenance scripts
examples/        worked declarative adapter manifests (pytest, cargo-nextest)
```

## License

MIT
