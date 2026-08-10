# Bug: intent cards explode into 133 generic "New X" groups

Reported 2026-08-07. Changes tab shows 133 single-file/single-hunk cards titled
"New import", "New usize", "New AtomicU64" instead of intent-titled multi-edit
cards ("Fix textbox input error").

## Root causes (verified)
1. User-scope hooks bake `--workspace <install root>` (hooks_json.rs:101-111) —
   installed hooks pinned to ONEflight, so edits in every other repo dropped by
   `relative_to`. No `edits.jsonl` exists anywhere; ONEflight has 52 orphaned labels.
2. code-basics has no `.code-basics/intents/` dir → `hook::is_enabled` false.
3. `intent_groups` never mines transcript history; 21 sessions unused.
4. Empty records → attribution abstains → symbol fallback makes ~1 card per hunk,
   silently.
5. `declaration_name` "last identifier wins" names the type (`usize`, `AtomicU64`)
   for `let x: Type` forms.
6. `symbol_from_header` emits any ≤80-char header ("New import" cards).
7. Rust label repeats badge verb → "NEW  New import".
8. No `Intent:` instructions in CLAUDE.md → Stop labels are prose fragments.

## Acceptance
- Intent-titled cross-file cards when intent data exists; visible banner with
  Enable/Import actions when it doesn't; fallback collapses singleton symbol
  cards per file; binding names not types; no verb duplication.
- User-approved: also repair ~/.claude/settings.json hooks, enable capture here,
  import past sessions.
