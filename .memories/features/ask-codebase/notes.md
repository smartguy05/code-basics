# Notes — Ask the codebase

## THE BIG ONE: "there is no shell to interpret it" is FALSE on Windows

Found by adversarial review of Stage 2, then independently verified. This is a
**command-injection path**, and it is not specific to this feature — anything that
sends caller-supplied text as argv through the PTY is exposed.

**The chain, all verified on this machine:**

1. `process::resolve::resolve_program` walks `PATHEXT`, so a bare program name
   resolves to a `.CMD` when no `.EXE` exists.
   - `claude` → `C:\Users\AnthonyJames\.local\bin\claude.exe` — a real executable, safe.
   - `codex`  → `C:\Users\AnthonyJames\AppData\Roaming\npm\codex.cmd` — **a batch
     shim**, and there is no `codex.exe` anywhere on PATH.
2. `PtyManager::open` spawns through portable-pty's `CommandBuilder`. Read
   `portable-pty-0.9.0/src/cmdbuilder.rs::append_quoted` directly: it implements
   **MSVC argv quoting only**. It returns an argument *unquoted* unless the arg
   contains a space, tab, newline, vertical tab or `"`. It has **none** of the
   CVE-2024-24576 batch-file mitigation that `std::process::Command` gained.
3. So `cmd.exe` re-parses the line. An argument containing `&`/`|`/`<`/`>`/`^`
   but no space passes through bare and `cmd.exe` treats it as a command
   separator. Even quoted, `cmd.exe` does not honour `\"`, so an odd number of
   quotes plus a metacharacter escapes the quoting. And `%VAR%` is expanded by
   `cmd.exe` *even inside quotes* — a question mentioning `%TEMP%` silently
   reaches the agent as an expanded path, a wrong answer with no signal.

**The contrast that makes it easy to miss:** the *headless* review path goes
through `process::Supervisor` (`tokio::process::Command`), which **does** carry
the std batch fix. Only the PTY path is exposed, and Stage 2 was the first thing
to send arbitrary user prose down it.

**Do not restore the old wording.** The claim "there is no shell to interpret any
of it" was asserted in ~5 files (`commands/review.rs`, the `agent_args_interactive`
doc, `terminalLogic.ts`, `ipc/api.ts`, `TerminalPanel.tsx`). It is true for a real
`.exe` and false for a `.cmd`/`.bat` target; the distinction is the whole rule.

## Ctrl+/ has *two* prior claimants, not one

- `src/components/FileEditor.tsx` binds `Mod-/` to `toggleComment` ahead of
  `defaultKeymap` — CLAUDE.md marks this load-bearing.
- **A focused xterm terminal**: Ctrl+/ is Ctrl+_, which is readline **undo** in
  bash and zsh. The first fix abstained only for `.cm-editor`, so it ate undo in
  the feature's own output window. Any focus-abstain rule must cover both.

The abstain check is DOM-touching, so it cannot live in `askLogic.ts` (vitest runs
in the node environment — no DOM). The split: `AskPanel.tsx` does the
`document.activeElement` query, `askLogic.ts` holds the tested predicate.

## The Codex model was validated and then thrown away

`agent_args_interactive` had `Codex => vec![prompt]`, ignoring `model` — while
`interactive_command` had already called `models_for(Codex)` (which parses
`~/.codex/config.toml`) and `resolve_model` (which *refuses* a name not in that
list). So the app validated the user's choice, persisted it, and ran the default.
Exactly the "distinct outcomes collapsing into one answer" this crate refuses.
It was invisible because `commands/review.rs` had **no Codex test case at all**.

Lesson worth generalising: when a wrapper validates an input, something must
assert the validated value actually reaches the process.

## Process notes

- The `review.rs` agent proved its tests discriminate by landing a deliberately
  naive implementation (delegating to the headless `agent_args`) and watching
  three of the five fail for the right reasons before writing the real one. That
  is the standard to hold — "it failed to compile" is a weaker signal than
  "it failed against a plausible wrong implementation".
- `the_headless_argv_is_unchanged` pins both agents' full headless argv by exact
  equality, so any future disturbance to the Review panel breaks a test here.

### Fixed (Stage 2 follow-up)

