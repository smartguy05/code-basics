# Notes — SQL console

## Dependency versions: the plan's numbers were wrong. Use these.

Checked against the crates.io API on 2026-08-30.

| Crate | Plan said | **Actual** | MSRV declared |
|---|---|---|---|
| `sqlx` | 0.8 | **pin `0.8.6`** | 0.8.x: unset · **0.9.0 requires Rust 1.94** |
| `tiberius` | 0.12 | **`0.12.3`** exists (plan was right) | unset |
| `sqlparser` | 0.52 | latest `0.62.0` | unset |
| `tokio-util` | — | `0.7.19` | 1.71 |
| `rusqlite` | (rejected) | `0.40.2` | unset |

**Do NOT take `sqlx 0.9`.** It declares `rust-version = 1.94.0`; this workspace declares
`rust-version = "1.82"`. Pin `sqlx = "0.8.6"` explicitly — a bare `"0.8"` is fine for
semver but pin the patch so a future 0.8.x MSRV bump cannot surprise us.

## MSRV settled — the `rust-version = "1.82"` promise is ALREADY BROKEN

Verified on 2026-08-30 by installing the toolchain, not by reasoning.
`rustup toolchain install 1.82` succeeded (rustc 1.82.0, 2024-10-15); rustup itself is
reachable at `~/.cargo/bin/rustup.exe` even though the cargo shim beside it is not.

**Result, verbatim, on the tree as it stood BEFORE sqlx was added:**

```
$ cargo +1.82 check --workspace --all-targets
error: failed to parse manifest at `.../registry/src/.../time-core-0.1.9/Cargo.toml`
Caused by:
  feature `edition2024` is required
  The package requires the Cargo feature called `edition2024`, but that feature is not
  stabilized in this version of Cargo (1.82.0 (8f40fc59f 2024-08-21)).

$ cargo +1.82 check -p cb-core --all-targets
error: failed to parse manifest at `.../registry/src/.../hashbrown-0.17.1/Cargo.toml`
Caused by:
  feature `edition2024` is required
```

This is **pre-existing and has nothing to do with the SQL console.** Both `time-core 0.1.9`
and `hashbrown 0.17.1` are in the lockfile at `HEAD`. Cargo 1.82 cannot even *parse* those
manifests, so resolution dies before a single line of this repository is compiled — the
failure is not "our code uses a new feature", it is "the locked dependency graph cannot be
read by cargo 1.82 at all".

Scale of the breakage, measured over all 553 registry packages in `Cargo.lock`:

- **33** use `edition = "2024"` (needs cargo ≥ 1.85 merely to parse).
- **61** declare a `rust-version` above 1.82. The highest are **1.88.0**:
  `time`/`time-core`/`time-macros`, `plist`, `psm`, `ar_archive_writer`, `darling*`,
  `serde_with*`. So the **effective floor of the current lockfile is 1.88**, six minor
  versions above what the workspace advertises.
- Several are **direct** workspace dependencies, not obscure transitives:
  `globset 0.4.19` (edition 2024), `uuid 1.24.0` (1.85), `reqwest 0.13.4` (1.85).

`time` arrives through `tauri`/`wry`/`plist`, so this is `cb-app`'s graph as much as
`cb-core`'s, and no dependency change inside the SQL work can fix it.

**What this means for the maintainer — a decision, not a formality.** Either:

1. Raise `rust-version` in the root `Cargo.toml` to **1.88** (or simply to the current
   stable) and say so in the commit. This makes the line true again, costs nothing at
   build time, and is what the lockfile has silently required for some time. Recommended.
2. Pin the offending crates back to pre-edition-2024 releases across the whole graph.
   That means downgrading `tauri`'s transitive `time`, `globset`, `uuid` and `reqwest` —
   a large, fragile change made to honour a floor nobody appears to be building on.

This was **not** changed here: raising the floor is a compatibility decision for the
maintainer, and the task that discovered it was scoped to two `Cargo.toml` files.
Until it is decided, `cargo +1.82` cannot be used as a gate — it fails for reasons that
predate and are unrelated to any change under review.

### sqlx's own contribution to the floor is small

sqlx 0.8.6 does **not** make this worse. `sqlx`, `sqlx-core`, `sqlx-sqlite` (and the
unbuilt `sqlx-macros*`) all declare `edition = "2021"` and **no** `rust-version` at all.
Across everything sqlx newly pulls in, exactly one crate declares an MSRV above 1.82:
**`crc 3.4.0` → 1.83**. Everything else is ≤ 1.83.

So had the graph been clean, adding sqlx would have moved the floor 1.82 → 1.83. The
1.88 floor is entirely pre-existing. Step 5 of the plan (`cargo +1.82 check -p cb-core`
after adding sqlx) reproduces the *same* `hashbrown 0.17.1` error as before — the driver
is not what fails.

`sqlx 0.9` remains refused: it declares `rust-version = "1.94.0"`.

## Phase 0 (module skeleton) — landed

- `sqlparser = "0.62.0"` added to `[workspace.dependencies]` and `crates/core/Cargo.toml`.
  Chosen over the plan's 0.52 (stale) and pinned to the minor because sqlparser is pre-1.0
  and reshapes its AST between minors — `sql::guard` matches on that AST, so a float across
  minors could silently change which statements classify as reads. It declares no
  `rust-version`, so it does not raise the workspace 1.82 floor.
- It pulls three **new transitive deps**: `recursive` / `recursive-proc-macro-impl` and
  `stacker` + `psm`. `psm` is native stack-manipulation code with a build script and
  hand-written asm. It compiled cleanly here on Windows/MSVC; worth knowing it exists
  before anyone tries a cross-compile.
- `pub mod sql;` sits in `lib.rs` **after** `setup`, not before it — the plan said "between
  `secrets` and `setup`", but the list is alphabetical and `setup` < `sql`.
- `crates/core/src/sql/` holds `mod.rs` (the three governing rules) plus six empty stub
  modules: `model`, `dsn`, `guard`, `format`, `dotenv`, `store`. No bodies, no `todo!()`,
  no `#[cfg(test)]` mod declarations — the filling agents add those.
