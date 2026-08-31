# Optional features: an install-time picker, a SQL console, and "Ask the codebase"

## Context

`code-basics` ships one fixed set of tabs and menus. Every user gets every
feature whether or not it suits their stack, and there is no way to say "I don't
do SQL" or "don't bind that chord". We want an **optional features** system:
during installation the user chooses which extra features to enable, and the app
honours that choice. Two features seed it:

1. **SQL console** — connect to a database, run queries, see results.
2. **Ask the codebase (Ctrl+/)** — a question box that opens a live terminal
   running `claude` or `codex` with the question already asked, so the answer
   lands in a session the user can keep talking to.

Outcome: adding a third feature is a small obvious addition, not another
hardcoded branch.

### The constraint everything follows from

**One binary ships all the code.** Neither installer can produce a
feature-reduced build, so an installer checkbox can only ever *write a
preference the app reads at startup*. The installer page is therefore a
front-end onto an ordinary settings store, and the same store is editable
in-app afterwards.

This is also what makes Ubuntu work: a `.deb` cannot show a checkbox page, so
the **in-app picker is the primary, cross-platform surface** and the Windows
NSIS page pre-seeds it.

---

# Part 1 — The features store and the installers

## 1a. `crates/core/src/features/` (new, pure)

Model directly on `crates/core/src/notes.rs`, the existing user-global store.

- `FeatureId` enum, stable string ids (`sqlConsole`, `askCodebase`), with `ALL`
  — the `review::ReviewAgent::ALL` shape.
- `FeaturesFile { version: u32, enabled: BTreeMap<String, bool> }`, camelCase.
- `features_path()` → `<config>/code-basics/features.json`, `CB_FEATURES_PATH`
  override. Copy `notes::notes_path()` exactly.
- `load(path)` turns a missing **or corrupt** file into defaults rather than
  erroring (the `notes::load` / `enhancements::runs` rule — a bad file must
  never stop the app starting).
- `save(path, &FeaturesFile)` atomic temp+rename with `.bak`, as `notes::save`.
- `merge_seed(user: Option<FeaturesFile>, seed: FeaturesFile) -> FeaturesFile`
  — pure, tested. **The seed applies only when there is no user file.** An
  upgrade must never silently re-enable something the user turned off.
- An **unknown id is kept, not dropped** (downgrade→upgrade must not lose a
  choice); an id the file omits falls back to the built-in default.
- **Default with no file and no seed: both features ON.** Justification: this is
  an existing app gaining capability, and a silent-off default makes a fresh
  install look broken to someone who never saw an installer (`cargo run`, a dev
  checkout, an AppImage). The installer's job is to let you turn things *off*.

Tests (`features_tests.rs`): `a_missing_file_loads_as_defaults` ·
`a_corrupt_file_loads_as_defaults_rather_than_erroring` ·
`a_seed_applies_only_when_there_is_no_user_file` ·
`an_unknown_feature_id_survives_a_round_trip` ·
`an_absent_id_falls_back_to_its_built_in_default` · `save_is_atomic`.

## 1b. Seed files, per platform

Both installers write the same JSON shape to a platform seed path; the app
merges it on first run when no user store exists.

| Platform | Seed path | Written by |
|---|---|---|
| Windows | `$INSTDIR\features.json` | the NSIS custom page |
| Ubuntu `.deb` | `/usr/share/code-basics/features.json` | `bundle.linux.deb.files` (static, all-defaults) |
| AppImage | none | AppImage supports only `files`, no scripts |

Seed lookup is a tested pure function (`seed_path_for(exe_dir, platform)`), not
a `cfg!` scattered through the command layer.

## 1c. Windows: forked NSIS template

**Verified** against `tauri-apps/tauri@dev`
`crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi` (977 lines), for
the Tauri 2.11.4 CLI / 2.11.5 runtime this repo pins:

- The config schema exposes `bundle.windows.nsis.template` and
  `nsis.installerHooks`.
- `installerHooks` is `!include`d at **line 35**, before every page declaration
  (MUI pages start at 169). A page declared in a hook lands *before* Welcome and
  the order is not controllable — **hooks are unusable for this**.
- The template already uses `nsDialogs` for its own `Page custom PageReinstall`
  (line 187), so no new tooling is needed.
- There is no `MUI_PAGE_COMPONENTS`, so the built-in components page is out too.

