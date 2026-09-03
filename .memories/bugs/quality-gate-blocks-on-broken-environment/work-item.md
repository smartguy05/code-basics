# The quality gate blocked turns on a broken environment

Reported 2026-09-02, after the `Stop` hook refused to let a turn end.

## Symptom

`pnpm typecheck` exited non-zero with ~3,860 diagnostics — `Cannot find module
'react'`, `'vitest'`, `'@codemirror/*'`, `'lucide-react'` — and the gate blocked
the turn and fed the whole wall back to the model.

## Root cause

None of it was about the code. Every package was really installed under
`node_modules/.pnpm/`; only the junctions at `node_modules/<pkg>` were unreadable
from that process (the documented Windows "untrusted mount point" block). The
gate had two answers available — pass or fail — and a run that could not reach
its own dependencies does not fit either.

Two things made it worse than a false positive:

- **The turn could not be unblocked by any edit.** No change to the source could
  make `tsc` resolve a directory the OS refuses to traverse, so the gate had
  wedged the session rather than gated it.
- **It flooded the context.** ~3,860 lines of cascade, none of them verdicts.

## Fix

A third answer, `GateVerdict::Unrunnable`: never blocks, never passes silently,
says out loud that nothing was checked, and caps its output at 20 lines.

Also fixed by the same rule: `cargo clippy` failing with `Access is denied.
(os error 5)` because the app is running and Windows will not let the linker
replace its exe — another documented local hazard that is not a code defect.

## Deliberate non-goal

**The gate never sifts a broken run for the "real" errors.** Once the dependency
tree is unreachable, `any`-poisoned diagnostics are indistinguishable from
genuine ones; choosing between them would be exactly the guess this codebase
refuses everywhere else. It abstains from the whole run and says so.
