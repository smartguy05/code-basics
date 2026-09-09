# Completed — F2 rename

## 1.1 `positions::byte_offset` (2026-09-04)

The only whole-document position→byte arithmetic in the app. `positions.rs`
previously had per-line conversions only, which is the gap the plan identified.

Files: `crates/core/src/lsp/positions.rs`, `positions_tests.rs` (+12 tests).

Signature is `byte_offset(text: &str, line: u32, character: u32) -> usize` —
**scalars, not a `protocol::Position`**, because `protocol` already imports this
module and taking its types would invert the layering.

Clamps per the module's existing rule, with one rule carrying real weight: a
column past a line's end resolves to the end of that line's **content**, before
a CRLF and never between the `\r` and the `\n`. Resolving past the carriage
return would let a replacement land inside the terminator and leave a lone `\r`
mid-file. Pinned by `an_overrunning_column_on_a_crlf_line_stops_before_the_carriage_return`
and by `no_position_can_panic_the_offset_lookup`, which asserts every result is
a char boundary across a table of hostile inputs.

Result: `cargo test -p cb-core --lib lsp::positions` → 37 passed.

## 1.2 `crates/core/src/lsp/edits.rs` (2026-09-04)

The Range→text applier, which had no precedent anywhere in the tree
(`git/repo.rs::apply_patch` shells out to `git apply` and is unified-diff and
single-file; `intents/patchfmt.rs` only parses for the audit trail).

Files: `crates/core/src/lsp/edits.rs` (new), `edits_tests.rs` (new, 30 tests),
`lsp/mod.rs` (+`pub mod edits;`), `protocol.rs` (+`TextEdit`).

Shape: `plan(&[TextEdit]) -> Result<Vec<TextEdit>, EditError>` takes **no text**,
so every positional refusal happens before a file is read — which is what lets a
whole multi-file rename be refused having touched nothing. `apply(text, planned)`
then walks forward with a cursor rather than applying in reverse; the usual
reverse trick is shorter and hides the end-of-document case, which here falls out
of the loop tail. `replaced_texts` supplies the stale-mirror evidence.

`EditError` has four variants, each a case where two answers are equally
defensible and picking one silently produces a file the server did not describe:
`Overlapping`, `Backwards`, `OutOfDocument`, `AmbiguousInsertion`. An exact
duplicate (same range *and* same text) is the deliberate exception — collapsed,
because deduplicating changes nothing while applying it twice would write the
replacement twice. Two *different* texts for one range is a disagreement, not a
duplicate, and is refused.

Two bugs the tests caught before any caller existed — both recorded in
`notes.md`: `AmbiguousInsertion` was unreachable behind the equal-range branch,
and a refusal message must render 1-based lines for a reader looking at a gutter.

Result: `cargo test -p cb-core --lib lsp::edits` → 30 passed. Full
`cargo test -p cb-core` → 3031 lib tests + every integration suite green;
`cargo fmt --check` clean.

## 1.3 / 1.3b `protocol.rs` — methods, params, decoders, capability (2026-09-04)

Files: `crates/core/src/lsp/protocol.rs`, `protocol_tests.rs` (+37 tests).

**Methods**: `method::{RENAME, PREPARE_RENAME}`, asserted inside the existing
`every_method_constant_is_spelled_the_way_the_protocol_spells_it` rather than in
a test of their own, so the enumeration stays the one place a spelling lives.

**Params**: `RenameParams { text_document, position, new_name }`, camelCase so
`newName` goes on the wire. `prepareRename` reuses `TextDocumentPositionParams`
— one struct plus one method constant, exactly as the three gotos do. No
`PrepareRenameParams` type exists and none should.

**Decoders**: `decode_workspace_edit` reads `null` (an empty edit), `{}` (also
empty — every field of a `WorkspaceEdit` is optional), the legacy `changes`
map, `documentChanges` with `TextDocumentEdit` entries, an `AnnotatedTextEdit`
(the edit kept, the `annotationId` dropped by the module's ignore-unknown-fields
rule), and the resource operations. Both shapes present prefers
`documentChanges`. A key present but `null` is read as **absent** rather than as
an unreadable shape, so `{"documentChanges": null, "changes": {...}}` falls back
to the legacy map.

