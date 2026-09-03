# Todos

- [ ] **Decide what to do about `IntentGroup.candidates`.** It is provably dead
      (see `completed.md`): the ambiguous-card branch that populated it is
      unreachable, so the field is always empty. Either delete the dead branch
      and the field — updating `model.rs`'s key-pinning test, `src/ipc/types.ts`
      and the Intent UI **together**, per the IPC contract rule — or, if
      ambiguous cards are meant to come back, restore the behaviour and give it
      a test. Right now the code claims a capability it does not have.
- [ ] **Consider a `.gitattributes` rule for `*.rs`.** The working tree is
      mixed (`grouping.rs` LF, `sqlite.rs` CRLF) under `core.autocrlf=true`.
      Only `crates/core/fixtures/**` is pinned to LF today. A broader rule would
      stop this class of bug but rewrites every developer's working tree, so it
      is a deliberate call, not a drive-by. The narrower rule already applied:
      **a source pin must normalise line endings or go through `str::lines`**,
      which drops a trailing `\r`.
