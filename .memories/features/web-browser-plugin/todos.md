# Todos - web browser plugin

## Phase 4a - DONE
- [x] `crates/core/src/browser/` - ten modules, each with a sibling `*_tests.rs`
- [x] `pub mod browser;` in `crates/core/src/lib.rs`
- [x] `src/components/browserPanelLogic.ts` + test (46 tests)
- [x] `capabilities/default.json`: `windows` -> `webviews`

## Phase 4b (this step) - DONE
- [x] `wry = "0.55.1"` as a direct `src-tauri` dependency (one line of Cargo.lock)
- [x] `src-tauri/src/browser/{mod,shared,shared_tests,host_tests}.rs` - the
      `thread_local!` host, the data half, `bounds_for`, `envelope_value`
- [x] `AppState.browser` (data only) + `browser(app)` / `browser_data(_mut)`
- [x] 13 `async` commands + `close_availability` / `read_gate`, with tests
- [x] `BrowserPanel.tsx`, `types.ts`, `api.ts`, `App.tsx` hosting (global)
- [x] all seven registration points + the NSIS page (`FeatureId::ALL` is now 4,
      so `store_tests` covers 16 checkbox combinations)
- [x] `docs/architecture/browser-panel.md`, `docs/reference/commands.md`,
      CLAUDE.md gotcha + `browser/` module entry
- [x] verified by driving the real app: render, navigate, URL-bar clicks and
      keystrokes, drag, minimize/restore, feature-off teardown, the refusal

## Still not verified by hand (the spike's list, carried again)
- [ ] high-DPI / a scale factor other than 1.0, and a multi-monitor move.
      `pageRect` **refuses** a mismatch rather than correcting it, so the
      failure mode is a stated refusal and not a misplaced page - but the
      path where the two readings agree at a factor other than 1 has still
      never run.
- [ ] parent window minimize / maximize / restore
- [ ] whether the page swallows app shortcuts while it holds focus
- [ ] downloads and new-window requests (both refused in code, neither
      exercised against a page that actually tries)

## Phase 4c (this step) - DONE
- [x] `crates/core/src/browser/{argv,instances,liveness,wire,tools,serve,render,install}.rs`
      + tests. `instances::choose_instance` was written red-first (22 failing
      against a stub); the rest were mutation-checked (11 rules broken one at a
      time, each caught by the test named for it).
- [x] `src-tauri/src/browser/{agent,registry,pipe}.rs` + tests - the gate, the
      registry publication, and the `#[cfg(windows)]` named pipe with an
      explicit DACL (`D:P(A;;GA;;;<user SID>)`) and OS-sourced peer naming.
- [x] `src-tauri/src/mcp_browser.rs` - the fifth self-dispatch mode.
- [x] `AppState.browser_pipe` (`#[cfg(windows)]`), started by `browser_open`
      and stopped by `browser_close`.
- [x] The consent banner - `browserPanelLogic.consentBanner` + the header
      controls, labelled from the two shared constants. A **Rust** test reads
      `browserPanelLogic.ts` and fails if the labels drift.
- [x] The installer entry: `browser::install::browser_mcp_entry`, five commands,
      `BrowserMcpPanel.tsx` reusing `mcpServerLogic` + `PlanPreview` whole.
- [x] `windows-sys` 0.61.2 (the version tokio already resolves) + `tempfile` as
      a `src-tauri` dev-dependency.

## Still not verified by hand (the spike's list, carried again)
- [ ] the automation *writes* against a real page: click / type / press key have
      host methods and core scripts, and have not been driven against a live
      document. `browser_read_page`'s refs are held page-side, so a ref from
      before a navigation resolves to nothing (the script says so) - untested.
- [x] two applications with a panel open each, i.e. the `Ambiguous` refusal
      against real pids. **Verified** without launching anything: a hand-written
      registry naming two *actually live* cb-app pids answered
      `several_browser_panels`, listed both pids and chose neither.
- [x] `--instance` selecting between them. **Verified**: `--instance <pid>`
      resolved the ambiguity and attempted that pid's pipe; a pid absent from
      the registry got its own sentence naming the flag.
