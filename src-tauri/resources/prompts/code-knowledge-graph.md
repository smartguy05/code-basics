Build me a code knowledge graph for this repo: a local development tool that joins
GIT HISTORY with the project's PROSE, so I can ask *why* the code is the way it is
rather than only reading *what* it does. Reading HEAD tells me what a guard does; it
never tells me about the outage that put it there.

This is a dev/planning tool ONLY. It must not ship with the product, add runtime
dependencies, or touch application code. Python 3 stdlib + SQLite, no services, no
network. Put it in `tools/kg/` (adjust if this repo has an established tooling dir).

## Step 1 — Measure this repo BEFORE designing anything. Report findings first.

Do not skip this. Every important decision below depends on numbers, and the answers
differ per repo. Run the commands and tell me what you find:

1. Scale and shape: `git rev-list --count HEAD`, date range, `git ls-files | wc -l`,
   languages present, and whether there are sibling/related repos I should index
   together (ask me).
2. Commit message quality, and whether it is CONSTANT OR BIMODAL over time. Sample
   commits across the whole history and measure what fraction have a body with real
   reasoning versus a bare subject. In many repos the habit started at some point —
   find that inflection date. This determines whether "why" queries can be trusted.
3. Conventions: are these Conventional Commits (`fix(scope):`)? Are there ticket ids
   (`ABC-123`)? How are PRs merged — merge commits (`Merge pull request #N from
   owner/branch`), SQUASH (`subject (#123)`, no branch name), or rebase (no marker at
   all)? This decides what your durable join key can be.
4. Renames and restructures: `git log --diff-filter=R --find-renames --name-status`.
   Has the repo been renamed or reorganised? Any file moved between directories will
   have its history orphaned unless you handle this.
5. CHURN, and this is the important one: `git log --pretty=format:'' --name-only |
   sort | uniq -c | sort -rn | head -40`. Look for files that top the list but are
   NOT source code — changelogs, notes files, lockfiles, generated snapshots, build
   artifacts that are tracked. Also find generated code (migrations, protobuf, .d.ts)
   and count what fraction of the codebase it is.
6. PROSE sources — where does this project write down *why*? Look for any of:
   `docs/`, ADR directories, `CHANGELOG.md`, RFCs, design docs, `.memories/`-style
   agent notes, a doc index/catalog with curated doc→code mappings, security or
   architecture review documents with finding ids, issue/PR templates. Also check
   whether `gh` is installed and authenticated (PR bodies and review comments are
   prose worth indexing).
7. JOIN KEYS: figure out what actually links prose to git here. Do notes cite commit
   shas, PR numbers, ticket ids, branch names, or `File.ext:LINE` anchors? Do ticket
   ids appear in both commit subjects and docs? This is the whole basis of the graph —
   if you can't find real join keys, tell me before building.

Then profile: time a full `git log --numstat`, and time `git blame` on a sample of
files scaled to the repo's file count. Blame usually dominates. Report the numbers.

## Step 2 — Non-negotiable design decisions

These are the ones that are wrong-by-default and expensive to discover. Implement all
of them, with a test for each.

**Rename resolution.** Use `git log --numstat -M` and a union-find over rename pairs
so every historical path collapses onto ONE canonical file node; the canonical path is
the one alive at HEAD. Parse git's compressed rename form (`pfx{a => b}sfx` and
`a => b`). Use `-M` only, NOT `-C`: copy detection will merge two distinct live files
into one node. Without this, a moved file silently reports a fraction of its history.

**Two DIFFERENT exclusion lists.** Conflating them poisons every ranking:
  - *narrative*: prose files — parsed as a source of rationale, but excluded from all
    churn / co-change / risk metrics. Derive this from your Step 1 churn measurement:
    any high-churn bookkeeping file (changelog, notes, todo list) will otherwise pair
    with everything and dominate every hotspot list.
  - *excluded*: generated, vendored, binary, lockfiles, build output — not nodes at
    all. Again derive from measurement, not guesswork.

**Evidence strength on every returned fact**, surfaced in the output:
  - strong: a commit body, a prose note, a curated doc, a blame line
  - medium: co-change — statistical; ALWAYS print the support count and lift, never a
    bare assertion
  - heuristic: SZZ bug links, staleness — a fix touching a line does not prove that
    line caused the bug
  - weak: a commit with no real body
  The tool must never present medium/heuristic findings as established fact.

**Era awareness.** Score each commit on how much rationale its body carries. If the
history is bimodal (Step 1), a query about an old file MUST report "no commit on this
file carries a real explanation" rather than presenting a bare subject like "test
fixes" as the reason. Silence is a correct answer; invention is not.

