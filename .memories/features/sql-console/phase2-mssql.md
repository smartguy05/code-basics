# Phase 2: the SQL Server driver (`driver/mssql.rs`) — done

Appended-as-a-file rather than inline in `completed.md` only because that file is already
long; fold it in when `completed.md` is next pruned.

`crates/core/src/sql/driver/mssql.rs` + `mssql_tests.rs` (**46 unit tests**) and
`crates/core/tests/sql_mssql.rs` (**24 integration tests, all `#[ignore]`d, none ever run** —
there is no SQL Server on this machine). `driver/mod.rs` gained one `pub mod mssql;` line and
a layout doc bullet; nothing else outside the driver was touched.

Verified: `cargo test -p cb-core --lib sql::` **312 green**; `cargo check --workspace
--all-targets` clean; `cargo fmt --all` applied. `cargo test -p cb-app` was **not** run — see
"Not done" below.

## The product fact worth stating first

**The user's priority engine is the one with the weakest read-only guarantee.** SQLite refuses
a write at the handle and PostgreSQL refuses it at the server; SQL Server has *no read-only
session mode at all*, so the driver runs each statement inside a transaction it always rolls
back — the write **executes** and is then undone. That is
`ReadOnlyEnforcement::RolledBackTransaction`, `EnforcementStrength::Undone`, and its own
wording. It is not a defect in the driver; it is what the engine offers.

The things that look like a session read-only mode and are not, all deliberately unused:
`ApplicationIntent=ReadOnly` (`Config::readonly`) is availability-group **routing** and
enforces nothing; a read-only database or filegroup is server state the console cannot set.

## The four decisions

1. **The transaction is per statement**: `BEGIN TRANSACTION` … `IF @@TRANCOUNT > 0 ROLLBACK
   TRANSACTION`. The `@@TRANCOUNT` guard is load-bearing — the user's own SQL may `COMMIT` and
   a server error may abort the transaction, and a bare `ROLLBACK` then raises error 3903,
   which this driver would have to report as *the transaction may still be open*: the opposite
   of the truth, and the sort of false alarm that teaches people to ignore the real one.
2. **A stopped statement gets no rollback; the connection is poisoned instead.** Sending one
   means calling `simple_query` again, and tiberius' `flush_stream` first drains every
   remaining packet of the result set the user just asked to stop reading. So `poisoned` is set
   (every later statement is refused) and a `SqlEvent::Notice` says the transaction is rolled
   back by the server when the connection closes. **A cap or a second result set does still get
   its rollback** — that drain is what reusing a tiberius connection after an abandoned stream
   costs at all, and a capped query is far too ordinary a thing to end a connection over.
3. **A failed rollback wins over a failed statement.** `(Err(stmt), Err(rollback))` reports the
   rollback's message with the statement's appended: "a write may still be there" is the fact
   that matters, and the two are different facts rather than one.
4. **Temporal values abstain.** Neither tiberius `chrono`/`time` feature is enabled, so
   `date`/`time`/`datetime2`/`datetimeoffset`/`datetime`/`smalldatetime` arrive as raw TDS day
   and increment counts. They become `Cell::Unsupported { type_name }` naming the SQL type —
   never calendar arithmetic nothing here could check against a server, and never a blank that
   reads as `NULL`. **Enabling a tiberius date feature is the fix and is a *dependency*
   decision**, which is why it was not made here.

## Two bugs in tiberius' own connection-string handling, found by reading its source

Both are two causes collapsing into one message, and both are handled here.

1. **`Trusted_Connection` is ignored entirely.** `tiberius-0.12.3/src/client/config.rs::
   ConfigString::authentication` reads only `integratedsecurity` / `integrated security`.
   Every other spelling falls through to `AuthMethod::sql_server("", "")` — an empty SQL login
   that fails *"Login failed for user ''"*, a message that means "wrong password". So
   `integrated_auth()` detects the request itself and, **on Windows**, supplies the auth method
   tiberius could not see, mirroring its own `Integrated` vs `windows(user, pw)` rule rather
   than inventing a second one.
2. **Integrated auth off Windows fails as a credentials error.** `AuthMethod::Integrated`
   exists only under `cfg(windows)` + `winauth`. `auth_preflight(dsn, supported)` refuses
   **before any socket opens**, naming the key that asked, saying integrated auth is
   Windows-only, and naming the way forward (`User Id` + `Password`). `supported` is a
   *parameter*, not a `cfg!`, so both platforms' behaviour is provable on either platform —
   the outcome under test is a message, and a message only one build can produce is a message
   only one build can check.

`IntegratedAuth` has **three** answers: `NotRequested` / `Requested { key,
understood_by_tiberius }` / `UnreadableValue { key }`. `Integrated Security=maybe` is not a
"no" — reading it as one would silently attempt a login the string never asked for.

## The keyword splitter, and why it is not shared

