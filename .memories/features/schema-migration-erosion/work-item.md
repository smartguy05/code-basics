# Feature: SchemaRisk erosion category

Add a new deterministic erosion category `SchemaRisk` that flags destructive,
backward-incompatible schema/migration changes on the ADDED side of a diff.

## Acceptance criteria
- New `ErosionCategory::SchemaRisk` variant serialising to `"schemaRisk"`.
- Built-in rules for EF Core `migrationBuilder.*` (C#) and raw SQL DDL (.sql),
  all `RuleSide::Added`.
- Frontend IPC union + `erosionLogic` order/label updated together with the
  Rust key-pinning test.
- Docs/examples list the new category.

## Constraints
- The erosion scan is a no-model, one-regex-per-diff-side detector.
- The Rust `regex` crate has NO lookahead — only single-line-unambiguous rules
  ship. "Nullable column without default" is deferred (needs lookahead).
- A wrong flag is worse than none (abstain-first).
