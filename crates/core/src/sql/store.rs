//! Saved connection profiles: `<config>/code-basics/sql-connections.json`.
//!
//! # Why this store is user-global and not `.code-basics/`
//!
//! Every other per-workspace thing this app persists lives in `.code-basics/`,
//! and it would be the obvious home for a list of the databases a workspace
//! talks to. It is the wrong one, for a reason that is not about tidiness:
//! **a connection string contains a password, and `.code-basics/` is the
//! directory this app deliberately shares with the team** — it is committed, it
//! carries the checked-in `config.json`, the declarative adapters and the
//! diagrams, and its whole purpose is to travel with the repository.
//!
//! A gitignore entry is not a defence for a secret. It is one `git add -f`, one
//! edit to `.gitignore`, one `git stash --include-untracked`, one zip of the
//! working tree or one "share your repo folder" away from being handed to
//! somebody else, and each of those is an ordinary thing for a developer to do
//! rather than a mistake. The failure is also silent and permanent: nothing
//! tells you a password left, and once it is in a commit it stays in the
//! history. So the file lives beside [`crate::notes`] and
//! [`crate::launcher::store`] under the user's config directory, outside every
//! repository, where nothing about sharing a codebase can carry it along.
//! Those two are already user-global for a weaker version of this argument —
//! "the commands I run" and "my scratchpad" belong to the person, not the repo
//! — and the same reasoning applies here with a much sharper edge.
//!
//! # Reference, not value
//!
//! The stronger protection is that in the common case there is nothing to
//! protect: a **discovered** connection records *where* its connection string
//! lives — an `appsettings.json` key, a user-secrets key, a `.env` key — and
//! that value is re-read at connect time. The app then stores no secret at all,
//! and a rotated password just works with no re-entry. Only
//! [`SecretSource::Literal`] holds a value, and it exists solely because the
//! user typed one and asked for it to be remembered;
//! [`SecretSource::holds_a_secret`] is what lets a caller tell the two apart
//! and keep the literal from crossing IPC or reaching a log. "The user typed
//! it" and "the app knows where to look" are different facts and must never
//! collapse into one.
//!
//! Each entry also records the `workspace_root` it was created under, so a
//! picker can group *this codebase first* out of this one store, exactly as
//! [`crate::launcher::recents::group`] does — a second, per-workspace store
//! would be the `.code-basics/` mistake by another route.
//!
//! # Shape
//!
//! Modelled directly on [`crate::notes`]: tolerant [`load`] (a missing **or**
//! corrupt file yields an empty store, never an error — a bad file must not
//! stop the picker opening), atomic crash-safe [`save`] (temp + rename, with a
//! `.bak` before an empty overwrite). The path is resolved once by
//! [`sql_connections_path`] and passed into [`load`]/[`save`], so tests drive
//! the store against a temp file without touching the real config directory.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::dsn::SqlEngine;

// ---------------------------------------------------------------------------
// Where the connection string comes from
// ---------------------------------------------------------------------------

/// Where a profile's connection string is to be found at connect time.
///
/// Three of the four variants are **references**: they name a file and a key,
/// and hold no secret whatsoever. The fourth is a value, and is the only one
/// that ever is — see the module docs. Adding a fifth variant that carries a
/// value is a decision about secrets, not a data-modelling convenience.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SecretSource {
    /// The user typed a connection string and asked for it to be remembered.
    /// **The only variant holding a secret value.**
    Literal { connection_string: String },
    /// A key in a .NET `appsettings*.json`, re-read at connect time.
    AppSettings { path: PathBuf, key: String },
    /// A key in a project's .NET user secrets, re-read at connect time.
    /// `project` is the `.csproj`, not the secrets file: the `<UserSecretsId>`
    /// is what resolves the location, and it can change.
    UserSecrets { project: PathBuf, key: String },
    /// A key in a `.env`-style file, re-read at connect time.
    DotEnv { path: PathBuf, key: String },
}

impl SecretSource {
    /// Whether this source carries a secret **value** rather than a reference
    /// to one.
    ///
    /// A caller that is about to send a profile across IPC, put it in an error
    /// message or write it anywhere outside this store must consult this — it
    /// is the single place the confinement rule is expressed, so it cannot
    /// drift the way a per-call-site check would.
    pub fn holds_a_secret(&self) -> bool {
        matches!(self, SecretSource::Literal { .. })
    }
}

// ---------------------------------------------------------------------------
// A saved profile
// ---------------------------------------------------------------------------

