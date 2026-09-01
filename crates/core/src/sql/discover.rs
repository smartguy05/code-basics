//! Finding the database connections a workspace already talks about.
//!
//! **Filesystem only. Nothing here opens a socket**, resolves a host name or
//! contacts a server, exactly as [`crate::secrets`] reads a `secrets.json`
//! without ever asking .NET anything. Discovery produces a *list to show the
//! user*; connecting is a separate, explicit act somewhere else.
//!
//! # Why this module may read a value at all
//!
//! [`crate::architecture::signals`] deliberately refuses to let a
//! connection-string **value** reach a graph:
//! `signals::dotnet::connection_strings` iterates the `ConnectionStrings`
//! object's `keys()` and never looks at what they map to, and
//! `signals::framework::admit` discards any signal whose text looks like a
//! secret assignment — whole, rather than redacted. That is not an accident to
//! be worked around: a diagram is exported and shared, so a value in one is the
//! most dangerous thing that subsystem could produce. **Nothing in
//! `architecture/` may call this module, and none of that screening is weakened
//! by it.**
//!
//! This is a separate read path with a different consumer: a SQL console the
//! user has explicitly pointed at a database. The two paths over the same
//! `appsettings.json` produce different things on purpose, and
//! `discovery_returns_values_that_the_graph_refuses` is the test that pins the
//! difference in one place.
//!
//! # Reference, not value — even here
//!
//! [`discover`] reads each value in order to sniff an engine and build the
//! redacted [`crate::sql::dsn::SqlConnectionDisplay`], and then **drops it**. A
//! [`SqlCandidate`] carries only a [`SecretSource`] naming *where* the string
//! lives, which is the same shape [`crate::sql::store`] persists and the same
//! reason: listing the connections in a workspace is not the same act as
//! handling their passwords, and a list is the thing that gets rendered, logged
//! and screenshotted. [`read_value`] is the one function that hands a value
//! back, and a caller has to ask for it by name.
//!
//! # Abstain rules
//!
//! * **An engine needs two agreeing signals** — the packages the project
//!   references *and* the connection string's own shape. Disagreement, or
//!   either signal missing, yields [`EngineChoice::NotDetermined`] /
//!   [`EngineChoice::Disagreed`] and `engine: None`: the connection is listed
//!   and not connectable until the user picks. Never a default. The two are
//!   separate variants because *"nothing said which"* and *"two things said
//!   different"* are different answers the user acts on differently.
//! * **The same logical name in two files does not collapse.**
//!   `appsettings.json`, `appsettings.Development.json` and user secrets each
//!   yield their own labelled candidate, because which one actually wins
//!   depends on `ASPNETCORE_ENVIRONMENT` at run time — something this module
//!   cannot see. Merging them would pick one and be silently wrong.
//! * **Nothing is skipped silently.** A value that is not a string, a file that
//!   will not parse and a `.env` line that cannot be read each become a warning
//!   in [`Discovery::warnings`], because a shorter list is indistinguishable
//!   from a correct one.
//! * **A warning may name a file, a project, a package or a key, and may never
//!   contain text read out of a value.** This is `DotnetSignals::warnings`'
//!   rule verbatim, and it matters more here, because this is the module that
//!   actually has the values. `a_discovery_warning_never_repeats_a_value_it_read`
//!   pins it.
//! * **Discovery never auto-saves and never auto-connects.**
//!
//! # The package table is a parameter, not an import
//!
//! Guessing an engine from a package reference needs a name table, and
//! `signals::dotnet` and `signals::node` already have one each. They are not
//! reused, because `sql -> architecture` is the wrong direction for a
//! dependency: `architecture/` is the subsystem forbidden to read values, and
//! the value-reading module reaching into it is exactly the coupling the split
//! exists to prevent. The table arrives as [`DiscoveryOptions::package_engines`]
//! instead, with [`DEFAULT_PACKAGE_ENGINES`] as what the app passes.
//!
//! It is also a genuinely different table rather than a copy: the graph's rows
//! map a package to a *diagram label* and cover MySQL, MongoDB, Redis, Kafka
//! and more, none of which the SQL console can speak. This one maps a package
//! to a [`SqlEngine`], and so has a row only for an engine that can actually be
//! connected to.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::adapters::dotnet as dotnet_manifest;
use crate::model::Project;
use crate::secrets;
use crate::workspace::Workspace;

