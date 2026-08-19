---
title: Extract Business Rules
id: extract-rules
---
Read this codebase and write down the business-rule invariants it enforces — the constraints that must always hold for the domain to be correct (e.g. "an order cannot ship before payment clears", "a tenant may only read its own records", "a balance never goes negative"). This is an edit-mode task: you will create files.

For each invariant you find:
1. Create `.code-basics/rules/<slug>.md` (make the `.code-basics/rules/` directory if it does not exist). One rule per file; `<slug>` is a short kebab-case name.
2. Give each file front matter:
   ```markdown
   ---
   id: <slug>
   title: <short human title>
   ---
   ```
3. In the body, state the invariant in one or two prose sentences, then note **where it is enforced** — the file(s) and function(s) that check or maintain it, quoted or cited by path.

State only invariants that are actually evidenced in the code — a guard, a validation, a constraint, a check you can point at. Do not invent rules the code does not enforce, and do not restate a general coding convention as a business rule. If you are unsure whether something is a real invariant, leave it out and say why rather than writing a file for it. When you finish, list the files you created.
