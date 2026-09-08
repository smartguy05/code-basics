# Notes — F2 rename

## `.memories/features/lsp-usages/notes.md` records no rename decision

Checked before starting (the plan made it a precondition). Nothing there
contradicts this feature. But it does carry one fact that matters a great deal —
see the next section.

## SETTLED BY MEASUREMENT: Roslyn does advertise rename

Probed 2026-09-04 against the real
`ms-dotnettools.csharp-2.140.9-win32-x64/.roslyn/Microsoft.CodeAnalysis.LanguageServer.exe`
over `--stdio --autoLoadProjects`, declaring `textDocument.rename.prepareSupport`
and `workspace.workspaceEdit.documentChanges: false` exactly as the plan
proposes:

```
renameProvider      {"prepareProvider":true}
referencesProvider  {"workDoneProgress":true}   (the control)
serverInfo          absent
```

So **both** `rename` and `prepare_rename` are available for C#, and the test
fixtures' `"renameProvider": {"prepareProvider": true}` is accurate rather than
invented. The section below is kept because the reasoning is still the right
reasoning — but the risk it describes is closed.

It also turns up 26 advertised capability keys where the usages notes list 7,
including `callHierarchyProvider`, `codeActionProvider`, `hoverProvider`,
`inlayHintProvider`, `semanticTokensProvider`, `signatureHelpProvider` and
`workspaceSymbolProvider`. That list is the menu for anything after rename.

Probe script kept at
`<scratchpad>/probe-rename.js` — it answers `workspace/configuration` with one
`null` per item, which the usages notes record as mandatory or the server hangs.

## Closed: the concern as originally written

`lsp-usages/notes.md` records a **real, executed** `initialize` against
`~/.vscode/extensions/ms-dotnettools.csharp-2.140.9-win32-x64/.roslyn/Microsoft.CodeAnalysis.LanguageServer.exe`
over `--stdio`, and the capabilities it lists are:

```
referencesProvider, definitionProvider, implementationProvider,
typeDefinitionProvider, documentSymbolProvider, codeLensProvider,
textDocumentSync
```

**No `renameProvider`.** Meanwhile the test fixture
`protocol_tests.rs::roslyn_capabilities()` and `tests/lsp_client.rs` both carry
`"renameProvider": {"prepareProvider": true}` — which was unread noise until this
feature, so nothing ever checked it against the server.

Two readings, and they have very different consequences:

1. The notes' list is abridged to the capabilities that feature cared about, and
   Roslyn does advertise rename. Then F2 works for C# as planned.
