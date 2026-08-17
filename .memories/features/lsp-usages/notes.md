# Notes — LSP usages

## What was verified against the real Roslyn server, and how

Everything below was **executed** this session, not read about. The probe was a
throwaway node script speaking framed JSON-RPC over the server's stdio.

Binary:
`~/.vscode/extensions/ms-dotnettools.csharp-2.140.9-win32-x64/.roslyn/Microsoft.CodeAnalysis.LanguageServer.exe`
— a self-contained `.exe`; launching it needs no `dotnet` on PATH.

**Licence: MIT.** The `ms-dotnettools.csharp` extension root carries an MIT
`LICENSE.txt` (".NET Foundation and Contributors"). This is the one that matters,
because we locate and launch the user's own copy and redistribute nothing.
`ms-dotnettools.csdevkit` is a **different, proprietary** extension — never reach
into it. A design pass flagged the licence as a blocking unknown; it was checked
and it is not one.

`--help` output (real, not inferred) includes `--stdio`, `--pipe`,
`--autoLoadProjects`, `--clientProcessId`, `--logLevel`, `--extensionLogDirectory`,
`--telemetryLevel` (defaults to off), `--extension`, `--devKitDependencyPath`,
`--razorSourceGenerator`, `--razorDesignTimePath`, `--csharpDesignTimePath`.

`initialize` over `--stdio` succeeds and advertises:

```
referencesProvider      {"workDoneProgress":true}
definitionProvider      true
implementationProvider  true
typeDefinitionProvider  true
documentSymbolProvider  true
codeLensProvider        {"resolveProvider":true}
textDocumentSync        {"openClose":true,"change":2,"save":{}}
serverInfo              absent
```

**`--autoLoadProjects` removes the non-standard handshake.** The VS Code
extension sends custom `solution/open` / `project/open` notifications and waits
for `workspace/projectInitializationComplete`. With `--autoLoadProjects` the
server discovers projects from the workspace folders itself, and a full
references query worked without either notification being sent:

```
initialize -> initialized -> didOpen sidecar/inspector/Collections.cs
  -> (20 s) -> textDocument/documentSymbol -> textDocument/references
REFERENCES -> 1 location: sidecar/inspector/Walker.cs line 138
```

Which is correct. Keep `solution/open` in reserve, not in the first cut.

**The server stalls unless server→client requests are answered.** It issues
`workspace/configuration`; replying with one `null` per requested item is
enough. `client/registerCapability` and `window/workDoneProgress/create` also
need replies. A client that handles only responses hangs forever, and the hang
looks exactly like a slow project load.

**`documentSymbol` is hierarchical and carries `selectionRange`** — the anchor
the inline row needs. Names carry signatures:
`TryGetElements(ClrObject, IReadOnlyList<ClrObject>, int) : bool`. Good for a
picker row, needs trimming for the inline row.

## File URIs: what is actually required

`file:///C:/Users/...` — **plain colon, upper-case drive** — is what the probe
sent, and it worked. So Roslyn does not require the `%3A` form and does not
require a lower-case drive.

A design pass found that the C# extension serialises with
`vscode.Uri.toString(true)` (skipEncoding), i.e. it sends `file:///c:/...`
(plain colon, *lower*-case drive). Both spellings therefore work; the common
factor is **the colon is not percent-encoded**. rust-analyzer, by contrast,
emits `%3A`.

Conclusion, and the rule to implement: send the plain-colon form to Roslyn, and
**never compare URI strings for identity**. Convert every incoming URI to a path
immediately and compare paths via `symbols::index::relative_to_root`. That makes
the encoding choice a compatibility detail rather than a correctness one — which
is the only way to survive four servers disagreeing about it.

A URI that does not resolve under the open root (a decompiled framework type, a
`source-generated:` document) must become a result with `path: None` carrying the
raw URI, **not** be dropped (the count would be wrong) and **not** be joined onto
the root (it would open the wrong file).

## Two things in `process/` that are not reusable as-is

Both checked in the source, not assumed:

- **`resolve_program` is private.** `process/mod.rs:8-12` declares
  `mod chunker; mod kill; mod resolve;` and re-exports only
  `chunker::{LineSplitter, Utf8Chunker}`. `resolve_program` (`resolve.rs:21`) is
  exactly the Windows PATHEXT/`.cmd`-shim walk that `typescript-language-server`
  and `pyright-langserver` need, so it has to be exported — a public-API change,
  so `pnpm docs:index` afterwards.
- **`Supervisor` cannot host a language server.** `process/mod.rs:109` sets
  `.stdin(Stdio::null())`, `run()` blocks until the child exits, and the output
  pump decodes bytes into display strings for a terminal via `Utf8Chunker`. All
  three are disqualifying for a long-lived duplex length-delimited protocol.
  `kill::kill_tree` *is* wanted, though: the Roslyn server spawns
  `BuildHost-netcore` / `BuildHost-net472` children (both directories are
  present in `.roslyn/`), and killing only the parent orphans them.

## Sync kind is the server's choice, not ours

