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
in the Changes tab, not a commit message.

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

The Changes file list carries a multi-selection that is separate from the file
shown in the diff pane. A right-click inside the selection acts on the whole
selection; a right-click outside it acts on that single row. A Shift-range
follows the rendered row order, not the flat order. Stashing selected files must
stash only those paths and leave every other change, staged or not, in the
working tree; conflicted files are never offered.
