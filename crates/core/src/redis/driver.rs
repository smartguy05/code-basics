//! The one module in the `redis/` subtree that opens a socket.
//!
//! Everything above it — [`super::dsn`], [`super::store`], [`super::discover`],
//! [`super::ops`], [`super::model`] — is pure and tested with no server, exactly
//! as the SQL subsystem keeps its decisions above `sql::driver`. This layer turns
//! a parsed [`super::dsn::RedisTarget`] into a live connection and runs the
//! [`ReadOp`]/[`WritePlan`] operations against it.
//!
//! The external `redis` crate is aliased `rds` because this file lives in
//! `crate::redis`, which shadows the crate name.
//!
//! **No driver message crosses to an agent.** A failure becomes a
//! [`RedisStatusKind`] (a variant, no text) plus a message that has been through
//! [`crate::sql::dsn::redact`] for the human console — the same rule
//! `sql::driver::DriverError` follows.

use std::time::Duration;

use rds::AsyncCommands;
use redis as rds;

use super::dsn::RedisTarget;
use super::model::{
    HashField, RedisKeyInfo, RedisStatusKind, RedisType, RedisValue, ScanPage, StreamEntry,
    ZMember, MAX_COLLECTION_ELEMENTS, MAX_STRING_BYTES,
};
use super::ops::{ReadOp, WriteOp, WritePlan};

/// How long a connect may take before it is abandoned.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A failure, carrying a category (safe to show an agent) and a redacted message
/// (safe for the human console only).
#[derive(Debug, Clone)]
pub struct DriverError {
    pub kind: RedisStatusKind,
    pub message: String,
}

impl DriverError {
    /// Build an error, redacting the message on the way in — the one place a
    /// driver's words are scrubbed, mirroring `sql::driver::DriverError::new`.
    fn new(kind: RedisStatusKind, message: impl AsRef<str>) -> Self {
        Self {
            kind,
            message: crate::sql::dsn::redact(message.as_ref()),
        }
    }

    fn from_redis(error: &rds::RedisError) -> Self {
        let kind = classify(error);
        DriverError::new(kind, error.to_string())
    }
}

/// Map a redis-rs error onto a category. Abstains to `Failed` rather than
/// guessing, and never folds authentication into connectivity.
fn classify(error: &rds::RedisError) -> RedisStatusKind {
    use rds::ErrorKind::*;
    if error.is_io_error() {
        return RedisStatusKind::Unreachable;
    }
    match error.kind() {
        AuthenticationFailed => RedisStatusKind::AuthFailed,
        IoError => RedisStatusKind::Unreachable,
        _ => {
            let text = error.to_string().to_ascii_lowercase();
            if text.contains("tls") || text.contains("certificate") {
                RedisStatusKind::TlsFailed
            } else if text.contains("noauth") || text.contains("wrongpass") {
                RedisStatusKind::AuthFailed
            } else {
                RedisStatusKind::Failed
            }
        }
    }
}

/// A live connection to one Redis server.
pub struct RedisClient {
    conn: rds::aio::MultiplexedConnection,
}

impl RedisClient {
    /// Open a connection to `target`, or return why it could not open.
    pub async fn connect(target: &RedisTarget, timeout: Duration) -> Result<Self, DriverError> {
        let info = connection_info(target);
        let client = rds::Client::open(info).map_err(|e| DriverError::from_redis(&e))?;
        let fut = client.get_multiplexed_async_connection();
        let conn = match tokio::time::timeout(timeout, fut).await {
            Err(_elapsed) => {
                return Err(DriverError::new(
                    RedisStatusKind::Timeout,
                    "the connection did not open within its deadline",
                ))
            }
            Ok(Ok(conn)) => conn,
            Ok(Err(e)) => return Err(DriverError::from_redis(&e)),
        };
        Ok(Self { conn })
    }

    /// `PING`, for a status probe.
    pub async fn ping(&mut self) -> Result<(), DriverError> {
        rds::cmd("PING")
            .query_async::<()>(&mut self.conn)
            .await
            .map_err(|e| DriverError::from_redis(&e))
    }

    /// Run a read. Reads never consult write consent.
    pub async fn read(&mut self, op: ReadOp) -> Result<ReadResult, DriverError> {
        match op {
            ReadOp::Scan {
                pattern,
                cursor,
                count,
            } => Ok(ReadResult::Scan(self.scan(pattern, cursor, count).await?)),
            ReadOp::KeyInfo { key } => Ok(ReadResult::KeyInfo(self.key_info(&key).await?)),
            ReadOp::GetValue { key } => Ok(ReadResult::Value(self.get_value(&key).await?)),
        }
    }