Roslyn advertises `change: 2` (Incremental), so a `TextDocumentSyncKind.Full`
notification is not permitted. The design that survives both constraints: send
**one change event whose range spans the whole document**. That is legal under
Incremental sync and keeps every notification self-describing, so a dropped one
costs staleness rather than permanent, invisible desynchronisation — which is
what mapping CodeMirror `ChangeSet` deltas across IPC risks. Revisit only if it
measures badly.

Related: offer `general.positionEncodings: ["utf-16"]` only, and **refuse a
server** that answers with anything else rather than proceeding with columns we
would compute wrongly. UTF-16 is free on the frontend — CodeMirror document
offsets already are UTF-16 code units.

## Phase 2, from the integration pass

**A trailing lone `\r` was excluded from the "whole document" range.**
`protocol::document_range` trimmed `\r` off its last segment, justified by a
comment about `\r\n` being one line break — but the last segment of a split on
`\n` *cannot* contain the `\r` of a `\r\n`, so the trim only ever fired on a
document whose final line ends in a lone `\r`, where the `\r` is content. The
range was then one character short of the document it claimed to replace, so the
server would keep that `\r` and append after it, gaining one per edit — the same
accumulation the neighbouring trailing-newline rule exists to prevent. Fixed, and
pinned by `a_document_ending_in_a_lone_carriage_return_is_still_covered_to_its_end`.
The lesson generalises: the existing test
(`a_carriage_return_stays_inside_the_line_it_ends`, over `"ab\r\ncd"`) passes
identically with and without the trim, i.e. it never exercised the line whose
comment it appeared to justify.

**Two agents sharing one crate's `#[cfg(test)]` tree cost real time.** For part
of the round `cargo test -p cb-core --lib` could not compile at all, because a
sibling agent's `protocol_tests.rs` was legitimately in its red phase. Only
`cargo check -p cb-core --lib` (non-test) was usable meanwhile. If Phase 3 is
split across agents again, either sequence the ones touching the same crate or
expect the test target to be unbuildable for stretches — a build failure in that
window is not evidence about anyone's own code.

**`docs:index` does not list `pub use` re-exports.** `resolve_program` was made
public via `pub use resolve::resolve_program;` in `process/mod.rs`, and
`generate-index.mjs` still shows `process/mod.rs` without it. The index was
regenerated correctly; it simply does not see re-exports. Do not go looking for
a broken run.

**Measured this round:** `cargo test -p cb-core` end to end is ~62 s for the lib
target plus ~26 s for the three integration targets, on a shared `target/` with
nothing else building.

## Teardown cannot `await` under `AppState`'s lock

`AppState` uses `std::sync::Mutex` and `set_workspace_interleaved` does its
cache clears *while holding the guard*, deliberately. A language server's
shutdown is asynchronous, so the session in `AppState` must be a **handle to an
actor** (an `mpsc` sender), not the servers themselves: requesting teardown is
then a non-blocking send that is legal under the guard, and the process shutdown
happens off-lock. That send is the atomic step — after it nothing can serve the
old session.

Consequence for the recorder: a rejected session is **a running process tree**,
not inert data like a stale test result. So `record_lsp_session` returns
`Result<(), LspHandle>` and is `#[must_use]` — handing the handle back — because
a `bool` would let a caller leak `Microsoft.CodeAnalysis.LanguageServer.exe` and
its BuildHost children for the life of the app. This is the one way it differs
from the four existing `record_*` recorders it otherwise copies exactly.

## Phase 3, from the review pass: four defects the tests could not see

All four were in `transport.rs`, all four were found by reading rather than by a
failing test, and each is a case where the *absence* of behaviour is silent.

**A strong sender in the reader made "close stdin" a no-op.** `spawn` handed
`read_loop` an `out_tx.clone()`, so `shutdown`'s `self.out.take()` never dropped
the last sender, `write_loop`'s `recv()` never returned `None`, and `ChildStdin`
was never dropped. Two doc comments asserted the opposite. Every server that
answers `shutdown`, ignores `exit` and waits for EOF therefore burned the full
3 s `EXIT_GRACE` and was then `taskkill /T /F`'d. Fixed with
`UnboundedSender::downgrade()` — the reader upgrades per reply, so once stdin is
closed it simply cannot answer, which is the correct state. **The invariant to
keep: `Transport.out` is the only strong sender in the process.** A second clone
anywhere silently restores the bug, and no test can see it because the mechanism
only "works" once the reader has already exited.

**`WriteFailed` was the one death after which nobody killed anything.**
`write_loop` published it and returned; `Drop` only kills while
`death.borrow().is_none()`, and `shutdown`'s grace wait resolves instantly
against an already-published death. So the single death reason that does *not*
imply the process ended was the one that leaked it. Now `write_loop` kills the
tree itself, matching what `read_loop` already did for `Framing`.

**A read error was treated as EOF**, publishing nothing at all: `death()` stayed
`None`, the restart policy was never consulted, and every later request waited
out its deadline and reported `TimedOut`. Now `DeathReason::ReadFailed`.

