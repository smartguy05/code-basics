//! Wire types for the SQL console: the request to run a query, the result set
//! that comes back, and the enumerations describing why a query was refused.
//!
//! Like `crate::model`, everything here is `serde(rename_all = "camelCase")` and
//! its exact JSON keys are pinned by a test, because `src/ipc/types.ts` mirrors
//! these by hand. Nothing in this module carries a connection string: a secret
//! travels in one direction, on the request, and never appears in a response.
//!
//! # Four rules that are easy to "tidy" away, and must not be
//!
//! **SQL `NULL`, an empty string and a truncated value are three different JSON
//! shapes.** `{"kind":"null"}`, `{"kind":"text","text":"","truncated":false}`
//! and the same object with `"truncated":true` are three answers to three
//! different questions — *there is no value here*, *there is a value and it is
//! empty*, and *there is a value and you are not looking at all of it*. A grid
//! that renders the first two identically has told the reader something untrue
//! about their data, and one that renders the third as though it were complete
//! has done worse. This is [`crate::inspect::model::ObjectValue`]'s
//! `Null`/`Text`/`Elided` split, applied to a cell.
//!
//! **Numbers cross as strings.** [`SqlValue::Number`] carries `text`, and it is
//! a `String` deliberately. `NUMERIC(38,10)`, `bigint`, `money` and SQL Server's
//! `decimal` all exceed what an IEEE-754 double represents exactly, so a JSON
//! number would arrive in the frontend already rounded — and a rounded value in
//! a console is not a slightly-wrong pixel, it is a wrong answer rendered
//! confidently, in the one place a user goes to find out what the data actually
//! says. It is the argument [`crate::inspect::model`] makes for heap addresses
//! crossing as hex, with the same conclusion: do not bet correctness on a value
//! that happens to fit today. Nothing here parses the text — it is whatever the
//! driver formatted, passed through.
//!
//! **A cap is reported, never silently applied.** `row_cap: None` means *you
//! have every row*; `Some(..)` names the limit and why it bit. The
//! [`crate::inspect::model::ObjectValue::Elided`] precedent: a quietly shorter
//! list is indistinguishable from a correct one.
//!
//! **No `skip_serializing_if`, anywhere in this file**, for the reason
//! [`crate::lsp::model`] gives — every key is always present and an absent value
//! crosses as an explicit `null`, so "the backend has no answer" is never
//! indistinguishable from "the backend forgot to send one". It bites hardest on
//! [`SqlResultSet::rows_affected`], where `Some(0)` ("it ran and matched
//! nothing") and `None` ("this statement has no row count to report") are
//! opposite facts that a skipped key collapses into one.

use serde::{Deserialize, Serialize};
use specta::Type;

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// One cell of one row.
///
/// The last three variants are the ones that admit ignorance, and they are
/// three rather than one on purpose: a value the driver said was absent, a type
/// this build cannot decode, and a cell the driver errored on are different
/// things to tell somebody, and only the first is a fact about the data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SqlValue {
    /// SQL `NULL`. Not an empty string, and not a missing cell.
    Null,
    /// Character data. `truncated` is set when a length cap cut it, and is
    /// **always present** — an empty string that was never truncated and one
    /// that was cut to nothing must not read the same.
    Text {
        text: String,
        truncated: bool,
    },
    /// A number, **already formatted by the driver and carried as a string**.
    /// See the module docs: a JSON number cannot hold `NUMERIC(38,10)`.
    Number {
        text: String,
    },
    Bool {
        value: bool,
    },
    /// Binary data as lowercase hex. `byte_length` is the length of the
    /// *original* value, so a truncated blob still reports how big it really
    /// was, rather than how much of it survived.
    Bytes {
        hex: String,
        byte_length: u64,
        truncated: bool,
    },
    /// This build has no representation for the column's type. `type_name` is
    /// the server's own name for it, so the user can tell what was skipped —
    /// never a placeholder value that would read as data.
    Unsupported {
        type_name: String,
    },
    /// The driver failed on this specific cell while the rest of the row read
    /// fine. A sentence for a human, and never built from the connection
    /// string (see the subsystem docs).
    Unavailable {
        reason: String,
    },
}

impl SqlValue {
    /// Whether this cell is a value the user can act on, as opposed to one of
    /// the two admissions of ignorance.
    ///
    /// `Null` counts as known: "there is no value here" is an answer the
    /// database gave, not one this application failed to obtain.
    pub fn is_known(&self) -> bool {
        !matches!(
            self,
            SqlValue::Unsupported { .. } | SqlValue::Unavailable { .. }
        )
    }
}

// ---------------------------------------------------------------------------
// Result sets
// ---------------------------------------------------------------------------

/// One column of a result set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SqlColumn {
    pub name: String,
    /// The server's own type name, when the driver reported one. `None` means
    /// *not reported*, never "it has no type" — it crosses as an explicit
    /// `null`.
    pub type_name: Option<String>,
}

/// Why a result set stopped short of every row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RowCapReason {
    /// A row count ceiling was reached.
    RowLimit,
    /// A total-bytes ceiling was reached first — which is why this is not the
    /// same answer as [`RowCapReason::RowLimit`]: raising the row limit would
    /// not return more rows.
    ByteLimit,
}

/// A cap that was applied to a result set.
///
/// Its presence *is* the report. Nothing anywhere may return a shortened
/// [`SqlResultSet`] with `row_cap: None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RowCap {
    /// The ceiling that bit, in rows — the number of rows actually delivered.
    pub limit: u32,
    pub reason: RowCapReason,
}