Two decisions the plan did not specify, each with a named test:

* **Several `documentChanges` entries naming one uri merge into one
  `DocumentEdits`.** Two entries for a file would otherwise be planned and
  applied separately, and the second would be computed against text the first
  had already changed. Merging is what lets `edits::plan` see the whole set for
  a file at once and refuse an overlap it would never otherwise be shown.
* **An operation `kind` this module does not know is still an operation.** A
  kind added to the protocol later is not an unreadable shape and is certainly
  not a text edit; dropping it would silently convert an answer this app must
  decline into one it would apply. Hence `ResourceOperation.kind` is the
  server's own `String` and not an enum.

`DecodeError` gained **no** variant, and a test says so by name: a resource
operation is a legible request this app declines, and the decline belongs in
`session.rs` where the sentence is written.

`documents` comes out ordered by uri via a `BTreeMap` accumulator, so neither a
`serde_json::Map`'s iteration order (a function of a dependency's feature flags)
nor a `documentChanges` array's order (the server's) can reach a caller. A
document with an **empty** edit list is kept, not pruned — the server named the
file, and pruning would make a caller counting documents disagree with one
counting edits.

`decode_prepare_rename` handles all four shapes. `null` becomes
`NotRenameable`, a real answer. `{"defaultBehavior": false}` means the same as
`true` (a statement about the *range*, not a refusal). The bare `{start, end}`
shape is the live path for C#.

**Capability**: `rename` (reusing `provides()`, so Roslyn's options-object
spelling counts) and `prepare_rename` (a new `prepare_provider()`, true only for
a literal `true` inside an options object). Two fields, and
`rename_provider_true_gives_rename_without_prepare_rename` is the test that says
why.

**The declaration**: `textDocument.rename = {dynamicRegistration: false,
prepareSupport: true}` and `workspace.workspaceEdit = {documentChanges: false,
resourceOperations: [], failureHandling: "abort"}`. `applyEdit: false` and
`transport::answer_for`'s `{"applied": false}` untouched, with a comment in
`initialize_params` explaining the apparent contradiction (applyEdit is a server
*push*; rename is a *pull*) so it is not "fixed". The whole-JSON pin was updated
in the same edit with a comment naming what is invited — nothing for rename, and
a *narrowing* for `workspaceEdit` — and
`the_client_never_declares_that_it_will_do_file_operations` now asserts
`resourceOperations: []`, `failureHandling: "abort"` and `applyEdit: false`
separately, so the next legitimate widening of that blob cannot carry the
guarantee away silently.

**Mutation-checked rather than trusted.** These tests all passed on their first
run, which proves nothing, so two mutations were applied and reverted: reversing
`documents_of`'s order, and preferring `changes` over `documentChanges`. Four
tests failed, naming exactly those two rules.

## 1.4 `client.rs` + 1.4b `fake_lsp.rs` (2026-09-04)

Files: `crates/core/src/lsp/client.rs`, `crates/core/src/bin/fake_lsp.rs`,
`crates/core/tests/lsp_client.rs` (+8 tests, +2 assertions).

`prepare_rename` and `rename` are shaped like `references`, written out in full
rather than through the private `goto` helper — that helper exists because three
requests share one body, and rename shares with nothing. Each: `require` first
(capability strings `"renameProvider"` and `"renameProvider.prepareProvider"`),
then `uri_for`, then `ask` on `timeouts.request` (a rename costs what
`references` costs; a separate budget would be a guess about an unmeasured
cost), then the decoder through the existing `malformed(method, error)`. Nothing
in `ask`, `require`, `uri_for`, the document mirror or `READINESS_CEILING`
changed.

The two capability assertions were added to
`the_real_roslyn_handshake_is_accepted_and_read_correctly`, which turns the
fixture's `renameProvider: {"prepareProvider": true}` from unread noise into
coverage at zero cost.

