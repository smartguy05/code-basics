# Configuration reference

## The `.code-basics/` directory

All per-workspace state lives in one directory at the workspace root:

```
.code-basics/
├── config.json       saved run configurations — check this in
├── adapters/*.toml   declarative ecosystem adapters — check these in
├── changelists.json  your change groups — local, gitignored
├── intents/          what a coding agent said it was doing — local, gitignored
├── inspect/          one directory per object capture (request/result) — local, gitignored
├── dumps/            crash dumps: process memory — local, gitignored, pruned
├── symbols.json      the search palette's index — local, gitignored
├── diagrams/         architecture diagrams — check these in (see below)
├── lsp-logs/         language-server logs — local, gitignored
└── results/          test report files written by runners — gitignore this
```

The app writes `.code-basics/.gitignore` covering everything transient (`results/`, `changelists.json`, `intents/`, `inspect/`, `dumps/`, `symbols.json`, `diagrams/derived/`, `diagrams/.prompts/`, `lsp-logs/`), appending entries it is missing rather than rewriting the file, so a workspace created before an entry existed still picks it up and hand-written rules survive. The scanner never descends into `.code-basics` itself.

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

Enabling capture also writes a `pre-commit` hook — into `core.hooksPath` when the repository sets one, otherwise `.git/hooks/pre-commit` — that refuses a commit whose staged files still carry a rejection note. Same rules: bounded by its own markers, appended to an existing script rather than replacing it, backed up first, and previewed before anything is written. `CB_ALLOW_REJECTED=1` overrides it for one commit.

## Object captures (`inspect/`) and crash dumps (`dumps/`)

Written by the Objects tab and by dump-capturing runs. Both are gitignored and safe to delete at any time. Full walkthrough: [Inspecting objects](../guides/inspecting-objects.md).

```
inspect/<session>/request.json   what was asked for
inspect/<session>/result.json    what the cb-inspector sidecar answered
dumps/<exe>_<pid>_<unix>.dmp     a heap dump the .NET runtime wrote on a crash
```

- **Sessions are pruned to the newest 20.** Dumps are pruned by *both* `keepDumps` and `maxDumpMegabytes`, before every .NET run as well as after every capture, so a workspace that crashes repeatedly without ever being inspected is still bounded.
- **The dump filename is the only attribution there is.** `%e` in the runtime's template expands to the executable name *including its extension* (`Crasher.exe_25764_1786044924.dmp`). The `DOTNET_Dbg*` variables are inherited by the whole process tree, so one `dotnet run` arms its build host too — the name is what tells those apart. Nothing matches a dump to a run automatically.
- Only files matching that exact pattern are ever deleted; anything else in the directory is left alone. Dumps VSTest's blame collector writes into `results/` under its own names are covered by the same byte budget, since they exist only because this app passed `--blame-crash-collect-always`.

## Diagrams (`diagrams/`)

Architecture diagrams: markdown files whose body is Mermaid and whose head is a small front matter block recording where the diagram came from. Derived by `cb-core`'s `architecture` module — design notes: [the core crate](../architecture/core-crate.md).

```
diagrams/
├── *.md          drawn by a person, or inferred and accepted — check these in
├── derived/*.md  recomputed from the manifests on demand — local, gitignored
└── .prompts/     text sent to an agent when inferring a diagram — local, gitignored
```

- **The split is the point.** `diagrams/` is committed like `config.json` and `adapters/`, because a diagram someone drew is a statement of intent the team wants to keep. `derived/` is not, because it is recomputed deterministically from the manifests: committing it would put a regenerated file in everyone's diff after every refactor while saying nothing the manifests do not already say. `.prompts/` is one person's session text.
- **Which directory a diagram lands in is not a parameter.** It follows from the diagram's derivation, so a derived diagram cannot end up committed and a hand-drawn one cannot be filed where the next regeneration would overwrite it.
- **Provenance lives in front matter, not in a `%%` comment.** A comment is part of the text a person edits — it can be deleted by accident, reflowed by a formatter, or copied verbatim onto a different diagram. A diagram must *always* be able to say how it was produced, so the block is a separate region with a parser that either understands it or refuses to.

Front matter keys, all optional except the format marker: `code-basics` (currently `v1`), `level`, `derivation` (`derived`, `inferred` or `user`), `agent` (with `inferred`), `generated`, `sourceCommit`, `edited`.

