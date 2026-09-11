//! Turning a parsed tool call into something runnable — and the consent gate.
//!
//! [`plan`] is where reads and writes part: a read becomes a [`ReadOp`] with no
//! reference to consent; a write goes through [`crate::redis::ops::plan_write`],
//! which mints a [`WritePlan`] only when `allow_writes` is set. There is no
//! parameter by which a read path could request a write, so the gate cannot be
//! bypassed by an argument. Exposure (whether the connection is reachable at all)
//! is enforced separately by [`super::expose::find_exposed`], one layer out.

use crate::redis::dsn::{self, RedisTarget};
use crate::redis::ops::{self, ReadOp, WriteOp, WritePlan};
use crate::redis::store::RedisConnection;

use super::answer::McpRefusal;
use super::tools::ToolCall;

/// The default `SCAN COUNT` when a call names none.
const DEFAULT_SCAN_COUNT: u64 = 200;

/// What a connection-scoped tool call resolves to once consent is applied.
#[derive(Debug, Clone, PartialEq)]
pub enum Planned {
    Read(ReadOp),
    Write(WritePlan),
    /// `redis.connection_status` — answered by probing the connection, not by a
    /// read op.
    ConnectionStatus,
}

/// Plan a connection-scoped call. **Not** for `ListConnections`, which names no
/// connection and is answered before a profile is resolved.
///
/// A write is refused here (`WritesNotAllowed`) unless `allow_writes` is set —
/// the whole write gate, since Redis has no server-side read-only mode.
pub fn plan(allow_writes: bool, call: ToolCall) -> Result<Planned, McpRefusal> {
    match call {
        ToolCall::ListConnections => {
            // The runner handles this before resolving a profile; reaching here
            // is a programming error, answered as a refusal rather than a panic.
            Err(McpRefusal::ConnectionNotFound {
                selector: "(list_connections)".to_string(),
            })
        }
        ToolCall::ScanKeys {
            pattern,
            cursor,
            count,
            ..
        } => Ok(Planned::Read(ReadOp::Scan {
            pattern,
            cursor: cursor.unwrap_or_else(|| "0".to_string()),
            count: count.unwrap_or(DEFAULT_SCAN_COUNT),
        })),
        ToolCall::GetKey { key, .. } => Ok(Planned::Read(ReadOp::GetValue { key })),
        ToolCall::ConnectionStatus { .. } => Ok(Planned::ConnectionStatus),
        ToolCall::SetString {
            key, value, ttl_ms, ..
        } => write(allow_writes, WriteOp::SetString { key, value, ttl_ms }),
        ToolCall::HashSet {
            key, field, value, ..
        } => write(allow_writes, WriteOp::HashSet { key, field, value }),
        ToolCall::ListPush {
            key, value, front, ..
        } => write(allow_writes, WriteOp::ListPush { key, value, front }),
        ToolCall::SetAdd { key, member, .. } => {
            write(allow_writes, WriteOp::SetAdd { key, member })
        }
        ToolCall::ZSetAdd {
            key, member, score, ..
        } => write(allow_writes, WriteOp::ZAdd { key, member, score }),
        ToolCall::StreamAdd {
            key, id, fields, ..
        } => write(allow_writes, WriteOp::StreamAdd { key, id, fields }),
        ToolCall::DeleteKey { key, .. } => write(allow_writes, WriteOp::DeleteKey { key }),
        ToolCall::Expire { key, ttl_ms, .. } => {
            write(allow_writes, WriteOp::Expire { key, ttl_ms })
        }
    }
}

fn write(allow_writes: bool, op: WriteOp) -> Result<Planned, McpRefusal> {
    ops::plan_write(allow_writes, op)
        .map(Planned::Write)
        .map_err(|_| McpRefusal::WritesNotAllowed)
}

/// Re-read a profile's connection string and parse it into connect parameters,
/// or the refusal that says why it could not be.
pub fn resolve_target(profile: &RedisConnection) -> Result<RedisTarget, McpRefusal> {
    let value = crate::redis::discover::read_value(&profile.secret).map_err(|_| {
        McpRefusal::SecretUnresolved {
            connection: profile.id.clone(),
        }
    })?;
    if !value.is_usable() {
        return Err(McpRefusal::SecretUnresolved {
            connection: profile.id.clone(),
        });
    }
    dsn::parse(value.as_written()).map_err(|_| McpRefusal::Unparseable {
        connection: profile.id.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_is_refused_without_allow_writes() {
        let call = ToolCall::DeleteKey {
            connection: "c".into(),
            key: "k".into(),
        };
        assert_eq!(plan(false, call), Err(McpRefusal::WritesNotAllowed));
    }

    #[test]
    fn a_write_is_planned_with_allow_writes() {
        let call = ToolCall::SetString {
            connection: "c".into(),
            key: "k".into(),
            value: "v".into(),
            ttl_ms: None,
        };
        assert!(matches!(plan(true, call), Ok(Planned::Write(_))));
    }

    #[test]
    fn a_read_never_consults_consent() {
        let call = ToolCall::GetKey {
            connection: "c".into(),
            key: "k".into(),
        };
        // Same answer whether or not writes are allowed.
        assert!(matches!(plan(false, call.clone()), Ok(Planned::Read(_))));
        assert!(matches!(plan(true, call), Ok(Planned::Read(_))));
    }

    #[test]
    fn scan_defaults_cursor_and_count() {
        let call = ToolCall::ScanKeys {
            connection: "c".into(),
            pattern: None,
            cursor: None,
            count: None,
        };
        match plan(false, call).unwrap() {
            Planned::Read(ReadOp::Scan { cursor, count, .. }) => {
                assert_eq!(cursor, "0");
                assert_eq!(count, DEFAULT_SCAN_COUNT);
            }
            other => panic!("wrong plan: {other:?}"),
        }
    }
}