use super::dotenv::{self, EnvValue};
use super::dsn::{self, SqlConnectionDisplay, SqlEngine};
use super::store::SecretSource;

// ---------------------------------------------------------------------------
// The package table
// ---------------------------------------------------------------------------

/// Package names that say which engine a project speaks, for the projects this
/// console can connect to.
///
/// Matching is on **name boundaries**, not raw prefixes — see
/// [`engine_from_packages`]. Rows exist only for [`SqlEngine`]s; a project
/// referencing `MongoDB.Driver` is not listed here because there is nothing the
/// console could do with the answer.
pub const DEFAULT_PACKAGE_ENGINES: &[(&str, SqlEngine)] = &[
    // .NET
    ("Npgsql", SqlEngine::Postgres),
    ("Microsoft.Data.SqlClient", SqlEngine::SqlServer),
    ("System.Data.SqlClient", SqlEngine::SqlServer),
    (
        "Microsoft.EntityFrameworkCore.SqlServer",
        SqlEngine::SqlServer,
    ),
    ("Microsoft.Data.Sqlite", SqlEngine::Sqlite),
    ("Microsoft.EntityFrameworkCore.Sqlite", SqlEngine::Sqlite),
    // npm
    ("pg", SqlEngine::Postgres),
    ("postgres", SqlEngine::Postgres),
    ("mssql", SqlEngine::SqlServer),
    ("tedious", SqlEngine::SqlServer),
    ("better-sqlite3", SqlEngine::Sqlite),
    ("sqlite3", SqlEngine::Sqlite),
];

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// How a project's .NET user secrets are read: the `secrets.json` path and its
/// text, `None` when the project declares no `<UserSecretsId>` or the file does
/// not exist, and `Err` when it declares one that could not be resolved or
/// read.
///
/// A function pointer rather than a direct call to [`secrets::read`] so tests
/// can supply their own store. The real one resolves through `%APPDATA%`, which
/// is process-global state; a test that pointed it somewhere else would be
/// racing every other test that reads it.
pub type UserSecretsReader = fn(&Path) -> Result<Option<(PathBuf, String)>, String>;

/// The knowledge [`discover`] needs that it deliberately does not own.
#[derive(Debug, Clone)]
pub struct DiscoveryOptions<'a> {
    /// Package name to engine. See the module docs for why this is a parameter.
    pub package_engines: &'a [(&'a str, SqlEngine)],
    /// How user secrets are read.
    pub read_user_secrets: UserSecretsReader,
}

impl Default for DiscoveryOptions<'static> {
    fn default() -> Self {
        Self {
            package_engines: DEFAULT_PACKAGE_ENGINES,
            read_user_secrets: default_user_secrets_reader,
        }
    }
}

