//! Turning an answer into the text an agent reads. No decisions here beyond
//! phrasing; the runner and dispatch decide nothing about the words.

use crate::redis::model::{RedisStatusKind, RedisType, RedisValue, ScanPage};
use crate::redis::ops::WriteOp;

use super::expose::ExposedListing;

/// The connection listing.
pub fn connections(listed: &ExposedListing) -> String {
    let mut lines: Vec<String> = listed
        .connections
        .iter()
        .map(|c| format!("{}  {}", c.id, c.name))
        .collect();
    if listed.scoped_out > 0 {
        lines.push(format!(
            "({} more exposed connection(s) are out of this server's --workspace scope; name one \
             by its handle to reach it.)",
            listed.scoped_out
        ));
    }
    lines.join("\n")
}

/// One SCAN page.
pub fn scan_page(page: &ScanPage) -> String {
    let mut lines = Vec::with_capacity(page.keys.len() + 1);
    for key in &page.keys {
        let ttl = match key.ttl_ms {
            Some(ms) => format!("ttl {ms}ms"),
            None => "no ttl".to_string(),
        };
        lines.push(format!("{}  [{}]  {ttl}", key.key, type_name(key.kind)));
    }
    if page.complete {
        lines.push("(scan complete)".to_string());
    } else {
        lines.push(format!("(more — continue with cursor \"{}\")", page.cursor));
    }
    lines.join("\n")
}

/// A key's value.
pub fn value(value: &RedisValue) -> String {
    match value {
        RedisValue::None => "(the key does not exist)".to_string(),
        RedisValue::String { text, truncated } => with_cap(text.clone(), *truncated),
        RedisValue::List { items, truncated } => with_cap(numbered(items), *truncated),
        RedisValue::Set { members, truncated } => with_cap(members.join("\n"), *truncated),
        RedisValue::ZSet { entries, truncated } => with_cap(
            entries
                .iter()
                .map(|e| format!("{}  (score {})", e.member, e.score))
                .collect::<Vec<_>>()
                .join("\n"),
            *truncated,
        ),
        RedisValue::Hash { fields, truncated } => with_cap(
            fields
                .iter()
                .map(|f| format!("{} = {}", f.field, f.value))
                .collect::<Vec<_>>()
                .join("\n"),
            *truncated,
        ),
        RedisValue::Stream { entries, truncated } => with_cap(
            entries
                .iter()
                .map(|e| {
                    let fields = e
                        .fields
                        .iter()
                        .map(|f| format!("{}={}", f.field, f.value))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{}: {fields}", e.id)
                })
                .collect::<Vec<_>>()
                .join("\n"),
            *truncated,
        ),
    }
}

/// A connection status.
pub fn status(kind: RedisStatusKind) -> String {
    let name = match kind {
        RedisStatusKind::Ok => return "ok: the connection opened.".to_string(),
        RedisStatusKind::AuthFailed => "authFailed",
        RedisStatusKind::Unreachable => "unreachable",
        RedisStatusKind::TlsFailed => "tlsFailed",
        RedisStatusKind::Timeout => "timeout",
        RedisStatusKind::SecretUnresolved => "secretUnresolved",
        RedisStatusKind::Unparseable => "unparseable",
        RedisStatusKind::Failed => "failed",
    };
    format!("{name}: the connection did not open. The server's own message is not forwarded here.")
}

/// A write confirmation, naming what happened without echoing the value.
pub fn write_ok(op: &WriteOp) -> String {
    match op {
        WriteOp::SetString { key, .. } => format!("Set string key `{key}`."),
        WriteOp::HashSet { key, field, .. } => format!("Set field `{field}` of hash `{key}`."),
        WriteOp::HashDel { key, field } => format!("Deleted field `{field}` of hash `{key}`."),
        WriteOp::ListPush { key, front, .. } => {
            format!(
                "Pushed to the {} of list `{key}`.",
                if *front { "front" } else { "back" }
            )
        }
        WriteOp::ListRemove { key, .. } => format!("Removed from list `{key}`."),
        WriteOp::SetAdd { key, member } => format!("Added `{member}` to set `{key}`."),
        WriteOp::SetRemove { key, member } => format!("Removed `{member}` from set `{key}`."),
        WriteOp::ZAdd { key, member, score } => {
            format!("Added `{member}` (score {score}) to sorted set `{key}`.")
        }
        WriteOp::ZRemove { key, member } => format!("Removed `{member}` from sorted set `{key}`."),
        WriteOp::StreamAdd { key, .. } => format!("Appended an entry to stream `{key}`."),
        WriteOp::DeleteKey { key } => format!("Deleted key `{key}`."),
        WriteOp::Expire { key, ttl_ms } => match ttl_ms {
            Some(ms) => format!("Set key `{key}` to expire in {ms}ms."),
            None => format!("Cleared the TTL on key `{key}` (now persistent)."),
        },
    }
}

fn type_name(kind: RedisType) -> &'static str {
    match kind {
        RedisType::String => "string",
        RedisType::List => "list",
        RedisType::Set => "set",
        RedisType::ZSet => "zset",
        RedisType::Hash => "hash",
        RedisType::Stream => "stream",
        RedisType::Unknown => "unknown",
    }
}

fn numbered(items: &[String]) -> String {
    items
        .iter()
        .enumerate()
        .map(|(i, v)| format!("{i}) {v}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn with_cap(body: String, truncated: bool) -> String {
    if truncated {
        format!("{body}\n(more — this value was capped; not all of it is shown)")
    } else {
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redis::model::{HashField, RedisKeyInfo};

    #[test]
    fn a_capped_value_says_so() {
        let out = value(&RedisValue::String {
            text: "abc".to_string(),
            truncated: true,
        });
        assert!(out.contains("capped"), "{out}");
    }

    #[test]
    fn scan_page_reports_completion_and_continuation() {
        let complete = ScanPage {
            cursor: "0".to_string(),
            keys: vec![RedisKeyInfo {
                key: "k".to_string(),
                kind: RedisType::Hash,
                ttl_ms: Some(1000),
            }],
            complete: true,
        };
        assert!(scan_page(&complete).contains("scan complete"));

        let more = ScanPage {
            cursor: "42".to_string(),
            keys: vec![],
            complete: false,
        };
        assert!(scan_page(&more).contains("42"));
    }

    #[test]
    fn a_write_confirmation_names_the_action_without_the_value() {
        let out = write_ok(&WriteOp::SetString {
            key: "k".to_string(),
            value: "secret-value".to_string(),
            ttl_ms: None,
        });
        assert!(out.contains("`k`"));
        assert!(
            !out.contains("secret-value"),
            "the value must not be echoed: {out}"
        );
    }

    #[test]
    fn a_hash_value_pairs_fields() {
        let out = value(&RedisValue::Hash {
            fields: vec![HashField {
                field: "a".to_string(),
                value: "1".to_string(),
            }],
            truncated: false,
        });
        assert!(out.contains("a = 1"), "{out}");
    }
}
