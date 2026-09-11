//! Saved Redis connection profiles: `<config>/code-basics/redis-connections.json`.
//!
//! Modelled directly on [`crate::sql::store`], with the same reasoning that put
//! that file under the user config dir rather than `.code-basics/`: a connection
//! string can carry a password, and `.code-basics/` is the directory this app
//! shares with the team. See [`crate::sql::store`] for the full argument.
//!
//! # Reference, not value — the type is shared, not copied
//!
//! A Redis connection is found in the same places a SQL one is — an
//! `appsettings.json` key, a user-secrets key, a `.env` key — so it reuses
//! **the same** [`SecretSource`] rather than defining a parallel one.
//! `SecretSource::holds_a_secret` is documented as the single place the
//! confinement rule lives, so a second copy could drift from it; sharing keeps
//! one definition of "which variant is a secret" for both subsystems. The one
//! cost is a `redis → sql` dependency for a type whose name says `sql`; it is
//! genuinely engine-neutral (it names a file and a key, nothing SQL), and this
//! note records that it is shared on purpose.
//!
//! Unlike [`crate::sql::store::SqlConnection`] there is **no `engine` field**:
//! Redis is one engine, so there is nothing to determine.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

pub use crate::sql::store::SecretSource;

/// One saved Redis connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RedisConnection {
    /// Stable id: the React key and the target of rename/delete.
    pub id: String,
    /// The label the user sees in the picker.
    pub name: String,
    /// Where the connection string comes from.
    pub secret: SecretSource,
    /// The workspace this profile was created under (lets a picker put the open
    /// codebase's connections first). `None` = created outside any workspace.
    pub workspace_root: Option<PathBuf>,
    /// **Consent for an agent to *write* to this connection, and nothing else.**
    ///
    /// Orthogonal to `expose_to_agents`. `#[serde(default)]` makes an absent key
    /// `false`: consent is withheld unless explicitly granted, and an older or
    /// hand-edited file must load as withheld and still load.
    #[serde(default)]
    pub allow_writes: bool,
    /// **Consent for an agent to see this connection at all, and nothing else.**
    ///
    /// `#[serde(default)]` = withheld, for the same reason. Reading exposes
    /// everything that login can read; writing additionally needs `allow_writes`.
    #[serde(default)]
    pub expose_to_agents: bool,
    /// **The user typed this name**, so the picker derives no label from the
    /// source. `#[serde(default)]` keeps the file at version 1.
    #[serde(default)]
    pub user_named: bool,
    /// When created, ms since the Unix epoch (the caller's clock).
    pub created_at_ms: u64,
    /// When last connected with. `None` = never used.
    pub last_used_ms: Option<u64>,
}

/// The whole file: a schema version and the saved connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RedisConnectionsFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub connections: Vec<RedisConnection>,
}

fn default_version() -> u32 {
    1
}

impl Default for RedisConnectionsFile {
    fn default() -> Self {
        Self {
            version: default_version(),
            connections: Vec::new(),
        }
    }
}

/// The file name inside `code-basics/`.
pub const REDIS_CONNECTIONS_FILE: &str = "redis-connections.json";

/// The environment variable that overrides the whole path.
pub const REDIS_CONNECTIONS_PATH_ENV: &str = "CB_REDIS_CONNECTIONS_PATH";

/// Where the connections file lives: `<config>/code-basics/redis-connections.json`.
/// Same resolution as [`crate::sql::store::sql_connections_path`].
pub fn redis_connections_path() -> PathBuf {
    if let Some(path) = std::env::var_os(REDIS_CONNECTIONS_PATH_ENV) {
        return PathBuf::from(path);
    }

    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));

    base.join("code-basics").join(REDIS_CONNECTIONS_FILE)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Read the connections at `path`. A missing or unparseable file yields an empty
/// store rather than an error — a corrupt file must not stop the picker opening.
pub fn load(path: &Path) -> RedisConnectionsFile {
    let Ok(text) = std::fs::read_to_string(path) else {
        return RedisConnectionsFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// Write the connections to `path`, atomically and crash-safely (temp + rename,
/// with a `.bak` before an empty overwrite). Mirrors [`crate::sql::store::save`].
pub fn save(path: &Path, file: &RedisConnectionsFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    if file.connections.is_empty() && !load(path).connections.is_empty() {
        let _ = std::fs::copy(path, sibling(path, ".bak"));
    }

    let json =
        serde_json::to_string_pretty(file).context("failed to serialise redis connections")?;
    let tmp = sibling(path, ".tmp");
    std::fs::write(&tmp, format!("{json}\n"))
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

// ---------------------------------------------------------------------------
// Mutations. Each consent flag moves ONLY through its own function, and `upsert`
// ignores both — a form round-trip can never raise consent. Mirrors the SQL
// bridge's store helpers.
// ---------------------------------------------------------------------------

/// Insert a new profile or replace an existing one **without touching its
/// consent flags**. A save from a form carries whatever flags the client sent;
/// ignoring them here is what stops a round-trip silently granting exposure or
/// writes. On replace, the stored `allow_writes`/`expose_to_agents` are kept.
pub fn upsert(file: &mut RedisConnectionsFile, incoming: RedisConnection) {
    if let Some(existing) = file.connections.iter_mut().find(|c| c.id == incoming.id) {
        let allow_writes = existing.allow_writes;
        let expose = existing.expose_to_agents;
        *existing = RedisConnection {
            allow_writes,
            expose_to_agents: expose,
            ..incoming
        };
    } else {
        // A brand-new profile starts with both flags OFF whatever the payload said.
        file.connections.push(RedisConnection {
            allow_writes: false,
            expose_to_agents: false,
            ..incoming
        });
    }
}

/// Remove a profile by id. Returns whether one was removed.
pub fn remove(file: &mut RedisConnectionsFile, id: &str) -> bool {
    let before = file.connections.len();
    file.connections.retain(|c| c.id != id);
    file.connections.len() != before
}

/// Rename a profile, marking it user-named so the picker stops deriving a label.
pub fn rename(file: &mut RedisConnectionsFile, id: &str, name: &str) -> bool {
    if let Some(c) = file.connections.iter_mut().find(|c| c.id == id) {
        c.name = name.to_string();
        c.user_named = true;
        true
    } else {
        false
    }
}

/// Set the write-consent flag — the only place it moves up or down.
pub fn set_allow_writes(file: &mut RedisConnectionsFile, id: &str, allow: bool) -> bool {
    if let Some(c) = file.connections.iter_mut().find(|c| c.id == id) {
        c.allow_writes = allow;
        true
    } else {
        false
    }
}

/// Set the exposure flag — the only place it moves up or down.
pub fn set_expose_to_agents(file: &mut RedisConnectionsFile, id: &str, expose: bool) -> bool {
    if let Some(c) = file.connections.iter_mut().find(|c| c.id == id) {
        c.expose_to_agents = expose;
        true
    } else {
        false
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
