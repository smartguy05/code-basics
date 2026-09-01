//! Tests for [`super::discover`].
//!
//! Every test builds a real temporary workspace and runs the real
//! [`crate::workspace::scan`] over it, for the reason
//! `crate::architecture::signals::dotnet_tests` gives: this module's whole job
//! is to line values in files up with the projects the scan found, and a test
//! that fabricated the scan's output would be testing the fabrication.
//!
//! No test here mutates process environment. `APPDATA` decides where
//! [`crate::secrets`] looks, and it is global to the process, so user secrets
//! are driven through [`DiscoveryOptions::read_user_secrets`] instead — the
//! seam that exists for exactly this.

use std::path::{Path, PathBuf};

use super::*;
use crate::sql::dotenv::EnvValue;
use crate::sql::dsn::{SqlAuthMode, SqlEngine};
use crate::sql::store::SecretSource;
use crate::workspace::{scan, Workspace};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn workspace_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, contents).unwrap();
    }
    dir
}

fn scanned(files: &[(&str, &str)]) -> (tempfile::TempDir, Workspace) {
    let dir = workspace_with(files);
    let ws = scan(dir.path()).unwrap();
    (dir, ws)
}

fn csproj(packages: &[&str]) -> String {
    let items: String = packages
        .iter()
        .map(|p| format!("    <PackageReference Include=\"{p}\" Version=\"1.0.0\" />\n"))
        .collect();
    format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup>\n  <ItemGroup>\n{items}  </ItemGroup>\n</Project>"
    )
}

/// A user-secrets reader that finds nothing, for the many tests that are not
/// about user secrets. Returning `Ok(None)` is *"this project has none"*, which
/// is a different answer from an error and must stay one.
fn no_secrets(_project: &Path) -> Result<Option<(PathBuf, String)>, String> {
    Ok(None)
}

fn options() -> DiscoveryOptions<'static> {
    DiscoveryOptions {
        read_user_secrets: no_secrets,
        ..DiscoveryOptions::default()
    }
}

fn found(files: &[(&str, &str)]) -> (tempfile::TempDir, Discovery) {
    let (dir, ws) = scanned(files);
    let out = discover(&ws, &options());
    (dir, out)
}

fn names(out: &Discovery) -> Vec<&str> {
    out.candidates.iter().map(|c| c.name.as_str()).collect()
}

fn one(out: &Discovery) -> &SqlCandidate {
    assert_eq!(
        out.candidates.len(),
        1,
        "expected exactly one candidate: {:#?}",
        out.candidates
    );
    &out.candidates[0]
}

/// A password nothing outside this module may ever repeat, plus a host and a
/// database to go with it. Distinctive enough that a substring search for any
/// of them is meaningful.
const PASSWORD: &str = "hunter2-do-not-repeat";
const HOST: &str = "db.internal";
const DATABASE: &str = "orders_prod";

fn postgres_string() -> String {
    format!("Host={HOST};Port=5432;Database={DATABASE};Username=svc_orders;Password={PASSWORD}")
}

fn appsettings_with(name: &str, value: &str) -> String {
    format!(
        "{{\n  \"ConnectionStrings\": {{\n    \"{name}\": \"{value}\"\n  }},\n  \"Logging\": {{ \"LogLevel\": {{ \"Default\": \"Information\" }} }}\n}}"
    )
}

// ---------------------------------------------------------------------------
// The justification for this module existing at all
// ---------------------------------------------------------------------------