2. The list is exhaustive and Roslyn over `--stdio --autoLoadProjects` genuinely
   does not offer rename. Then F2 correctly refuses for C#, and the feature is
   only useful for TypeScript and Rust until a different launch mode (the
   VS Code extension's `solution/open` handshake, held "in reserve, not in the
   first cut") is adopted.

**Do not assume either.** Re-run the probe before the frontend work — the same
throwaway framed-JSON-RPC script the usages feature used — and record the answer
here. The capability decode is correct in both cases; what changes is whether
the fixture is a fiction, which would make
`the_real_roslyn_capability_set_advertises_rename_and_prepare` a test that pins
a lie.

## MEASURED: what Roslyn actually answers to a C# type rename

Probed 2026-09-04, `<scratchpad>/probe-rename2.js`: renamed `Walker` →
`HeapWalker` at `sidecar/inspector/Walker.cs` line 31 (a type declared in a file
named after it — the case where a server might want to rename the file too),
with the client declaring `documentChanges: false`, `resourceOperations: []`.

Four findings, two of which change the design.

### 1. Roslyn IGNORES `documentChanges: false` and answers `documentChanges` anyway

```
top-level keys: documentChanges
documentChanges present, length 2
resource operation kinds: (none)
```

There is no `changes` map at all. So **the capability declaration buys no
protection from Roslyn** — the plan's reasoning that declaring `false` forces the
legacy envelope and thereby forbids file operations does not hold, because the
server does not honour it. What actually protects this app is the decoder: it
puts every create/rename/delete into `WorkspaceEdit::resource_operations` and
`rename.rs` refuses the whole rename if that list is non-empty, regardless of
what was declared.

Keep the declaration anyway — it is a true statement about what this client will
do, and `resourceOperations: []` *was* respected (no operations came back) — but
do not describe it as a safeguard. The safeguard is the refusal.

Corollary already in the plan and now confirmed necessary: the decoder **must**
read `documentChanges`, because for Roslyn it is the only shape that arrives.

### 2. A same-named file does NOT provoke a file rename

`resource operation kinds: (none)` for exactly the case that was supposed to be
riskiest. So the blast radius the `documentChanges: false` decision was hedging
against does not materialise here. `Walker.cs` keeps its name and the type
inside it changes.

### 3. `prepareRename` returns a BARE range, with no placeholder

```json
{ "start": {"line":30,"character":22}, "end": {"line":30,"character":28} }
```

So `PrepareRenameResponse::Range { placeholder: None }` is the live path for C#,
and the frontend must derive the field's prefill from the buffer
(`renameLogic.identifierAt`). A design that only handled the
`{range, placeholder}` shape would show an empty rename box.

### 4. THE BIG ONE — Roslyn's rename edits are INSERTIONS, not replacements

```json
{"range":{"start":{"line":160,"character":29},"end":{"line":160,"character":29}},
 "newText":"Heap"}
```

Renaming `Walker` → `HeapWalker` produces **zero-width edits inserting `"Heap"`**,
not edits replacing `Walker` with `HeapWalker`. Roslyn computes a *minimal diff*
against the new name.

**This destroys the stale-mirror check as the plan specified it.** The plan's
§1.9.5 rule — "the replaced text contains the old identifier" — would refuse
every single Roslyn rename, because the replaced text of an insertion is the
empty string. The plan anticipated needing evidence here and said plainly: *do
not weaken it silently, and do not keep it if it refuses correct renames.* The
evidence is in, so it does not survive in that form.

**The replacement rule, which works for both edit styles:** for each edit,
expand its start position outward to the enclosing identifier token (word
characters plus `_`) and require that token to equal the old name. For the
insertion above, position 160:29 sits inside `Walker` in `new Walker(`, and the
enclosing token is `Walker` — which matches. For a conventional
replace-the-whole-identifier server the enclosing token is also `Walker`. So one
rule covers both, and it still catches the case the check exists for: a stale
mirror pointing at text that is not the symbol being renamed.

Two caveats to carry into the implementation:

- It cannot be a *hard* refusal for edits landing on something that is not an
  identifier at all. Roslyn's rename-in-comments and rename-in-strings options,
  and TypeScript's shorthand-property expansion (`{ foo }` → `{ newFoo: foo }`),
  both legitimately touch text that is not the bare identifier. Treat a
  non-matching token as grounds to refuse only when **no** edit in the file
  matches; if some do, the answer is plausible and the disagreement belongs in
  the result's `message`.
- `documentChanges` entries carry `"version": null`, so the version field cannot
  be used to detect a stale mirror. The `didChange`-flush ordering guarantee
  (§1.9.1–1.9.4) is the real protection, and this token check is the
  belt-and-braces beside it — not the other way round.

### And a consequence for `edits.rs`

Insertions are not an edge case in this feature; for Roslyn they are the *normal*
case. So `AmbiguousInsertion`, the zero-width handling in `apply`, and
`an_insertion_at_the_end_of_the_document_appends` are load-bearing rather than
defensive, and `replaced_texts` returning `""` for an insertion — which has its
own test — is the ordinary answer rather than a curiosity.

## Two bugs the tests caught, and both were worth the test

**`AmbiguousInsertion` was unreachable.** Two zero-width insertions at one
position have an equal `range`, so the equal-range branch caught them first and
reported `Overlapping` — a true statement about the wrong thing. The
ambiguous-insertion check now runs *inside* the equal-range branch, before the
disagreement case, because both are true of two insertions and only one names
the actual problem: the answer contains no statement about which goes first.

**A refusal message renders 1-based lines.** The wire is 0-based; the person
reading the sentence is looking at an editor gutter. `describe`/
`describe_position` add the 1, and a test asserts the rendered `"line 2"` rather
than the protocol's `1`. (My first version of that test asserted the 0-based
number and failed — the code was right.)

