# Plan

Authored by Codex in plan mode on 2026-09-02; the full text lives only in that
session transcript (`~/.codex/sessions/2026/09/02/rollout-2026-09-02T08-31-52-*.jsonl`,
assistant message index 209). Summarised here so it survives the transcript.

## 1. Configuration identity and Rider import — done

- `.code-basics/config.json` goes to **schema version 2**.
- `config::rider_config_id` derives a Rider id from the normalised project path
  plus name/kind/ecosystem/profile/script, hashed with a hand-written FNV-1a
  (`DefaultHasher` is not stable across Rust releases) behind a readable
  `rider:<project-stem>:<hash>` prefix.
- `config::migrate` rewrites v1 Rider ids on load and carries `favorites`,
  `order` and compound member references through the old→new map.
- `import` prefers `.run/` over legacy `.idea/**/runConfigurations/` for the
  same identity and warns when the two disagree.
- `resolve_compounds` drops an **ambiguous** same-named member with a warning
  rather than picking a project.

## 2. Attached debug runtime — done

- `src-tauri/src/commands/debug.rs`: adapter spawn, DAP handshake, lifecycle.
- .NET: prebuild with the toolbar's configuration/framework, then ask MSBuild
  for the real `TargetPath` (never a guessed `bin/Debug/...`), launch it under
  `netcoredbg` over **stdio**.
- Node: translate the existing package-manager `Invocation` into a `pwa-node`
  launch against the standalone js-debug DAP server over **loopback TCP**.
- Reverse requests are answered (unsupported → an explicit failure response) so
  an adapter cannot deadlock waiting on this client.
- Compounds preflight **every** member before any adapter starts.
- `state::DebugSessions` is the per-workspace session map; `start_run` and
  `start_debug` each cancel the other's generation for the same config id.

## 3. Discovery and UI — done

- vsdbg from Microsoft's C# extension is **no longer discovered**: its runtime
  licence restricts it to the Visual Studio product family. `netcoredbg`
  (MIT) on `PATH` or `CB_DAP_DOTNET`; `js-debug-adapter` or `CB_DAP_NODE`.
- A missing adapter is reported with everything looked for. There is never a
  silent fallback to an ordinary run.
- Debug button in the Run toolbar; `views/debugLogic.ts` owns both decisions.

## 4. Icon — done

`lib.rs` `setup` applies `app.default_window_icon()` to the main window.

## 5. Verification — done

Rust workspace suite, `tsc --noEmit`, vitest, `cargo fmt --check`,
`cargo clippy`, `docs:index`, `docs:check`.
