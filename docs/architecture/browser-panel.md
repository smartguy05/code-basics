# The browser panel

A floating window inside the app hosting a real web page, so "does my deployment
work" is answerable without leaving it. One instance for the whole application,
not one per open codebase.

- Decisions: [`crates/core/src/browser/`](../../crates/core/src/browser/mod.rs)
- Host (the webview itself): [`src-tauri/src/browser/`](../../src-tauri/src/browser/mod.rs)
- Commands: [`src-tauri/src/commands/browser.rs`](../../src-tauri/src/commands/browser.rs),
  documented in [the command reference](../reference/commands.md#embedded-browser)
- UI: `src/components/BrowserPanel.tsx` over the tested `browserPanelLogic.ts`

## Why the engine is raw `wry` and never a Tauri webview

This is a security finding, not a preference.

`tauri-2.11.5/src/manager/webview.rs::prepare_pending_webview` pushes
`__TAURI_INTERNALS__`, the invoke bootstrap and the invoke key into **every**
webview Tauri creates — remote content included — and the scripts are *appended*,
so a builder cannot opt out.

The ACL rejects remote-origin invokes with one exception:
`plugin:__TAURI_CHANNEL__|fetch` is excluded from the origin check for *all*
origins (`src/webview/mod.rs:1820-1850`). Its handler
(`src/ipc/channel.rs:318-337`) reads a **process-global** queue keyed by a
monotonic `u32`, never checks that the requesting webview owns that channel, and
`remove`s the entry. In this app the channels carrying above-threshold payloads
are the PTY terminals (where the user runs a coding agent and pastes tokens),
process output, debug-adapter output and SQL rows. A remote page in a
Tauri-created child webview could poll ids and both read and steal those chunks.

So the page is created by **raw `wry`**, where the Phase 4 spike observed
`__TAURI_INTERNALS__` as `undefined` on every page load. The only bridge from the
page is the single `with_ipc_handler` the host installs, and everything crossing
it is parsed by `cb_core::browser::ipc::parse_page_message`, whose entire
capability is *append to a bounded ring, or set a title*.

Two spellings matter:

- **`tauri::wry` does not exist on Windows.** `tauri-2.11.5/src/lib.rs:179-184`
  is `#[cfg(all(feature = "wry", target_os = "android"))]`. `src-tauri` depends on
  `wry` directly, pinned to the version already in `Cargo.lock`, so there is
  exactly one wry in the tree (adding it changed one line of the lockfile).
- **A capability granted by *window* label reaches every webview of that window**
  (`RuntimeAuthority::resolve_access` matches `webviews.any() || windows.any()`),
  and a child webview of `main` carries window label `main`. That is why
  `capabilities/default.json` grants by `webviews`.

**The main window's CSP is not involved.** `manager/webview.rs:485-493` are the
only two `csp` references and both sit inside the `WebviewUrl` data-URL branch,
so the configured CSP never applies to a separate webview. `tauri.conf.json`
needs no `frame-src`, no `connect-src` and no `dangerousRemoteDomainIpcAccess`.
If a page fails to load, **do not "fix" a CSP error — there is none.**

## An OS webview is not a DOM layer

The page is a WebView2 child HWND. It composites **above the whole DOM**: it
ignores `--z-panel`, `--z-notes` and `--z-overlay` in `styles.css`, and `hidden`
on a React div does not hide it. Confirmed by image: Notes, Search Everywhere and
the *Optional features* modal — the one a user would open to switch the browser
off — are all clipped by the page while the panel is open.

**This is accepted. There is deliberately no occlusion mechanism.** Three things
hide the page, and all three go through the host:

| Case | Mechanism | Why not the others |
|---|---|---|
| Minimized | `browser_set_visible(false)`, panel stays mounted | The page, its session and its running SPA survive — the `SqlPanel`/`AppOutputPanel` rule |
| Plugin switched off | `browser_close`, which **drops** the wry `WebView` | A disabled browser must keep no WebView2 process, no cookie jar and no connection. Measured: six `msedgewebview2` processes → zero, and the child HWND gone |
| Unusable rect | left hidden, with `hiddenPageReason` rendered in its place | The `createResizeGate` 0×0 lesson, extended to the DPI case |

## Why the webview is not in `AppState`

A wry `WebView` is `!Send` and main-thread-affine. `AppState` is `Sync`, shared by
every command, and `set_workspace` clears caches *while holding a
`std::sync::Mutex` guard* — so nothing in it may be main-thread-bound and nothing
in it may need to `.await`.

So the resource lives in a `thread_local!` on the main thread, and `AppState`
holds only **data** (`BrowserShared`: availability, url, title, consent, two
bounded rings, two refusal counters). Every operation crosses to the main thread
through `AppHandle::run_on_main_thread` and returns on a `tokio::sync::oneshot`.
This is the `LspHandle` reasoning: never an `Arc<Mutex<the resource>>`.

**Consequence: every browser command is `async`.** Tauri warns at
`src/webview/mod.rs:1290` that webview creation deadlocks when called from a
synchronous command or an event handler on Windows. A synchronous command here
would hang rather than fail to compile.

Because `BrowserShared` is plain data, every rule about *state* is provable with
no webview in existence — consent dying with the page, `Ready` never being
reported over a half-loaded document, a page's fabricated `evalReply` being
counted rather than applied. Those are `src-tauri/src/browser/shared_tests.rs`.

## Defence in depth

Each of these is one builder call in the host with its reason beside it:

- navigation refused for everything `browser::origin::navigation_verdict`
  refuses — the app's own origins, the dev server, `file:`, `javascript:`,
  `data:`, every custom scheme, every `about:` page but `blank`;
- a **separate** WebView2 user-data directory (`<LocalData>/<id>/browser`) — both
  isolation and a hard requirement, since two environments sharing a folder with
  different options fail to create. Verified: its own browser process tree;
- popups denied (`NewWindowResponse::Deny`) — a popup would be a webview outside
  the navigation handler, outside the panel's rect, with no URL bar;
- downloads refused — a browser panel that can write files is a bigger feature;
- clipboard, devtools, autoplay and back/forward gestures all off;
- exactly one non-configurable page-side global, from
  `cb_core::browser::script::init_script`.

Automation consent (`AutomationConsent`) is **withheld by default**, scoped to an
origin, reset on any origin change including a redirect's landing url, and never
persisted. `browser_set_automation_consent` is the only thing that moves it.

Three things follow from consent being per page, and each was a real hole:

- **The rings are the document's, not the session's.** `note_navigation` clears
  both console and network logs when the url changes, and `note_load_finished`
  clears them when the *origin* moved without a navigation event (an SSO bounce
  seen only there). Without it a checkout page's `console.error` was answered as
  the settings page's complete record, and cross-origin it was worse: consent
  granted for the page now on screen served the rows of a page consent was
  never given for. `note_navigation` fires up to three times per load, so the
  clear is keyed on the url changing rather than called unconditionally.
- **Cleared is not evicted.** `Ring::clear` records where the page boundary
  fell, so a reader is told separately how many entries went with a page that is
  gone (`discarded`) and how many this page's own log lost to the cap
  (`missed`). Only the second means the record in front of it is partial.
- **The read must have run on the granted page.** The gate releases the lock
  before the injected script executes, so a `meta refresh` can replace the
  document in between. `page_text_script` and `read_page_script` return their own
  `location.href`, and `consent::read_ran_on_the_granted_page` refuses the answer
  when it no longer matches the grant - including when the page reported no
  address at all, which is not assumed to match.

## Two header gotchas this panel re-proved

The URL bar is an `<input>` inside the draggable header, so both of the
`styles.css`-adjacent rules in CLAUDE.md apply directly:

- **Pointer capture eats `click`.** A control inside a header that drags via
  `setPointerCapture` receives no clicks unless the press is exempted from the
  drag. But the exemption list here is `button, input, textarea, select` and
  deliberately **not** `strong`, unlike `TerminalPanel`'s where the `<strong>` is
  the rename target: the URL bar is `flex: 1`, so the label is the only grab
  area, and exempting it shipped a panel that could not be dragged at all —
  found by driving the real app and watching the page's child HWND never move.
- **Focus set during `pointerdown` is undone by the default action**, so the URL
  bar focuses inside a `setTimeout(…, 0)`.

Related: the panel must not default to nearly the whole viewport, or
`clampPanelPosition` saturates on the first drag and it reads as a window that
will not move.

## What the instrumentation cannot see, stated as data

`browser_network` is **not** DevTools' network panel and every answer carries
`coverage` (`NETWORK_COVERAGE_NOTE`) saying so: rows come from a patched
`fetch`/`XMLHttpRequest` plus a `PerformanceObserver` over resource timings, so
there are no headers, no bodies, statuses only for fetch/XHR rows, and nothing at
all from a request issued before the init script ran. Console capture is the same
shape: `console.*`, `onerror` and `unhandledrejection` only.

Two more admissions that are in the code rather than only here:

- **`BrowserAvailability::Failed` is narrower than it looks.** wry exposes no
  `NavigationCompleted`/`IsSuccess`, and `PageLoadEvent::Finished` fires for the
  browser's own network-error page exactly as for the requested one. So a page
  that failed to resolve is reported `Ready` over an error document, and `Failed`
  is reserved for a navigation this app could not start.
- **Back and forward go through `history.back()`/`history.forward()`**, because
  wry 0.55.1 has no `go_back`/`go_forward` on `WebView` — `reload` is its only
  navigation verb. A page with empty history does nothing and reports success,
  because the DOM gives no answer either way.

## Agent access: the pipe, the server, and what actually guards it

An agent's MCP server is a **separate process** (`cb-app mcp-browser`, the fifth
self-dispatch mode of this executable) started by the agent, not by the user. It
owns no browser state and phrases nothing: every tool call is forwarded to a
running application over a named pipe, and the reply is *that application's own
words*, composed by `cb_core::browser::render`. Putting the six-state
distinction and the consent refusals in the shim as well as in the panel would
have been two copies of the same wording, drifting.

