//! The object explorer's catalog queries, and the rows they come back as.
//!
//! Moved out of the SQL console's Tauri bridge for the two reasons in
//! [`super::plan`]: a command body must not decide anything, and the MCP server
//! is a separate process with no `AppState` and so cannot call one. The wire
//! types live here now and are re-exported from the bridge under their old
//! names, so the JSON that crosses IPC is byte-identical.
//!
//! # Identifiers: refused, not sanitised — and this is not parameter binding
//!
//! Every arm below interpolates a schema or table name into a **string-literal**
//! position (`table_name = '{table}'`, `pragma_table_info('{table}')`), so
//! doubling `'` into `''` is the correct escape *for a literal* and
//! [`sql_literal`] does it. That is the second layer, and it is not enough on
//! its own:
//!
//! - PostgreSQL with `standard_conforming_strings = off`, or an `E''` string,
//!   treats a backslash as an escape, so `\'` defeats the doubling.
//! - A NUL can truncate the statement inside a client library, after this
//!   crate's arithmetic has already decided the string is safe.
//! - These names now arrive from an **agent** as well as from a click on a row
//!   this app itself read out of the catalog.
//!
//! So [`identifier`] refuses anything carrying those hazards outright, naming
//! which rule it broke, and the doubling stays underneath it. **Refusing is not
//! binding.** The durable fix is a `run_statement_with_params` on
//! [`super::driver::SqlConnection`], which today takes no parameters at all —
//! it is recorded in `.memories/features/mcp-sql-server/todos.md`, and nothing
//! here should be read as claiming validation is equivalent to it.

use super::dsn::SqlEngine;
use super::model::{SqlResultSet, SqlValue};
use serde::Serialize;
use specta::Type;

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

/// A database object category. The wire shape is deliberately broader than
/// today's explorer so procedures, views, and functions can be added later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SqlObjectKind {
    Table,
}

/// One entry in the object explorer. The schema remains separate because
/// engines quote and qualify identifiers differently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SqlObject {
    pub kind: SqlObjectKind,
    pub schema: Option<String>,
    pub name: String,
}

/// Normalized column metadata. Optional fields mean the engine did not report
/// that fact, not that the fact is false.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SqlColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: Option<bool>,
    pub default_value: Option<String>,
    pub ordinal: u32,
    pub max_length: Option<u64>,
    pub numeric_precision: Option<u64>,
    pub numeric_scale: Option<u64>,
    pub primary_key: Option<bool>,
}

// ---------------------------------------------------------------------------
// Identifier acceptance
// ---------------------------------------------------------------------------

/// The longest identifier this app will interpolate.
///
/// Comfortably above every shipped engine's own limit (PostgreSQL 63 bytes,
/// SQL Server 128 characters), so a name a database could really hold is never
/// refused for its length alone.
pub const MAX_IDENTIFIER_BYTES: usize = 128;

/// Which rule an identifier broke.
///
/// One variant per rule rather than a single "invalid identifier", for the
/// reason this crate keeps every other abstention apart: a name refused for its
/// length and a name refused for carrying a quote lead the reader to two
/// different actions, and only one of them looks like an attack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentifierRefusal {
    /// No name at all. An absent schema is spelled [`None`], not `""`.
    Empty,
    /// Longer than [`MAX_IDENTIFIER_BYTES`].
    TooLong { bytes: usize },
    /// A NUL, which can truncate the statement inside a client library long
    /// after this crate decided the text was safe.
    Nul,
    /// A single quote, which is what the literal position is delimited by.
    Quote,
    /// A backslash, which escapes the quote doubling under PostgreSQL's
    /// `standard_conforming_strings = off` and in an `E''` string.
    Backslash,
    /// A newline or carriage return.
    Newline,
    /// Any other control character.
    Control { code: u32 },
}