/// The real user-secrets reader, over [`crate::secrets::read`].
fn default_user_secrets_reader(project: &Path) -> Result<Option<(PathBuf, String)>, String> {
    let mut secrets = secrets::read(project).map_err(|e| format!("{e:#}"))?;
    if secrets.secrets_id.is_none() {
        if let Some(id) = crate::adapters::msbuild::evaluate(project)
            .and_then(|properties| properties.get("UserSecretsId").cloned())
            .filter(|id| !id.trim().is_empty())
        {
            secrets = secrets::read_with_id(&id).map_err(|e| format!("{e:#}"))?;
        }
    }
    Ok(match (secrets.path, secrets.content) {
        (Some(path), Some(content)) => Some((path, content)),
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

/// Why a candidate cannot simply be connected to, or that it can.
///
/// Three variants, never folded into a boolean: *ready*, *the value is there
/// but nobody agreed what it speaks*, and *the value is not a value yet* are
/// three different things to tell somebody, and only the first is actionable
/// without asking them anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CandidateState {
    /// The value was read and the engine is agreed.
    Ready,
    /// The value was read; the engine was not determined. The user picks.
    EngineUnknown { reason: String },
    /// The value still contains something for another system to substitute, so
    /// there is nothing to connect *to* yet. Reported ahead of
    /// [`CandidateState::EngineUnknown`] because an engine sniffed from a
    /// half-written string is not evidence of anything.
    Unresolved { reason: String },
}

impl CandidateState {
    /// Whether the console may connect with this candidate as it stands.
    pub fn is_connectable(&self) -> bool {
        matches!(self, CandidateState::Ready)
    }
}

/// One connection the workspace mentions.
///
/// Carries **no connection string**: see the module docs. `display` is the
/// redacted view, which is the only form allowed to be shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SqlCandidate {
    /// Stable id, distinct per (file, key) so two files defining the same name
    /// are two rows rather than one.
    pub id: String,
    /// The logical connection name the author chose (`Orders`).
    pub name: String,
    /// Where it came from, workspace-relative, for the label beside the name.
    pub origin: String,
    /// The project it was found under. [`None`] means *outside any project*,
    /// which nothing currently produces but which is a different fact from a
    /// project named `""`.
    pub project: Option<String>,
    /// [`None`] whenever the two signals did not agree — never a default.
    pub engine: Option<SqlEngine>,
    /// Where the connection string is, to be re-read at connect time.
    pub source: SecretSource,
    /// The redacted description of the string that was read.
    pub display: SqlConnectionDisplay,
    pub state: CandidateState,
}

/// Everything one scan found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    pub candidates: Vec<SqlCandidate>,
    /// Everything that was seen and not listed, and why. Never contains text
    /// read out of a value.
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Engine choice
// ---------------------------------------------------------------------------

/// What the two engine signals jointly said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineChoice {
    /// Both signals were present and named the same engine.
    Agreed(SqlEngine),
    /// Both were present and named different engines.
    Disagreed {
        packages: SqlEngine,
        connection_string: SqlEngine,
    },
    /// At least one signal had nothing to say.
    NotDetermined,
}

impl EngineChoice {
    /// The engine, which only an agreement produces.
    pub fn engine(self) -> Option<SqlEngine> {
        match self {
            EngineChoice::Agreed(engine) => Some(engine),
            _ => None,
        }
    }

    /// The sentence shown beside a connection the console will not open. Names
    /// engines — which are this module's own classification of a value, not
    /// text read out of one — and never appears in [`Discovery::warnings`].
    fn reason(self) -> Option<String> {
        match self {
            EngineChoice::Agreed(_) => None,
            EngineChoice::Disagreed {
                packages,
                connection_string,
            } => Some(format!(
                "the project's package references say {}, and the connection string itself looks \
                 like {}; pick the engine before connecting",
                label(packages),
                label(connection_string)
            )),
            EngineChoice::NotDetermined => Some(
                "an engine is only taken when the project's package references and the \
                 connection string agree, and here they do not both say. Pick the engine before \
                 connecting."
                    .to_string(),
            ),
        }
    }
}

fn label(engine: SqlEngine) -> &'static str {
    match engine {
        SqlEngine::Postgres => "PostgreSQL",
        SqlEngine::SqlServer => "SQL Server",
        SqlEngine::Sqlite => "SQLite",
    }
}

