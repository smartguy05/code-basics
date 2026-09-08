//! Turning an answer into the words an agent reads.
//!
//! Two rules govern everything here, and both are about a reader that will act
//! on the text without seeing the panel it came from.
//!
//! # A cap is rendered first, not last
//!
//! [`crate::sql::model::RowCap`]'s presence *is* the report. A model that reads
//! a hundred rows and only afterwards a footnote saying there were more has
//! already drawn its conclusion — so the caveat leads. And
//! [`crate::sql::model::RowCapReason::RowLimit`] and
//! [`crate::sql::model::RowCapReason::ByteLimit`] render differently, because
//! raising the row limit would not return more rows in the second case.
//!
//! The mirror of that rule is the one implementations get wrong: an **uncapped
//! empty** result is not a cap and not a failure. It says the query matched no
//! rows and that this is the complete answer, or the reader will assume the
//! result was withheld.
//!
//! # The honesty surface
//!
//! [`enforcement`] is what `sql.read_only_enforcement` returns, and it is why
//! that tool exists. For SQL Server a write **executes and is rolled back**:
//! [`crate::sql::driver::EnforcementStrength`] deliberately refuses to rank
//! [`Refused`](crate::sql::driver::EnforcementStrength::Refused) against
//! [`Undone`](crate::sql::driver::EnforcementStrength::Undone), and an agent
//! that reads "read-only" as "cannot write" is being told something untrue. So
//! the phrase *"before it runs"* is reserved for the two mechanisms that really
//! refuse a write, and a test asserts it never appears in the SQL Server
//! rendering. Folding this into every other tool's output would bury it, which
//! is why it is a tool of its own.

use serde_json::{json, Map, Value};

use super::answer::ConnectionStatusKind;
use super::expose::ExposedListing;
use crate::sql::catalog::{SqlColumn as CatalogColumn, SqlObject};
use crate::sql::driver::{EnforcementStrength, ReadOnlyEnforcement};
use crate::sql::model::{RowCap, RowCapReason, SqlResultSet, SqlValue};
use crate::sql::plan::engine_name;

/// The sentence an uncapped empty result carries.
///
/// Its own constant so the distinction cannot be lost to a refactor: this is
/// *the query matched nothing*, which is an answer, not *nothing came back*,
/// which would be a failure.
pub const NO_ROWS: &str =
    "The query ran and matched no rows. This is the complete answer, not a truncated one.";

/// The line a capped result leads with.
pub fn cap_notice(cap: RowCap) -> String {
    match cap.reason {
        RowCapReason::RowLimit => format!(
            "INCOMPLETE: this result stopped at the row ceiling of {} rows. More rows match than \
             are shown; narrow the query or aggregate rather than treating this as the whole \
             answer.",
            cap.limit
        ),
        RowCapReason::ByteLimit => format!(
            "INCOMPLETE: this result stopped at a total-bytes ceiling after {} rows. Raising a \
             row limit would not return more rows — the rows are wide, not many. Select fewer \
             columns, or narrow the query.",
            cap.limit
        ),
    }
}

/// One result set, as an agent reads it.
///
/// The cap notice leads when there is one; the rows follow as JSON so a model
/// can parse them without a table-layout heuristic.
pub fn result(set: &SqlResultSet) -> String {
    let mut out = String::new();
    if let Some(cap) = set.row_cap {
        out.push_str(&cap_notice(cap));
        out.push_str("\n\n");
    }

    if set.rows.is_empty() && set.row_cap.is_none() {
        out.push_str(NO_ROWS);
        if let Some(affected) = set.rows_affected {
            out.push_str(&format!(" Rows affected: {affected}."));
        }
        return out;
    }

    out.push_str(&format!(
        "{} row{} in {} ms.\n",
        set.rows.len(),
        if set.rows.len() == 1 { "" } else { "s" },
        set.elapsed_ms
    ));
    let names: Vec<&str> = set.columns.iter().map(|c| c.name.as_str()).collect();
    for row in &set.rows {
        let mut object = Map::new();
        for (index, cell) in row.iter().enumerate() {
            let name = names
                .get(index)
                .map(|n| (*n).to_string())
                .unwrap_or_else(|| format!("column{index}"));
            object.insert(name, value(cell));
        }
        out.push_str(&Value::Object(object).to_string());
        out.push('\n');
    }
    out
}

