# Agent intent capture

The Changes tab can collapse a diff into the *decisions* behind it — "add retry to token refresh", "Whitespace only", "`EstimateCost`" — and stage or revert each as a unit. Switch between **Files** and **Intent** at the top of the Changes sidebar.

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
```

or, when one turn made unrelated changes, one line per reason scoped to the files it touched (paths workspace-relative, comma-separated):

```
Intent(src/api.ts, src/apiLogic.test.ts): <why, for those files>
```

A scoped line covers the files it names and one plain line may cover the rest; extra plain lines are ignored — only the first is used.

When a Claude Code turn edits files and ends without an `Intent:` line, the Stop hook asks for one before the turn finishes. It asks **once per turn** — if the agent does not comply, the turn ends anyway and the card falls back to being titled from its changes, so the request can never wedge a session. It applies only to workspaces that enabled capture, only to turns that actually changed something, and only to Claude Code (nothing establishes that Codex honours a blocking stop). Set `askForIntent` to `false` in [`config.json`](../reference/configuration.md#configjson) to turn it off.

The section is marked with `<!-- code-basics: agent intent -->` … `<!-- /code-basics -->`, so installing twice does not repeat it, and it appears in the confirmation dialog like any other file. Delete the section if you would rather not have it — capture still works, but the label falls back to the first sentence of the closing message, which was written for someone reading a chat rather than as a card title.

Once capture is on, the setup panel shows **Re-apply setup…** instead of the enable buttons. Use it after an update changes the hook command or the instruction wording: it previews and re-writes both at the same level — the hook entry is replaced rather than duplicated, and whatever sits between the section's markers is replaced with the current request, leaving the rest of the file untouched. A section whose closing marker has been deleted is left alone rather than guessed at.

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

A user-level hook fires for every repository on your machine, and does not name a workspace. Each time it runs it walks up from the agent's working directory and records into the first **enabled** workspace it finds — enabled meaning `.code-basics/intents/` exists there. Anywhere else it exits having written nothing. That is what makes "Enable for me…" a one-time action: turn capture on in a new repo and the hook you already have starts feeding it, without being reinstalled.

A repo-level hook is the opposite: it names its own workspace on the command line, because it only ever runs inside it.

### A user hook installed by an older version

Earlier versions pinned the user-level hook to whichever workspace was open when you installed it, so it recorded into that one repository and nothing else. If code-basics finds such a hook, the setup panel stops claiming capture is on and says so instead:

> Your user-level hook is pinned to `C:\Users\you\Code\SomeRepo` and will not record here. Enable capture again to repair it — the entry is replaced, not duplicated.

The repair is just **Enable for me…** again. The install is additive as always, but the marked entry is *replaced* rather than added beside the old one, so you do not end up recording twice.

## When nothing is agent-stated

Every card being inferred looks the same whatever the cause, so when there are cards and **none** of them is Stated, a banner appears above the list saying which of three situations this actually is:

| Situation | What it says | Buttons |
|---|---|---|
| Capture is off, past sessions exist | These edits were never recorded; *n* past sessions can be imported now | **Enable capture**, **Import past sessions (n)** |
| Capture is off, no sessions found | Nothing is being recorded for this workspace and no past sessions were found | **Enable capture** |
| Capture is on, nothing matched | The records may be from another branch, or these edits may predate them | **Import past sessions (n)**, when there are any |

Anything the providers report as standing in the way — the pinned user hook above, an untrusted Codex project — is listed under the sentence rather than left in the collapsed setup pane. **Enable capture** opens that pane; **Import past sessions** runs the import and reports how many records it ended up with.

The same abstain rule applies here as to the cards: the banner stays silent when there is no diff at all, and when even one card is Stated, since capture is then demonstrably working.

## What the cards mean

| Badge | Meaning |
|---|---|
| **Stated** | The agent declared this with an `Intent:` line — the only badge that claims a reason |
| **One turn** | One turn made these changes but never declared why. The hunks really did change together, so they are still grouped; the title describes them rather than explaining them |
| **Formatting** | Whitespace only — no code changed. The card is titled "Whitespace only" |
| **New** / **Changed** | A symbol not in the baseline, or one whose body changed |
| **Unexplained** | Nothing could be determined; grouped by file |

**Only a declared `Intent:` line earns the Stated badge.** Anything the app had to mine out of prose — the first sentence of a closing message, or a sentence lifted from session history — becomes a **One turn** card instead. The sentence is still shown when there is one, because it is the best description available, but the badge says where it came from. When there is no usable sentence at all the card is titled from its own contents: the enclosing symbol if every hunk shares one, otherwise "N files changed together". That title is a description and is never presented as a reason.

Mined sentences are also filtered before they are kept. Prose written for a human reading a chat is full of things that are not labels — status about the tooling or the model, pleasantries, and announcements of what the agent is *about to* do — and a card once ended up titled *"The workflow is running with Opus"* over three files. Those shapes are now refused outright, on the standing rule that no label is better than a wrong one. A declared `Intent:` line is never filtered: it is the agent's own words for the card.

A symbol card is titled with the **bare symbol name** — `EstimateCost`, not "New method EstimateCost()". The badge beside it already says New or Changed, and a declaration that names both a type and a binding is read as the binding: `let total: usize` is `total`, `static COUNTER: AtomicU64` is `COUNTER`, `const cache: Map<…>` is `cache`.

Lines that name no symbol never become a title. A hunk header that is an import (`import`, `use`, `using`, `from`, `#include`, `package`) or an ordinary statement — anything containing `;` or a quote — falls through to be grouped by file instead, because "New import" on a card says less than the file's name does.