/// One `appsettings.json`, fed through both read paths.
///
/// `architecture::signals::dotnet` iterates the `ConnectionStrings` object's
/// **keys** and never touches a value; `sql::discover` exists to return the
/// value, for a console the user explicitly pointed at a database. If these two
/// ever agreed, one of them would be wrong: either the graph had started
/// leaking secrets into a diagram that gets exported and shared, or the SQL
/// console had lost the only thing it needs.
///
/// The graph half of this fixture is deliberately the same shape as
/// `architecture::signals::framework_tests::a_connection_string_value_never_reaches_the_graph`,
/// which is untouched by this work and must keep passing.
#[test]
fn discovery_returns_values_that_the_graph_refuses() {
    let secret = postgres_string();
    let (_dir, ws) = scanned(&[
        ("src/Api/Api.csproj", &csproj(&["Npgsql"])),
        (
            "src/Api/appsettings.json",
            &appsettings_with("Orders", &secret),
        ),
    ]);

    // --- The graph path: keys only. ---
    let graph = crate::architecture::signals::dotnet::signals(&ws);
    let rendered = format!("{graph:#?}");
    assert!(
        rendered.contains("Orders"),
        "the key is the author's own label and is fair game: {rendered}"
    );
    for leaked in [PASSWORD, HOST, DATABASE, "Username", "Port=5432"] {
        assert!(
            !rendered.contains(leaked),
            "the graph must not carry `{leaked}` anywhere, including evidence: {rendered}"
        );
    }

    // --- The discovery path: the value, on request. ---
    let out = discover(&ws, &options());
    let candidate = one(&out);
    assert_eq!(candidate.name, "Orders");
    assert_eq!(candidate.engine, Some(SqlEngine::Postgres));
    assert!(candidate.state.is_connectable(), "{:?}", candidate.state);

    let value = read_value(&candidate.source, &options()).unwrap();
    assert_eq!(
        value,
        EnvValue::Literal {
            text: secret.clone()
        },
        "discovery is the read path that returns the value the graph refuses"
    );

    // And the candidate itself carries only the redacted view, so listing them
    // is not the same act as connecting with one.
    let listed = format!("{candidate:#?}");
    assert!(
        !listed.contains(PASSWORD),
        "a listed candidate must not carry the password: {listed}"
    );
    assert_eq!(candidate.display.auth_mode, SqlAuthMode::Password);
}

// ---------------------------------------------------------------------------
// Warnings
// ---------------------------------------------------------------------------

/// A warning may name a file, a project, a package or a key, and may never
/// contain text read out of a *value*. This is `DotnetSignals::warnings`' rule
/// verbatim; discovery reads values, so it is the module that could break it.
#[test]
fn a_discovery_warning_never_repeats_a_value_it_read() {
    let secret = postgres_string();
    let (_dir, out) = found(&[
        // Two clients, so no engine is agreed and nothing is attached.
        (
            "src/Api/Api.csproj",
            &csproj(&["Npgsql", "Microsoft.Data.SqlClient"]),
        ),
        (
            "src/Api/appsettings.json",
            &format!(
                "{{\n  \"ConnectionStrings\": {{\n    \"Orders\": \"{secret}\",\n    \"Broken\": {{ \"nested\": \"{secret}\" }},\n    \"Numeric\": 5\n  }}\n}}"
            ),
        ),
        // A file that will not parse at all.
        (
            "src/Api/appsettings.Broken.json",
            "{ \"ConnectionStrings\": ",
        ),
        // A .env whose lines cannot be read.
        (
            "web/package.json",
            "{\"name\":\"web\",\"dependencies\":{\"pg\":\"^8\"}}",
        ),
        ("web/.env", &format!("DATABASE_URL=\"{secret}\nJUST_A_LINE\n")),
    ]);

    assert!(
        !out.warnings.is_empty(),
        "the fixture must produce warnings"
    );
    let joined = out.warnings.join("\n");
    for leaked in [PASSWORD, HOST, DATABASE, "svc_orders"] {
        assert!(
            !joined.contains(leaked),
            "a warning repeated `{leaked}`:\n{joined}"
        );
    }
    assert!(
        joined.contains("Broken") && joined.contains("Numeric"),
        "a warning must name the key it skipped:\n{joined}"
    );
}

#[test]
fn a_connection_string_value_that_is_not_a_string_is_skipped_with_a_warning_naming_the_key() {
    let (_dir, out) = found(&[
        ("src/Api/Api.csproj", &csproj(&["Npgsql"])),
        (
            "src/Api/appsettings.json",
            "{ \"ConnectionStrings\": { \"Orders\": \"Host=h;Database=d\", \"Legacy\": null, \"Odd\": [1,2] } }",
        ),
    ]);

    assert_eq!(names(&out), ["Orders"], "only the string value is listed");
    let joined = out.warnings.join("\n");
    assert!(joined.contains("Legacy"), "{joined}");
    assert!(joined.contains("Odd"), "{joined}");
}