/// One saved connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SqlConnection {
    /// Stable id: the React key and the target of rename/delete.
    pub id: String,
    /// The label the user sees in the picker.
    pub name: String,
    /// The engine, when it is known. [`None`] means *not determined or
    /// ambiguous* — [`super::dsn::sniff_engine`] abstains rather than
    /// defaulting, and so does this.
    pub engine: Option<SqlEngine>,
    /// Where the connection string comes from.
    pub secret: SecretSource,
    /// The workspace this profile was created under, which is what lets a
    /// picker put the open codebase's connections first. [`None`] means
    /// *created outside any workspace* — a different fact from being created
    /// under one, so it crosses as an explicit `null`.
    pub workspace_root: Option<PathBuf>,
    /// **Consent for the read-only guard, and nothing else.**
    ///
    /// `#[serde(default)]` makes an absent key `false`: a hand-written file, an
    /// older file, or one whose entry was edited by hand must load as *consent
    /// withheld*, and must still load — an entry that vanished because it did
    /// not mention this key would be indistinguishable from one whose consent
    /// silently appeared.
    #[serde(default)]
    pub allow_writes: bool,
    /// When the profile was created, milliseconds since the Unix epoch. The
    /// clock is the caller's, as in [`crate::notes::Note`].
    pub created_at_ms: u64,
    /// When it was last connected with. [`None`] means *never used*, which is
    /// not the same as "used at time zero".
    pub last_used_ms: Option<u64>,
}

/// The whole file: a schema version and the saved connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SqlConnectionsFile {
    /// Schema version, so a future format change can migrate rather than fail.
    #[serde(default = "default_version")]
    pub version: u32,
    /// The saved connections, in the order the picker lists them.
    #[serde(default)]
    pub connections: Vec<SqlConnection>,
}

fn default_version() -> u32 {
    1
}

impl Default for SqlConnectionsFile {
    fn default() -> Self {
        Self {
            version: default_version(),
            connections: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Location
// ---------------------------------------------------------------------------

/// The file name inside `code-basics/`.
pub const SQL_CONNECTIONS_FILE: &str = "sql-connections.json";

/// The environment variable that overrides the whole path.
pub const SQL_CONNECTIONS_PATH_ENV: &str = "CB_SQL_CONNECTIONS_PATH";

/// Where the connections file lives:
/// `<config>/code-basics/sql-connections.json`.
///
/// [`SQL_CONNECTIONS_PATH_ENV`] overrides the whole path and is checked
/// **first**, matching [`crate::notes::notes_path`] and
/// [`crate::launcher::store::launchers_path`]; otherwise the base is
/// `%APPDATA%`, then `$XDG_CONFIG_HOME`, then `~/.config`, then the current
/// directory.
pub fn sql_connections_path() -> PathBuf {
    if let Some(path) = std::env::var_os(SQL_CONNECTIONS_PATH_ENV) {
        return PathBuf::from(path);
    }

    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));

    base.join("code-basics").join(SQL_CONNECTIONS_FILE)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

// ---------------------------------------------------------------------------
// Read and write
// ---------------------------------------------------------------------------

/// Read the connections at `path`. A missing **or** unparseable file yields an
/// empty store rather than an error — a corrupt file must not stop the picker
/// opening, the same tolerance as [`crate::notes::load`].
pub fn load(path: &Path) -> SqlConnectionsFile {
    let Ok(text) = std::fs::read_to_string(path) else {
        return SqlConnectionsFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// A sibling of `path` with `suffix` appended to its file name
/// (`sql-connections.json` + `.tmp` → `sql-connections.json.tmp`).
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// Write the connections to `path`, creating the parent directory if absent.
///
/// **Atomic and crash-safe**: the JSON goes to a sibling `.tmp` and is then
/// renamed over the target, so a crash mid-write can never leave a truncated
/// file — which, given the tolerant [`load`], would read back as *no saved
/// connections* and then be clobbered on the next save. `fs::rename` replaces
/// the destination atomically on both Windows and POSIX.
///
/// As a second guard against that same cascade, replacing a **non-empty** file
/// with an **empty** one first copies the previous content to a sibling `.bak`.
/// A deliberate "forget them all" still applies, but the profiles — which took
/// discovery or typing to create — stay recoverable.
pub fn save(path: &Path, file: &SqlConnectionsFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    if file.connections.is_empty() && !load(path).connections.is_empty() {
        // Best effort: a failed backup must not block the save itself.
        let _ = std::fs::copy(path, sibling(path, ".bak"));
    }

    let json = serde_json::to_string_pretty(file).context("failed to serialise connections")?;
    let tmp = sibling(path, ".tmp");
    std::fs::write(&tmp, format!("{json}\n"))
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