**Plan:** vendor the template as `src-tauri/installer/windows/installer.nsi`
with a header comment recording the exact upstream tag it was forked from and
the re-sync step. Insert an `nsDialogs` checkbox page after
`!insertmacro MUI_PAGE_DIRECTORY` (line 389), before `MUI_PAGE_INSTFILES`
(line 402); its leave function writes `$INSTDIR\features.json`. Point
`bundle.windows.nsis.template` at it.

> Knowingly accepted cost: this fork must be re-synced on Tauri upgrades. The
> header comment plus a line in `docs/getting-started/installation.md` are what
> keep that from being discovered the hard way.

## 1d. Ubuntu

**Verified** in the config schema: `bundle.linux.deb` supports
`preInstallScript`, `postInstallScript`, `files`, `depends`, `desktopTemplate`;
`appimage` supports only `files`. There is no route to the Debian *control*
archive, so **debconf templates are unavailable — a `.deb` cannot ask the
question.**

Ship an all-defaults seed via `deb.files` and rely on the in-app picker (1e).
`targets: "all"` already emits `.deb` + `.AppImage` + `.rpm` when built on
Linux; there is no cross-compilation from Windows, so the Ubuntu artefact needs
a Linux build host.

Two things to verify on the first Linux build, neither ever run here:

- **That it builds and runs at all.** The code is written cross-platform
  (`#[cfg(unix)] libc`, `pty/shell.rs` handles `$SHELL`, `git2`/`sysinfo`/
  `portable-pty` are portable), but no Linux build has ever happened.
- **The Objects tab will be inert.** `scripts/build-sidecar.mjs` publishes only
  `win-x64` and `win-x86` (lines 34–35), so a Linux bundle ships no
  `cb-inspector`. The app already reports that as unavailable rather than
  failing — a known gap, not a bug. Adding `linux-x64` to that RID array is a
  one-line change and is **out of scope here** unless you want it.

## 1e. In-app picker, and changing your mind later

The first-open modal already does *choose → preview → apply*:
`src/components/SetupPrompt.tsx` + `setupPromptLogic.ts` (dismissal in
`localStorage` per root) over `crates/core/src/setup.rs::setup_plan`. Add the
feature checkboxes there, and a permanent **Features** entry in
`src/components/MenuBar.tsx` (which today hardcodes
`type TopMenu = "file" | "enhancements"`) so the choice is changeable.

## 1f. Gating the UI

`TABS` is in `src/components/WorkspaceTab.tsx` line 32 (**not** `App.tsx`) over
`type Tab = "tests"|"run"|"changes"|"history"|"architecture"|"inspect"`. A new
tested `src/components/featuresLogic.ts` filters it:

- `visibleTabs(all, enabled)`
- `tabAfterDisable(active, visible)` — **the case with no obvious default**:
  when the active tab's feature is switched off, fall back to `"run"` rather
  than leaving a selected tab with no body.

The enabled set must be available **synchronously at render time** (loaded once
in `App.tsx`, passed down as a prop). An async check per render makes the tab
appear a frame late and flicker on every workspace switch.

## 1g. Commands

`src-tauri/src/commands/features.rs` — `list_features`, `set_feature`.
Following the `notes` / `launcher` precedent these take **no `AppState`**: the
store is user-global, not per-workspace. Register in `lib.rs`, document in
`docs/reference/commands.md`.

---

# Part 2 — Ask the codebase (Ctrl+/)

## 2a. The chord

Ctrl+/ is already taken: `src/components/FileEditor.tsx:700` binds `Mod-/` to
`toggleComment` ahead of `defaultKeymap`, and CLAUDE.md marks that as
load-bearing. **Decision: keep Ctrl+/ globally, but abstain while focus is
inside a CodeMirror editor.**

Split per the repo's rule (vitest runs in node — no DOM):

- `src/components/askLogic.ts` — `recogniseAskShortcut(event: ShortcutEvent)`,
  pure, over the structural event type `searchLogic.ts` already uses.
- `AskPanel.tsx` holds the DOM-touching half:
  `document.activeElement?.closest(".cm-editor")` → bail. Window-level,
  **capture-phase**, gated on `active` so only the foreground `WorkspaceTab`
  listens, with `if (event.repeat) return;` — the `SearchEverywhere.tsx:191-220`
  pattern exactly. The abstain rule needs a comment saying *why*, the way
  `searchLogic.ts` documents why Ctrl+F and Ctrl+A were not taken.

