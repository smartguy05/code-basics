# Rename a SQL connection — completed

## What it is

A saved SQL connection can be renamed, from the picker's per-row right-click
menu (`Rename…`) or by double-clicking its name. The typed name is then shown
verbatim — the composite `project · source · key` label the picker synthesizes
for a reference-backed connection stops being derived.

## Why a separate verb, not a `sql_save_connection` round-trip

`SqlConnectionView.secret` is a **redacted** `SqlSecretView`: a literal secret
comes back as a `display` string, never as the connection string. So a caller
holding a view cannot rebuild the `SqlConnectionProfile` an upsert needs — a
rename posted that way would store the display form where the password was and
break the connection it renamed. `sql_rename_connection(id, name)` takes only
the two fields a rename is about, which makes that impossible rather than
merely unlikely. Exactly the reasoning already recorded for
`sql_set_allow_writes`.

## The `user_named` flag

`savedConnectionLabel` composes an identity for a connection it named itself,
and that is right only until the user supplies one. Without a flag, a typed
name containing no `" · "` is indistinguishable from an old generic key and
silently reacquires the prefix on the next read. `user_named` is a separate
fact from the name because **a name alone cannot say who wrote it**.

`#[serde(default)]`, so the file stays at **version 1** — every profile saved
before renaming existed *was* derived, so an absent key means exactly `false`.

Two rules, both pinned by tests named after them:

- `upsert` will not overwrite a name when `user_named` is set. Re-adopting a
  discovered connection (a rescan, a re-save from the form) carries the derived
  name and would otherwise revert the rename, leaving the flag disagreeing with
  the name it guards.
- `upsert` clears `user_named` on a **new** entry whatever the payload asked
  for — the same rule as `allow_writes`, so only the rename verb sets it.

## One acceptance rule

`acceptedConnectionName` (`sqlPickerLogic.ts`) delegates to `normalizeLabel`,
and the manual-create form was moved onto it too. It used a bare `.trim()`,
which strips neither U+0000 nor a bidi override — so the create path could save
a name the rename refuses. That is the two-acceptances disagreement
`acceptedTerminalTitle` exists to prevent.

Worth knowing: `normalizeLabel` replaces control and bidi characters with a
**space** and then collapses, so `a<U+0000>b` cleans to `a b`, not `ab`. A test
asserting `ab` is wrong about the rule, not finding a bug.

## Files

- `crates/core/src/sql/store.rs` (+ `store_tests.rs`) — the field.
- `src-tauri/src/commands/sql.rs` — `rename` mutator, `sql_rename_connection`,
  the `upsert` guard, `user_named` on `SqlConnectionView`; `src-tauri/src/lib.rs`
  registers the command.
- `src/ipc/types.ts`, `src/ipc/api.ts`, `src/components/sqlPickerLogic.ts`,
  `src/components/SqlConnectionPicker.tsx`, `src/views/SqlView.tsx`,
  `src/views/sqlViewLogic.ts`, `src/styles.css` (`.sql-conn-rename`).

The inline-edit mechanics are copied from `TerminalPanel` wholesale, including
the `abandoningRename` ref — unmounting the input fires `onBlur`, so without it
Escape commits the very edit it is cancelling.
