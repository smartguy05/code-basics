//! The Redis panel's bridge: list/save/discover connection profiles, browse
//! keys, and edit values. Thin, like the SQL bridge it mirrors — every decision
//! lives in [`cb_core::redis`].
//!
//! # Nothing here sends a connection string toward the frontend
//!
//! No command returns a [`cb_core::redis::store::RedisConnection`] (it can hold a
//! `SecretSource::Literal`, a password). Every read-back goes through
//! [`redact_connection`], whose [`RedisSecretView`] has no variant that could
//! carry one — a literal is described only by the redacted
//! [`cb_core::redis::dsn::display_form`]. Driver errors are redacted twice, on
//! construction and again on the way out.
//!
//! # Consent is its own verb — twice
//!
//! [`redis_set_allow_writes`] and [`redis_set_expose_to_agents`] are the only
//! ways those flags move; [`cb_core::redis::store::upsert`] ignores both on save,
//! so no form round-trip raises consent. **The UI itself may always write** —
//! `allow_writes` is the *agent* gate, so the human panel's edits go through the
//! driver directly, exactly as the SQL console can run writes the agent path
//! forbids.

use std::path::PathBuf;
use std::time::Duration;

use cb_core::redis::discover::{self, Discovery};
use cb_core::redis::driver::{DriverError, ReadResult, RedisClient, DEFAULT_CONNECT_TIMEOUT};
use cb_core::redis::dsn;
use cb_core::redis::model::{RedisKeyInfo, RedisStatusKind, RedisValue, ScanPage};
use cb_core::redis::ops::{self, ReadOp, WriteOp};
use cb_core::redis::store::{self, RedisConnection, SecretSource};
use serde::{Deserialize, Serialize};
use specta::Type;

const OP_TIMEOUT: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------------------
// The redacted view (rule: no connection string toward the frontend)
// ---------------------------------------------------------------------------

/// Where a profile's connection string lives, with **no variant** that can carry
/// the string itself. A literal is described only by its redacted display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RedisSecretView {
    Literal {
        display: dsn::RedisConnectionDisplay,
    },
    AppSettings {
        path: PathBuf,
        key: String,
    },
    UserSecrets {
        project: PathBuf,
        key: String,
    },
    DotEnv {
        path: PathBuf,
        key: String,
    },
}

/// A saved connection as the frontend sees it — never carrying a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RedisConnectionView {
    pub id: String,
    pub name: String,
    pub secret: RedisSecretView,
    pub holds_a_secret: bool,
    pub workspace_root: Option<PathBuf>,
    pub allow_writes: bool,
    pub expose_to_agents: bool,
    pub user_named: bool,
    pub created_at_ms: u64,
    pub last_used_ms: Option<u64>,
}

/// The one seam that turns a stored profile into a view. A literal secret becomes
/// a redacted display and never crosses whole.
pub(crate) fn redact_connection(stored: &RedisConnection) -> RedisConnectionView {
    let secret = match &stored.secret {
        SecretSource::Literal { connection_string } => RedisSecretView::Literal {
            display: dsn::display_form(connection_string),
        },
        SecretSource::AppSettings { path, key } => RedisSecretView::AppSettings {
            path: path.clone(),
            key: key.clone(),
        },
        SecretSource::UserSecrets { project, key } => RedisSecretView::UserSecrets {
            project: project.clone(),
            key: key.clone(),
        },
        SecretSource::DotEnv { path, key } => RedisSecretView::DotEnv {
            path: path.clone(),
            key: key.clone(),
        },
    };
    RedisConnectionView {
        id: stored.id.clone(),
        name: stored.name.clone(),
        holds_a_secret: stored.secret.holds_a_secret(),
        secret,
        workspace_root: stored.workspace_root.clone(),
        allow_writes: stored.allow_writes,
        expose_to_agents: stored.expose_to_agents,
        user_named: stored.user_named,
        created_at_ms: stored.created_at_ms,
        last_used_ms: stored.last_used_ms,
    }
}

fn views(file: &store::RedisConnectionsFile) -> Vec<RedisConnectionView> {
    file.connections.iter().map(redact_connection).collect()
}

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn redis_list_connections() -> Result<Vec<RedisConnectionView>, String> {
    Ok(views(&store::load(&store::redis_connections_path())))
}

#[tauri::command]
pub async fn redis_discover(root: String) -> Result<Discovery, String> {
    let workspace = cb_core::workspace::workspace_from_dir(&PathBuf::from(&root))
        .map_err(|e| format!("{e:#}"))?;
    Ok(discover::discover(&workspace))
}

