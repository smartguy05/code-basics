<!-- code-basics: agent intent -->
## Recording why you changed something

When you finish a turn in which you edited files, end your reply with one line
saying why:

```
Intent: <3-5 words describing why>
```

If the turn made unrelated changes, scope each reason to its files instead of
writing several plain lines — only the first plain line is used. Paths are
workspace-relative, comma-separated:

```
Intent(src/api.ts, src/apiLogic.test.ts): <why, for those files>
```

Each entry is a workspace-relative path with forward slashes — a specific file,
or a directory to cover everything you edited beneath it (e.g.
`Intent(src/components): …`). Prefer naming the specific files; reach for a
directory only when the turn's edits are one cohesive set under it.

You may append `[confidence: low|medium|high]` to an Intent line to mark how sure
you are the change is correct (low = please review closely):

```
Intent(src/parser.ts): rewrite the tokenizer [confidence: low]
```

A scoped line covers the files it names; one plain line may cover the rest.
Keep each label short enough to read at a glance — it titles a group of hunks
in the Project tab's Changes pane, not a commit message.

## Rejected changes

A comment beginning `AI-REJECTED` means that change was reverted during review
and the reason is on the following line. It is a task, not documentation:

1. Implement a correct version that addresses the stated reason.
2. Delete the whole `AI-REJECTED` comment block in the same edit.

Do not reinstate the reverted code as it was, and do not leave the comment
behind once the code is right — a commit that still carries one is refused by
a `pre-commit` hook.
<!-- /code-basics -->

## Frontend customization contracts

Appearance preferences are global and versioned in `src/appearanceLogic.ts`;
DOM application and terminal colour derivation live in `src/appearance.ts`.
Keep theme decisions in the pure logic module and cover migrations or validation
changes in `src/appearanceLogic.test.ts`.

Adding a field to those preferences is not a reason to bump the schema version.
`readAppearance` re-derives each field rather than spreading the parsed object,
so a new one whose absent value is safe reads correctly at version 1. A bump
makes the version gate reject every stored blob and fall back to the defaults,
discarding each user's custom themes and font sizes. Bump only when an existing
field's meaning changes.

Window transparency keeps the stored number (`windowOpacity`) apart from the
runtime decision (`src/windowTransparencyLogic.ts`), because that decision spans
open codebases. Its rule is that a codebase which has not reported whether its
editor area is empty has not reported it empty: an unknown root resolves to
fully opaque, which is what stops the window flashing translucent during
startup. The **welcome screen** (no codebase open, `activeRoot === null`) is also
fully opaque; translucency begins only once an open codebase reports an empty
editor. Only the active codebase is consulted. `applyAppearance` must never
write `--app-bg-opacity` — it cannot know the editor state, and a second writer
would race the one in `App`. Floating chrome paints its own opaque surface so it
stays readable over a translucent window; the top Run/Debug/Stop `.toolbar` does
the same (`background: var(--bg)`).

The Tasks plugin panel is **per-workspace**, unlike the global Notes panel: every
tasks command takes the panel's own `root` explicitly rather than reading the
active workspace, since several codebases can be open at once. Its store is
gitignored (private to the machine). **Assign to AI** persists `owner = ai` then
launches the agent in an interactive terminal with the task as the prompt;
**Assign to me** only persists `owner = me`.

The Roslyn MCP server (`mcp-roslyn`, opened as a Plugins-menu panel) exposes four
**read-only** code-intelligence tools — `find_references`, `get_type_hierarchy`,
`resolve_overloads`, `get_diagnostics` — to a coding agent by forwarding each call over a
per-pid named pipe to the running app's already-warm language-server session, rather than
starting its own. It is scoped by `--workspace` (the consent boundary) and installs into
the same agent configs as the SQL and Tasks servers with a preview-then-apply flow. Every
tool is capability-gated and **abstains** (`Unsupported`) for a language whose server does
not support it; a symbol whose position is ambiguous is refused and listed, never guessed;
and no internal error text crosses to the agent.

The Editor Context MCP server (`editor_context/`, `mcp-editor`, opened from the Plugins menu)
is the **Roslyn twin** for *what the user is looking at*: four **read-only** tools —
`get_active_file`, `get_selection`, `get_open_files`, `get_recent_files` — answered over a
per-pid named pipe from state the frontend **pushes** into `AppState` per workspace
(`set_editor_context`), not from a language server. Positions are 1-based line / 0-based
UTF-16 char; selection text and the recent-edited list are capped. It is `--workspace`-scoped
(the consent boundary) and gated by `FeatureId::EditorContextMcp` (default on) **two ways** —
the frontend stops pushing when the feature is off, and the pipe host re-reads the feature per
call and refuses `Disabled` — with `NoWorkspace`/`Disabled`/`NoContext`/`NoActiveFile` kept as
distinct refusals. Installs into the same agent configs with the preview-then-apply flow, its
caveat stating that selection text and file paths reach the agent.

