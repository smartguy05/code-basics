//! The wire model the Redis panel and the MCP server share.
//!
//! Every type is `camelCase` with its keys pinned by a test at the bottom, the
//! same IPC-contract discipline as [`crate::sql::model`]. Values are carried as
//! text (lossy for non-UTF-8 bytes) in this first cut; a huge collection or a
//! long string is **capped and the cap reported** (`truncated`), never silently
//! shortened.

use serde::{Deserialize, Serialize};
use specta::Type;

/// The Redis type of a key, as `TYPE` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RedisType {
    #[serde(rename = "string")]
    String,
    #[serde(rename = "list")]
    List,
    #[serde(rename = "set")]
    Set,
    #[serde(rename = "zset")]
    ZSet,
    #[serde(rename = "hash")]
    Hash,
    #[serde(rename = "stream")]
    Stream,
    /// `TYPE` returned something this build does not model, or the key is gone.
    #[serde(rename = "unknown")]
    Unknown,
}

impl RedisType {
    /// Map the string `TYPE` returns onto a variant, abstaining to `Unknown`.
    pub fn from_wire(s: &str) -> Self {
        match s {
            "string" => RedisType::String,
            "list" => RedisType::List,
            "set" => RedisType::Set,
            "zset" => RedisType::ZSet,
            "hash" => RedisType::Hash,
            "stream" => RedisType::Stream,
            _ => RedisType::Unknown,
        }
    }
}

/// One key's identity: its name, its type, and its TTL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RedisKeyInfo {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: RedisType,
    /// Milliseconds until expiry. `None` means **no expiry** (a persistent key),
    /// which is a different fact from "expires soon" and so is not `0`.
    pub ttl_ms: Option<i64>,
}

/// One member of a sorted set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ZMember {
    pub member: String,
    /// The score, carried as text so a large or fractional score is not reshaped
    /// by JSON's number handling.
    pub score: String,
}

/// One field of a hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HashField {
    pub field: String,
    pub value: String,
}

/// One entry of a stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StreamEntry {
    pub id: String,
    pub fields: Vec<HashField>,
}

/// A key's value, by type. `truncated` on a collection/string means a cap bit —
/// there is more than is shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RedisValue {
    String {
        text: String,
        truncated: bool,
    },
    List {
        items: Vec<String>,
        truncated: bool,
    },
    Set {
        members: Vec<String>,
        truncated: bool,
    },
    ZSet {
        entries: Vec<ZMember>,
        truncated: bool,
    },
    Hash {
        fields: Vec<HashField>,
        truncated: bool,
    },
    Stream {
        entries: Vec<StreamEntry>,
        truncated: bool,
    },
    /// The key does not exist.
    None,
}

/// One page of a `SCAN`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanPage {
    /// The cursor to pass to the next `SCAN`. `"0"` when the scan is complete.
    pub cursor: String,
    pub keys: Vec<RedisKeyInfo>,
    /// Whether this was the last page (`cursor == "0"`).
    pub complete: bool,
}

/// Whether a connection opens, as a category with no driver message — the Redis
/// counterpart of [`crate::mcp::answer::ConnectionStatusKind`], kept small.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RedisStatusKind {
    Ok,
    AuthFailed,
    Unreachable,
    TlsFailed,
    Timeout,
    /// The connection string could not be re-read from where the profile says it
    /// lives. Nothing was attempted.
    SecretUnresolved,
    /// The string could not be parsed as a Redis connection.
    Unparseable,
    /// It failed and this build has no rule for the reason.
    Failed,
}

/// How many collection elements a single read returns before the cap bites.
pub const MAX_COLLECTION_ELEMENTS: usize = 1000;
/// How many bytes of a string value are returned before the cap bites.
pub const MAX_STRING_BYTES: usize = 64 * 1024;

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