**An unreadable response was dropped with its id.** `classify` returns
`UnknownShape` for `{"id":3,"error":{"code":"E_FAIL",...}}` — a **string** error
code, which `jsonrpc.rs` swallows via `.and_then(..).ok()` and which non-Roslyn
servers do emit. The waiter then burned its whole deadline. `RequestFailure::Malformed`
existed for exactly this and was constructed only in `client.rs`. The reader now
recovers the id by hand and fails that waiter by name — which is why `Pending`
carries the method string: the reader can correlate by id but cannot name a
method, and `Malformed` has to.

The generalisable lesson: **`TimedOut` is the default failure of any correlated
transport**, because a waiter nobody wakes eventually times out on its own. Every
message this layer drops therefore *becomes* "the server was slow" unless
something deliberately fails the waiter first. Three of the four defects above
are that one shape.

## Two things the fake could not prove, and now can

- **Nothing checked that `answer_for`'s value reached the wire.** `Server::resolve`
  matched a response on its id and discarded the payload, so replacing the whole
  reply with `Outgoing::reply(id, Value::Null)` passed every test in the suite —
  and a flat `null` to a three-item `workspace/configuration` is a real Roslyn
  start-up hang. The `echoAnswer` step field sends the client's own answer back
  as the result, which is the only way a test can observe it.
- **A `replyOutOfOrder` frame could be lost outright.** The push into `held`
  happens on the request's own task and the flush happens from `answer` on
  another; nothing orders them. A held frame pushed after the last answer was
  never written. There is now a 50 ms sweeper that flushes `held` once any answer
  has gone out — which also makes the loss *testable*: script the held reply with
  the longer delay and it is deterministic.

## Two review claims that were wrong

- **`readiness()` promoting a corpse is real; the reviewer's reason was not.**
  The claim was that `watch_readiness`'s `RecvError::Closed` arm is dead code. It
  is not dead — it is reachable when the `Client` is dropped — but it is
  *useless*, because the broadcast sender lives on the transport and a dead
  *server* never closes it. Same fix (`select!` on `watch_death`), different
  reason, and the difference matters: deleting the arm would have been wrong.
- **The Unix tree kill was reported as "gate the test".** The better fix was the
  one the code's own comment already asked for: export
  `process::{kill_tree, configure_process_group}` and delete the transport's
  single-pid copy. Gating the test would have preserved the orphaning on Unix.

## Counting

`cargo test -p cb-core` is now **five** targets plus the lib, not three. The
Phase 2 note recorded "1780 lib + 49 + 2 + 11", and a baseline quoted that way
after Phase 3 is wrong in two directions at once: the lib had already grown to
1808 with Phase 3's own unit tests, and `lsp_transport` / `lsp_client` did not
exist. Count the targets, not the remembered number.


## Phase 4's review pass: three defects the tests could not have caught, and why

All three were confirmed by **mutating the code and running the suite**, not by
reading. Recording them because the *reason* the tests missed them is a property
of this test suite that will not change on its own.

- **Nothing pinned the request direction of the line convention.** `to_lsp_line`
  had exactly three references: the import and the two `Position { line: … }`
  constructions in `session.rs`. Replacing both with the raw `line` left
  `cargo test -p cb-core` **fully green** (1897 lib + every integration target),
  because every reply in `tests/lsp_session.rs` is scripted on the *method name*
  only — `cb-fake-lsp` never looks at the params, so no scripted test can observe
  where a request was aimed. In the app that mutation aims every Find Usages one
  line below the caret, the server resolves no symbol there and answers `[]`, and
  that arrives as `outcome: ready, total: 0`. The fix is `session::request_position`,
  extracted purely so a unit test can assert `(1,4) → line 0, character 4`. **Any
  future outgoing-params rule needs either that shape or a fake that records
  params.**
- **Deleting the `if refusals.len() == 3` branch was also invisible**, for the
  same reason at one remove: the only goto test with a refusal used *one* missing
  capability, which is the branch's `else`. Now covered by
  `a_goto_answer_nobody_could_be_asked_for_is_not_ready_with_three_empty_lists`.
  While there: the branch took `refusals[0]`, and the groups are pushed in a fixed
  order, so a server that lacks `definitionProvider` *and then dies* was reported
  `Unsupported` — the single outcome that tells the user retrying is pointless.
  Severity ordering (`worst`) replaced position.
- **The anchor id's column was unpinned by an assertion that only checked
  distinctness.** Dropping `:{selection_character}` from the id makes two
  overloads on one line `Order.Add@4` and `Order.Add@4#1` — still distinct, still
  green — but the second id is then a function of *position in the answer* rather
  than of the declaration, so the two swap owners when the server lists them in
  the other order and the inline count is drawn against the wrong method. The
  test now asserts the literal ids and that no id contains `#`. Same class:
  `a_range_spanning_lines_underlines_to_the_end_of_its_first_line` was ASCII, so
  `len()`, `chars().count()` and `encode_utf16().count()` all passed it; it now
  contains `𝄞` and the mutation to `chars().count()` fails it (2..9 vs 2..10).

## A measurement, and a stress test that vouched for a pair it never ran

