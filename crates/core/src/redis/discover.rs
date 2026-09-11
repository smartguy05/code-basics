//! Finding the Redis connections a workspace already talks about.
//!
//! **Filesystem only — nothing here opens a socket.** The Redis counterpart of
//! [`crate::sql::discover`], and it reuses that module's readers rather than
//! copying them: `read_dotnet_config` for `appsettings*.json`/user secrets,
//! `dotenv::parse` for `.env`, and `read_value` for the value-returning path.
//! What differs is selection — Redis has one engine, so a candidate is chosen by
//! whether the connection *looks* like Redis ([`super::dsn::looks_like_redis`])
//! or its key names Redis/cache, and there is no engine to determine.
//!
//! # The architecture graph still may not read values
//!
//! [`crate::architecture::signals`] refuses to let any connection-string value
//! reach a diagram, and that is unchanged: this is a separate read path with a
//! different consumer (a Redis panel the user pointed at a server), exactly as
//! the SQL discoverer is. Nothing in `architecture/` calls this module.
//!
//! # Reference, not value
//!
//! Each value is read only to build the redacted [`super::dsn::RedisConnectionDisplay`]
//! and then dropped; a [`RedisCandidate`] carries only a [`SecretSource`] naming
//! where the string lives. [`super::store`] persists that same shape.
//!
//! # A known limitation, stated
//!
//! Only `ConnectionStrings:*` entries and `.env` keys are scanned. A Redis
//! configuration under a custom section (`Redis:Configuration`,
//! `RedisCacheOptions:Configuration`) is not found — capturing those needs a
//! second reader, and this first cut abstains rather than half-reading them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

use super::dsn::{self, RedisConnectionDisplay};
use super::store::SecretSource;
use crate::model::Project;
use crate::sql::discover::{read_dotnet_config, DiscoveryOptions};
use crate::sql::dotenv::{self, EnvValue};
use crate::workspace::Workspace;

/// Whether a candidate is usable now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CandidateState {
    /// A literal Redis connection string is present.
    Ready,
    /// The value has an unsubstituted placeholder, or is empty.
    Unresolved { reason: String },
}

impl CandidateState {
    pub fn is_connectable(&self) -> bool {
        matches!(self, CandidateState::Ready)
    }
}

/// One discovered Redis connection. Carries **no** connection string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RedisCandidate {
    pub id: String,
    pub name: String,
    pub origin: String,
    pub project: Option<String>,
    pub source: SecretSource,
    pub display: RedisConnectionDisplay,
    pub state: CandidateState,
}

/// Everything a scan found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    pub candidates: Vec<RedisCandidate>,
    pub warnings: Vec<String>,
}

/// The `.env` files scanned, beside a `package.json` — the same fixed list the
/// SQL discoverer uses (environments this machine actually is).
const ENV_FILES: &[&str] = &[".env", ".env.local", ".env.development"];

/// Find every Redis connection the workspace mentions. Reads files only.
pub fn discover(workspace: &Workspace) -> Discovery {
    let options = DiscoveryOptions::default();
    let mut out = Discovery::default();

    for project in &workspace.projects {
        match project.ecosystem.as_str() {
            "dotnet" => dotnet_project(workspace, project, &options, &mut out),
            "node" => node_project(workspace, project, &mut out),
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
        for entry in read.entries {
            if !is_redis_connection(&entry.name, entry.value.as_str()) {
                continue;
            }
            out.candidates.push(candidate(
                format!("appsettings:{origin}:{}", entry.key),
                entry.name,
                origin.clone(),
                project,
                SecretSource::AppSettings {
                    path: path.clone(),
                    key: entry.key.clone(),
                },
                dotenv::classify_value(entry.value),
            ));
        }
    }

    let manifest = relative(&workspace.root, &project.manifest_path);
    match (options.read_user_secrets)(&project.manifest_path) {
        Ok(Some((_secrets_path, text))) => {
            let origin = format!("user secrets ({manifest})");
            match read_dotnet_config(&text) {
                Ok(read) => {
                    for entry in read.entries {
                        if !is_redis_connection(&entry.name, entry.value.as_str()) {
                            continue;
                        }
                        out.candidates.push(candidate(
                            format!("usersecrets:{manifest}:{}", entry.key),
                            entry.name,
                            origin.clone(),
                            project,
                            SecretSource::UserSecrets {
                                project: project.manifest_path.clone(),
                                key: entry.key.clone(),
                            },
                            dotenv::classify_value(entry.value),
                        ));
                    }
                }
                Err(why) => out
                    .warnings
                    .push(format!("{}: its user secrets file {why}", project.name)),
            }
        }
        Ok(None) => {}
        Err(why) => out.warnings.push(format!(
            "{}: its user secrets could not be read ({why})",
            project.name
        )),
    }
}

fn node_project(workspace: &Workspace, project: &Project, out: &mut Discovery) {
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
            let effective = file.get(&entry.key).unwrap_or(entry);
            if !is_redis_env(&effective.key, effective.value.as_written()) {
                continue;
            }
            out.candidates.push(candidate(
                format!("dotenv:{origin}:{}", effective.key),
                effective.key.clone(),
                origin.clone(),
                project,
                SecretSource::DotEnv {
                    path: path.clone(),
                    key: effective.key.clone(),
                },
                effective.value.clone(),
            ));
        }
    }
}

/// Whether a `ConnectionStrings` entry names a Redis connection: the value looks
/// like Redis, or the connection name mentions redis/cache.
fn is_redis_connection(name: &str, value: &str) -> bool {
    dsn::looks_like_redis(value) || name_mentions_redis(name)
}

/// Whether a `.env` entry names a Redis connection: a redis-ish key, or a value
/// whose own shape is Redis.
fn is_redis_env(key: &str, value: &str) -> bool {
    name_mentions_redis(key) || dsn::looks_like_redis(value)
}

fn name_mentions_redis(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("redis") || lower.contains("cache")
}

fn candidate(
    id: String,
    name: String,
    origin: String,
    project: &Project,
    source: SecretSource,
    value: EnvValue,
) -> RedisCandidate {
    let written = value.as_written();
    let display = dsn::display_form(written);
    let state = match &value {
        EnvValue::Unresolved { reason, .. } => CandidateState::Unresolved {
            reason: reason.clone(),
        },
        EnvValue::Literal { text } if text.trim().is_empty() => CandidateState::Unresolved {
            reason: "the configured value is empty".to_string(),
        },
        EnvValue::Literal { .. } => CandidateState::Ready,
    };
    RedisCandidate {
        id,
        name,
        origin,
        project: Some(project.name.clone()),
        source,
        display,
        state,
    }
}

/// Re-read the connection string a candidate (or a saved profile) points at.
/// Delegates to the SQL discoverer's value path — the source shapes are shared.
pub fn read_value(source: &SecretSource) -> Result<EnvValue, String> {
    crate::sql::discover::read_value(source, &DiscoveryOptions::default())
}

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
