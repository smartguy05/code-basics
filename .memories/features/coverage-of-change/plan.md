# Plan — coverage of change (backend)

1. `testing/coverage.rs` — `FileCoverage { path, lines: BTreeMap<u32,u32> }`,
   `parse_cobertura`, `parse_lcov`, plus loaders `load_report(&CoverageSpec)` and
   `newest_cobertura(dir)` (walkdir + mtime). Tests in `coverage_tests.rs`.
2. `testing/changecov.rs` — IPC types `UncoveredLine`, `FileChangeCoverage`,
   `ChangeCoverage` (camelCase + specta) and `map_change_coverage`. Tests in
   `changecov_tests.rs`.
3. `model.rs` — `CoverageFormat { Cobertura, Lcov }`, `CoverageSpec { path, format }`,
   `Invocation.coverage: Option<CoverageSpec>`. Key-pinning tests updated.
4. Adapters — coverage gated behind a flag; ordinary runs unchanged.
   - `invocation.rs`: `build` delegates to `build_with(..,false)`; new
     `build_coverage` delegates to `build_with(..,true)`. Lower churn than
     threading `coverage` through `build`'s public signature — existing callers
     untouched. Adapter knowledge stays in adapters (dotnet `BuildContext.coverage`,
     node `test_invocation(.., coverage)`); manifest adapters ignore coverage.
   - dotnet: `--collect:XPlat Code Coverage` + `--results-directory`; spec =
     results **dir** (Cobertura). node vitest: `--coverage --coverage.reporter=lcov
     --coverage.reportsDirectory=<dir>`; jest: `--coverage --coverageReporters=lcov
     --coverageDirectory=<dir>`; spec = `<dir>/lcov.info` (Lcov).
5. `state.rs` — `last_coverage: Mutex<HashMap<String, ChangeCoverage>>` with
   `record_coverage(root, id, cov)` / `previous_coverage(Option<id>)`.
6. `commands/run.rs` — `run_tests` gains `with_coverage: bool`; on success loads
   + maps coverage (WorkingToHead) and caches it. `collect_change_coverage` glue
   fn. New `coverage_of_change(mode)` command → `resolve_coverage` free fn
   (cached or empty+warning), unit-tested. Registered in `lib.rs`.
7. IPC mirror: three interfaces in `types.ts`; `coverageOfChange` + `withCoverage`
   in `api.ts`.
8. `pnpm docs:index`.

## Design decisions / tensions

- Item 5 (store ChangeCoverage) vs item 6b ("diff_all(mode) and maps"): item 5's
  concrete type wins. Mapping happens at collection time in `run_tests` (against
  the working tree); `coverage_of_change` serves the cached mapped result. `mode`
  accepted for symmetry with other diff commands but the cache reflects the tree
  as it stood at collection.
- Missing/unreadable coverage is a **warning**, never a test-run failure (mirrors
  the missing-TRX tolerance).