#[test]
fn an_unparseable_file_becomes_a_warning_naming_the_file() {
    let (_dir, out) = found(&[
        ("src/Api/Api.csproj", &csproj(&["Npgsql"])),
        ("src/Api/appsettings.json", "{ this is not json"),
    ]);

    assert!(out.candidates.is_empty());
    let joined = out.warnings.join("\n");
    assert!(
        joined.contains("appsettings.json"),
        "a shorter list must never be the only symptom:\n{joined}"
    );
}

// ---------------------------------------------------------------------------
// What is read
// ---------------------------------------------------------------------------

#[test]
fn the_section_and_the_flat_key_spellings_are_both_read() {
    let (_dir, out) = found(&[
        ("src/Api/Api.csproj", &csproj(&["Npgsql"])),
        (
            "src/Api/appsettings.json",
            "{ \"ConnectionStrings\": { \"Orders\": \"Host=h;Database=d\" }, \"ConnectionStrings:Legacy\": \"Host=h;Database=old\", \"ConnectionStrings__Env\": \"Host=h;Database=env\" }",
        ),
    ]);

    let mut got = names(&out);
    got.sort_unstable();
    assert_eq!(got, ["Env", "Legacy", "Orders"], "{:#?}", out.candidates);
}

#[test]
fn the_json_dialect_dotnet_accepts_is_read() {
    // A byte-order mark, a comment and a trailing comma: `dotnet user-secrets`
    // and Rider both write files like this and .NET reads them.
    let (_dir, out) = found(&[
        ("src/Api/Api.csproj", &csproj(&["Npgsql"])),
        (
            "src/Api/appsettings.json",
            "\u{feff}{\n  // local\n  \"ConnectionStrings\": {\n    \"Orders\": \"Host=h;Database=d\",\n  },\n}",
        ),
    ]);

    assert_eq!(names(&out), ["Orders"], "{out:#?}");
}

#[test]
fn user_secrets_yield_their_own_candidate() {
    fn reader(project: &Path) -> Result<Option<(PathBuf, String)>, String> {
        assert!(project.ends_with("Api.csproj"), "{}", project.display());
        Ok(Some((
            PathBuf::from("C:/secrets/id/secrets.json"),
            "{ \"ConnectionStrings:Orders\": \"Host=secret-host;Database=d\" }".to_string(),
        )))
    }

    let (_dir, ws) = scanned(&[("src/Api/Api.csproj", &csproj(&["Npgsql"]))]);
    let out = discover(
        &ws,
        &DiscoveryOptions {
            read_user_secrets: reader,
            ..DiscoveryOptions::default()
        },
    );

    let candidate = one(&out);
    assert_eq!(candidate.name, "Orders");
    assert!(
        matches!(&candidate.source, SecretSource::UserSecrets { key, .. } if key == "ConnectionStrings:Orders"),
        "{:?}",
        candidate.source
    );
}

#[test]
fn nested_user_secrets_preserve_the_full_configuration_key() {
    fn reader(_project: &Path) -> Result<Option<(PathBuf, String)>, String> {
        Ok(Some((
            PathBuf::from("C:/secrets/id/secrets.json"),
            r#"{
              "AppConfiguration": {
                "ConnectionStrings": {
                  "DatabaseConnection": "Host=secret-host;Database=app",
                  "CosmosDbConnection": "AccountEndpoint=https://example.invalid"
                }
              }
            }"#
            .to_string(),
        )))
    }

    let (_dir, ws) = scanned(&[("src/Api/Api.csproj", &csproj(&["Npgsql"]))]);
    let options = DiscoveryOptions {
        read_user_secrets: reader,
        ..DiscoveryOptions::default()
    };
    let out = discover(&ws, &options);

    let database = out
        .candidates
        .iter()
        .find(|candidate| candidate.name == "DatabaseConnection")
        .expect("the nested PostgreSQL connection should be discovered");
    assert_eq!(database.engine, Some(SqlEngine::Postgres));
    assert!(matches!(
        &database.source,
        SecretSource::UserSecrets { key, .. }
            if key == "AppConfiguration:ConnectionStrings:DatabaseConnection"
    ));
    assert!(matches!(
        read_value(&database.source, &options),
        Ok(EnvValue::Literal { text }) if text == "Host=secret-host;Database=app"
    ));

    let cosmos = out
        .candidates
        .iter()
        .find(|candidate| candidate.name == "CosmosDbConnection")
        .expect("non-SQL entries remain visible but blocked");
    assert!(cosmos.engine.is_none());
    assert!(matches!(cosmos.state, CandidateState::EngineUnknown { .. }));
}