### Finding the application

`<config>/code-basics/browser-instances.json` (`CB_BROWSER_INSTANCES_PATH`
overrides), written atomically and read **tolerantly** — a corrupt file loads as
*no instances*, never as an error, because a server that fails to start is
dropped silently by its host and the user never learns why.

The entry holds only durable facts: a pid, the executable that wrote it, the
protocol that build speaks, whether the plugin is on, and — only while a browser
panel is open — the pipe name and a token. Liveness is **never** one of them:
the entry outlives a crash, so `instances::choose_instance` takes liveness as a
parameter and `liveness::alive` re-probes, comparing the **executable path** as
well as the pid. A pid is not identity.

The listener exists only while a panel is open, and that is what makes five of the six
refusals readable from the file:

| the file says | the answer |
| --- | --- |
| no entry, or the pid is dead / a different exe | `NoneRunning` |
| `browserFeature: false` | `PluginDisabled` |
| feature on, `listener: null` | `PanelClosed` |
| two entries with listeners | `Ambiguous` — **refuses, never picks** |
| a different `protocol` | `VersionMismatch` |

`Ambiguous` is the one worth stating twice: driving a browser in a window the
user is not looking at is worse than asking, so there is no most-recent
tie-break. The refusal names the pids and the `--instance <pid>` flag, and
`browser_status`'s answer carries this application's pid so an agent can supply
it. `--instance` is a disambiguator and **not** a permission.