- **Parsed by hand against a fixed key set, with no YAML.** Pulling in a YAML parser to read six keys would buy anchors, block scalars and type coercion this format does not want, and would answer a malformed file with a guess.
- **A file the parser cannot understand is shown as a `user` diagram with a warning, never rejected** — refusing to show someone their own file is worse than showing it unlabelled. The deliberate cost: a diagram written by a *later* version of this format, carrying a key this version has never heard of, also reads as an unlabelled user diagram, because an unknown key may change the meaning of the ones beside it and this code cannot know that it does not. That is why the key set is kept small.

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
  "msbuildEvaluation": false,
  "askForIntent": true,
  "inspector": { /* optional, see below */ },
  "lsp": { /* optional, see below */ }
}
```

- `version` exists so a future format change can migrate rather than fail (currently `1`).
- Meant to be **checked in**, sharing run configurations the way Rider's `.run/` directory does.
- Only user-created and imported configurations are written here. Auto-detected ones are re-derived on every scan, which keeps the file small and lets detection keep working as projects change. On open/rescan, saved configs are merged over detected ones.
- `favorites` holds starred config ids; they sort before everything else in the UI. `order` is the user's preferred ordering — ids listed there sort by position, anything unlisted follows in name order. Both keys are omitted while empty.
- `askForIntent` controls whether a Claude Code turn that edited files and ended without an `Intent:` line is asked for one before it stops. **On by default and omitted from the file while true**; set it to `false` to turn it off. It is the only thing the app does that deliberately interrupts an agent mid-session, so it is worth being able to switch off without uninstalling capture — and it only ever fires once per turn, so a session can always end. See [agent intent capture](../guides/agent-intent-capture.md).
- `msbuildEvaluation` opts this workspace into evaluating .NET projects with `dotnet msbuild -getProperty` during a scan instead of only reading them as XML. Off by default and omitted while false. Turn it on when projects set properties behind MSBuild `Condition`s or in imported `.props` files, which the XML scan cannot see; the cost is one `dotnet` process per project at scan time, and a machine with no SDK simply falls back to the XML scan.

### `inspector`

Per-workspace settings for the object inspector. The whole section is optional and is omitted from the file unless the user configures something — saving a run configuration never introduces it.

```json
"inspector": {
  "captureDumps": false,
  "caps": { "maxDepth": 5, "maxChildren": 100, "maxStringLength": 512, "maxNodes": 5000 },
  "keepDumps": 3,
  "maxDumpMegabytes": 2048,
  "env": { }
}
```

- `captureDumps` opts this workspace into writing a crash dump when a run crashes. **Off unless explicitly enabled**, and treated as off whenever the section is absent. A dump is a verbatim copy of process memory — connection strings, tokens, whatever the application had in flight — so nothing infers this setting from anything else. Dumps land in a gitignored directory and never reach the shared history. **This file is checked in**, so enabling it here enables it for everyone who works in the repository; every armed run therefore carries a warning saying so in the Run tab.
- `caps` bounds how much of an object graph a capture walks. Each key falls back to its built-in default (shown above) on its own, so writing only `{ "maxDepth": 2 }` is valid — a partly written section must never be the reason a workspace will not open.
- `keepDumps` defaults to `3` — enough to compare a repeated crash against its two predecessors.
- `maxDumpMegabytes` defaults to `2048`. This is the limit that actually binds: a dump of a trivial console app measured 9.3 MB and a real application's runs to hundreds of megabytes, so bytes run out long before the file count does. It is applied before every .NET run as well as after every capture, so a workspace that crashes repeatedly and is never inspected is still bounded, and it covers both the dumps in `.code-basics/dumps/` and the ones VSTest's blame collector leaves in `.code-basics/results/`.
- `env` adds environment variables applied only to dump-capturing runs, for the rare project that needs a different dump type. They layer over the built-in `DOTNET_Dbg*` defaults and under the run configuration's own environment.

### `lsp`

Per-workspace language-server settings, used by "find usages" and "go to definition". The whole section is optional and is omitted from the file unless the user configures something — saving a run configuration never introduces it, and a workspace with no section at all uses discovery for every server.

```json
"lsp": {
  "servers": {
    "csharp": {
      "program": "C:/Users/me/.vscode/extensions/ms-dotnettools.csharp-2.140.9-win32-x64/.roslyn/Microsoft.CodeAnalysis.LanguageServer.exe",
      "args": ["--stdio", "--autoLoadProjects"],
      "env": { "DOTNET_NOLOGO": "1" },
      "uriStyle": "plain"
    },
    "python": { "enabled": false }
  }
}
```

Keys are server ids: `csharp`, `typescript`, `rust`, `python`. Every field inside one is optional, and every absence means "use the built-in default".

- **Nothing is bundled; a server is located.** The four servers are hundreds of megabytes between them and the licences differ per option, so this app launches the copy already on your machine and redistributes nothing. The cost is that a server can simply be absent, which is reported as such rather than worked around.
- `program` names an explicit executable, absolute or on `PATH`. **If it does not resolve, that server fails with an error naming this file** — it never falls back to discovery. You asked for one specific build, usually to match a project's SDK, and quietly starting a different one would attribute its usage counts and definition jumps to a server that never ran. Discovery is what an *absent* `program` means. A bare name is resolved through `PATHEXT` on Windows, so npm shims (`typescript-language-server.cmd`) work by name.
- `args` **replaces** the built-in argument list rather than appending to it, so an unwanted default can be removed. Note the difference between omitting the key (keep the built-in arguments) and `"args": []` (launch with none at all) — the Roslyn server without `--stdio` never says a word, which looks like a hang rather than a mistake.
- `enabled` switches one server off. Only an explicit `false` counts: an absent flag means enabled, so a block written to set a `program` does not disable the server it just configured.
- `env` layers over the inherited environment for that server's process only.
- `uriStyle` (`"encoded"` | `"plain"`) overrides how the drive colon is spelled in the `file:` URIs sent to that server — `file:///C%3A/x` versus `file:///C:/x`. An escape hatch, not a routine knob: the per-server default is what each real server was observed to accept, and file identity is decided on paths rather than on URI strings, so this only matters for a server that rejects a spelling outright.
- Unknown keys are ignored rather than rejected, since this file is shared: a block written by a newer build must still load for a teammate on an older one.
- Servers write their own logs into `.code-basics/lsp-logs/`, which is gitignored — a verbose trace of one person's editing session, rewritten on every launch and safe to delete at any time.

