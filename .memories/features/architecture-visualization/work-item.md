# Architecture visualization + Search Everywhere

Source plan: `C:/Users/AnthonyJames/.claude/plans/to-better-understand-the-compressed-pascal.md`

## The problem

`code-basics` can run, test and review a repository but cannot **explain** one.
`Project` (`crates/core/src/model.rs:21-46`) carries no dependency edges,
`<ProjectReference>` is parsed nowhere, and no source file is ever read for
meaning.

Two requests turned out to need the same missing artifact — a workspace symbol
index — so they are one work item:

1. Diagrams of how a project hangs together, at a user-chosen level of detail.
2. Rider's Ctrl+N "Search Everywhere".

## Decisions already made

| Question | Decision |
|---|---|
| Where diagrams come from | Hybrid: structural levels parsed from disk (*derived*), call-flow and data-model authored by the user's coding agent (*inferred*). The label is visible on the diagram. |
| Ecosystems | .NET and JS/TS; deterministic half covers structural levels only |
| Levels | (1) solution/project map, (2) runtime component map, (3) call flow for one method, (4) data/type model |
| Persistence | mermaid markdown under `.code-basics/diagrams/`, CodeMirror editor beside a live preview |
| Sequencing | symbol index → palette → derived diagrams → agent-authored diagrams |
| Index accuracy | heuristic declaration scanner lifted from `git/grouping.rs`. **No new Rust dependency.** |

## Acceptance criteria

- **Symbol index** in `cb-core`: shared source walker, declaration heuristic
  lifted out of `git/`, fuzzy matcher, index, cache, search entry point. Pure,
  headlessly testable, no new Rust dep.
- **Palette**: double-Shift (All), Ctrl+N (Symbols), Ctrl+Shift+N (Files),
  Ctrl+Shift+A (Actions). Opening a workspace stays instant — the index builds
  on a background thread and search answers from `files` until ready. Saving a
  file re-indexes it.
- **Derived diagrams**: `<ProjectReference>` edges, node sibling deps,
  `workspaces` globs, solution containment, deterministic mermaid emission. A
  reference pointing outside the workspace becomes an `External` node **plus** a
  warning — never a silently dropped arrow.
- **Level 2 is graded**: a node or edge exists only on a HIGH signal (declared
  facts in manifests). MEDIUM may only enrich an existing node. Every skipped
  candidate is counted into `ArchGraph.warnings` and rendered under the diagram.
  Hard rule with a test: a connection-string *value* never reaches the graph.
- **Agent-authored diagrams**: write a prompt file by default; shelling out is
  opt-in via `diagrams.agentCommand`. Validation rejects `click … call`,
  `%%{init:}%%`, script/iframe/javascript:/on…=/href, unbalanced subgraphs,
  undeclared edge endpoints. On failure the file is still saved and still shown.
- **Architecture tab**: id `architecture`, label `Architecture` — deliberately
  matching, unlike `inspect`/"Objects". Click-to-navigate uses a side table of
  `{id, kind, path, line}` returned by cb-core, never mermaid's `click … call`.
- **CSP is never loosened.** `'unsafe-eval'` is not added under any outcome.

## Governing rule

A wrong label is much worse than no label
(`git/attribution.rs:16`, `git/grouping.rs:19-27`, `inspect/mod.rs:42-53`).
Every threshold abstains rather than guesses. A diagram asserting "A calls B" is
a much stronger claim than a card titled with a function name, so signals are
graded and weak ones may only enrich a node, never create one.
