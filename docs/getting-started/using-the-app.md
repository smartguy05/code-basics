# Using the app

## Opening a workspace

A workspace is any directory. On launch the app shows a welcome screen with an **Open** dialog and a list of recently opened workspaces (kept in `localStorage`, most recent first, capped at eight). You can also launch with a path: `code-basics <dir>`.

When the workspace is a git repository, the titlebar shows a branch widget (⎇ branch, with ahead/behind counts): a Rider-style menu with fetch/pull/push, a new-branch field, and click-to-switch (× deletes a branch). It is available from every tab; the History tab keeps the full console output for network operations. Next to it sits the run-configuration dropdown (see [Run](#run)), and the six view tabs are on their own row below the titlebar.

Opening a workspace scans it for projects — filesystem-only, so it is effectively instant — and layers any saved configurations from `.code-basics/config.json` on top of the detected ones. The backend keeps the open workspace across a window reload. See [Configuration](../reference/configuration.md) for what gets written where.

Directories never scanned: `.git`, `node_modules`, `bin`, `obj`, `target`, `dist`, `.next`, `.nuxt`, `.vs`, `.idea`, `.vscode`, `TestResults`, `.code-basics` (case-insensitive), to a maximum depth of 10.

## Search everywhere

Once a workspace is open, one keystroke finds a file, a symbol or a run configuration from anywhere in the app:

| Keys | Opens the palette on |
|------|----------------------|
| **Shift Shift** (twice within 300 ms) | everything |
| **Ctrl+N** | symbols |
| **Ctrl+Shift+N** | files |
| **Ctrl+Shift+A** | run configurations |

Inside it: type to search, **↑/↓** move, **Tab** / **Shift+Tab** cycle the scope (the four buttons along the top do the same), **Enter** takes the selected row and **Esc** closes. Matching is fuzzy — `tlog` finds `treeLogic.ts`, and capitals match the humps of a camelCase name. A trailing **`:123`** means "line 123": `Program.cs:40` opens that file at that line, and a line past the end of the file lands at the end rather than failing.

Choosing a file or a symbol opens it in the Run tab's editor and scrolls to the line. Choosing a run configuration **selects** it in the titlebar dropdown and does not start it — a fuzzy match is a guess about what you meant, and the cost of guessing wrong is a build or a service talking to something real, so pressing Run stays your decision.

Ctrl+F is deliberately not a palette binding: it belongs to the console's find bar.

The index behind it is built in the background when the workspace is opened, so it is never in the way — on a large solution it takes about a second, and the palette says "Indexing the workspace" rather than showing an empty list while it does. Saving a file re-indexes just that file. The footer shows how many files and symbols are indexed, with a **Rebuild index** button for the rare case where you can see it is wrong (a file rewritten twice inside the same second to exactly the same length can slip past the cache's fingerprint). The cache lives in `.code-basics/symbols.json` and is safe to delete.

Only what the project scan can see is indexed — the same skipped directories listed above — and files over 1 MiB are listed by name but not read for symbols, because a generated file yields tens of thousands of entries nobody has ever wanted to jump to. Anything the one-line scan cannot confidently classify gets no kind badge rather than a guessed one.

## The six views

The Run tab is first and opens by default.

### Tests

Lists every test configuration, grouped by project. Running one streams the runner's own console output live — provisional pass/fail/skip counts tick up as each test's result line appears, and from the second run onward the test tree shows every known test greyed out, colouring each green/red as it completes — then parses the structured report the runner leaves behind into the authoritative project → suite → test tree with outcome roll-ups (one failure colours every ancestor). Failed tests expose their message, stack trace, and captured stdout. **Re-run failed** restricts the next run to the failures from the previous one, expressed in each runner's own filter syntax.

Detected runners: VSTest and Microsoft.Testing.Platform for .NET, Vitest and Jest for Node — plus anything contributed by a [declarative adapter](../guides/adding-an-ecosystem.md). How the .NET split is detected — and why it matters — is covered in [the core crate](../architecture/core-crate.md#adapters).

### Run

Application launches: .NET executable projects (including `launchSettings.json` profiles) and `package.json` scripts. Rust crates are detected too — a `Cargo.toml` becomes a project, classified as an executable or a library — but detection is all it is: no `cargo run` or `cargo test` configuration is offered, deliberately, since that would add entries to this tab and the Tests tab for every Rust repository. To run Rust, drop in the `cargo-nextest` [declarative adapter](../guides/adding-an-ecosystem.md); it supplies configurations for the same directory rather than being shadowed by the built-in detection. Configurations are picked from the dropdown in the titlebar, next to the branch widget — its status dot is grey when idle, yellow while building/starting, green once the app is up, red on failure. Output streams to a console as it is produced, including bare-`\r` progress redraws. Each run (and each build action) gets its own console tab labeled with the configuration name, so running several projects at once keeps their output separate; closing a tab does not stop its process.

The sidebar is a directory tree of the workspace, filtered like the project scan (no `node_modules`, `bin`, `obj`, …) but with no depth limit — each directory is listed the first time it is expanded. Clicking a file opens it in an editor pane above the console (syntax highlighting per extension, tabs per open file). **Ctrl+S** saves; unsaved files show a ● on their tab, and closing such a tab discards the changes. The divider between editor and console drags to resize, and the split persists.

The console **collapses out of the way** while you are reading code: the ▾ beside its tabs folds it down to that strip, and the ▸ brings it back. Collapsed it is still a tab strip, not a hidden panel, so there is always something to click. Nothing stops: a running process keeps running, its output keeps accumulating, and the scrollback is all there when you expand it again. Both the collapsed state and the divider position are remembered **per workspace** — how much room the terminal deserves is a property of what you are doing in a given repository, so a service you run and watch and a library you only read do not fight over one setting.

#### Finding where a method is used

Above every declaration in an open file the editor draws a quiet row: **"7 usages"**, **"1 usage"**, or **"No usages"**. Click it for the list, grouped by file, and click a row there to open that file at that line. The count is semantic, not textual — `Order.Total` and `Invoice.Total` are different symbols and are counted separately — because the answer comes from a real language server for that language.

**Middle-click any symbol to go to its definition.** One place to go, and it goes there. More than one — an interface method and the classes implementing it, say — and a picker opens grouped **Declarations** / **Implementations** / **Type definitions**; it never picks for you. A location outside the workspace (a framework type, a generated document) is still listed, greyed out, because it is a real answer, and hovering it says why clicking does nothing.

**The row never guesses.** Instead of a number you may see:

| The row says | What it means |
|---|---|
| *Usages* (faint) | Nothing has been asked yet — it asks about what is on screen. |
| *Finding usages…* | Asked; waiting. A references search covers the whole workspace. |
| *Language server starting… / loading…* | No answer exists yet. C# solutions take tens of seconds to load. |
| *No language server* | Nothing is installed or configured for this language — hover for what to install. |
| *Language server failed* | It died; hover for the reason. |
| *This server cannot answer* | This server does not offer find-usages at all. That is **not** "there are none". |
| *No usages* | A real answer: there genuinely are none. |

A count is only ever shown for a settled answer about the text on screen. Edit the file and the counts clear rather than staying behind as a number that is no longer true; they come back a moment after you stop typing.

**"No language server"?** Nothing is broken — code-basics never bundles a language server, it uses the one on your machine. Hover the row (or the titlebar indicator, which appears whenever a server has something to say) for the exact command to install the one you need. The full list, per language, is in [Language servers](../guides/language-servers.md), and the `lsp` block of `.code-basics/config.json` — for pointing the app at a server in an unusual place, or turning one off — is documented under [Configuration](../reference/configuration.md#lsp).

Inside the console: **Ctrl+F** opens the find/filter bar — Enter/Shift+Enter cycle matches, the severity dropdown hides lines below a level (Info+ / Warn+ / Errors, with indented continuation lines like stack traces following their parent), and the **Filter** toggle hides non-matching lines instead of jumping between them; closing the bar restores the full output. Selecting text copies it, and right-click opens a menu with Copy selection / Copy all output / **Copy diagnostics** — a paste-ready block with the command line, exit code, and the last 100 output lines. URLs are clickable (default browser) and severity markers are colour-coded. Cancelling kills the whole process *tree*, not just the wrapper — so `dotnet run`'s built assembly or a bundler behind `npm run dev` actually dies and releases its port.

A project whose manifest will not parse is **shown, not hidden**. It appears under a **Could not be read** heading at the top of the sidebar, greyed out, with the parser's reason on the row and the full manifest path in the tooltip — usually a line and column you can go straight to. Such a project offers nothing to run, and a saved configuration that targets it has Run, Restart and the build buttons disabled with the same reason as their tooltip, because starting it would only reproduce the failure in a console several seconds later. Earlier versions dropped these projects from the scan entirely, which meant a stray comma in a `package.json` made a project vanish from this list with no error anywhere.

Configurations can be created, edited (arguments, environment, working directory, framework, build configuration), and saved. Saved configurations go to `.code-basics/config.json`, which is meant to be checked in — like Rider's `.run/` directory. You can also [import Rider run configurations](../guides/rider-import.md).

The dropdown's list can be arranged to taste: the ☆ on each row stars it as a favourite (favourites sort first), and ↑/↓ move a row within its group. Both are saved to `config.json`. **+ New configuration…** and **Import from Rider…** live at the bottom of the same menu.

For .NET configurations the toolbar has 🔨 build / ⟳ rebuild / 🧹 clean buttons, and an **Env** dropdown that sets `ASPNETCORE_ENVIRONMENT` for the run (default `Development`). Options are managed inside the dropdown itself — a free-text row adds one, the × beside an option removes it, and "(config default)" passes the configuration through untouched. The list is per-workspace and personal (localStorage), not written to `config.json`.

For .NET configurations, **Secrets…** edits the project's [user secrets](../reference/configuration.md#net-user-secrets) — the same store `dotnet user-secrets` and Rider manage, kept under your user profile rather than in the repository. Saving for a project without a `<UserSecretsId>` adds one to the `.csproj` first.

### Changes

Working-copy review:

- Three comparison modes: working ↔ HEAD (everything), working ↔ index (unstaged only), index ↔ HEAD (staged only).
- Two layouts: **side by side** (default — baseline read-only on the left, editable working copy on the right) or inline/unified; the choice persists.
- **Finding the changes.** A strip down the right edge marks every change in the file at its position, so a change far below the fold is visible without scrolling — click a mark to jump to it. **F7** and **Shift+F7** (or the ↑ / ↓ buttons) step through them and wrap at the ends, and the toolbar says how many there are.
- **Reading them.** The horizontal scrollbar along the bottom drives **both panes at once**, so a long line stays lined up while you scroll sideways; the line numbers stay pinned. Shift+wheel does the same. **A− / A+** — or **Ctrl+-** / **Ctrl+=**, with **Ctrl+0** to reset — set the text size for every editor in the app, and the size is remembered.
- **Collapse unchanged** folds long runs of untouched code down to a few lines either side of each change. **Ignore whitespace** stops reindents, reflows and line-ending changes being drawn as differences — it changes only what is *drawn*, never what Stage or Revert act on, because a whitespace-only hunk is still a real change on disk.
- Stage/unstage whole files, hunks, or individually selected lines. **Right-click a file** to stage or unstage it without opening it.
- The file list is grouped into **Staged**, your own named **change groups**, and **Unstaged**. Groups are for organising work in progress — right-click a file to move it into one, or use "+ New group". They are local to you and never committed; see [change groups](../reference/configuration.md#change-groups-changelistsjson). A partially staged file shows under both Staged and its unstaged group, as `git status` reports it.
- Revert individual lines — the app builds a reverse patch of just your selection and lets `git apply` do the surgery ([how that works](../architecture/core-crate.md#git)).
- **Reject** an agent's change instead of silently reverting it: the change goes back *and* the reason you type is left as a comment where the code was, for the agent to read and act on. A `pre-commit` hook then refuses to commit while that comment is still there. See [rejecting a change](../guides/agent-intent-capture.md#rejecting-a-change).
- Commit (with amend), branches (create/checkout/delete), stash save/pop.
- Push/pull/fetch shell out to your system `git`, so existing credentials (SSH agent, credential manager) work with no prompts inside the app.

### History

The commit log (subject, author, time). Selecting a commit lists the files it
touched and opens the first one in the same diff viewer the Changes tab uses —
read-only, but with the same colours, marker strip, F7 navigation and text size.

### Architecture

Diagrams of the workspace, drawn from what the manifests actually say.

The list opens on two built-ins. **Project map** is what is in this repository — the projects the scan found and the references between them, from `<ProjectReference>`, `package.json` dependencies, `.sln` grouping and workspace globs. **Component map** is what the system consists of when it runs: the services and the data stores they declare. They are two questions rather than two zoom levels, so the component map drops every project-reference arrow and adds stores that appear nowhere in the first. Any diagrams saved in the workspace follow underneath.

Nothing is cached. Every time you select a diagram it is derived again from the files as they are on disk right now, and **Regenerate** does the same on demand — an arrow drawn from a stale manifest would assert a dependency you may have deleted since.

**What is missing from the picture is shown beside it.** The count of things the deriver read and refused to draw sits in the canvas toolbar, and the list is under the diagram: a project reference that resolves to nothing, a workspace membership it would not infer, a relationship no arrow can express, and — on the component map — every candidate that did not clear the evidence bar. The rule throughout is that an arrow is a strong claim: a name that nearly matches is reported rather than matched, an ambiguous package drops the edge, and nothing is inferred from an `import` line or a comment. A picture that quietly left these out would look complete and would not be.

The canvas pans by dragging and zooms with the wheel or the **+ / −** buttons; **Fit** frames the whole diagram and **Reset** returns to 1:1. Where you left each diagram is remembered, so glancing at another tab does not cost you the zoom. **Clicking a box opens its file** in the Run tab's editor — for a built-in map that is exact, and for a saved diagram it opens only when the node name matches one indexed symbol unambiguously; otherwise nothing happens rather than the wrong file.

**Save a copy** stores the current picture as a diagram of your own, which **Edit** then opens in a Mermaid editor that validates as you type (Ctrl+S saves; an unsaved diagram shows a ● and asks before you navigate away). Hand-drawn and accepted diagrams live in `.code-basics/diagrams/` and are meant to be checked in; derived ones are regenerated locally and gitignored. Which directory a diagram lands in follows from how it was produced and is not a setting, so a derived file cannot end up committed and a drawing cannot be overwritten by the next regeneration. See [Diagrams](../reference/configuration.md#diagrams-diagrams).

### Objects

Reads the real managed heap of a .NET process — either from the crash dump the runtime wrote as it died, or from a process that is still running — and shows the objects it actually held. No debugger is involved and the application does not have to be started differently.

Two limits define the whole feature: **no method is ever called** and **no property is ever evaluated**, because this reads memory rather than a running execution context (auto-properties are the exception — they have a backing field). In exchange, inspecting cannot throw, deadlock, or alter what it inspects. Anything unreadable is shown as an explicit gap with a reason, and anything cut short by a limit says so — never a shorter list that looks complete.

Crash-dump capture is opt-in per workspace and off by default, because a dump is a verbatim copy of process memory. Attaching to a running process offers **every** attachable .NET process on the machine, each labelled *launched*, *descendant* or *unrelated* — the `dotnet run` child is usually the one you want, since the pid code-basics started is the CLI launcher. Full detail, including what attaching costs the target: [Inspecting objects](../guides/inspecting-objects.md).

## Where app state lives

Everything workspace-local is under `.code-basics/` in the workspace root: `config.json` (saved run configurations), `adapters/*.toml` (declarative adapters), `diagrams/` (architecture diagrams — check these in; `diagrams/derived/` is gitignored), and `results/` (test report files — gitignore this). Details in [Configuration](../reference/configuration.md).