- No database driver was added. Phase 0 needs none.
- Verified: `cargo check -p cb-core` clean, `cargo fmt --all --check` clean. `Cargo.lock`
  gained 72 lines and changed no existing version.

## Phase 1 dependency cost — measured, not estimated

`sqlx = { version = "0.8.6", default-features = false, features = ["runtime-tokio-rustls",
"sqlite"] }` added to `[workspace.dependencies]` and `crates/core/Cargo.toml`.

The plan estimated **130-160 new crates and +60-90s**. Both were high:

| | Plan | **Actual** |
|---|---|---|
| crates added to `cb-core`'s compile graph | 130-160 | **34** (172 -> 206 unique) |
| new packages in `Cargo.lock` | - | **21** (`Cargo.lock` +254 lines) |
| check time | +60-90s | **48.15s** |

Method: `cargo tree -p cb-core --prefix none --no-dedupe | sort -u | wc -l` before and
after; time is a `cargo check -p cb-core --all-targets` against a target dir that was warm
for everything else (warm baseline was 0.87s), so the 48s *is* the cost of compiling the
sqlx addition from scratch. A fully cold whole-graph check was deliberately not measured —
it would have meant discarding several GB of artifacts for a number nobody needs.

The gap is explained by `default-features = false`: the default set drags in `macros`
(hence `syn`/`darling`/`sqlx-macros-core`), `json`, `migrate`, `any`, and the chrono/uuid
type integrations. **`sqlx-macros` and `sqlx-macros-core` appear in `Cargo.lock` but are
absent from `cargo tree -p cb-core`** — the lock records optional dependencies whether or
not they are enabled, so the lockfile is not evidence that `query!` is reachable. The
tree is.

Two things to know about what did come in:

- **`libsqlite3-sys 0.30.1` builds with the `bundled` feature on**, and that is not
  optional: `sqlx sqlite` -> `sqlx-sqlite/bundled` -> `libsqlite3-sys/bundled`. So the
  SQLite C amalgamation is compiled by `cc` in a build script on every cold build. Good
  news for a shipped desktop app (no system SQLite to find at runtime, no version skew),
  but it means a C toolchain is required to build `cb-core` — previously only `git2` and
  `psm` needed one — and it is a cross-compile consideration alongside `psm`.
- `runtime-tokio-rustls` is still a valid feature name in 0.8.6 (it resolves to
  `runtime-tokio` + the rustls TLS backend). It is renamed/reorganised in 0.9, which is a
  second reason the 0.9 upgrade is not a version bump alone.

Verified: `cargo check -p cb-core --all-targets` clean on stable (1.97.1). Nothing under
`crates/core/src/` was touched — only the two manifests and this note.

## `sql/discover.rs` — the value-reading path, and why it is not in `architecture/`

`architecture/signals/` is *forbidden* to read a connection-string value:
`signals::dotnet::connection_strings` iterates `section.keys()` only, and
`framework::screen`/`contains_secret_assignment` discard such text whole. That is
load-bearing (a diagram is exported and shared), so discovery was built as a **separate
read path** rather than by relaxing anything there. `signals::` is unchanged — 148 tests
still pass — and `discovery_returns_values_that_the_graph_refuses` feeds one fixture
through both paths in one test so the divergence cannot drift.

Things worth knowing before extending it:

- **`sql -> architecture` must never become a dependency.** Both package tables in
  `signals/` are private, and importing them would couple the value-reading module to the
  value-refusing one. The table is a parameter — `DiscoveryOptions::package_engines`, with
  `DEFAULT_PACKAGE_ENGINES` as what the app passes. It is also genuinely a different
  table: the graph's rows map a package to a *diagram label* (MySQL, MongoDB, Redis,
  Kafka); this one maps a package to a `SqlEngine`, so it has a row only where the console
  could actually connect.
- **An engine needs two agreeing signals**, packages *and* `dsn::sniff_engine`.
  `EngineChoice` is three variants because `Disagreed` and `NotDetermined` both end in
  `engine: None` but are different sentences to show a user. A single signal is the
  tempting simplification and is wrong: a project referencing `Npgsql` can hold a legacy
  SQL Server string, and `Server=x;Database=y` is spoken by several drivers.
- **A candidate holds no secret.** Discovery reads each value to sniff the engine and
  build the redacted `SqlConnectionDisplay`, then drops it; `SqlCandidate.source` is a
  `store::SecretSource` *reference*. `read_value` is the only function that hands a value
  back, and it is separate on purpose — listing connections and handling a password are
  different acts.
- **User secrets go through `DiscoveryOptions::read_user_secrets`, a fn pointer.**
  `secrets::secrets_path` resolves through `%APPDATA%`, which is process-global; a test
  that repointed it would race `sql::store_tests`, which already mutates `APPDATA`.
- **`secrets::strip_jsonc` was promoted to `pub(crate)` and is reused, not copied** —
  `appsettings*.json` and `secrets.json` are the same comment/trailing-comma/BOM dialect
  and there must be one description of it.
- **`dotenv::classify_value` was added** (a thin public wrapper over the private
  `classify`) so the `${VAR}` unresolved-reference rule is shared rather than re-derived
  for JSON values.
- **The same logical name in two files does not collapse.** `appsettings.json`,
  `appsettings.Development.json` and user secrets each yield their own candidate, because
  precedence depends on `ASPNETCORE_ENVIRONMENT` which this module cannot see.

Mutation-tested rather than trusted: four plausible-but-wrong variants (single-signal
engine, name-keyed ids, name-collapsing dedup, a skip warning that quotes the value) were
each caught by exactly the test written for them, and each failed on its assertion rather
than on a compile error.

Still open: nothing reads `.env.production`/`.env.test` (deliberate — `ENV_FILES` is a
fixed list, not a glob), and no Tauri command surfaces `discover` yet.

## Phase 1 gotchas (driver + SQLite)

- **`sqlx` re-exports no stream adapter.** `Executor::fetch_many` returns a
  `futures_core::stream::BoxStream`, and `sqlx`'s facade re-exports neither `Stream` nor
  `StreamExt`. Added `futures-core` (already in the lock at 0.3.33 via sqlx) and poll it
  with `std::future::poll_fn(|cx| Pin::new(&mut *s).poll_next(cx))` — no `futures-util`.
