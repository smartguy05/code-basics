# Agent Scaffold Bootstrap Prompt

Paste the block below into Claude Code at the root of a repository that has **no**
agent scaffolding yet. It reproduces the system that exists in the ONEflight repo:
`docs/`, `.claude/indexes/` + generator, `.claude/runbooks/`, `.claude/cookbooks/`,
`.claude/agents/`, `.claude/commands/`, `.claude/hooks/`, doc-validation scripts,
and the `CLAUDE.md` instruction layer that makes an agent actually use them.

Notes before you run it:

- It is **phased on purpose**. Run it as one prompt and let the agent work through
  the phases, or paste one phase at a time for tighter control. Phase 0 must run first.
- Everything in `{{double braces}}` is discovered by the agent in Phase 0 — do not
  pre-fill unless you want to override its findings.
- Expect it to take several sessions on a large repo. Phases 1–2 (docs) are the long
  pole; Phases 3–9 are mostly mechanical once Phase 0 is right.

---

```markdown
# TASK: Bootstrap an agent-readable knowledge layer for this repository

You are setting up the documentation + agent-tooling layer for this repo. The goal is
a system where a coding agent answers structural questions from **indexes and
checklists** instead of grepping blind, and where a human onboarding gets the same
material. Build it in the phases below, in order. Do not skip Phase 0.

## Ground rules (apply to every phase)

1. **Never invent facts.** Every path, class name, command, and count you write must
   be verified with Read/Grep/Glob in this session. If you cannot verify something,
   write `TODO(verify): <question>` instead of a plausible guess.
2. **No orphans.** Every file you create is linked from a `README.md` index in its own
   directory, and that README is linked from its parent.
3. **Link, don't duplicate.** Each fact lives in exactly one file. Everything else
   references it by relative path.
4. **Size caps are hard.** Target <500 lines per doc, 800 absolute; runbooks <=60 lines;
   any `CLAUDE.md` <300 lines. Split rather than exceed.
5. **Terse over complete.** Bullets and tables, not prose. Lead with the action.
6. Stamp `**Last Updated**: YYYY-MM-DD` on docs you actually author. Never mass-stamp
   files you only touched incidentally.
7. Commit in phase-sized increments so each phase is reviewable on its own.

## Phase 0 — Discover the repo (no files written yet)

Investigate and then report back a short findings block. Do not proceed to Phase 1
until I have seen it.

Determine:

- **Stack + language(s)**, build tool, test command, and how the app runs.
- **Project/module layout** — the top-level units and what each is for.
- **The dominant recurring patterns** — the 3–6 shapes that repeat across the codebase
  (e.g. a service/manager layer, an entity base class, a component base class, a
  handler convention). For each, name a canonical example file.
- **Every entry point class** — HTTP routes, RPC services, message/queue consumers,
  scheduled jobs, long-running workers, websocket/hub endpoints, CLI commands.
- **Naming conventions** actually in use (prefixes, suffixes, folder-per-domain, etc.).
- **Generated or vendored trees to exclude** — build output, dependency dirs, generated
  migrations/clients, proof-of-concept or orphaned projects, agent worktree checkouts.
- **The 20 largest source files** by line count.
- Existing docs, if any, and whether they are accurate or stale.

Output as:

```
Stack: ...
Modules: <name> — <purpose>  (one line each)
Patterns: <PatternName> — <shape> — canonical example: <path>
Entry points: <kind> — <count> — where they live
Naming: ...
Exclusions: ...
Largest files: <path> (<lines>)  x20
Existing docs: ...
Open questions for the human: ...
```

Substitute your Phase 0 findings wherever the later phases say `{{...}}`.

## Phase 1 — `docs/` skeleton and authoring standard

Create `docs/` with one subdirectory per document *type*, only for types this repo
actually needs:

| Dir | Contents |
|---|---|
| `architecture/` | system overview, data flow, deployment, per-module architecture |
| `adr/` | `ADR-###-kebab-title.md`, one decision per file, with a Status field |
| `api/` | one doc per public surface (endpoint group / RPC service) |
| `components/` | UI component docs, if the repo has a UI |
| `data-models/` | entity/schema docs, one per domain area |
| `services/` | service-layer docs, one per service |
| `conventions/` | positive-form rules ("do X because Y") |
| `gotchas/` | traps ("don't do X because Y") — Phase 2 |
| `guides/` | learning walkthroughs (quick-start, local setup, per-area how-to) |
| `glossary/` | domain + technical terms; split by category if >1 page |
| `dependencies/` | third-party deps and why each is there |
| `security/` | auth, authorization, data protection, secret handling |
| `qa/` | test strategy, defect triage, severity definitions |
| `templates/` | authoring templates (start with the ADR template) |

