# The SQL MCP server

The SQL console reaches three engines (SQLite, PostgreSQL, SQL Server),
classifies a statement as read-only, and caps a result set. All of that was
reachable only by a human sitting at the panel. The MCP server is the second
consumer: it lets a coding agent — Claude Code, Codex, anything that speaks the
Model Context Protocol — ask the same databases the same read-only questions.

It runs as `cb-app mcp-sql`: the **same executable**, self-dispatched over stdio,
exactly as `record-intent` and `quality-gate` are. There is no second artifact to
keep in step and no interpreter to depend on being installed.

## Consent is per connection, and off by default

A saved connection carries `exposeToAgents`. Nothing an agent does can set it —
it moves only through the app's own `sql_set_expose_to_agents`, and an absent key
in the store is consent **withheld**, so an older or hand-edited file loads as
unexposed rather than failing to load.

It is separate from `allowWrites` on purpose. The two are orthogonal, and no
combination of them lets an agent write (see below).

The exposure list is **re-read on every call**. Revoking exposure in the app
takes effect on the agent's next request; there is no cached list and no cached
connection handle, because the server process is long-lived and a cache would go
on answering for a database the user had already un-exposed.

> **What granting it means.** An agent with read access can read anything that
> login can read — including credentials the *database itself* stores
> (`pg_stat_activity`, `SHOW ALL`, `sys.dm_exec_connections`, a foreign-data-
> wrapper user mapping). That is inherent in giving an agent read access and
> nothing in this app removes it. Expose only connections whose login you would
> give a colleague read access to.

## Read-only is forced, not requested

Three layers, each because the one above it can be wrong:

1. `exposeToAgents` decides whether the connection is reachable at all.
2. The read-only guard classifies the statement with `writes_allowed = false`.
   Anything not positively recognised as a read is refused — including statements
   the guard simply does not know. **The connection's own `allowWrites` setting
   is not consulted on this path.**
3. The handle is opened with `writes_allowed: false`, so the engine's own
   mechanism fires: `SQLITE_OPEN_READONLY`, a read-only PostgreSQL session, a
   rolled-back SQL Server transaction.

`mcp::execute::agent_plan` has **no `writes_allowed` parameter** — not a defaulted
one, an absent one — so there is no value a caller could pass and no tool argument
that lifts it. A refusal says so, rather than repeating the console's advice to
"enable writes for this connection", which on this path would be false.

## The tools

| Tool | Answers |
| --- | --- |
| `sql.list_connections` | id, name, engine (`null` when undetermined) |
| `sql.list_tables` | schema and name pairs |
| `sql.describe_table` | column rows, with unreported facts said to be unreported |
| `sql.query` | capped rows, cap reported **first** |
| `sql.read_only_enforcement` | what is really stopping a write, and what is not |
| `sql.connection_status` | whether the connection opens, as a category |

`sql.read_only_enforcement` is the honesty surface, and it earns its own tool.
For SQL Server a write **executes and is rolled back** — it is undone, not
refused — and anything it does outside the transaction survives. An agent reading
"read-only" as "cannot write" is being told something untrue, and folding this
onto every other tool's output would bury it.

Two omissions are deliberate:

- **No explain tool.** The hazard is already closed (the guard makes
  `EXPLAIN ANALYZE INSERT` a write, and an unrecognised option a refusal), but
  **SQL Server has no `EXPLAIN`** — its equivalent is a session statement the
  guard refuses — so the tool would be unsupported for one of three shipped
  engines while buying nothing, since `sql.query` already accepts
  `EXPLAIN SELECT ...` as an ordinary read.
- **No `sql.discover`.** It reads `appsettings.json`, .NET user secrets and
  `.env` files hunting for connection strings: a filesystem scraper aimed at
  exactly the values this subsystem exists to protect.

## `--workspace` scopes the listing; it is not a security boundary

`cb-app mcp-sql --workspace <root>` filters `sql.list_connections` to connections
created under that root, and says how many exposed connections it left out.

That is a convenience, and the docs say so out loud because the flag looks like
an access control and is not one. The connection store is **user-global**, consent
is **per connection**, and a connection excluded from a scoped listing can still
be named by its id. A flag in a config file a team shares is not a place to put an
access decision — `exposeToAgents` is.

## What an agent is never told

- **No tool accepts or echoes a connection string.** The console's
  `sql_test_connection_string` is a human command and is not on this surface.
- The agent's view of a connection is `{ id, name, engine }` — a narrower type
  than the picker's view, not a filtered copy of it. **No field can carry a
  secret**, structurally: there is nowhere to put one.
- **The `id` is an opaque handle, not the stored id.** A connection the app
  *discovered* rather than one you typed carries its origin file in its stored
  id (`appsettings:<workspace-relative path>:<key>`, and the `dotenv:` /
  `usersecrets:` equivalents), and the connection store is user-global — so
  without this, an agent working in one repository could read the directory
  layout of every other repository you have opened. What crosses is
  `conn-<hash>`; `find_exposed` resolves both, so a hand-written tool call
  naming the real id still works.
- **The `name` keeps its filename on purpose.** A discovered connection's label
  is `Api · appsettings.json · ConnectionStrings:Db` — the bare filename is how
  a person or an agent tells two connections apart, and it is worth more than it
  costs. Any path component is stripped, which matters because `discover` falls
  back to an absolute path when it cannot make an origin workspace-relative.
- **Driver text is never forwarded at all.** A connect failure crosses as a
  *category* and a statement failure as a *stage*. `dsn::redact` is a keyword
  heuristic, and the party on the other end of this transport will paste what it
  is told into a transcript, a commit message and a model provider's logs — a
  different risk profile from the human console.
- A secret that could not be resolved reports that fact without naming the file
  or the key it looked in.

## Every abstention is its own answer

*No connection exposed* is not *no connection saved*; *not exposed* is not *not
found*; an ambiguous selector is **refused, never resolved**; a capped result is
**incomplete**, not short, and a row cap is not a byte cap (raising the row limit
would not return more rows). An uncapped empty result says the query matched no
rows **and that this is the complete answer**, so silence is not read as failure.

Every one of these is an MCP **tool execution error**, not a JSON-RPC protocol
error — the specification says a client should hand tool errors to the model so it
can correct itself. Protocol errors are reserved for the two things no amount of
model reasoning fixes: an unknown tool name and arguments of the wrong shape.

## Where the code is

| Module | Holds |
| --- | --- |
| `crates/core/src/mcp/argv.rs` | is this process the server or the application |
| `crates/core/src/mcp/ndjson.rs` | line-delimited framing (**not** LSP's `Content-Length`) |
| `crates/core/src/mcp/expose.rs` | which connections are visible, and the narrow view |
| `crates/core/src/mcp/execute.rs` | forcing read-only |
| `crates/core/src/mcp/answer.rs` | every refusal, kept apart |
| `crates/core/src/mcp/render.rs` | the words an agent reads |
| `crates/core/src/mcp/tools.rs` | the six tools, their schemas, and call parsing |
| `crates/core/src/mcp/serve.rs` | version negotiation, method routing, the JSON-RPC envelope |
| `crates/core/src/mcp/install/` | the previewed installer: which file each provider uses, and how one entry is merged into it |

Everything above is pure and unit-tested headlessly with no database and no MCP
client. See also [the core crate](../architecture/core-crate.md) and
[agent intent capture](agent-intent-capture.md), whose hook installer this
feature's installer follows.