**Rank explanations by surviving code.** Use `git blame` at HEAD to count how many
lines each commit still owns in the file, and factor that into ranking. A well-argued
commit whose code was entirely overwritten explains history, not the current state.

**Drift detection uses `git merge-base --is-ancestor`, NOT reachability.** After
`git commit --amend` the old commit still exists as a dangling object, so
`rev-list old..new` succeeds and you will wrongly conclude it was a fast-forward and
reuse caches built from rewritten history. Get this wrong and the tool lies silently.

## Step 3 — What to build

Modules, roughly: `schema.sql`, `store.py`, `config.json` + `config.py`,
`extract_git.py`, `extract_prose.py`, `analyze.py`, `freshness.py`, `query.py`,
`kg.py` (CLI), `build.py`, and `tests/`.

Nodes: commits, files (+ historical aliases), work items (branch / PR / ticket),
prose entries, docs, findings, people. Edges: commit→file touches, commit→work item,
file↔file co-change (support / confidence / lift), SZZ bug links, blame lines,
doc→code, prose→file:line.

Commands, each supporting `--json`:
  why <file>[:line]     the dossier — rationale commits, hazards, docs, coupling
  fence <file>:<a>-<b>  was this range written by a fix? RUN BEFORE DELETING CODE
  impact <file>         co-change partners; flag cross-language and cross-repo pairs
  risk                  hotspots, each WITH THE REASON it scored
  timeline <target>     chronological decision narrative
  gotchas <file|topic>  hazards recorded in prose
  trace <ticket>        finding → branch → PR → commits → files → prose
  stale                 prose anchors that no longer match the code they cite
  stats                 graph health, including rationale coverage over time

Two lookup details that matter: resolve targets by full path, path suffix, OR bare
basename (refusing ambiguous matches rather than guessing); and when searching prose
for a file, also search its bare type/module name — notes say `TenantContext`, not
`src/data/TenantContext.cs`.

## Step 4 — Upkeep. Build this in; do not leave it to my memory.

A stale graph is indistinguishable from a fresh one — it answers old facts
confidently. So:

- Store each repo's indexed HEAD. EVERY query compares it against the real HEAD and
  refreshes before answering, reporting that on stderr. Offer `--stale-ok` to skip.
- Make refresh cheap enough that doing it automatically is reasonable. Re-extract from
  scratch (cheap, and keeps rename resolution correct); cache only the expensive
  derived layers, using keys that are provably safe:
    * blame → key on the commit that LAST TOUCHED that file. A file's blame at HEAD
      depends only on commits touching it, so any change misses the cache.
    * SZZ  → key on the fix commit's sha. A commit is immutable, so its links can
      never change once computed.
  Profile first (Step 1) and target the layers that actually dominate.
- Any history rewrite must fall back to a full rebuild. Provide a `--full` flag.
- The database is a DERIVED CACHE: gitignore it, never commit it, git stays the source
  of truth.

## Step 5 — Documentation and mandate

Documenting it is not enough; nobody will run it. Add a directive section near the TOP
of this repo's agent instructions file (CLAUDE.md / AGENTS.md / .cursorrules —
match whatever exists and its house style and emphasis conventions) containing:
  - a when → run → why trigger table (before editing an unfamiliar file; before
    deleting or "simplifying" ANY code; before calling a change complete; when a line
    looks arbitrary; when starting in a new subsystem; when working a ticket)
  - the hard rule: never delete or rewrite a guard, early-return, serialization
    annotation, lock, or null-check without running `fence` on it first
  - the instruction to trust the evidence marks and never report statistical or
    heuristic findings as fact
  - a note that the graph self-refreshes, so nobody needs to remember to rebuild it
Also write a README for the tool explaining WHY each design decision exists, and
update whatever doc index this repo has.

## Step 6 — Verification. Show me evidence, not claims.

- Tests: fixture tests that build throwaway git repos in a temp dir and assert exactly;
  plus an EQUIVALENCE TEST proving an incremental refresh produces a byte-identical
  graph to a full rebuild, after a commit, a fix, a rename, an add+delete, and an
  amend. That test is what makes caching safe to trust.
- A few corpus tests against this real repo asserting SUPERSETS (never exact equality)
  so they survive new history.
- Then prove it works on real history: pick 3-4 things you know are true about this
  repo from Step 1 — a past incident, a file that moved, a cross-language coupling, a
  ticket that spans commits — and show me the tool surfacing each one.
- Confirm the top hotspot list contains NO generated files, bookkeeping files, or
  lockfiles. If it does, your exclusion lists are wrong.
- Run this repo's own test suite to confirm nothing else broke.

Work test-first, show me the Step 1 measurements before you start building, and tell
me if anything here doesn't fit this repo rather than forcing it.