**`Misbehave::EchoParams` writes the params to a `paramsFile` rather than
replying with them** — a deliberate departure from the plan's wording, for a
reason that is the point of the whole step: the client reads every answer
through a decoder that ignores keys it does not know, so a `rename` whose reply
was its own echoed params would decode as a **perfectly valid empty edit** and
prove nothing. A file is the only channel that carries the evidence, and it
follows the existing `EchoArgv`/`argvFile` precedent exactly. The step's own
`reply` still goes back, so the client under test sees a normal answer.

Two client tests carry the weight, and both were mutation-checked:

* `a_rename_is_refused_before_a_byte_goes_out_when_the_server_never_advertised_it`
  scripts the fake to **never answer** `rename` and `prepareRename`, so a
  request that went out would hang. Replacing `self.capabilities.rename` with
  `true` made it fail with `TimedOut { after: 8s }` instead of `Unsupported` —
  the test really does prove the refusal precedes the send.
* `a_server_that_renames_but_cannot_prepare_still_renames` is the two-field
  split at the client level: `renameProvider: true`, `prepareRename` scripted
  never to answer, and the rename must still work.

Result: `cargo test -p cb-core --lib lsp::` → 423 passed;
`cargo test -p cb-core --test lsp_client` → 42 passed; `cargo fmt --check`
clean.

## 1.6 `crates/core/src/lsp/rename.rs` (2026-09-04)

Files: `crates/core/src/lsp/rename.rs` (new), `rename_tests.rs` (new, 23 tests),
`lsp/mod.rs` (+`pub mod rename;`), `crates/core/src/files.rs`
(`MAX_EDITABLE_BYTES` is now `pub`).

`apply_workspace_edit(root, edit, open, expect, files) -> RenameResult`, over an
injected `Files` trait (`read`/`write` on **absolute** paths). `RealFiles`
carries the root so a write goes through `files::write_file` — see `notes.md`
for why it is not the unit struct the plan wrote.

**Phase 1 writes nothing**, and every foreseeable problem is arranged to land
there: a non-empty `resource_operations` list (keyed on non-empty, never on known
kinds), a non-`file:` uri, a path outside the root, any `edits::plan` failure, an
unreadable closed file, one over `files::MAX_EDITABLE_BYTES`, pre-images over
`RENAME_MAX_TOTAL_BYTES` (32 MB), and the token check. Each refusal is whole and
each is a named test — unlike find-usages, which abstains per row, an unapplied
edit is a partial rename and a partial rename does not compile.

**Phase 2 writes closed files only.** An open buffer is never read and never
written; its planned edits come back as `BufferEdits` in the IPC convention
(1-based line, 0-based UTF-16 character, both ends). A failed write restores
every earlier write from its pre-image, newest first; a restore that also fails
becomes a `RenameFailure { unrecoverable: true }` whose wording sends the user
to git immediately, and after any rollback `written` **and** `buffers` are both
emptied so the editor cannot apply half a rename.

The stale-mirror rule is `enclosing_identifier`, per the measurement in
`notes.md`: expand outward over word characters and `_` from each edit's start
and compare to `expect`. A file is refused only when **no** edit matches; a
partial match is applied and the disagreement goes in `message`. An empty
`expect` abstains rather than refusing everything.

Mutation-checked, three mutations, three real failures: the token check never
disagreeing (1 test), no rollback (2 tests), and writing open buffers to disk
(2 tests).

Result: `cargo test -p cb-core --lib lsp::rename` → 23 passed.

## 1.7 `model.rs` + `src/ipc/types.ts` (2026-09-04)

Files: `crates/core/src/lsp/model.rs`, `model_tests.rs` (+7 tests),
`src/ipc/types.ts`, `docs/architecture/ipc-contract.md`.

