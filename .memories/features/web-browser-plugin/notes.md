# Notes — web browser plugin

## `tauri::wry` does not exist on Windows

`tauri-2.11.5/src/lib.rs:184` re-exports it under
`#[cfg(all(feature = "wry", target_os = "android"))]`. Add `wry` as a direct
`src-tauri` dependency at the version already in `Cargo.lock` (0.55.1) so no
duplicate compiles. Found by the Phase 4 spike.

## The framing question, answered: `mcp::ndjson` fits, reused unchanged

`browser::framing` re-exports `crate::mcp::ndjson`. It fits on all four counts
that matter and nothing was written twice:

- The browser pipe carries JSON-RPC over a byte stream with no `Content-Length`,
  exactly like MCP stdio, so a newline **is** the message boundary.
- `LineDecoder` already implements the two poison rules this needs and they are
  the right way round here too: a non-JSON line is one bad message (a newline is
  a resynchronisation point) while an over-long unterminated line poisons,
  because where the next message begins is genuinely unknown.
- `Line::Malformed` reports rather than drops, which is what a prober-visible
  counter needs.
- `encode` is compact and appends exactly one trailing newline.

The only browser-specific addition is `MAX_PIPE_LINE_BYTES`, which is *smaller*
than `mcp::ndjson::MAX_LINE_BYTES`: a browser request is a tool call, never a
page dump (page text travels in the *reply*), and the pipe is reachable by any
local process that can read `%APPDATA%`, so the allocation a prober can force is
worth bounding tighter.

## `BrowserAvailability` is six answers because each licenses something different

`pluginDisabled` (nothing is running — no WebView2 process at all),
`panelClosed` (the plugin is on, the panel is not up), `blank` (a panel with no
page — `about:blank`), `loading` (a page whose text would be wrong, not absent),
`ready` (the only one that licenses a read), `failed` (a navigation that did not
land). Collapsing the first two loses the only distinction an agent can act on:
one is a setting the user turned off, the other is one click away.

## The `writes && !reads` shape is refused structurally, not validated

`AutomationConsent`'s fields are private and there are exactly three
constructors (`none`, `reads_only`, `reads_and_writes`). Deserialisation goes
through `#[serde(try_from = "ConsentWire")]` so the invalid shape cannot arrive
over IPC either — a validation call at each call site would have been the
"unlikely" the brief refuses.


## A `host:port` is not a `scheme:body`, and the grammar cannot tell you which

`localhost:5173` and `javascript:alert(1)` have the same shape, and `localhost`
is a legal RFC 3986 scheme name - so checking the scheme grammar and stopping
there classifies every `host:port` the user types as an unknown scheme and
refuses it. `url::split_scheme` adds two signals in order: `//` after the colon
is always a scheme, and otherwise a body that is all digits up to the first
`/?#` is a port. Caught by `a_host_with_a_port_is_not_read_as_a_scheme` on its
first run.

The residual imprecision is documented in the function: an opaque scheme with a
numeric body (`tel:12345`) is read as a host and a port, so it refuses as
`NotAnAddress` rather than `RefusedScheme`. Less precise message, same refusal,
and `origin::navigation_verdict` refuses the scheme a second time regardless.

## `mcp::install::plan` has two tests that read the developer's real config

Not ours and not caused by Phase 4, but it will waste somebody an afternoon.
`a_project_install_writes_dot_mcp_json_and_reads_back_as_installed` and
`uninstalling_a_file_that_holds_no_entry_of_ours_produces_no_write` fail on a
clean tree on this machine. They assert `status_for(..., None) == None` against
a `tempdir`, but a `None` scope hint makes `status_for` read the real
`~/.claude.json`, which now contains the `code-basics-sql` entry Phase 3's own
installer wrote. They pass only where nobody has used the feature. Verified
pre-existing with `git stash -u`.

## Script assembly is `str::replace`, never `format!`

The injected script bodies are hundreds of characters of JavaScript, which is
nothing but braces. Under `format!` every brace must be doubled, and a missed
pair is a compile error at best and a silently malformed script at worst. The
bodies are `const` raw strings with `__CB_*__` placeholders, and
`no_assembled_script_contains_a_raw_placeholder` is the alarm for a
substitution that did not happen. `init_script` replaces `__CB_GLOBAL_NAME__`
before `__CB_GLOBAL__`; the two do not overlap as substrings, and the order is
belt and braces.

## `browser_navigate` is unusable from a blank panel, deliberately