## `byte_offset` takes scalars, not a `Position`

`protocol.rs` already does `use super::positions::byte_to_utf16`, so `positions`
sits *below* `protocol` in the layering the module docs set out. Accepting a
`protocol::Position` in `positions::byte_offset` would invert that. It takes
`(text, line, character)` instead, matching `utf16_to_byte`'s shape.

## The one place `edits` deliberately does not clamp

Everything in `positions` clamps, and the module doc explains why: slicing on a
bad index would panic inside a command, which the user experiences as the app
breaking. `byte_offset` therefore resolves line 40 of a three-line file to the
end of the document.

`edits::apply` refuses that instead, because the *consequence* is different. A
clamped position in a usage row shows the reader the wrong line; a clamped
position in a rename applies an edit **somewhere plausible** in a file the user
may not have open. A server naming a line we do not have is describing a
different version of the file, and the only safe answer is to stop. That is why
`OutOfDocument` exists and why `apply` checks line bounds against its own
`line_count` before touching the text.

`replaced_texts` is the exception to the exception: it is diagnostic, so it
reports what it can for a nonsense range rather than refusing, because a caller
asking "what is about to be replaced?" is owed an answer even when the answer is
that the ranges make no sense.

## Writing Rust through the Bash tool: heredocs mangle escapes

A `python - <<'PYEOF'` heredoc carrying `\r\n` inside a Rust string literal put
**real** carriage returns and newlines into `positions.rs`, splitting the source
mid-token. The quoted heredoc was not the problem on the bash side — the
round-trip through Python's string literals was. Use the Write/Edit tools for
any file content containing backslash escapes; `cat >> file <<'EOF'` is safe for
plain text but not worth the risk for code.

Also: `cd a/b/c && cat >> file` silently does nothing when the shell is already
in `a/b/c`, because the `cd` fails and `&&` short-circuits. The working
directory persists between Bash calls — check `pwd` rather than re-`cd`.

## The obvious `Needs::OpenDocument` test passes under `Needs::Position` too

Worth knowing before writing the frontend, because it is the same trap one layer
up. The first version of
`a_rename_for_a_document_the_server_was_never_told_about_is_refused_not_answered`
scripted the fake to reply with an edit naming `absent.ts`, then asserted the
result was `Failed` with a message containing "absent.ts" and "could not be
read". It passed — **and it still passed after `on_rename` was mutated to
`Needs::Position`**, because with `Position` the request goes out, the reply
comes back, and `rename.rs` then refuses the unreadable closed file on its own
with a message that reads almost identically.

So the test proved nothing about the line it was named after. Two changes fixed
it, and both are needed:

- the fake is scripted `"misbehave": "never"` for `textDocument/rename`, and the
  call is wrapped in a 5-second `tokio::time::timeout` — a request that went out
  hangs, so the test now measures *that the refusal precedes the send*;
- the assertion looks for `"no server could be told about it"`, which is
  `Session::ensure_open`'s own wording and appears in no message `rename.rs`
  produces.

Re-mutated afterwards: `Needs::Position` now fails with `Elapsed(())`. The
general lesson is the one this repository keeps relearning — two layers that
refuse the same input for different reasons make each other's tests vacuous, so
the assertion has to name the *layer*, not just the outcome.

## `RealFiles` carries the workspace root

The plan wrote it as a unit struct. It holds a `root: PathBuf` instead, because
`crate::files::write_file` takes `(root, relative)` — its whole value is the
`resolve` containment check, and synthesising a `(parent_dir, file_name)` pair to
fit a unit struct would make that check vacuous while looking like it did
something. `RealFiles::write` therefore re-derives the relative path with
`relative_to_root` and goes through `files::write_file`, which is a second
opinion on containment in the one place this app writes files the user is not
looking at.

`read` is a plain `std::fs::read_to_string`: the size refusal is
`rename.rs`'s own, measured on the text it got back against
`files::MAX_EDITABLE_BYTES` (now `pub`), so a fake can exercise "too large"
separately from "unreadable". Going through `files::read_file` would have
collapsed those two refusals into one error string.