## 2b. Carrying a command into a terminal

Verified: `crates/core/src/pty/model.rs::PtySpec` already has `shell` and
`args`, and `PtyManager::open_inner` already honours them. Only the Tauri layer
hardcodes them (`commands/terminal.rs::terminal_open` sets
`shell: default_shell(), args: Vec::new()`).

- `terminal_open` gains `program: Option<String>`, `args: Option<Vec<String>>`.
- The defaulting decision goes in a tested free function **beside the existing
  `spec_cwd`** — `spec_program(requested, args) -> (String, Vec<String>)`,
  falling back to `default_shell()` with no args. Command bodies decide nothing.
- `src/ipc/api.ts::terminalOpen` gains the matching optional parameters.
- `terminalLogic.ts::TerminalDescriptor` gains an optional
  `command?: { program: string; args: string[] }`; `makeTerminal(seq, cwd)`
  keeps its signature and a sibling `makeAgentTerminal(seq, cwd, program, args,
  title)` produces the agent variant. `TerminalPanel.tsx` passes it through in
  its mount-once open effect.

**The question goes as an argv argument, not as typed keystrokes.** Two reasons,
both real: `PtyManager` spawns via `CommandBuilder` with `args` directly — no
shell, so no quoting or metacharacter hazard — and typing a multi-line question
into a TUI would submit at the first `\r`. There is also a race in the
keystroke approach (`TerminalPanel` resolves the session id asynchronously, and
bytes written before the TUI finishes starting get dropped). Argv avoids all of
it.

`pty/shell.rs::is_session_marker` already strips `CLAUDE_CODE_*`, `CLAUDECODE`,
`CLAUDE_PID`, `CLAUDE_EFFORT`, `AI_AGENT`, so a nested `claude` starts as a
clean top-level session. Nothing to add.

## 2c. Interactive argv

`crates/core/src/review.rs::agent_args(agent, mode, model, prompt)` builds
**headless** argv (`claude -p … --output-format stream-json --verbose`,
`codex exec --sandbox …`). That is the wrong posture — it is one-shot with
stdin closed, which is exactly the gap this feature fills.

Add `agent_args_interactive(agent, model, prompt) -> Vec<String>` in `review.rs`
with its own tests, reusing `program()`, `resolve_model` and `compose_prompt`:
`claude_interactive_argv_passes_the_prompt_positionally` ·
`codex_interactive_argv_is_not_the_exec_form` ·
`a_multi_line_question_stays_one_argument` ·
`the_headless_argv_is_unchanged`.

## 2d. When the agent is not installed

`review::detect_agents()` / `is_installed()` already resolve the program on
PATH. If the chosen agent is missing, **do not open a terminal**: the Ask panel
says which program name was looked for and where, per the abstain rule — the
`lsp/registry` precedent of reporting what was sought and how to install it,
never working around it. Tested in `askLogic.ts::launchBlockedReason(agents,
chosenId)`, which returns a **reason string, not a boolean**, so the disabled
Ask button can say why.

## 2e. Pieces

- `src/components/AskPanel.tsx` — small modal: question textarea, agent picker,
  Ask button. Model on `LauncherPicker.tsx` (input + keyboard nav), and use the
  shared `components/ContextMenu.tsx` if a menu is needed rather than a fourth
  hand-rolled copy.
- `src/components/askLogic.ts` + `askLogic.test.ts` —
  `recogniseAskShortcut` · `canAsk(question)` (non-empty after trim) ·
  `launchBlockedReason` · `terminalTitle(question)` (first ~40 chars, so the
  terminal tab is identifiable) · `askArgs(agent, model, question)`.
- **Agent preference is shared with the Review panel**: reuse `AgentPrefs` /
  `PREFS_KEY = "cb.agentPanel"` in `src/components/reviewLogic.ts`
  (`preferredAgentId`, `preferredModel`, `saveAgentPrefs`). One "my agent"
  across the app; the Ask window writing it is the intended behaviour.
- `WorkspaceTabHandle` gains `openAskTerminal(question, agentId, model)`,
  joining `openTerminal` / `openRunAgent` / `openReview` / `openNoteInAgent`.
- Optional titlebar button in `App.tsx` beside `+ Terminal`.
- **Feature gate:** when `askCodebase` is off, the key listener is never
  registered and the button is not rendered — the chord returns to the editor
  cleanly.

---