Then author these three meta-docs:

- `docs/README.md` — the master index. Entry point for humans and agents. Table per
  section with a "read this when…" column.
- `docs/DOCUMENTATION_GUIDE.md` — philosophy, the tree above, and a DO/DON'T list.
- `docs/MAINTENANCE.md` — per-doc-type owner, update trigger, review cadence; the
  quality metrics table; the PR checklist for docs; the red-flag list. Mark any
  cadence that is aspirational as **RECOMMENDED, NOT YET ENFORCED** — do not imply a
  process exists when it does not.
- `docs/conventions/documentation-standards.md` — file naming (kebab-case), required
  document structure, breadcrumb format
  (`> **Navigation**: [Home](../README.md) | [Section](README.md) | Page`), size
  limits, relative-link rule, and the required trailing "Related Documentation" section.

Every directory gets a `README.md` index table. Populate the type directories with
real content for the patterns and entry points found in Phase 0 — one doc per
concrete thing, not one doc per directory.

## Phase 2 — Conventions and gotchas

For each pattern from Phase 0, write `docs/conventions/<pattern>.md`:

- What the pattern is, when to use it, when **not** to.
- The canonical example, quoted from real code with its file path.
- Required members / signature shape.
- The decision rule when several variants exist (e.g. which base class to pick).

Also write, if applicable: `naming.md`, `coding-standards.md`, `async-patterns.md`,
`anti-patterns.md`, `testing-guidelines.md`.

Then `docs/gotchas/` — one file per trap, each with exactly these sections:
**Symptom** (what the developer sees), **Cause**, **Fix**, **How to detect it**
(a grep or a check). Seed it from: steps that are easy to forget in multi-file
registrations, flags that silently pick the wrong target, ordering-sensitive
declarations, config that fails silently at runtime. Index them in
`docs/gotchas/README.md` with a one-line "trap" column. State the boundary
explicitly: gotcha = "don't do X"; convention = "do X"; runbook = ordered checklist.

## Phase 3 — Machine-readable indexes + generator

Create `.claude/indexes/` containing `build-indexes.py` (Python 3 stdlib only) plus
its generated JSON outputs and a `README.md`.

Generate one index per structural question an agent asks repeatedly. Derive the set
from Phase 0; the ONEflight set is the reference shape:

| Index | Shape | Answers |
|---|---|---|
| `<pattern>.json` | `{ Name: { file, base, <owned-type> } }` | "Where is X, what does it own, what does it extend?" |
| `<type>.json` | `{ Name: { declarations[], owners[] } }` | "Where is this type declared, who owns it?" |
| `endpoints.json` | `{ <kind>: [ {…} ] }` per entry-point kind | "What runs when this endpoint fires?" |
| `components.json` | `{ Name: { file, callers[], caller_count, callers_truncated } }` | "Who uses this component?" |
| `file-weights.json` | `{ path: { lines, recommend } }` | "Safe to Read in full, or grep-only?" |
| `summary.json` | `{ generated_utc, counts, exclude_dir_names, self_test } ` | Freshness + sanity |

Hard requirements on the script — these each exist because of a real failure:

1. **Locate the repo root from `__file__`**, not the cwd. Runnable from anywhere.
2. **Atomic writes** — temp file in the output dir, then `os.replace`.
3. **Deterministic output** — `json.dump(..., indent=2, sort_keys=True)`. Filesystem
   walk order differs per machine and produces enormous spurious diffs otherwise.
   The one exception is `file-weights.json`, which is intentionally ordered by line
   count descending with path as tiebreaker.
4. **A single hard-coded exclusion set** applied by directory *name* on every walk:
   build output, dependency dirs, generated code, POC/orphan projects, **and any
   `worktrees` or `.claude` directory**. Agent worktrees are full repo copies and
   will multiply every count.
5. **Self-test**: assert each index has >=1 entry; print a PASS/FAIL table; `exit 1`
   if any fail. An empty index is a broken regex, never an empty codebase.
6. **`file-weights.json` thresholds**: include at >=500 lines, `recommend` is
   `"split or grep"` at 500–999 and `"grep only"` at >=1000.
7. Cap any unbounded list (e.g. callers at 25) and emit a `*_truncated` boolean —
   never truncate silently.
8. Progress to stderr, data to files.

`.claude/indexes/README.md` must state: the outputs are **generated, do not
hand-edit**; the regen command; when to regen (after source changes affecting the
indexed shapes, or a docs restructure); and a **Known limits** section listing every
place the parsing is approximate (which folders are scanned, what look-ahead is used,
which cases legitimately yield `null`). The limits section is what stops a future
agent from filing an accurate `null` as a bug.

Run the script. Report the counts and the self-test table.

## Phase 4 — Runbooks

