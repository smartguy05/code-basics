# Plan

Tests-first, in order:

1. Extend `rules_tests.rs::categories_serialise_in_camel_case` with SchemaRisk
   (appended last) and add `builtin_rules_include_schema_risk_detectors`
   asserting non-empty and all `RuleSide::Added`.
2. Add `scan_tests.rs` test feeding an added `migrationBuilder.DropColumn(...)`
   line in a `.cs` file, asserting exactly one SchemaRisk flag.
3. Implement in `rules.rs`: `SchemaRisk` variant after `Secret`; `const SQL`;
   seven `r(...)` rules (3 CS migrationBuilder, 4 SQL DDL), all Added.
4. Frontend: `types.ts` union `| "schemaRisk"`; `erosionLogic.ts`
   CATEGORY_ORDER (after removedSafeguard) + CATEGORY_LABEL.
5. Docs/examples: `examples/erosion/custom.toml`, `docs/reference/commands.md`.

Verify with `cargo test -p cb-core erosion::` (Git Bash / sh on PATH).