#[test]
fn nested_flat_spellings_are_canonicalised_and_deduplicated() {
    let read = read_dotnet_config(
        r#"{
          "AppConfiguration": {
            "ConnectionStrings": { "Orders": "Host=h;Database=d" }
          },
          "AppConfiguration__ConnectionStrings__Orders": "Host=other;Database=d"
        }"#,
    )
    .unwrap();

    assert_eq!(read.entries.len(), 1);
    assert_eq!(
        read.entries[0].key,
        "AppConfiguration:ConnectionStrings:Orders"
    );
    assert_eq!(read.skipped.len(), 1);
    assert!(read.skipped[0].reason.contains("is spelled more than once"));
}

#[test]
fn a_user_secrets_read_failure_is_a_warning_naming_the_project() {
    fn reader(_project: &Path) -> Result<Option<(PathBuf, String)>, String> {
        Err("the id is not usable".to_string())
    }

    let (_dir, ws) = scanned(&[("src/Api/Api.csproj", &csproj(&["Npgsql"]))]);
    let out = discover(
        &ws,
        &DiscoveryOptions {
            read_user_secrets: reader,
            ..DiscoveryOptions::default()
        },
    );

    assert!(out.candidates.is_empty());
    assert!(
        out.warnings.iter().any(|w| w.contains("Api")),
        "{:?}",
        out.warnings
    );
}

#[test]
fn env_files_beside_a_package_json_are_read() {
    let (_dir, out) = found(&[
        (
            "web/package.json",
            "{\"name\":\"web\",\"dependencies\":{\"pg\":\"^8.11.0\"}}",
        ),
        (
            "web/.env",
            "DATABASE_URL=postgres://svc:pw@localhost:5432/app\n",
        ),
        (
            "web/.env.local",
            "ConnectionStrings__Orders=postgres://svc:pw@localhost:5432/local\n",
        ),
        ("web/.env.development", "NOT_A_DATABASE=hello\n"),
    ]);

    let mut got = names(&out);
    got.sort_unstable();
    assert_eq!(
        got,
        ["DATABASE_URL", "Orders"],
        "an ordinary variable is not a database: {:#?}",
        out.candidates
    );
    assert!(out
        .candidates
        .iter()
        .all(|c| c.engine == Some(SqlEngine::Postgres)));
}

// ---------------------------------------------------------------------------
// Abstain rules
// ---------------------------------------------------------------------------

/// `appsettings.json`, `appsettings.Development.json` and user secrets can each
/// define `Orders`, and which one wins depends on `ASPNETCORE_ENVIRONMENT`,
/// which this module cannot see. Merging them would pick one and be silently
/// wrong, so each is its own labelled candidate.
#[test]
fn the_same_logical_name_in_two_files_does_not_collapse() {
    fn reader(_project: &Path) -> Result<Option<(PathBuf, String)>, String> {
        Ok(Some((
            PathBuf::from("C:/secrets/id/secrets.json"),
            "{ \"ConnectionStrings:Orders\": \"Host=secrets;Database=d\" }".to_string(),
        )))
    }

    let (_dir, ws) = scanned(&[
        ("src/Api/Api.csproj", &csproj(&["Npgsql"])),
        (
            "src/Api/appsettings.json",
            &appsettings_with("Orders", "Host=base;Database=d"),
        ),
        (
            "src/Api/appsettings.Development.json",
            &appsettings_with("Orders", "Host=dev;Database=d"),
        ),
    ]);
    let out = discover(
        &ws,
        &DiscoveryOptions {
            read_user_secrets: reader,
            ..DiscoveryOptions::default()
        },
    );

    assert_eq!(
        names(&out),
        ["Orders", "Orders", "Orders"],
        "three files, three candidates: {:#?}",
        out.candidates
    );

    let mut origins: Vec<&str> = out.candidates.iter().map(|c| c.origin.as_str()).collect();
    origins.sort_unstable();
    origins.dedup();
    assert_eq!(
        origins.len(),
        3,
        "each must be labelled by where it came from: {origins:?}"
    );

    let mut ids: Vec<&str> = out.candidates.iter().map(|c| c.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 3, "ids must be distinct or a picker merges them");
}

