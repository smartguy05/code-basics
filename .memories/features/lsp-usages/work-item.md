# Find usages + go to definition, over real language servers

## The ask

Rider's method-tracing behaviour, in this app:

1. An inline row above a method declaration showing how many places it is used,
   which drops down a list of those places; clicking one opens that file at that
   line.
2. Middle-click a method to go to its definition — the interface, base or
   virtual member when there is one.

Plus a second, unrelated small change: the Run tab's console panel must
collapse and restore so it can be put out of the way (see
`.memories/features/console-panel-collapse/`).

## Acceptance

- Counts and locations are **semantically correct**, not textual. `Order.Total`
  and `Invoice.Total` must not be conflated.
- Languages in the first cut: **C#, TypeScript/JavaScript, Rust, Python**.
- C# is served by `Microsoft.CodeAnalysis.LanguageServer` (the Roslyn server the
  VS Code C# extension ships), **located on disk, never bundled**.
- Middle-click: exactly one target ⇒ jump. More than one ⇒ a picker grouped
  Declarations / Implementations. **Never a silent pick.**
- When no server is configured, or one is still loading, the UI says so plainly
  and shows **no count at all**. No textual estimate, ever — a number that later
  changes is a wrong answer.

## Decisions taken by the user

Recorded because each closed off a cheaper option:

- Rejected: extending the existing text symbol index with an occurrence scan.
- Rejected: tree-sitter.
- Rejected: showing an approximate count first and replacing it.

## Status

Console panel collapse: **done**. LSP work: Phases 1, 2 and 3 **done and
integrated** — `lsp/{framing,jsonrpc,uri,positions,protocol,registry,settings,
transport,client}.rs`, the scripted `cb-fake-lsp` binary, the `WorkspaceConfig.lsp`
block, `lsp-logs/` in `IGNORED`, and `process::{resolve_program, kill_tree,
configure_process_group}` exported. A real process is now spawned, spoken to and
killed as a tree; nothing above `cb-core` uses it yet.

`cb-core` lib tests 1575 → 1808 → **1813** after the review pass, plus the two
new integration targets (`lsp_transport` 24, `lsp_client` 30). Whole gate green:
clippy back to the two documented baseline warnings, coverage 88.57%.

Phase 4 is **done, reviewed and integrated**: `lsp/{model,results,documents,session}.rs`
with their `*_tests.rs`, the `crates/core/tests/lsp_session.rs` target, `AppState.lsp`
plus the generation-guarded `begin/record/take_lsp_session`, the seven `lsp_*`
commands, and the ten mirrored types and seven wrappers in `src/ipc/`. Three
read-only review lenses then went over it and a fourth pass acted on them — six
real defects fixed, each with a test that failed first, and two findings rejected
as wrong (see `notes.md`).

Counts, all measured this session: `cb-core` lib **1901**, integration
49 / 2 (1 ignored) / 30 / **16** (`lsp_session`) / 24 / 11; `cb-app` **40**;
`pnpm test` 494. Clippy at exactly the two documented baseline warnings, fmt
clean, `docs:check` and `docs:index` clean, coverage **88.48%** lines.

Phase 5 — **the UI** — is now done, reviewed and integrated: `usagesLogic.ts`,
`usagesExtension.ts` (the repo's first `WidgetType`), `FileEditor.tsx` as the
whole language-server client, the usages dropdown and the middle-click picker,
`lspStatusLogic.ts` + `LspStatus.tsx` in the titlebar, and the `App`/`RunView`
wiring over the existing `requestToken`. Three read-only lenses reviewed it and a
fourth pass acted on them: fourteen findings confirmed and fixed, one rejected
with a reason, one reviewer premise found factually wrong (see `notes.md`).

Counts after that pass, all measured: `pnpm test` **607** in 23 files, coverage
**99.71%** lines over the logic modules, `pnpm build` clean. Rust untouched and
verified unchanged — clippy at exactly the two documented baseline warnings,
`cb-core` 1901 / 49 / 2 (1 ignored) / 30 / 16 / 24 / 11, `cb-app` 40.

**What is left is verification in the running app**, not code: nothing in this
feature's CodeMirror or React layer can be unit-tested in a node environment. The
steps to see it working are in `notes.md`.

## Status after the Phase 5 fix round

Verification in the running app found the feature **wrong** despite a green gate,
and the fix round is complete: the one-shot readiness signal is sticky state, the
90 s ceiling is a bound on patience rather than a verdict, the caveat reaches both
the answer and its own `ServerStatus.caveat` field, and no caveated answer renders
as authoritative anywhere. Root cause, regression tests and — the durable part —
why 1901 + 607 passing tests missed it are in `notes.md`.

Counts measured at the end of this round: `cb-core` lib **1917**, integration
49 / 2 (1 ignored) / 32 (`lsp_client`) / 16 (`lsp_session`) / 26 (`lsp_transport`)
/ 11; `cb-app` **40**; `pnpm test` **627** in 23 files; coverage **99.72%** lines;
clippy at exactly the two documented baseline warnings; fmt, `docs:check` and
`docs:index` clean. The baselines quoted into this round's instructions (1901 /
30 / 24 / 607) were already stale — the fourth phase running in a row where that
was true. Count the targets, never the remembered number.

**Next: re-verify in the running app.** The steps are in `notes.md`; the row above
`TryGetElements` in `sidecar/inspector/Collections.cs` must read "1 usage".