# Part 3 — SQL console

Read-only by default with per-connection opt-in writes; connections both
user-entered and discovered; SQL Server, PostgreSQL, SQLite.

## 3a. Three sentences that must survive review

1. **The new value-reading path lives outside `architecture/`, and the graph's
   refusal is unchanged.** Today `signals/dotnet.rs::connection_strings` reads
   only `section.keys()`, and `framework.rs::screen` +
   `contains_secret_assignment` discard connection-string text *whole*
   (`Discarded::label` is `None` for `SecretValue`). None of that moves.
2. **The read-only guard is a text heuristic and says so in its own refusal
   message** — pinned by a test asserting the message text, so it cannot be
   softened into "safe" or "sandboxed" by someone who does not know why.
3. **The connection string crosses IPC in one direction only** and never enters
   `.code-basics/`, git, a graph, a log, or an error message.

## 3b. Dependencies — phased, so no phase throws work away

| Phase | Added | ~crates | What it proves |
|---|---|---|---|
| 0 | `sqlparser 0.52` | ~2 | Pure Rust, no C, no TLS. The guard, model, discovery, store and grid — most of the work and **all** the tested decisions — need nothing else. |
| 1 | `sqlx 0.8` `default-features = false`, `["runtime-tokio-rustls","sqlite"]` | ~130-160 (incl. `libsqlite3-sys` C build) | The whole vertical slice with **no server, no credentials, no network** — a `tempfile` `.db` is a real integration test. |
| 2 | `tiberius 0.12` `["rustls","tds73","winauth"]` + `tokio-util ["compat"]` | ~60 | SQL Server. sqlx has never supported MSSQL. `winauth` is not optional in practice — real .NET strings carry `Trusted_Connection=True`. |
| 3 | add `"postgres"` to sqlx | ~25 | PostgreSQL, near-zero incremental design cost. |

`default-features = false` on sqlx is **mandatory**: the default features
include `macros`, whose `query!` forms need a live `DATABASE_URL` *at compile
time*. A console builds SQL at runtime; that would make `cargo build` depend on
a reachable database.

**Honest build cost, to be measured and recorded in `.memories/`, not trusted:**
whole-program LTO over ~250 extra crates, estimated +60–90 s cold debug build of
`cb-core` and a materially longer release relink. This repo has a documented
history of `target/debug/deps` reaching 45 GB across eight test targets — do the
`rm -rf target/debug` maintenance after the phase-1 spike.

**Check MSRV first:** `rust-version = "1.82"`. Run `cargo +1.82 check` against
`sqlparser`, `sqlx 0.8` and `tiberius 0.12` *before* phase 0 lands.

## 3c. Where connection strings live

**User-global, never per-workspace:** `<config>/code-basics/sql-connections.json`,
`CB_SQL_CONNECTIONS_PATH` override, tolerant `load`, atomic `save` — the
`notes.rs` / `launcher/store.rs` shape. Each entry records the `workspace_root`
it was created under so the picker groups *this codebase first* with no second
store (`launcher::recents::group` is the precedent).

`config::IGNORED` would technically work, but a gitignored secret *inside* a
repository is one `git add -f` away from being shared — and `.code-basics/` is
explicitly the directory this app shares with the team.

**Discovered connections store a reference, not a value:**

```
enum SecretSource {
  Literal     { connection_string: String },       // user typed it
  AppSettings { path: PathBuf, key: String },      // re-read at connect time
  UserSecrets { project: PathBuf, key: String },
  DotEnv      { path: PathBuf, key: String },
}
```

The app then stores no secret at all in the common case, and a rotated password
just works. Copying a value into the store is a separate explicit action.

**What this protects:** the value never enters `.code-basics/`, git, an
`ArchGraph`, a diagram, an intent record, or a log; and it **never crosses IPC
toward the frontend** — the wire type carries a redacted display form
(`engine`, `server`, `database`, `authMode`) computed in `sql/dsn.rs`. Driver
errors routinely embed the whole DSN, so every one passes through
`dsn::redact` before becoming a `SqlEvent::Failed`.

**What this does not protect — and the UI must say so next to the field:** the
store is **plaintext JSON on disk**, readable by any process running as this
user. No keychain, no DPAPI, no encryption. Adding one is a three-platform
project with a migration and a recovery story, and is out of scope; pretending
otherwise would be worse than the plaintext.