The Redis plugin (`redis/`, opened from the Plugins menu; always-on, no `FeatureId`) is a
RedisInsight-style panel plus a **write-capable** `mcp-redis` server, modelled on the SQL
console. It discovers connections from appsettings/user-secrets/`.env` (filesystem only,
values dropped; it **shares `sql::store::SecretSource`**), browses the keyspace, and views
and edits string/hash/list/set/zset/stream values. Two separate per-connection consents,
both off by default: `expose_to_agents` (reads) and `allow_writes` (writes). Redis has no
server-side read-only mode, so the write gate is structural — a `WriteOp` runs only once
`redis::ops::plan_write(allow_writes, op)` mints a `WritePlan`, unreachable from any read
path. The panel itself may always write; `allow_writes` gates only the agent. No connection
string, path or driver message ever reaches an agent.

The Build & Diagnostics MCP server (`build/`, `mcp-build`, opened from the Plugins menu) lets
a coding agent build one repository and read the build's **structured** diagnostics —
`build_solution`, `get_errors`, `get_warnings`, `get_build_status` — over the same per-pid
named-pipe pattern as Roslyn/Editor context. It exists because the bundled Roslyn advertises
no `diagnosticProvider`, so `get_diagnostics` abstains `Unsupported` for C#; this server runs
`dotnet build` with MSBuild errors-only/warnings-only file loggers (invariant culture, full
paths) and parses the artifacts into a `BuildReport` — the `testing/trx.rs` "parse what the
tool emitted" model, pure and tested in `build/{model,parse}.rs`. `BuildStatus` is six distinct
answers (**neverBuilt / building / succeededClean / succeededWithWarnings / failed /
couldNotStart**, all reachable — `Building` is written before the build is awaited so a
concurrent `get_build_status` is never stale); an unparseable line lands in `warnings` not
`diagnostics`; no driver/OS text crosses to the agent. `--workspace`-scoped (the consent
boundary) and gated by `FeatureId::BuildMcp` (default on) two ways — re-read per call. Installs
into the same agent configs with the preview-then-apply flow; its caveat states that a build
writes compiler output (`bin/`, `obj/`) but edits no source.

Per-**tool** MCP gating lives in `tool_gate/` (user-global `mcp-tools.json`): each of the
six built-in servers (SQL, Tasks, Roslyn, Browser, Editor context, Build) filters its `tools/list`
through `tool_gate::filter_descriptors` and
refuses a disabled-but-known tool at dispatch (re-read per call). It is a **preference, not
a security boundary** — a missing/corrupt store means every tool on, the opposite of the
consent stores. Surfaced in Settings → **MCP tools**. Redis is not one of the gated
`ServerId`s and advertises all its tools.

App-owned shortcuts are declared in `src/shortcutLogic.ts` and dispatched by
`src/shortcuts.ts`. A command shown in Settings must have a registered handler
or a stable `data-command` target. Editor and terminal native shortcuts are
reference-only. Keep conflict, normalization, and persistence decisions in the
logic module with tests.

Background project signals are merged by severity in
`src/components/workspaceTabsLogic.ts`. Failures persist, successful completion
expires, cancellation is quiet, and events already visible in the active
workspace must not be latched for later.

Search All must reserve room for files, symbols, and actions; a large symbol
population must never crowd matching files (including `.razor`) out of Ctrl+N.

An Intent card represents one declared intent. Merge exact identical agent
labels across turns, preserve user-authored card identity, assign uniquely
evidenced lines only to their intent, and duplicate only genuinely ambiguous
lines into each plausible intent card. Retirement runs conservatively on every
Intent load, including the first.

Notes colors belong to individual note records in the versioned global schema,
not to the Notes window. The titlebar Notes action must restore a mounted,
minimized panel.

Debug is a separate Run-tab action, and its decisions belong in
`src/views/debugLogic.ts`, not in `RunView.tsx`. Availability must match what
`start_debug` accepts and must explain a refusal, including checking every
member of a compound before the button is offered. Event mapping must keep the
six debug states distinct: preserve what a missing adapter looked for, preserve
a failure's detail, emit nothing for `notRunning`, and never report a null exit
code as a failure — that is what a stop or a replacement launch produces.

