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

Bundled starters: `code-review.md` and `write-tests.md`.