#[test]
fn an_unresolved_placeholder_is_listed_but_not_connectable() {
    let (_dir, out) = found(&[
        ("src/Api/Api.csproj", &csproj(&["Npgsql"])),
        (
            "src/Api/appsettings.json",
            &appsettings_with("Orders", "Host=${DB_HOST};Database=orders"),
        ),
    ]);

    let candidate = one(&out);
    assert_eq!(candidate.name, "Orders", "it is still listed");
    match &candidate.state {
        CandidateState::Unresolved { reason } => {
            // The reason crosses IPC, so it names the *syntax class* and never
            // the match: the match is text read out of a value, and a password
            // containing a `$` is enough to make it a secret fragment.
            assert!(reason.contains("${NAME}"), "{reason}");
            assert!(
                !reason.contains("DB_HOST"),
                "a reason must not quote text from the value: {reason}"
            );
        }
        other => panic!("expected Unresolved, got {other:?}"),
    }
    assert!(!candidate.state.is_connectable());
    // Which reference is missing is still shown — through `display`, the
    // redacting display path, which is where value text is allowed to appear.
    assert!(
        format!("{:?}", candidate.display).contains("${DB_HOST}"),
        "{:?}",
        candidate.display
    );
}

// ---------------------------------------------------------------------------
// Engine selection: the DSN, unless package evidence contradicts it
// ---------------------------------------------------------------------------

#[test]
fn two_agreeing_signals_name_the_engine() {
    assert_eq!(
        resolve_engine(Some(SqlEngine::Postgres), Some(SqlEngine::Postgres)),
        EngineChoice::Determined(SqlEngine::Postgres)
    );
}

#[test]
fn an_empty_connection_placeholder_is_listed_but_cannot_be_adopted() {
    let (_dir, out) = found(&[
        ("src/Api/Api.csproj", &csproj(&["Npgsql"])),
        (
            "src/Api/appsettings.json",
            &appsettings_with("DatabaseConnection", ""),
        ),
    ]);

    let candidate = one(&out);
    assert_eq!(candidate.engine, None);
    assert!(matches!(
        &candidate.state,
        CandidateState::Unresolved { reason } if reason.contains("empty")
    ));
}

#[test]
fn disagreement_and_missing_dsn_are_different_answers() {
    // A contradiction is different from a package reference with no usable
    // DSN, while a usable DSN needs no package reference to repeat its answer.
    assert_eq!(
        resolve_engine(Some(SqlEngine::Postgres), Some(SqlEngine::SqlServer)),
        EngineChoice::Disagreed {
            packages: SqlEngine::Postgres,
            connection_string: SqlEngine::SqlServer,
        }
    );
    assert_eq!(
        resolve_engine(Some(SqlEngine::Postgres), None),
        EngineChoice::NotDetermined
    );
    assert_eq!(
        resolve_engine(None, Some(SqlEngine::Postgres)),
        EngineChoice::Determined(SqlEngine::Postgres)
    );
    assert_eq!(resolve_engine(None, None), EngineChoice::NotDetermined);
    assert_eq!(resolve_engine(None, None).engine(), None);
    assert_eq!(
        resolve_engine(Some(SqlEngine::Postgres), Some(SqlEngine::SqlServer)).engine(),
        None,
        "a disagreement never yields an engine"
    );
}

#[test]
fn an_unambiguous_dsn_survives_ambiguous_package_references() {
    let (_dir, out) = found(&[
        (
            "src/Api/Api.csproj",
            &csproj(&["Npgsql", "Microsoft.Data.SqlClient"]),
        ),
        (
            "src/Api/appsettings.json",
            &appsettings_with("Orders", "Host=h;Port=5432;Database=d"),
        ),
    ]);

    let candidate = one(&out);
    assert_eq!(candidate.engine, Some(SqlEngine::Postgres));
    assert!(candidate.state.is_connectable());
}