Accepted cost: with no read path, a saved connection string cannot be viewed or
edited in place — editing replaces it write-only.

## 3d. Discovery — `crates/core/src/sql/discover.rs`

Strictly outside `architecture/signals/`. Filesystem only, never the network,
like `secrets.rs`. A producer of *candidates*; nothing in `architecture/` calls
it.

| Source | Read |
|---|---|
| `appsettings*.json` beside a `.csproj` | the `ConnectionStrings` object **and** flat `ConnectionStrings:Name` keys — reuse `secrets::strip_jsonc` (promote to `pub(crate)`; do not copy it) |
| .NET user secrets | `secrets::read(project).content`, same key shapes — already exposed |
| `.env`, `.env.local`, `.env.development` beside a `package.json` | `DATABASE_URL`, `*_CONNECTION_STRING`, `PG*`, `MSSQL*` — new pure `sql/dotenv.rs`, **no interpolation** |

**Engine guessing needs two agreeing signals:** the package tables
(`signals/dotnet.rs` `Npgsql`→PostgreSQL etc., `signals/node.rs` `pg`,
`better-sqlite3`) and the string itself (`sql/dsn.rs`: `postgres://`,
`Initial Catalog=`, `Data Source=*.db`). **When they disagree or neither
matches: `engine: None`** — listed, not connectable until the user picks. Never
a default, never "probably SQL Server".

Move the package-table constants down into `adapters/dotnet.rs` /
`adapters/node.rs` so the dependency runs `signals → adapters` and
`sql → adapters`, never `sql → signals`. Prove no behaviour change with
`cargo test -p cb-core signals::`.

**Abstains on:** non-string values; unresolved placeholders (`${...}`, `%VAR%`,
`{{...}}`) → `Unresolved { reason }`, visible but not connectable; empty values;
unparseable files → a warning naming the file, never a silently shorter list
(the `workspace.rs::unreadable` precedent). **The same name in two files does
not collapse** — `appsettings.json`, `appsettings.Development.json` and user
secrets each yield a separate labelled candidate, because real precedence
depends on `ASPNETCORE_ENVIRONMENT`, which this module cannot know. Discovery
**never auto-saves and never auto-connects.**

A warning may name a file, project, package or key and **may never contain text
read out of a value** — `DotnetSignals::warnings`' rule, kept verbatim.

The test that pins the deliberate asymmetry:
**`discovery_returns_values_that_the_graph_refuses`** — one `appsettings.json`
fixture through *both* paths, asserting the first yields only keys and the
second yields values, with
`framework_tests::a_connection_string_value_never_reaches_the_graph` still
passing untouched. It fails loudly if anyone later "unifies" the two.

## 3e. The read-only guard — `crates/core/src/sql/guard.rs`

```
enum Verdict { ReadOnly, Write { kind }, Refused { reason } }
```

**Three outcomes, never two.** "This is a write" and "I could not tell" are
different facts; collapsing them makes a parser gap look deliberate and hides
the bug report.

1. Split the batch — T-SQL `GO` is a *client* separator, refused by name in
   read-only mode (it also takes a repeat count).
2. `sqlparser::Parser::parse_sql` with the per-engine dialect. **A parse failure
   is `Refused`, never a pass-through.** This kills the whole "comment hides a
   keyword" / "keyword inside a string literal" / "bracketed identifier named
   `delete`" / "`$$…$$` dollar-quoted body" class that a regex cannot.
3. Classify every statement; the batch is as strict as its strictest.
   `Query` is read-only **unless** `SELECT … INTO`, a data-modifying CTE
   (`WITH x AS (DELETE … RETURNING *) SELECT * FROM x`), or a denylisted
   function (`xp_cmdshell`, `sp_executesql`, `OPENROWSET`, `pg_read_file`, …).
   `Explain` is read-only **except `analyze: true`** — `EXPLAIN ANALYZE INSERT`
   actually executes. Unknown variant → **`Refused`, not `Write`**.

**The guard is the message; the engine setting is the enforcement**, and it
differs per engine — so the badge text differs per engine:

| Engine | Enforcement | Wording |
|---|---|---|
| SQLite | `read_only(true)` on the driver | "Opened read-only by the driver." |
| PostgreSQL | `SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY` | "The session is read-only; the server refuses writes." |
| SQL Server | none exists — statements run in an always-rolled-back transaction | "Rolled back, not sandboxed: `xp_cmdshell`, a linked server or an autonomous procedure can still act." |

