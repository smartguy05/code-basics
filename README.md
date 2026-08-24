# code-basics

A lightweight desktop IDE companion: run projects, run tests, and work with git across .NET and JavaScript/TypeScript workspaces — without a full IDE.

Built as a native desktop application (Tauri 2): a compiled Rust core does all the real work, and the interface renders in a native window. There is no web server and nothing is hosted; `pnpm tauri build` produces a regular Windows executable and installers.

## What it does

- **Run** — discovers runnable projects, `launchSettings.json` profiles, and `package.json` scripts; launches them with streamed output in per-run console tabs, and kills the whole process tree on cancel. Configurations live in a titlebar dropdown (status dots, favourites, custom ordering); an `ASPNETCORE_ENVIRONMENT` picker, build/rebuild/clean actions, and a .NET user-secrets editor round out the tab.
- **Edit** — a directory tree of the workspace sits beside the Run tab's console; files open in a syntax-highlighted CodeMirror editor above it (Ctrl+S saves, Ctrl+F finds within the file, the split is resizable, and the console collapses to a strip when you want the screen for code), covering the quick fix-and-rerun loop without switching tools.
- **Trace a method** — an inline row above every declaration says how many places use it, and drops down the list; clicking one opens that file at that line. Middle-click a symbol to go to its definition, with a grouped picker when there is more than one place to go. The answers come from real [language servers](docs/guides/language-servers.md) already on your machine (Roslyn for C#, `rust-analyzer`, `typescript-language-server`, a Python server) — nothing is bundled, and a count is never guessed: while a server is still loading, or when none is installed, the row says so instead of showing a number.
- **Tests** — discovers test projects (VSTest, Microsoft.Testing.Platform, Vitest, Jest — plus anything added via a [declarative adapter](docs/guides/adding-an-ecosystem.md)), runs them with live output and live per-test progress, parses the report into a project → suite → test tree, and can re-run only the failures.
- **Changes** — git working-copy review with side-by-side or inline diffs, staging, unstaging, and revert down to the individual line, plus commit operations. A **Files / Intent / Stashes / Erosion** toggle switches the list between the plain file grouping, the [decisions behind the diff](docs/guides/agent-intent-capture.md) (captured from a coding agent as it works — where each card and file also shows whether it is staged), a full **stash manager** (named stashes with a per-stash diff preview and apply / pop / drop / clear), and an **erosion scan** — a rules-based, no-model pass that flags the changes quietly weakening the codebase (deleted assertions, skipped tests, widened catches, introduced panics, stubs left in production paths, removed safeguards or logs), each flag clicking through to its diff line and extensible with your own regex rules in `.code-basics/erosion/`. A change you disagree with can be **rejected**: reverted with your reason left in the code for the agent to fix, and a `pre-commit` hook that refuses to commit until it has been. The Intent grouping also offers **before/after evidence**: **Run before/after** builds the change against both `HEAD` (in an isolated git worktree) and the working tree, runs the same tests and captures the same console output — and, when a `.http` scenario and a server launch are present, replays the requests against both — then diffs the observable outcomes and attaches each difference to the card that plausibly caused it. It is evidence a change did what was claimed without reading it; every threshold abstains rather than guess.
- **History** — commit log with per-commit diffs, a branch folder tree (slash-named branches group into folders, with multi-select bulk delete), and push/pull/fetch.
- **Branches everywhere** — a Rider-style titlebar widget with a branch tree (slash-named branches group into folders), switching (including remote checkout), creation from any base, and fetch/pull/push.
- **Rider import** — converts JetBrains Rider `.run/*.xml` configurations, with a review step that surfaces anything that could not be translated.
- **[Enhancements](docs/guides/instruction-enhancements.md)** — a menu bar with a File menu and an Enhancements menu driven by plain `.md` files you drop in a folder: **Add Instructions** adds a reusable, marker-bounded section to the workspace's `CLAUDE.md`/`AGENTS.md` (idempotent, backed up, confirmed first), and **Run Agent** runs a saved prompt as an agent (read-only or edit), with `once: true` prompts recorded per repo.
- **Terminals** — floating, interactive terminal windows you can open anytime (drag, resize, minimize to a pill that flashes when a hidden session produces output). Each is a real pseudo-terminal running your shell, so you can launch Claude Code — or anything — and interact with it live; several can run at once and they survive tab switches. Closing one ends its whole process tree.
- **Console built for troubleshooting** — clickable URLs, severity colouring, Ctrl+F search, filtering by severity/text, copy-on-select, and a one-click paste-ready diagnostics block.

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
- [Using the app](docs/getting-started/using-the-app.md) — the six views
- [Language servers](docs/guides/language-servers.md) — what to install per language for find-usages and go-to-definition
- [Architecture overview](docs/architecture/overview.md) — the three layers and the dependency rule
- [Adding an ecosystem](docs/guides/adding-an-ecosystem.md) — support a new language/test runner with a TOML file, no code
- [Development guide](docs/guides/development.md) — commands, tests, conventions
- [Reference](docs/reference/commands.md) — command surface, configuration format, report formats
- [Code index](docs/INDEX.md) — generated map of files, commands, and public APIs (`pnpm docs:index` to regenerate)

## Repository layout

```
crates/core/     cb-core: all logic (detection, adapters, test parsing, git, processes) — no Tauri dependency
src-tauri/       the Tauri shell: state, adapter dispatch, IPC command surface
src/             React frontend: six tab views, CodeMirror diff/editor components, typed IPC wrappers
docs/            documentation (see docs/README.md)
scripts/         docs/index maintenance scripts
examples/        worked declarative adapter manifests (pytest, cargo-nextest)
```

## License

MIT
