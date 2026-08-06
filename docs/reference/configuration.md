# Configuration reference

## The `.code-basics/` directory

All per-workspace state lives in one directory at the workspace root:

```
.code-basics/
├── config.json       saved run configurations — check this in
├── adapters/*.toml   declarative ecosystem adapters — check these in
├── changelists.json  your change groups — local, gitignored
├── intents/          what a coding agent said it was doing — local, gitignored
└── results/          test report files written by runners — gitignore this
```

The app writes `.code-basics/.gitignore` covering everything transient (`results/`, `changelists.json`, `intents/`), appending entries it is missing rather than rewriting the file, so a workspace created before an entry existed still picks it up and hand-written rules survive. The scanner never descends into `.code-basics` itself.

## Agent intent (`intents/`)

Two JSON-lines files recording what a coding agent did and why, so the Changes tab can group hunks into the decisions behind them. Written by hooks the agent runs, or imported from its session history. Full walkthrough: [Agent intent capture](../guides/agent-intent-capture.md).

```
intents/
├── edits.jsonl    {provider, turnId, toolUseId, seq, path, edit, branch}
└── labels.jsonl   {provider, turnId, label, paths}
```

- **Gitignored on purpose.** This is a log of one person's session, and it is large.
- **The two files join on `turnId`** — `prompt_id` in Claude Code, `turn_id` in Codex. The edit hook knows what changed but has no way to carry a reason; the end-of-turn hook has the reason but not the geometry. Neither agent offers a payload that has both.
- **Records are content, not positions.** Line numbers are recorded nowhere, because the file moves between the edit and the review. Matching is entirely by text.
- Safe to delete at any time; nothing else reads it.

Hook configuration is written **outside** this directory, into the agent's own files (`.claude/settings.json`, `.codex/hooks.json`, or their user-level equivalents). Installation is additive and backed up — see the guide.

## Change groups (`changelists.json`)

Named buckets for working-tree files, in the spirit of JetBrains' changelists — a way to keep a half-finished refactor visibly apart from the fix being committed next. Git has no equivalent, so this is purely local bookkeeping and nothing here changes the repository.

```json
{ "version": 1, "groups": [ { "name": "Refactor", "paths": ["src/a.rs"] } ] }
```

- **Gitignored on purpose.** Groups describe one person's work in progress; committing them would impose that structure on everyone.
- **A file belongs to at most one group.** Assigning it somewhere removes it from wherever it was.
- **Groups hold unstaged work only.** Once something is staged, the index is the grouping that matters, so the Changes tab lists it under Staged. A partially staged file appears in *both* Staged and its group — the same way `git status` lists it under both headings.
- **Assignments outlive a file becoming clean**, so a file that is committed and later edited again returns to its group rather than being silently forgotten.
- Deleting a group leaves its files ungrouped rather than discarding anything.

## `config.json`

```json
{
  "version": 1,
  "configs": [ /* RunConfig objects */ ],
  "favorites": [ /* config ids, optional */ ],
  "order": [ /* config ids, optional */ ],
  "msbuildEvaluation": false
}
```

- `version` exists so a future format change can migrate rather than fail (currently `1`).
- Meant to be **checked in**, sharing run configurations the way Rider's `.run/` directory does.
- Only user-created and imported configurations are written here. Auto-detected ones are re-derived on every scan, which keeps the file small and lets detection keep working as projects change. On open/rescan, saved configs are merged over detected ones.
- `favorites` holds starred config ids; they sort before everything else in the UI. `order` is the user's preferred ordering — ids listed there sort by position, anything unlisted follows in name order. Both keys are omitted while empty.
- `msbuildEvaluation` opts this workspace into evaluating .NET projects with `dotnet msbuild -getProperty` during a scan instead of only reading them as XML. Off by default and omitted while false. Turn it on when projects set properties behind MSBuild `Condition`s or in imported `.props` files, which the XML scan cannot see; the cost is one `dotnet` process per project at scan time, and a machine with no SDK simply falls back to the XML scan.

### Detected .NET configurations

What a scan generates per project, before anything saved is layered on:

| Project | Generated |
|---------|-----------|
| Executable | One configuration per launchable `launchSettings.json` profile, plus one per build configuration (`Debug`, `Release`, and anything in `<Configurations>`) |
| Test | One Debug configuration. Release is deliberately not offered — `#if !DEBUG` paths make it a trap — but the editor's build-configuration dropdown lists every configuration the project declares |
| Library | None; there is nothing to launch or test |

A multi-targeted project (`<TargetFrameworks>`) multiplies the above by framework, since `dotnet run` and `dotnet test` refuse to guess between them. A single-targeted project omits `-f` entirely.

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
