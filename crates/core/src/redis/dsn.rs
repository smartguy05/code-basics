//! Parsing and redacting a Redis connection string.
//!
//! Two shapes are recognised, and both abstain rather than guess:
//!
//! - a URL — `redis://`, `rediss://` (TLS), `redis+sentinel://` — with optional
//!   `user:password@`, `:port` and `/db`;
//! - the StackExchange.Redis host-list form — `host:6379,password=…,ssl=true,
//!   defaultDatabase=0` — comma-separated, the first token an endpoint.
//!
//! The **only** form allowed to leave this module is [`RedisConnectionDisplay`],
//! which never carries a password, exactly as [`crate::sql::dsn::SqlConnectionDisplay`]
//! is the only view SQL lets across IPC. The password-bearing [`RedisTarget`] is
//! used solely by the driver at connect time and is never serialised. Every error
//! string a caller forwards must go through [`crate::sql::dsn::redact`] first —
//! that keyword heuristic already covers `password=` and URL userinfo, so it is
//! reused rather than reimplemented.

use serde::{Deserialize, Serialize};
use specta::Type;

/// The connect parameters parsed from a connection string. **Carries the
/// password** and so is never serialised or logged — only the driver reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisTarget {
    pub host: String,
    pub port: u16,
    /// The logical database index. Redis defaults to 0.
    pub db: i64,
    pub use_tls: bool,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// The default Redis port when a connection string names none.
pub const DEFAULT_PORT: u16 = 6379;

/// A redacted view of a Redis connection, safe to cross IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RedisConnectionDisplay {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub db: Option<i64>,
    pub uses_tls: bool,
    /// Whether a password is present — never the password itself.
    pub has_password: bool,
}

/// Whether `s` looks like a Redis connection string.
///
/// Conservative on purpose (the abstain rule): a URL with a redis scheme, or the
/// StackExchange host-list form carrying a Redis-specific option, is a yes;
/// anything else is a no, so a SQL connection string (`;`-separated, `Server=`)
/// is never mistaken for one. Discovery also keys off the config key *name*, so a
/// `redis://` value is not the only way a connection is found.
pub fn looks_like_redis(s: &str) -> bool {
    let trimmed = s.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("redis://")
        || lower.starts_with("rediss://")
        || lower.starts_with("redis+sentinel://")
    {
        return true;
    }
    // SQL connection strings use `;` between options; a `;` rules Redis out.
    if trimmed.contains(';') {
        return false;
    }
    // StackExchange.Redis host-list form: `host:port,opt=val,...`. Require a
    // Redis-specific option so a bare `host:port` (which could be anything) is not
    // claimed.
    let has_endpoint = trimmed
        .split(',')
        .next()
        .map(|first| first.contains(':') && !first.contains('='))
        .unwrap_or(false);
    let has_redis_option = trimmed.split(',').skip(1).any(|opt| {
        let key = opt
            .split('=')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        matches!(
            key.as_str(),
            "ssl" | "abortconnect" | "defaultdatabase" | "allowadmin" | "password" | "user"
        )
    });
    has_endpoint && has_redis_option
}

/// Parse a connection string into connect parameters, or a reason it could not
/// be understood. The reason never quotes the input (which contains the
/// password).
pub fn parse(s: &str) -> Result<RedisTarget, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("the connection string is empty".to_string());
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("redis://")
        || lower.starts_with("rediss://")
        || lower.starts_with("redis+sentinel://")
    {
        parse_url(trimmed)
    } else if trimmed.contains(';') {
        Err("this looks like a SQL connection string, not a Redis one".to_string())
    } else {
        parse_host_list(trimmed)
    }
}

/// The redacted display of a connection string. Unparseable input still yields a
/// display (all-absent) rather than an error — a picker row must render.
pub fn display_form(s: &str) -> RedisConnectionDisplay {
    match parse(s) {
        Ok(target) => RedisConnectionDisplay {
            host: Some(target.host),
            port: Some(target.port),
            db: Some(target.db),
            uses_tls: target.use_tls,
            has_password: target.password.is_some(),
        },
        Err(_) => RedisConnectionDisplay {
            host: None,
            port: None,
            db: None,
            uses_tls: false,
            has_password: false,
        },
    }
}

fn parse_url(s: &str) -> Result<RedisTarget, String> {
    let use_tls = s.to_ascii_lowercase().starts_with("rediss://");
    let after_scheme = s
        .splitn(2, "://")
        .nth(1)
        .ok_or_else(|| "malformed URL".to_string())?;

    // Split authority from the path (the db index).
    let (authority, path) = match after_scheme.find('/') {
        Some(i) => (&after_scheme[..i], &after_scheme[i + 1..]),
        None => (after_scheme, ""),
    };

    // Userinfo, if any.
    let (userinfo, hostport) = match authority.rfind('@') {
        Some(i) => (Some(&authority[..i]), &authority[i + 1..]),
        None => (None, authority),
    };
    let (username, password) = match userinfo {
        None => (None, None),
        Some(info) => match info.find(':') {
            Some(i) => (non_empty(&info[..i]), Some(percent_decode(&info[i + 1..]))),
            None => (non_empty(info), None),
        },
    };

    let (host, port) = split_host_port(hostport)?;
    let db = if path.trim().is_empty() {
        0
    } else {
        path.trim()
            .parse::<i64>()
            .map_err(|_| "the database index in the URL path is not a number".to_string())?
    };

    Ok(RedisTarget {
        host,
        port,
        db,
        use_tls,
        username,
        password,
    })
}

fn parse_host_list(s: &str) -> Result<RedisTarget, String> {
    let mut parts = s.split(',');
    let endpoint = parts
        .next()
        .ok_or_else(|| "no endpoint in the connection string".to_string())?;
    let (host, port) = split_host_port(endpoint.trim())?;

    let mut use_tls = false;
    let mut db = 0i64;
    let mut password = None;
    let mut username = None;
    for opt in parts {
        let mut kv = opt.splitn(2, '=');
        let key = kv.next().unwrap_or("").trim().to_ascii_lowercase();
        let value = kv.next().unwrap_or("").trim();
        match key.as_str() {
            "ssl" => use_tls = value.eq_ignore_ascii_case("true"),
            "defaultdatabase" => db = value.parse::<i64>().unwrap_or(0),
            "password" => password = non_empty(value).map(|v| v.to_string()),
            "user" => username = non_empty(value),
            _ => {}
        }
    }

    Ok(RedisTarget {
        host,
        port,
        db,
        use_tls,
        username,
        password,
    })
}

fn split_host_port(hostport: &str) -> Result<(String, u16), String> {
    if hostport.is_empty() {
        return Err("no host in the connection string".to_string());
    }
    match hostport.rfind(':') {
        Some(i) => {
            let host = &hostport[..i];
            let port = hostport[i + 1..]
                .parse::<u16>()
                .map_err(|_| "the port is not a number".to_string())?;
            if host.is_empty() {
                return Err("no host before the port".to_string());
            }
            Ok((host.to_string(), port))
        }
        None => Ok((hostport.to_string(), DEFAULT_PORT)),
    }
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// Minimal percent-decoding for a URL password (`%40` → `@`, etc.). Redis
/// passwords with reserved characters are percent-encoded in a URL.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
#[path = "dsn_tests.rs"]
mod tests;