Six new crossing types — `RenameResult`, `RangeEdit`, `RenamedFile`,
`BufferEdits`, `RenameFailure`, `PrepareRenameResult` — following
`UsageResult`'s conventions exactly: `Availability` outcome,
`unavailable(..)`/`with_server`, camelCase, and **no `skip_serializing_if`
anywhere**. `total: Option<u32>` so `Some(0)` ("the server found nothing to
change") and `None` ("nobody was asked") stay apart.

`RangeEdit` is the first type on this surface with an *end* position, so the
1-based-line / 0-based-UTF-16-character rule is restated on it in both languages,
along with the half-open range and the fact that a **zero-width insertion is the
normal case** for Roslyn rather than an edge one. It carries no byte offsets:
Rust has no text for an open buffer.

`PrepareRenameResult::not_renameable(server)` is a constructor of its own rather
than `unavailable` with a different outcome — `prepareRename` answering `null` is
`Ready` with `renameable: false`, and sharing a constructor with the failure path
is how two opposite claims end up sharing a rendering.

Pins written before the implementation (they failed to compile, naming every
missing type) and mutation-checked afterwards by renaming
`RenameFailure::unrecoverable`.

## 1.5 `session.rs` (2026-09-04)

Files: `crates/core/src/lsp/session.rs`, `crates/core/tests/lsp_session.rs`
(+11 tests, +`renameProvider` in the shared `capabilities()` fixture).

`Message::{Rename, PrepareRename}`, `LspHandle::{rename, prepare_rename}` with
the `TORN_DOWN` fallback, `Unready::{rename, prepare_rename}`, `Session::
{on_rename, on_prepare_rename}`, and a new `Session::open_paths()` snapshotting
the open buffers' absolute paths **on the actor** before the request is spawned.

`LspHandle::rename` takes `old_name` as well as `new_name`: it is the token
check's input, the frontend already has it (it read the identifier out of the
buffer to prefill the field), and this layer cannot re-derive it for a buffer it
has no text for.

**Both** handlers use `Needs::OpenDocument`. `references` may proceed against a
file the server knows only from the project because a maybe beats a no; a rename
may not, because the ranges are applied to our text and so must have been
computed from our text — and `prepareRename`'s range is handed to the editor to
place a field over. `request_position` is reused unchanged and its doc comment
now lists the rename call sites and what a one-line-low rename looks like
("renamed 0 things"). `caveat_note`/`with_caveat` are reused rather than
rephrased, with a comment recording that for a rename the caveat means **missed
call sites** and the files it did change have already been written.

`on_prepare_rename` maps all three response shapes: `NotRenameable` →
`not_renameable`, `Range` → `Ready` + converted 1-based lines + whatever
placeholder there was (`None` for real Roslyn), `DefaultBehavior` → `Ready`,
renameable, **no range** (legal: the frontend derives the span from the buffer).

Integration tests cover not-configured, starting, unsupported (both the whole
rename and the prepare-only split), dead-mid-request, torn-down, an empty edit
being `Ready` with `total: Some(0)`, a `null` prepare being `Ready` and not
renameable, a prepare range arriving 1-based with no placeholder, the
closed-file-written / open-buffer-returned split end to end, and the refusal for
a document no server was told about — that last one rewritten after a mutation
showed the obvious version of it passing under `Needs::Position`; see `notes.md`.

Result: `cargo test -p cb-core` → 3093 lib tests + every integration suite green
(`lsp_session` 28, `lsp_client` 42); `cargo fmt --all --check` clean;
`cargo clippy -p cb-core --all-targets` clean; `cargo check --workspace
--all-targets` clean; `pnpm test` 1782 passed, `npx tsc --noEmit -p
tsconfig.json` clean, `pnpm docs:check` passed, `pnpm docs:index` regenerated.

## 1.8 + 1.9 — the commands and the whole frontend (this session)

**Backend bridge**
- `src-tauri/src/commands/lsp.rs` — `lsp_prepare_rename` and `lsp_rename`. Both
  bodies resolve state, clone the handle, drop the lock, await. `lsp_rename`
  additionally loops `reindex_saved_file` over `result.written` (workspace-
  relative, joined onto the root) so the search palette stops serving the closed
  files' pre-rename declarations. `buffers` is deliberately **not** reindexed —
  nothing has been written for those, and indexing an unsaved buffer would put
  declarations in the palette that are not on disk. **No decision in either
  body**, so no free function beside `status_for` was needed.
