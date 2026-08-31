# Completed — SQL console

## Phase 0: pure core (done, 127 tests green)

No database driver, no network. Everything testable with zero database software installed.
`sqlparser = "0.62.0"` is the only new dependency, pinned to an exact minor because it
reshapes its AST between minors.

| Module | What it owns |
|---|---|
| `sql/model.rs` | Wire types. NULL / empty / truncated are **three distinct JSON shapes**; numbers cross as **strings** (precision); `row_cap: Option<RowCap>` reports a cap rather than applying one silently; `rows_affected: Option<u64>` keeps `Some(0)` and `None` apart; no `skip_serializing_if` anywhere. |
| `sql/dsn.rs` | Engine sniffing (two signals must agree or `None`), redaction, `display_form`. |
| `sql/guard.rs` | Read-only verdict: `ReadOnly` / `Write` / **`Refused`** — three outcomes, never two. |
| `sql/format.rs` | Driver cell → `SqlValue`, incl. UTF-8-safe truncation. |
| `sql/dotenv.rs` | `.env` parser. **No interpolation** — `${VAR}` stays literal and unresolved. |
| `sql/store.rs` | User-global connection store; discovered entries hold a **reference, not a value**. |
| `sql/discover.rs` | Candidate connections from appsettings / user secrets / `.env`. |

## The five bugs adversarial review caught — all confirmed by execution, all fixed

Worth reading before touching this code; each is a *class* of mistake, not a typo.

1. **`EXPLAIN (ANALYZE) INSERT` was reported read-only and allowed to run.** sqlparser 0.62
   puts the parenthesised form in `options: Some([UtilityOption{name:"ANALYZE"}])` and leaves
   the struct field `analyze` **false**. The code read only the field. The *bare* form sets
   `analyze: true` and was caught — which is exactly why the existing test passed and the hole
   survived. Now `explain_effect(analyze, options)` returns Executes / PlanOnly / **Unknown →
   Refused**, so an option a future engine adds cannot silently carry execution past the check.
   **Lesson: when a parser encodes the same fact in two places, reading one is a bug.**
2. **Quoting defeated the denylist.** `bare_words` kept only `quote_style.is_none()` tokens, so
   `EXEC [master]..[xp_cmdshell] 'dir'` passed. A separate `denylisted_call` scan now ignores
   quote style but only matches a *call* (followed by `(`, or in an EXEC routine-name run) —
   so a string literal or a column named `readfile` still does not trip it.
3. **A password containing a quote was half-redacted**: `Password=ab'cd` → `Password=***'cd`.
   A bare quote is no longer treated as a provable terminator; when the end cannot be proven
   the value is elided to end of input.
4. **A secret fragment crossed IPC inside a diagnostic.** `dotenv::classify` built its
   "unresolved" reason from the matched value text, and that reason is a `Serialize` field on
   `SqlCandidate`. Reasons now name the **syntax class**, never the text.
5. **`display_form` emitted a password fragment as the server name.** A repeated key is now
   treated as evidence that a pair was parsed from inside a value, and the whole description
   is **refused** (`SqlDisplayConfidence::{Described, Refused}`) rather than half-filled.
   Also found while testing: a URL password containing `/` was previously **not redacted at
   all**, because the authority scan stopped at the first `/`.

**The generalisable rule behind 3, 4 and 5:** a partially redacted secret is a leaked secret.
Where the parse cannot be proven, refuse the whole thing — never emit a confident fragment.

## Deliberate over-refusals (documented, not accidental)

- A repeated key refuses the whole `display_form` description, even when duplication is
  legitimate (ADO.NET would take the last). Indistinguishable from a pair read out of a
  password, so it errs safe.
- An unrecognised `EXPLAIN` option is Refused rather than run. Extend
  `PLAN_ONLY_EXPLAIN_OPTIONS` when adding an engine; do not loosen the fallback.
- `EXEC xp_cmdshell.dbo.SomeProc` is refused — a denylisted name is not a legitimate schema
  qualifier either.