/// Combine the two engine signals.
///
/// **Two agreeing signals or nothing.** One signal alone is not enough in
/// either direction: a project referencing `Npgsql` may well hold a SQL Server
/// connection string for a legacy store, and `Server=x;Database=y` is spoken by
/// several drivers. Connecting to the wrong engine is not a cosmetic error —
/// it is a failed handshake at best and a query run against the wrong server at
/// worst — so the user is asked instead.
pub fn resolve_engine(
    from_packages: Option<SqlEngine>,
    from_connection_string: Option<SqlEngine>,
) -> EngineChoice {
    match (from_packages, from_connection_string) {
        (Some(a), Some(b)) if a == b => EngineChoice::Agreed(a),
        (Some(packages), Some(connection_string)) => EngineChoice::Disagreed {
            packages,
            connection_string,
        },
        _ => EngineChoice::NotDetermined,
    }
}

/// The single engine a project's package references name, or [`None`].
///
/// Matching is exact or on a **dotted name boundary**: NuGet ids are dotted, so
/// `Npgsql.EntityFrameworkCore.PostgreSQL` is the same client as `Npgsql` and
/// matches, while `NpgsqlRest` — a different package by a different author —
/// does not. A raw `starts_with` would take the second as well. Two rows naming
/// two different engines abstain; two rows naming the *same* engine still
/// agree.
pub fn engine_from_packages(packages: &[String], table: &[(&str, SqlEngine)]) -> Option<SqlEngine> {
    // A `Vec` rather than a set because `SqlEngine` is deliberately not `Ord`:
    // there is no order over engines that means anything.
    let mut found: Vec<SqlEngine> = Vec::new();
    for package in packages {
        for (name, engine) in table {
            if matches_package(package, name) && !found.contains(engine) {
                found.push(*engine);
            }
        }
    }
    match found.as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

fn matches_package(package: &str, name: &str) -> bool {
    if package.eq_ignore_ascii_case(name) {
        return true;
    }
    package.len() > name.len()
        && package
            .get(..name.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(name))
        && package.as_bytes()[name.len()] == b'.'
}

// ---------------------------------------------------------------------------
// Reading a .NET configuration document
// ---------------------------------------------------------------------------

/// One connection string read out of a configuration document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEntry {
    /// The logical name the author chose (`Orders`).
    pub name: String,
    /// The key as .NET addresses it (`ConnectionStrings:Orders`), which is what
    /// a [`SecretSource`] records so the value is re-read from the same place
    /// whichever spelling the file used.
    pub key: String,
    /// The value, verbatim.
    pub value: String,
}

/// A key that was seen and not turned into an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedKey {
    pub key: String,
    /// Built from the key and the JSON *type* only — never from the value.
    pub reason: String,
}

/// What one configuration document yielded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigRead {
    pub entries: Vec<ConfigEntry>,
    pub skipped: Vec<SkippedKey>,
}

/// The `ConnectionStrings` of one `appsettings*.json` or `secrets.json`.
///
/// Both spellings .NET accepts are read: the nested `ConnectionStrings` object
/// and the flat `ConnectionStrings:Name` / `ConnectionStrings__Name` keys that
/// user secrets and environment-shaped configuration use. They address the same
/// configuration key, so a document spelling one name twice yields one entry
/// and a skip naming the key — which of two spellings .NET's loader ends up
/// with is not something this module can know.
///
/// The text is reduced through [`crate::secrets::strip_jsonc`] first, because
/// .NET's loader accepts comments, trailing commas and a byte-order mark and
/// `serde_json` accepts none of them. That function is *reused*, not copied: it
/// is the same dialect and there must be exactly one description of it.
///
/// `Err` is the reason the document could not be read at all, and like every
/// other message here it never quotes the text.
pub fn read_dotnet_config(text: &str) -> Result<ConfigRead, String> {
    let value: serde_json::Value = serde_json::from_str(&secrets::strip_jsonc(text))
        .map_err(|e| format!("is not JSON .NET could load (line {})", e.line()))?;
    let Some(root) = value.as_object() else {
        return Err("is not a JSON object of configuration keys".to_string());
    };

    let mut out = ConfigRead::default();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    let mut take = |name: &str, value: &serde_json::Value, out: &mut ConfigRead| {
        let key = format!("ConnectionStrings:{name}");
        if !seen.insert(key.to_ascii_lowercase()) {
            out.skipped.push(SkippedKey {
                key: key.clone(),
                reason: format!(
                    "`{key}` is spelled more than once in this file and which spelling the \
                     configuration loader keeps is not stated"
                ),
            });
            return;
        }
        match value.as_str() {
            Some(text) => out.entries.push(ConfigEntry {
                name: name.to_string(),
                key,
                value: text.to_string(),
            }),
            None => out.skipped.push(SkippedKey {
                key: key.clone(),
                reason: format!(
                    "`{key}` is {}, not a connection string",
                    json_type_name(value)
                ),
            }),
        }
    };

    if let Some((_, section)) = root
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("ConnectionStrings"))
    {
        match section.as_object() {
            Some(map) => {
                for (name, value) in map {
                    take(name, value, &mut out);
                }
            }
            None => out.skipped.push(SkippedKey {
                key: "ConnectionStrings".to_string(),
                reason: format!(
                    "`ConnectionStrings` is {}, not a section of connection strings",
                    json_type_name(section)
                ),
            }),
        }
    }

    for (key, value) in root {
        if let Some(name) = connection_key_name(key) {
            take(&name, value, &mut out);
        }
    }

    Ok(out)
}

