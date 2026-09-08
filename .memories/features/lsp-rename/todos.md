# Todos — F2 rename

## Still open when Phase 1 finished (2026-09-04)

Phase 1 is **shipped and green**: every step 1.1-1.9 is done, the whole gate
passes, and the feature works end to end as far as anything headless can show.
Three things remain, and the first is the one that matters:

1. **Nothing has been exercised against a real language server, or in the
   running app.** The C# behaviour was settled by direct probe (`notes.md`), but
   that probe was a throwaway script, not this code. The oracle harness exists
   and is `#[ignore]`d — see 1.10 below.
2. **The TypeScript and Rust oracles.** `typescript-language-server` and
   `rust-analyzer` have never answered this code. Their rename edit style is
   *assumed* to be whole-identifier replacement rather than Roslyn's minimal-diff
   insertion; the token check covers both, but that is reasoning, not evidence.
3. **`workspace/didChangeWatchedFiles`** for the closed files Rust wrote — see
   "Deferred, deliberately".
4. **`cargo build --release` has not been re-run since the last two fixes.**
   `target/release/cb-app.exe` is what the `Stop` intent hook and the quality-gate
   hook actually execute, so it is one batch behind. Rebuild before relying on
   either to reflect this work. (Note the measured surprise from the previous
   batch: the release link **succeeded with the app running** — CLAUDE.md's
   "Access is denied (os error 5)" is not guaranteed, so try it before assuming.)

## Next

- [x] **1.3 `protocol.rs`** — `method::{RENAME, PREPARE_RENAME}` (+ the two
      assertions in `every_method_constant_is_spelled_the_way_the_protocol_spells_it`),
      `RenameParams`, decoders for `WorkspaceEdit`/`DocumentEdits`/
      `ResourceOperation`/`PrepareRenameResponse`, `ServerCapabilities.rename`
      and `.prepare_rename` + `prepare_provider()`.
- [x] **1.3b the capability declaration** — `textDocument.rename` and
      `workspace.workspaceEdit` in `initialize_params`; re-pin
      `initialize_params_are_exactly_this_json` **with a comment naming what new
      server behaviour is invited** (for rename: none; for `workspaceEdit`: a
      narrowing); add the standalone
      `the_client_never_declares_that_it_will_do_file_operations`.
- [x] **1.4 `client.rs`** — `prepare_rename`, `rename`, via `require(..)`.
- [x] **1.4b `bin/fake_lsp.rs`** — `Misbehave::EchoParams`, which **writes the
      params to a `paramsFile`** rather than replying with them. Replying would
      prove nothing: the client decodes every answer with a decoder that ignores
      unknown keys, so a rename's own echoed params decode as a valid *empty*
      edit. Follows the existing `EchoArgv`/`argvFile` precedent.
- [x] **1.5 `rename.rs`** — injected `Files` trait, two phases, pre-image
      rollback, every refusal named. Test over a fake that fails on the *n*th
      write — that fake is the only way the rollback path is reachable.
- [x] **1.6 `model.rs`** — `RenameResult`, `PrepareRenameResult`, `RangeEdit`,
      `RenamedFile`, `BufferEdits`, `RenameFailure`; six key pins **then**
      `src/ipc/types.ts` in the same commit.
- [x] **1.7 `session.rs`** — `Message::{Rename, PrepareRename}`, `on_rename`
      with **`Needs::OpenDocument`** (not `Needs::Position` — the ranges are
      applied to our text so they must have been computed from our text),
      `Unready::rename`, and add rename to `request_position`'s call-site list.
- [x] **1.8 `commands/lsp.rs`** — `lsp_rename`, `lsp_prepare_rename`; loop
      `reindex_saved_file` over `written`. No decision in the body.
- [x] **1.9 `renameLogic.ts`** + test, then F2 in `shortcutLogic.ts`,
      `FileEditor` field + flush-then-ask, `RunView` fan-out, notification.
