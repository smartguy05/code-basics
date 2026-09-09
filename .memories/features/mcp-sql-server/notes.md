# Notes — issues, gotchas, lessons

## `Refusal::to_event()` could not stay a method

The plan says `to_event()` stays in the bridge. Once `Refusal` moved to
`cb-core`, an inherent `impl` in `src-tauri` is an orphan-rule violation. It is
now the free function `commands::sql::refusal_event(&Refusal) -> SqlEvent`,
which keeps the actual property that mattered: the wrapping (and the redacting
`failed_event` constructor) stays in the bridge, and the *words* come from
`Refusal::sentence()` in `cb-core`, so the console and an agent cannot describe
one refusal two different ways.

## The stale supported-engine list (fixed)

`Refusal::EngineUnsupported`'s sentence hard-coded *"SQLite is the only engine
currently supported"*. It was written when it was true; Postgres and SQL Server
drivers shipped afterwards (`driver::ENGINES` has three, `driver::for_engine`
answers for all three) and the sentence kept saying it. It is now **derived**
from `driver::ENGINES` filtered by `for_engine`, pinned by
`engine_unsupported_names_the_engine_and_not_a_stale_supported_list` and
`the_unsupported_sentence_lists_exactly_the_engines_this_build_can_open`.

Lesson worth generalising: a sentence that enumerates a capability must be
computed from the thing that provides the capability, not typed beside it.

## Identifier validation is NOT parameter binding

Every catalog arm interpolates schema/table into a **string-literal** position,
so `''` doubling is the correct escape *for a literal*. It is not sufficient:

- PostgreSQL with `standard_conforming_strings = off`, or an `E''` string,
  treats `\` as an escape, so `\'` defeats the doubling. Pinned by
  `a_backslash_is_refused_even_though_the_quote_would_be_doubled`, which also
  asserts that `sql_literal` really *would* have produced `\''` — that is why
  the first layer has to exist.
- A NUL can truncate inside the client library after this crate decided the
  string was safe.
- These names now arrive from an **agent**.

