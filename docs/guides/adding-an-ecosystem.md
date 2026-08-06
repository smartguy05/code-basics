# Adding an ecosystem (declarative adapters)

Support for a new language or test runner usually needs **no code**. Because every supported runner boils down to "run this command, then read this report file", a new ecosystem needs only a command template and a report format — supplied by a TOML manifest.

JUnit XML is what makes this practical: pytest, cargo-nextest, go-junit-report, Gradle, PHPUnit, and RSpec can all emit it, and the [JUnit parser](../reference/test-reports.md#junit-xml) already exists. If a runner can produce JUnit XML, it can be integrated from a config file.

## Where manifests live

`.code-basics/adapters/*.toml` inside a workspace. Every `*.toml` in that directory is loaded when the workspace opens; a malformed manifest is skipped with its error surfaced, so one bad file cannot disable the rest. Worked examples ship in [`examples/adapters/`](../../examples/adapters) — copy one in and edit.

## A complete example (pytest)

```toml
id = "pytest"          # stable identifier; becomes the config's `ecosystem`
name = "pytest"        # display name

# A directory containing ANY of these files is a project of this ecosystem.
detect = ["pytest.ini", "pyproject.toml", "tox.ini", "setup.cfg"]

[test]
program = "pytest"
args = ["--junit-xml={report}", "-q"]
report_format = "junitXml"
report_extension = "xml"
filter_template = "-k"        # flag that introduces a test-name filter
filter_separator = " or "     # how multiple names are joined

# Optional named application launches, one per [run.<name>] section.
[run.serve]
program = "uvicorn"
args = ["app.main:app", "--reload"]
env = { PYTHONUNBUFFERED = "1" }
```

With this in place, matching projects appear in the Tests view (one test configuration) and the Run view (one configuration per `[run.<name>]`) alongside the built-in .NET and JS/TS ones.

## How matching works during a scan

A directory is claimed by a manifest when it contains any file named in `detect`. Two rules keep that predictable:

- **Built-in adapters win.** A directory that .NET or Node already claimed keeps its built-in project, so a `pyproject.toml` sitting beside a `package.json` does not turn a Node project into a Python one.
- **The first matching manifest wins.** Manifests are considered in `id` order, so a directory matching two of them becomes one project rather than two sharing a directory.

The same skip rules as the rest of the scan apply: `node_modules`, `bin`, `obj`, `.git` and the other [skipped directories](../architecture/core-crate.md), a depth limit of 10, and nested checkouts left to their own workspace.

## Template substitutions

Available in `args` (and `report_path`):

| Placeholder | Replaced with |
|-------------|---------------|
| `{report}` | Absolute path the app expects the report at (in `.code-basics/results/`) |
| `{project}` | Absolute path of the detected project directory |
| `{root}` | Absolute path of the workspace root |

## Field reference

Top level: `id` (required, non-empty), `name`, `detect` (list of file names). A manifest must define a `[test]` command or at least one `[run.<name>]` command — one that defines neither is rejected.

`[test]` / `[run.<name>]` command template:

| Field | Meaning |
|-------|---------|
| `program` | Executable to run (required) |
| `args` | Arguments, with substitutions applied |
| `env` | Environment defaults; a configuration's own `env` overrides them key-by-key |
| `report_format` | `"junitXml"`, `"trx"`, or `"jestLike"` — omit for run commands |
| `report_extension` | Extension for the generated `{report}` path; defaults to `xml` |
| `report_path` | Fixed report location, for runners that decide it themselves (see below) |
| `filter_template` | Flag introducing a test-name filter (e.g. `-k`) |
| `filter_separator` | Joiner for multiple names; defaults to `" or "` |

## Two subtleties

**Runners that choose their own report path.** cargo-nextest's JUnit output location comes from `.config/nextest.toml`, so `{report}` never reaches it. Set `report_path` to where the runner will actually write (relative paths resolve against the project directory; `{project}`/`{root}` substitutions apply) and the app reads it from there. See [`examples/adapters/cargo-nextest.toml`](../../examples/adapters/cargo-nextest.toml).

**Filters are optional but honest.** "Re-run failed" needs `filter_template`/`filter_separator` to express a test-name filter in the runner's own syntax. Without them the whole suite runs — and the app attaches a warning saying so, because silently running everything looks like the filter was ignored.

## Behaviour details

- Detection matches if **any** listed file exists in a directory (checked during the normal workspace scan, same skip-list and depth).
- The configuration's own `args` are appended after the template's, and its `env` wins over the manifest's.
- The command runs in the project directory unless the configuration sets `cwd`.
- Projects matched by a manifest get `ecosystem = <id>`, and config ids of the form `<project>:<id>:test` / `<project>:<id>:<run-name>`.

If a runner genuinely cannot emit any supported report format, that is the point where a real Rust adapter is needed — see how [`adapters/node.rs`](../architecture/core-crate.md#adaptersnode) is built.

Related: [test report formats](../reference/test-reports.md) · [configuration layout](../reference/configuration.md).