**Worth knowing before phase 2: the priority engine is the one where the engine
cannot enforce read-only.** If read-only enforcement matters more than SQL
Server does, that ordering is worth revisiting.

Tests: `a_plain_select_is_read_only` · `a_cte_containing_an_insert_is_a_write` ·
`select_into_creates_a_table_and_is_a_write` · `a_comment_cannot_hide_a_keyword`
· `a_keyword_inside_a_string_literal_is_not_a_write` ·
`sql_that_will_not_parse_is_refused_not_allowed` ·
`an_unclassifiable_statement_is_refused_rather_than_called_a_write` ·
`explain_is_read_only_but_explain_analyze_is_not` ·
`the_go_batch_separator_is_refused_in_read_only_mode` ·
**`a_refusal_says_it_is_a_text_heuristic_and_not_a_sandbox`** (asserts the
message text) · plus the batch, `PRAGMA`/`ATTACH`, empty-statement and
writes-allowed cases.

Do **not** try to make it exhaustive — a stored procedure body is unknowable.
Keep the refusal list short and honest and let the engine setting carry weight.

## 3f. Module layout — the pure/impure split

```
crates/core/src/sql/
  model.rs   dsn.rs   guard.rs   format.rs   dotenv.rs      PURE (+ *_tests.rs)
  discover.rs   store.rs                                    fs only
  session.rs                                                registry + cancel
  driver/mod.rs  sqlite.rs  mssql.rs  postgres.rs           the ONLY sockets
```

The rule, written in `driver/mod.rs` the way `behavioral/replay.rs` and
`lsp/transport` write theirs: **`sql/driver/` is the only module here that opens
a socket or a database handle; everything above it is pure and tested with no
server installed.** A driver contains no `match` deciding a rendering — it hands
`format.rs` a typed value and `format.rs` decides.

`session.rs` mirrors `PtyManager`: a clone-cheap handle over a map keyed by
connection and query id, holding cancellation senders. It becomes an eighth
`AppState` field and, like `pty`, is **not** cleared per workspace.

## 3g. Result model and IPC

```rust
#[serde(tag = "kind", rename_all = "camelCase")]
enum SqlValue {
  Null,
  Text  { text: String, truncated: bool },
  Number{ text: String },                 // a STRING — see below
  Bool  { value: bool },
  Bytes { hex: String, byte_length: u64, truncated: bool },
  Unsupported { type_name: String },      // this build cannot decode it
  Unavailable { reason: String },         // the driver errored on this cell
}
```

**Three distinct renderings stay three distinct JSON shapes** — SQL `NULL`, an
empty string, and a truncated value. Pinned by
`null_empty_string_and_truncated_are_three_different_shapes`; rendered by
`sqlLogic.cellRender`, not in JSX. This is `inspect::model::ObjectValue`'s
discipline applied to the same problem.

**Numbers cross as strings.** `NUMERIC(38,10)`, `bigint`, `money` and SQL Server
`decimal` all lose precision through a JavaScript number, and a rounded value in
a console is a wrong answer rendered confidently — the same argument
`inspect/model.rs` makes for heap addresses crossing as hex.

`Unsupported` is the honest landing place for sqlx's hardest hidden cost:
Postgres uses the **binary** wire protocol, so `uuid`, `jsonb`, `numeric`,
`interval`, arrays and enums need matching features or per-OID handling. What
this build can decode it decodes; what it cannot renders as
`Unsupported { type_name }` — never blank, never guessed.

```rust
struct SqlResultSet {
  columns: Vec<SqlColumn>, rows: Vec<Vec<SqlValue>>,
  row_cap: Option<RowCap>,        // None => you have every row
  rows_affected: Option<u64>,     // Some(0) and None stay distinct
  elapsed_ms: u64, statement_index: usize,
}
struct RowCap { limit: u32, reason: RowCapReason }  // RowLimit | ByteLimit
```

**A cap is reported, never silently applied** (`ObjectValue::Elided`
precedent): a final *row* — not a toast, which scrolls away — reads "stopped at
1,000 rows — row limit — more rows exist", with a fetch-more that raises *the
limit that actually bit*, the way `Caps::widened` does. **Two caps, both live:**
1,000 rows *and* an ~8 MiB byte budget, because a wide row blows the byte budget
long before the row count bites.