/// The logical name in a flat connection-string key, or [`None`].
///
/// Both separators .NET accepts: `:` in a configuration path, `__` in the
/// environment-variable spelling that user secrets and containers use.
fn connection_key_name(key: &str) -> Option<String> {
    for separator in [":", "__"] {
        let prefix = format!("ConnectionStrings{separator}");
        if key.len() > prefix.len()
            && key
                .get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(&prefix))
        {
            return Some(key[prefix.len()..].to_string());
        }
    }
    None
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Find every connection the workspace mentions.
///
/// Reads files and nothing else. Saves nothing, connects to nothing.
pub fn discover(workspace: &Workspace, options: &DiscoveryOptions<'_>) -> Discovery {
    let mut out = Discovery::default();

    for project in &workspace.projects {
        match project.ecosystem.as_str() {
            "dotnet" => dotnet_project(workspace, project, options, &mut out),
            "node" => node_project(workspace, project, options, &mut out),
            _ => {}
        }
    }

    out.candidates.sort_by(|a, b| a.id.cmp(&b.id));
    out.warnings.sort();
    out.warnings.dedup();
    out
}

fn dotnet_project(
    workspace: &Workspace,
    project: &Project,
    options: &DiscoveryOptions<'_>,
    out: &mut Discovery,
) {
    let packages = match std::fs::read_to_string(&project.manifest_path) {
        Ok(text) => dotnet_manifest::parse_project_file(&text).package_references,
        Err(_) => {
            out.warnings.push(format!(
                "{}: {} could not be read, so no engine could be taken from its package \
                 references",
                project.name,
                relative(&workspace.root, &project.manifest_path)
            ));
            Vec::new()
        }
    };
    let from_packages = engine_from_packages(&packages, options.package_engines);

    for path in appsettings_files(&project.dir) {
        let origin = relative(&workspace.root, &path);
        let Ok(text) = std::fs::read_to_string(&path) else {
            out.warnings
                .push(format!("{}: {origin} could not be read", project.name));
            continue;
        };
        let read = match read_dotnet_config(&text) {
            Ok(read) => read,
            Err(why) => {
                out.warnings
                    .push(format!("{}: {origin} {why}", project.name));
                continue;
            }
        };
        emit(
            read,
            &format!("appsettings:{origin}"),
            &origin,
            project,
            from_packages,
            |key| SecretSource::AppSettings {
                path: path.clone(),
                key,
            },
            out,
        );
    }

    let manifest = relative(&workspace.root, &project.manifest_path);
    match (options.read_user_secrets)(&project.manifest_path) {
        Ok(Some((secrets_path, text))) => {
            let origin = format!("user secrets ({manifest})");
            match read_dotnet_config(&text) {
                Ok(read) => emit(
                    read,
                    &format!("usersecrets:{manifest}"),
                    &origin,
                    project,
                    from_packages,
                    |key| SecretSource::UserSecrets {
                        project: project.manifest_path.clone(),
                        key,
                    },
                    out,
                ),
                Err(why) => out
                    .warnings
                    .push(format!("{}: its user secrets file {why}", project.name)),
            }
            // The path is resolved and then deliberately unused: a candidate
            // records the *project*, because the `<UserSecretsId>` that resolves
            // the location can change between now and connect time.
            let _ = secrets_path;
        }
        Ok(None) => {}
        Err(why) => out.warnings.push(format!(
            "{}: its user secrets could not be read ({why})",
            project.name
        )),
    }
}