### Several small edits in one file

Naming each hunk after its enclosing symbol is right when a symbol collects several hunks, and wrong when one file is touched in a dozen unrelated places: that produces one card per hunk, which is the pile the grouping exists to remove. So when a single file yields **two or more** cards that are each one hunk of one symbol, they merge into a single **Several changes in `<file>`** card, together with that file's Unexplained bucket if it has one.

Deliberately untouched: a card with several hunks, a card spanning several files (a symbol touched in two places is a real grouping), and a file's lone symbol card — its name is a better title than the file's. Nothing is ever merged across files, and Stated and Formatting cards are never merged at all.

The dots are confidence. Only a verbatim match against distinctive text reads ●●●; text that matches apart from formatting reads ●●○, because something has been through the file since the agent wrote it.

Selecting a card opens its first file and lists every file the card touches, each with its share of hunks and lines. Clicking a file shows **only that card's changes in it** — the same file can sit in several cards, and each shows just its own hunks. **Stage group** and **Revert group** act on every line in the card, across every file; the **Stage**/**Revert** buttons on a file row act on that file's share alone.

## Rejecting a change

**Revert group** removes the code and says nothing. The agent that wrote it learns nothing either, so next turn it writes the same thing again and you pay for the same mistake twice.

**Reject group…** (and **Reject…** on a file row) is a revert that leaves the reason behind. Type why it was wrong, and code-basics reverts the change and writes a comment where it was:

```rust
    // AI-REJECTED <date> — reverted during review.
    // Reason: matches a column that happens to be called limit
    // Next: fix this properly, then delete these AI-REJECTED lines.
```

A comment in the file rather than an entry in `.code-basics/` for one reason: the agent reads the file. It does not read `.code-basics/`, and nothing can make it read it. The note also goes into `CLAUDE.md`/`AGENTS.md` as a standing instruction — what the marker means, and that the fix is to implement a correct version and delete the whole block in the same edit.

The note lands **above the restored line**, indented to match it, one per rejected hunk. Rejecting the same place twice replaces the note rather than stacking a second one.

Three things it deliberately will not do:

- **No block comments.** A `/*` that fails to close silently comments out the rest of the file. So only line-comment languages are marked; a reverted `.json` or `.css` is reported back as *reverted without a note*, rather than the reason being silently dropped.
- **Not in the staged view.** A revert in `indexToHead` changes the index, so the note would explain a change you are not looking at — and would itself be unstaged. The button is disabled there.
- **No reason, no rejection.** The reason is the entire difference between this and a revert.

### The commit guard

Enabling capture also installs a `pre-commit` hook. It refuses any commit whose staged files still carry a note, naming them:

```
code-basics: these staged files still carry an unresolved AI-REJECTED note:
  src/lib.rs
Fix the code and delete the note, or commit with CB_ALLOW_REJECTED=1.
```

Without it the note survives into a commit, then into review, and eventually reads as ordinary commentary nobody dares delete. `CB_ALLOW_REJECTED=1` gets one past deliberately; `--no-verify` skips the hook entirely.

The hook is installed the same way as everything else here — into `core.hooksPath` when one is set, otherwise `.git/hooks/pre-commit`; bounded by `# >>> code-basics: rejected-change guard >>>` markers so an existing hook is appended to and a re-install rewrites only that span; backed up first; and shown in full in the confirmation dialog.

It matches the token *followed by a date*, not the bare token, so a file that merely mentions `AI-REJECTED` — this guide, the source that defines it — stays committable. For the same reason the script assembles the token at runtime instead of containing it.

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