#[tauri::command]
pub async fn redis_save_connection(
    connection: RedisConnection,
) -> Result<Vec<RedisConnectionView>, String> {
    let path = store::redis_connections_path();
    let mut file = store::load(&path);
    store::upsert(&mut file, connection);
    store::save(&path, &file).map_err(|e| format!("{e:#}"))?;
    Ok(views(&file))
}

#[tauri::command]
pub async fn redis_delete_connection(id: String) -> Result<Vec<RedisConnectionView>, String> {
    mutate(|file| {
        store::remove(file, &id);
    })
}

#[tauri::command]
pub async fn redis_rename_connection(
    id: String,
    name: String,
) -> Result<Vec<RedisConnectionView>, String> {
    mutate(|file| {
        store::rename(file, &id, &name);
    })
}

#[tauri::command]
pub async fn redis_set_allow_writes(
    id: String,
    allow_writes: bool,
) -> Result<Vec<RedisConnectionView>, String> {
    mutate(|file| {
        store::set_allow_writes(file, &id, allow_writes);
    })
}

#[tauri::command]
pub async fn redis_set_expose_to_agents(
    id: String,
    expose_to_agents: bool,
) -> Result<Vec<RedisConnectionView>, String> {
    mutate(|file| {
        store::set_expose_to_agents(file, &id, expose_to_agents);
    })
}

fn mutate(
    f: impl FnOnce(&mut store::RedisConnectionsFile),
) -> Result<Vec<RedisConnectionView>, String> {
    let path = store::redis_connections_path();
    let mut file = store::load(&path);
    f(&mut file);
    store::save(&path, &file).map_err(|e| format!("{e:#}"))?;
    Ok(views(&file))
}

// ---------------------------------------------------------------------------
// Browsing and editing (the UI may always write)
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn redis_test_connection(id: String) -> Result<RedisStatusKind, String> {
    let profile = resolve(&id)?;
    let target = match resolve_target(&profile) {
        Ok(t) => t,
        Err(kind) => return Ok(kind),
    };
    match RedisClient::connect(&target, DEFAULT_CONNECT_TIMEOUT).await {
        Ok(mut client) => match client.ping().await {
            Ok(()) => Ok(RedisStatusKind::Ok),
            Err(e) => Ok(e.kind),
        },
        Err(e) => Ok(e.kind),
    }
}

#[tauri::command]
pub async fn redis_scan_keys(
    id: String,
    r#match: Option<String>,
    cursor: Option<String>,
    count: Option<u64>,
) -> Result<ScanPage, String> {
    match do_read(
        &id,
        ReadOp::Scan {
            pattern: r#match.filter(|m| !m.is_empty()),
            cursor: cursor.unwrap_or_else(|| "0".to_string()),
            count: count.unwrap_or(200),
        },
    )
    .await?
    {
        ReadResult::Scan(page) => Ok(page),
        _ => Err("unexpected result".to_string()),
    }
}

#[tauri::command]
pub async fn redis_get_key(id: String, key: String) -> Result<RedisValue, String> {
    match do_read(&id, ReadOp::GetValue { key }).await? {
        ReadResult::Value(v) => Ok(v),
        _ => Err("unexpected result".to_string()),
    }
}

#[tauri::command]
pub async fn redis_key_info(id: String, key: String) -> Result<RedisKeyInfo, String> {
    match do_read(&id, ReadOp::KeyInfo { key }).await? {
        ReadResult::KeyInfo(info) => Ok(info),
        _ => Err("unexpected result".to_string()),
    }
}

#[tauri::command]
pub async fn redis_set_string(
    id: String,
    key: String,
    value: String,
    ttl_ms: Option<i64>,
) -> Result<(), String> {
    do_write(&id, WriteOp::SetString { key, value, ttl_ms }).await
}

#[tauri::command]
pub async fn redis_hash_set(
    id: String,
    key: String,
    field: String,
    value: String,
) -> Result<(), String> {
    do_write(&id, WriteOp::HashSet { key, field, value }).await
}

#[tauri::command]
pub async fn redis_hash_delete(id: String, key: String, field: String) -> Result<(), String> {
    do_write(&id, WriteOp::HashDel { key, field }).await
}

#[tauri::command]
pub async fn redis_list_push(
    id: String,
    key: String,
    value: String,
    front: bool,
) -> Result<(), String> {
    do_write(&id, WriteOp::ListPush { key, value, front }).await
}

