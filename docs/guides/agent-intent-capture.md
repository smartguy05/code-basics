# Agent intent capture

The Changes tab can collapse a diff into the *decisions* behind it — "add retry to token refresh", "reformatted 3 files", "new method `EstimateCost()`" — and stage or revert each as a unit. Switch between **Files** and **Intent** at the top of the Changes sidebar.

This works with [Claude Code](https://claude.com/claude-code) and [OpenAI Codex](https://github.com/openai/codex), together or separately.

## Why ask the agent instead of analysing the diff

A diff records what changed and never why. Reconstructing intent afterwards means guessing from syntax. But the agent that wrote the change knew exactly why it touched each region seconds earlier — so the grouping asks, and only falls back to analysis for what nobody explained.

Three passes, in decreasing confidence:

1. **What the agent said.** Captured live from its hooks.
2. **Formatting.** A hunk whose tokens are identical either side changed no code. Decidable, not guessed.
3. **The enclosing symbol.** Which function or type the hunk sits in, mostly from the hunk header git already writes.

Anything none of them explain is shown as unexplained rather than folded into a neighbour. **A card never claims more than it knows** — that rule is why a wrong label is treated as much worse than no label.

## Working with no setup at all

Both agents already keep session transcripts. Open the Intent view, press **⚙**, then **Import past sessions** — code-basics reads what is on disk and groups your existing working tree retroactively.

Labels recovered this way are coarse. In a Claude Code transcript an assistant message contains *either* prose *or* tool calls, never both, so the best available label is the nearest preceding sentence — which usually covers the handful of edits that followed it. Good enough to group by; not as good as asking.

| | Claude Code | Codex |
|---|---|---|
| Sessions | `~/.claude/projects/<encoded-path>/*.jsonl` | `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl` |
| Matched to this workspace by | the `cwd` recorded in the file | the `cwd` on the opening `session_meta` line |

Codex compresses cold sessions to `.jsonl.zst`. Those are skipped, and the count is reported in the setup panel rather than passed over silently.

## Capturing intent as it happens

**Set up agent intent capture… → Enable for this repo…** or **Enable for me…** installs two hooks:

- `PostToolUse` records *what* changed after every edit.
- `Stop` records *why*, from the agent's closing message.

Neither agent lets a model attach a rationale to a tool call — that is why the reason has to come from the end of the turn. The two are written separately and joined on the turn identifier both carry, so the join is exact rather than a guess.

Nothing is written until you have seen it. The dialog shows the full final contents of every file it would touch, and marks any file it is merging into rather than creating.

### Asking for good labels

The install also appends a short section to `CLAUDE.md` (Claude Code) or `AGENTS.md` (Codex), asking the agent to end an editing turn with:

```
Intent: <3-5 words describing why>
Intent(path/to/file.rs): <why, for one file specifically>
```

Append-only and marked, so installing twice does not repeat it, and it appears in the confirmation dialog like any other file. Delete the section if you would rather not have it — capture still works, but the label falls back to the first sentence of the closing message, which was written for someone reading a chat rather than as a card title.

### Where the hooks go

| | Claude Code | Codex |
|---|---|---|
| This repo | `.claude/settings.json` | `.codex/hooks.json` |
| Just me | `~/.claude/settings.json` | `$CODEX_HOME/hooks.json` |
| Label request | `CLAUDE.md` | `AGENTS.md` |

The hook config is a single `PostToolUse` entry matching `apply_patch|Edit|Write` and one `Stop` entry, each running this application as `record-intent`. One matcher serves both agents because Codex registers `apply_patch` with `Write` and `Edit` as aliases.

Both project files are normally committed, so enabling for a repo shares the hook with everyone who clones it. The dialog says so.

Installation is **additive**. Existing hooks are preserved, the file is backed up first, and installing twice does not duplicate anything. This matters: agent hook files commonly already drive unrelated tooling.

Two Codex-only conditions are reported in the setup panel rather than left to puzzle over, because in both cases the configuration looks right while doing nothing:

- A repo-local `.codex/` directory is ignored until the project is **trusted** — open it in Codex once and accept the prompt.
- Codex asks you to review a new command hook before it will run it.

A user-level hook fires for every repository on your machine. It does nothing in workspaces that have not enabled capture, and records only into the one named on its command line.

## What the cards mean

| Badge | Meaning |
|---|---|
| **Stated** | The agent said this is what it was doing |
| **Formatting** | Whitespace only — no code changed |
| **New** / **Changed** | A symbol not in the baseline, or one whose body changed |
| **Unexplained** | Nothing could be determined; grouped by file |

The dots are confidence. Only a verbatim match against distinctive text reads ●●●; text that matches apart from formatting reads ●●○, because something has been through the file since the agent wrote it.

Selecting a card opens its first file with the relevant lines already selected. **Stage group** and **Revert group** act on every line in the card, across every file.

## Where records live

`.code-basics/intents/` — `edits.jsonl` (what changed) and `labels.jsonl` (why). Gitignored automatically: this is a log of one person's session, and large. Remove it any time; nothing else depends on it.

## When it will not label something

Stated honestly, because the UI shows these as unexplained rather than pretending:

- Code rewritten after the agent touched it — a rename, a lint autofix.
- Very short edits: a version bump, a flipped boolean.
- Repetitive code, where no line is distinctive enough to identify.
- Formatters that reflow *across* line boundaries. The matcher is line-based and cannot follow that.
- Files changed by a shell command rather than an edit tool. Neither agent records a structured change for those.
- Edits made before capture was enabled, in a workspace with no session history.

Two records that made the same change in the same file can also be confused with each other; the newer one wins. Rename detection is not attempted at all — git's own is similarity-based, and a wrongly claimed rename reads as a far stronger statement than "these hunks are near each other".

Related: [Changes and history](../getting-started/using-the-app.md) · [configuration](../reference/configuration.md) · [commands](../reference/commands.md)
