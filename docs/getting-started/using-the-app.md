# Using the app

## Opening a workspace

A workspace is any directory. On launch the app shows a welcome screen with an **Open** dialog and a list of recently opened workspaces (kept in `localStorage`, most recent first, capped at eight). You can also launch with a path: `code-basics <dir>`.

When the workspace is a git repository, the titlebar shows a branch widget (⎇ branch, with ahead/behind counts): a Rider-style menu with fetch/pull/push, a new-branch field, and click-to-switch (× deletes a branch). It is available from every tab; the History tab keeps the full console output for network operations. Next to it sits the run-configuration dropdown (see [Run](#run)), and the five view tabs are on their own row below the titlebar.

Opening a workspace scans it for projects — filesystem-only, so it is effectively instant — and layers any saved configurations from `.code-basics/config.json` on top of the detected ones. The backend keeps the open workspace across a window reload. See [Configuration](../reference/configuration.md) for what gets written where.

Directories never scanned: `.git`, `node_modules`, `bin`, `obj`, `target`, `dist`, `.next`, `.nuxt`, `.vs`, `.idea`, `.vscode`, `TestResults`, `.code-basics` (case-insensitive), to a maximum depth of 10.

## The five views

The Run tab is first and opens by default.

### Tests

Lists every test configuration, grouped by project. Running one streams the runner's own console output live — provisional pass/fail/skip counts tick up as each test's result line appears, and from the second run onward the test tree shows every known test greyed out, colouring each green/red as it completes — then parses the structured report the runner leaves behind into the authoritative project → suite → test tree with outcome roll-ups (one failure colours every ancestor). Failed tests expose their message, stack trace, and captured stdout. **Re-run failed** restricts the next run to the failures from the previous one, expressed in each runner's own filter syntax.

Detected runners: VSTest and Microsoft.Testing.Platform for .NET, Vitest and Jest for Node — plus anything contributed by a [declarative adapter](../guides/adding-an-ecosystem.md). How the .NET split is detected — and why it matters — is covered in [the core crate](../architecture/core-crate.md#adapters).

### Run

Application launches: .NET executable projects (including `launchSettings.json` profiles) and `package.json` scripts. Configurations are picked from the dropdown in the titlebar, next to the branch widget — its status dot is grey when idle, yellow while building/starting, green once the app is up, red on failure. Output streams to a console as it is produced, including bare-`\r` progress redraws. Each run (and each build action) gets its own console tab labeled with the configuration name, so running several projects at once keeps their output separate; closing a tab does not stop its process.

The sidebar is a directory tree of the workspace, filtered like the project scan (no `node_modules`, `bin`, `obj`, …) but with no depth limit — each directory is listed the first time it is expanded. Clicking a file opens it in an editor pane above the console (syntax highlighting per extension, tabs per open file). **Ctrl+S** saves; unsaved files show a ● on their tab, and closing such a tab discards the changes. The divider between editor and console drags to resize, and the split persists.

Inside the console: **Ctrl+F** opens the find/filter bar — Enter/Shift+Enter cycle matches, the severity dropdown hides lines below a level (Info+ / Warn+ / Errors, with indented continuation lines like stack traces following their parent), and the **Filter** toggle hides non-matching lines instead of jumping between them; closing the bar restores the full output. Selecting text copies it, and right-click opens a menu with Copy selection / Copy all output / **Copy diagnostics** — a paste-ready block with the command line, exit code, and the last 100 output lines. URLs are clickable (default browser) and severity markers are colour-coded. Cancelling kills the whole process *tree*, not just the wrapper — so `dotnet run`'s built assembly or a bundler behind `npm run dev` actually dies and releases its port.

Configurations can be created, edited (arguments, environment, working directory, framework, build configuration), and saved. Saved configurations go to `.code-basics/config.json`, which is meant to be checked in — like Rider's `.run/` directory. You can also [import Rider run configurations](../guides/rider-import.md).

The dropdown's list can be arranged to taste: the ☆ on each row stars it as a favourite (favourites sort first), and ↑/↓ move a row within its group. Both are saved to `config.json`. **+ New configuration…** and **Import from Rider…** live at the bottom of the same menu.

For .NET configurations the toolbar has 🔨 build / ⟳ rebuild / 🧹 clean buttons, and an **Env** dropdown that sets `ASPNETCORE_ENVIRONMENT` for the run (default `Development`). Options are managed inside the dropdown itself — a free-text row adds one, the × beside an option removes it, and "(config default)" passes the configuration through untouched. The list is per-workspace and personal (localStorage), not written to `config.json`.

For .NET configurations, **Secrets…** edits the project's [user secrets](../reference/configuration.md#net-user-secrets) — the same store `dotnet user-secrets` and Rider manage, kept under your user profile rather than in the repository. Saving for a project without a `<UserSecretsId>` adds one to the `.csproj` first.

### Changes

Working-copy review:

- Three comparison modes: working ↔ HEAD (everything), working ↔ index (unstaged only), index ↔ HEAD (staged only).
- Two layouts: **side by side** (default — baseline read-only on the left, editable working copy on the right) or inline/unified; the choice persists.
- Stage/unstage whole files, hunks, or individually selected lines. **Right-click a file** to stage or unstage it without opening it.
- The file list is grouped into **Staged**, your own named **change groups**, and **Unstaged**. Groups are for organising work in progress — right-click a file to move it into one, or use "+ New group". They are local to you and never committed; see [change groups](../reference/configuration.md#change-groups-changelistsjson). A partially staged file shows under both Staged and its unstaged group, as `git status` reports it.
- Revert individual lines — the app builds a reverse patch of just your selection and lets `git apply` do the surgery ([how that works](../architecture/core-crate.md#git)).
- **Reject** an agent's change instead of silently reverting it: the change goes back *and* the reason you type is left as a comment where the code was, for the agent to read and act on. A `pre-commit` hook then refuses to commit while that comment is still there. See [rejecting a change](../guides/agent-intent-capture.md#rejecting-a-change).
- Commit (with amend), branches (create/checkout/delete), stash save/pop.
- Push/pull/fetch shell out to your system `git`, so existing credentials (SSH agent, credential manager) work with no prompts inside the app.

### History

The commit log (subject, author, time) with the full per-file diff of any commit.

### Objects

Reads the real managed heap of a .NET process — either from the crash dump the runtime wrote as it died, or from a process that is still running — and shows the objects it actually held. No debugger is involved and the application does not have to be started differently.

Two limits define the whole feature: **no method is ever called** and **no property is ever evaluated**, because this reads memory rather than a running execution context (auto-properties are the exception — they have a backing field). In exchange, inspecting cannot throw, deadlock, or alter what it inspects. Anything unreadable is shown as an explicit gap with a reason, and anything cut short by a limit says so — never a shorter list that looks complete.

Crash-dump capture is opt-in per workspace and off by default, because a dump is a verbatim copy of process memory. Attaching to a running process offers **every** attachable .NET process on the machine, each labelled *launched*, *descendant* or *unrelated* — the `dotnet run` child is usually the one you want, since the pid code-basics started is the CLI launcher. Full detail, including what attaching costs the target: [Inspecting objects](../guides/inspecting-objects.md).

## Where app state lives

Everything workspace-local is under `.code-basics/` in the workspace root: `config.json` (saved run configurations), `adapters/*.toml` (declarative adapters), and `results/` (test report files — gitignore this). Details in [Configuration](../reference/configuration.md).
