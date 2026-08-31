# Notes — app launcher

## The agent shell cannot run the pnpm gate in this environment (2026-08-28)

Every process the agent can spawn (Bash, PowerShell, and `node` under either) fails to traverse a
**directory junction**, which is how pnpm builds `node_modules`:

```
The path cannot be traversed because it contains an untrusted mount point.   (os error 448)
Error: UNKNOWN: unknown error, open '...\node_modules\.pnpm\...\@vitest\utils\package.json'
```

The same file reads fine at its *real* path under `.pnpm/<pkg>/node_modules/...`; only the junction
hop fails. `icacls` shows normal ACLs, so it is a process-level redirection-trust mitigation, not
permissions. Consequences:

- `pnpm test`, `pnpm typecheck`, `pnpm coverage` cannot run — `vitest` cannot resolve
  `@vitest/utils`, and `tsc` reports every `node_modules` type as missing (`JSX.IntrinsicElements`
  does not exist, `Cannot find module 'vitest/config'`). Those errors are the environment, not the
  code, and `pnpm install --force` cannot repair it (it fails the same way).
- `cargo`/`rustc` **also** fail this way through the `~/.cargo/bin` shims (they are symlinks to
  `rustup.exe`). Workaround that does work: put the real toolchain bin on PATH —
  `/c/Users/<user>/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin` — and invoke
  `cargo.exe` from there.
- `node scripts/generate-index.mjs` and `scripts/check-docs.mjs` are fine: node builtins only.

So the Rust half of the gate is runnable by an agent and the pnpm half is not; ask the user to run
`pnpm typecheck` / `pnpm test` (the `! pnpm test` prompt prefix works). The `Stop` quality-gate hook
runs `pnpm typecheck` whenever a `.ts`/`.tsx` file changed, so it will fail for this reason too.

## TypeScript details this feature had to get right

- **`noUncheckedIndexedAccess` is on.** Every `array[i]` is `T | undefined`: `closeTab` guards the
  neighbour read, `needsShell` guards its char reads, and the new tests use `[0]!` the way
  `notesLogic.test.ts` and `erosionLogic.test.ts` already do.
- **A parameter's narrowing does not survive into a closure.** `applyEvent` returns early for
  `output` and then aliases `const settled = event` before `tabs.map(...)`; without the alias the
  `switch` inside the callback sees the full `ProcessEvent` union again, leaves `output` unhandled,
  and the mapped element becomes `AppTab | undefined`.

## Why the launch key is minted in the frontend

`launch_command` spawns the process and returns; its first output events reach the channel **before**
the promise resolves. A key minted in Rust would therefore arrive too late for the console, so the
frontend mints `ext:<uuid>`, passes it in, and `launch_key(None)` remains the fallback for any other
caller. `App` additionally buffers events that arrive before the console has mounted.
