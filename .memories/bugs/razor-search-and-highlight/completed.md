# Completed — .razor/.cshtml search + highlighting

## Symptom
`.razor` pages "don't show up in search and don't show proper code highlighting."

## Findings
- **File-name search already worked**: `symbols::index::walk` collects every file
  from `workspace::source_walker` (no extension filter), so `Counter.razor`
  appears in go-to-file. The real gap was **symbol** search.
- `.razor`/`.cshtml` were absent from `PARSABLE_EXTENSIONS`, so the C# inside
  `@code` / `@functions` / `@{ … }` blocks was never scanned for declarations —
  no methods/fields from a component were jump-able.
- Highlighting: `languageFor` had no case for `razor`/`cshtml`, so they fell to
  `[]` (plain text).

## Fix
- `crates/core/src/symbols/index.rs` — added `"razor"`, `"cshtml"` to
  `PARSABLE_EXTENSIONS`. Safe because `declarations` requires a lowercase
  declaring keyword: razor markup and `@page`/`@inject`/`@using` directives
  abstain (no fabricated symbols), while embedded C# (`private void Foo()`,
  `private int count`) indexes via the existing C# rules. `@using` is caught by
  `NOT_A_SYMBOL`. Save/re-index path (`parse_file`) shares the same allowlist.
- `src/components/language.ts` — map `razor`/`cshtml` → `html()` (HTML-dominant
  with C# interleaved; the close-enough approximation, like cpp for C#).

## Tests (first)
- `symbols/index_tests.rs::a_razor_component_indexes_the_csharp_in_its_code_block`
- `components/language.test.ts` — razor/cshtml → "html", case-insensitive.

## Verified
- `cargo test -p cb-core symbols::` → 124 passed.
- `pnpm typecheck` clean; `pnpm test` → 1022 passed; `cargo fmt --check` clean.
