# Example business-rule docs

A *rule doc* is a business invariant your team writes down in plain markdown —
something the code must always uphold, stated once so a reviewing agent can
judge a diff against it. Unlike an erosion rule (`examples/erosion/`), a rule
doc carries no regex and matches nothing on its own: it is prose, injected into
a review as **context** so the agent reads the rules *before* the instruction
that acts on the diff.

## Using them

Copy a `.md` file into `.code-basics/rules/` in a workspace. Rule docs are
**committed** like erosion rules and declarative adapters — the whole team
shares them — so `rules/` is deliberately not gitignored.

`list_rules` loads every `*.md` file in that directory, sorted deterministically
by filename. A file that will not read is reported in `warnings`, never dropped.

## File format

The same `---`-fenced front matter the Enhancements library uses, then a
markdown body:

```markdown
---
id: money-minor-units
title: Money is stored in minor units
---
Every monetary amount is an integer count of the currency's minor unit
(e.g. cents), never a floating-point value. Conversions to a display string
happen only at the presentation edge.
```

Both front-matter keys are optional and abstain to safe fallbacks rather than
dropping the file:

- `id` — a stable slug. Missing or blank ⇒ the file stem (`money-minor-units.md`
  becomes `money-minor-units`).
- `title` — a human label. Missing or blank ⇒ the first `#` heading in the body,
  or the file stem if there is no heading.

A file with no front matter at all is still a valid rule doc: its whole text is
the body, its id is the file stem, and its title is the first heading.

See `money-minor-units.md` in this directory for a copyable starting point.