## Phase 1: the driver seam + the SQLite driver (done, 184 `sql::` tests green)

`sql/driver/{mod,caps,value,sqlite}.rs` and `sql/session.rs`. The rule at the top of
`driver/mod.rs`: **this subtree is the only place that opens a database handle, and a
driver decides no rendering** — it hands `format.rs` a `Cell` and `format.rs` decides.

| Piece | What it owns |
|---|---|
| `driver/mod.rs` | `SqlDriver`/`SqlConnection` (boxed-future traits so they stay `dyn`-safe), `ConnectSpec`, `ReadOnlyEnforcement`, `DriverError` (**every** construction runs through `dsn::redact`), `StatementOutcome::{Finished,Stopped}`. |
| `driver/caps.rs` | Two live caps — 1000 rows and 8 MiB — and the confirmation rule. |
| `driver/value.rs` | The mechanical `format::SqlValue` → `model::SqlValue` relabelling (the two enums are still duplicated; `format.rs`'s own comment says its copy belongs in `model`). |
| `driver/sqlite.rs` | `connect_target` (pure DSN → URL/path), `column_type_name`, cell decoding, the row loop. |
| `session.rs` | `SqlSessions`: `PtyManager`-shaped registry keyed by connection id then query id, holding stop handles. Decides nothing. |

### Four decisions worth not re-litigating

1. **A cap is only real once a further row has been *seen*.** Reaching the ceiling merely
   *arms* the cap (`CapTracker::confirm_more` confirms it). A table holding exactly
   `max_rows` rows would otherwise be reported as truncated when it is complete — the same
   family of lie as a silently shortened list, pointing the other way. This was the
   deliberate plausible-but-wrong first implementation and its test caught it.
2. **`read_only` is set one-way.** `SqliteConnectOptions::read_only(true)` when writes are
   not allowed, and **never** `read_only(false)` — a DSN carrying `?mode=ro` already set the
   flag and clearing it would silently upgrade a connection the author wrote as read-only.
   `create_if_missing(false)` always: a console that conjures an empty database has answered
   the user's question with "this table does not exist".
3. **`rows_affected` is always `None` for SQLite.** sqlx reads it from `sqlite3_changes()`,
   whose own comment in the sqlx source says it "returns the number of changes of the *last*
   statement; not necessarily this one". After a `SELECT` it still holds the previous write's
   count. A number that may belong to another statement is worse than no number.
4. **Three enforcement answers, not two.** `ReadOnlyEnforcement::{Driver,GuardOnly,
   NotEnforced}` — SQLite earns `Driver` ("Opened read-only by the driver"); tiberius/postgres
   will need the weaker wording, and a test asserts the weak labels never contain "by the
   driver". The wording is **data on the driver**, so the UI cannot hand one engine another's
   promise.

Also: **stopping is not cancelling**, and nothing is named as if it were. `StopSignal` is a
`tokio::sync::watch` the row loop `select!`s on; it stops *this side* reading and drops the
connection, and the server may still be executing. `StopOutcome::{Signalled,AlreadyStopping,
NotFound}`; `stop` does not deregister (the runner's `finish` does), or a second Stop click
would report NotFound for a visibly-running statement.

New dependency: **`futures-core = "0.3"`** (workspace + `cb-core`) — the `Stream` trait alone,
so the row loop can poll sqlx's `BoxStream` via `poll_fn` without pulling in `futures-util`.
Already in the tree as one of sqlx's own dependencies; no new download.

### The permanent integration test (`crates/core/tests/sql_sqlite.rs`, 18 tests, green)

The first test in this subsystem that touches a real database. Seeds through the driver
itself (no `sqlx` in the test), on a zero-length file the test creates because the driver
never will. Covers the happy path, NULL/empty/truncated as three shapes, both caps plus the
exactly-on-the-ceiling non-cap, read-only refusal **with the file proved unchanged**, the
writable insert, `Stopped` as its own terminal state, and redaction.

**Mutation-verified.** Six wrong implementations were applied and reverted; five were caught.
The sixth — removing `redact` from `DriverError::new` — was **not**, because sqlx's SQLite
errors do not echo the DSN, so the obvious test proved nothing. The test now uses the one
message shape that does carry a secret (`?immutable=Password=hunter2`) and fails without
`redact`. It also pins the known boundary: a secret in `?vfs=`/`?mode=` is echoed unredacted.
See `notes.md` — **check the next driver's messages the same way.**

Two brief expectations the code refused, deliberately: `rows_affected` is `None` for every
SQLite statement (not `Some(1)`), and `Some(0)` cannot arise here, so that distinction is
pinned at the serialisation boundary instead.

Earlier verification, with a throwaway test (written, run, deleted): the read-only handle refuses an INSERT
with sqlite's own `(code: 8) attempt to write a readonly database`; `SELECT 1+1 AS calc`
crosses with `typeName: null` (sqlx spells "no declared type" as the type name `NULL`, which
`column_type_name` maps to `None` — `Some("NULL")` would claim the column's type *is* the
null type); NULL and `''` stay distinct; a 2-row table capped at 1 reports the cap and capped
at 2 reports none; a pre-signalled stop yields `Stopped` with zero rows; a missing file is not
created.

Not done: `tiberius` (phase 2), the `postgres` feature (phase 3), the Tauri command layer,
and the UI.

## Phase 1b: the Tauri command surface (done, 30 new `cb-app` tests; 103 green in `cb-app`)

`src-tauri/src/commands/sql.rs`, registered in `lib.rs`, plus `AppState::sql`
(`SqlSessions`, global — a connection belongs to the profile, not the open root, so it is
cleared by nothing a tab does, exactly like `pty`).

Eight commands: `sql_list_connections`, `sql_discover(root)`, `sql_save_connection`,
`sql_delete_connection`, `sql_set_allow_writes(id, allowWrites)`, `sql_test_connection`,
`sql_execute(queryId, connectionId, sql, channel)`, `sql_cancel(queryId)`.

### The four decisions this layer owns, all free functions with tests

A `#[tauri::command]` body cannot be called by any test in this crate, so every decision is
extracted beside it (`commands/lsp.rs::status_for` is the pattern).

1. **`redact_connection`** — the seam a stored password could escape through. The response
   type is `SqlSecretView`, which has **no variant that can carry a connection string**: a
   `Literal` is described by `dsn::display_form` only, the three reference variants describe
   themselves by naming a file and a key. `SecretSource` (which *can* carry one) stays a
   request-only type. Pinned by a test that serialises a profile holding a password and
   asserts the password is nowhere in the JSON.
2. **`upsert` ignores the incoming `allow_writes`.** This is what makes `sql_set_allow_writes`
   a real separate verb rather than a decorative one — otherwise a rename, or any UI round-trip
   that posted the profile back, could raise consent silently. A new profile starts `false`; an
   update also keeps the stored `created_at_ms`/`last_used_ms`.
3. **`execution_plan(connection, sql, allow_writes) -> Result<Plan, Refusal>`** with three
   refusals. Only `Refusal::Guard` becomes a `SqlEvent::Refused` (which means *the guard
   refused, so nothing was sent*); `EngineUnknown`/`EngineUnsupported` never reached the guard
   and become `Failed { statement_index: None }`.
4. **`classify_connect_failure` abstains to `Failed`, never `Unreachable`.** Filing an
   unrecognised driver message under "unreachable" would send somebody to check a firewall.

### `SqlTestOutcome` has nine variants, not the seven asked for

The brief's seven plus two the abstain rule forces:

- **`EngineUnsupported { engine }`** — a saved SQL Server profile tested in phase 1 must say
  "no driver in this build", which is not "the engine could not be determined". One is fixed by
  picking an engine, the other by waiting for a release.
- **`Failed { message }`** — the fallback above. With only the seven listed variants an
  unclassifiable failure would have had to be filed as one of them.

Also: `Timeout { after_ms: Option<u64> }`. `Some` when *this app's* 10s timeout bit (it knows how
long it waited), `None` when the driver reported a timeout in its own message. A `u64` would have
meant inventing a duration.

### One statement, index 0 — the splitter is deliberately absent

`run_statement` takes a single statement and says splitting is the caller's job. There is no
splitter in `cb-core`, and the obvious one — cut on `;` — is exactly the guessing this subsystem
refuses (a semicolon inside a string literal, a dollar-quoted body or a `BEGIN … END` block).
So the text is guarded as one batch (`guard::classify` already parses every statement in it) and
run as one. `commands/sql.rs` is the single place that changes when a real splitter lands.

### The ordering bug that was designed out

`sql_execute` drains the driver's `mpsc` **inline** rather than in a `forward`-style task, and
sends `Finished` only afterwards. A forwarding task plus an inline `Finished` would race, and the
frontend could see the run end before its last rows. A closed IPC channel stops the *sending* but
not the draining, or the run task would block forever on a full channel.

### Frontend contract (types only, no UI)

`src/ipc/types.ts` gained the whole SQL section (mirrors `sql/{model,dsn,store,discover}` plus the
two command-layer types) and `src/ipc/api.ts` the eight wrappers, `sqlExecute` over a
`Channel<SqlEvent>`. `SqlEngine`, `SqlAuthMode`, `SqlDisplayConfidence` and `CandidateState` were
pinned nowhere in `cb-core`, so `commands/sql.rs` pins their exact wire strings — the TS was
written against a test, not against serde's documented behaviour.

Verified: `cargo check --workspace --all-targets`, `cargo test -p cb-app` (103), `cargo fmt --all
--check`, `npx tsc` over `src/ipc/types.ts` in isolation (clean), `node scripts/generate-index.mjs`
(151 commands) and `node scripts/check-docs.mjs`. `pnpm test`/`pnpm typecheck` could not be run —
the junction block; nothing in this change touches a `*Logic.ts` module.

## Not done

Phases 2-3 (SQL Server via tiberius, then PostgreSQL) and the **UI** — no view, no panel, no
menu entry yet; the commands are registered and reachable but nothing calls them. Also not done:
a statement splitter (see above), and `last_used_ms` is never written by any command. See
`notes.md` for the pinned versions and the **unverified MSRV** problem before adding a driver.

## Verification pass (2026-08-30)

Re-ran every one of the eleven findings' fixes against real inputs (see `notes.md` for the
table of inputs and observed outputs). All sound except one: `src/ipc/types.ts` still
declared nine `SqlTestOutcome` variants after the backend grew to eleven. Added
`cannotOpenFile` and `notADatabase` to the union with the doc comments that say why they are
not `ok`, and kept one new test — `commands/sql.rs::tests::the_typescript_mirror_declares_
every_test_outcome_kind` — that reads the mirror and compares it against the shared
`every_test_outcome()` list. `cargo test -p cb-app --lib` 119 green, `sql::` 208 green,
`sql_sqlite` 23 green, `cargo fmt --check` clean.