`PanelClosed` has two causes the registry cannot separate — a closed panel, and
a panel that opened while the pipe failed to create — so its sentence names
both. A user told to "open the panel" while looking at an open panel has been
sent to do something they cannot do.

### What guards the pipe, stated honestly

`\\.\pipe\code-basics.browser.<pid>`, never a fixed name: `AppState` is
per-process, and a fixed name would make a second application fail to bind while
the first answered for a codebase it does not have open.

1. **The DACL keeps other accounts out, and nothing else.** It is
   `D:P(A;;GA;;;<this user's SID>)`, and it is **not optional**: a named pipe
   created with a NULL security descriptor gets a default that grants read
   access to *Everyone* and to the anonymous account. If the descriptor cannot
   be built, no pipe is created at all rather than falling back to that.
2. **The token is a speed bump and is described as one.** 32 bytes per launch,
   published in a file the same user can read, so it reduces "any local program"
   to "any program that can read this user's `%APPDATA%`".
3. **Consent is the actual control.** No token and no pipe access grants
   automation. The most a rogue local process gets with the token and no user
   click is learning that a panel is open — which is also all `browser_status`
   reveals, and it answers in every state on purpose.
4. **The banner names the asker**, from `GetNamedPipeClientProcessId` and the
   client's process image — never from anything the caller sent, since a
   self-reported name is the one field a rogue caller would lie about. An
   unidentifiable caller is reported as unidentified rather than given a
   plausible name.