- **`fetch_many` only reveals columns with the first row.** So `run_statement` calls
  `prepare` first and takes the columns from the `SqliteStatement`; otherwise an empty
  result set arrives with no headers, indistinguishable from a statement that has no
  result set at all.
- **sqlx spells "sqlite reported no type" as the type name `NULL`.** `DataType::Null`'s
  `name()` is `"NULL"`, and `column_decltype` returns a null pointer for an expression or
  subquery column. `column_type_name` maps that to `None`.
- **A value's `type_info()` is the runtime storage class, not the column's affinity** —
  `ValueHandle::type_info_opt` reads `sqlite3_value_type` and only falls back to the
  declared type when the value is NULL. So decoding on it is decoding the real type of
  *that cell*.
- **A `watch` receiver that was already signalled never fires `changed()`.** The row loop
  therefore checks `*stop.borrow_and_update()` at the top of each iteration *before*
  awaiting, or a stop that arrived before the first row would be missed entirely.
- **`watch::Sender::send` fails when every receiver is gone**, which is how
  `session::signal` distinguishes "the row loop already dropped its signal" (`NotFound`)
  from "signal delivered" (`Signalled`).
- `sqlx`'s `read_only` flag beats `create_if_missing` in `establish.rs`'s flag arithmetic,
  so `?mode=rwc` in a DSN cannot defeat `read_only(true)`.

## Command layer (phase 1b)

- **`SqlEngine`, `SqlAuthMode`, `SqlDisplayConfidence` and `CandidateState` had their wire
  strings pinned nowhere.** `sql/model_tests.rs` and `store_tests.rs` pin `SqlValue`,
  `SqlEvent` and `SecretSource`, but not these four — and `src/ipc/types.ts` mirrors all of
  them by hand. `commands/sql.rs::the_core_enumerations_the_frontend_mirrors_serialise_to_their_exact_strings`
  pins them now. If any of the four moves into a shared pin in `cb-core`, drop that test rather
  than keeping two.
- **`AppState::sql` is not cleared by `set_workspace` or `close`.** Deliberate, and the same
  argument as `pty`: a connection profile is user-global, so a statement running against a
  database must survive a tab switch. If a "close everything" verb is ever added, it belongs
  next to the PTY teardown, not inside a workspace method.
- **Do not add a `forward`-style task to `sql_execute`.** The events are drained inline and
  `Finished` is sent afterwards precisely so it can never overtake the driver's own events.
  The `run.rs::forward` pattern is right for `ProcessEvent`, where the terminal event comes
  *from* the supervisor; here `Finished` is minted by the command.

## The SQLite integration test (`crates/core/tests/sql_sqlite.rs`, 18 tests)

- **`sqlx` is reachable from `tests/`** — cargo compiles a test target against
  `[dependencies]` *and* `[dev-dependencies]` — but the test deliberately does not use it.
  Every table is seeded **through the driver** with `writes_allowed: true`, which makes the
  seeding a second proof that the writable path really writes.
- **The driver never creates a database**, so the test creates a **zero-length file** first.
  SQLite accepts one as a valid empty database. That is the whole setup.
- **Mutation-verified, not just green.** Six deliberate plausible-but-wrong implementations
  were applied to the driver and reverted; five were caught for the right reason
  (`read_only(true)` removed, `create_if_missing(true)`, `column_type_name` keeping `"NULL"`,
  `CapTracker::cap` ignoring `confirmed`, `rows_affected` filled from the row count,
  `Stopped` collapsed into `Finished`).
- **The sixth mutation exposed a real gap and changed the test.** Removing `redact` from
  `DriverError::new` was **not** caught: sqlx's SQLite errors mostly do not echo the DSN
  (`unable to open database file` carries no path), so the obvious missing-file test proves
  nothing about redaction. A probe found the one shape that does echo — an unknown *value*
  for a *known* query parameter:

  ```text
  sqlite:<path>?immutable=Password=hunter2
    -> error with configuration: unknown value "Password=hunter2" for `immutable`
  ```

  `a_driver_message_that_does_carry_a_secret_is_redacted` uses that, asserts the message
  contains `Password=***`, and does fail when `redact` is removed. **Before adding tiberius
  or postgres, check the new driver's messages the same way** — a redaction test written
  against a message that never contained a secret is worth nothing.
- **Known redaction boundary, now pinned rather than surprising.** `dsn::redact` recognises
  two forms only (a `Password=`/`Pwd=` keyword, and URL userinfo). A secret placed in a
  *known* parameter sqlx rejects — `?vfs=hunter2`, `?mode=hunter2`, `?immutable=hunter2` —
  is echoed verbatim and passes through untouched. Pinned by
  `a_non_password_parameter_value_is_not_redacted_and_this_is_the_known_boundary`, whose
  failure message says to update it, not delete it, if `redact` is widened. Narrow (it needs
  a known parameter with a bad value) but real.
- **`ReadOnlyEnforcement::Driver` is a claim, not evidence.** It is computed from
  `spec.writes_allowed` alone, so it kept reporting `Driver` under the mutation that removed
  `read_only(true)` entirely. Only the behavioural assertions caught that. Any future
  read-only test must assert *the write is refused and the file is unchanged*, never just
  the label.