`keyword_pairs` is quote-aware (`'...'`, `"..."`, doubled-quote escape) because ADO.NET quotes
a value containing `;`. A naive split reads keys **out of a password**:
`Password='a;Trusted_Connection=True'` would be refused as a request for integrated auth on a
string that asked for nothing of the kind. An unterminated quote takes the rest of the string
as part of the value rather than splitting it — the same abstention as `dsn::redact`, and not
shared with it for the reason `sqlite.rs` gives for its own splitter: that one over-matches by
design, which is the wrong bias for deciding what a string asked for.

## Other decisions

- **`Float4` / `f32` is not widened to `f64`.** `3.14f32 as f64` is `3.140000104904175` —
  thirteen digits that are an artefact of the widening and indistinguishable on screen from
  real precision. A finite f32 is handed over as `Cell::Numeric(n.to_string())`, the shortest
  text that reads back as the same f32; the three non-finite values are widened losslessly so
  `format` spells them in the way JSON accepts.
- **`numeric_text(value: i128, scale: u8)`** inserts the point into the scaled integer's own
  digits. Nothing goes through a float, so `0.10` stays `0.10`.
- **Column type names report the TDS token, not a guessed width.** `Intn` / `Floatn` /
  `Datetimen` name a *family* (a nullable integer of unstated width is tinyint through
  bigint), so they keep the token's own name; every token that names exactly one SQL type gets
  that name. `ColumnType::Null` is `None`, the same rule as SQLite's.
- **A second result set is a `SqlEvent::Notice`, not a silent drop.** Its columns differ, so it
  cannot be appended to the first, and this layer has one `SqlResultSet` to report.
- **A statement with no result set still emits an empty `Columns` event** — an absent one is
  indistinguishable from one still in flight.
- **`rows_affected` is always `None`**, like SQLite's but for a different reason: tiberius
  surfaces a count only through `ExecuteResult`, the non-streaming path. `rows.len()` is a
  different number wearing this one's name.
- **Named instances take `SqlBrowser::connect_named` and nothing else does.** That path
  swallows the TCP error and reports "Could not resolve server host", which would send a
  reader to check DNS for a server that answered; the ordinary path keeps the OS's own error.
- `StopWait { Stop, Alive, Gone }`, `within_connect_deadline`, both caps charged at source
  size, borrowed decode and the oversize-binary abstention are carried over from `sqlite.rs`
  unchanged in substance.

## Mutation-verified: 14 wrong implementations applied, all 14 caught

Written, executed, reverted — the phase-1 practice. Each is a *class*, not a typo.

| Mutation | Caught by |
|---|---|
| `keyword_pairs` ignores quoting | 5 tests |
| `integrated_value`: only `true` counts as yes | 3 |
| `Trusted_Connection` dropped from the ignored-key list | 3 |
| unreadable `Integrated Security` value read as "no" | 3 |
| `auth_preflight` lets integrated auth through everywhere | 3 |
| f32 widened to f64 | 2 |
| temporal value becomes a blank cell | 2 |
| `numeric_text` via float division | 3 |
| `Intn` narrowed to `int` | 2 |
| `ColumnType::Null` named `"null"` | 2 |
| oversize binary clipped instead of refused | 2 |
| `ROLLBACK` unguarded | 2 |

## Known weakness, stated rather than papered over

`sql_sqlite.rs` proves redaction with a message shape sqlx *genuinely* echoes a secret in.
**No equivalent was found for tiberius**: neither `tiberius::error` nor `connection-string
0.2.0` was observed to echo more than a single offending character. So the mssql redaction
test (`a_string_that_cannot_be_parsed_fails_at_connect_without_echoing_itself`) proves
redaction **by construction, not by observation**. Whoever first runs `tests/sql_mssql.rs`
against a real server should hunt for a server-side message that carries the DSN and pin it
there — this is the same lesson `notes.md` recorded for SQLite.

## Not done, and deliberately

**`for_engine(SqlEngine::SqlServer)` still returns `None`.** Registering it is one line, but it
flips four `cb-app` tests that currently assert SQL Server has no driver in this build (in
`src-tauri/src/commands/sql.rs`: `driver_for(SqlEngine::SqlServer).is_none()`,
`Refusal::EngineUnsupported`, `SqlTestOutcome::EngineUnsupported`), and this phase was scoped
to the driver file. `probe_query` / `version_query` returning `None` for SQL Server is already
*correct* and needs no change — a client/server engine needs no "is this really a database"
probe.

**The follow-up is exactly: add the `for_engine` arm, update those four `cb-app` tests, run
`cargo test -p cb-app`.** Until then the driver compiles, is tested and is unreachable from
the command layer.

Also still owed, unchanged from phase 1: `sqlViewLogic.ts::enforcementBadge` needs the
`undone` tone for this engine rather than falling through to `guard`.