- `src-tauri/src/lib.rs` — both registered in `generate_handler!`.
- `docs/reference/commands.md` — two rows in the language-server table.
- `docs/INDEX.md` regenerated (163 commands, 164 wrappers).

**Frontend**
- `src/ipc/api.ts` — `lspPrepareRename`, `lspRename` beside `lspFindUsages`.
- `src/shortcutLogic.ts` — `refactor.rename`, F2, `context: "view"`,
  `allowInText: true`. New "Refactor" category needed nothing else:
  `commandSections` groups by `plugin`, not by category.
- `src/components/renameLogic.ts` + `renameLogic.test.ts` — 64 tests, every
  decision: `shouldHandleRename`, `renameReadiness`, `renameOffer`,
  `identifierAt`, `posAt`, `acceptedNewName`, `refusalNote` (reusing
  `usagesLogic.availabilityPhrase`), `provisionalRenameWarning`,
  `replacedTextMatches`, `bufferVerdict`, `renameSummary`, `bufferPaths`,
  `placeRenameField`.
- `src/components/FileEditor.tsx` — the F2 command registration (one per mounted
  editor, `getClientRects().length > 0` decides which acts), `sendChange()` split
  out of `flushChange` so the flush can be awaited, the flush-then-open ordering,
  the positioned `<input>` (Enter commits; Escape and blur cancel), the
  version re-check at Enter, and `applyRenameEdits` — one transaction per file,
  with the enclosing-token check over the buffer.
- `src/views/RunView.tsx` — `renameEdits: { token, byPath }` fed by
  `distributeRename`, sliced per tab on `sameWorkspaceFile`. A `buffers` entry
  with no open editor is counted out of `received` and reported by
  `renameSummary`.
- `src/components/WorkspaceTab.tsx`, `src/App.tsx` — `onNotify` threaded to the
  existing `notificationLogic` stack, the same route `onSignal` takes.
- `src/styles.css` — `.rename-field` / `.rename-input` / `.rename-error`.

**Gate run**: `npx tsc --noEmit` clean; `npx vitest run` 1846/1846 across 71
files (the junction wall was not up this session); `cargo fmt --check` clean;
`cargo check -p cb-app --all-targets` clean; `pnpm docs:check` passed.

## 2026-09-04 — whole-batch quality gate, re-run independently

Steps 1-3 were verified by re-running the entire gate rather than trusting the
step reports. Every command was run to completion in this session and every one
passed on the first attempt; **no fix was required**, so no source file changed
during this verification pass.

| Command | Result |
|---|---|
| `cargo test -p cb-core` | ok — **3093** lib tests (baseline was 3031, so the batch added 62), plus every integration target green: 73 `git_operations`, 42, 28, 26, 13, 9, 7, 4, 3, 2, 1. 0 failed anywhere. ~6 min wall. |
| `cargo fmt --check` | clean, no output |
| `cargo clippy --workspace --all-targets` | clean, zero warnings |
| `pnpm typecheck` | clean (the pnpm junction wall was **not** up this session — `node_modules/react/package.json` resolved, so the literal pnpm scripts ran, not the CLAUDE.md workarounds) |
| `pnpm test` | 1846 passed across 71 files |
| `pnpm docs:index` | 521 files, 163 commands, 164 IPC wrappers, 136 core modules — `lsp_prepare_rename`/`lsp_rename` present in both `INDEX.md` and `reference/commands.md` |
| `pnpm docs:check` | passed: 23 files, all under 500 lines, all relative links resolve |
| `pnpm coverage` | passed the ≥70% line gate with room: **98.23% lines** (2391/2434), 97.46% statements, 93.87% branches. `renameLogic.ts` is 100% lines / 98.75% branches. |
| `cargo build --release` | **succeeded**, 9m54s, `target/release/cb-app.exe` restamped 10:55:38 |