/// The `.env` files this module reads, beside a `package.json`.
///
/// A fixed list rather than a glob: `.env.production` and `.env.test` name
/// environments this machine is not, and offering their connections in a picker
/// invites connecting to one.
const ENV_FILES: &[&str] = &[".env", ".env.local", ".env.development"];

fn node_project(
    workspace: &Workspace,
    project: &Project,
    options: &DiscoveryOptions<'_>,
    out: &mut Discovery,
) {
    let packages = match std::fs::read_to_string(&project.manifest_path) {
        Ok(text) => node_dependencies(&text),
        Err(_) => {
            out.warnings.push(format!(
                "{}: {} could not be read, so no engine could be taken from its dependencies",
                project.name,
                relative(&workspace.root, &project.manifest_path)
            ));
            Vec::new()
        }
    };
    let from_packages = engine_from_packages(&packages, options.package_engines);

    for name in ENV_FILES {
        let path = project.dir.join(name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let origin = relative(&workspace.root, &path);
        let file = dotenv::parse(&text);

        for problem in &file.problems {
            out.warnings
                .push(format!("{}: {origin}: {}", project.name, problem.reason));
        }

        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for entry in &file.entries {
            if !seen.insert(entry.key.as_str()) {
                continue;
            }
            // The *last* assignment is the one a shell sourcing the file ends
            // up with, so that is the one read.
            let effective = file.get(&entry.key).unwrap_or(entry);
            let Some(name) = env_connection_name(&effective.key, effective.value.as_written())
            else {
                continue;
            };
            out.candidates.push(candidate(
                format!("dotenv:{origin}:{}", effective.key),
                name,
                origin.clone(),
                project,
                SecretSource::DotEnv {
                    path: path.clone(),
                    key: effective.key.clone(),
                },
                effective.value.clone(),
                from_packages,
            ));
        }
    }
}

/// Whether a `.env` entry names a database connection, and under what name.
///
/// Two ways in, and no third: the key is a `ConnectionStrings` key or the
/// ubiquitous `DATABASE_URL`, **or** the value's own shape is one
/// [`dsn::sniff_engine`] recognises — the string saying what it is. Everything
/// else is left alone, because listing every environment variable as a possible
/// database is a guess, and a noisy one.
fn env_connection_name(key: &str, value: &str) -> Option<String> {
    if let Some(name) = connection_key_name(key) {
        return Some(name);
    }
    if key.eq_ignore_ascii_case("DATABASE_URL") || dsn::sniff_engine(value).is_some() {
        return Some(key.to_string());
    }
    None
}

/// The dependency names in a `package.json`.
///
/// `package.json` is strict JSON — npm itself will not read anything else — so
/// a parse failure here is a real error rather than a dialect difference, and
/// the caller reports it.
fn node_dependencies(text: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for section in ["dependencies", "devDependencies"] {
        if let Some(map) = value.get(section).and_then(|v| v.as_object()) {
            out.extend(map.keys().cloned());
        }
    }
    out
}

/// Turn one document's read into candidates and warnings.
#[allow(clippy::too_many_arguments)]
fn emit(
    read: ConfigRead,
    id_prefix: &str,
    origin: &str,
    project: &Project,
    from_packages: Option<SqlEngine>,
    source: impl Fn(String) -> SecretSource,
    out: &mut Discovery,
) {
    for skipped in read.skipped {
        out.warnings
            .push(format!("{}: {origin}: {}", project.name, skipped.reason));
    }
    for entry in read.entries {
        out.candidates.push(candidate(
            format!("{id_prefix}:{}", entry.key),
            entry.name,
            origin.to_string(),
            project,
            source(entry.key.clone()),
            dotenv::classify_value(entry.value),
            from_packages,
        ));
    }
}

/// Build one candidate, reading the value and then dropping it.
fn candidate(
    id: String,
    name: String,
    origin: String,
    project: &Project,
    source: SecretSource,
    value: EnvValue,
    from_packages: Option<SqlEngine>,
) -> SqlCandidate {
    let written = value.as_written();
    let choice = resolve_engine(from_packages, dsn::sniff_engine(written));
    // `display_form` is whitelist-based and redacts, so it is safe over an
    // unresolved value too — and showing `Host=${DB_HOST}` is exactly what
    // tells the user which reference is missing.
    let display = dsn::display_form(written);

    let state = match &value {
        EnvValue::Unresolved { reason, .. } => CandidateState::Unresolved {
            reason: reason.clone(),
        },
        EnvValue::Literal { .. } => match choice.reason() {
            Some(reason) => CandidateState::EngineUnknown { reason },
            None => CandidateState::Ready,
        },
    };

    SqlCandidate {
        id,
        name,
        origin,
        project: Some(project.name.clone()),
        engine: choice.engine(),
        source,
        display,
        state,
    }
}

// ---------------------------------------------------------------------------
// Reading a value back
// ---------------------------------------------------------------------------

/// Re-read the connection string a candidate points at.
///
/// **This is the value-returning path**, and it is a separate function from
/// [`discover`] on purpose: listing the connections in a workspace and handling
/// one's password are different acts, and only this one is the second. It is
/// also what makes a saved profile hold no secret — the value lives in the file
/// the user already had, and a rotated password just works.
///
/// `Err` names what could not be read and never quotes what was read.
pub fn read_value(
    source: &SecretSource,
    options: &DiscoveryOptions<'_>,
) -> Result<EnvValue, String> {
    match source {
        SecretSource::Literal { connection_string } => {
            Ok(dotenv::classify_value(connection_string.clone()))
        }
        SecretSource::AppSettings { path, key } => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("{} could not be read: {e}", path.display()))?;
            let read =
                read_dotnet_config(&text).map_err(|why| format!("{} {why}", path.display()))?;
            find(read, key, &path.display().to_string())
        }
        SecretSource::UserSecrets { project, key } => {
            let Some((path, text)) = (options.read_user_secrets)(project)? else {
                return Err(format!("{} has no user secrets file", project.display()));
            };
            let read =
                read_dotnet_config(&text).map_err(|why| format!("{} {why}", path.display()))?;
            find(read, key, &path.display().to_string())
        }
        SecretSource::DotEnv { path, key } => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("{} could not be read: {e}", path.display()))?;
            dotenv::parse(&text)
                .get(key)
                .map(|entry| entry.value.clone())
                .ok_or_else(|| format!("`{key}` is no longer in {}", path.display()))
        }
    }
}

fn find(read: ConfigRead, key: &str, where_: &str) -> Result<EnvValue, String> {
    read.entries
        .into_iter()
        .find(|entry| entry.key.eq_ignore_ascii_case(key))
        .map(|entry| dotenv::classify_value(entry.value))
        .ok_or_else(|| format!("`{key}` is no longer in {where_}"))
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// `appsettings*.json` in the project's own directory, sorted.
///
/// Only that directory, matching what ASP.NET Core loads from the content root
/// — walking deeper would pick up the copies that sit beside test fixtures and
/// sample apps.
fn appsettings_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            path.is_file() && name.starts_with("appsettings") && name.ends_with(".json")
        })
        .collect();
    files.sort();
    files
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
#[path = "discover_tests.rs"]
mod tests;
