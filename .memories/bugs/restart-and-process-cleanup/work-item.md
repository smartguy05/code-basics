# Work item: Restart / process cleanup / stop-all / build-solution / setup prompt / LSP restart

Seven related fixes requested by the user (Anthony), all around running/stopping code.

## Requests
1. **Restart bug** — Restart does not end the original process; a new one can't start (port held). Also app close/crash orphans spawned processes — should auto-kill.
2. **Stop All** button (right of Stop) — stop all running code. Scope chosen: **runs only** (leave tests/builds).
3. **Build Solution** button — build all projects in the solution. Scope chosen: **build each project**.
4. **First-open setup prompt** to install agent intent capture — it currently never appears.
5. **Stop Tests** button on the Tests tab.
6. **LSP "restart language server"** action in the C# status dropdown.

## Confirmed root causes
- **Restart:** `Supervisor::run_inner` (crates/core/src/process/mod.rs) overwrites the running-map entry for a reused id without killing the old child; and the exit removes by id unconditionally, so a superseded run's exit evicts the NEW entry → new run becomes uncancellable. Frontend Restart just calls `start()` again (RunView.tsx ~1057) with no stop.
- **Close/crash orphans:** src-tauri/src/lib.rs uses plain `.run(generate_context!())` — no RunEvent::Exit / window-close handler; no Drop on Supervisor; children spawned without kill_on_drop.
- **Setup prompt never appears:** `needsSetup` (src/components/setupPromptLogic.ts) treats a USER-scope (global ~/.claude/settings.json) install as installed. Anthony has record-intent + quality-gate hooks at user scope, so status is Some("user") for every repo → needsSetup false everywhere. Fix: key on PROJECT scope.
- **LSP failed:** all failure origins surface as Availability::Failed; restart fixes transient crash (2nd death in 60s window) but NOT missing-binary/handshake/misconfig. Add lsp_restart command + dropdown row w/ caveat.

## Scope answers (from user)
- Setup prompt: "It never appears" (bug).
- Build solution: "Build each project".
- Stop All: "Runs only".

Plan file: C:\Users\AnthonyJames\.claude\plans\hit-a-bug-restart-clever-newt.md