- **`rows_affected` cannot be `Some(1)` here.** The brief for this test asked for it; the
  driver returns `None` for every statement by documented design (`sqlite3_changes()` may
  report a previous statement's count). The test pins `None`, proves the insert landed by
  selecting it back, and proves the `Some(0)`/`None` distinction at the **serialisation**
  boundary instead — no SQLite statement can produce `Some(0)`, so that contract is the next
  driver's to exercise.
- **Stopping is tested pre-signalled, on purpose.** Signalling mid-stream races sqlite (which
  finishes a 300-row table faster than a task can react) and would be a flaky test asserting
  a real rule. The pre-signalled path exercises the same `borrow_and_update` check.

## Bridge review round: `src-tauri/src/commands/sql.rs` (six findings, all fixed)

- **"I connected" was reported for a file that is not a database.** `select sqlite_version()`
  is a pure SQL function that reads no page, and sqlite3 defers its header check to the first
  page read — so a profile whose `Data Source` pointed at a `README.md` connected, answered
  the version query with `3.46.0`, and `sql_test_connection` returned
  `Ok { server_version: "3.46.0" }`. Reproduced by running the real driver against a real text
  file before touching anything. The fix is `probe_query(engine)` — `select count(*) from
  sqlite_master` for SQLite, which *is* a page read — run **before** the version query, with
  its failure deciding the outcome and the version merely decorating it. New outcome
  `notADatabase`. An empty (zero-length) file is still a valid database and still tests `ok`;
  that is pinned too, because a probe that rejected an empty database would be worse than the
  bug.
- **Two dead needles, found by running the driver rather than by reading it.**
  `file is not a database` and `file is encrypted` sat in `classify_connect_failure`'s
  `authFailed` list and can never fire there: sqlite3 does not say either at the connect
  stage. They now live in `classify_probe_failure`, which is a *different vocabulary* for a
  different stage. Real strings captured from real runs and used verbatim in the tests:
  connect → `error returned from database: (code: 14) unable to open database file`;
  probe → `error returned from database: (code: 26) file is not a database`.
- **A missing database file was reported as an unreachable host.** `unable to open database
  file` routed to `unreachable`, which sends the reader to check a firewall over a wrong
  path. New outcome `cannotOpenFile`, and the network needles are now checked **before** the
  file ones, because PostgreSQL's unix-socket failure reads `could not connect to server: No
  such file or directory` and matches both — the first half is the true one.
- **The test timeout covered only the connect.** A server that accepts a connection and then
  never answers the probe was reported as anything but `timeout`. `within_timeout` is now
  generic over the probe future, so the deadline covers the whole attempt by construction.
  *Cannot* be proved by giving `test_outcome` a 0 ms deadline against a local SQLite file:
  `tokio::time::timeout` returns the inner future's answer whenever that future is ready on
  its first poll, whatever the deadline — verified by execution, and it is why that test was
  dropped rather than left as a flake.
- **`Plan.note` was computed and thrown away.** The guard's sentence for an *allowed* write
  never reached anybody. Delivered as a new `SqlEvent::Notice` — its own variant, because a
  note sent as `failed` claims the statement did not run and one sent as `refused` claims it
  never reached the database, and both are untrue of a statement that is about to run.
- **Every row crossed IPC twice.** Rows streamed in `SqlEvent::Rows` batches *and* the whole
  result set was repeated inside `SqlEvent::Completed`. Streaming is authoritative, so
  `Completed` now carries `SqlCompletion` — statement index, row count, cap, affected,
  elapsed — and no rows. `SqlResultSet::completion()` is the one place a completion is
  derived. `src/ipc/types.ts` and `docs/reference/commands.md` updated to describe what is
  actually sent.
- **A source-scanning test is the only way to pin "one constructor for `Failed`".**
  `Refusal::to_event` bypassed `failed_event`, and a future bypass would be just as invisible.
  `every_failed_event_in_this_module_is_built_by_its_one_constructor` reads `include_str!`
  and asserts the only construction is the one inside `failed_event`. **Two mistakes made
  while writing it, both caught by deliberately re-introducing the bypass:** matching on
  `trim().starts_with(..)` misses the real shape (`Refusal::EngineUnknown => SqlEvent::Failed
  {`, the construction on the same line as the arm), and the needle must be spelled
  `concat!("SqlEvent", "::Failed {")` or the test's own source lines match themselves.

## Driver seam: a dropped stop sender, the byte budget, and the connect deadline

- **`_ = stop.changed()` in a `select!` reads a *dropped sender* as a user stop.**
  `watch::Receiver::changed()` resolves `Err(RecvError)` the instant the last sender goes, and
  `_` matches `Err` as readily as `Ok`, so the row loop reported `Stopped` — an action nobody
  took — and returned 0 of 50 rows. Reproduced by dropping the `StopHandle` before running.
  Fixed with `StopWait { Stop, Alive, Gone }` in `driver/sqlite.rs`: only a genuine `true`
  stops, a signal that was sent *before* the sender dropped is still a stop (the flag is read,
  not the channel's liveness), and a `Gone` channel makes the loop stop selecting on it — it
  must, because `changed()` on a closed channel is ready every time and would spin the loop.
  Impossible-by-construction was rejected: the sender lives in `sql::session`, whose timeline
  the row loop cannot see.
- **The byte budget was charged at the *rendered* size, which defeats it.** `format` caps a
  cell at ~8 KiB, so a column of 2 MiB values weighed a few kilobytes a row and an 8 MiB
  budget never bit. Now `decode_cell` returns `(Cell, source_bytes)` and
  `caps::charged_cell_bytes` charges whichever is larger, wire or source — so one enormous
  cell exhausts the budget by itself and is reported as an ordinary `ByteLimit` cap.
- **Text and blobs get *different* copy bounds, and the reason is the wire shape.** Text is
  clipped to `caps::TEXT_COPY_BYTES` (= `MAX_TEXT_BYTES + 4`, one whole character past the
  display cap so `format` can still see there was more) — honest, because `SqlValue::Text`
  claims no length. `SqlValue::Bytes` reports `byteLength`, *the whole value's* size, so a
  clipped copy would understate it; past `caps::MAX_BLOB_COPY_BYTES` (1 MiB) the driver
  therefore abstains with `Cell::Error` naming the size rather than shortening it into a lie.
  Both decode **borrowed** (`&str`/`&[u8]` out of sqlite's own buffer), so nothing enormous is
  copied at all.
- **`sql_execute` had no connect timeout**, so a stop aimed at a still-connecting query
  reported `Signalled` while nothing stopped. `driver::within_connect_deadline` +
  `DEFAULT_CONNECT_TIMEOUT` (10 s) at the seam; running out of it is a new
  `ErrorStage::ConnectTimeout`, distinct from `Connect` (a refusal) and from any `Ok` stop.
- **The zero-deadline connect test is flaky and this is the second time it bit.** The note
  above about `sql_test`'s `within_timeout` says it: `tokio::time::timeout` returns the inner
  future's answer whenever that future is ready on its first poll. Measured here — a real
  SQLite connect under `Duration::ZERO` passed 54/60 and *failed* 2/60. Replaced with
  deterministic unit tests over `within_connect_deadline` (a `pending()` future) plus a
  source pin, `the_only_call_that_opens_a_handle_is_the_one_under_the_deadline`, verified
  non-vacuous against a compiling bypass (the open lifted out of the wrapper) and the mutation
  reverted with a checksum.

## Verification pass (2026-08-30): eleven findings re-attacked by execution

Every fix was re-proved by running real inputs against the real driver, not by reading.
Probes were temporary and have been deleted; one permanent test was kept (below).

| Claim | Input actually run | Observed |
|---|---|---|
| F1 probe | profile → a real `README.md` | `NotADatabase { "(code: 26) file is not a database" }` |
| F1 probe | file with a valid `SQLite format 3\0` header + 4 KiB of `0xAB` | `NotADatabase` |
| F1 probe | a seeded database truncated to its first 120 bytes | `Failed { "(code: 11) database disk image is malformed" }` — abstains, never `Ok` |
| F1 probe | a real seeded database | `Ok { serverVersion: "3.x" }` |
| F2 | handle dropped **mid-stream** (1-slot event channel, 20 ms/event reader, drop at 30 ms, 500 rows) | not stopped, all 500 rows in the result **and** 500 across the `Rows` events |
| F2 | a genuine mid-stream signal, handle held 300 ms after | `Stopped`, fewer than 500 rows |
| caps | two 2 MiB cells under a 64 KiB budget | `ByteLimit`, `limit == 1`, widest delivered cell < 64 KiB — no OOM |
| caps | 25 rows under `max_rows: 10` | `RowLimit`, `limit == 10`, not a stop |
| redaction | `?immutable=Password=hunter2` → `…Password=***`; `Password=ab'cd;…` → `Password=***`; `Password="ab;cd";…` → `Password=***;Database=d`; `postgres://user:pa/ss@host/db` → `user:***@host`; `Pwd=p@ss;w0rd!` → `Pwd=***;Database=d` | no fragment survived |
| redaction | `near "SELEC": syntax error`, `file is not a database`, `no such table: customers` | returned byte-identical — not over-redacted |
| double rows | 250 rows | 250 across `Rows` events, one `Completed`, `row_count: 250`, and the completion's JSON has **no** `rows` key |
| timeouts | `within_connect_deadline(5 ms, pending)` | `ErrorStage::ConnectTimeout` (≠ `Connect`), event `failed` with `statementIndex: null` |
| timeouts | `within_timeout(5 ms, pending)` | `Timeout { after_ms: Some(5) }` — a stop is not representable in `SqlTestOutcome` at all |

### The one real defect found — the IPC mirror never caught up with F1

`SqlTestOutcome` gained `CannotOpenFile` and `NotADatabase`; `src/ipc/types.ts` still declared
nine variants. The backend can send two kinds the frontend's union does not describe, and — per
the runs above — `notADatabase` is the *usual* answer for a wrong file, so a `switch` would have
met an undeclared kind in exactly the case those variants exist to report. Fixed in `types.ts`,
and pinned by `commands/sql.rs::tests::the_typescript_mirror_declares_every_test_outcome_kind`,
which `include_str!`s the mirror and checks it against the shared `every_test_outcome()` list
(also used by the serialisation test, so a new variant cannot be added to one check alone).
Confirmed to fail for the right reason: deleting the `cannotOpenFile` line from `types.ts`
produced `does not describe what this build sends: ["cannotOpenFile"]`; the line was restored
and the test re-run green.

### Sibling hunt — nothing else of the three classes in `sql/`

- **Class 1 (an `Err` arm read as success).** The only `select!` in the subsystem is the fixed
  row loop; `stop_wait` returns `Stop`/`Alive`/`Gone`. No other `_ = rx.recv()` or
  `_ = ….changed()` exists under `sql/`. `session::signal` already maps `send`'s `Err` to
  `NotFound` rather than `Signalled`.
- **Class 2 (a probe proving less than it claims).** `probe_query` is the only probe.
  `version_query` is explicitly best-effort and cannot turn a failure into `Ok`. The
  `None` probe for SqlServer/Postgres is unreachable — `driver_for` answers
  `EngineUnsupported` — but it is the thing to re-examine when tiberius lands: a handshake
  is only a probe if it really reached a database.
- **Class 3 (a value computed and dropped).** `Plan`'s four fields are all consumed
  (`note` via `notice_event`). The only remaining discards are deliberate and commented:
  `discover.rs:598 let _ = secrets_path` (a candidate records the project, not the resolved
  path) and the `let _ = events.send(..)`/`let _ = channel.send(..)` sends, where a closed
  receiver is a window that went away and is not a failure of the run.

## Verification pass (2026-08-30) — what the gate actually reports here

Full numbers, measured not assumed, via the CLAUDE.md `npx tsc` + vitest-shim recipe
(fresh outDir; 42 `*Logic.test.ts` files):

**1192 passed / 11 failed.** Baseline before this feature was 1179 / 11 — the failure
count and the failing tests are *unchanged*, so nothing here regressed.

New modules, all clean: `sqlLogic` 72/0, `sqlViewLogic` 31/0, `sqlPickerLogic` 17/0,
`featuresLogic` 15/0, `askLogic` 34/0.

**The 11 failures are shim artefacts, not product bugs**, and all sit in files this work
never touched (`configLogic` 1, `intentPanelLogic` 1, `launcherLogic` 1, `runningLogic` 1,
`usagesLogic` 7). Root cause worth knowing before anyone "fixes" one: vitest's `expect`
takes an optional **message as its second argument** — `expect(needsShell(cmd), cmd)` —
but the shim's signature is `makeExpect(actual, negated)`, so that message is read as the
negation flag and inverts the assertion. Hence the tell-tale `expected true not to be true`.
Fixing the shim (accept and ignore a second string arg) would recover several of these.

Three non-`*Logic` test files cannot run under the harness at all, all untouched:
`language.test.ts` (imports `@codemirror/language`), `reexportGuards.test.ts` (needs vite's
`import.meta.glob`), `views/architecture/nodeTargets.test.ts` (needs a `--resolveJsonModule`
fixture import).

Rust, via the direct-toolchain path: `cargo fmt --all --check` clean;
`cb-core --lib sql::` **208 passed / 0 failed** (2598 filtered ⇒ 2806 lib tests, matching
the recorded baseline); `--test sql_sqlite` **23 / 0**; `cb-app` **119 / 0**.

Docs: `generate-index.mjs` regenerated `docs/INDEX.md` (459 files, 151 commands, 152 IPC
wrappers, 131 core modules) and is **idempotent** on a second run; all ten `sql_*` commands
are in the surface. `check-docs.mjs` passed (22 files).

Still unverifiable in this environment and **must not be claimed**: every `.tsx` —
`SqlView`, `SqlConnectionPicker`, `SqlResultGrid`, `AskPanel`, `FeaturesPicker` — because
React only resolves through the pnpm junctions. Confirmed by grep that no `.tsx` imports
`@codemirror/lang-sql` (the only mention is a comment in `SqlView.tsx` saying it is not a
dependency); every other import resolves to a package already in `package.json`.

## Full-gate run, 2026-08-30 — three harness facts that change the headline number

Running the whole logic suite through the `tsc` + vitest-shim harness gives
**1360 passed / 11 failed** (baseline was quoted as 1346 / 11). The extra 14 are not new
tests; they are the harness getting further than the run that produced the baseline. Three
files do not behave like the rest, and a tally that ignores them silently under-counts:

- `src/views/architecture/nodeTargets.test.ts` (**42 tests**) imports a cb-core JSON fixture.
  With the plain recipe it throws at `require` before the shim's exit handler prints, so it
  scores `0 passed, 0 failed` and looks like an empty file rather than a crash. It runs
  clean once compiled with **`--resolveJsonModule --esModuleInterop --rootDir .`**. Add
  those flags for this file (a whole-tree compile with `--rootDir .` relocates every output,
  so compile it separately).
- `src/components/language.test.ts` (**14 tests**) requires `@codemirror/language` — a
  junction, so genuinely unreachable here, same class as `.tsx`. Count it as not-run.
- `src/reexportGuards.test.ts` (1 test) uses `import.meta.glob`; Vite-only, unrunnable under
  commonjs. Count it as not-run.

**Always scan stderr per file, not just the summary line.** A file that dies at `require`
prints a summary of zero, so `MODULE-ERROR` detection keyed on a missing summary misses it.

### The 11 failures are shim gaps; here is the one that explains most of them

The shim's `makeExpect(actual, negated)` reads a **second argument as the `negated` flag**,
but vitest's second argument is a *message string* — which is truthy, so every
`expect(x, "why").toBe(...)` asserts the inverse and "fails" on correct code. That is
`launcherLogic > needsShell`, `runningLogic > hasOutput`, `configLogic`, `intentPanelLogic`
and the seven `usagesLogic` rows. None of those modules were touched by this feature, and
all six new/changed logic modules are green: `sqlLogic` 72, `sqlViewLogic` 38,
`sqlPickerLogic` 21, `askLogic` 34, `featuresLogic` 18, `terminalLogic` 46 — 0 failed.

---

## Phase 2/3 groundwork: MSRV corrected, two engines' dependencies added, driver seam widened

Done on 2026-08-30. Measured with the toolchain invoked directly
(`~/.rustup/toolchains/<tc>/bin/cargo.exe`, `RUSTC` pinned) — the `~/.cargo/bin` shim is
`Permission denied` in this environment.

### `rust-version` is now `"1.88"`, and it is now true

The section above ("the `rust-version = "1.82"` promise is ALREADY BROKEN") recorded the
measurement but left the decision open. It is taken: the root `Cargo.toml` now says
`rust-version = "1.88"`, with a comment recording that it was measured, that 1.82 could not
build the workspace at all, and how to re-measure. **This restricts nothing that previously
worked**, because 1.82 did not work — `cargo +1.82 check --workspace` died parsing
`time-core 0.1.9` / `hashbrown 0.17.1` (`feature edition2024` unstabilised) before compiling
a line of this repository.

Verified verbatim, after the dependency additions below:

```
$ rustup toolchain install 1.88
  1.88-x86_64-pc-windows-msvc installed - rustc 1.88.0 (6b00bc388 2025-06-23)

$ cargo +1.88 check --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s)   # exit 0
```

So 1.88 is not merely the declared floor of the lockfile — it actually builds this
workspace, drivers' dependencies included.

### The two drivers' dependencies

| Change | Where |
|---|---|
| `"postgres"` added to the existing `sqlx` feature list | root `Cargo.toml` (`default-features = false`, still pinned `0.8.6`) |
| `tiberius = "0.12.3"`, `default-features = false`, features `["tds73", "rustls", "sql-browser-tokio"]` | root `Cargo.toml` + `crates/core` |
| `tokio-util = "0.7"`, feature `["compat"]` | root `Cargo.toml` + `crates/core` |
| `tiberius` re-declared with `features = ["winauth"]` under `[target.'cfg(windows)'.dependencies]` | `crates/core/Cargo.toml` |

**Measured cost** (Windows, warm registry, shared `target/`):

| | Before | After |
|---|---|---|
| packages in `Cargo.lock` | 576 | **626** (+50) |
| `cargo check -p cb-core --all-targets` | 31.7 s (warm) | **1 m 24.8 s** (cold for the new crates) |

`cargo check --workspace --all-targets` afterwards: **1 m 05 s** warm. For comparison, the
earlier sqlx addition was 34 crates / 48 s.

**Why each tiberius feature.** `default = ["tds73", "winauth", "native-tls"]`, and all three
defaults are dropped so the two problematic ones can be replaced:

- `native-tls` → **`rustls`**. rustls is already in the tree twice (reqwest, sqlx), and
  native-tls means schannel on Windows / an OpenSSL build on Linux.
- `winauth` is **Windows-only** — the `winauth 0.0.4` crate it enables does not build off
  Windows — so it is re-enabled under `[target.'cfg(windows)'.dependencies]`. Cargo unions
  features per target, so a Windows build gets integrated authentication (`cargo tree -p
  tiberius` shows `winauth v0.0.4` here) and a Linux build still builds. This is the whole
  reason `crates/core/Cargo.toml` declares tiberius twice.
- `tds73` re-enabled: it is the TDS 7.3 protocol level, and without it `date`, `time`,
  `datetime2` and `datetimeoffset` columns cannot be read from any modern SQL Server. Not
  optional in practice.
- `sql-browser-tokio` added: resolves a **named instance** (`Server=host\SQLEXPRESS`) via
  the SQL Browser's UDP port. Without it only `host:port` can be opened, and a named
  instance is the ordinary shape of a local developer install.

**A cost worth knowing before the mssql phase starts:** tiberius 0.12.3's `rustls` feature
pins `rustls 0.21.12` / `tokio-rustls 0.24.1` / `rustls-native-certs 0.6.3`, so the lockfile
now carries **two rustls majors** (0.21 alongside the existing 0.23.43). That is a duplicate
TLS stack in the binary and an old one. It is not fixable inside tiberius 0.12.3 — the
alternative is `native-tls`, which trades it for schannel/OpenSSL. Recorded, not hidden.

### The driver seam (`crates/core/src/sql/driver/mod.rs`) — no engine implemented

Neither tiberius nor postgres connects yet. What changed is the shape the two engine files
plug into.

**`ReadOnlyEnforcement` went from three variants to six**, because the three engines enforce
read-only three genuinely different ways and one is much weaker:

| Variant | Engine | Does a write run? |
|---|---|---|
| `Driver` | SQLite — handle opened `SQLITE_OPEN_READONLY` | no |
| `ServerSession` | PostgreSQL — the server holds the session read-only | no |
| `RolledBackTransaction` | SQL Server — **no read-only session mode exists**; each statement runs in a transaction that is always rolled back | **yes, then it is undone** |
| `GuardOnly` | — nothing on the connection enforces anything | yes |
| `NotEnforced` | — writes were allowed | yes |
| `Undetermined` | — engine never determined, so no driver to ask | not known |

`Driver`'s `label()` is **unchanged verbatim** (`"Opened read-only by the driver"`) because
`src/views/sqlViewLogic.ts` mirrors it and a vitest pins the string.

New API, all in `driver/mod.rs`:

- `label()` (short) and `detail()` (the mechanism, and what it does not cover).
- `strength() -> EnforcementStrength` — `Refused` / `Undone` / `HeuristicOnly` / `Nothing` /
  `Unknown`. `Refused` and `Undone` are separate arms with **no ordering between them**:
  "never runs" and "runs and is reverted" must not be ranked as near-neighbours.
- `refuses_writes_before_they_run()` — true for `Driver` and `ServerSession` only.
- `is_driver_enforced()` kept, narrowed in the docs to the literal fact "the handle was
  opened read-only" (SQLite's mechanism and nobody else's), so it is **false** for
  `ServerSession` even though that is just as strong.
- `declared_read_only_enforcement(SqlEngine)` — the single place each engine's real guarantee
  is written down. A driver defers to it (SQLite now does) so it cannot drift into claiming a
  mechanism it does not use.
- `expected_enforcement(Option<SqlEngine>, writes_allowed)` — answers before anything is
  opened. `writes_allowed` ⇒ `NotEnforced`; engine `None` ⇒ `Undetermined`, never the
  weakest or the strongest real answer.
- `ENGINES` — the array the total-mapping tests iterate.
- **`for_engine(SqlEngine) -> Option<Box<dyn SqlDriver>>`** — the registry, **moved down**
  from `src-tauri/src/commands/sql.rs::driver_for`, which is now a one-line delegation. It
  moved so it can be tested: a `#[tauri::command]` module cannot be, and "registered under
  the wrong engine" is precisely the mistake that would open a SQLite file for a PostgreSQL
  DSN.

Tests added in `driver/mod_tests.rs` (all six new ones green):

- no two variants share a `label()` **or** a `detail()`;
- each engine gets its own wording, and the three do not read alike;
- **SQL Server's wording never claims to be a sandbox** — it must contain "not a sandbox"
  and "rolled back", and must not contain `"before it runs"`, `"refuses a write"`,
  `"by the driver"` or `"read-only session"`;
- exactly `Driver` and `ServerSession` carry the strong phrase "before it runs";
- an unknown engine stays `Undetermined` and borrows none of the other details;
- allowing writes is `NotEnforced` for every engine, including an unknown one;
- the registry never returns a driver whose `engine()` differs from the arm, and any
  registered driver's `read_only_enforcement(false)` equals its engine's declared policy
  (this is the drift alarm the two engine agents will trip if they hardcode).

The superseded `the_three_enforcement_answers_never_share_a_sentence` was **removed, not
weakened**: `no_two_enforcements_share_a_label_or_a_detail` runs the same comparison over all
six variants and over `detail()` as well.

### What the two engine agents still have to do

1. Add one file (`driver/postgres.rs`, `driver/mssql.rs`), one `pub mod` line, one arm in
   `for_engine`. Do **not** add a second arm to `declared_read_only_enforcement` — it is
   already total.
2. A driver may report `ServerSession` **only after the setting was accepted**. If
   `SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY` fails, the connect must **fail**;
   silently continuing as `GuardOnly` would downgrade a promise the user already consented to
   without telling them.
3. Both must go through `within_connect_deadline` — a connect with no deadline makes a stop
   report `Signalled` while nothing stops.

### Follow-up the frontend still owes (NOT done here, no TS agent in this phase)

`src/views/sqlViewLogic.ts::enforcementBadge` currently derives its tone from the engine
string, with `"sqlite"` → `driver` and **everything else falling through to `guard`**
("this driver has no read-only open mode"). Once either engine lands that fallback is wrong
for both: PostgreSQL is stronger than it says, and SQL Server is a different mechanism
entirely. It currently *understates*, so nothing unsafe ships in the meantime — but the
badge needs a fifth tone (`undone`) and a `serverSession` case, and the honest thing is to
stop mirroring strings and let `ReadOnlyEnforcement` cross IPC.

## Phase 3 — the PostgreSQL driver (2026-08-30)

- **The whole decode path is a pure function on purpose.** `PgValueRef` cannot be
  constructed outside sqlx, so a decoder written against it is untestable without a
  server. `postgres::decode_value(type_name, WireFormat, bytes)` therefore takes the
  three facts a `PgValueRef` carries and nothing else; `decode_cell` is the only
  function that touches a driver type and it decides nothing. 48 unit tests, no server.
- **The type map is keyed on the type, not on the wire format.** A type is decodable or
  it is not; which format a value arrived in must not change that answer. Both formats
  are implemented for every decodable type.
- **`sqlx::query(sql)` not `sqlx::raw_sql(sql)`.** `raw_sql` has no argument list, so
  sqlx takes the *simple query* protocol, which returns TEXT format **and** never
  resolves a custom type OID — a user-defined enum would reach the header as `?`.
  `sqlx::query` carries an empty argument list, which makes it a prepared statement:
  binary format, and `prepare` resolves custom type names. The text arm still exists
  because the format is data on the value.
- **`PgSslMode` implements neither `PartialEq` nor `Eq`**, so `PgKeywords` carries this
  crate's own `SslMode` and maps it at options-build time.
- **`rows_affected` is reported here and `None` in SQLite** — Postgres' `CommandComplete`
  describes *this* statement, `sqlite3_changes()` may describe an earlier one. It stays
  `None` when the loop ended early (a stop, or a cap), because *no count to report* is
  not `Some(0)`.
- **Read-only is verified, not assumed.** `SET SESSION CHARACTERISTICS AS TRANSACTION
  READ ONLY` is sent *and* `SHOW default_transaction_read_only` is read back; anything
  but `on` fails the connect. `ReadOnlyEnforcement::ServerSession` is a claim about the
  session *being* read-only, not about the `SET` having been sent. Mutation-checked: this
  is the one rule no unit test can catch (it needs a server) — `crates/core/tests/
  sql_postgres.rs::a_read_only_connection_has_the_server_refuse_a_write_...` is where it
  is covered, and that test has never been run.
- **`Debug` is hand-written for `PgTarget` and `PgKeywords`.** A derived one prints the
  password into every `{:?}`, every failed assertion and every `tracing` line.
- Numeric is decoded from Postgres' base-10000 binary form to a string (scale intact),
  and dates/timestamps from Hinnant's `civil_from_days` — no calendar crate, because
  adding a `chrono` sqlx feature to render a date is a far larger change than the
  arithmetic. `timetz` deliberately abstains: its binary zone field's sign convention
  cannot be verified here, and rendering an offset backwards is worse than saying so.

## Full-gate pass, 2026-08-31 — two real failures found and fixed

Both were **drift between `cb-core` and the command layer / lints**, not logic bugs.

1. **`cb-app` was red.** `commands/sql.rs::tests::only_sqlite_has_a_driver_in_this_build`
   asserted `driver_for(SqlEngine::Postgres).is_none()`, but phase 3 registered
   `PostgresDriver` in `driver::for_engine`. The core registry tests could not catch this —
   `a_registered_driver_reports_exactly_the_enforcement_its_engine_declares` and its sibling
   both `continue` past an unregistered engine, so **nothing in `cb-core` pins *which*
   engines are registered**; that `cb-app` test is the only place the set is stated.
   Renamed to `sqlite_and_postgres_have_drivers_in_this_build_and_sql_server_does_not`, with
   a message on each assertion. **Lesson: a tolerant loop is not a registry test.**

2. **`cargo clippy` was error-level, in `sql/`.** `approx_constant` is a deny-by-default
   *correctness* lint, so the two `3.14` literals in
   `driver/mssql_tests.rs::a_32_bit_float_is_not_widened_into_precision_it_never_had` were
   `error:`, not `warning:` — `cargo check` and `cargo test` are both clean, so only clippy
   sees it. The value is deliberate (`3.14f32 as f64 == 3.140000104904175`, verified) and is
   not pi, so it is silenced with `#[allow(clippy::approx_constant)]` plus a reason rather
   than changed.

### SQL Server is still unreachable from the app

`driver::for_engine(SqlEngine::SqlServer)` returns `None`. The driver and its 46 unit tests
exist; nothing above `cb-core` can open a SQL Server handle, so `sql_test_connection` on such
a profile answers `EngineUnsupported`. Phase 2 deferred the wiring; phase 3 wired Postgres and
left this arm alone. The renamed test above is now the alarm for it.

### `version_query`/`probe_query` never grew a Postgres arm

`commands/sql.rs` maps `SqlEngine::SqlServer | SqlEngine::Postgres => None` in both. So a
Postgres profile now *connects* (the driver is registered) and tests `Ok { server_version:
None }`. Honest — the abstain is real, not a lie — but `select version()` is known and the
missing arm is an omission rather than a decision. `probe_query` correctly stays `None`
(a client/server handshake already reached a real database).

### Measured numbers

`fmt --check` clean; `check --workspace --all-targets` clean; `cb-core --lib` **2916**;
`--tests` all green (`sql_sqlite` **23** passed, `sql_mssql` **24** ignored, `sql_postgres`
**19** ignored, each naming its env var in the skip reason); `cb-app` **119**; clippy **0
errors, 0 warnings under `sql/`** (6 warnings remain, all pre-existing and outside `sql/`
except `features/store_tests.rs:19`, which belongs to the optional-features work).

### MSRV

Root `Cargo.toml` now reads `rust-version = "1.88"`.
`cargo +1.88 check --workspace --all-targets` **passes, exit 0** (rustc 1.88.0, 6b00bc388
2025-06-23). The manifest comment's reasoning was re-read and matches: 1.82 could not parse
edition-2024 lockfile entries, so raising the floor restricts nothing that previously worked.

Nothing frontend was verified — pnpm is unrunnable here (junction block).