Navigate is a **write**, consent is scoped to an origin, and `about:blank` has
no origin - so no grant can name it, and navigating from a blank panel is always
refused. That is the conservative reading of "consent is a statement about a
page", and `navigate_cannot_be_used_from_a_blank_panel_and_that_is_deliberate`
records it so nobody reads it as a bug and adds a `Blank` exemption. Such an
exemption would be an un-consented navigation, which is exactly what the write
category exists to require consent for.

## The three constants the panel and the refusals share

`consent::READ_CONSENT_ACTION` and `WRITE_CONSENT_ACTION` are quoted verbatim
inside the refusal sentences an agent reads, so the consent banner in 4c must be
labelled from them rather than from a literal. Same reasoning as
`useShortcutHint`: a hardcoded label drifts the moment someone rewords the
button, and the refusal then sends the user looking for a control that does not
exist.

## Phase 4b notes

**`wry` 0.55.1 has no `go_back`/`go_forward`.** `reload` is the only navigation
verb on `WebView`. Back/forward therefore go through
`evaluate_script("history.back()")`, which is genuinely weaker and says so in the
doc comment: a page with empty history does nothing and reports success.

**`with_new_window_req_handler` does not return `bool`** (the briefing said it
did): it is `Fn(String, NewWindowFeatures) -> NewWindowResponse`, and denying is
`NewWindowResponse::Deny`. `with_download_started_handler` *is* `-> bool`.

**`with_incognito(true)` would have been a mistake.** wry's own doc note says the
`WebContext` is **ignored** when incognito is on, which would put the page's data
back in the default environment - the opposite of the isolation wanted. The
isolation is the separate data directory plus a session that ends when the panel
closes.

**`Ring::push` takes the entry by value and mints the sequence**, so a row's own
`seq` field has to be computed one instruction early as `latest_seq() + 1`. Every
caller `debug_assert`s it against what `push` returned, and that assert earns its
keep: the mutation that set `seq = 0` was caught by the assert firing, not only
by the test's own assertion.

**The `WebContext` is boxed and declared *after* the `WebView`.** Field order is
drop order, so the webview is torn down before the context owning its WebView2
environment; boxing keeps the context's address stable even though the builder's
borrow ends at `build_as_child`.

**`AppState` holds the browser *data*, not a `BrowserHandle`.** `AppState::default`
runs before `tauri::Builder`, so there is no `AppHandle` to store; a command
already has one (as `start_debug` does), so `AppState::browser(app)` pairs the
two on demand and there is no "not initialised yet" state for a caller to handle.

**Driving the app with synthetic input: two things cost time.** Each PowerShell
tool call is a fresh session, so the `Add-Type` helper must live in a `.ps1` that
is dot-sourced. And the **first** `Click` after `SetForegroundWindow` is
routinely swallowed re-activating the window - send a harmless one first, or
repeat the real one. The foreground guard (refuse unless `GetForegroundWindow()`
is our window) fired for real when the user's own browser took focus; it is not
optional.

## Phase 4c notes

### A NULL security descriptor on a named pipe grants **Everyone** read access