`the_lsp_slot_is_taken_in_the_same_order_as_every_other_cache` used one root on
every call, so `set_workspace`'s `changed` branch — the **only** place
`set_workspace` touches the `lsp` mutex — never executed. Reversing the two
statements in that branch (holding the slot guard across `workspace.lock()`) left
the old test green. Rewritten with two thread groups (swappers alternating `/a`
and `/b`, recorders claiming a fresh generation per iteration) plus an assertion
that at least one record actually published, the same reversal **hangs**:
verified by running it under a 100 s timeout and watching
`has been running for over 60 seconds`. A lock-order stress test with only one of
the two bodies live is decoration.

## Two review claims that were wrong (Phase 4 pass)

- **"`current_workspace` goes through `state.workspace()`, so no `.lock()` exists
  outside `state.rs`."** It does not: `commands/workspace.rs:67` is a literal
  `state.workspace.lock()`, and it is the one such call outside `state.rs`. The
  doc sentence a reviewer wanted corrected was already right; it now names the
  grep result so the next reader does not re-litigate it.
- **"Move the disk read and the buffer snapshot out of `prepare` into the spawned
  task."** Refused, with the reasoning now in `prepare`'s own docs: the `didOpen`
  that `ensure_open` emits must be written ahead of the request *and* stay ordered
  against the next `didChange`, and both properties come from being written on the
  actor in the order the actor handled its messages. The latency is real; the
  proposed fix trades away the one guarantee the module's ordering argument rests
  on. Bounded read (1 MiB, once per file per server) and a snapshot of open
  buffers only, instead.

## `Ready` plus a `message` is now a legal shape

Two places produce it, and a frontend that renders `message` only for non-`Ready`
outcomes drops both: a partially refused goto answer (one `outcome` for three
lists — the refusal cannot be spelled any other way), and a count taken from a
server promoted at the 90 s readiness ceiling. `client::ask`'s docs had always
made qualifying that count the caller's obligation; until this pass no caller did.

## Counting, again

Instructions written from the Phase 3 notes quoted "1855 lib + 49 + 2 + 24 + 30 +
11" and no `lsp_session` target. Actual state at the start of this pass: **1897**
lib and **six** integration targets including `lsp_session` (14). `cb-app` was
quoted as 27 and was **36**. Count the targets, never the remembered number —
this is the third phase in a row where the remembered baseline was stale.

## Two version counters, and only one of them is sent

Found at the Phase 4 verification pass, after the agent that noticed it flagged it
in its report and nobody carried it into these notes. Recording it because the
shape recurs.

`documents.rs` maintains a per-document version with a **high-water rule**: it
never goes backwards, including across a close-and-reopen. `client.rs` maintains
its *own* counter (`let version = 1` on `did_open`, incremented in `did_change`)
and **forgets a document on close**, so it restarts at 1. `session.rs:1520-1521`
then destructures the action and throws the mirror's number away:

```rust
SyncAction::DidOpen { path, text, .. } => client.did_open(path, text),
SyncAction::DidChange { path, text, .. } => client.did_change(path, text),
```

Nothing is broken today, and the client's behaviour is the more defensible of the
two: LSP versions are per-document as the *client* tracks them, and a server
discards its state on `didClose`, so restarting at 1 after a reopen is legitimate.

The defect is not the divergence, it is that **`Documents::version()` is an
authority nobody consults**. It looks like the number on the wire and is not,
which is the same trap as the unpublished `Component::details` recorded in the
architecture notes: *a value that is currently unread is not currently correct.*
The next person to reach for `documents.version()` — to log a desync, to decide
whether a change is stale — will get a number that was never sent.

Two honest resolutions, and the choice is a design decision rather than a fix:
either `Client` takes the version from the `SyncAction` (one counter, the mirror
becomes the authority it appears to be), or `Documents` stops exposing a version
at all and keeps it privately for ordering only. Do not "align" the two counters
by making the mirror restart on close — that reintroduces a backwards-going
version, which is the one thing its rule exists to prevent.

## Phase 5's review pass: a defect three reviewers looked straight through

**Four files in this tree contained raw NUL bytes (0x00) in their source**, written
where the two-character escape `\0` was meant. One was found by two of the three
lenses (`views/RunView.tsx:936`, the `pollKey` separator). The other **five were in
`components/usagesLogic.ts`** — two in `groupUsages`' key and three in
`usageCacheKey` and its doc comment — and nobody spotted them, because a NUL is
invisible in every tool that renders the file. What happened instead:

- ripgrep classifies such a file as binary and prints `binary file matches` with no
  lines, so the repo's primary code-search path is blind to the file. `Grep` over
  `usagesLogic.ts` returned nothing useful all round.
- `Read` renders the byte as nothing, so the doc comment `` Joined with `\0` ``
  reads as `` Joined with ` ` ``. A reviewer therefore filed a confident finding
  that the separator *was* a space and that keys could collide — with a trigger, a
  fix and a citation. The premise was false: the code already separated with NUL,
  which is exactly what it should do. Acting on that finding as written would have
  been a no-op at best.
