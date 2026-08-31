# TODO — SQL console

## Blocked on the environment (not on the code)

- [ ] **Run the real frontend gate: `pnpm typecheck` and `pnpm test`.** Needs a session whose
      shell can traverse the pnpm junctions — see CLAUDE.md. The `npx tsc` + vitest-shim
      harness used so far covers the pure `*Logic.ts` modules **only, and can never cover a
      `.tsx`** (React resolves only through the junctions). So `SqlView.tsx`,
      `SqlConnectionPicker.tsx` and `SqlResultGrid.tsx` are **unverified — do not claim
      otherwise**. Current harness numbers: 1346 passed / 11 failed, where the 11 are
      pre-existing shim artefacts in untouched modules (baseline before this work was
      1179 / 11 — nothing regressed).
- [ ] **`pnpm docs:index`** from that same session, for the same reason.

## The MSRV decision — the user's to make, not an agent's

`rust-version = "1.82"` in the root `Cargo.toml` is **false**, and has been since before this
feature. The locked graph's real floor is **1.88**: 33 crates use edition 2024 (cargo ≥ 1.85
merely to *parse*), 61 declare an MSRV above 1.82, and several are **direct** dependencies —
`globset` (edition 2024), `uuid` (1.85), `reqwest` (1.85). `cargo +1.82 check` dies parsing
the lockfile at HEAD, so **`cargo +1.82` is useless as a gate until this is decided**.

**sqlx is not the cause.** Verified: sqlx 0.8.6 and its subcrates declare edition 2021 and no
`rust-version`; the single MSRV it adds anywhere is `crc 3.4.0` → 1.83.

Options, measured in `notes.md`: raise the floor to 1.88 (or current stable) and say so in the
commit — recommended, costs nothing at build time; or downgrade `tauri`'s transitive `time`,
plus `globset`/`uuid`/`reqwest`, across the graph to honour a floor nobody builds on.

## Remaining SQL phases

- [ ] **Phase 2 — SQL Server via `tiberius` 0.12.3.** Note **`winauth` is Windows-only**, so a
      Linux build gets SQL auth only; that is a user-visible capability difference and must be
      said, not silently degraded.
- [ ] **Phase 3 — PostgreSQL.**
- Both need, per driver: a `driver_for` arm, a `version_query` arm, a `probe_query` decision
  (`None` is right only if the handshake really reached a database — say why; this is exactly
  the HIGH finding above), a pass over `classify_connect_failure`/`classify_probe_failure`
  against the **real strings that driver emits** (needles written from memory are how two dead
  ones got in), the weaker `ReadOnlyEnforcement` wording (a test asserts the weak labels never
  say "by the driver"), and a check of whether the driver's error messages echo the DSN before
  writing any redaction test — the SQLite one proved nothing until it was rebuilt around the
  one message shape that does.
- [ ] Both need **env-gated integration tests** (a live server behind an env var, skipped when
      absent). **There is no CI**, so nothing runs them for you.

## Build/packaging

- [ ] **Stage 6 installers**: the forked NSIS template and the `.deb` seed.
- [ ] **A C toolchain is now required to build `cb-core`.** `sqlx sqlite` →
      `sqlx-sqlite/bundled` → `libsqlite3-sys/bundled`, non-optionally, so the SQLite C
      amalgamation is compiled by `cc` on every cold build. Previously only `git2` and `psm`
      needed one. Relevant to any CI image and to any cross-compile.

## Smaller, known gaps (carried forward)

- [ ] **A statement splitter, in `cb-core`.** `commands/sql.rs` runs the submitted text as one
      statement at index 0 and says so; a `;`-splitter is not acceptable (string literals,
      dollar-quoted bodies, `BEGIN … END`). Until then `statementIndex` is always 0.
- [ ] **`last_used_ms` is never written.** Left out because "used" wants a definition (opened?
      ran a statement? succeeded?) and a wrong one is worse than a null.
- [ ] **`sql_discover` cannot save a candidate.** Consider `sql_save_candidate(id)` so the
      `SecretSource` is copied by the backend rather than round-tripped through the frontend.
- [ ] **Nothing closes a connection.** `sql_execute` opens and drops one per statement, so
      `SqlSessions::stop_connection` is unused. A persistent-session model (needed for temp
      tables and transactions) is a design decision, not a gap to fill by reflex.
- [ ] **`sql_cancel` scans every connection** to find a query id. Fine at this scale.
- [ ] **`AttachConfirm` uses the shared `.modal-backdrop` at z-index 50**, below the terminal
      band, so a raised terminal floats over it. `SqlView` lifts its own copy; the general fix
      is still open.

## Done

