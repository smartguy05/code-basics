# Notes — UI + browser bug batch

## Bug 3 (Ctrl+F) — why the multi-handler pattern, not a dispatchShortcut change
`dispatchShortcut` deliberately `preventDefault`s any matched chord (the F5→reload
guard), so letting Ctrl+F "fall through" to CodeMirror is not an option. The fix
keeps it inside the command system: `executeCommand` runs every handler for an id
newest-first until one returns non-`false`. FileEditor and OutputConsole both
register `console.find`; each guards on focus so exactly one acts. If you add a
third find surface (e.g. the diff pane's CodeMirror — currently still can't
Ctrl+F), register another focus-guarded `console.find` handler the same way.

## Bug 1 (tab blink) — the flash is edge-triggered now
The terminal attention flag is sticky (clears only on focus); Codex rings the bell
constantly. The tab flash is decoupled into a bounded pulse in
`workspaceTabsLogic.ts` (`pulseAttention` is idempotent while flashing/settled).
It re-arms only via `acknowledgeAttention` (on `activateWorkspace`/close) or when
`attentionByRoot[root]` falls to false. The TerminalPanel pill latch is unchanged.

## Bug 5 (LSP warm) — Python stays lazy on purpose
`Language::from_ecosystem` maps only the three built-in ecosystems (dotnet/node/
cargo). Manifest-adapter ecosystems ("pytest", …) are arbitrary user text, so
Python is not eagerly warmed — it keeps starting lazily (no regression).

## Bugs 6+7 (browser per-workspace) — running as a Workflow
User chose "true per-workspace webviews" and to execute via a Workflow (Opus).
Launched workflow `browser-per-workspace` (run wf_54018998-772). Phases: Design →
Implement (tests-first) → Review (security/lifecycle/contract/frontend/tests) →
Verify → Fix. The single `thread_local HOST` and one `BrowserShared` become
per-root; the per-process agent pipe must resolve the ACTIVE workspace only, and
consent must stay per-root (the cross-workspace leak risk). After it completes,
RE-RUN the gate manually (per [[verify-agent-claims]]) before trusting the report:
pnpm typecheck; pnpm test; cargo test -p cb-core browser:: ; cargo fmt;
cargo check --workspace --all-targets.