- `tsc` accepts it, `git diff` still treats the file as text (the byte is past the
  first 8000 bytes it samples), and every gate stayed green. Nothing in the quality
  gate can see this.

Generalisable: **never write a control character as a literal byte in source.**
And when a review finding cites a *quoted character* from a doc comment, verify it
at the byte level (`python -c "open(f,'rb').read().count(b'\x00')"`) rather than by
reading the rendered text — the rendering is where the information was lost.

## Why the version-stamped cache key was not enough on its own

The key was right and the ordering was not. `requestVisible` was guarded against
"a change is still owed to the server" in exactly **one** of its three callers (the
viewport path); the anchors reply and the dropdown-click recompute both skipped it.
So typing during a `lspChangeDocument` round trip issued queries the server
answered about the *previous* text and filed them under the *current* version's
key — where nothing would ever correct them, because the next flush finds those
keys answered and skips them. The number was then a confident, permanent lie about
the buffer on screen.

The lesson is about where a guard lives, not about which guard: a precondition of a
function belongs *in* the function whenever it has more than one caller, and this
one had three, two of them added after the guard was written. The fix also needed a
second counter (`syncedVersion`) rather than the debounce timer alone, because the
timer is null for the whole duration of the in-flight request.

## What the tests could not have caught, and why

`usagesLogic.test.ts`'s `anchor()` builder set `selectionLine: over.line`, so
`line === selectionLine` in all eight `visibleAnchors` cases and the field under
test was single-valued: switching the filter from `a.line` to `a.selectionLine`
passed the whole suite. Same shape as the Phase 4 anchor-id finding, and the same
cause — a builder that derives one field from another removes the distinction the
test was supposed to make. `truncated` was likewise never pinned `false`, so
hardcoding `true` in all three return paths passed. Both are now covered.

## A measurement

`pnpm test` 570 → **607** (23 files) across this pass; coverage 99.71% lines.
`cargo test -p cb-core` end to end was 62 s (lib) + ~98 s (six integration
targets) on a shared `target/`, i.e. the CLAUDE.md warning about the `process::`
tests looking hung still applies — the `git_operations` target alone takes 70 s.

## How to see this working in the running app

`cargo test`/`pnpm test` cannot exercise any of the CodeMirror or React code here,
so this is the only verification that exists. Requires the VS Code C# extension
installed (`ms-dotnettools.csharp`) — otherwise the row correctly reads
*No language server* and step 3 is what you are verifying instead.

1. `pnpm tauri dev`, open **this** repository as the workspace.
2. Run tab → sidebar → `sidecar/inspector/Collections.cs`. Wait: the corner badge
   reads *Language server starting…* then *…loading projects…* for tens of seconds
   (Roslyn loads the solution). **No count appears during that time** — that is
   the feature working, not stalling.
3. Once loaded, an inline row appears above each declaration. Above
   `TryGetElements` it should read **"1 usage"** — the known cross-file call site
   is `sidecar/inspector/Walker.cs:138`, verified against the real server in the
   Phase 0 probe above.
4. Click that row: the dropdown lists `sidecar/inspector/Walker.cs` with line 138
   and the identifier underlined. Click it → Walker.cs opens at line 138 in a new
   tab.
5. Middle-click an identifier (not the row): one target jumps straight there; a
   symbol with an interface and an implementation opens the picker, grouped
   Declarations / Implementations / Type definitions. Middle-click the **row**
   instead — nothing should happen, and in particular the Windows autoscroll
   (four-way arrow) cursor must not appear.
6. Type a character. Every count clears to the faint *Usages* within ~120 ms and
   comes back ~250 ms after you stop. It must never show a stale number.
7. Open a `.ts` file in `src/` with no `typescript-language-server` installed: the
   row reads *No language server* and its tooltip carries the `npm i -g …` command,
   and the titlebar indicator appears with the searched paths under it.

---

# Phase 5 verified in the running app — and it does not work yet

The static gate was green (607 vitest, 99.71% coverage, clippy at baseline). The
feature is still wrong, and only running it showed that. Recorded in full because
the diagnosis took an hour and the root cause is a pattern this repo has now hit
six times.

## The symptom

Open `sidecar/inspector/Collections.cs` in the running app. The inline row above
`TryGetElements` reads **"No usages"**. There is one usage, at
`sidecar/inspector/Walker.cs:138`, proven against the real server in the Phase 0
probe. A confident wrong zero on the one claim this whole subsystem exists to get
right.

## The evidence chain, in the order it was gathered

1. **The UI is not inventing it.** `lsp_find_usages` invoked directly through
   `window.__TAURI_INTERNALS__.invoke` returns `outcome: "ready", total: 0`.
2. **The backend was honest about it.** That same result carries
   `message: "this answer was taken from a server that never finished priming
   (the server did not send \`workspace/projectInitializationComplete\` within
   90s...), so a count may be low."`
3. **`goto_definition` works, `references` does not** — 1 declaration at the
   right line, 0 references, stable over six minutes. That combination is Roslyn
   answering from single-file context with **no project loaded**.