## `enclosing_identifier` treats a token's end boundary as inside it

`enclosing_identifier(text, line, col)` expands outward from the byte offset, so
a position sitting immediately *after* an identifier names that identifier. That
is deliberate and is pinned twice: a server may aim a zero-width insertion at any
position from the token's start to its end inclusive (Roslyn aims at the start,
but nothing in the protocol says it must), and refusing the end boundary would
refuse a correct rename. The consequence to remember is that
`enclosing_identifier("a = b;", 0, 1)` is `Some("a")` and not `None` — my first
test asserted `None` and failed, and the code was right.

## Two decisions `rename.rs` makes that the plan did not specify

- **Documents are merged by resolved path, not by uri string.** The decoder
  merges by uri, and Roslyn spells a drive colon plainly while rust-analyzer
  percent-encodes it, so one file can legitimately arrive as two
  `documentChanges` entries. Applying them separately would compute the second
  against text the first had already changed. Pinned by
  `edits_for_two_spellings_of_one_path_are_planned_together`, which arranges the
  two entries to overlap so that planning them together is what catches it.
- **A `DocumentEdits` with an empty `edits` list is skipped entirely** — not
  read, not written, no `written` entry. It is legal (the server named the file
  and said it needs no changes), and reading it would let an unreadable file
  refuse a rename that was never going to touch it.

## The heredoc warning in this file is real, and it is not only about escapes

