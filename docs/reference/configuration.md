# Configuration reference

## The `.code-basics/` directory

All per-workspace state lives in one directory at the workspace root:

```
.code-basics/
├── config.json       saved run configurations — check this in
├── adapters/*.toml   declarative ecosystem adapters — check these in
└── results/          test report files written by runners — gitignore this
```

A single `.gitignore` entry (`.code-basics/results/`) covers everything transient. The scanner never descends into `.code-basics` itself.

## `config.json`

```json
{
  "version": 1,
  "configs": [ /* RunConfig objects */ ],
  "favorites": [ /* config ids, optional */ ],
  "order": [ /* config ids, optional */ ]
}
```

- `version` exists so a future format change can migrate rather than fail (currently `1`).
- Meant to be **checked in**, sharing run configurations the way Rider's `.run/` directory does.
- Only user-created and imported configurations are written here. Auto-detected ones are re-derived on every scan, which keeps the file small and lets detection keep working as projects change. On open/rescan, saved configs are merged over detected ones.
- `favorites` holds starred config ids; they sort before everything else in the UI. `order` is the user's preferred ordering — ids listed there sort by position, anything unlisted follows in name order. Both keys are omitted while empty.

## .NET user secrets

Secrets are deliberately **not** part of `config.json` (which is checked in). The Run tab's **Secrets…** button edits the standard .NET user-secrets store: the project's `<UserSecretsId>` names a `secrets.json` under the user profile (`%APPDATA%\Microsoft\UserSecrets\<id>\` on Windows, `~/.microsoft/usersecrets/<id>/` elsewhere), which the .NET configuration system reads at runtime. Saving secrets for a project without an id adds one to the `.csproj`, exactly like `dotnet user-secrets init`. Core logic: `crates/core/src/secrets.rs`.

## `RunConfig` fields

Defined in `crates/core/src/model.rs`, mirrored in `src/ipc/types.ts`. Optional fields are omitted from JSON entirely (never `null`), keeping the checked-in file minimal.

| Field | Type | Meaning |
|-------|------|---------|
| `id` | string | Unique id. Detected configs use stable shapes like `<project>:<eco>:test` |
| `name` | string | Display name |
| `kind` | `"app"` \| `"test"` | Launch vs. test run |
| `ecosystem` | string | `"dotnet"`, `"node"`, or a manifest id |
| `source` | `"detected"` \| `"userFile"` \| `"riderImport"` | Where it came from; surfaced in the UI |
| `project` | path? | Target project, **relative to the workspace root** so the file stays portable |
| `buildConfiguration` | string? | .NET `Debug` / `Release` |
| `framework` | string? | .NET target framework when multi-targeted (`net8.0`) |
| `launchProfile` | string? | `launchSettings.json` profile name. When absent, `dotnet run` applies its default profile (the first `Project` one), environment and `applicationUrl` included |
| `ignoreLaunchSettings` | bool? | Skip `launchSettings.json` entirely (`--no-launch-profile`). Warned about when the project has one, since it drops `ASPNETCORE_ENVIRONMENT` and with it user secrets |
| `script` | string? | npm/pnpm script name (or manifest run-command name) |
| `args` | string[] | Arguments passed to the program itself |
| `env` | map | Environment layered on top of the inherited environment |
| `cwd` | path? | Working directory, relative to the root; defaults to the project dir |
| `warnings` | string[] | Free-form notes; the Rider importer records untranslatable bits here |

## Report files

Test runs write their structured report to `.code-basics/results/<sanitised-config-id>.<ext>` (the `{report}` path handed to runners), except when a [declarative adapter sets `report_path`](../guides/adding-an-ecosystem.md#two-subtleties) because the runner chooses its own location. Formats: [test report reference](test-reports.md).

## Related

- [Adding an ecosystem](../guides/adding-an-ecosystem.md) — the `adapters/*.toml` schema
- [Rider import](../guides/rider-import.md) — where `"source": "riderImport"` configs come from
- [Using the app](../getting-started/using-the-app.md)