/// What one statement produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SqlResultSet {
    pub columns: Vec<SqlColumn>,
    /// Row-major, each inner vector the same length as `columns`.
    pub rows: Vec<Vec<SqlValue>>,
    /// `None` means **every row is here**. `Some(..)` names the cap that bit.
    pub row_cap: Option<RowCap>,
    /// `Some(0)` — it ran and matched nothing — and `None` — this statement has
    /// no row count to report — are different answers and must stay distinct.
    /// Hence no `skip_serializing_if`.
    pub rows_affected: Option<u64>,
    /// Wall-clock time for this statement, as measured by the caller.
    pub elapsed_ms: u64,
    /// Which statement of the submitted script this is, 0-based. Present on
    /// every result and on most events so a multi-statement script's output can
    /// be reassembled without relying on arrival order.
    pub statement_index: usize,
}

impl SqlResultSet {
    /// Whether the caller is looking at the complete answer.
    pub fn is_complete(&self) -> bool {
        self.row_cap.is_none()
    }
}

/// The end of one statement: everything about it that only became known when
/// the rows ran out.
///
/// **It carries no rows, deliberately.** Rows arrive in [`SqlEvent::Rows`],
/// batched and streamed as they are read, and that stream is the authoritative
/// copy — the one the grid is built from. Repeating the whole result set in the
/// completion event sent every row across IPC twice and held two copies of it
/// on each side, which for a wide thousand-row grid is the largest allocation
/// this subsystem makes, doubled, for no reader.
///
/// What is left is what the stream could not say: the cap that bit, the row
/// count to check the stream against, the affected count, and the elapsed time.
/// `row_count` is a count and not the rows, so a consumer that has dropped rows
/// can still tell that it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SqlCompletion {
    /// Which statement of the submitted script this is, 0-based.
    pub statement_index: usize,
    /// How many rows were delivered, across every [`SqlEvent::Rows`] batch.
    pub row_count: u64,
    /// `None` means **every row is here**. `Some(..)` names the cap that bit.
    pub row_cap: Option<RowCap>,
    /// `Some(0)` — it ran and matched nothing — and `None` — this statement has
    /// no row count to report — are different answers and must stay distinct.
    pub rows_affected: Option<u64>,
    pub elapsed_ms: u64,
}

impl SqlResultSet {
    /// The completion event's payload for this result set. The one place a
    /// completion is derived, so it cannot drift from the result it describes.
    pub fn completion(&self) -> SqlCompletion {
        SqlCompletion {
            statement_index: self.statement_index,
            row_count: self.rows.len() as u64,
            row_cap: self.row_cap,
            rows_affected: self.rows_affected,
            elapsed_ms: self.elapsed_ms,
        }
    }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// What the backend streams while a script runs.
///
/// Modelled on [`crate::process::ProcessEvent`]: rows arrive as they are read, so
/// a long query fills the grid instead of blocking on the whole answer.
///
/// The three ways a run can stop are three variants, never one. A statement
/// **refused** by the read-only guard never reached the database at all; a
/// statement that **failed** did reach it, or the connection did not open; and a
/// [`SqlEvent::Finished`] carrying `cancelled: true` means the user stopped a
/// run that was otherwise fine. Collapsing any two of these tells the user the
/// database rejected something it never saw, or that their own cancellation was
/// an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SqlEvent {
    /// A statement is about to be sent.
    Started { statement_index: usize },
    /// The columns are known, before any row has been read.
    Columns {
        statement_index: usize,
        columns: Vec<SqlColumn>,
    },
    /// A batch of rows, in order.
    Rows {
        statement_index: usize,
        rows: Vec<Vec<SqlValue>>,
    },
    /// One statement finished. Carries a [`SqlCompletion`] and **not** the
    /// result set: the rows already crossed, once, in [`SqlEvent::Rows`], and
    /// sending them again doubled the payload of every query.
    Completed { completion: SqlCompletion },
    /// Something the reader should know about how this statement is being run,
    /// which is neither a refusal nor a failure — an *allowed* write, for
    /// instance, which the guard still names as a write.
    ///
    /// Its own variant on purpose: a note delivered as a
    /// [`SqlEvent::Failed`] would say the statement did not run, and one
    /// delivered as a [`SqlEvent::Refused`] would say it never reached the
    /// database. Both are untrue of a statement that is about to run.
    Notice {
        statement_index: usize,
        message: String,
    },
    /// The read-only guard refused this statement, so **nothing was sent to the
    /// database**. `reason` is the guard's own sentence, which names the
    /// classification as a text heuristic — see the subsystem docs: the user
    /// must not read this as "the database is protected".
    Refused {
        statement_index: usize,
        reason: String,
    },
    /// Something went wrong. `statement_index` is `None` when the failure
    /// happened before any statement ran — a connection that could not open —
    /// which is a different situation from a statement erroring, so it crosses
    /// as an explicit `null` rather than being skipped or defaulted to `0`.
    ///
    /// `message` is built from the redacted description in
    /// [`super::dsn`], never from the connection string.
    Failed {
        statement_index: Option<usize>,
        message: String,
    },
    /// The run is over and no further events will arrive. `cancelled` is true
    /// only when the user stopped it; a run that ended in
    /// [`SqlEvent::Failed`] or [`SqlEvent::Refused`] still finishes with
    /// `cancelled: false`.
    Finished { cancelled: bool },
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod model_tests;