#[test]
fn an_unambiguous_string_is_enough_to_name_an_engine() {
    let (_dir, out) = found(&[
        ("src/Api/Api.csproj", &csproj(&[])),
        (
            "src/Api/appsettings.json",
            &appsettings_with("Orders", "Host=h;Port=5432;Database=d"),
        ),
    ]);

    let candidate = one(&out);
    assert_eq!(candidate.engine, Some(SqlEngine::Postgres));
    assert!(candidate.state.is_connectable());
}

#[test]
fn structured_entries_in_a_custom_nested_section_are_ignored() {
    let read = read_dotnet_config(
        r#"{
          "AppConfiguration": {
            "ConnectionStrings": {
              "DatabaseConnection": "Host=h;Database=d",
              "TransientConnection": { "RetryCount": 3 }
            }
          }
        }"#,
    )
    .unwrap();

    assert_eq!(read.entries.len(), 1);
    assert_eq!(read.entries[0].name, "DatabaseConnection");
    assert!(read.skipped.is_empty());
}

#[test]
fn jsonc_user_secret_layout_keeps_active_strings_and_ignores_objects() {
    let read = read_dotnet_config(
        r#"{
          "AppConfiguration": {
            "ConnectionStrings": {
              // Active developer database
              "DatabaseConnection": "Server=pg.internal;Database=app;Port=5432;User Id=svc;Password=not-real;Ssl Mode=Require;Include Error Detail=True;",
              // "DatabaseConnection": "Server=commented-out;Database=other;",
              "TransientConnection": {
                "Database": "cache",
                "DefaultTimeToLive": 3600,
              },
            },
          },
        }"#,
    )
    .expect("comments and trailing commas are accepted");

    assert_eq!(read.entries.len(), 1);
    assert_eq!(read.entries[0].name, "DatabaseConnection");
    assert!(read.entries[0].value.contains("Server=pg.internal"));
    assert!(read.skipped.is_empty());
}

#[test]
fn package_matching_stops_at_a_name_boundary() {
    let table = DEFAULT_PACKAGE_ENGINES;
    assert_eq!(
        engine_from_packages(&["Npgsql".to_string()], table),
        Some(SqlEngine::Postgres)
    );
    assert_eq!(
        engine_from_packages(
            &["Npgsql.EntityFrameworkCore.PostgreSQL".to_string()],
            table
        ),
        Some(SqlEngine::Postgres)
    );
    assert_eq!(
        engine_from_packages(&["NpgsqlRest".to_string()], table),
        None,
        "a different package by a different author"
    );
    assert_eq!(
        engine_from_packages(
            &["Npgsql".to_string(), "Microsoft.Data.SqlClient".to_string()],
            table
        ),
        None,
        "two engines is not one engine"
    );
    assert_eq!(
        engine_from_packages(
            &[
                "Npgsql".to_string(),
                "Npgsql.EntityFrameworkCore.PostgreSQL".to_string()
            ],
            table
        ),
        Some(SqlEngine::Postgres),
        "two rows naming the same engine still agree"
    );
}

// ---------------------------------------------------------------------------
// Discovery does nothing on its own
// ---------------------------------------------------------------------------

/// Discovery lists; it never saves and never connects. A discovered candidate
/// is a *reference* — nothing it produces holds a secret — so listing
/// candidates cannot leak one, and nothing is written anywhere.
#[test]
fn discovery_neither_saves_nor_holds_a_secret() {
    let files: &[(&str, &str)] = &[
        ("src/Api/Api.csproj", &csproj(&["Npgsql"])),
        (
            "src/Api/appsettings.json",
            &appsettings_with("Orders", &postgres_string()),
        ),
    ];
    let (dir, ws) = scanned(files);
    let before = tree(dir.path());

    let out = discover(&ws, &options());

    assert!(!out.candidates.is_empty());
    assert!(
        out.candidates.iter().all(|c| !c.source.holds_a_secret()),
        "a discovered candidate is a reference, never a value"
    );
    assert_eq!(
        tree(dir.path()),
        before,
        "discovery must write nothing at all"
    );
}

/// Every file under `root`, relative and sorted.
fn tree(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                out.push(
                    path.strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn a_workspace_with_nothing_to_find_yields_nothing_rather_than_a_guess() {
    let (_dir, out) = found(&[("src/Api/Api.csproj", &csproj(&["Npgsql"]))]);

    assert!(out.candidates.is_empty(), "{:#?}", out.candidates);
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}