**Streaming via `Channel<SqlEvent>`** (`Started`/`Columns`/`Rows`/`Completed`/
`Refused`/`Failed`/`Finished{cancelled}`), the `startRun`/`ProcessEvent` and
`inspectCapture` precedent. A console's defining failure is the 40-second query
and the 900 MB result: streaming gives first rows immediately, gives cancel
something to cancel, and applies the cap at the edge with the partial result
already delivered.

**Cancellation, honestly:** `sql_cancel(queryId)` fires a `watch` the row loop
`select!`s on. A true server-side cancel needs Postgres' second-socket cancel
request or SQL Server's attention packet, which tiberius does not expose — so
the terminal event says **"stopped"**, not "cancelled on the server". Three
terminal states (finished / stopped / failed) stay three.

**Commands** (`src-tauri/src/commands/sql.rs`): `sql_list_connections` ·
`sql_discover(root)` · `sql_save_connection` · `sql_delete_connection` ·
`sql_set_allow_writes(id, bool)` (a separate verb — it is the consent action) ·
`sql_test_connection` · `sql_execute(queryId, connectionId, sql, channel)` ·
`sql_cancel(queryId)`. `sql_test_connection` returns a **variant enum, not a
bool** — `Ok{server_version}` / `AuthFailed` / `Unreachable` / `TlsFailed` /
`Timeout` / `EngineUnknown` / `SecretUnresolved` — the `lsp::Availability` rule.
The decision extracts to a tested free function
`execution_plan(connection, sql, allow_writes)` beside the command
(`commands/lsp.rs::status_for` is the worked example).

Key-pinning lives in `sql/model_tests.rs` (the `inspect/model.rs` and `notes.rs`
subsystem precedent), with `src/ipc/types.ts` updated in the same edit:
`a_connection_never_serialises_its_connection_string` ·
`a_big_integer_crosses_as_a_string_not_a_number` ·
`no_field_whose_absence_means_something_is_skip_serialized` · and the three-shape
test above.

## 3h. UI

- **Tab:** add `"sql"` to `type Tab` and `{ id: "sql", label: "SQL" }` to
  `TABS`, **mounted while hidden** (`hidden={tab !== "sql"}`) — it owns live
  connections and a running query, so it joins Run/Tests/Objects, not
  Changes/History/Architecture. Gated on `FeatureId::SqlConsole` via 1f.
- **`src/views/SqlView.tsx`** — connection bar / editor / results. Needs
  `pnpm add @codemirror/lang-sql` (siblings already installed). Wire like
  `FileEditor.tsx`; keep `@codemirror/search`; bind **Ctrl+Enter = run**,
  **Ctrl+Shift+Enter = run selection**, with `preventDefault` **ahead of**
  `defaultKeymap` (the Ctrl+/ lesson), and the caret fix
  `&.cm-focused .cm-cursor { display: block }`.
- **`src/views/sqlLogic.ts` + test** — every decision: `applyEvent(state,
  event)` (the streaming reducer, `reviewStreamLogic.ts` precedent) ·
  `cellRender(value)` (the five-way NULL/empty/truncated/unsupported/unavailable
  discipline) · `columnWidths` · `capNotice` · `groupConnections(views, root)`
  (this-codebase-first) · `runDisabledReason(state)` returning a **reason, not a
  boolean** · `statusLine(status)` (`lspStatusLogic.ts` precedent).
- **`src/components/SqlResultGrid.tsx`** — plain divs on the `RunningPanel`
  idiom (`.row` at styles.css:455). No `<table>`, **no virtualization library**
  — the cap already bounds the DOM. A `CaptureHeader`-style strip carries
  elapsed time, row count and the cap notice.
- **`SqlConnectionPicker.tsx`** — saved connections this-codebase-first, plus a
  separate **"Found in this codebase"** section, *offered and never
  pre-selected*, each showing its source file and key, unresolved ones visibly
  unconnectable. Per-row menu via the shared `components/ContextMenu.tsx`.
- **Writes opt-in** behind an explicit confirm modelled on `AttachConfirm.tsx`,
  naming the connection and stating what the guard is and is not.
- **Cancellation** — a Stop button calling `sql_cancel(queryId)`, with the
  `queryId` **minted in the frontend** (the `AppOutputPanel` reason: first
  events land before `invoke` resolves).
- Do **not** reuse `OutputConsole` for the message list — its severity
  inference and rebuild machinery exist for streamed process output; here it is
  twenty lines of divs.