4. **It is not the arguments, the cwd, or the handshake.** The live process
   command line is `--stdio --logLevel Warning --autoLoadProjects`;
   `Launch::cwd` is the workspace root; `initialize` sends `rootUri` *and*
   `workspaceFolders`; `initialized` is sent.
5. **The control experiment settles it.** A throwaway probe with the app's
   *exact* args and cwd, from the repo root, **received
   `workspace/projectInitializationComplete`** and returned
   `Walker.cs line 138`, inside 30 s. Same binary, same workspace, at the same
   time as the app was getting zero.

So the server sends the notification and resolves the reference. The app does not
see the notification.

## Root cause: a one-shot signal on a channel that is allowed to drop

`transport.rs:83` sets `NOTIFICATION_BUFFER = 256` on a `broadcast::channel`, and
`client.rs:754` handles the lag like this:

```rust
// Dropped messages are progress reports, and the *next* one
// still arrives. Giving up here would strand a healthy server
// on "loading" because it was chatty.
Err(broadcast::error::RecvError::Lagged(dropped)) => {
    tracing::debug!(dropped, "fell behind the server's notifications");
}
```

The rationale is false in exactly the way that matters. The dropped messages are
**not** all progress reports: `workspace/projectInitializationComplete` is sent
**once**. Roslyn emits far more than 256 notifications while loading a solution,
so the one message the readiness watch exists to see falls inside the lag window
and there is no next one. The ceiling then fires at 90 s, the server is promoted
to `ReadyWithCaveat`, and every answer taken from it is empty — because the
project genuinely has not loaded.

**This is the sixth instance of the false-rationale pattern** CLAUDE.md records:
a doc comment asserting a safety property the code does not have, where the
conclusion reads as verified and the evidence was never executed. The tell is the
same every time — the sentence explains why a failure is *safe* rather than
describing what the code *does*.

The fix belongs in the transport, not the watcher: a one-shot signal must be
**sticky state recorded as the reader decodes it**, so no subscriber can miss it
by being slow. Raising the buffer only moves the window. Do not make `Lagged`
fatal either — the comment is right that a chatty healthy server must not be
stranded; it is the *conclusion drawn for a one-shot message* that is wrong.

## Second defect, independent of the first: the UI drops the caveat

`usagesLogic` renders `ready` + `total: 0` as an authoritative "No usages" and
ignores `message`. The backend said "a count may be low" and the row did not.
The honesty rule was kept for five phases and broken at the last inch. A `ready`
result carrying a message must not render as a plain count — that is what
`ReadyWithCaveat` is *for*.

## Third defect, found while diagnosing: two documented args are never appended

`registry.rs:479` says `--clientProcessId <pid>` and
`--extensionLogDirectory <dir>` "are the caller's to append". **Nothing appends
them** — verified from the live command line, and `.code-basics/lsp-logs/` was
never created. `config::lsp_log_dir` exists, is gitignored, and has no caller.

The **verified** consequence is the missing logs, and that is what made the hour
above necessary: with `--extensionLogDirectory` wired up, step 5's control
experiment would not have been needed at all.

The missing `--clientProcessId` is a **latent** risk, not an observed leak, and
the distinction was checked rather than assumed: killing `cb-app.exe` in this
session left **no** `Microsoft.CodeAnalysis.LanguageServer.exe` behind, because
closing the child's stdin is enough to end it. What `--clientProcessId` buys is
the case stdin closure does not cover — the app dying without an orderly
shutdown. Worth passing, but do not write it up as a leak that was seen.

## How to drive this app for verification

