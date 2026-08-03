# Importing Rider run configurations

The Run view can import JetBrains Rider run configurations (`.run/*.xml` in the workspace) and convert them to code-basics [run configurations](../reference/configuration.md).

## Best effort, by design

JetBrains does not publish a stable schema for these files. The element and option names the importer reads are what Rider writes in practice, but they vary by configuration type, plugin, and version. So the importer **never silently converts**:

1. **Preview** — `preview_rider_import` parses every `.run/*.xml` and converts what it can. Anything that cannot be translated is recorded as a warning *on the specific configuration* it belongs to.
2. **Review** — the import dialog shows each converted configuration with its warnings, plus anything that could not be converted at all.
3. **Apply** — only after confirmation does `apply_rider_import` save the selected configurations to `.code-basics/config.json` (marked with `source: "riderImport"` so the UI can tell them apart).

After import, `.code-basics/config.json` is the source of truth. This is a one-time migration, not a live binding — later edits in Rider are not picked up.

## What gets translated

The importer (`crates/core/src/importers/rider.rs`) parses the XML into name/kind/option maps, expands Rider's path macros (e.g. `$PROJECT_DIR$`) against the workspace root, and maps known configuration kinds (such as `DotNetProject`) onto `RunConfig` fields: project, arguments, environment variables, working directory, launch profile. Unknown kinds and unrecognised options become warnings rather than guesses.

Warnings survive into the saved configuration's `warnings` field, so the context is still visible when editing later.

## Extending it

Handling a new Rider configuration kind means extending `convert()` in `rider.rs`. The test suite (`rider_tests.rs`) uses XML samples shaped the way Rider actually writes them — add a real sample for any new kind.

Related: [core crate](../architecture/core-crate.md#importersrider) · [command reference](../reference/commands.md#workspace--configuration).
