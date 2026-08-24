# Enhancements: instructions & prompts

The menu-bar **Enhancements** menu has two submenus, both generated from plain
`.md` files so the libraries are edited without recompiling the app:

- **Add Instructions** — add a reusable section to the workspace's `CLAUDE.md`
  and `AGENTS.md`.
- **Run Agent** — run a saved prompt as an agent (Claude Code / Codex) in the
  panel.

The Enhancements menu also has a **Review changes…** item — the same adversarial
review as the Changes-tab **Review** button, reachable without leaving the
current tab. The panel remembers the agent, model, and prompt you last ran and
pre-selects them next time (the read-only / allow-edits posture is **not**
remembered — editing is an explicit choice each run).

(The menu bar also has a **File** menu — Open, Rescan, Exit — mirroring the
titlebar buttons.)

## Instructions

### Where the templates live

Templates are read from a per-user directory:

- Windows: `%APPDATA%\code-basics\instructions\`
- Other platforms: `$XDG_CONFIG_HOME/code-basics/instructions/`, falling back to
  `~/.config/code-basics/instructions/`
- `CB_INSTRUCTIONS_PATH` overrides the location entirely (used under
  `pnpm tauri dev`, where the bundled resource directory does not resolve — point
  it at `src-tauri/resources/instructions`).

The directory is created on first use and seeded with the bundled defaults. Your
own edits are never overwritten by seeding. **Drop a new `.md` file in and it
appears in the menu automatically** — there is no registry to update.

### Template format

A template is markdown with a small `---`-fenced front matter block:

```markdown
---
title: Memory Files
id: memory
placement: after-first-heading
---
## CRITICAL: Memory Files

...the section body...
```

| Key | Meaning |
|-----|---------|
| `id` | Stable slug. Sources the section markers and prevents duplicates. Defaults to the file name. |
| `title` | Menu label. Defaults to `id`. |
| `placement` | Where the section is inserted (see below). Defaults to `end`. |
| `anchor` | Required only for `before-marker` / `after-marker`. |

#### Placement values

- `top` — the very top of the file.
- `after-first-heading` — immediately after the first `#` heading. No heading
  falls back to `top`.
- `end` — appended after everything else.
- `before-marker` / `after-marker` — relative to a line containing `anchor`. A
  missing anchor falls back to `end`.

### What clicking does

Clicking an instruction asks for confirmation first (it writes to files you
share with your team). On confirm it writes the section into **both** `CLAUDE.md`
and `AGENTS.md` (creating them if absent), wrapped in a marker so it can be found
later:

```html
<!-- code-basics: enhancement:memory -->
...body...
<!-- /code-basics: enhancement:memory -->
```

The write is idempotent: adding an already-present section refreshes it in place
rather than duplicating it, and the original file is backed up to `*.bak` before
any merge. An installed item shows an **added** badge; the ✕ removes its section
from both files and normalises the surrounding blank lines (no confirmation — it
is a revert). Editing a template and re-adding it updates the section already in
your files.

## Run Agent (prompts)

Prompts reuse the same `.md` format, but instead of being written anywhere their
**body is run as an agent** in the floating panel — the same panel the
adversarial Review uses. Use them for reusable tasks (a knowledge-graph build,
an initial setup, a scripted refactor).