Create `.claude/runbooks/`, one file per task that a developer performs repeatedly
and can get wrong. Derive the list from Phase 0 — typically "add a <pattern>" for
each pattern, plus schema migration, plus each entry-point kind.

Format, strictly:

- Title line: `# Runbook — <task>`, then one line on scope and when to use a sibling
  runbook instead.
- `## Steps` — numbered, **one action per step**, in dependency order. Bold the verb.
  Every step that has a trap links the relevant `docs/gotchas/*.md` by relative path.
  Every step that needs background links the convention or guide. **No prose
  explanations inline** — the link is the explanation.
- `## Verify` — the exact commands and greps that prove the steps worked.
- `## Related` — narrative docs.
- <=60 lines total.

`.claude/runbooks/README.md`: the table of runbooks with a "when to run it" column,
the style rules above, the runbook-vs-guide-vs-convention-vs-gotcha boundary, and a
"how to add a new runbook" section whose first rule is *only add a task you have
performed at least twice*.

## Phase 5 — Search cookbook

Create `.claude/cookbooks/ripgrep.md`: categorized, **pre-validated** `rg` recipes.

- A `## Setup` section listing the flags and globs this repo always needs
  (type filters, generated-code excludes).
- Categories mirroring the questions agents actually ask: pattern/class definitions;
  entry points; "who uses this?"; DI / config / wiring; data model + schema;
  UI markup; convention and discipline checks; testing; file weights; caching;
  external integrations; error handling.
- Per recipe: a `###` headline, a one-line **Returns:** note, then the command in a
  fenced block. Keep path scopes narrow and intentional.
- Where a recipe has a blind spot, say so in bold in the recipe itself.
- **Promotion rule, stated in the file:** a recipe is only added after you have run it
  successfully at least twice. Add it the second time you write it.

Run every recipe before committing it. Delete any that returns nothing.

## Phase 6 — Sub-agent definitions

Create `.claude/agents/`, one Markdown file per sub-agent, with YAML frontmatter:

```yaml
---
name: kebab-case-name
description: What it does + "Use when the user asks …" with 2–3 verbatim example asks.
tools: Read, Grep, Glob      # least privilege; read-only unless it must write
---
```