- [x] Phase 0 (pure core), phase 1 (driver seam + SQLite + session), phase 1b (ten Tauri
      commands), the eleven review findings, and the SQL tab UI incl.
      `FEATURE_BY_TAB.sql = "sqlConsole"`. See `completed.md`.

## From the phase 2/3 dependency + seam groundwork (2026-08-30)

- [ ] **Frontend: `enforcementBadge` needs two more answers.** `src/views/sqlViewLogic.ts`
      derives the tone from the engine string and falls **everything that is not `sqlite`
      through to `guard`**. Once postgres/mssql land that is wrong for both — PostgreSQL is
      stronger than the badge says, SQL Server is a different mechanism (a rollback, not a
      refusal). Needs a `serverSession` case and an `undone` tone. It currently understates
      rather than overstates, so nothing unsafe ships until then.
- [ ] **Stop mirroring the Rust label strings in TypeScript.** Now that
      `ReadOnlyEnforcement` has six variants with `label()` + `detail()` + `strength()`, the
      honest fix is to let it cross IPC (`model.rs` key-pinning test + `types.ts`) instead of
      duplicating sentences and pinning them with a vitest.
- [ ] **Two rustls majors in the lockfile.** tiberius 0.12.3's `rustls` feature pins rustls
      0.21.12 beside the existing 0.23.43. Not fixable inside 0.12.3; revisit if tiberius
      ships a newer TLS stack.
- [ ] **`Cargo.lock` grew 576 → 626 packages.** If `target/` size becomes a problem again,
      this is part of why.

## From phase 3 (the PostgreSQL driver, 2026-08-30)

- [ ] **`crates/core/tests/sql_postgres.rs` has never been executed.** 19 `#[ignore]`d
      tests gated on `CB_SQL_PG_URL`. Run them against a real server before the console
      is believed: `cargo test -p cb-core --test sql_postgres -- --ignored
      --test-threads=1`. The read-only one is the load-bearing case.
- [ ] **Types that abstain and could later decode**: arrays, ranges, `money`, `timetz`,
      enums, composites, `inet`/`cidr`, `bit`/`varbit`, `xml`. Each is one arm in
      `postgres::decoder_for` plus a decoder; today each shows `Unsupported` by name.

## After phase 2 (SQL Server driver)

- [ ] **Register the driver**: add the `SqlEngine::SqlServer` arm to
      `cb_core::sql::driver::for_engine`, then fix the four `cb-app` tests in
      `src-tauri/src/commands/sql.rs` that assert SQL Server has no driver in this build
      (`driver_for(..).is_none()`, `Refusal::EngineUnsupported`, `SqlTestOutcome::
      EngineUnsupported`), and run `cargo test -p cb-app`. Phase 2 was scoped to the driver
      file and deliberately left this. `probe_query`/`version_query` need **no** change.
- [ ] **Run `crates/core/tests/sql_mssql.rs` against a real server** (`CB_SQL_MSSQL_URL`,
      `-- --ignored`). Nobody has ever run it. Expect to fix something.
- [ ] While doing that, **hunt for a tiberius/server message that carries the DSN** and pin
      it, the way `sql_sqlite.rs` pins sqlx's. Today mssql redaction is proved by
      construction, not by observation.
- [ ] Decide whether to enable a tiberius date feature (`chrono` or `time`). Until then every
      `date`/`time`/`datetime2`/`datetimeoffset` cell is `Unsupported { type_name }` — honest,
      but poor for a console. The pinning test is
      `a_temporal_value_abstains_rather_than_showing_a_derived_date`.
- [ ] `sqlViewLogic.ts::enforcementBadge`: add the `undone` tone for SQL Server rather than
      falling through to `guard` (carried over from phase 1).

## Found by the full gate, 2026-08-31

- [ ] **Wire `for_engine(SqlEngine::SqlServer)`** — the mssql driver is complete (46 unit
      tests) but unregistered, so nothing above `cb-core` can reach it. Registering it flips
      `commands/sql.rs::tests::sqlite_and_postgres_have_drivers_in_this_build_and_sql_server_does_not`,
      which is deliberate: that test is the alarm.
- [ ] **Give `version_query` a Postgres arm** (`select version()`), or write down why it
      abstains. Today a Postgres profile tests `Ok { server_version: None }`.
- [ ] **Add a `cb-core` test that pins the registered-engine set.** The two existing
      `for_engine` tests `continue` past an unregistered engine, so the set is pinned only
      in `cb-app` — which is how the phase-3 drift got through.
- [ ] `Cargo.toml`: the sqlx comment block is separated from the `sqlx = ...` line by the
      `futures-core` comment + entry, so it reads as documentation of `futures-core`.