    /// Run a write. The [`WritePlan`] token proves consent was checked upstream.
    pub async fn write(&mut self, plan: WritePlan) -> Result<(), DriverError> {
        let err = |e: rds::RedisError| DriverError::from_redis(&e);
        match plan.into_op() {
            WriteOp::SetString { key, value, ttl_ms } => match ttl_ms {
                Some(ms) if ms > 0 => rds::cmd("SET")
                    .arg(&key)
                    .arg(value)
                    .arg("PX")
                    .arg(ms)
                    .query_async::<()>(&mut self.conn)
                    .await
                    .map_err(err),
                _ => self.conn.set::<_, _, ()>(&key, value).await.map_err(err),
            },
            WriteOp::HashSet { key, field, value } => self
                .conn
                .hset::<_, _, _, ()>(&key, field, value)
                .await
                .map_err(err),
            WriteOp::HashDel { key, field } => {
                self.conn.hdel::<_, _, ()>(&key, field).await.map_err(err)
            }
            WriteOp::ListPush { key, value, front } => {
                if front {
                    self.conn.lpush::<_, _, ()>(&key, value).await.map_err(err)
                } else {
                    self.conn.rpush::<_, _, ()>(&key, value).await.map_err(err)
                }
            }
            WriteOp::ListRemove { key, value, count } => rds::cmd("LREM")
                .arg(&key)
                .arg(count)
                .arg(value)
                .query_async::<()>(&mut self.conn)
                .await
                .map_err(err),
            WriteOp::SetAdd { key, member } => {
                self.conn.sadd::<_, _, ()>(&key, member).await.map_err(err)
            }
            WriteOp::SetRemove { key, member } => {
                self.conn.srem::<_, _, ()>(&key, member).await.map_err(err)
            }
            WriteOp::ZAdd { key, member, score } => self
                .conn
                .zadd::<_, _, _, ()>(&key, member, score)
                .await
                .map_err(err),
            WriteOp::ZRemove { key, member } => {
                self.conn.zrem::<_, _, ()>(&key, member).await.map_err(err)
            }
            WriteOp::StreamAdd { key, id, fields } => {
                let mut cmd = rds::cmd("XADD");
                cmd.arg(&key).arg(id.as_deref().unwrap_or("*"));
                for (f, v) in &fields {
                    cmd.arg(f).arg(v);
                }
                cmd.query_async::<()>(&mut self.conn).await.map_err(err)
            }
            WriteOp::DeleteKey { key } => self.conn.del::<_, ()>(&key).await.map_err(err),
            WriteOp::Expire { key, ttl_ms } => match ttl_ms {
                Some(ms) if ms > 0 => rds::cmd("PEXPIRE")
                    .arg(&key)
                    .arg(ms)
                    .query_async::<()>(&mut self.conn)
                    .await
                    .map_err(err),
                _ => self.conn.persist::<_, ()>(&key).await.map_err(err),
            },
        }
    }

    async fn scan(
        &mut self,
        pattern: Option<String>,
        cursor: String,
        count: u64,
    ) -> Result<ScanPage, DriverError> {
        let mut cmd = rds::cmd("SCAN");
        cmd.arg(&cursor);
        if let Some(p) = &pattern {
            cmd.arg("MATCH").arg(p);
        }
        cmd.arg("COUNT").arg(count.max(1));
        let (next, keys): (String, Vec<String>) = cmd
            .query_async(&mut self.conn)
            .await
            .map_err(|e| DriverError::from_redis(&e))?;

        let mut infos = Vec::with_capacity(keys.len());
        for key in keys {
            infos.push(self.key_info(&key).await?);
        }
        Ok(ScanPage {
            complete: next == "0",
            cursor: next,
            keys: infos,
        })
    }

    async fn key_info(&mut self, key: &str) -> Result<RedisKeyInfo, DriverError> {
        let type_name: String = rds::cmd("TYPE")
            .arg(key)
            .query_async(&mut self.conn)
            .await
            .map_err(|e| DriverError::from_redis(&e))?;
        let pttl: i64 = rds::cmd("PTTL")
            .arg(key)
            .query_async(&mut self.conn)
            .await
            .map_err(|e| DriverError::from_redis(&e))?;
        Ok(RedisKeyInfo {
            key: key.to_string(),
            kind: RedisType::from_wire(&type_name),
            // -1 = no expiry, -2 = key missing; both mean "no TTL to show".
            ttl_ms: if pttl >= 0 { Some(pttl) } else { None },
        })
    }