---

# Ordered implementation

**Stage 1 — features system.** `features/` module and tests → commands →
`featuresLogic.ts` + gating → SetupPrompt checkboxes + MenuBar entry. Ship with
both features defaulting on and nothing yet to gate; verify the store
round-trips and a disabled tab disappears.

**Stage 2 — Ask the codebase.** `agent_args_interactive` + tests →
`terminal_open` program/args + `spec_program` + test → api/descriptor/panel
plumbing → `askLogic.ts` + tests → `AskPanel.tsx` → gate it.

**Stage 3 — SQL phase 0** (pure core, `sqlparser` only): model → dsn → guard →
format → dotenv → store → adapters table move → discover. **Gate: `cargo test
-p cb-core` green with zero database software installed.**

**Stage 4 — SQL phase 1** (SQLite end to end): sqlx → `driver/sqlite.rs` →
`crates/core/tests/sql_sqlite.rs` over a `tempfile` db → session/AppState →
commands → types.ts/api.ts → `sqlLogic.ts` tests → views → tab.

**Stage 5 — SQL Server**, then **PostgreSQL**. Integration tests
`#[ignore]`-gated on `CB_SQL_MSSQL_URL` / `CB_SQL_PG_URL` (no CI here, so
env-gated is the right shape).

**Stage 6 — installers.** Fork and vendor `installer.nsi` with the nsDialogs
page; `deb.files` seed; `docs/getting-started/installation.md`. Last, because
it is a thin front-end onto Stage 1 and needs both features to exist to be
worth testing.

---

# Verification

- **Rust loop:** `cargo test -p cb-core sql::` / `features::` / `review::`.
  **Before done:** `cargo test -p cb-core` **from Git Bash** — the `process::`
  tests spawn `sh`, the suite takes about a minute, and stalling in `process::`
  is the documented false alarm, not a hang.
- **Frontend:** `pnpm typecheck` · `pnpm test` · `pnpm coverage` (≥70% over
  logic modules — `sqlLogic.ts`, `askLogic.ts`, `featuresLogic.ts` must carry
  real tests). Note the known junction hazard: if hundreds of
  `Cannot find module 'react'` errors appear in untouched files, that is the
  unreadable pnpm junction, and the frontend gate must be run by you rather than
  reported green.
- **Rust coverage:** `cargo llvm-cov --workspace --summary-only
  --fail-under-lines 70`. The driver and command modules will pull it down;
  judge them by whether the untested lines *decide* anything — they must not.
- **Manual, cheapest first:** SQLite needs no server. Point the app at any `.db`
  and walk discover → save → refuse a write → enable writes → insert → hit the
  cap → cancel. Then `docker run mcr.microsoft.com/mssql/server`, then
  `postgres:16`.
- **Ask:** Ctrl+/ in the Run tab opens the panel; Ctrl+/ **inside an open file**
  still toggles comments; a multi-line question reaches the agent intact; an
  uninstalled agent names what was looked for instead of opening a terminal.
- **Installers:** `pnpm tauri build` on Windows, run the MSI/NSIS, confirm the
  feature page appears after the directory page and that `$INSTDIR\features.json`
  matches the choice and is honoured on first run. The `.deb` needs a Linux
  build host and is the first Linux build this project has ever had.
- `pnpm docs:index` after new files; `pnpm docs:check` (docs stay under 500
  lines, links must resolve); `docs/reference/commands.md` must track
  `generate_handler!`.
- `.memories/features/{optional-features,sql-console,ask-codebase}/` updated
  throughout, with the **measured** build-time and `target/` size deltas in
  `notes.md`.

---

# Flagged, before you approve

1. **The SQL half is much larger than the other two combined.** Stages 1 and 2
   are days; the SQL console with three engines is weeks. Stages 1+2+3 are a
   coherent shippable increment on their own.
2. **The NSIS fork is a maintenance debt** re-paid on every Tauri upgrade.
3. **Ubuntu cannot ask the question.** The `.deb` gets defaults; the in-app
   picker is the real surface there — which is why Stage 1 comes first.
4. **The Ubuntu build has never been attempted**, and its Objects tab will be
   inert until the sidecar gains a `linux-x64` RID.
5. **SQL Server, the priority engine, is the one that cannot enforce read-only.**
6. **Connections are stored as plaintext JSON.** Called out here so it is a
   decision rather than a discovery.