/// One cell.
///
/// A **number arrives as a JSON string**, deliberately: a JSON number cannot
/// hold `NUMERIC(38,10)` without losing digits, and the driver has already
/// formatted it. The two admissions of ignorance —
/// [`SqlValue::Unsupported`] and [`SqlValue::Unavailable`] — become objects
/// rather than `null`, because "the database said there is no value" and "this
/// build could not read the value" are different facts and `null` is already
/// taken by the first.
pub fn value(cell: &SqlValue) -> Value {
    match cell {
        SqlValue::Null => Value::Null,
        SqlValue::Text { text, truncated } => {
            if *truncated {
                json!({ "text": text, "truncated": true })
            } else {
                Value::String(text.clone())
            }
        }
        SqlValue::Number { text } => Value::String(text.clone()),
        SqlValue::Bool { value } => Value::Bool(*value),
        SqlValue::Bytes {
            hex,
            byte_length,
            truncated,
        } => json!({ "hex": hex, "byteLength": byte_length, "truncated": truncated }),
        SqlValue::Unsupported { type_name } => {
            json!({ "unreadable": "this build has no representation for the column's type",
                    "typeName": type_name })
        }
        // The reason is **dropped, not filtered**, which is this module's rule
        // everywhere else and was the one place it leaked. A `Cell::Error`
        // reason is built straight from a `sqlx` error (`postgres.rs` and
        // `sqlite.rs` both do `e.to_string()` in `decode_cell`), and a driver
        // error routinely embeds the DSN — which is exactly what
        // `dsn::redact` is a *heuristic* against. A connect failure crosses as
        // a variant and a statement failure as a stage; a cell failure now
        // crosses as nothing but the admission itself.
        //
        // The cell still says it is unreadable, so the agent is never shown a
        // value that is not there — it simply is not told the driver's words.
        SqlValue::Unavailable { .. } => {
            json!({ "unreadable": "the driver could not decode this cell" })
        }
    }
}

/// What is enforcing read-only, said plainly. See the module docs.
pub fn enforcement(mechanism: ReadOnlyEnforcement) -> String {
    format!(
        "{}\n\nWhat that means: {}\n\nStrength: {}",
        mechanism.label(),
        mechanism.detail(),
        strength(mechanism.strength())
    )
}

/// The strength, in words. The phrase "before it runs" belongs to
/// [`EnforcementStrength::Refused`] and to nothing else.
pub fn strength(level: EnforcementStrength) -> &'static str {
    match level {
        EnforcementStrength::Refused => "the write is refused before it runs",
        EnforcementStrength::Undone => {
            "the write executes and is then undone — it is not stopped, and anything it does \
             outside the transaction is not undone either"
        }
        EnforcementStrength::HeuristicOnly => {
            "only a text heuristic stands in the way; nothing on the connection is enforcing \
             anything"
        }
        EnforcementStrength::Nothing => "nothing at all is enforcing read-only",
        EnforcementStrength::Unknown => {
            "it is not known which mechanism, if any, applies; nothing here may be read as a \
             guarantee"
        }
    }
}

/// The exposed connections, and what a workspace scope left out.
pub fn connections(listed: &ExposedListing) -> String {
    let mut out = String::new();
    for connection in &listed.connections {
        out.push_str(&format!(
            "{}  {}  ({})\n",
            connection.id,
            connection.name,
            connection
                .engine
                .map(engine_name)
                .unwrap_or("engine not determined")
        ));
    }
    if listed.scoped_out > 0 {
        out.push_str(&format!(
            "\n{} further exposed connection{} {} outside this server's --workspace scope and {} \
             not listed. That flag scopes this listing only; it is not a security boundary, and \
             those connections can still be named by id.",
            listed.scoped_out,
            if listed.scoped_out == 1 { "" } else { "s" },
            if listed.scoped_out == 1 { "is" } else { "are" },
            if listed.scoped_out == 1 { "is" } else { "are" },
        ));
    }
    out
}

/// The tables a catalog listing found.
pub fn tables(objects: &[SqlObject]) -> String {
    if objects.is_empty() {
        return "The catalog returned no tables. This is the complete answer for this connection."
            .to_string();
    }
    objects
        .iter()
        .map(|object| match &object.schema {
            Some(schema) => format!("{schema}.{}", object.name),
            None => object.name.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One table's columns.
///
/// Every optional fact is rendered as *not reported* rather than as a default,
/// because [`crate::sql::catalog::SqlColumn`]'s optionals mean the engine did
/// not say, not that the answer is no.
pub fn columns(rows: &[CatalogColumn]) -> String {
    if rows.is_empty() {
        return "The catalog returned no columns for that table. It may not exist, or the login \
                may not be able to see it — this build cannot tell those apart."
            .to_string();
    }
    let mut out = String::new();
    for column in rows {
        let mut facts = vec![format!("ordinal {}", column.ordinal)];
        facts.push(match column.nullable {
            Some(true) => "nullable".to_string(),
            Some(false) => "not null".to_string(),
            None => "nullability not reported".to_string(),
        });
        if let Some(pk) = column.primary_key {
            facts.push(if pk {
                "primary key".to_string()
            } else {
                "not a primary key".to_string()
            });
        }
        if let Some(length) = column.max_length {
            facts.push(format!("max length {length}"));
        }
        if let (Some(precision), Some(scale)) = (column.numeric_precision, column.numeric_scale) {
            facts.push(format!("numeric({precision},{scale})"));
        }
        if let Some(default) = &column.default_value {
            facts.push(format!("default {default}"));
        }
        out.push_str(&format!(
            "{}  {}  [{}]\n",
            column.name,
            column.data_type,
            facts.join(", ")
        ));
    }
    out
}

/// A connection test's outcome, with no driver text in it.
pub fn status(kind: ConnectionStatusKind) -> String {
    format!("{}: {}", kind.name(), kind.detail())
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
