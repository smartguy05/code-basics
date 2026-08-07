# Object inspector

Read the **actual objects** out of real running .NET code — especially the object involved in a failure. code-basics is a Rider replacement, and inspecting object state was the one thing Rider did that it could not.

## Origin

Originally requested as a "REPL pad" on the Roslyn scripting API. That framing was rejected during planning: a REPL constructs *fresh* objects in a separate process, so it can never show the one that actually threw. Anthony's clarification settled it — "I need the actual objects, especially if one is causing something to fail."

The feature is therefore a **heap reader**, not an evaluator.

## Acceptance criteria

Four failure modes, all confirmed as in scope:

| Mode | Requirement |
|---|---|
| Unhandled crash | Read the objects from the crash dump |
| Test failure | Same, for a failing test |
| Wrong value, no exception | Inspect a running process on demand |
| Caught / logged exception | Find exceptions still resident on the heap |

## Hard limits (state these wherever the feature is surfaced)

- **Reads memory, not a live execution context.** No method can be called — nothing is executing. `EstimateCost()` cannot be run.
- **No property is ever evaluated.** ClrMD reads *fields*, so `public int Count => _items.Length` shows as `_items`. The upside: inspection cannot throw or alter what it inspects.

## Governing rule

Inherited from `git/grouping.rs`: **a wrong value is much worse than no value.** A field rendered as `0` that was never actually read is worse than a visible gap, because the user believes it and debugs the wrong thing. Every uncertainty becomes an explicit `Unavailable`; every cap becomes an explicit `Elided`.