Two things worth recording because they contradict what the docs would lead you
to expect:

1. **The release link succeeded with the app running.** `cb-app.exe` PID 36288
   was live throughout, and CLAUDE.md's "Access is denied (os error 5)" failure
   did *not* occur. So that failure is not guaranteed by a running app — try the
   build before assuming it. The intent hooks and the `Stop` quality-gate hook
   therefore point at a binary that includes this batch.
2. **The junction wall is intermittent, as `agent-shell-junction-block.md`
   says.** It was down here. Do not pre-emptively reach for the tsc-paths or
   vitest-shim recipes; test first.

**Left unrun, deliberately:** `cargo llvm-cov` (the Rust coverage gate) — it
builds a second complete target tree, ~6 GB, and the batch's Rust additions are
`cb-core` modules that carry their own tests; and the ignored oracle tests
(`cargo test -p cb-core --test lsp_oracle -- --ignored`), which are plan step
1.10 and need a real language server. **Nothing in this batch has been exercised
against a real server or in the running app** — that gap is unchanged from what
step 3 reported.

Cosmetic, not a gate failure: several files rewritten during the batch carry LF
endings where the working copy is CRLF, so `git diff` prints "LF will be replaced
by CRLF" warnings for 24 files. `git diff --stat` shows only the genuine added
lines (3764 insertions, 69 deletions, no whole-file churn), and git normalises on
commit.

No stray private target directories exist (checked `$TEMP/claude/*/target`,
`target/wf-*`, `target/agent-*` — all empty).

## 2026-09-04 — adversarial-review fix and the final gate

Two things landed after the batch above: one behaviour fix found by adversarial
review, and one text fix found by the final gate.

### The fix: a `prepareRename` refusal from a ceiling-promoted server kept its caveat

`Session::on_prepare_rename` merged the readiness caveat onto the `Range` and
`DefaultBehavior` arms and not onto `NotRenameable` — the branch where it matters
most, because `prepare()` deliberately lets a `ReadyState::ReadyWithCaveat`
server through. A half-loaded server answering `null` therefore reached the user
as `renameOffer`'s own fallback, *"Put the caret on the symbol's name and try
again"*, blaming the user's caret for the server's loading state.

The three-arm decision was first extracted verbatim (bug intact) out of the
spawned task into a free function `session::prepare_rename_answer(response,
server, caveat)`, a test was written against it and watched to fail naming the
symptom, and only then was it fixed. The extraction was **necessary rather than
cosmetic**: `ReadyWithCaveat` is only reachable through the real 90-second
`READINESS_CEILING`, and `session::start` takes no ceiling override, so the
integration harness in `tests/lsp_session.rs` cannot reach that state inside its
45-second bound. That is CLAUDE.md's "a body that decides anything must not be
the only thing that can be run" rule applied one layer down.

The fix supplies **both halves** of the sentence, because `renameOffer` renders
`prepare.message ?? <its own sentence>` — *instead of*, not beside (verified in
`renameLogic.ts:237`). `with_caveat(None, caveat)` alone would have shown only
the priming note and lost the refusal. So `NO_RENAME_HERE` states the fact
without the frontend's caret advice — the wrong next step for a server that never
primed — and `caveat_note`'s wording is reused verbatim exactly as `on_rename`
reuses it. A **primed** server's refusal stays bare (`message: None`), preserving
`PrepareRenameResult::not_renameable`'s rule; both halves are asserted by
`session_tests.rs::a_prepare_rename_refusal_from_a_promoted_server_keeps_its_caveat`.
No frontend change was needed.

### The text fix: collapsed line continuations in the failure messages

Found by reading the code during the gate, not by any check. Three `format!`
literals in `rename.rs::write_plan` and two `assert!` messages in
`tests/lsp_client.rs` had been re-flowed so their continuations collapsed and
their indentation stayed, leaving eighteen-space holes mid-sentence — including
in the unrecoverable message that sends the user to git. `cargo fmt`, `clippy`
and every test passed regardless, because rustfmt does not reformat string
literals and no test asserts the prose. See `notes.md` for the grep that finds
them.