`cat >> file <<'EOF'` through the Bash tool failed twice this session with
``unexpected EOF while looking for matching `''`` on content that was valid
inside a quoted heredoc. Use `Write` for the block and `cat scratch >> target`,
or use `Edit` directly. Do not spend time debugging the heredoc.

## The confirm step cannot be driven from the rename answer, only from prepare

The plan and the carried-forward todo both say `provisionalRenameWarning` must
fire **before** Enter is honoured. That is impossible over a `RenameResult`: a
`ready`-with-`message` rename is only knowable once the rename has run, and the
writes have already landed by then. The caveat is knowable earlier from
`PrepareRenameResult`, which carries the same `outcome`/`message` pair.

So `provisionalRenameWarning` takes `{ outcome, message }` rather than a
`RenameResult`, and both types satisfy it. It is used twice and means the same
thing both times:

- over the **prepare** answer, at F2, to arm `RenameField.caveat` — Enter then
  needs a second press, which is the confirmation the plan asked for and the only
  point at which one is worth anything;
- over the **rename** answer, in `renameSummary`, when the caveat only appears
  after the fact and can no longer be confirmed, only reported.

One function so the two surfaces cannot describe one condition two ways, and a
test asserts they produce the identical sentence for the identical message.

## `flushChange` had to be split, and the split changed one existing behaviour

F2 needs a promise it can refuse with, and `flushChange` was fire-and-forget.
`sendChange()` is the send alone and returns the promise; `flushChange` keeps the
debounce and the not-opened-yet reschedule and delegates.

The one behaviour change: `sendChange`'s `.catch` now **rethrows**, including on
the stale-generation path where the old code returned early. That matters. Had
the generation guard kept its bare `return`, a flush belonging to a superseded
mount would *resolve successfully* and let the rename proceed against a mirror
nobody updated. The state writes stay behind the guard; only the rejection
escapes it.

## The rename field is placed on the identifier's start, not on the caret

`view.coordsAtPos(docLine.from + found.start)` rather than the selection head, so
pressing F2 with the caret in the middle of a word puts the box in the same place
as pressing it at the start. A field that moves depending on where in the word
you happened to be is the kind of thing nobody reports and everybody notices.

## `shouldHandleRename` deliberately does not consult `lspEnabled`

A secrets tab on screen is still the editor the user is looking at. If it
declined the command, `executeCommand` would walk on to the next registered
handler — an editor inside a `display: none` wrapper — and rename a symbol in a
file nobody can see. So being rendered is the whole test, and a tab with no
server *acts* and then refuses out loud through `renameReadiness`.

## `renameReadiness` retries a stale `syncError` rather than reporting it

A `syncError` is set exactly when the server's copy is stale, so it always
co-occurs with "a change is owed" — and reporting the old error without trying
again would strand F2 for the life of the tab on one transient failure. It
answers `"flush"`, and the *second* failure supplies the reason.

## Mutation-tested, because everything passed first time

64 tests in `renameLogic.test.ts` all passed on the first run, which proves
nothing. Four mutations were applied and each was caught:

| Mutation | Tests that failed |
| --- | --- |
| the plan's original "replaced text contains the old name" rule | the two insertion/replacement tests |
| refuse a file when *any* edit misses the token | the some-match-with-a-note test |
| `renameable: false` opens an empty field | the two refusal tests |
| flush check on the debounce timer only, not the versions | the versions-disagree test |

## A `prepareRename` refusal from a ceiling-promoted server has to carry the caveat too

Found by adversarial review, reproduced, fixed. `on_prepare_rename` merged the
readiness caveat onto the `Range` and `DefaultBehavior` arms and **not** onto
`NotRenameable` — the one branch where it matters most. `prepare()` lets a
request through for `ReadyState::ReadyWithCaveat` (`is_ready()` is deliberately
true there), so a half-loaded server can answer `null` about a workspace it never
finished reading; with `message: None` the frontend's `renameOffer` falls back to
*"…Put the caret on the symbol's name and try again"*, blaming the user's caret
for the server's loading state.

Two things the fix had to get right, and the second is the non-obvious one:

- `renameOffer` renders `prepare.message ?? <its own sentence>` — **instead of**,
  not beside. So `with_caveat(None, caveat)` alone would have shown only the
  priming note and lost the refusal. The backend now supplies both halves:
  `NO_RENAME_HERE` (the fact, without the frontend's caret advice, which is the
  wrong next step for a server that never primed) merged with `caveat_note`'s
  wording, reused verbatim exactly as `on_rename` reuses it.
- A **primed** server's refusal stays bare (`message: None`), which is
  `PrepareRenameResult::not_renameable`'s rule: the frontend declines in its own
  words and says nothing about the server. Both halves are asserted.

The decision was extracted out of the spawned task into the free function
`session::prepare_rename_answer(response, server, caveat)` to make it testable at
all: `ReadyWithCaveat` is only reachable through the real 90-second
`READINESS_CEILING`, and `session::start` takes no ceiling override, so **the
integration harness in `crates/core/tests/lsp_session.rs` cannot reach this state
inside its 45-second bound**. That is the CLAUDE.md "a body that decides anything
must not be the only thing that can be run" rule applied one layer down. Test:
`session_tests.rs::a_prepare_rename_refusal_from_a_promoted_server_keeps_its_caveat`
(it failed on the extracted-but-unfixed code with "a refusal from a server that
never finished priming has to say so").

## A message split across source lines lost its spaces, and only a reader would notice

Caught by the final gate, not by any test — because no test asserts the wording.
Three `format!` literals in `rename.rs::write_plan`'s failure arms, and two
`assert!` messages in `tests/lsp_client.rs`, had been re-flowed at some point in
a way that collapsed their line continuations and kept the indentation:

```
"this file could not be written ({detail}), was left part-way through the                  write, and could not be put back ..."
```

So the most important sentence this feature can produce — the unrecoverable one
that sends the user to git — rendered with an eighteen-space hole in the middle
of it. `cargo fmt` is happy (rustfmt does not reformat string literals),
`clippy` is happy, and every test passed, because the tests assert on
`unrecoverable` and on paths, never on the prose.

Two lessons worth keeping:

- **The gate does not read.** A user-facing sentence is the one artifact in this
  repository that nothing automated checks. When a turn writes or re-flows one,
  read it back rendered.
- The mechanical tell is `grep -rnE '"[^"]*[^ ] {6,}[^ ][^"]*"' --include=*.rs`.
  Run it over new code that carries messages. It found exactly the affected
  lines and one pre-existing instance in `dap/registry.rs:265` (out of scope
  here, and still there).

Fixed by collapsing runs of two-or-more spaces that follow a non-space, which
leaves leading indentation alone.
