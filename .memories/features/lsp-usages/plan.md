# Plan — closing out the LSP usages work item

Written 2026-08-17, after the real-server oracle round. Covers **every** open
`- [ ]` in `todos.md` for this work item (11 of them). Read `notes.md` first for
the captures behind the oracle round; this file assumes them.

---

## Before anything else

**The working tree is uncommitted.** The oracle round (`lsp_oracle.rs`, the
registry/client readiness fixes, the docs and memory updates) is written,
formatted and green but was never committed — the user had not asked for it.
Branch off `claude/lightweight-ide-replacement-52cp2n` and commit that work as
its own change **before** starting anything below, so a bisect can tell the
oracle round apart from the follow-ups.

```sh
git status --short          # expect ~11 modified + crates/core/tests/lsp_oracle.rs untracked
```

**Servers installed on this machine** (needed by the oracle; not in the repo):
Roslyn 2.140.9 via the VS Code C# extension, rust-analyzer (rustup component),
`typescript-language-server` + `typescript@5`, `pyright`, `basedpyright`.
`basedpyright-langserver` lives in `%APPDATA%\Python\Python313\Scripts`, which is
**not on PATH** — add it or the Python oracle skips.

---

## Phase 1 — the two real defects (do these together)

Same subsystem, small, and both are "a value that looks authoritative and is
not" — the recurring failure this work item keeps rediscovering.

### 1.1 Capture what shape rust-analyzer actually sends for go-to-definition

`todos.md`: *the `LocationLink` path is not backed by a captured payload*.

`protocol.rs:224 decode_goto` handles four shapes (`Null`, `Single`, `Locations`,
`Links`). Roslyn answers `Location[]` even though we send `linkSupport: true`, so
the `Links` arm — including `LocationLink::aim` at `protocol.rs:153`, which
prefers `targetSelectionRange` over `targetRange` — is exercised only by
hand-written JSON. The Rust oracle's `goto_definition` step now sends real
traffic through the decoder and passes, but **nobody recorded which arm ran**, so
it is still not evidence.

Do:

1. Probe rust-analyzer directly (the pattern is in `notes.md`; a ~60-line node
   script speaking `Content-Length` framing over stdin/stdout). Send
   `initialize` with `textDocument.definition.linkSupport: true`, wait for
   `rustAnalyzer/cachePriming` to end, then `textDocument/definition` on the call
   site. **Print the raw result verbatim.**
2. If it is `LocationLink[]`, paste the real payload into
   `protocol_tests.rs` as a captured fixture beside the hand-written ones, named
   so it is obvious which is which, and assert `aim()` picks
   `targetSelectionRange`.
3. If it is `Location[]` after all, that is the finding: write it in `notes.md`
   and say plainly that no server we can reach sends `LocationLink`, so the arm
   stays speculative.

Either outcome closes the item. Do not close it by reasoning about the spec.

### 1.2 `Documents::version()` is an authority nobody consults

`session.rs:1554-1570 dispatch` destructures every `SyncAction` with `..`, so the
mirror's version is dropped on the floor; `client.rs:459` sends its own counter
instead. `documents.rs:71-78` already documents the divergence.

`notes.md:359-364` fixes the design decision — **two acceptable resolutions, one
forbidden**:

- **Either** `Client` takes the version from the `SyncAction` (one counter; the
  mirror becomes the authority it appears to be), passing it through `dispatch`
  and into `did_open`/`did_change`.
- **Or** `Documents` stops exposing `version()` publicly and keeps it privately
  for ordering only.
- **Never** "align" them by making the mirror restart on close. That reintroduces
  a backwards-going version, which is the one thing the mirror's rule exists to
  prevent.

Prefer the first: the mirror already survives close-and-reopen correctly, and a
single counter removes the trap rather than hiding it. Start with a failing test
in `documents_tests.rs` or `lsp_session.rs` asserting the version on the wire
equals `documents.version()`.

---

## Phase 2 — the wire-type change

### 2.1 `SymbolKind` has no `Property`

`symbols/declarations.rs:46-58` has eleven variants and no `Property`, so LSP
`Property` (7) and `Field` (8) both land on `Variable` and carry the wrong badge.

This crosses IPC, so follow `docs/architecture/ipc-contract.md` exactly and in
this order:

1. Update the key-pinning test in `crates/core/src/lsp/model.rs`'s test module
   **first** (and `symbols` model tests if they pin the kind spellings).
2. Add the variant in `declarations.rs`.
3. Map LSP 7 → `Property` in `protocol.rs`'s symbol-kind conversion; keep 8
   (`Field`) wherever it belongs once `Property` exists — decide deliberately and
   write the reason down.
4. Mirror it in `src/ipc/types.ts` in the same change.
5. Check every `match` on `SymbolKind` — `git/grouping.rs` uses
   `declarations.rs` for enclosing symbols, and the badge rendering in the
   frontend needs a case or it silently shows nothing.

`declarations.rs` is pure text-in / kind-out and `git/grouping.rs` depends on it
— **the dependency points that way and must stay that way.**

---

## Phase 3 — decide, then record

Both of these are decisions rather than code. Each has now been raised more than
once; leaving them undecided is what makes them recur.