## The SQL tab assembled (frontend shell)

New: `src/views/SqlView.tsx` (connection bar + editor + results), `src/views/sqlViewLogic.ts`
and its test. Changed: `src/components/WorkspaceTab.tsx` (`Tab` union, `TABS`, and the first
real entry in `FEATURE_BY_TAB` — `sql: "sqlConsole"`), `src/components/featuresLogic.test.ts`
(a suite over the real tab strip), `src/styles.css` (the whole `sql-*` block).

Decisions worth remembering:

- **No SQL syntax highlighting, on purpose.** `@codemirror/lang-sql` is not a dependency and
  cannot be installed here (junction block). It *is* present in the pnpm store as a transitive
  dep of `@codemirror/language-data`, which makes it tempting — importing it would be a phantom
  dependency and would break a clean install. The editor is the repository's ordinary
  CodeMirror setup with no language mode, keeping `@codemirror/search` for Ctrl+F.
- **Ctrl+Enter / Ctrl+Shift+Enter** are bound ahead of `defaultKeymap` with `preventDefault`,
  the same lesson as Ctrl+/ in `FileEditor`. The caret needs
  `&.cm-focused .cm-cursor { display: block }` or the WebView paints none.
- The chords fire from extensions captured once on mount, so the run reads `runRef`/`stateRef`
  rather than closed-over state — otherwise Ctrl+Enter runs against whichever connection was
  selected when the editor was created.