`first_pipe_instance` is set on the first instance, so a squatter already
holding the name makes creation **fail** and no listener is published.

### The tools

Read-only: `browser_status`, `browser_current_url`, `browser_page_text`,
`browser_read_page`, `browser_console_messages`, `browser_network_requests`.
State-changing, needing the stronger grant: `browser_navigate`,
`browser_click`, `browser_type`, `browser_press_key`, `browser_back`,
`browser_forward`, `browser_reload`.

`browser_navigate` is a **write**: moving the page the user is looking at
mutates state they can see, in a window they may not be watching.

`browser_status` always answers, with no consent and in every state. Gating it
would make "switched off" indistinguishable from "refused", and it is the tool
that explains every other refusal.

**There is deliberately no `browser_evaluate`.** Arbitrary JavaScript in a live
authenticated session is a general-purpose credential-exfiltration and
account-mutation tool: it can read every cookie the page can, fetch any
same-origin endpoint with the user's session, and post anywhere they are logged
in. No consent wording makes that a proportionate default, and "the user clicked
Allow once, on this page" cannot mean "and also authorised arbitrary code
against every credential this session holds". The click/type/press-key tools are
the narrow alternative: each addresses **one element by `ref`** from
`browser_read_page`'s own outline, and the reference is JSON-encoded so it
cannot escape into the script body.

Encoding alone was not enough, and the reason is worth keeping. Script assembly
was a *chain* of `str::replace`, and while `replace` does not rescan its own
replacement, a chain rescans everything the previous link emitted. `type_script`
has two caller-supplied placeholders, so a reference spelling `__CB_TEXT__` was
emitted as the string literal `"__CB_TEXT__"` and the next link rewrote that
placeholder inside the literal it had just written - splicing the encoded text
between two quotes, into a code position. `json_string` does not escape `'`,
`+`, `(` or `)`, so `browser_type` became the `browser_evaluate` this section
says is absent. Assembly is now `script::substitute`, one pass that never
re-reads what it has emitted.

Every network answer repeats `NETWORK_COVERAGE_NOTE` **as data**, empty list
included: these rows are not a browser's network panel, and an empty list is not
evidence that nothing happened.

### Installing it

`browser::install` reuses `mcp::install` wholesale — `config_target` for the
three files MCP servers actually live in, `mcp_merge` for the pointer-based JSON
merge, `codex_toml` for the `toml_edit` merge, and `apply_writes_atomically` for
the write. There is no second merge implementation. `BrowserMcpPanel` likewise
reuses `mcpServerLogic` and the shared `PlanPreview`.

What is **not** shared is the caveats. What an agent gains here is the page the
user is looking at in their own logged-in session, so `PAGE_ACCESS_NOTE` says
exactly that, verbatim, in every scope — and the panel says *installing this
grants nothing on its own* before anything else.

There is no `--workspace` argument: a browser panel belongs to a window, not a
repository, so scoping it to one would be a flag that reads like a boundary and
is not one.
