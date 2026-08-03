# Test report formats

Every runner integration follows the same pattern: stream live console output, then parse a structured report file on exit ([why](../architecture/overview.md#the-central-design-run-a-command-read-a-report)). Three formats cover every supported runner. All parsers emit a flat `TestCase` list with a `TestSummary`; the tree is built separately so every format yields an identical hierarchy.

A **missing** report file is its own distinct error, because it almost always means the runner never produced one — classically, VSTest-style flags handed to Microsoft.Testing.Platform, which ignores them silently and exits cleanly.

## TRX (`trx`)

Visual Studio's XML report. Parser: `crates/core/src/testing/trx.rs`.

- Emitted by **both** `dotnet test` paths — classic VSTest and Microsoft.Testing.Platform — which is exactly why the .NET adapter targets it: the two runners disagree about nearly every command-line flag but agree on the report.
- Structurally, the data the UI needs is split across two sections joined on `testId`: `<Results>` carries outcomes and timings, `<TestDefinitions>` carries the class name and owning assembly. Neither alone is enough.
- Outcomes map conservatively: anything unrecognised (inconclusive, aborted, timed out) becomes `other`, never a pass.

## Jest-like JSON (`jestLike`)

The `--json` output shape shared by Jest and Vitest. Parser: `crates/core/src/testing/jest_like.rs`.

- Vitest deliberately mirrors Jest's format, so one parser serves both; where they diverge (Jest's suite-level `failureMessage` vs. Vitest's `message`), both spellings are accepted.
- Adapter-side quirk worth knowing: Vitest's JSON reporter *replaces* the console reporter, so the adapter requests both reporters together and passes the file as `--outputFile.json=` — otherwise live output goes silent.

## JUnit XML (`junitXml`)

The universal fallback. Parser: `crates/core/src/testing/junit.rs`.

- pytest, cargo-nextest, go-junit-report, Gradle, PHPUnit, RSpec, and most other runners can emit it — this parser is what lets a [new ecosystem be added with a TOML manifest](../guides/adding-an-ecosystem.md) and no Rust.
- The format is a convention rather than a standard, so parsing is deliberately permissive: the root may be `<testsuites>` or a bare `<testsuite>`, suites may nest, and outcome is expressed by the *presence* of a child element (`<failure>`, `<error>`, `<skipped>`) rather than an attribute.

## From flat list to tree

`testing::tree::build` groups cases into project → suite → test nodes. Roll-up outcome uses worst-wins precedence — `Failed > Other > Passed > Skipped` — so a single failure colours every ancestor, a mostly-passing suite with one skip reads as passing, and an entirely-skipped suite reads as skipped. `failed_names` extracts fully-qualified names of failures for "re-run failed", which each adapter translates into its runner's own filter syntax.

Fixture reports used by the parser tests live in `crates/core/fixtures/reports/`.

Related: [core crate](../architecture/core-crate.md#testing) · [adding an ecosystem](../guides/adding-an-ecosystem.md).
