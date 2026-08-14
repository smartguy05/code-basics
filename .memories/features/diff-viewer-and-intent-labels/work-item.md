# Diff viewer parity with Rider, and intent labels that earn their title

Reported 2026-08-13 from real review work in the app.

## What was asked

**Diff viewer** — copy Rider's diff viewer (screenshot supplied: per-pane line
number gutters, change-marker strip on the outer edge, ↑/↓ jump-to-change,
"Side-by-side viewer" / "Do not ignore" / "Highlight words" dropdowns, a
"1 difference, 0 included" counter):

1. No way to scroll horizontally.
2. A marker on the outer edge showing where changes are, so they are easy to
   scroll to.
3. Buttons to jump to the next / previous change, F7 as the hotkey.
4. Hard to read — not much contrast, text too small, and the size should be
   changeable.
5. Horizontal scroll matched between the two panes so a line can be compared
   while scrolling sideways.

**Intent grouping** — the grouping itself now works, but the stated cause is
poor: three files were titled *"The workflow is running with Opus"*.

## Decisions taken with the user

- Intent fix: **update the Stop hook to require a better intent title** (the
  user's own suggestion, chosen over abstaining or gating alone).
- Toolbar scope: navigation + readability **plus** the whitespace-ignore
  dropdown and collapse-unchanged, and a difference counter.
- Font size: **app-wide**, not diff-only (file editor and diagram editor too).
- History tab: **switch it to the same viewer** (it renders a plain `<pre>`
  today), which needs new IPC for a commit's file contents.

## Acceptance

Each numbered complaint above is fixed and demonstrated in the running app —
none of Part 1 is provable by unit test alone. A card either carries a title the
agent actually declared, or says honestly that it has none.

Full plan: `plan.md` beside this file.