### Two review items checked and already correct

The task preamble listed both as possibly outstanding. Neither was:

- **Word-class divergence.** `rename.rs::is_word` is `is_alphanumeric() || '_' ||
  '$'` and carries a doc comment naming `renameLogic.ts`'s `/[\p{L}\p{N}_$]/u`
  as the same rule, with the `$scope` failure it prevents spelled out.
- **Partial-write rollback.** `write_plan` restores from pre-images newest first
  and sets `unrecoverable: true` when the restore itself fails; the frontend
  escalates on that flag in `renameSummary` rather than listing it as a row.

### The final gate, re-run independently (2026-09-04)

Both fix reports were treated as claims and the whole gate was re-run from
scratch. The cargo suite was then re-run a **second** time, because the first
run's test binaries were built at 12:35 and the whitespace fix landed at 12:40 —
a green from binaries that predate the edit is not a green for the edit.

| Command | Result |
|---|---|
| `cargo test -p cb-core` | **ok — 3331 passed, 0 failed** across 17 targets. Lib **3100** (baseline 3031; the previous batch reported 3093, so review and this pass added 7). Integration: `git_operations` 73, `lsp_client` 42, `lsp_session` 28, `lsp_transport` 26, `sql_sqlite` 23, `intent_retirement` 9, `pty_roundtrip` 7, `durable_why` 4, `intent_attribution` 3 (+1 ignored), `behavioral_replay` 2, `lsp_oracle` 1 (+5 ignored), `sql_postgres` 0 (+19 ignored, needs a server). |
| `cargo fmt --all --check` | clean, no output (run twice — before and after the whitespace fix) |
| `cargo clippy -p cb-core --all-targets` | clean, **zero warnings** |
| `pnpm typecheck` | clean |
| `pnpm test` | **1856 passed** across 71 files |
| `pnpm coverage` | passed the ≥70% gate: **98.24% lines** (2409/2452), 97.48% statements, 93.92% branches. `renameLogic.ts` **100% lines**, 98.98% branches. |
| `pnpm docs:index` | 521 files, 163 commands, 164 IPC wrappers, 136 core modules |
| `pnpm docs:check` | passed: 23 files, all under 500 lines, all links resolve |

The pnpm junction wall was **down** this session — `node_modules/react/package.json`
resolved, so the literal pnpm scripts ran and none of the CLAUDE.md workarounds
were needed. (Third session in a row confirming `agent-shell-junction-block.md`:
test before assuming.)

Stray target directories: **none**. `$TEMP/claude/*/target`, `target/wf-*` and
`target/agent-*` all empty; `target/` holds only `debug`, `release`, `doc`,
`tmp` and rust-analyzer's `flycheck0`, with no `llvm-cov-target`.

**Still unrun, and not claimed:**

- `cargo llvm-cov` (the Rust coverage gate) — it builds a second complete target
  tree of several GB, and the Rust additions all carry their own tests.
- `cargo test -p cb-core --test lsp_oracle -- --ignored` — the 5 real-server
  oracles. **Nothing in this feature has been exercised against a real language
  server or in the running app.** That gap is unchanged from what the earlier
  batches reported, and it is the top item in `todos.md`.
- `cargo build --release` was not re-run, so `target/release/cb-app.exe` still
  carries the previous batch's build and **not** the two fixes above. The intent
  and quality-gate hooks run that binary; rebuild release before relying on
  either to reflect this work.

One incidental timing fact worth knowing: `tests/lsp_transport.rs` takes
**302 seconds**, essentially all of it in
`dropping_the_transport_kills_the_whole_tree`, which libtest reports as "running
for over 60 seconds". It is not hung. Its `bounded!` 30-second wrapper cannot fire
during the blocking `sh -c kill -0` polls, so the wall time legitimately exceeds
the nominal bound. Do not kill the run when you see that line.
