# Feature: Help → About

Phase 3 of the "Terminal fixes, a shell picker, and Help → About" plan.

## Why

`MenuBar.tsx` had File and Enhancements only, and nothing in the repo consumed a
version API, so there was **nowhere to read which build you are running**. A bug
report against this app could not identify the build it was filed against.

## Acceptance

- A **Help** menu beside File and Enhancements, with one item: *About code-basics…*.
- An About dialog carrying enough to identify a build: app version, commit
  (**marked when the tree was dirty**), build instant, Tauri version, OS/arch.
- Anything that could not be established shows the literal `unknown` — never a
  blank, never a fabricated value.
- `build.rs` never fails the build over the stamp.