### Detected .NET configurations

What a scan generates per project, before anything saved is layered on:

| Project | Generated |
|---------|-----------|
| Executable | One configuration per launchable `launchSettings.json` profile, plus one per build configuration (`Debug`, `Release`, and anything in `<Configurations>`) |
| Test | One Debug configuration. Release is deliberately not offered — `#if !DEBUG` paths make it a trap — but the editor's build-configuration dropdown lists every configuration the project declares |
| Library | None; there is nothing to launch or test |

A multi-targeted project (`<TargetFrameworks>`) multiplies the above by framework, since `dotnet run` and `dotnet test` refuse to guess between them. A single-targeted project omits `-f` entirely.

## User-global stores (outside any repository)

Three things the app persists are a property of **you**, not of a workspace, so they live under the user
config directory instead of `.code-basics/` — which also means there is no gitignore entry to keep in step
and nothing of yours is shared with the team by accident. The base is `%APPDATA%` on Windows, then
`$XDG_CONFIG_HOME`, then `~/.config`:

| Path | What | Override |
|------|------|----------|
| `code-basics/notes.json` | The [Notes](../getting-started/using-the-app.md#notes) scratchpad: a schema `version` plus ordered notes. Written atomically (temp + rename, `.bak` before an empty overwrite) so notes survive a crash | `CB_NOTES_PATH` (whole path) |
| `code-basics/launchers.json` | The [app launcher's](../getting-started/using-the-app.md#running-other-apps) remembered commands — each with the `cwd` it ran in, plus your pin and rename. Unpinned entries cap at 30, oldest first; pinned ones never age out. Same atomic write | `CB_LAUNCHERS_PATH` (whole path) |
| `code-basics/instructions/`, `code-basics/prompts/` | The [Enhancements](../guides/instruction-enhancements.md) library of `.md` templates, seeded from the bundled defaults without overwriting your edits | `CB_INSTRUCTIONS_PATH`, `CB_PROMPTS_PATH` |
| `code-basics/running.json` | Which processes the app had running, so one that outlived a crash can be found and killed on the next launch (the Running panel's "possible orphans") | `CB_RUNNING_PATH` |

A missing or corrupt file in any of these reads as **empty**, never as an error: none of them is important
enough to stop a panel opening.

## .NET user secrets

Secrets are deliberately **not** part of `config.json` (which is checked in). The Run tab's **Secrets…** button opens the standard .NET user-secrets store as an ordinary editor tab: the project's `<UserSecretsId>` names a `secrets.json` under the user profile (`%APPDATA%\Microsoft\UserSecrets\<id>\` on Windows, `~/.microsoft/usersecrets/<id>/` elsewhere), which the .NET configuration system reads at runtime. Saving secrets for a project without an id adds one to the `.csproj`, exactly like `dotnet user-secrets init`. Core logic: `crates/core/src/secrets.rs`.

Saving validates against the same JSON dialect .NET's configuration loader accepts, rather than strict JSON — `//` and `/* */` comments, trailing commas, and a leading UTF-8 byte-order mark all pass, because `dotnet user-secrets` and Rider write files containing them and rejecting your own tooling's output would be worse than a late failure. A file that really is malformed is refused with the line number **and the offending line quoted**, since the invisible causes (that byte-order mark, most of all) are otherwise indistinguishable from whatever else is unusual in the file.

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