- [ ] a squatter on the pipe name (the `first_pipe_instance` fail-closed path).
- [ ] high-DPI / a scale factor other than 1.0, and a multi-monitor move.
- [ ] parent window minimize / maximize / restore
- [ ] whether the page swallows app shortcuts while it holds focus
- [ ] downloads and new-window requests (both refused in code, neither
      exercised against a page that actually tries)

## Later phases
- [ ] 4d: `CapturePreview` (a screenshot tool). `consent::BrowserTool::Screenshot`
      already exists and is categorised as a read; it is deliberately **not** in
      `browser::tools::ALL`, so adding it is a descriptor plus a host method.

## Gate step (2026-09-08) - DONE
- [x] whole gate re-run and green: `cargo test -p cb-core` (exit 0, lib 3633 +
      13 integration targets), `cargo test -p cb-app` (225), `cargo fmt --all
      --check`, `cargo clippy --workspace --all-targets` (zero warnings),
      `pnpm typecheck`, `pnpm test` (1947), `pnpm docs:index` + `pnpm docs:check`
- [x] `target/release/cb-app.exe` rebuilt (8m25s, no lock error)
- [x] isolation audit by reading: `navigation_verdict` refuses data/javascript/
      file/every custom scheme, `localhost:1420`, `127.0.0.1:1420` and
      `tauri://localhost`; the handler at `browser/mod.rs:589` consults it and
      returns `false`; the production path is raw `wry::WebViewBuilder::
      build_as_child` with no Tauri webview API and no bridge-exposing init
      script. **Not a critical finding.**
- [x] `mcp-browser` driven over stdio from the release binary: 13 tools, no
      `browser_evaluate`, specific `app_not_running` / `--instance` /
      `unknown_tool` refusals, stdout MCP-only, stderr empty
- [x] no stray `CARGO_TARGET_DIR`s (`target/wf-*` and `$TEMP/claude/*/target`
      both empty)

## Manual verification the user must do

Everything below needs a human at the screen: it either requires the browser
panel to be open with a real page in it, or it requires a second application
window. This step deliberately launched nothing (the user was working on this
machine), so **none of it is verified** and none of it should be reported as
working. Each item names what to do and what a pass looks like.

1. **THE CHECK THAT GATES SHIPPING: no Tauri bridge inside the page.** There is
   deliberately no `eval` command or agent tool, so drive this from a page you
   control. Serve one file over http on **any port other than 1420** (e.g.
   `python -m http.server 8099` in a scratch directory) whose script is:

   ```html
   <script>
   var names = ['__TAURI_INTERNALS__','__TAURI__','__TAURI_INVOKE_KEY__',
                '__TAURI_EVENT_PLUGIN_INTERNALS__','__TAURI_METADATA__'];
   var out = names.map(function (n) { return n + '=' + typeof window[n]; }).join(' ');
   document.title = out;
   document.body.textContent = out;
   console.log(out);
   try {
     window.ipc.postMessage(JSON.stringify({cmd:'plugin:__TAURI_CHANNEL__|fetch',
       callback:1, error:2, payload:{}}));
     console.log('postMessage did not throw');
   } catch (e) { console.log('postMessage threw: ' + e); }
   </script>
   ```

   Open the browser panel, load that URL, and read the result three ways: the
   panel's pill/title, the panel's Console pane (the init script captures
   `console.*`), and — after granting read consent — an agent
   `browser_page_text`.

   *Pass:* every one of the five globals reads `undefined`, and the
   `plugin:__TAURI_CHANNEL__|fetch` message produces **nothing** in the page: no
   value comes back, and no terminal / process output / DAP / SQL chunk appears.
   The rejected-message count is **not** rendered in the panel, so read it from
   an agent `browser_status` before and after: it must rise by one (the host
   counts an unrecognised page message rather than applying it).

   *Fail — STOP AND DO NOT SHIP:* any global is `object`, or the page receives a
   channel payload. The whole engine choice (raw wry rather than a Tauri child
   webview) rests on this property. The spike observed it, and reading the
   production path confirms it builds the same way — raw
   `wry::WebViewBuilder::build_as_child`, no Tauri webview API anywhere, and the
   one `with_initialization_script` is `cb_core::browser::script::init_script`,
   which defines only `window.__CB_GLOBAL__` and touches nothing Tauri-shaped.
   But it has **not** been re-confirmed live against the shipped code.