impl IdentifierRefusal {
    /// The sentence this refusal is described by, wherever it is described.
    pub fn sentence(&self) -> String {
        match self {
            IdentifierRefusal::Empty => {
                "This name is empty, so there is no object to look up.".to_string()
            }
            IdentifierRefusal::TooLong { bytes } => format!(
                "This name is {bytes} bytes long; the longest this app will look up is \
                 {MAX_IDENTIFIER_BYTES}."
            ),
            IdentifierRefusal::Nul => "This name contains a NUL byte, which can truncate the \
                 statement inside the database client. It is refused rather than cleaned."
                .to_string(),
            IdentifierRefusal::Quote => "This name contains a single quote. The catalog query \
                 puts it in a string literal, so it is refused rather than escaped."
                .to_string(),
            IdentifierRefusal::Backslash => "This name contains a backslash, which escapes the \
                 quote doubling on some PostgreSQL settings. It is refused rather than cleaned."
                .to_string(),
            IdentifierRefusal::Newline => "This name contains a line break, which cannot appear \
                 in an identifier the catalog would return."
                .to_string(),
            IdentifierRefusal::Control { code } => format!(
                "This name contains the control character U+{code:04X}, which cannot appear in \
                 an identifier the catalog would return."
            ),
        }
    }
}

/// Accept a schema or table name for interpolation, or say which rule it broke.
///
/// The **first** of the two layers described in the module docs. It refuses
/// rather than repairing: a name this app cannot look up safely is not turned
/// into a different name that it can, because the row that would come back
/// would then be about some other object.
pub fn identifier(name: &str) -> Result<&str, IdentifierRefusal> {
    if name.is_empty() {
        return Err(IdentifierRefusal::Empty);
    }
    if name.len() > MAX_IDENTIFIER_BYTES {
        return Err(IdentifierRefusal::TooLong { bytes: name.len() });
    }
    for ch in name.chars() {
        match ch {
            '\0' => return Err(IdentifierRefusal::Nul),
            '\'' => return Err(IdentifierRefusal::Quote),
            '\\' => return Err(IdentifierRefusal::Backslash),
            '\n' | '\r' => return Err(IdentifierRefusal::Newline),
            _ if ch.is_control() => return Err(IdentifierRefusal::Control { code: ch as u32 }),
            _ => {}
        }
    }
    Ok(name)
}

// ---------------------------------------------------------------------------
// The queries
// ---------------------------------------------------------------------------

/// The statement that lists an engine's tables.
pub fn object_catalog_query(engine: SqlEngine) -> &'static str {
    match engine {
        SqlEngine::Postgres => {
            "SELECT table_schema, table_name FROM information_schema.tables \
             WHERE table_type = 'BASE TABLE' \
             AND table_schema NOT IN ('pg_catalog', 'information_schema') \
             ORDER BY table_schema, table_name"
        }
        SqlEngine::SqlServer => {
            "SELECT TABLE_SCHEMA, TABLE_NAME FROM INFORMATION_SCHEMA.TABLES \
             WHERE TABLE_TYPE = 'BASE TABLE' ORDER BY TABLE_SCHEMA, TABLE_NAME"
        }
        SqlEngine::Sqlite => {
            "SELECT 'main' AS table_schema, name AS table_name FROM sqlite_schema \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
        }
    }
}

/// Double a single quote for a SQL **string literal** position.
///
/// The second layer only. See the module docs for why it is not sufficient on
/// its own and what [`identifier`] refuses ahead of it.
pub fn sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

