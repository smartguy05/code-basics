# Completed — coverage of change (backend + IPC mirror)

Status: DONE (backend + IPC types). Frontend UI is a separate agent's work.

## Files changed

New:
- `crates/core/src/testing/coverage.rs` + `coverage_tests.rs`
- `crates/core/src/testing/changecov.rs` + `changecov_tests.rs`
- `.memories/features/coverage-of-change/*`

Edited:
- `crates/core/src/testing/mod.rs` — declare `changecov`, `coverage` modules.
- `crates/core/src/model.rs` — `CoverageFormat`, `CoverageSpec`,
  `Invocation.coverage`; key-pinning tests.
- `crates/core/src/invocation.rs` — `build_with` + `build_coverage`; thread
  `coverage` into `build_dotnet`/`build_node`.
- `crates/core/src/adapters/dotnet.rs` — `BuildContext.coverage`; `--collect`
  + Cobertura spec in `test_invocation`. `+ coverage: None` on other literals.
- `crates/core/src/adapters/node.rs` — `test_invocation(.., coverage)`; vitest +
  jest lcov flags; Lcov spec.
- `crates/core/src/adapters/dotnet_tests.rs`, `node_tests.rs` — coverage tests
  + non-coverage byte-identical tests; helper updates for new params.
- `crates/core/src/adapters/manifest.rs`, `crates/core/src/git/repo.rs`,
  `crates/core/src/process/mod.rs` — `coverage: None` on Invocation literals.
- `src-tauri/src/commands/inspect.rs` (x2), `commands/review.rs` — `coverage: None`.
- `src-tauri/src/state.rs` + `state_tests.rs` — `last_coverage` + record/previous.
- `src-tauri/src/commands/run.rs` — `with_coverage` arg, coverage collection,
  `collect_change_coverage`, `resolve_coverage` (+ tests), `coverage_of_change`.
- `src-tauri/src/lib.rs` — register `coverage_of_change`.
- `src/ipc/types.ts` — `UncoveredLine`, `FileChangeCoverage`, `ChangeCoverage`.
- `src/ipc/api.ts` — `coverageOfChange`, `withCoverage` on `runTests`.
- `docs/INDEX.md` — regenerated.

## Tests added (all passing)

- coverage_tests: cobertura_reads_line_hits_per_file,
  cobertura_merges_two_classes_for_the_same_file, lcov_reads_line_hits_per_file,
  load_report_reads_lcov_from_the_file_the_spec_names,
  newest_cobertura_picks_the_most_recently_written_report
- changecov_tests: classifies_changed_lines_as_covered_or_uncovered,
  abstains_on_a_changed_line_the_report_never_mentioned,
  an_unmatched_file_warns_and_fabricates_nothing,
  an_ambiguous_match_warns_rather_than_guessing
- model::tests: a_coverage_invocation_carries_its_coverage_spec_across (+ enum
  variant assertions)
- dotnet_tests: coverage_mode_collects_xplat_coverage_and_sets_the_spec,
  a_non_coverage_test_run_is_byte_identical_to_today
- node_tests: coverage_mode_adds_the_vitest_lcov_flags_and_sets_the_spec,
  coverage_mode_adds_the_jest_lcov_flags, a_non_coverage_run_is_byte_identical_to_today
- state_tests: coverage_is_recorded_per_root_and_read_from_the_active_workspace
- commands::run::tests: resolve_coverage_returns_the_cached_map_unchanged,
  resolve_coverage_abstains_to_an_empty_map_with_a_warning

## Final camelCase JSON shape (frontend contract)

ChangeCoverage {
  files: FileChangeCoverage[]  // FileChangeCoverage { path, uncovered: UncoveredLine[], coveredChanged, uncoveredChanged }
  changedLines: number         // covered + uncovered (classifiable only)
  coveredLines: number
  uncoveredLines: number
  warnings: string[]
}
UncoveredLine { line, index }   // line = new-side source line; index = DiffLine.index (highlight anchor)

## Notes / gotchas

- `mermaid_id_matches_committed_fixture` fails pre-existing on this branch;
  unrelated to this work (no architecture code touched). Regenerate with
  `UPDATE_FIXTURES=1 cargo test -p cb-core mermaid_id_matches_committed_fixture`
  if it needs fixing separately.
- `File::set_modified` needs a WRITE handle on Windows (open with OpenOptions),
  not the read-only handle `File::open` gives — used in the newest_cobertura test.
- Cobertura spec.path is a DIRECTORY (coverlet writes into a GUID subfolder);
  consumer uses `newest_cobertura`. LCOV spec.path is the lcov.info file.