2. **The consent banner names the asking process.** Open a browser panel, load
   any page, then from an agent call `browser_page_text`. *Pass:* the panel
   shows a banner naming the real client — e.g. "codex.cmd (pid 12345) wants to
   read app.example.com" — with the two buttons labelled exactly
   `consent::READ_CONSENT_ACTION` and `WRITE_CONSENT_ACTION`. *Fail:* a generic
   "an agent" with no process name or pid, which would mean
   `GetNamedPipeClientProcessId` / `QueryFullProcessImageName` returned nothing
   and the fallback is being shown.
3. **A read is refused until the button is clicked.** Same setup, call
   `browser_page_text` and do **not** click. *Pass:* the tool answers a refusal
   that quotes the button label, and no page text appears in the transcript.
4. **Consent does not survive an origin change.** Grant read, confirm
   `browser_page_text` works, then navigate the panel to a **different origin**
   by hand. *Pass:* the next `browser_page_text` is refused again. *Pass also:*
   a same-origin path change does **not** re-prompt.
5. **Writes need the stronger grant.** With only read granted, call
   `browser_click`. *Pass:* refused, naming the write consent label — a read
   grant must never license a single write.
6. **`browser_navigate` from a blank panel is refused.** Open a panel and do not
   load anything. *Pass:* refused. This is deliberate (`about:blank` has no
   origin, so no grant can name it) — do not "fix" it.
7. **The automation writes against a real document.** With read-and-control
   granted: `browser_read_page`, then `browser_click` and `browser_type` using a
   `ref` from that outline, then `browser_press_key`. *Pass:* the page visibly
   changes and each tool reports success. Then navigate away and reuse an old
   `ref`. *Pass:* it resolves to nothing and says so, rather than clicking
   whatever now occupies that position.
8. **`browser_network_requests` states its own coverage.** Load a page that
   issues a `fetch`. *Pass:* the answer's last line is
   `cb_core::browser::model::NETWORK_COVERAGE_NOTE` verbatim (it begins
   `instrumented:` and states that there are no headers, no bodies, and no
   statuses outside fetch/XHR rows), and a request issued *before* the init
   script ran is absent — which is expected, and is exactly why that note must
   be present on every network answer.
9. **A squatter on the pipe name.** Create `\.\pipe\code-basics.browser.<pid>`
   for the app's pid from another process, then open the panel. *Pass:* the app
   creates **no** pipe and publishes no listener (the `first_pipe_instance`
   fail-closed path), so a client sees `browser_panel_closed` rather than
   talking to the squatter.
10. **The installer entry, end to end.** Plugins → the browser MCP panel →
   install for Claude Code (user scope). *Pass:* `PlanPreview` shows the whole
   final `~/.claude.json` inside its own scroll container without pushing the
   confirm buttons off screen, a `.bak` is written beside it, and the success
   note names the file and says a running agent needs a restart. Then restart
   the agent and confirm `browser_status` answers through the installed entry.
11. **High-DPI and multi-monitor.** Move the window to a display with a scale
    factor other than 1.0 and drag it between monitors. *Pass:* the page tracks
    the panel, or `pageRect` states a refusal. *Fail:* a page painted at the
    wrong place or size. This path has never run at a factor other than 1.0.
12. **Parent window minimize / maximize / restore**, and whether the page
    swallows app shortcuts (Ctrl+N, Escape, Tab) **while it holds focus**. Both
    untested; the spike only confirmed shortcuts work again after clicking back
    into the app DOM.
13. **Downloads and new-window requests.** Click a download link and a
    `target="_blank"` link. *Pass:* both refused (they are refused in code);
    neither has been exercised against a page that actually tries.