Debug adapters ship with the installer, vendored by `pnpm debuggers:fetch` with
pinned versions and SHA-256 verification. Resolution order is environment pin,
then bundle, then `PATH`; an absent bundle is an ordinary answer, and a missing
adapter is always reported rather than degraded into an ordinary run.

Reading a debug adapter's protocol stream must stay decoupled from emitting to
the UI, and the queue between them must stay unbounded. A client that stops
draining the adapter's stdout deadlocks the debuggee: the pipe fills, the adapter
blocks writing from inside a runtime debug callback that holds every debuggee
thread suspended, and the application freezes with no output, no error and no
exit — looking exactly like one that is merely quiet. Cost is bounded by merging
output (`dap::coalesce`), never by making the reader wait. Merging must not
combine `stdout` with `stderr`, must not split a chunk over the byte cap, and
must not withhold: flush before blocking, and flush before any non-output event,
so nothing overtakes output still held back.

A control placed inside a floating panel's drag header will not receive `click`
or `dblclick`: the drag handler takes pointer capture, and the browser then
dispatches those to the capturing element, never down to a child. Exempt such a
control from the drag the way the header buttons already are. For the same
family of reasons, focusing another element during `pointerdown` does not stick —
the default mousedown action moves focus afterwards — so focus inside a
`setTimeout(..., 0)`.

Returning `false` from xterm's `attachCustomKeyEventHandler` stops only xterm's
key translation, not the webview's native handling. A paste chord must
`preventDefault()` as well, or the native paste reaches xterm's hidden textarea
and its own paste listener writes the text a second time. Route pasted text
through `term.paste` so bracketing and CRLF normalization match the native path
rather than diverging from it, and keep `Ctrl+C` a passthrough that never
prevents the default. Any `Ctrl+`key chord also needs an `altKey` guard, because
Windows reports AltGr as Ctrl+Alt.

A rename with more than one destination must apply one acceptance rule to all of
them. Terminal titles go through `acceptedTerminalTitle`, which is
`normalizeLabel`; a second test such as `trim()` at one call site diverges on
exactly the inputs the cleaning exists for and lets a refused title reach the
other destination. SQL connection names follow the same shape through
`acceptedConnectionName`, and the manual-create form asks it too rather than
trimming — otherwise creating accepts a name renaming refuses.

A name the user typed and a name the app derived are different facts, and a name
alone cannot tell them apart. A saved SQL connection therefore carries
`userNamed` beside its `name`: the picker composes `project · source · key` only
while that is false, and `upsert` will not overwrite a name once it is set, so
re-adopting a discovered connection cannot revert a rename. Only the rename verb
sets the flag, exactly as only the consent verb moves `allowWrites`.

A menu opened from the titlebar sits above the floating panels, and the generic
`.dropdown` chrome does not: at z-index 41 over a backdrop at 40 it hides behind
any terminal, and so does the backdrop that closes it. Use `ContextMenu`'s
`elevated`, or the `dropdown-elevated` / `dropdown-elevated-backdrop` classes for
a plain dropdown. Raise the backdrop in the same change as the menu, and express
the level as a class — the bands live in `styles.css` and no z-index integer is
written in TypeScript.

Shell detection omits what it cannot find. An empty detected list is a
legitimate answer, not a fallback — terminals still open on `default_shell` —
and `pick_shell`'s last-candidate fallback must not be copied into it. A saved
shell preference whose shell has disappeared is reported and kept, never erased
and never spawned as a bare program name. Do not offer `wsl.exe`: it ships on
every Windows install regardless of whether a distribution exists, so its
presence is not evidence a shell would start.

Build provenance abstains to the literal `unknown` and never fails the build. A
sha from a modified tree must carry its `-dirty` marker, because an unmarked sha
is a wrong statement about which code is running. `.git/HEAD` alone is not
enough to keep the stamp fresh — committing on a branch rewrites the branch ref
and leaves `HEAD` untouched — so watch the resolved ref and `.git/index` too,
and show the build date beside the commit rather than asking a reader to infer
freshness from the sha.

The Changes file list carries a multi-selection that is separate from the file
shown in the diff pane. A right-click inside the selection acts on the whole
selection; a right-click outside it acts on that single row. A Shift-range
follows the rendered row order, not the flat order. Stashing selected files must
stash only those paths and leave every other change, staged or not, in the
working tree; conflicted files are never offered.
- F2 rename is `refactor.rename` in `shortcutLogic.ts`, `allowInText` because the
  caret is by definition in CodeMirror when it fires. Every decision lives in the
  tested `renameLogic.ts`, and it must **abstain on language rules**: reject only
  an empty name, one containing whitespace, and one equal to the old name. A
  per-language identifier validator here would be a second, worse opinion than
  the language server's own and would refuse `@class`, non-ASCII names and `$`.