#[tauri::command]
pub async fn redis_list_remove(
    id: String,
    key: String,
    value: String,
    count: i64,
) -> Result<(), String> {
    do_write(&id, WriteOp::ListRemove { key, value, count }).await
}

#[tauri::command]
pub async fn redis_set_add(id: String, key: String, member: String) -> Result<(), String> {
    do_write(&id, WriteOp::SetAdd { key, member }).await
}

#[tauri::command]
pub async fn redis_set_remove(id: String, key: String, member: String) -> Result<(), String> {
    do_write(&id, WriteOp::SetRemove { key, member }).await
}

#[tauri::command]
pub async fn redis_zset_add(
    id: String,
    key: String,
    member: String,
    score: f64,
) -> Result<(), String> {
    do_write(&id, WriteOp::ZAdd { key, member, score }).await
}

#[tauri::command]
pub async fn redis_zset_remove(id: String, key: String, member: String) -> Result<(), String> {
    do_write(&id, WriteOp::ZRemove { key, member }).await
}

#[tauri::command]
pub async fn redis_stream_add(
    id: String,
    key: String,
    entry_id: Option<String>,
    fields: Vec<(String, String)>,
) -> Result<(), String> {
    do_write(
        &id,
        WriteOp::StreamAdd {
            key,
            id: entry_id,
            fields,
        },
    )
    .await
}

#[tauri::command]
pub async fn redis_delete_key(id: String, key: String) -> Result<(), String> {
    do_write(&id, WriteOp::DeleteKey { key }).await
}

#[tauri::command]
pub async fn redis_expire(id: String, key: String, ttl_ms: Option<i64>) -> Result<(), String> {
    do_write(&id, WriteOp::Expire { key, ttl_ms }).await
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve(id: &str) -> Result<RedisConnection, String> {
    store::load(&store::redis_connections_path())
        .connections
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("no saved connection with id {id:?}"))
}

/// Resolve a profile to connect parameters, mapping failure onto a status kind
/// (so `redis_test_connection` can report it) — never leaking the string.
fn resolve_target(profile: &RedisConnection) -> Result<dsn::RedisTarget, RedisStatusKind> {
    let value =
        discover::read_value(&profile.secret).map_err(|_| RedisStatusKind::SecretUnresolved)?;
    if !value.is_usable() {
        return Err(RedisStatusKind::SecretUnresolved);
    }
    dsn::parse(value.as_written()).map_err(|_| RedisStatusKind::Unparseable)
}

async fn open(profile: &RedisConnection) -> Result<RedisClient, String> {
    let target = resolve_target(profile).map_err(status_message)?;
    RedisClient::connect(&target, DEFAULT_CONNECT_TIMEOUT)
        .await
        .map_err(driver_message)
}

async fn do_read(id: &str, op: ReadOp) -> Result<ReadResult, String> {
    let profile = resolve(id)?;
    let mut client = open(&profile).await?;
    run(client.read(op)).await.map_err(driver_message)
}

async fn do_write(id: &str, op: WriteOp) -> Result<(), String> {
    let profile = resolve(id)?;
    let mut client = open(&profile).await?;
    // The UI is always allowed to write; `allow_writes` gates only the agent
    // path. `plan_write(true, …)` mints the token the driver requires.
    let plan = ops::plan_write(true, op).expect("plan_write(true, ...) always grants");
    run(client.write(plan)).await.map_err(driver_message)
}

async fn run<T>(
    fut: impl std::future::Future<Output = Result<T, DriverError>>,
) -> Result<T, DriverError> {
    match tokio::time::timeout(OP_TIMEOUT, fut).await {
        Ok(result) => result,
        Err(_) => Err(DriverError {
            kind: RedisStatusKind::Timeout,
            message: "the command did not complete within its deadline".to_string(),
        }),
    }
}

fn driver_message(e: DriverError) -> String {
    // Already redacted on construction; redact again on the way out for the same
    // belt-and-braces reason the SQL bridge does.
    cb_core::sql::dsn::redact(&e.message)
}

fn status_message(kind: RedisStatusKind) -> String {
    match kind {
        RedisStatusKind::SecretUnresolved => {
            "the connection string could not be read from where the profile says it lives"
                .to_string()
        }
        RedisStatusKind::Unparseable => {
            "the stored connection string is not a valid Redis address".to_string()
        }
        _ => "the connection could not be opened".to_string(),
    }
}