- Location: `%APPDATA%\code-basics\prompts\` (or `$XDG_CONFIG_HOME`/`~/.config`
  elsewhere); `CB_PROMPTS_PATH` overrides it for `pnpm tauri dev`.
- Same discovery and seeding as instructions — drop a `.md` file in and it
  appears under **Enhancements → Run Agent**.
- Same front matter (`id`, `title`); `placement` is irrelevant and ignored. One
  extra key applies: `once`.
- Clicking a prompt opens the panel with that prompt pre-selected. Pick the
  agent, model, and **posture** — *Read-only* (explore and report) or *Allow
  edits* (modify files) — then press **Run**. Both postures run headlessly and
  never block on an approval prompt.

### Run-once prompts

A prompt whose front matter includes `once: true` is meant to run a single time
per repository (setup, a one-off migration, a graph build):

```markdown
---
id: knowledge-graph
title: Build the knowledge graph
once: true
---
...prompt body...
```

- When such a prompt **finishes successfully**, that is recorded for the current
  workspace in `.code-basics/agent-runs.json` (a failed or cancelled run leaves
  no record).
- An already-run run-once prompt shows a **ran …** badge in the menu, and
  clicking it asks *"…already ran here — run again?"* before re-running. An
  ordinary prompt (no `once`) never records or confirms.

Bundled starters:

| Prompt | Mode | What it does |
|--------|------|--------------|
| `code-review.md` | read-only | Reviews the current diff for correctness bugs and cleanup opportunities. |
| `security-review.md` | read-only | Reviews the diff for injection, authz/authn gaps, secret handling, unsafe deserialization, SSRF/path traversal, and missing validation. |
| `perf-review.md` | read-only | Reviews the diff for needless allocation, N+1 access, quadratic loops, blocking in async, and missing pagination/index. |
| `concurrency-review.md` | read-only | Reviews the diff for data races, lock-ordering/deadlock, await-holding-lock, non-atomic check-then-act, and cancellation/teardown gaps. |
| `naming-drift.md` | read-only | Flags names in the diff that no longer match behavior, terminology inconsistent with the surrounding code, and misleading identifiers. |
| `write-tests.md` | edits | Writes tests for a change tests-first, watching them fail before implementing. |
| `extract-rules.md` | edits | Infers the codebase's business-rule invariants and writes each to `.code-basics/rules/<slug>.md`. |
| `verify-rules.md` | read-only | Checks the diff against the recorded rules in `.code-basics/rules/` and reports violations. |
| `verify-claims.md` | read-only | Extracts the diff's checkable claims/ACs and judges each against a prepended behavioral before/after report. |
| `code-knowledge-graph.md` | edits | Builds a local git-history + prose knowledge graph tool under `tools/kg/`. |
| `setup-docs.md` | edits | Bootstraps a phased `docs/` + `.claude/` agent-readable knowledge layer. |

## Business rules: `.code-basics/rules/`

Two of the bundled prompts work as a pair around an **authored, committed**
rules directory:

- **Extract Business Rules** (`extract-rules.md`) reads the codebase and writes
  each business-rule invariant it can point at — a guard, a validation, a
  constraint the code actually enforces — to its own file under
  `.code-basics/rules/<slug>.md`, with `id`/`title` front matter, a prose
  statement of the invariant, and a note of where it is enforced. One rule per
  file; it abstains rather than inventing rules the code does not enforce.
- **Verify Business Rules** (`verify-rules.md`) reads every
  `.code-basics/rules/*.md` and checks the current diff for any change that
  violates a stated invariant, citing the rule, the file and line, and the
  concrete scenario in which the invariant breaks. An empty rules directory
  makes it point you at Extract Business Rules first.

Unlike the gitignored per-workspace state under `.code-basics/` (intents,
symbols cache, agent-runs), the `rules/` directory is meant to be **authored and
committed** — it is a shared, reviewable statement of the domain's invariants,
so it travels with the repo the way `CLAUDE.md` does.

## Verify Claims and the behavioral context flow

**Verify Claims / ACs** (`verify-claims.md`) checks whether the diff does what
it claims. When it runs, the app may **prepend a before/after behavioral report**
as context — the same test-result / console-output / HTTP-replay deltas the
Intent view renders beside the coverage Scorecard, computed by running a config
against `HEAD` and against the working tree. The prompt treats that report as the
evidence and the diff plus its stated intents as the claims: it extracts each
checkable claim, judges it **confirmed / contradicted / unverified** against the
evidence that speaks to it, and for any claim the evidence does not cover says
exactly what test or `.http` scenario would confirm it. It never marks a claim
confirmed without evidence — the same prepend-context-as-input pattern that
Verify Business Rules uses with the loaded rules.