- [~] **1.10 oracles** — the harness is **written and committed**
      (`crates/core/tests/lsp_oracle.rs`, 5 `#[ignore]`d tests plus the
      un-ignored `fixtures_say_what_the_oracles_assert`, which runs on every
      `cargo test` and stops a fixture edit quietly invalidating the others).
      **It has never been run against a real server.** The C# question it was
      going to settle was answered by direct probe instead (see `notes.md`), so
      what remains open is the **TypeScript and Rust oracles**: nothing in this
      feature has been exercised against `typescript-language-server` or
      `rust-analyzer`, and their rename edit style (whole-identifier replacement
      versus Roslyn's minimal-diff insertion) is assumed, not measured. Run:
      `cargo test -p cb-core --test lsp_oracle -- --ignored`.

## Carried forward from 1.3/1.4 into the layers above

- [ ] `decode_workspace_edit` **merges** several `documentChanges` entries that
      name one uri into one `DocumentEdits`, so `rename.rs` may assume one entry
      per file and hand the whole set to `edits::plan` at once.
- [ ] `ResourceOperation.kind` is the server's own `String`, not an enum, so an
      operation kind added to the protocol later still arrives and is still
      declined. `rename.rs` must refuse on the list being **non-empty**, never
      on a match over known kinds.
- [ ] A `DocumentEdits` with an **empty** `edits` list is legal and reaches
      `rename.rs`: the server named the file and said it needs no changes.
      Neither that nor zero documents is a refusal.
- [ ] `client.prepare_rename` refuses with capability
      `"renameProvider.prepareProvider"` when only `renameProvider` was
      advertised — `session.rs` must render that as *this server cannot
      prepare*, not as *this server cannot rename*, because `rename` is still
      available and `renameLogic.identifierAt` can supply the prefill.

## Blocking unknowns to settle by measurement

- [x] **Re-probe Roslyn for `renameProvider`.** Done 2026-09-04: it advertises
      `{"prepareProvider": true}`, so both rename and prepareRename are
      available for C# and the fixtures are accurate. See `notes.md`.
- [x] Whether Roslyn returns text-only edits for a C# type rename under
      `documentChanges: false`. **Measured**: it answers `documentChanges`
      regardless, ignoring the declaration, with no resource operations. The
      decoder must read `documentChanges`; the declaration stays because it is
      honest, but it is not the safeguard — the refusal is. See `notes.md`.
- [x] Whether a real server's rename edit replaces a range **not** containing
      the old identifier. **Measured, and worse than expected**: Roslyn's edits
      are zero-width *insertions* carrying the minimal diff (`Walker` →
      `HeapWalker` inserts `"Heap"`), so the replaced text is `""` every time.
      The replaced-text rule is therefore replaced by an
      enclosing-identifier-token check, which covers both edit styles. See
      `notes.md`.

## Carried into the implementation from those measurements

- [x] `renameLogic.identifierAt` is on the **live** path for C#, not a fallback:
      Roslyn's `prepareRename` returns a bare range with no placeholder, so
      without it the rename box opens empty.
- [x] The token check must not hard-refuse a file where *some* edits match — a
      non-matching edit can be legitimate (rename-in-comments, TS shorthand
      expansion). Refuse only when **no** edit in the file matches; otherwise the
      disagreement belongs in the result's `message`.
- [ ] `documentChanges` entries carry `"version": null`, so the version field
      cannot detect a stale mirror. The flush-before-asking ordering is the real
      protection and the token check is the belt-and-braces beside it.

## Deferred, deliberately

- [ ] `workspace/didChangeWatchedFiles` (`type: 2`) for the closed files Rust
      wrote — the correct LSP answer to a caching server serving pre-rename
      text. **Separate commit** after the oracle passes, so a rename regression
      and a watched-files regression cannot arrive together.
- [ ] An "undo rename" command. Needs a durable inverse-edit log and a
      guarantee no other write intervened — a feature, not a mitigation. The
      shipped answer is a notification pointing at the Changes tab's line-level
      revert, which is honest about the closed files not being undoable.

## Carried forward from 1.5/1.6/1.7 into `commands/lsp.rs` and the frontend

- [x] `LspHandle::rename(path, line, character, old_name, new_name)` takes the
      **old** name as well as the new one — it is the token check's input, and
      `rename.rs` refuses a file none of whose edits land on it. The command must
      therefore accept it, and `renameLogic.identifierAt`'s answer is what fills
      it. Passing `""` disables the check (documented abstention), so do not let
      an empty string reach it by accident.
- [x] `RenameResult.written` carries workspace-relative paths, which is exactly
      what `reindex_saved_file` wants — loop it in the command body, no decision.
- [x] `RenameResult.buffers` must be dispatched **synchronously** in the `.then`,
      and an entry with no receiving editor **reported**, not dropped. That is
      the one window the design cannot close and it is one promise resolution
      wide.
- [x] Match a `BufferEdits.path` with `sameWorkspaceFile(tab, path)`, never
      `file.id === path` — a diff tab carries a `path` too.
- [x] A `RenameFailure` with `unrecoverable: true` needs its own escalation, not
      a row in a list: the file holds part of a rename and nothing here has its
      previous contents. `RenameResult.message` already says "review them in git
      now"; the notification must not soften it.
- [x] `PrepareRenameResult` can be `renameable: true` with **all four position
      fields null** (`defaultBehavior`). Treat that as renameable-with-no-range
      and derive the span from the buffer; treating it as a refusal would break
      any server that answers that way.
- [x] A `Ready` rename may still carry a `message` — the ceiling caveat means
      **missed call sites** and the writes have already happened, so
      `provisionalRenameWarning` has to fire *before* Enter is honoured rather
      than reporting afterwards.