`agent_args_interactive` now emits `-m <model>` for Codex when one resolved,
mirroring the headless `agent_args`. `-m/--model` is a **global** Codex flag,
not one of `exec`'s, so the interactive form may carry it. Pinned by
`codex_interactive_argv_carries_a_resolved_model` /
`codex_interactive_argv_omits_the_model_flag_when_none_resolved`, and
`a_multi_line_question_stays_one_argument` now runs both agents **with and
without** a model so no future flag can swallow or split the prompt.

`commands/review.rs` gained its first Codex cases. Note the limit honestly:
Codex's model list is read from the user's own `~/.codex/config.toml`, so a
command-layer test cannot assert a specific model. `codex_runs_the_interactive_
form_with_the_question_last` asserts the shape instead (argv is either just the
prompt, or `-m <model> <prompt>` — never a dropped flag), and
`a_codex_model_the_user_configured_reaches_the_argv` exercises the real path
only where a config supplies a model. The exact-argv guarantee lives in cb-core.

## Ctrl+/ must abstain for any focused text-entry surface (not just CodeMirror)

The first cut of `AskPanel` abstained only on `.cm-editor`. xterm focuses a hidden
`.xterm-helper-textarea`, which matched nothing, so Ctrl+/ was `preventDefault`ed inside a
terminal — and over a PTY Ctrl+/ is Ctrl+_ , readline's **undo** in bash/zsh. The surface it
broke was Ask's own output window.

The rule now lives in `askLogic.shouldAbstainForFocus(FocusedSurface | null)`, with
`TEXT_ENTRY_ANCESTORS` (`.cm-editor`, `.xterm`) and `TEXT_ENTRY_TAGS` (`input`, `textarea`)
plus contenteditable. `AskPanel.describeFocus()` does only the `document.activeElement` +
`closest()` lookup — the pure/DOM split is what keeps the decision testable in vitest's node
environment. Nothing focused is **not** an abstain (that is the shortcut's own case).

## The backend now checks install, not just the picker

`AskPanel` only lists installed agents, so `launchBlockedReason`'s not-installed branch was
unreachable through the picker while the backend checked nothing. `commands/review.rs::
ensure_installed(agent, installed)` takes the installed set as a **parameter** (so it is
testable without PATH) and is called from `agent_interactive_command` with
`review::detect_agents()`. Wording mirrors `launchBlockedReason` so the two surfaces cannot
disagree.

## Two argv holes closed (verified empirically, not inferred)

1. **The batch guard checked arguments but not the program path.**
   `portable-pty-0.9.0/src/cmdbuilder.rs:679` applies the same `append_quoted`
   to the resolved exe path as to every argument, so the path is on the line
   `cmd.exe` re-parses. A shim under a directory named `dev&test` split at the
   `&` (reproduced). `pty::argv::check_batch_argv` now runs a **separate**
   `batch_program_refusal` over `resolved.display()` *first*: the fault and the
   fix differ (move/reinstall the program vs. rephrase the question), so the two
   causes never share a message — pinned by
   `the_refusal_distinguishes_a_bad_program_path_from_a_bad_argument`.

2. **A question starting with `-` was parsed as a flag.** `agent_args_interactive`
   emitted no `--`. Observed on the installed CLIs:
   - `codex "--why-does-the-build-fail"` → `error: unexpected argument ... found`,
     with Codex's own tip naming `-- --why-does-the-build-fail` as the fix.
   - `claude "--why-does-the-build-fail"` → `error: unknown option '--why-...'`.
     (Claude was *not* assumed to match Codex — it was tested.)
   Both accept `--` and treat what follows as the prompt (round-tripped a
   sentinel through `codex exec -- …` and `claude -p --model haiku -- …`, both
   with a dash-leading and a plain prompt). The model flag must sit **before**
   `--`, since everything after it is a positional. Interactive argv is now
   `claude [--model m] -- <prompt>` / `codex [-m m] -- <prompt>`.

   **Not changed: the headless `agent_args`.** Claude's headless form puts the
   prompt right after `-p` with flags following it, so a `--` there would
   swallow `--permission-mode`/`--output-format`; and headless prompts are
   library/inline bodies rather than a typed question. `codex exec` has the same
   dash-leading gap in principle — left alone deliberately, out of scope.
