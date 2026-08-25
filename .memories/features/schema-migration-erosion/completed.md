# Completed

## Files touched
- `crates/core/src/erosion/rules.rs` — added `ErosionCategory::SchemaRisk`
  (after `Secret`), `const SQL: &[&str] = &[".sql"]`, and seven built-in rules.
- `crates/core/src/erosion/rules_tests.rs` — extended
  `categories_serialise_in_camel_case`; added
  `builtin_rules_include_schema_risk_detectors`.
- `crates/core/src/erosion/scan_tests.rs` — added
  `an_introduced_migration_drop_column_is_flagged`.
- `src/ipc/types.ts` — added `| "schemaRisk"` to `ErosionCategory`.
- `src/components/erosionLogic.ts` — `schemaRisk` in CATEGORY_ORDER (after
  `removedSafeguard`) and CATEGORY_LABEL
  ("Backward-incompatible schema changes").
- `examples/erosion/custom.toml` — added `secret | schemaRisk` to the category
  comment.
- `docs/reference/commands.md` — appended `secret | schemaRisk` to the category
  enumeration comment.

## Rules added (all RuleSide::Added)
- cs-migration-drop-column  — `migrationBuilder\.DropColumn`   (.cs)
- cs-migration-drop-table   — `migrationBuilder\.DropTable`    (.cs)
- cs-migration-rename-column— `migrationBuilder\.RenameColumn` (.cs)
- sql-drop-column — `(?i)\bALTER\s+TABLE\b.*\bDROP\s+COLUMN\b` (.sql)
- sql-drop-table  — `(?i)\bDROP\s+TABLE\b`                     (.sql)
- sql-not-null    — `(?i)\bALTER\s+COLUMN\b.*\bNOT\s+NULL\b`   (.sql)
- sql-rename      — `(?i)\b(RENAME\s+COLUMN|sp_rename)\b`       (.sql)

## Deferred limitation
"Nullable column without a default" cannot be expressed as one single-line
regex — it needs lookahead, which the Rust `regex` crate does not support. Left
out deliberately rather than shipping an ambiguous rule (a wrong flag is worse
than none).

## Result
`cargo test -p cb-core erosion::` — 25 passed; 0 failed. `pnpm typecheck` clean;
`erosionLogic.test.ts` 8 passed.
