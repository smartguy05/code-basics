# Completed: "Verify claims — evidence" showed counts, not evidence

## Symptom (reported from a screenshot)

A 21-second before/after run finished and the whole report read:

```
2 outcomes compared · 1 delta · 0 attributed · 1 unattributed · 0 abstained
Tests: before 186 passed / 0 failed → after 186 passed / 0 failed
UNATTRIBUTED DIFFERENCES
console: 2 added, 2 removed
```

No way to see *which* lines, why it pinned to no card, or how much the run
trusted it.

## Root cause

Not a backend gap — `cb_core::behavioral` carries all of it over IPC and
`src/ipc/types.ts` mirrors it faithfully:

* `ConsoleDelta.added_lines` / `removed_lines` / `normalized` / `confidence`
* `HttpDelta.header_changes` / `body` (`BodyDelta` +/- lines) — **rendered
  nowhere at all before this**
* `TestDelta.cases` (`CaseDelta` with `base`/`work`/`transition`)

The loss was entirely in the frontend: `behavioralPanelLogic.deltaLine` read
only `.length`, `BehavioralReportView` rendered one line per delta, and
`claimVerifyLogic.describeDelta` handed the *verifying agent* the same counts.

## Fix (frontend only — no Rust, no IPC contract change)

* `behavioralPanelLogic.ts`: `deltaDetail`, `testCaseRows`, `deltaConfidenceNote`,
  `unattributedReason`, `EVIDENCE_LINE_CAP` (20/side, `+N more` remainder).
* `BehavioralPanel.tsx`: Console / Evidence tabs (the console pane stays
  **mounted** when hidden — its `ResizeObserver` refits on show); auto-selects
  Evidence when the report lands; each delta is a click-to-expand row, opened by
  default when the report holds ≤ 3 deltas.
* `claimVerifyLogic.ts`: `describeDelta` reuses `deltaDetail`, so the panel and
  the agent prompt can never show different evidence; each unattributed entry
  gains a `why:` line.
* `styles.css`: `.behavioral-tabs`, `.behavioral-detail*`; `.behavioral-report`
  lost `max-height: 40%` (it owns the tab pane now).

## Why "0 attributed" is not a bug (now stated in the UI)

`behavioral/attribute.rs::candidate_paths`: HTTP deltas return no candidate
files **by design**, and `compare.rs:131` leaves every `CaseDelta.files_hint`
empty (documented in `prepare.rs`), so **test and HTTP deltas can never
attribute**. Console attributes only when its changed lines name the files of
exactly one card. `unattributedReason` says which rule applied; its console
wording is true of both the zero-owner and the ambiguous case, since the report
alone cannot distinguish them.

## Gate

`pnpm test` / `pnpm typecheck` could **not** be run from the agent shell — the
pnpm junction wall (`ERR_MODULE_NOT_FOUND` for `@vitest/utils`,
`Test-Path node_modules/vitest/package.json` False). The new pure logic was
instead executed directly under `node --experimental-strip-types` against the
same expectations the vitest cases assert, and all passed. The vitest run itself
still needs to be done by the user.

## Docs updated (2026-08-28)

- `README.md` — the before/after bullet now says each difference expands to its evidence.
- `docs/getting-started/using-the-app.md` — Console/Evidence tabs, what expands, and that
  test/HTTP deltas never attach to a card by design.
- `docs/architecture/frontend.md` — new `- **BehavioralPanel**` bullet (tab strip, the
  stays-mounted console rule, `AUTO_EXPAND_LIMIT`, `EVIDENCE_LINE_CAP`, shared `deltaDetail`);
  the `behavioralPanelLogic.ts` tree entry lists the new exports.
- `CLAUDE.md` — the Intent-view sentence gained the panel's rules; the `behavioral/` core
  paragraph now records that HTTP and test deltas **always** abstain (`candidate_paths` returns
  an empty set for HTTP; `compare.rs` sets every `CaseDelta.files_hint` to `Vec::new()`), so
  console is the only kind that can attribute. That fact was re-derived from source during this
  work and is the reason "0 attributed" is not a bug.
- `docs/INDEX.md` regenerated with `node scripts/generate-index.mjs`; `node scripts/check-docs.mjs`
  passes (22 files, all under 500 lines, all links resolve).

Note: both scripts import only node builtins, so they run in this shell even though `pnpm`
cannot resolve through the `node_modules` junctions here.