`catalog::identifier` refuses (empty, >128 bytes, NUL, `'`, `\`, CR/LF, other
control) naming which rule bit; the doubling stays underneath. The durable fix
is `run_statement_with_params` — see `todos.md`. Do not let a later reader read
validation as binding; the module docs say so explicitly.

The length limit counts **bytes**, not characters, and
`the_length_limit_counts_bytes_and_not_characters` catches the `chars().count()`
mutation.

## The two consents are orthogonal, and must stay so

`allow_writes` and `expose_to_agents` never move each other
(`the_two_consents_do_not_move_each_other`). The agent path forces
`writes_allowed: false` regardless of `allow_writes`, so no combination lets an
agent write. Deriving one from the other anywhere would make that forcing look
like the only thing standing between an agent and a write.

`upsert` guards exposure in **both** directions: a payload cannot raise it and
cannot revoke it. Only `set_expose_to_agents` moves it. The revoke half matters
— a save carrying `false` silently un-exposing would make the toggle look
broken.

## Mutation checks actually run this session

- `serde(default)` removed from `expose_to_agents` → `expose_to_agents_defaults_to_false`
  fails (alongside the two existing default tests).
- `fresh.expose_to_agents = false` removed from `upsert`'s `None` arm plus an
  `existing.expose_to_agents = incoming...` added to the `Some` arm →
  `a_new_profile_starts_unexposed_even_if_the_payload_says_otherwise` and
  `a_save_cannot_raise_agent_exposure` fail.
- Backslash arm removed from `catalog::identifier` →
  `a_backslash_is_refused_even_though_the_quote_would_be_doubled` and
  `each_refused_identifier_names_the_rule_it_broke` fail.
- Byte limit changed to a char limit → `the_length_limit_counts_bytes_and_not_characters`
  fails.

## `Some("")` is refused for a schema

`None` means *this engine does not qualify by schema*; `Some("")` is a name
nobody has, and interpolating it would silently match nothing while looking like
a working lookup. `table_objects` already filters an empty schema to `None`, so
no existing UI path hits it.


---

## Step 2 notes

### Mutation checks run (each broken deliberately, seen fail by name, reverted)

1. `mcp/execute.rs`: `execution_plan(connection, sql, connection.allow_writes)`
   instead of `false`. **Four** tests failed —
   `the_plan_this_path_produces_always_carries_writes_allowed_false`,
   `an_exposed_connection_with_allow_writes_on_still_refuses_a_write`,
   `every_engine_refuses_a_write_on_this_path`,
   `a_write_refusal_says_that_enabling_writes_would_not_help_here`.
2. `mcp/render.rs`: cap notice moved from before the rows to after them.
   `a_capped_result_renders_its_cap_first` and
   `a_capped_empty_result_is_not_the_no_rows_answer` failed.
3. `mcp/argv.rs`: `MARKER` set to `"code-basics-qgate"`.
   `the_self_dispatch_markers_are_disjoint` failed.
4. `commands/sql.rs`: `AuthFailed` mapped to `ConnectionStatusKind::Failed`.
   `every_test_outcome_maps_to_an_agent_kind_and_none_carries_its_message`
   failed with "two outcomes map to one agent kind".

### `SqlTestOutcome` has eleven variants, not thirteen

The plan says "a test enumerating all thirteen". Counted on disk: `Ok`,
`AuthFailed`, `Unreachable`, `CannotOpenFile`, `NotADatabase`, `TlsFailed`,
`Timeout`, `EngineUnknown`, `EngineUnsupported`, `SecretUnresolved`, `Failed` —
**eleven**. The test therefore asserts `every_outcome().len() ==
ConnectionStatusKind::ALL.len()` rather than a literal, so a twelfth fails
whichever side it is added to.

### `Ok { server_version }` is a leak too, and is dropped

Not an error message, and still the server's own text — it names a product and a
build number. `an_ok_outcomes_server_version_does_not_cross_either` pins that.

### The write refusal drops the guard's finding; the other refusal keeps it

`restate_for_agent` keeps the guard's finding and swaps `HEURISTIC_NOTE` for
`AGENT_NOTE`, and `GuardRefused` uses it. `WriteRefusedUnconditionally` does
**not**: it states the finding itself (`This is a write (UPDATE)...`), because
the console's version of that sentence ends "writes are off for this connection",
which reads as a setting worth changing. The verdict is re-derived from
`guard::classify` rather than parsed out of the sentence — the words are prose,
the verdict is data.

### The tooling eats backslashes in heredocs

Writing Rust through `cat <<'RS'` in the Bash tool collapsed `\n` to `
` and
once broke heredoc termination outright. Use the Write tool for any Rust file
containing backslashes, or build the string with `chr(92)` in Python.

## Step 3 gotchas

- **`rmcp` model types are `#[non_exhaustive]`.** `ServerCapabilities { .. }` and
  even `..Default::default()` are compile errors outside the crate, and the
  `builder()` that would solve it is behind the `server` feature this workspace
  does not take. Mutate a `Default` field by field.
- **`ProtocolVersion` deserialises anything.** Unknown strings are kept as-is
  rather than rejected, so membership has to be checked against
  `ProtocolVersion::KNOWN_VERSIONS` — which is what `negotiated_version` does.
- **A statement timeout is not a connect timeout.** `ConnectionStatusKind::Timeout`'s
  own sentence says *nothing was sent and no statement ran*, so it may only be
  used when the connect expired. The runner therefore has two deadlines, not one
  around the whole call. Same trap as `commands/sql.rs::within_timeout`.
- **The store must be reloaded inside the request loop.** Verified live: editing
  `sql-connections.json` while the server runs changes the next call's answer.
  Hoisting `store::load` out of `dispatch` would silently break revocation and
  no test in this crate would notice.
- **A query builder is only tested by an engine.** See `completed.md`: the
  SQLite column-catalog query was valid text and invalid SQL for as long as it
  has existed, and every unit test over it passed.

## Step 4 — installer machinery (2026-09-04)

### Where MCP servers actually live (measured, and none of it is obvious)

| provider | scope | file | shape |
|---|---|---|---|
| Claude Code | project | `<root>/.mcp.json` | top-level `mcpServers` |
| Claude Code | user | `~/.claude.json` | top-level `mcpServers` |
| Codex | user | `$CODEX_HOME/config.toml` | `[mcp_servers.<name>]` |

`.claude/settings.json` holds **only** `hooks` — it is not where `mcpServers`
goes, and writing there would both be inert and collide with the two hook
installers whose plans carry full file contents. Pinned by
`claude_project_scope_is_dot_mcp_json_at_the_root_and_not_dot_claude_settings`
and, from the other side, by
`the_setup_plan_writes_no_path_the_mcp_installer_also_writes` (which intersects
both providers' and both scopes' write paths against `setup::setup_plan`'s).
Mutating `config_target` to return `.claude/settings.json` fails **both** plus
`a_project_install_writes_dot_mcp_json_and_reads_back_as_installed` — verified.

`~/.claude.json` is a **sibling** of `~/.claude`, not a file inside it. The
`home` parameter names the *provider home* (same convention as
`qgate::install::settings_path`), so the sibling is derived with `.parent()`.

### `toml_edit` is not a preference, it is a correctness requirement

Mutation-verified: swapping `doc.to_string()` for a `toml::Value` round-trip
fails `project_trust_tables_with_quoted_windows_path_keys_survive_verbatim` and
`removal_takes_only_our_entry_and_reports_nothing_when_absent`. The real
`~/.codex/config.toml` carries single-quoted **literal** keys holding Windows
paths and colons (`[projects.'c:\users\...']`,
`[hooks.state.'C:\...\hooks.json:stop:1:0']`); a serializer respells them, and
`codex::is_trusted_in` then disagrees with the file. It also carries bare
key-values that TOML requires *before* any table, and Codex rewrites the file
while it runs.

`codex::is_trusted_in` keeps text-scanning **on purpose** and must not be
"unified" with this: a tolerant READ answering yes/no about a fuzzy path has the
opposite failure mode to a WRITE, which must refuse and change nothing.

### serde_json `Map::shift_remove` is feature-gated

`shift_remove` exists on `serde_json::Map` only under `preserve_order`, which
this tree does not enable. Use `remove` — the map is a `BTreeMap`, so key order
is unaffected anyway.

### Atomic writes: what the tests pin, and what they cannot

`apply_writes_atomically` is `.bak` → temp sibling → rename. The **atomicity**
is a property of `rename` and is not observable single-threaded. Discovered by
mutation: replacing the temp path with the destination itself passed the backup,
the no-leftovers and the new-file tests, because `rename(p, p)` succeeds. The
test that does have teeth plants a file at the exact temp path this process
would choose and asserts it was consumed:
`the_content_reaches_the_destination_by_rename_and_not_by_writing_it`.

Stated in the doc comment rather than glossed: on Windows a rename over a file
another process holds open without `FILE_SHARE_DELETE` **fails outright**, and
nothing here stops a running Claude Code from rewriting `~/.claude.json` after
our rename. That is why the user-scope caveat says to close Claude Code first —
no write strategy removes the need for it.

### The config file is pretty, the wire is compact

Both pinned in one test
(`config_output_is_pretty_printed_and_newline_terminated_unlike_the_compact_wire`)
so neither drifts into the other: a config file is read and diffed by a human,
an MCP frame must contain no embedded newline.

## Adding a feature flag fails in the NSIS installer first, not in the UI

`FeatureId::ALL` is read by `features::store_tests`, which *interprets* the
`WriteFeaturesSeed` function in `src-tauri/installer/windows/installer.nsi` -
deriving each checkbox's variable name (`$FeatureMcpSqlServer`) from the feature
id, and asserting the page uses the exact `label()` and `description()` strings.
So the first failures after adding a variant are
`the_windows_installer_writes_json_the_app_can_parse` and
`every_feature_appears_in_both_installers`, and they are correct: a feature with
no checkbox ships a switch nobody can reach. Budget the NSIS edit as part of
adding a feature. Both seeds must list the id and *only* known ids - a seed
offering an id this build cannot toggle is a choice the user can make and never
unmake from the picker.

`every_choice()` in `store_tests.rs` is now every bitmask over `FeatureId::ALL`
rather than a hand-written list of pairs, so the combination coverage grows on
its own. The cost is 2^n seed parses, which is nothing at n = 3.

## `default_enabled` is an exhaustive match on purpose

It used to be a blanket `true`. That is an answer nobody made for the feature
being added: the next one whose honest default is *off* - one that costs money,
opens a port, or grants an agent access - would have inherited "on" silently.
Adding a variant is now a compile error until somebody decides, and
`every_feature_states_its_own_default` is the reader-facing half.

## Review round 1: 13 findings, 7 confirmed, 6 refuted (2026-09-08)

Three adversarial lenses (secret-leakage, abstain/IPC, installer-safety), two
independent skeptics per finding, both defaulting to *refuted* when uncertain —
except for secret-leakage and agent-write findings, held to a normal standard
because a missed leak costs more than a wasted edit.

### The one that mattered most: `read_to_string(path).ok()`

Reported independently by two lenses, and it was real. Both `mcp_merge` and
`codex_toml` collapsed "this file exists but could not be read" into "this file
is absent". Two individually-correct behaviours then composed into data loss:

* the absent case legitimately plans a **brand-new** config, and
* `merges_existing: false` legitimately tells `apply_writes_atomically` there is
  nothing to back up.

Applied to a file that *does* exist, that renames a four-line document over the
target with **no `.bak`**. The target is `~/.claude.json` — 122 KB here, holding
every project's trust decisions, and **rewritten continuously by a running
Claude Code**, so "briefly unreadable" is its ordinary state, not a rare
accident. `is_installed` swallowed the same error via `.unwrap_or(false)`, so the
UI said "not installed" and offered the install: one click from catastrophe.

Fixed with `install::read_existing` — `Ok(None)` absent, `Err` unreadable — plus
`installed_state` returning **three** answers (`Some(true)`, `Some(false)`,
`None` = no answer). Mutation-checked: restoring the collapse fails
`an_existing_path_that_cannot_be_read_is_an_error_and_never_a_fresh_file` and
`an_unreadable_file_is_not_reported_as_not_installed` by name.

`plan_removal` had the same collapse in both modules. Less dangerous (it plans
zero writes) but it rendered as *nothing to remove* — a claim it had not earned
about a file it could not read.

### `schema: None` meant the schema named ""

`column_catalog_query` did `sql_literal(schema.unwrap_or(""))` and interpolated
it into `WHERE table_schema = '{schema}'`. So an omitted schema asked for the
schema whose name is the empty string, which no server has — and the renderer
reported the empty result as "the table may not exist". Meanwhile
`tools_tests.rs` has a test *named*
`an_omitted_schema_means_every_schema_and_an_empty_one_does_not` which only
checked argument parsing and never followed `None` into the query it produced.
An omitted schema is now the **absence of a predicate**.

Still unverified: the new Postgres and SQL Server strings are not executed by
any test — both suites are `#[ignore]`d without a live server. The SQLite arm is
covered by `tests/sql_sqlite.rs`, which is what caught the `"notnull"` quoting
hazard, so the same class of mistake on the other two engines would not be
caught here.

### `AgentConnection` claimed "no path" and carried one

`sql::discover` builds ids as `appsettings:<workspace-relative path>:<key>`
(also `dotenv:`/`usersecrets:`), and `AgentConnection::of` copied the id
verbatim into `sql.list_connections`. The store is **user-global**, so an agent
working in repo A could read the layout of every other repo the user has opened
— and `--workspace` is documented as a convenience, not a boundary, so it was
not standing in the way.

The pinning test could not have caught it: its fixture hard-codes
`id: "conn-1"`, a literal that cannot be path-derived, so it only ever proved
that `of()` drops `secret` and `workspace_root`.

Fixed with `connection_handle` (FNV-1a, deterministic so an agent may cache it,
dependency-free) shown in place of the id, resolved by `find_exposed` alongside
the stored id so a hand-written tool call still works. Names keep their bare
**filename** deliberately — it is how two connections are told apart — with
`shown_name` as the backstop for the case where `discover` falls back to an
absolute path. The module doc now separates the structural guarantee (the
secret, and the id) from the filtered one (the name), because the previous
wording overstated it.

Three existing tests asserted the raw id and were updated to assert
`connection_handle(...)` — a deliberate behaviour change, not a weakened test,
and they now pin the *rule* rather than the literals.

### Driver text through a successful query

The module rule is that driver words are **dropped, not filtered** — a connect
failure crosses as a variant, a statement failure as a stage. `render::value`
broke it for `SqlValue::Unavailable { reason }`, which `sql/format.rs` builds
from `Cell::Error`, which `postgres.rs` and `sqlite.rs` build from sqlx's
`e.to_string()` in `decode_cell`. A driver error routinely embeds the DSN. The
reason is now dropped entirely; the cell still admits it is unreadable, so no
value is invented. `Unsupported { type_name }` deliberately keeps its type name
— that is the schema's own word, read from the catalog, with nothing to leak.

### A caveat that described the wrong feature

`caveats_for(ClaudeCode, Project)` reused `qgate::install::SHARED_EXE_PATH_NOTE`
verbatim, whose words are "The **hook** command … will silently get no
**gate**". Nothing about an MCP install is a hook and no gate is involved. The
gate's own doc comment invites sharing the constant, but sharing a sentence that
names the wrong feature is a wrong warning rather than consistency, so the
*rule* is now restated for this feature and a test asserts the caveat contains
neither "hook" nor "no gate".

### Refuted, and why the refutations were good

Six were dropped, and the refutations were substantive rather than
hand-waves — e.g. the claim that `~/.claude.json` gets its keys reordered
alphabetically by `to_string_pretty` (serde_json without `preserve_order` uses a
BTreeMap, so the *input* was already sorted on parse and nothing is reordered by
the write), and the claim that a project install discloses the developer's
account name with no caveat (two visible caveats already state it).

**The recurring failure mode worth knowing**: several findings across both
workflows cited line numbers that no longer pointed at the code described. The
skeptic prompt was amended after round 1 to check the cited line first, and that
is what caught most of the refutations here.

## Known gap, deliberately not fixed

`plan::status_for` still calls `is_installed`, which folds unreadable into
`false`. The *catastrophe* is fixed — the install now refuses with a message
naming the file and telling the user to close the agent — but the status line
can still read "not installed" for a file that could not be read. Surfacing the
third state properly means changing what `status_for` returns and therefore the
IPC surface, which is a bigger blast radius than this warranted right now. In
`todos.md`.