- **`sqlViewLogic.enforcementBadge` mirrors `driver::ReadOnlyEnforcement::label`'s exact
  strings** and a test pins them. That enum crosses no IPC boundary today, so the frontend has
  to duplicate the wording; if the Rust labels change, change them here too. It has a *fourth*
  tone Rust has no variant for — engine `null`, where there is no driver yet and so no way to
  say which promise applies.
- Writes consent goes through `writesConfirm`, which returns `null` for *disallowing* (that
  direction only restores a protection, and a modal there trains dismissal) and, for SQLite,
  states in its own paragraph that `SQLITE_OPEN_READONLY` is being given up.
- `runTarget("selection", …)` **refuses** an empty selection rather than falling back to the
  whole editor.
- `.sql-view .modal-backdrop` is lifted to `--z-overlay`: the shared `.modal-backdrop` sits at
  z-index 50, below the terminal band, so a raised terminal would float over a consent dialog.
  `AttachConfirm` has the same latent problem and was left alone.

Verified with the `npx tsc` + vitest-shim harness (CLAUDE.md's recipe): `sqlLogic` 72,
`sqlViewLogic` 31, `featuresLogic` 15, `sqlPickerLogic` 17 — 135 passed, 0 failed. The only
tsc diagnostic was `Cannot find module 'vitest'` (the shim has no types), which is expected.
`SqlView.tsx` and the other components remain unverified: React needs the junction.
`pnpm docs:index` was not run for the same reason — `docs/INDEX.md` is stale for the new files.

---

# Phase 1 + UI — the consolidated record

The sections above were written phase by phase; this is the single summary of what shipped.
Detail is not repeated — `notes.md` holds the measured numbers, the driver gotchas and the
verification tables.

## SQLite, end to end

- **`sql/driver/` is the only module in the codebase that opens a database handle** — the rule
  is at the top of `driver/mod.rs` and is what keeps the rest of `sql/` testable with no
  database installed. A driver decides no rendering: it hands `format.rs` a `Cell`.
- `sql/session.rs` (`SqlSessions`) is the `PtyManager`-shaped registry of stop handles; it
  decides nothing.
- `src-tauri/src/commands/sql.rs` — ten commands, every decision extracted into a free
  function beside the command because a `#[tauri::command]` body is untestable here.
- `crates/core/tests/sql_sqlite.rs` — **23 integration tests against a real tempfile
  database**, seeded *through the driver* so the seeding is itself a proof the writable path
  writes. The driver never creates a database, so the test creates a zero-length file; SQLite
  accepts one as valid and empty.

## Read-only is enforced by the *driver*, and that rests on execution

The single most important claim this feature makes. It is not "the text guard rejects writes"
— `SqliteConnectOptions::read_only(true)` is set on the handle whenever `writes_allowed` is
false, so the refusal comes from SQLite itself (`(code: 8) attempt to write a readonly
database`), and the guard is the *second* line, not the only one.

**Six bypass attempts were run against the live driver. All six failed safe:**

| Attempt | Result |
|---|---|
| `?mode=rwc` in the DSN | refused — sqlx's flag arithmetic has `read_only` beat `create_if_missing` |
| `Data Source=file:…?mode=rw` | refused |
| `Filename=` keyword form | refused |
| plain `ATTACH DATABASE '…' AS w` | refused |
| URI-form `ATTACH` | refused |
| multi-statement batch (write hidden after a read) | refused |

These were **throwaway probes: written, executed, observed, deleted** — they are not in
`sql_sqlite.rs`. Recorded here because the claim rests on having run them, not on reading the
code. Anyone doubting it should re-run them rather than re-reason about them; and note the
pinned lesson from the mutation pass: **`ReadOnlyEnforcement::Driver` is a computed label, not
evidence** (it kept reporting `Driver` under a mutation that deleted `read_only(true)`
entirely). A read-only test must assert *the write is refused and the file is unchanged*.

## The eleven review findings

All eleven were confirmed by execution and fixed; the full input/observed table is in
`notes.md`. The two HIGH ones are the ones to remember, because both are *classes*:

1. **`sql_test_connection` returned `Ok` for a `README.md`.** `select sqlite_version()` is a
   pure SQL function that reads no page, and sqlite3 defers its header check to the first page
   read — so a profile pointing at a text file connected and answered `3.46.0`. Fixed with
   `probe_query(engine)` (`select count(*) from sqlite_master`, which *is* a page read) run
   **before** the version query, its failure deciding the outcome; new outcomes
   `notADatabase` and `cannotOpenFile`. A zero-length file is still a valid database and still
   tests `ok` — pinned, because a probe that rejected an empty database would be worse than
   the bug. **Class: a probe that proves less than it claims.**
2. **A dropped stop-sender was read as a user stop, silently returning zero rows.**
   `watch::Receiver::changed()` resolves `Err(RecvError)` the instant the last sender drops,
   and `_ = stop.changed()` in a `select!` matches `Err` as readily as `Ok` — so the row loop
   reported `Stopped`, an action nobody took, with 0 of 50 rows. Fixed with
   `StopWait { Stop, Alive, Gone }`: only a genuine `true` stops; a signal sent *before* the
   sender dropped is still a stop (the flag is read, not the channel's liveness); and a `Gone`
   channel makes the loop stop selecting on it, because `changed()` on a closed channel is
   ready every time and would spin. **Class: an `Err` arm read as success.**

A sibling hunt for both classes plus "a value computed and dropped" found nothing else under
`sql/`; see `notes.md`.

## The UI

| Piece | Notes |
|---|---|
| `src/views/sqlLogic.ts` (+ test) | **72 tests.** The tab's decisions. |
| `src/views/SqlView.tsx` | connection bar + editor + results |
| `src/components/SqlResultGrid.tsx` | plain `div`s on the **`RunningPanel` idiom** — there is no table component in this codebase and no virtualization library installable here, so the grid is hand-rolled on the existing panel CSS rather than on a new dependency |
| `src/components/SqlConnectionPicker.tsx` + `sqlPickerLogic.ts` (+ test) | 17 tests |
| `src/components/WorkspaceTab.tsx` | `FEATURE_BY_TAB = { sql: "sqlConsole" }` — the **first real entry**, so the optional-features picker gates a real tab for the first time |
| `src/styles.css` | the `sql-*` block |

### No SQL syntax highlighting, and that is deliberate

`@codemirror/lang-sql` **cannot be installed here** (the pnpm junction block), so the editor is
the repository's ordinary CodeMirror setup with no language mode, keeping `@codemirror/search`
for Ctrl+F. It is present in the pnpm store as a transitive dependency of
`@codemirror/language-data`, which makes importing it *look* like it works — it would be a
phantom dependency and would break a clean install. **Do not "fix" the missing highlighting by
adding that import.** A comment in `SqlView.tsx` says the same thing at the source.


---

# Phase 2: SQL Server (`driver/mssql.rs`)

Done. The full record is in **`phase2-mssql.md`** beside this file, kept separate only
because this one is already long — fold it in when this file is next pruned.

Headline: **the priority engine has the weakest read-only guarantee.** SQL Server has no
read-only session mode, so each statement runs in a transaction that is always rolled back —
the write *executes* and is undone (`ReadOnlyEnforcement::RolledBackTransaction`). 46 unit
tests, 24 `#[ignore]`d integration tests that **nobody has run** (no server on this machine),
14/14 mutations caught, `sql::` 312 green.

One wiring step is deliberately left: `for_engine(SqlEngine::SqlServer)` still returns `None`,
because registering it flips four `cb-app` tests this phase was scoped not to touch.