Body sections: **Input** (what it accepts), **Chain of resolution** (numbered — and
step 1 is always "consult `.claude/indexes/<x>.json`", source files only after),
**Output format** (a literal template block, so callers can rely on the shape),
**Constraints** (scope limits, "grep don't Read files listed grep-only in
`file-weights.json`", "never synthesize behavior from a name — read the body"), and
**When to decline** (the cases where it should ask for a narrower target instead of
guessing).

Build one tracer per entry-point kind, one explorer per major pattern, a
feature-scout that maps a feature keyword to its candidate file set across all
layers, and a code-reviewer that reviews against `docs/conventions/`.

## Phase 7 — Commands and skills

Create `.claude/commands/<name>.md` for each repeatable multi-step workflow this repo
has — at minimum: run the review, check pipeline/CI status, plan a work item,
save/restore working context, end-of-session wrap-up.

Where a workflow needs bundled scripts, references, or templates, make it a skill
instead: `.claude/skills/<name>/SKILL.md` with `name` + `description` frontmatter,
plus `scripts/`, `references/`, and `assets/` subdirectories as needed. `SKILL.md`
stays short and dispatches to those files rather than inlining them.

## Phase 8 — Documentation validation scripts

Under `.claude/skills/documentation-expert/scripts/`, create checks that each exit
non-zero on violation and accept `--docs-dir` / `--src-dir` overrides:

| Script | Checks |
|---|---|
| `check-doc-size.sh` | >800 lines = violation, 500–800 = warning |
| `check-doc-links.sh` | broken relative links; orphaned docs (nothing links to them) |
| `check-doc-freshness.py` | missing or >90-day-old `Last Updated` stamp |
| `check-adr-status.sh` | every ADR has a valid Status; superseded ADRs point somewhere real |
| `verify-code-examples.sh` | fenced code blocks reference paths/symbols that exist |
| `generate-doc-metrics.sh` | aggregate counts; `--output json`, `--quiet`, `--no-history` |
| `run-all-checks.sh` | wrapper: runs the metrics generator, writes `scripts/reports/gap-report.json` (gitignored), prints a `jq` summary, propagates the exit code |

Wire `run-all-checks.sh` into CI as a **blocking** check, not advisory.

## Phase 9 — Hooks and settings

`.claude/settings.json`:

- `permissions.allow` — the read-only and routine commands that should never prompt
  (build, test, the repo's CLI read verbs).
- `permissions.ask` — the genuinely consequential ones: `git push`, schema/database
  commands, edits to project files, edits to `appsettings*`/env config, publish.
- `context.excludePatterns` — build output, dependency dirs, minified assets.
- `hooks.PostToolUse` on `Edit` and `Write` → a formatter hook that formats only the
  file just touched (read the path from the hook payload; never format the tree).
- `hooks.Stop` → a health-check hook. It must be **fast, network-free, and silent
  unless something needs attention**: memory file approaching its size limit,
  unpromoted gotchas, too many open todos. Print one line per alert.

Both hooks go in `.claude/hooks/` as standalone scripts with a header comment stating
when they run and what they assume.

## Phase 10 — The instruction layer (`CLAUDE.md`)

This is what makes the previous nine phases get used. Write a root `CLAUDE.md`
containing, in this order:

1. **A mandatory-first-action decision tree.** A short, unmissable block at the top:
   what to check before responding at all (e.g. branch-name work-item detection →
   launch the planner in a sub-agent before writing code), and the rule that any
   request touching patterns or architecture reads the relevant convention doc first.
2. **Memory-file protocol** — where per-task state is written, one table row per file
   (path, purpose, when to update), and the rule that cross-task recurring patterns
   get promoted into `CLAUDE.md` rather than left in per-task notes.
3. **The indexes section** — the table of index files with a "use when…" column, how
   to query them cheaply, worked lookup examples, and the fact that they are generated
   with a stated regen command.
4. **The runbooks section** — the task→runbook table and the workflow: match the task,
   read the runbook *before* writing code, follow steps in order without batching,
   run the Verify section before reporting done.
5. **The cookbooks section** — plus the explicit **lookup order** for any structural
   question: *indexes → cookbook → custom grep*, and the rule that a custom recipe
   used twice gets promoted to the cookbook.
6. **The house rules that reviewers actually enforce here.** Whatever they are for
   this repo, state them as MANDATORY with the naming convention, positive and
   negative examples, and the shared-component variant. (In ONEflight this is the
   `data-testid` rule on every interactive and state-bearing element.)
7. **Pattern table** — pattern → doc link → canonical example.
8. **Project guides table** — one row per module linking its own `CLAUDE.md`.
9. **Domain notes** — terms whose local meaning differs from their common meaning.

Then a per-module `CLAUDE.md` for each module: purpose, when to work here versus
elsewhere, annotated structure, common commands, step-by-step patterns for that
module's frequent tasks, troubleshooting, cross-references. <300 lines each.

Also write `.claude/README.md` explaining the `.claude/` layout, the boundary with
`docs/` (`docs/` = documentation for humans and agents; `.claude/` = assets the agent
runtime consumes directly), and what must **not** go there.

## Acceptance criteria — verify all of these before reporting done

- [ ] `python .claude/indexes/build-indexes.py` exits 0 with every self-test PASS, and
      the counts match spot-checks done by hand with `rg`.
- [ ] `bash .claude/skills/documentation-expert/scripts/run-all-checks.sh` reports
      0 broken links, 0 orphans, 0 size violations.
- [ ] Every `.md` you created is reachable by relative links from `docs/README.md` or
      `.claude/README.md`.
- [ ] Every runbook is <=60 lines and has a Verify section.
- [ ] Every cookbook recipe has been executed and returns >0 results.
- [ ] No file contains an unresolved `TODO(verify)` you could have resolved yourself.
- [ ] Root `CLAUDE.md` <300 lines; each module `CLAUDE.md` <300 lines.
- [ ] A fresh-context check: ask a sub-agent "where is <pattern X> and who owns it?"
      with no context beyond the repo, and confirm it answers from the indexes without
      grepping the source tree.

## Anti-goals — do not do these

- Do not generate a doc per source file. Docs are per *concept*, per *pattern*, per
  *entry point*.
- Do not write aspirational process. If a cadence or gate is not real, label it.
- Do not duplicate narrative explanation into runbooks or cookbooks. Link.
- Do not hand-edit generated index JSON. Fix the generator and regenerate.
- Do not add a runbook or recipe for a task performed once.
- Do not stamp `Last Updated` on files whose content you did not change.
```

---

## Maintaining what it builds

Once the scaffold exists, the recurring work is:

| Trigger | Action |
|---|---|
| Source change affecting an indexed shape | `python .claude/indexes/build-indexes.py`, commit the JSON with the change |
| New pattern adopted by the team | Convention doc + runbook + cookbook recipe + `CLAUDE.md` pattern row |
| A trap hit twice | New `docs/gotchas/*.md`, linked from the step in the runbook that causes it |
| A grep written twice | Promote to `.claude/cookbooks/ripgrep.md` |
| Per-task note that recurs across tasks | Promote into the relevant `CLAUDE.md` |
| Doc restructure | Regen indexes, then `run-all-checks.sh` |