/// The statement that describes one table's columns.
///
/// Callers must put `schema` and `table` through [`identifier`] first; this
/// escapes but does not judge.
pub fn column_catalog_query(engine: SqlEngine, schema: Option<&str>, table: &str) -> String {
    let table = sql_literal(table);
    // An omitted schema means **every** schema, which is the absence of a
    // predicate — not `= ''`. Folding `None` into the empty string asked for the
    // schema literally named "", which no server has, so the answer came back
    // empty and the renderer reported the table as possibly absent. The tool
    // description promises the model "omit it to search every schema", and this
    // is the line that has to keep that promise.
    let schema_predicate = |column: &str| match schema {
        Some(schema) => format!("{column} = '{}' AND ", sql_literal(schema)),
        None => String::new(),
    };
    match engine {
        SqlEngine::Postgres => format!(
            "SELECT column_name, data_type, is_nullable, column_default, \
             ordinal_position::text, COALESCE(character_maximum_length::text, ''), \
             COALESCE(numeric_precision::text, ''), COALESCE(numeric_scale::text, ''), '' \
             FROM information_schema.columns WHERE {}table_name = '{table}' \
             ORDER BY ordinal_position",
            schema_predicate("table_schema")
        ),
        SqlEngine::SqlServer => format!(
            "SELECT COLUMN_NAME, DATA_TYPE, IS_NULLABLE, COLUMN_DEFAULT, \
             CAST(ORDINAL_POSITION AS varchar(20)), COALESCE(CAST(CHARACTER_MAXIMUM_LENGTH AS varchar(20)), ''), \
             COALESCE(CAST(NUMERIC_PRECISION AS varchar(20)), ''), COALESCE(CAST(NUMERIC_SCALE AS varchar(20)), ''), '' \
             FROM INFORMATION_SCHEMA.COLUMNS WHERE {}TABLE_NAME = '{table}' \
             ORDER BY ORDINAL_POSITION",
            schema_predicate("TABLE_SCHEMA")
        ),
        // `notnull` is **quoted**, and must stay quoted: SQLite parses a bare
        // `notnull` as its postfix NOTNULL operator, so `notnull = 0` is a
        // syntax error rather than a comparison and the whole listing fails at
        // execute. Pinned by
        // `the_sqlite_column_catalog_query_is_valid_sql_and_describes_the_table`
        // in `tests/sql_sqlite.rs`: a unit test over the built string cannot
        // catch it, because the text itself is perfectly well formed.
        SqlEngine::Sqlite => format!(
            "SELECT name, type, CASE WHEN \"notnull\" = 0 THEN 'YES' ELSE 'NO' END, dflt_value, \
             CAST(cid + 1 AS TEXT), '', '', '', CASE WHEN pk = 0 THEN 'NO' ELSE 'YES' END \
             FROM pragma_table_info('{table}') ORDER BY cid"
        ),
    }
}

// ---------------------------------------------------------------------------
// Reading the rows back
// ---------------------------------------------------------------------------

fn object_text(value: &SqlValue) -> Option<&str> {
    match value {
        SqlValue::Text {
            text,
            truncated: false,
        } => Some(text),
        _ => None,
    }
}

/// The tables in a catalog result.
///
/// A capped result is an **error**, not a short list: the explorer would render
/// it as the whole schema.
pub fn table_objects(result: &SqlResultSet) -> Result<Vec<SqlObject>, String> {
    if result.row_cap.is_some() {
        return Err("the table catalog exceeded the explorer's result limit".to_string());
    }
    result
        .rows
        .iter()
        .map(|row| {
            let schema = row.first().and_then(object_text).filter(|s| !s.is_empty());
            let name = row
                .get(1)
                .and_then(object_text)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "the database returned a table without a name".to_string())?;
            Ok(SqlObject {
                kind: SqlObjectKind::Table,
                schema: schema.map(str::to_string),
                name: name.to_string(),
            })
        })
        .collect()
}

fn optional_number(value: Option<&SqlValue>) -> Option<u64> {
    value
        .and_then(object_text)
        .filter(|text| !text.is_empty())
        .and_then(|text| text.parse().ok())
}

fn yes_no(value: Option<&SqlValue>) -> Option<bool> {
    match value
        .and_then(object_text)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("yes" | "true" | "1") => Some(true),
        Some("no" | "false" | "0") => Some(false),
        _ => None,
    }
}

/// The columns in a column-catalog result. A capped result is an error, for the
/// same reason as [`table_objects`].
pub fn table_columns(result: &SqlResultSet) -> Result<Vec<SqlColumn>, String> {
    if result.row_cap.is_some() {
        return Err("the column catalog exceeded the explorer's result limit".to_string());
    }
    result
        .rows
        .iter()
        .map(|row| {
            let required = |index, label| {
                row.get(index)
                    .and_then(object_text)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| format!("the database returned a column without {label}"))
            };
            Ok(SqlColumn {
                name: required(0, "a name")?.to_string(),
                data_type: required(1, "a type")?.to_string(),
                nullable: yes_no(row.get(2)),
                default_value: row
                    .get(3)
                    .and_then(object_text)
                    .filter(|text| !text.is_empty())
                    .map(str::to_string),
                ordinal: required(4, "an ordinal position")?
                    .parse()
                    .map_err(|_| "the database returned an invalid column position".to_string())?,
                max_length: optional_number(row.get(5)),
                numeric_precision: optional_number(row.get(6)),
                numeric_scale: optional_number(row.get(7)),
                primary_key: yes_no(row.get(8)),
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
