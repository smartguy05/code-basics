# Using the app

## Opening a workspace

A workspace is any directory. On launch the app shows a welcome screen with an **Open** dialog and a list of recently opened workspaces (kept in `localStorage`, most recent first, capped at eight). You can also launch with a path: `code-basics <dir>`.

Opening a workspace scans it for projects — filesystem-only, so it is effectively instant — and layers any saved configurations from `.code-basics/config.json` on top of the detected ones. The backend keeps the open workspace across a window reload. See [Configuration](../reference/configuration.md) for what gets written where.

Directories never scanned: `.git`, `node_modules`, `bin`, `obj`, `target`, `dist`, `.next`, `.nuxt`, `.vs`, `.idea`, `.vscode`, `TestResults`, `.code-basics` (case-insensitive), to a maximum depth of 10.

## The four views

### Tests

Lists every test configuration, grouped by project. Running one streams the runner's own console output live, then parses the structured report it leaves behind into a project → suite → test tree with outcome roll-ups (one failure colours every ancestor). Failed tests expose their message, stack trace, and captured stdout. **Re-run failed** restricts the next run to the failures from the previous one, expressed in each runner's own filter syntax.

Detected runners: VSTest and Microsoft.Testing.Platform for .NET, Vitest and Jest for Node — plus anything contributed by a [declarative adapter](../guides/adding-an-ecosystem.md). How the .NET split is detected — and why it matters — is covered in [the core crate](../architecture/core-crate.md#adapters).

### Run

Application launches: .NET executable projects (including `launchSettings.json` profiles) and `package.json` scripts. Output streams to a console as it is produced, including bare-`\r` progress redraws. Cancelling kills the whole process *tree*, not just the wrapper — so `dotnet run`'s built assembly or a bundler behind `npm run dev` actually dies and releases its port.

Configurations can be created, edited (arguments, environment, working directory, framework, build configuration), and saved. Saved configurations go to `.code-basics/config.json`, which is meant to be checked in — like Rider's `.run/` directory. You can also [import Rider run configurations](../guides/rider-import.md).

### Changes

Working-copy review:

- Three comparison modes: working ↔ HEAD (everything), working ↔ index (unstaged only), index ↔ HEAD (staged only).
- Stage/unstage whole files, hunks, or individually selected lines.
- Revert individual lines — the app builds a reverse patch of just your selection and lets `git apply` do the surgery ([how that works](../architecture/core-crate.md#git)).
- Commit (with amend), branches (create/checkout/delete), stash save/pop.
- Push/pull/fetch shell out to your system `git`, so existing credentials (SSH agent, credential manager) work with no prompts inside the app.

### History

The commit log (subject, author, time) with the full per-file diff of any commit.

## Where app state lives

Everything workspace-local is under `.code-basics/` in the workspace root: `config.json` (saved run configurations), `adapters/*.toml` (declarative adapters), and `results/` (test report files — gitignore this). Details in [Configuration](../reference/configuration.md).