- A rename refuses to open its field while a `didChange` is owed, flushes first,
  and re-checks the document version again at Enter. Ranges computed against a
  stale mirror are plausible and wrong, which is worse than a wrong count.
- Several `FileEditor`s are mounted at once inside hidden wrappers, so exactly
  one must answer F2: visibility is `getClientRects().length > 0`, never
  `offsetParent`, which is null for the fixed floating panels.
- The rename field is a positioned `<input>`, not a CodeMirror widget. DOM inside
  CodeMirror fights its event handling and is torn out by the next viewport
  update, and a document-mutating approach would fire `docChanged` and
  desynchronise the very mirror the ordering rule protects.
- Rename edits reach other open tabs through the request-and-consume monotonic
  token pattern (`pendingEdits`/`pendingEditsToken`), matched on
  `source.kind === "workspace" && source.path === path` — **never**
  `file.id === path`, because a diff tab carries a `path` too. A buffer with no
  receiving editor is reported, never dropped.
- `PlanPreview` renders the entire final contents of each file it would write,
  and one of those is a 122 KB `~/.claude.json`. Its `<pre>` must stay inside its
  own capped scroll container: unstyled, it pushed out of a `max-height: 70vh`
  modal in both axes and took the confirm buttons with it. Content is
  deliberately not wrapped — this is JSON and TOML a person is verifying before
  it is written to their machine.
- A modal that clears its own preview on success must say what it did. Without a
  written-note surface a successful install is indistinguishable from a cancel,
  and the natural response is to install a second time. `writtenNote` names the
  files actually written — from the approved plan, not a second read — and states
  both silent-no-op conditions: a project `.mcp.json` needs the agent's approval
  before it loads, and a running agent will not see the change until it restarts.
- A capability granted by window label reaches **every** webview of that window:
  `RuntimeAuthority::resolve_access` matches a capability's `webviews` *or* its
  `windows`, and a child webview inherits its parent's window label. So grant by
  `webviews` (`capabilities/default.json`), and anything hosting untrusted content
  must appear in no capability that names its window.
- A capability granted by **window** label reaches every webview of that window
  (`RuntimeAuthority::resolve_access` matches webviews OR windows, and a child
  webview of `main` has window label `main`). Anything hosting untrusted content
  must therefore be granted by `webviews` and appear in no capability naming its
  window — which is why `capabilities/default.json` says `"webviews": ["main"]`.
- The browser page is an **OS surface**, not a DOM layer: it composites above
  everything, ignores the `--z-panel`/`--z-notes`/`--z-dock`/`--z-overlay` bands, and
  `hidden` on a React div does not hide it — so anything that must appear over it hides
  the page (`pageVisible`/`occlusionContext`, overlap-scoped, not blanket). Occlusion is
  **raise-aware** (`occludedByAbovePanels`): the panel joins the app-wide focus order
  (`focusOrderContext`) with terminals and Notes, so only a peer *stacked above* the
  browser blanks the page — clicking the browser brings it to the front. It is a bug for
  it to be visible when **minimized, feature-off, at a degenerate rect, its workspace not
  foreground, its setup modal open, or a menu/modal/above-it panel covering it**.
- **Split docking never remounts a moved editor/terminal.** `DockableTerminal`/
  `DockableEditorSlot` portal through `components/StablePortal.tsx` — one stable host node
  moved between the float layer and a region slot with `appendChild`. Swapping
  `createPortal`'s container instead *does* remount in React, which closed a docked
  terminal's PTY and discarded a docked editor's CodeMirror/LSP state (the bug this fixes).
  The **SQL console and browser dock too** (`DockableKind` gained `"sql"`/`"browser"`,
  `pruneLayout` takes a per-kind `valid` map): SQL rides `StablePortal` like an editor; the
  browser keeps its page-lifecycle effects top-level (never remount) and moves only its
  stateless chrome via `createPortal`, the OS page following the placeholder rect through
  `browser_set_bounds` — so docking it needs **no Rust change**.
- Anything that shows or positions that surface must be **generation-stamped and
  abandon its writes when superseded**, checked before *each* write rather than
  once. `sync()` awaits a scale factor and two IPC calls having captured
  `minimized` at entry, so without it a stale run re-shows a window the user
  cannot dismiss over what they are reading.
- Never open a pipe, path or address a *file* merely states. `instances::pipe_name(pid)`
  is derived from a pid already verified against its recorded executable; a
  registry naming anything else is refused as tampering rather than corrected,
  because that refusal is the only visible symptom of a local process answering
  an agent as if it were this application.
