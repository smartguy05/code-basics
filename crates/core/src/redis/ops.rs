//! The Redis operations, split read from write, and the **one** place the
//! write-consent rule lives.
//!
//! Redis has no server-side read-only mode, so `allow_writes` at this planning
//! tier is the *whole* of the write gate — there is nothing behind it to catch a
//! mistake. The rule is made structural rather than conventional: a [`WriteOp`]
//! can only be run once it has become a [`WritePlan`], and the **only** way to
//! mint a `WritePlan` is [`plan_write`], which refuses unless writes are allowed.
//! A read path never receives a `WritePlan` and so has no value it could pass to
//! reach a write — the same shape as [`crate::mcp::execute::agent_plan`] having
//! no `writes_allowed` parameter at all.

/// A read. Reads never consult `allow_writes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOp {
    /// One `SCAN` page: an optional `MATCH` glob, the cursor, and a `COUNT` hint.
    Scan {
        pattern: Option<String>,
        cursor: String,
        count: u64,
    },
    /// A key's `TYPE` and TTL.
    KeyInfo { key: String },
    /// A key's value, read the way its type requires.
    GetValue { key: String },
}

/// A write. Cannot be executed until [`plan_write`] turns it into a [`WritePlan`].
///
/// No `Eq`: `ZAdd` carries an `f64` score, which is `PartialEq` but not `Eq`.
#[derive(Debug, Clone, PartialEq)]
pub enum WriteOp {
    SetString {
        key: String,
        value: String,
        ttl_ms: Option<i64>,
    },
    HashSet {
        key: String,
        field: String,
        value: String,
    },
    HashDel {
        key: String,
        field: String,
    },
    ListPush {
        key: String,
        value: String,
        /// `true` = LPUSH (front), `false` = RPUSH (back).
        front: bool,
    },
    ListRemove {
        key: String,
        value: String,
        /// LREM count: 0 removes all matching.
        count: i64,
    },
    SetAdd {
        key: String,
        member: String,
    },
    SetRemove {
        key: String,
        member: String,
    },
    ZAdd {
        key: String,
        member: String,
        score: f64,
    },
    ZRemove {
        key: String,
        member: String,
    },
    StreamAdd {
        key: String,
        /// `None` lets Redis assign the id (`*`).
        id: Option<String>,
        fields: Vec<(String, String)>,
    },
    DeleteKey {
        key: String,
    },
    /// Set or clear a key's TTL. `None` = `PERSIST`.
    Expire {
        key: String,
        ttl_ms: Option<i64>,
    },
}

/// A write that has passed the consent gate. Its field is **private**, so the
/// only way to hold one is through [`plan_write`] — that is what makes the gate
/// structural rather than a check a caller could forget.
#[derive(Debug, Clone, PartialEq)]
pub struct WritePlan {
    op: WriteOp,
}

impl WritePlan {
    /// The op to run. Consumes the plan, so it cannot be run twice by accident.
    pub fn into_op(self) -> WriteOp {
        self.op
    }

    /// The op, borrowed (for rendering the answer).
    pub fn op(&self) -> &WriteOp {
        &self.op
    }
}

/// Why a write was refused at the planning tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The connection does not have `allow_writes` granted.
    WritesNotAllowed,
}

impl Refusal {
    pub fn code(self) -> &'static str {
        match self {
            Refusal::WritesNotAllowed => "writesNotAllowed",
        }
    }

    pub fn sentence(self) -> String {
        match self {
            Refusal::WritesNotAllowed => "This connection is read-only for agents. Writing needs \
                'allow writes' granted for it in the app's Redis panel, which is a separate consent \
                from exposure and is off by default; nothing here can grant it."
                .to_string(),
        }
    }
}

/// The one gate: a `WriteOp` becomes a runnable `WritePlan` only when writes are
/// allowed. Reads do not call this.
pub fn plan_write(allow_writes: bool, op: WriteOp) -> Result<WritePlan, Refusal> {
    if allow_writes {
        Ok(WritePlan { op })
    } else {
        Err(Refusal::WritesNotAllowed)
    }
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
