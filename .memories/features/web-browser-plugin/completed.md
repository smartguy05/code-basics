# Completed - web browser plugin

## Phase 4a - every browser decision in cb-core, plus the panel logic

**Files added**

- `crates/core/src/browser/{mod,model,url,origin,ring,text,console,script,ipc,consent,framing}.rs`,
  each with a sibling `*_tests.rs` - **191 tests, all passing**.
- `src/components/browserPanelLogic.ts` + `.test.ts` - 46 tests.
- `crates/core/src/lib.rs` - `pub mod browser;`.

Nothing in any of it touches a webview, a process, a socket or the filesystem.

**Framing: reused, not rewritten.** `browser::framing` re-exports
`crate::mcp::ndjson` unchanged and adds one constant, `MAX_PIPE_LINE_BYTES`
(1 MiB - deliberately *smaller* than the MCP 8 MiB, because a browser request
is a tool call and the pipe is reachable by any local process running as this
user). Full reasoning in notes.md.

**Teeth proved by mutation**, not by a first-run pass. Eight Rust rules were
broken one at a time and each was caught by the test named for it:

| rule broken | test that failed |
|---|---|
| `writes && !reads` accepted over the wire | `deserialising_writes_without_reads_is_refused_rather_than_silently_fixed` |
| ring skips the gap silently | `a_cursor_pointing_into_the_dropped_region_reports_the_gap` (+3) |
| truncation reports the slice length as the total | `truncation_reports_the_pages_total_not_the_slices` (+5) |
| unknown console method ranked `Error` | `an_unknown_console_method_is_other_not_error` (+5) |
| unknown page-message kind silently applied | `an_unknown_kind_is_refused_with_the_kind_preserved` (+2) |
| read consent implies write consent | `read_consent_does_not_allow_a_single_write` |
| URL bar falls back to searching | `a_search_phrase_is_refused_rather_than_searched` (+3) |
| script argument spliced in raw | `a_hostile_element_reference_cannot_escape_into_the_script` |

Four more on the frontend: same-reference-on-no-op, the 0x0 refusal, the scale
mismatch, and the empty-draft URL bar.

**One genuine bug found by a test on its first run.** `url::split_scheme` read
`localhost` as an RFC 3986 scheme and `5173` as its body, so every `host:port`
the user typed was refused as an unknown scheme.
`a_host_with_a_port_is_not_read_as_a_scheme` caught it; the fix is a bare-port
test after the grammar test, documented with the one case it still gets wrong.

**Pre-existing failure found, not fixed** (out of scope - `mcp/install/`):
`mcp::install::plan::tests::a_project_install_writes_dot_mcp_json_and_reads_back_as_installed`
and `..::uninstalling_a_file_that_holds_no_entry_of_ours_produces_no_write` fail
on a **clean tree** (verified with `git stash -u`). Both assert
`status_for(ClaudeCode, temp_root, None) == None`, but `status_for` with a
`None` scope hint reads the developer's real `~/.claude.json`, which now holds a
`code-basics-sql` entry - installed by the very feature Phase 3 shipped. So they
pass only on a machine where nobody has used the feature. The fix belongs with
`mcp/install`: the user-scope path needs to be injectable in a test the way
`CB_LAUNCHERS_PATH` / `CB_NOTES_PATH` are.

---

## Phase 4b - the host, the commands, the panel

**Files.** `src-tauri/src/browser/mod.rs` (host), `shared.rs` + `shared_tests.rs`
(23 tests), `host_tests.rs` (7), `src-tauri/src/commands/browser.rs` (13 async
commands + 4 tests), `AppState.browser` + 2 `state_tests` cases,
`src/components/BrowserPanel.tsx`, `ipc/types.ts`, `ipc/api.ts`, `App.tsx`,
`styles.css`, plus registration in `features/mod.rs`, `features.json`,
`installer.nsi`, `featuresLogic.ts`, `shortcutLogic.ts`, `pluginMenuLogic.ts`
(+3 tests), and docs (`architecture/browser-panel.md` new,
`reference/commands.md`, `docs/README.md`, CLAUDE.md).

