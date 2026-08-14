# Todos — diff viewer and intent labels

Everything in `plan.md` is done; see `completed.md`. What is left is follow-up.

## Needs a real session to confirm

- [ ] **The Stop hook's request, in a live Claude Code session.** It has unit
      tests and the contract is confirmed from the docs (exit 2 on `Stop`
      "prevents Claude from stopping"), but no agent has actually been asked
      yet. The installed hook runs `target/release/cb-app.exe`, so **a release
      build is needed before it does anything.** Check: a turn that edits files
      with no `Intent:` line is asked once; ignoring the request still lets the
      turn end; a conversational turn is never asked.

## Worth watching

- [ ] **The narration gate's false positives.** `TOOLING_WORDS` contains
      "agent", which is this repository's own domain vocabulary — a legitimate
      label like "record what the agent said" is refused here. That is the
      intended trade (no label beats a wrong one, and a declared `Intent:` line
      is never filtered), but if it bites in practice the list is one constant
      in `hook.rs`.
- [ ] **Whitespace-ignore on a file with tabs vs spaces mid-line.** The offset
      mapping is unit-tested but has only been exercised on ordinary source; a
      pathological file is worth a look if a highlight ever lands oddly.

## Small, deliberately not done

- [ ] `src/editorFontSizeLogic.ts` and `src/recentsLogic.ts` sit directly under
      `src/` and the coverage `include` glob (`src/**/*Logic.ts`) does not pick
      up top-level files, so their tests run but do not count. Pre-existing;
      changing the glob was out of scope.
- [ ] The marker strip is on the right edge only. The request said "either
      side", so this is a choice rather than a gap.