### 3.1 `.tsx` is opened as `typescript`, not `typescriptreact`

`registry.rs:280` gives `ServerSpec` one `language_id` per language, and
`client.rs:459` sends it verbatim in `didOpen`. `todos.md` parked this until
`didOpen` was written; it is written.

Decide whether `language_id` is a property of the **extension** rather than the
language, then either implement it (a lookup from extension → language id beside
`language_for_extension`, which already knows the extension) or record in
`registry.rs` that one id per language is deliberate and what it costs. Check
against a real server before choosing: open a `.tsx` file through the oracle
harness and see whether `typescript-language-server` answers differently. Given
this round's record, do not assume it does not.

### 3.2 `src-tauri/src/commands/lsp.rs` is 49.7% line-covered

Nine commands, 25 of 37 functions untested, 165 lines. They are thin delegators
by design — but "thin" is an unchecked claim, and this is the **third** command
module to collect the same note (`arch_*` before it).

Pick one and do it:

- Test the delegation (each command calls the right `LspHandle` method and maps
  the result), or
- Write in `CLAUDE.md`'s `src-tauri` section that command modules are
  deliberately untested and why, so the next coverage pass stops re-filing it.

Do not leave it open a fourth time.

---

## Phase 4 — robustness, in priority order

### 4.1 The Unix tree kill has never been executed

**Highest-value item on this list.** `dropping_the_transport_kills_the_whole_tree`
depends on `configure_process_group` making the child a group leader. That is the
mechanism `Supervisor` already relies on, but this machine is Windows-only and
the test has never run on Linux or macOS.

This is exactly the shape of every bug the oracle round found: a claim that holds
by construction and has never been executed. Run it under WSL or a Linux
container. If that is not possible, say so explicitly in `notes.md` — an
unverified platform is a known unknown, not a passing test.

### 4.2 `kill_tree` blocks the calling runtime worker

`taskkill` via `std::process::Command::status`, or a 2-second escalation thread on
Unix, called from async tasks and from `Drop`. Invisible on the app's
multi-thread runtime; on the current-thread runtime every `#[tokio::test]` uses,
it stalls every other task **including the timeout that bounds the test**.

Not worth fixing on its own. Fix it the moment an LSP test starts flaking, and
the fix is `spawn_blocking` — record that here so the diagnosis is not repeated.

### 4.3 `Transport::shutdown` burns `EXIT_GRACE` on a second call

`&self`, always publishes `ShutDown`; first-death-wins means the second call
re-publishes nothing but still waits out the grace period. Harmless while only
`Client::shutdown` calls it. Revisit when the session actor can request teardown
twice — that is the trigger, not a date.

---

## Phase 5 — accepted, with the reason written down

These stay open deliberately. The work is to make sure each carries its reason so
it is not re-litigated.

- **`codeLens` is not the first cut.** Roslyn advertises
  `codeLensProvider: {resolveProvider: true}` and would give the count directly,
  but it is not uniform across servers and needs its own configuration plumbing.
  `documentSymbol` + `references` is one code path for all of them. Revisit only
  if the two-request cost becomes a measured problem.
- **`usagesExtension.ts` and every component are untested by construction.**
  vitest runs in the node environment, so there is no DOM: the block widget's
  `toDOM`, both `mousedown` paths and the viewport reporter have no coverage of
  any kind. This is why the decisions were extracted into `usagesLogic.ts` and
  `lspStatusLogic.ts`, which *are* tested. The mitigation is the manual CDP script
  in `notes.md`; run it after any change to the widget.
- **The dirty dot stays lit after undoing back to saved content.** Predates this
  feature — `FileEditor` sets dirty on any `docChanged` and clears only on save,
  so the flag has always meant "has been edited", not "differs from disk". Fixing
  it means comparing against the loaded text or a CodeMirror `isClean`
  checkpoint. Belongs to the editor, not to this work item; move it out.

---

## Verification, every phase

Run from **Git Bash** — the `process::` tests spawn `sh` and fail without it.

```sh
cargo test -p cb-core                       # the whole logic layer
cargo clippy --workspace --all-targets      # exactly two baseline warnings:
                                            # importers/rider.rs, workspace.rs
cargo fmt --check
pnpm typecheck && pnpm test                 # after any types.ts change
pnpm docs:index && pnpm docs:check          # after any public API or doc change

# The real servers. --test-threads=1 is not optional.
cargo test -p cb-core --test lsp_oracle -- --ignored --nocapture --test-threads=1
```

**Count the test targets; never quote a remembered baseline.** `todos.md` records
four phases in a row where the numbers carried into the round were already stale.

Anything touching readiness, discovery or the document mirror must also pass the
oracle, not just the unit tests — that is the whole reason it exists.

## Housekeeping when the work finishes

```sh
rm -rf target/debug target/llvm-cov-target target/tmp   # never cargo clean,
                                                        # never target/release
ls -d "$TEMP"/claude/*/target 2>/dev/null               # stray agent target dirs
ls -d target/wf-* target/agent-* 2>/dev/null
```

`target/release/cb-app.exe record-intent` is what the `Stop` hook runs; deleting
the release profile silently breaks intent capture.