`pnpm tauri dev` with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`,
then CDP `Runtime.evaluate` over the page target from `http://127.0.0.1:9333/json/list`.
Node 22 has a built-in `WebSocket`, so this needs no dependency (`ws` is not in
this repo's `node_modules`). Two things that cost time:

* **`pnpm tauri dev -- -- .` opens the wrong workspace.** `cargo run` executes
  from `src-tauri/`, so `.` resolves there and you get a 1-project workspace.
  Pass an absolute path.
* React rows respond to `el.dispatchEvent(new MouseEvent("click", {bubbles:true}))`;
  the file tree's rows are `button.tree-row` with the name in `.tree-name`.

## The fix round: what the root cause turned out to be, and why 1901 + 607 tests missed it

**Root cause, confirmed.** The one-shot readiness signal travelled only on the
`broadcast` notification stream (`NOTIFICATION_BUFFER = 256`), and `client.rs`
swallowed `RecvError::Lagged` under a comment reasoning that "the *next* one still
arrives". There is no next one: `workspace/projectInitializationComplete` is sent
once, behind thousands of Roslyn progress notifications. Fixed by recording the
signal as **sticky state in the transport's reader** (`SignalSink`) and reading it
there; `Lagged` stays non-fatal, because the comment was right that a chatty
healthy server must not be stranded — only its conclusion for a one-shot message
was wrong.

**Two further defects of the same shape, found by review of the fix.**

* **The ceiling was a verdict.** `watch_readiness` published `ReadyWithCaveat` and
  *returned*, dropping its `watch::Sender`. So the moment the sticky signal made a
  late correction possible, nothing could apply it: a solution slower than 90 s was
  described as ready-with-a-caveat for the life of the app, and `caveat_note` is
  evaluated per request, so every count on every C# file rendered as "at least N
  usages" (or "Usages unknown" at zero) with exact numbers available the whole time.
  The watch now outlives the ceiling and withdraws the caveat; death still ends it
  without publishing, so a corpse is not laundered by a late signal.
  Pinned by `a_server_promoted_at_the_ceiling_is_corrected_when_the_signal_finally_lands`
  and `a_server_that_dies_after_the_ceiling_is_not_upgraded_by_a_late_signal`.
* **The caveat was invisible in the only surface built to explain server state.**
  `refresh_status` put it in `ServerStatus.detail` under a comment saying a silent
  promotion is prevented — while `summariseLspStatus` drops every row whose tone is
  `ok` and `toneFor("ready") === "ok"`, so the titlebar showed *nothing at all*,
  byte-identical to a healthy workspace. `detail` could not carry it either: for a
  plain `Ready` that field is the program path. Now `ServerStatus.caveat` is its own
  field (`model.rs` key-pinning test + `types.ts` together), `toneFor` takes it and
  returns `warn`, and `spawn_supervisor` compares the whole `ReadyState` rather than
  its `is_ready()` bit — otherwise nothing would ever recompute the row when the
  caveat is withdrawn.

**Why 1901 + 607 passing tests missed all three, which is the durable part.**

* **The lost signal was invisible to every scripted test** because `cb-fake-lsp`
  never sent more than a handful of notifications, so no test ever put the readiness
  watch behind a 256-slot ring. The reproduction had to *manufacture the lag*: the
  flood is scripted onto `initialize` with the signal at index 100 of 600, written
  before the reply, while nothing is draining the channel. A flood emitted later,
  with the watcher already reading, **passed against the unfixed code** — measured.
  Generalisable: a capacity bug needs a test that exceeds the capacity, and
  "plausibly concurrent" is not the same as deterministic.
* **The two follow-on defects were each a state the tests could reach but no test
  asked about.** `a_server_that_never_reports_ready_says_so_rather_than_waiting_forever`
  asserted the caveat is published and stopped there — the *next* transition was
  simply never named, and a sender being dropped is not a failure any assertion sees.
  `lspStatusLogic.test.ts` actively pinned "says nothing when every server is ready",
  which was true of the code and wrong about the product, because the suite had no
  fixture for a `ready` row with a caveat: the state existed on the wire and in no
  builder.
* **The pattern in one line:** all three are the same false-rationale shape CLAUDE.md
  records — a comment explaining why a loss is *safe* rather than describing what the
  code does — and in all three cases the test suite covered the state before the gap
  and the state after it, never the transition across it.

**Two smaller things fixed in the same round, both verified before acting.**
`availabilityPhrase("ready")` was documented as "never actually shown" and is shown
(the fallthrough for a `ready` result whose `total` is not a number) with tone
`count`, the one tone the stylesheet reserves for a row carrying an answer; it is now
`{"Usages unknown", reason}`. And a count answer of any outcome was cached under the
version key for ever, so a mid-session server restart froze every row on
"Language server loading…" until the user typed — the cache now keeps unsettled
answers separately and re-asks them up to `UNSETTLED_RETRY_LIMIT`, which is the
`UsageAnswers` store in `usagesLogic.ts` (extracted precisely so that rule is
testable).

`prepare_log_dir` also created `.code-basics/lsp-logs/` for every language on every
start and nothing pruned it. It is now created only when `registry::takes_caller_args`
says the flag will be used, and pruned oldest-first by count and bytes like
`inspect::dumps`.

---

# The real-server oracle, and the five things it disproved (2026-08-14)

`crates/core/tests/lsp_oracle.rs` starts each language's **real** server over a
fixture it writes itself and asserts the one usage it knows is there. Writing it
was supposed to be a formality. Every claim in `registry.rs` that had been
derived from documentation rather than from a run turned out to be wrong, and
each one produced the exact failure this subsystem exists to prevent: a
confident, prompt, wrong zero.

Captures below are from this machine. Keep them — they are the evidence the
tests cite, and none of it is recoverable without re-running the probes.

## 1. rust-analyzer was promoted ten seconds early

`Readiness::Progress { token_prefix: "rustAnalyzer" }` matched the **first** end
under the namespace. The namespace is not the signal:

```text
 3.8s  rustAnalyzer/Fetching              end   <- the old prefix matched here
 3.9s  rustAnalyzer/Building CrateGraph   end
 4.9s  rustAnalyzer/Roots Scanned         end
 6.0s  rustAnalyzer/Fetching              end   <- and it is not even one-shot
10.3s  rustAnalyzer/cachePriming          begin (title "Indexing")
13.9s  rustAnalyzer/cachePriming          end   <- usable here
```

Note `rustAnalyzer/cachePriming` is spelled nothing like the "Indexing" title it
displays under — which is exactly why guessing it from the UI got it wrong. Also
note `rust-analyzer/flycheck/0`, hyphenated: a different namespace again.

Now `token_prefix: "rustAnalyzer/cachePriming"`. The doc comment on
`Readiness::Progress` had predicted this and said "narrow the prefix if that is
ever observed"; it has been observed.

## 2. TypeScript's readiness token is a UUID, so no prefix can ever match

```text
0.2s  initialize result
0.5s  $/progress begin  token "41f6b03a-c261-42d6-b5a6-676ce8b664b1"
                        title "Initializing JS/TS language features…"
1.3s  $/progress end    (the end value carries the token and nothing else)
2.2s  textDocument/references -> the one real call site
```

`Immediate` published `Ready` at 0.2s and `references` in that window answers
`[]`. The only identifying information is the `begin`'s **title**, so matching it
requires remembering the token at `begin` and recognising it at `end` — hence
`Readiness::ProgressTitle` and the stateful `TitledProgress` filter in
`client.rs`. `readiness_filter` builds **one** `Arc` shared by the transport's
sticky sink and the live stream, because two matchers would each see only part
of the stream.

## 3. `npm i -g typescript` now installs a package with no tsserver

TypeScript **7.0.2** ships `lib/tsc.js` and nothing else — no `tsserver.js`.
`typescript-language-server` starts anyway, logs
`$/typescriptVersion {"version":"1.0.0","source":"bundled"}`, and answers every
query with an empty list. Our own install hint produced this. The hint and the
docs now say `typescript@5` and explain why. Verified working on 5.9.3.

(Ignore `Invalid languageId "typescript" … Correcting to "typescript"` in the
log — it is cosmetic and self-correcting.)

## 4. basedpyright lies for about six hundred milliseconds, and announces nothing

```text
0.7s  initialize result
1.3s  textDocument/references -> []                      <- a wrong zero
1.3s  window/logMessage "Found 2 source files"
1.3s  textDocument/publishDiagnostics                    <- correct from here on
1.3s  textDocument/references -> the real call site
```

No progress, no custom notification. The temptation is the log line, which states
the truth in English — **prose is never read for meaning here**. Diagnostics are
a protocol signal and are causally downstream of the same work: the server cannot
analyse the file without resolving its imports, which is what makes `references`
correct. Published even when there are none. Hence `Readiness::FirstDiagnostics`.

## 5. Two Python servers cannot answer a count at all, and are now not candidates

Both were reachable until they were run.

| Server | `--stdio` | Answer for a fixture with exactly one call site |
|---|---|---|
| `basedpyright-langserver` | required | 1 — correct |
| `pyright-langserver` | required | 1 — correct |
| `pylsp` | **exits 2**, `unrecognized arguments: --stdio` | **0** once launched correctly |
| `jedi-language-server` | **exits 2**, same | **2** — ignores `includeDeclaration: false` |

`jedi` counting the declaration means every count is one too high, so "1 usage"
appears above a method nothing calls. Both removed from `candidates`, following
the `ruff` precedent exactly.

That table also produced a second fix: `default_args` chose by **language**, so
`--stdio` went to whatever program a user named in `lsp.servers.python.program`.
Arguments and readiness are properties of the *program*, so `candidates` now
carries both and `default_args` / `default_readiness` look up by file stem.

## Two environmental traps that cost real time

- **`%TEMP%` here is the 8.3 short path** (`C:\Users\ANTHON~1\...`), and **Roslyn
  cannot load a project underneath one**. It starts, answers `documentSymbol`
  happily, never sends `projectInitializationComplete`, is promoted at the 90s
  ceiling and answers `references` with `[]`. The identical fixture under a long
  path loads in ~7s and is correct. The oracle therefore writes fixtures into
  `CARGO_TARGET_TMPDIR`, never `tempfile::tempdir()`. Do not "simplify" that.
  (Related: a `\?\`-prefixed verbatim path crashes the server outright, so
  `fs::canonicalize` is not the fix.)
- **`dotnet restore` is required** before Roslyn can resolve anything, and the
  fixture needs its own `NuGet.config` with `<clear />` because a machine-wide
  `packageSources` entry here points at a directory that does not exist. The
  target framework is chosen from `dotnet --list-sdks` rather than hardcoded —
  an SDK ships its own band's targeting pack, so `net<major>.0` restores offline,
  and a hardcoded `net8.0` failed on a box carrying only the 9 and 10 SDKs.
- **`rust-analyzer` on PATH was a rustup shim for an uninstalled component.**
  Discovery found it, the launch failed with
  `Unknown binary 'rust-analyzer.exe' in official toolchain`. Reported as
  `Failed` rather than `NotConfigured`, so the row does not carry the
  `rustup component add rust-analyzer` hint that would fix it. Not addressed —
  detecting it would mean executing every candidate at probe time.

## Running it

```sh
cargo test -p cb-core --test lsp_oracle -- --ignored --nocapture --test-threads=1
```

`--test-threads=1` is not optional: four servers indexing at once plus a
`dotnet restore` starved Roslyn past the readiness ceiling and failed the C#
oracle on a machine where it passes in seven seconds. Serially: ~25s for all five.