This is why `pipe::user_only_descriptor` exists and why it **refuses to create
the pipe at all** if it cannot be built. Windows' documented default security
descriptor for a named pipe grants full control to LocalSystem, Administrators
and the creator owner *and read access to the Everyone group and the anonymous
account*. So "just pass NULL and you get the creator's own DACL" is exactly
wrong, and the failure would be invisible - the pipe works, and so does anybody
else's client. The SDDL is `D:P(A;;GA;;;<this user's SID>)`; `P` keeps anything
inheritable out.

Getting the SID needs `OpenProcessToken` + `GetTokenInformation(TokenUser)` +
`ConvertSidToStringSidW`, which is why `windows-sys` became a `src-tauri`
dependency. It is pinned to **0.61.2, the version tokio already resolves**, so
no second copy compiles.

And say what it does *not* do. The DACL stops other **users**; it does not stop
another program running as this user. The token in the registry reduces that to
"another program that can read `%APPDATA%`". Neither grants automation - only
`consent` does - and the honest one-line summary is in the `pipe` module docs.

### The listener is scoped to the *panel*, and that is what makes five
### `InstanceError` variants reachable from durable facts

Tying the pipe to the process would leave `PanelClosed` with nothing durable to
read: panel state is live, and a registry that recorded it would go stale on
every open and close. With the pipe created by `browser_open` and dropped by
`browser_close`, the file's own contents answer all five:

| file says | answer |
|---|---|
| no entry / dead pid | `NoneRunning` |
| `browserFeature: false` | `PluginDisabled` |
| feature on, `listener: null` | `PanelClosed` |
| two listeners | `Ambiguous` (refuses, never picks) |
| `protocol` differs | `VersionMismatch` |

It is also the smaller surface: with no browser panel open there is no pipe on
the machine at all.

### `PanelClosed` has two causes and the registry cannot tell them apart

A closed panel, and a panel that opened while the pipe **failed to create** (a
squatter on the name, or a DACL that would not build - both fail closed and
publish no listener). The sentence therefore names both, because a user told to
"open the panel" while looking at an open panel has been sent to do something
they cannot do. Pinned by
`the_panel_closed_refusal_names_both_ways_of_getting_there`.

### The banner record dies with consent, not with the page

`shared::note_navigation` deliberately does **not** call `reset_page` (console
and network survive a navigation), so "clear the agent request when the page
changes" had nowhere obvious to live. The rule that is actually right: the
record and the consent are both statements about a *page*, so both are cleared
by an **origin change** and neither by a same-origin path change. Leaving the
record up across an origin change would put "codex.cmd wants to read
<somewhere else>" above a control that grants access to *this* page; dropping it
on every path change would hide a request the user has not answered yet. Two
tests, one each way.

### `mcpServerLogic.ts` was reusable whole - one sentence named a server

Provider labels, scope options, status wording, confirm label, the zero-write
answer and `writtenNote` are all generic. The only SQL-specific string was
`planOutcome`'s "holds no entry for the SQL MCP server", now a defaulted
parameter so the existing call site keeps its exact words. `BrowserMcpPanel`
is therefore a component and no new logic module. What is deliberately **not**
shared is the description and the caveats: what an agent gains here is the
user's own logged-in browser session, and the SQL warnings are about a database
login.

### Small things that cost a minute each

- `browser::framing::encode` returns `Vec<u8>`, not `String` (it is
  `mcp::ndjson::encode`), so `write_all(&encoded)` and not `.as_bytes()`.
- `tauri::RunEvent::Exit` already had a handler for tree-killing children, so
  withdrawing the registry entry had a home. It is not load-bearing - liveness
  is re-probed, so a leftover entry for a dead pid reads as `NoneRunning` - but
  a stale entry makes a second application's entry look like an ambiguity.
- The one cross-language pin in this feature is a **Rust** test that reads
  `src/components/browserPanelLogic.ts` and asserts it quotes both consent
  labels verbatim. `consent`'s refusals name them as the control to click, so a
  reworded button sends the user hunting for something that does not exist, and
  neither side's own tests would notice.

### Two bugs found by driving the real application, both invisible to the tests

**1. A finished load of `about:blank` reported `Ready`.** `browser_status`
against a freshly opened panel said *"the page has finished loading"* about a
panel with nothing in it, because `about:blank` genuinely does finish loading
and `note_load_finished` set `Ready` unconditionally. True about the load, wrong
about the panel — and it collapses `Blank` into `Ready`, which leaves the six
states as five in practice. `Blank` exists for exactly this case.

The fix keys on the **origin**, not on the literal `about:blank`: every opaque
origin is the same fact, a page nothing can be read from and nothing can be
granted for. Pinned by
`a_finished_load_of_about_blank_stays_blank_rather_than_becoming_ready`.

It also moved one existing refusal to a better place, which is worth knowing
before reading that test: `grant_consent` on a blank panel is now refused by the
*availability* check ("the browser panel is open with no page loaded") rather
than by the origin check ("there is no origin to grant against"). Both refuse
and record nothing; the new one names the panel state, which is the more useful
answer. `a_blank_panel_carries_no_origin_so_consent_can_never_attach_to_it` was
updated to assert the new reason and says why in a comment.

**2. The consent buttons were invisible and still clickable.** The banner was
one wrapping flex row: message with `flex: 1`, then the buttons. With the short
*idle* message they sat beside it and looked fine. With the longer *asked*
message — the state that actually matters, because that is when an agent has
asked for something — they wrapped onto a second line that landed in the strip
the page's WebView2 child HWND paints over.

So the panel showed *"cb-app.exe (pid 36028) wants to read and control
https://example.com"* with **no visible control**, and a click into that empty
strip granted **read and control** with nothing on screen to say so. That is the
worst combination available here, and no unit test could see it: `consentBanner`
returned `offerRead: true, offerWrite: true` perfectly correctly. It was found by
clicking where a wrapped row would be and then asking `browser_status`, which
reported *"Permission: read and control"*.

The fix is an explicit **column** — message row, then a `browser-consent-actions`
row — so the banner's height is a plain function of its content and the page's
existing `ResizeObserver` on `.browser-page` follows it. The reasoning is in the
CSS beside the rule, because "make this a wrapping row again" is the obvious
simplification and it is the bug.

**The general lesson for this panel, third time it has bitten:** an OS webview
composites above the DOM, so *any* variable-height chrome added above
`.browser-page` risks putting a control under the page. Prefer a deterministic
height; if the height must change, the page rect has to be re-measured (it is,
via the observer) **and** the control must not be able to land in the page's
band even for one frame.

### How the frontend was actually served during this verification

Worth knowing before repeating it. `dist/` was three hours stale, and
`cargo build --release -p cb-app` does **not** rebuild it (`beforeBuildCommand`
belongs to the tauri CLI, not to `cargo build`) — yet the panel showed
code written minutes earlier. A Vite dev server was listening on 1420 (started
by something else on this machine) and the app loaded from it. So the frontend
under test was the **live source**, not the bundle. That is fine for verifying
behaviour and wrong to describe as "the release build"; if nothing is listening
on 1420, run `pnpm build` before `cargo build --release` or the panel under test
is whatever `dist/` last held.

### The script envelope arrives JSON-encoded, and every unit test had assumed otherwise

**This is the bug that made every page read fail.** `browser_page_text` and
`browser_read_page` came back as *"the page refused without saying why"* against
a real document, while all fifteen host tests passed.

Every injected script ends in `return JSON.stringify({ok:true,...})`, so what
`evaluate_script_with_callback` hands back is a **string**, and WebView2
delivers it JSON-encoded. The Phase 4 spike measured exactly this and wrote it
down — `document.title` came back as `"focus-test"`, quotes included — and the
implementation still parsed the raw text once and looked for `ok` on the result.
Parsing `"{\"ok\":true,...}"` once yields a `Value::String`, whose `get("ok")`
is `None`, which is the "no ok flag" branch: the refusal that says the page
refused without saying why.

The unit tests could not catch it because they fed `envelope_value` the *inner*
shape directly — they pinned the assumption rather than the measurement. It took
calling the tool through the MCP server against `example.com`.

`envelope_value` now unwraps a `Value::String` and parses it again, accepting
both shapes, and the inner parse is attempted **only** for a string so nothing
else is unwrapped twice. Three tests build the raw text with
`serde_json::to_string(inner)` so they exercise the real double encoding, and
one pins that a string which is not an envelope is still refused rather than
unwrapped optimistically.

**The lesson, and it is the second time this feature has produced it:** a test
that constructs its own input to the shape the code expects proves the code
self-consistent and nothing else. Where the shape comes from a runtime, the
spike's measurement is the specification — and this one was written down and then
not used.

## Phase 4c verification notes

### The collapse the stdio drive found: an unknown tool read as "no app running"

`mcp_browser::call_tool` forwarded the tool **name** to the application
unparsed, deliberately ("the application owns the tool table"). The consequence
was a genuine four-into-one collapse in the case a client hits most often: with
no application running, `browser_evaluate` — the tool whose absence is a
*documented decision* — was answered `app_not_running: ... Start code-basics
and open a browser panel`. An agent reads that as *start the app and retry*, and
retries forever a tool that will never exist. Worse, `tools::parse_call`'s
carefully written refusal (which names `browser_evaluate` and points at
`browser_read_page` + `browser_click`) is reachable **only** from
`pipe.rs`, app-side, so it could never be read by the client that needed it.

Note the sibling precedent: `mcp_sql.rs:200` calls `tools::parse_call` **in the
shim**. The browser server had diverged from it.

Fix: `tools::is_known` + `tools::unknown_tool_sentence` (one wording, used by
both sides) and `serve::unknown_tool_answer`, checked in `call_tool` **before**
the registry is read. Only the *name* — the application still parses the
arguments, so nothing became reachable-but-unadvertised. Proved by mutation:
`if false && !tools::is_known(tool)` reproduced the original
`app_not_running` for `browser_evaluate` and
`an_unknown_tool_is_refused_by_name_before_an_application_is_looked_for` caught
it.

### A `\` line continuation is a real hazard in these long sentences

Rust's `\`+newline strips the newline **and** the following indentation, so a
sentence split across lines reads correctly — but an edit that joins the lines
without deleting the indentation silently leaves a run of spaces inside text an
**agent** reads, and no assertion about content notices. It had already happened
once: `serve::unsupported_answer` said "a WebView2 child␣␣␣␣␣␣window". Fixed,
and `no_sentence_this_server_hands_out_carries_a_run_of_spaces` is now the alarm
for the whole class (it covers `INSTRUCTIONS`, both refusal answers and every
tool description). It failed on the real defect before the fix, so its teeth are
proved rather than assumed.

### A live-pid fixture goes stale inside a single session

My first end-to-end registry run recorded pid 29244 (a `target/release`
quality-gate/hook child) as the live instance. Every case answered
`app_not_running` and it looked like the five-answer split had collapsed. It had
not: 29244 had **exited** between enumerating it and using it, so the entries
were correctly filtered by `liveness::alive`. Re-enumerate immediately before
writing the fixture, and prefer a long-lived pid (the installed
`C:\Program Files\code-basics\cb-app.exe`). The near-miss is worth remembering:
the honest-looking failure and the real collapse produce the same output.

## The two `mcp::install::plan` tests: failed, then fixed by another session

Recorded because I got this wrong once mid-session and the wrong answer was
plausible.

They **did** fail on a clean tree at the start of this step, independently
confirmed: `git diff HEAD -- crates/core/src/mcp/` was empty and the real
`~/.claude.json` holds a `code-basics-sql` entry, so `status_for(.., None)`
correctly answered `Some(User)` where the tests hard-code `None`.

They passed later in the same session. My first explanation was a **read race**
(`is_installed` = `installed_state(..).unwrap_or(false)`, so an unreadable
config reads as *not installed*, and a running Claude Code rewrites that file
continuously). That folding is a real property of the code — worth knowing — but
it is **not** why they started passing. `crates/core/src/mcp/install/plan_tests.rs`
was edited at 14:27 by a **concurrent session**, not by this step and not by
`cargo fmt`: it adds an `isolated_home` helper and passes
`home: Some(&tempdir/home)` instead of `None`, which is exactly the injectable
user-scope fix this file's notes had recommended. Left in place; it is correct
and it is somebody else's work.

The lesson for a shared working tree: a test that changes verdict between two
runs is not necessarily flaky. **Check the file's mtime and `git diff` before
theorising** — another agent may simply have fixed it. I reached for a
mechanism (which existed, and was true, and was irrelevant) before checking
whether the code had changed underneath me.

The residual code observation still stands and is out of scope here:
`status_for` consults `is_installed`, which folds `installed_state`'s third
answer (*could not be read*) into `false`. That is a wrong *status*, not a
data-loss path — `merged_text` uses `read_existing(path)?` and propagates the
error, so the one-click catastrophe that comment describes stays closed.

## `cargo test -p cb-core` is far slower than the 2.5 min the briefing claims, and one pre-existing test is why

Measured twice in the gate step (2026-09-08). The **lib** target is fast and
matches expectations: 3633 tests in 62.9 s. Everything after it is quick too
(`git_operations` 26 s, `lsp_client` 3.2 s, `lsp_session` 2.1 s, `sql_sqlite`
0.1 s). The cost is concentrated in **one** integration test:

  crates/core/tests/lsp_transport.rs::dropping_the_transport_kills_the_whole_tree

libtest prints `has been running for over 60 seconds` for it, and the
`lsp_transport` target measured **302.42 s** (26 tests, all passing) - five
minutes for one target, against 63 s for the entire 3633-test lib. The mechanism
is visible in the source: the test ends with
`until(|| !pid_alive(server))` / `until(|| !pid_alive(grandchild))`, `until`
polls every 10 ms, and `pid_alive` on Windows **spawns `tasklist`** each time.
On a loaded machine a `tasklist` enumeration is close to a second, so the loop
is a process-spawn storm that also blocks the runtime thread — which is why the
enclosing `bounded!` 30 s `tokio::time::timeout` does not cut it short.

Two things to be clear about:

1. **It is not caused by the browser work.** `git status --porcelain
   crates/core/tests/lsp_transport.rs` is empty and its last commit is
   `25105ad`; it is untouched by this feature.
2. **It is not a hang.** It completes. Do not report the suite as broken, and do
   not kill it (the CLAUDE.md rule about the `process::` tests applies here for
   the same reason).

Consequence for anyone timing a gate run here: budget **7-8 minutes** of test
time for `cargo test -p cb-core` on a warm target (measured: 63 s lib + 302 s
lsp_transport + ~40 s everything else), plus 3-4 minutes of compilation on the
first run after touching `cb-core`. Not the 2.5 minutes the briefing states. If you only need the decisions, `cargo test
-p cb-core --lib` is the 63-second command and covers every `browser::` module.

A fix, if someone wants it later: `pid_alive` should not be polled at 10 ms
through a process spawn. Either back the loop off to ~250 ms for this one
assertion or use a single wait handle. Out of scope for this phase - changing a
test that is currently proving something real, to make it faster, is its own
piece of work.