**Verified by driving the real application** (a temporary second bin over
`cb_app_lib::run()`, because `target/debug/cb-app.exe` was held open by two
agents' `mcp-sql` children; every trace of it removed afterwards):

- the child HWND renders inside the panel, and `EnumChildWindows` lists it
  **above** the main webview's;
- `example.com` loaded; the URL bar took clicks and keystrokes and then showed
  the url the load landed on;
- dragging the panel moved the page with it (the rect followed exactly);
- minimize hid the OS surface (`visible=False`) and the pill was labelled
  **from the page title**; restore brought it back with the page intact;
- the separate `WebContext` really is separate: its own browser process tree
  under `dev.codebasics.app/browser/EBWebView`;
- **feature-off dropped it**: six `msedgewebview2` processes to zero, the child
  HWND gone from the window tree, and the pill gone (unmounted, not hidden);
- a search phrase was refused in the panel with the reason, draft preserved.

**One real bug found by using it, then fixed.** `strong` was in the header's
drag exemption, copied from `TerminalPanel` where the `<strong>` is the rename
target. Here the URL bar is `flex: 1`, so the label is the only grab area - the
panel could not be dragged at all, proved by the page's child HWND not moving.
Fix: drop `strong` from the exemption, pad the label into a real handle, add
`user-select: none` to the header, and shrink the default size so
`clampPanelPosition` does not saturate on the first drag.

**Also observed, accepted rather than fixed:** the page clips the *Optional
features* modal - the very dialog used to switch the browser off. Recorded in
the CLAUDE.md gotcha and the new guide; the panel's own close button remains the
way out.

**Tests proved by mutation, not by passing.** All 23 `shared_tests` passed on
their first run, so eight load-bearing rules were broken one at a time (Ready
instead of Loading on navigation; a refused navigation moving the url; consent
surviving an origin change; the landing url not re-checked; a fabricated
`evalReply` applied as a console row; a close keeping the consent and the logs;
rows carrying no sequence; a page error ranked `Log`) and each was caught by the
test named for it. All reverted; suite green.

---

## Phase 4c — the pipe, the MCP server, and the installer entry

**cb-core (8 new modules, all pure, all tested with no pipe and no webview)**

`browser/{argv,instances,liveness,wire,tools,serve,render,install}.rs` plus a
sibling `*_tests.rs` for each. `liveness` is the one platform seam (sysinfo, the
`running::probe` precedent) and is still mostly pure — `same_executable` is
where the recycled-pid decision lives.

**src-tauri**

`browser/{agent,registry,pipe}.rs` + tests, `commands/browser_mcp.rs` (the five
plan/apply/re-status commands), `mcp_browser.rs` (the fifth self-dispatch mode),
`AppState.browser_pipe` (`#[cfg(windows)]`), and four new `BrowserHandle`
methods (`read_page`, `click`, `type_text`, `press_key`) over the core scripts
Phase 4a had already written and tested.

**Frontend**

`browserPanelLogic.consentBanner` + the two label constants, the banner and an
**Agents** button in `BrowserPanel.tsx`, `BrowserMcpPanel.tsx`, the
`BrowserAgentRequest` type and `lastAgentRequest` on `BrowserSnapshot`, five
`api.ts` wrappers, and `.browser-consent` CSS. `mcpServerLogic.ts` was reused
whole: the only SQL-specific string in it (`planOutcome`'s "holds no entry for
the SQL MCP server") became a defaulted parameter, so the existing call site
keeps its exact words.

### Test discipline, honestly

- **`instances::choose_instance` was written red-first.** The tests went in
  against a stub; 22 of 28 failed for the right reasons (the stub answered
  `NoneRunning` to everything), then the implementation turned them green.
- **The other seven modules passed on their first run, which proves nothing**, so
  eleven load-bearing rules were broken one at a time and each was caught by the
  test named for it:

| rule broken | test that failed |
|---|---|
| an empty expected token accepts every caller | `an_application_with_no_token_accepts_nobody` |
| the token is checked before the protocol | `a_protocol_mismatch_is_reported_before_the_token_is_looked_at` |
| an eviction count folded into the count line | `evicted_entries_are_reported_and_never_folded_into_a_quiet_page` |
| the coverage note omitted when there are no rows | `every_network_answer_carries_the_coverage_note_verbatim` |
| a grant for another page reads as a plain grant | `consent_granted_for_another_page_is_reported_as_exactly_that` |
| a negative `since` cursor clamped to zero | `a_negative_or_fractional_cursor_is_refused_rather_than_clamped` |
| the page-access note dropped from the caveats | `every_scope_states_what_an_agent_gains_access_to` |
| a codeless refusal returned as a success | `a_refusal_with_no_code_still_reads_as_a_refusal` |
| `history_step` claiming the page moved | `a_history_step_never_claims_it_moved` |
| `Ambiguous` picking the first panel | `two_open_panels_are_refused_and_never_picked` |
| the TypeScript consent label reworded | `the_typescript_banner_quotes_the_consent_labels` |

The last one is a **Rust** test that reads `browserPanelLogic.ts`: `consent`'s
refusals name the two labels as the control the user would click, so a reworded
button sends them hunting for something that does not exist and neither side's
own tests would notice.

### Two things found by a test rather than by reasoning

- **The banner record had no correct home in `note_navigation`.** The first
  attempt asserted it was cleared on any navigation and failed, because
  `note_navigation` deliberately does not `reset_page`. The rule that is
  actually right — cleared on an **origin change**, exactly like consent, and
  not on a same-origin path change — came out of that failure and now has a
  test each way.
- **A genuine state collapse in the first wiring.** `announce(state, panel_open,
  listener)` published "panel open" as a fact of its own, which meant a panel
  that opened while the pipe failed to create would have been published as
  *panel closed*. The parameter is gone (the listener's presence **is** that
  fact) and `InstanceError::PanelClosed`'s sentence now names both causes.

### Decisions worth not relitigating

- **No `browser_evaluate`**, stated in the module docs, the tool-list refusal and
  the installer's caveats so it reads as a decision rather than an omission.
- **The listener is scoped to the panel**, which is both the smaller surface and
  what makes all five `InstanceError` variants readable from durable facts.
- **An explicit DACL is mandatory**, because a NULL security descriptor on a
  named pipe grants read access to Everyone. If it cannot be built, no pipe is
  created at all.
- **`--workspace` does not exist here.** A browser panel belongs to a window,
  not a repository.

---

## Phase 4c — verification pass (this step)

Phase 4c's code was already in the tree when this step began, so this pass was
about **proving** it rather than trusting the record above. Everything below was
run and read, not inferred.

**Verified headlessly, opening no window.**

- `cargo test -p cb-core --lib browser::` — **351 pass** (346 before this pass'
  five new tests).
- `cargo test -p cb-app --lib` — **221 pass** (219 + the two new shim tests).
- `cargo fmt --check` clean, `cargo check -p cb-app --all-targets` clean,
  `npx tsc --noEmit` clean, `pnpm test` **1947/1947 in 73 files**,
  `pnpm docs:index` + `pnpm docs:check` regenerated and passing.
- **The real `mcp-browser` server driven over stdio** (a Node script; the
  `target/release/cb-app.exe` in the tree was confirmed to be built from exactly
  this working tree — no `.rs` file was newer — and confirmed to contain the
  `mcp-browser` self-dispatch string *before* being invoked, so it could never
  reach `tauri::Builder` and open a window). With no instance registry present:
  `initialize` answers, `tools/list` lists all **13** tools, `ping` answers,
  a notification is **not** answered, every `tools/call` returns the
  `app_not_running` text rather than failing, stdin close exits 0 — and
  **stderr was 0 bytes and every stdout line parsed as a JSON-RPC frame**, which
  is the "looks hung" failure this check exists for.
- **All five `InstanceError` variants driven end-to-end** against hand-written
  registry files and **real live pids** (re-enumerated immediately before use):
  `browser_plugin_off`, `browser_panel_closed`, `protocol_mismatch`,
  `several_browser_panels` (lists both pids and refuses to choose),
  `app_not_running`. Also: a **corrupt** registry loads as *no instances*, and a
  **recycled pid** (a live cb-app pid recorded against `notepad.exe`) is refused
  by the exe-path comparison. This closes the "Ambiguous against real pids" and
  "`--instance` selecting between them" todos: `--instance <pid>` resolves the
  ambiguity and attempts that pid's pipe, and `--instance` naming a pid not in
  the registry gets its own sentence that names the flag.

**One real defect found and fixed** — an unknown tool name was answered
"No code-basics application is running". Full reasoning in notes.md; three new
pure functions (`tools::is_known`, `tools::unknown_tool_sentence`,
`serve::unknown_tool_answer`), the guard placed **before** the registry read,
and five new tests. Two mutations proved the teeth (`if false && !is_known`
reproduced the original bug; `ALL.contains(&tool) || true` broke the name check)
and both were reverted.

**One pre-existing defect fixed in passing:** `serve::unsupported_answer`
carried a collapsed `\` continuation ("WebView2 child␣␣␣␣␣␣window") in text an
agent reads. Fixed, with a new class-wide alarm test that failed on the real
defect first.

**Two failures reported and NOT fixed by this step** — they were fixed at 14:27
by a **concurrent session** (an `isolated_home` helper in
`crates/core/src/mcp/install/plan_tests.rs`, passing `home: Some(..)` instead of
`None`), which is why the final `cargo test -p cb-core --lib` is **3633 passed,
0 failed**. That edit is in the working tree and is not mine; it is correct and
was left in place. I first mis-explained the change of verdict as a read race —
see notes.md. (out of this step's scope —
the brief forbids touching `crates/core/src/mcp/`):
`mcp::install::plan::tests::a_project_install_writes_dot_mcp_json_and_reads_back_as_installed`
and `..::uninstalling_a_file_that_holds_no_entry_of_ours_produces_no_write`.
Independently confirmed pre-existing this session, not taken from the earlier
note: `git diff HEAD -- crates/core/src/mcp/` is **empty** and there are no
untracked files there, and the developer's real `~/.claude.json` holds the
`code-basics-sql` entry that makes `status_for(.., None)` answer `Some(User)`
instead of `None`. The fix belongs with `mcp/install`: the user-scope path needs
to be injectable the way `CB_NOTES_PATH` is.

## Gate step (2026-09-08) - the whole quality gate re-run, plus the isolation audit

**Nothing was launched.** No window opened, no `cb-app.exe` killed, no
screenshot taken. Everything below is either a command whose output was read or
a file that was read.

### The gate, re-run from scratch and all green

| Command | Result |
|---|---|
| `cargo test -p cb-core` | **exit 0.** lib 3633 passed / 0 failed; all 13 integration targets ok (git_operations 73, lsp_transport 26, lsp_client 42, lsp_session 28, sql_sqlite 25, pty_roundtrip 13, intent_retirement 9, reject_markers 7, durable_why 4, intent_attribution 3+1 ignored, behavioral_replay 2, lsp_oracle 1+5 ignored; sql_mssql 24 and sql_postgres 19 ignored, no server) |
| `cargo test -p cb-app` | **exit 0.** 225 passed / 0 failed |
| `cargo fmt --all --check` | clean (re-run after this step's own edits) |
| `cargo clippy --workspace --all-targets` | **exit 0, zero warning lines** |
| `pnpm typecheck` / `npx tsc --noEmit` | exit 0 |
| `pnpm test` | 73 files, **1947 passed** / 0 failed (`browserPanelLogic.test.ts` alone: 56) |
| `pnpm docs:index` | regenerated: 610 files, 187 commands, 188 IPC wrappers, 168 core modules |
| `pnpm docs:check` | 25 files, all under 500 lines, all relative links resolve |
| `cargo build --release -p cb-app` | **exit 0** in 8m25s - no "Access is denied", despite four `target/debug/cb-app.exe` and two installed `cb-app.exe` running. `target/release/cb-app.exe` is fresh (15,480,832 bytes), so the intent hook, the quality gate and both MCP servers now run this code |

The first `cargo test -p cb-core` run was piped through `tail`, which means the
exit code reported was **`tail`'s, not cargo's** - it proved nothing. It was
re-run redirecting to a file. Worth remembering: `cmd | tail` always exits 0.

### The isolation audit - the property the engine choice rests on

Read, not inferred:

- **`browser::origin::navigation_verdict` refuses everything it must**, and
  `origin_tests.rs` covers each one by name: `data:`, `javascript:`, `file:` and
  every other scheme by *allowlist* (`every_dangerous_scheme_is_refused_naming_itself`,
  `a_custom_scheme_is_refused_by_allowlist_not_by_a_blocklist`),
  `http://localhost:1420` and `http://127.0.0.1:1420`
  (`the_dev_server_is_refused_because_it_is_this_apps_own_ui`), `tauri://localhost`
  under all three spellings (`tauri_localhost_is_refused_under_both_schemes`),
  plus the evasions - a written-out default port, userinfo, and casing.
- **The predicate is actually wired to the webview, and returns `false`.**
  `src-tauri/src/browser/mod.rs:589` is
  `.with_navigation_handler(move |url| match navigation_verdict(&url, &APP_ORIGINS) { … Refuse => { note_refused_navigation(…); false } })`.
  It is checked a second time in `load()` (line 518) so a *user*-typed refusal is
  a sentence rather than a click that does nothing. A perfect predicate wired to
  nothing was the failure mode being looked for; it is not present.
- **The production path builds the webview exactly as the spike did.** Raw
  `wry::WebViewBuilder::new_with_web_context(...).build_as_child(&window)`. No
  `tauri::WebviewBuilder`, no `Window::add_child`, no `Manager::get_webview`, no
  `unstable` feature - grepped the whole host module. The single
  `with_initialization_script` is `cb_core::browser::script::init_script`, whose
  only global is a non-configurable `window.__CB_GLOBAL__`; grepping the entire
  `crates/core/src/browser/` tree for `TAURI`/`invoke` finds **only doc comments**.
  So no `__TAURI_INTERNALS__`, no invoke bootstrap and no invoke key is
  reintroduced. **Not a critical finding - the production path is the spiked one.**
- The rest of the refusal surface is present in the builder and each line says
  why: popups `NewWindowResponse::Deny`, downloads `false`, `with_clipboard(false)`,
  `with_devtools(false)`, `with_autoplay(false)`,
  `with_back_forward_navigation_gestures(false)`, `with_focused(false)`, and a
  distinct `WebContext` directory.
- `capabilities/default.json` grants by `webviews`, not `windows` - so even a
  hypothetical Tauri-created child of `main` would not inherit the command surface.
- `ipc::parse_page_message` bounds the size first, then requires typed fields,
  and an unknown `kind` is *counted* rather than applied. `accept_eval_reply`
  refuses an id the host is not waiting for - the same ownership check whose
  absence is the Tauri channel bug.

**Re-confirming it live still gates shipping** and is item 1 of the manual
checklist in todos.md, rewritten this step to be executable (there is no `eval`
command or tool, so it is driven from a page you serve yourself).

### Headless verification of the MCP server, over stdio

Ran `target/release/cb-app.exe mcp-browser` with a JSON-RPC script on stdin. No
window, no pipe, nothing launched.

- `initialize` -> `code-basics-browser` 1.4.0, protocol `2024-11-05`.
- `tools/list` -> **13 tools**, and `browser_evaluate` is absent as designed.
- `browser_status` with no app running ->
  `app_not_running: No code-basics application is running, so there is no browser
  to look at. Nothing was read. Start code-basics and open a browser panel from
  the titlebar Plugins menu.`
- `--instance 999999` -> its own sentence naming the flag and why a pid does not
  come back, rather than the generic none-running answer.
- an unknown tool name -> `unknown_tool`, listing the real ones and stating that
  there is deliberately no JavaScript-evaluating tool.
- **stdout carried only well-formed MCP frames; stderr was empty** on both runs -
  the two rules for this self-dispatch mode hold.

### Two stale doc comments fixed

`browser::install::entry_args` and `browser::argv::MARKER` both claimed the
installed entry "also carries" the marker flag. It does not - `entry_args()`
emits the subcommand alone, and `the_entry_is_a_command_field_and_an_args_array`
pins `args == ["mcp-browser"]`. Both comments now say what the marker actually
is: an accepted *alternative* spelling, so a hand-edited command line still
starts the server. No behaviour changed.

### One finding that is not this feature's

`crates/core/tests/lsp_transport.rs::dropping_the_transport_kills_the_whole_tree`
makes that target take **302 s**. Pre-existing (the file is unmodified, last
commit `25105ad`), not a hang, and the mechanism plus the timing budget are in
notes.md.