    async fn get_value(&mut self, key: &str) -> Result<RedisValue, DriverError> {
        let type_name: String = rds::cmd("TYPE")
            .arg(key)
            .query_async(&mut self.conn)
            .await
            .map_err(|e| DriverError::from_redis(&e))?;
        let e = |err: rds::RedisError| DriverError::from_redis(&err);
        match RedisType::from_wire(&type_name) {
            RedisType::Unknown => Ok(RedisValue::None),
            RedisType::String => {
                let bytes: Vec<u8> = self.conn.get(key).await.map_err(e)?;
                let (text, truncated) = cap_string(&bytes);
                Ok(RedisValue::String { text, truncated })
            }
            RedisType::List => {
                let cap = MAX_COLLECTION_ELEMENTS as isize;
                let raw: Vec<Vec<u8>> = self.conn.lrange(key, 0, cap).await.map_err(e)?;
                let (items, truncated) = cap_list(raw);
                Ok(RedisValue::List { items, truncated })
            }
            RedisType::Set => {
                let raw: Vec<Vec<u8>> = self.conn.smembers(key).await.map_err(e)?;
                let (members, truncated) = cap_list(raw);
                Ok(RedisValue::Set { members, truncated })
            }
            RedisType::ZSet => {
                let raw: Vec<(Vec<u8>, f64)> = self
                    .conn
                    .zrange_withscores(key, 0, MAX_COLLECTION_ELEMENTS as isize)
                    .await
                    .map_err(e)?;
                let truncated = raw.len() > MAX_COLLECTION_ELEMENTS;
                let entries = raw
                    .into_iter()
                    .take(MAX_COLLECTION_ELEMENTS)
                    .map(|(m, s)| ZMember {
                        member: String::from_utf8_lossy(&m).into_owned(),
                        score: format_score(s),
                    })
                    .collect();
                Ok(RedisValue::ZSet { entries, truncated })
            }
            RedisType::Hash => {
                let raw: Vec<(String, Vec<u8>)> = self.conn.hgetall(key).await.map_err(e)?;
                let truncated = raw.len() > MAX_COLLECTION_ELEMENTS;
                let fields = raw
                    .into_iter()
                    .take(MAX_COLLECTION_ELEMENTS)
                    .map(|(f, v)| HashField {
                        field: f,
                        value: String::from_utf8_lossy(&v).into_owned(),
                    })
                    .collect();
                Ok(RedisValue::Hash { fields, truncated })
            }
            RedisType::Stream => {
                let reply: rds::streams::StreamRangeReply = self
                    .conn
                    .xrange_count(key, "-", "+", MAX_COLLECTION_ELEMENTS + 1)
                    .await
                    .map_err(e)?;
                let truncated = reply.ids.len() > MAX_COLLECTION_ELEMENTS;
                let entries = reply
                    .ids
                    .into_iter()
                    .take(MAX_COLLECTION_ELEMENTS)
                    .map(|entry| StreamEntry {
                        id: entry.id,
                        fields: entry
                            .map
                            .into_iter()
                            .map(|(f, v)| HashField {
                                field: f,
                                value: redis_value_to_string(&v),
                            })
                            .collect(),
                    })
                    .collect();
                Ok(RedisValue::Stream { entries, truncated })
            }
        }
    }
}

/// The typed result of a read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadResult {
    Scan(ScanPage),
    KeyInfo(RedisKeyInfo),
    Value(RedisValue),
}

fn connection_info(target: &RedisTarget) -> rds::ConnectionInfo {
    let addr = if target.use_tls {
        rds::ConnectionAddr::TcpTls {
            host: target.host.clone(),
            port: target.port,
            insecure: false,
            tls_params: None,
        }
    } else {
        rds::ConnectionAddr::Tcp(target.host.clone(), target.port)
    };
    rds::ConnectionInfo {
        addr,
        redis: rds::RedisConnectionInfo {
            db: target.db,
            username: target.username.clone(),
            password: target.password.clone(),
            protocol: rds::ProtocolVersion::default(),
        },
    }
}

fn cap_string(bytes: &[u8]) -> (String, bool) {
    if bytes.len() > MAX_STRING_BYTES {
        (
            String::from_utf8_lossy(&bytes[..MAX_STRING_BYTES]).into_owned(),
            true,
        )
    } else {
        (String::from_utf8_lossy(bytes).into_owned(), false)
    }
}

fn cap_list(raw: Vec<Vec<u8>>) -> (Vec<String>, bool) {
    let truncated = raw.len() > MAX_COLLECTION_ELEMENTS;
    let items = raw
        .into_iter()
        .take(MAX_COLLECTION_ELEMENTS)
        .map(|v| String::from_utf8_lossy(&v).into_owned())
        .collect();
    (items, truncated)
}

fn format_score(score: f64) -> String {
    // A whole score prints without a trailing `.0`, matching redis-cli.
    if score.fract() == 0.0 && score.is_finite() {
        format!("{}", score as i64)
    } else {
        format!("{score}")
    }
}

fn redis_value_to_string(value: &rds::Value) -> String {
    match value {
        rds::Value::BulkString(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        rds::Value::SimpleString(s) => s.clone(),
        rds::Value::Int(i) => i.to_string(),
        rds::Value::Nil => String::new(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
