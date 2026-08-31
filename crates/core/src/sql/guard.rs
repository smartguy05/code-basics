//! The read-only guard: parse a statement with `sqlparser` and decide whether it
//! is a read.
//!
//! This is a **text heuristic**, not a permission model, and every refusal says
//! so in its own message. It abstains by default — a statement is refused unless
//! it is positively recognised as a read — and it keeps its outcomes distinct:
//! allowed, a recognised write, unparseable, and parsed-but-unrecognised are
//! separate answers that must never collapse into one.
//!
//! The shape of the module follows from that last sentence. [`Verdict`] has
//! **three** variants, not two: "this is a write" and "I could not tell" are
//! different facts about the world, and folding the second into the first makes
//! a gap in the classifier look like a deliberate decision — which is how a bug
//! here stops being reported. Everything the guard cannot positively recognise
//! lands in [`Verdict::Refused`] carrying a [`RefusalReason`] that says which
//! kind of not-knowing it was.
//!
//! The one thing that is *not* modelled at all is what a called routine does. A
//! stored procedure body is unknowable from here, so the denylist below is
//! deliberately short and honest rather than exhaustive: the real weight is
//! carried by connecting with a login that cannot write.

use sqlparser::ast::{Query, SetExpr, Statement, UtilityOption};
use sqlparser::dialect::{Dialect, MsSqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, Tokenizer};

/// The engine whose dialect the SQL is parsed with.
///
/// Declared here rather than taken from `sql::model`, which does not yet carry
/// an engine type; it is expected to move there once it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    SqlServer,
    Postgres,
    Sqlite,
}

impl Engine {
    fn dialect(self) -> Box<dyn Dialect> {
        match self {
            Engine::SqlServer => Box::new(MsSqlDialect {}),
            Engine::Postgres => Box::new(PostgreSqlDialect {}),
            Engine::Sqlite => Box::new(SQLiteDialect {}),
        }
    }
}

/// Why the guard would not vouch for a batch.
///
/// Each variant is a *different* kind of not-knowing, kept apart on purpose: an
/// unparseable batch is a bug report waiting to happen, an unrecognised
/// statement is a classifier gap, and a denylisted function is a recognised
/// escape hatch. Reporting all three as "blocked" would hide the first two
/// forever.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefusalReason {
    /// Nothing to run: empty, whitespace, or only comments and semicolons.
    Empty,
    /// A client-side batch separator (`GO`) that is not SQL at all.
    BatchSeparator { keyword: String },
    /// `sqlparser` could not parse the batch. This is what makes the
    /// comment-hides-a-keyword, keyword-in-a-string-literal and
    /// dollar-quoted-body classes safe: a text scan guesses, a parser refuses.
    ParseError { message: String },
    /// The batch parsed into a statement this guard does not classify. **Not**
    /// the same as a write, and never reported as one.
    Unrecognised { statement: String },
    /// A function that reaches outside the query — the filesystem, another
    /// server, or SQL built at runtime that this guard never sees.
    DenylistedFunction { name: String },
    /// The parse said "read", but the text contains a keyword a read cannot
    /// contain — so the two readings disagree and the guard abstains.
    HiddenKeyword { keyword: String },
}

/// What the guard concluded about a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Positively recognised as a read.
    ReadOnly,
    /// Positively recognised as a write, named by kind.
    Write { kind: &'static str },
    /// Not classified. Distinct from [`Verdict::Write`], and unlike a write it
    /// is **never** lifted by the writes-allowed setting: permission to write is
    /// not permission to run something nobody classified.
    Refused { reason: RefusalReason },
}

/// A [`Verdict`] resolved against the connection's writes-allowed setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub verdict: Verdict,
    /// Whether the batch may be sent to the server.
    pub allowed: bool,
    /// Present for anything that is not a plain read — including an *allowed*
    /// write, which still says what it is.
    pub message: Option<String>,
}

/// The sentence every refusal carries.
///
/// Pinned by `a_refusal_says_it_is_a_text_heuristic_and_not_a_sandbox`, which
/// also asserts the wording never acquires "sandboxed", "protected" or "is
/// safe". The user must not read a refusal as proof that the database itself
/// would have stopped the statement — it would not have.
pub const HEURISTIC_NOTE: &str = "This is a heuristic over the SQL text, not a database-enforced sandbox, \
and it can be wrong in both directions. To run this anyway, either enable writes for this connection, \
or connect with a login that has no write permission and let the server decide.";

/// Functions that reach outside the query, or that run SQL this guard never
/// sees. Deliberately short: an exhaustive list is not achievable, and
/// pretending otherwise is worse than being clear about the limit.
const DENYLIST: &[&str] = &[
    "xp_cmdshell",
    "sp_executesql",
    "OPENROWSET",
    "OPENDATASOURCE",
    "pg_read_file",
    "pg_ls_dir",
    "lo_import",
    "readfile",
    "writefile",
];

/// Keywords that cannot occur as bare words in a statement that is genuinely a
/// read. Checked only when the parse already said "read", and only for plain
/// queries — see the call site in [`classify`].
const READ_INCOMPATIBLE: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "MERGE", "DROP", "TRUNCATE", "ALTER", "GRANT", "REVOKE",
    "CREATE", "EXEC", "EXECUTE", "CALL", "ATTACH", "VACUUM",
];

/// Classify a batch of SQL, without reference to any setting.
pub fn classify(sql: &str, engine: Engine) -> Verdict {
    if let Some(keyword) = batch_separator(sql) {
        return Verdict::Refused {
            reason: RefusalReason::BatchSeparator { keyword },
        };
    }
    if sql.trim().is_empty() {
        return Verdict::Refused {
            reason: RefusalReason::Empty,
        };
    }

    let dialect = engine.dialect();
    let statements = match Parser::parse_sql(dialect.as_ref(), sql) {
        Ok(statements) => statements,
        Err(err) => {
            return Verdict::Refused {
                reason: RefusalReason::ParseError {
                    message: err.to_string(),
                },
            }
        }
    };
    if statements.is_empty() {
        return Verdict::Refused {
            reason: RefusalReason::Empty,
        };
    }

    // A denylisted name outranks everything else: it is the one case where the
    // guard knows that what it is looking at cannot be classified at all.
    let words = bare_words(sql, dialect.as_ref());
    if let Some(name) = denylisted_call(sql, dialect.as_ref()) {
        return Verdict::Refused {
            reason: RefusalReason::DenylistedFunction {
                name: name.to_string(),
            },
        };
    }

    let mut strictest = Verdict::ReadOnly;
    let mut all_plain_queries = true;
    for statement in &statements {
        if !matches!(statement, Statement::Query(_)) {
            all_plain_queries = false;
        }
        strictest = stricter(strictest, classify_statement(statement));
    }

    // The AST walk does not descend into every nested position a query can hide
    // a data-modifying CTE in, so when it reports a read, cross-check the text.
    // A disagreement is abstained on, never resolved. Only plain queries are
    // checked: `EXPLAIN INSERT ...` is a read whose text legitimately contains a
    // write keyword.
    if strictest == Verdict::ReadOnly && all_plain_queries {
        if let Some(keyword) = hidden_keyword(&words) {
            return Verdict::Refused {
                reason: RefusalReason::HiddenKeyword { keyword },
            };
        }
    }

    strictest
}

/// Classify a batch and resolve it against the connection's writes-allowed
/// setting.
pub fn guard(sql: &str, engine: Engine, writes_allowed: bool) -> Decision {
    let verdict = classify(sql, engine);
    let allowed = match &verdict {
        Verdict::ReadOnly => true,
        Verdict::Write { .. } => writes_allowed,
        Verdict::Refused { .. } => false,
    };
    let message = describe(&verdict, allowed);
    Decision {
        verdict,
        allowed,
        message,
    }
}

fn describe(verdict: &Verdict, allowed: bool) -> Option<String> {
    match verdict {
        Verdict::ReadOnly => None,
        Verdict::Write { kind } if allowed => Some(format!(
            "Running a write ({kind}) — writes are enabled for this connection."
        )),
        Verdict::Write { kind } => Some(format!(
            "This looks like a write ({kind}), and writes are off for this connection. {HEURISTIC_NOTE}"
        )),
        Verdict::Refused { reason } => Some(format!("{} {HEURISTIC_NOTE}", reason_line(reason))),
    }
}

fn reason_line(reason: &RefusalReason) -> String {
    match reason {
        RefusalReason::Empty => "There is no statement to run.".to_string(),
        RefusalReason::BatchSeparator { keyword } => format!(
            "`{keyword}` is a client-side batch separator rather than SQL, and this console sends \
one batch, so run the parts separately."
        ),
        RefusalReason::ParseError { message } => {
            format!("This could not be parsed, so it was not classified: {message}.")
        }
        RefusalReason::Unrecognised { statement } => {
            format!("`{statement}` is not a statement this check recognises as a read.")
        }
        RefusalReason::DenylistedFunction { name } => format!(
            "`{name}` reaches outside the query, so what it does cannot be read off the SQL."
        ),
        RefusalReason::HiddenKeyword { keyword } => format!(
            "This parsed as a read but contains `{keyword}`, which a read cannot contain, so the \
two readings disagree."
        ),
    }
}

/// Refused outranks Write outranks ReadOnly — a batch is as strict as its
/// strictest statement, and a refusal is stricter than a write because a write
/// can be permitted and a refusal cannot.
fn stricter(a: Verdict, b: Verdict) -> Verdict {
    fn rank(v: &Verdict) -> u8 {
        match v {
            Verdict::ReadOnly => 0,
            Verdict::Write { .. } => 1,
            Verdict::Refused { .. } => 2,
        }
    }
    if rank(&b) > rank(&a) {
        b
    } else {
        a
    }
}

fn classify_statement(statement: &Statement) -> Verdict {
    let kind: &'static str = match statement {
        Statement::Query(query) => match query_write_kind(query) {
            Some(kind) => kind,
            None => return Verdict::ReadOnly,
        },

        // `EXPLAIN` plans without running — except `EXPLAIN ANALYZE`, which
        // actually executes the statement it is explaining. That is the single
        // most-missed case in a hand-rolled guard, and it hides in *two* places
        // in the AST: see [`explain_effect`].
        Statement::Explain {
            analyze, options, ..
        } => match explain_effect(*analyze, options.as_deref()) {
            ExplainEffect::PlanOnly => return Verdict::ReadOnly,
            ExplainEffect::Executes => "EXPLAIN ANALYZE",
            ExplainEffect::Unknown { option } => {
                return Verdict::Refused {
                    reason: RefusalReason::Unrecognised {
                        statement: format!("EXPLAIN ({option})"),
                    },
                }
            }
        },
        Statement::ExplainTable { .. } => return Verdict::ReadOnly,

        // A `PRAGMA` with a value sets it; without one it reads it.
        Statement::Pragma { value: Some(_), .. } => "PRAGMA",
        Statement::Pragma { value: None, .. } => return Verdict::ReadOnly,

        Statement::Insert(_) => "INSERT",
        Statement::Update(_) => "UPDATE",
        Statement::Delete(_) => "DELETE",
        Statement::Merge(_) => "MERGE",
        Statement::Truncate(_) => "TRUNCATE",

        Statement::CreateTable(_) | Statement::CreateVirtualTable { .. } => "CREATE TABLE",
        Statement::CreateView(_) => "CREATE VIEW",
        Statement::CreateIndex(_)
        | Statement::CreateRole(_)
        | Statement::CreateSecret { .. }
        | Statement::CreateServer(_)
        | Statement::CreatePolicy(_)
        | Statement::CreateConnector(_)
        | Statement::CreateOperator(_)
        | Statement::CreateOperatorFamily(_)
        | Statement::CreateOperatorClass(_)
        | Statement::CreateExtension(_)
        | Statement::CreateCollation(_)
        | Statement::CreateSchema { .. }
        | Statement::CreateDatabase { .. }
        | Statement::CreateFunction(_)
        | Statement::CreateTrigger(_)
        | Statement::CreateProcedure { .. }
        | Statement::CreateMacro { .. }
        | Statement::CreateStage { .. }
        | Statement::CreateSequence { .. }
        | Statement::CreateDomain(_)
        | Statement::CreateType { .. }
        | Statement::CreateUser(_) => "CREATE",

        Statement::AlterTable(_) => "ALTER TABLE",
        Statement::AlterSchema(_)
        | Statement::AlterIndex { .. }
        | Statement::AlterView { .. }
        | Statement::AlterFunction(_)
        | Statement::AlterType(_)
        | Statement::AlterCollation(_)
        | Statement::AlterOperator(_)
        | Statement::AlterOperatorFamily(_)
        | Statement::AlterOperatorClass(_)
        | Statement::AlterRole { .. }
        | Statement::AlterPolicy(_)
        | Statement::AlterConnector { .. }
        | Statement::AlterUser(_) => "ALTER",

        Statement::Drop { .. }
        | Statement::DropFunction(_)
        | Statement::DropDomain(_)
        | Statement::DropProcedure { .. }
        | Statement::DropSecret { .. }
        | Statement::DropPolicy(_)
        | Statement::DropConnector { .. }
        | Statement::DropExtension(_)
        | Statement::DropOperator(_)
        | Statement::DropOperatorFamily(_)
        | Statement::DropOperatorClass(_)
        | Statement::DropTrigger(_) => "DROP",

        Statement::Grant(_) => "GRANT",
        Statement::Deny(_) => "DENY",
        Statement::Revoke(_) => "REVOKE",

        Statement::Copy { .. } | Statement::CopyIntoSnowflake { .. } => "COPY",
        Statement::Call(_) => "CALL",
        Statement::Execute { .. } => "EXEC",

        Statement::AttachDatabase { .. } | Statement::AttachDuckDBDatabase { .. } => {
            "ATTACH DATABASE"
        }
        Statement::DetachDuckDBDatabase { .. } => "DETACH DATABASE",

        Statement::RenameTable(_) => "RENAME TABLE",
        Statement::LoadData { .. } => "LOAD DATA",
        Statement::Vacuum(_) => "VACUUM",
        // `ANALYZE` rewrites the planner's statistics.
        Statement::Analyze(_) => "ANALYZE",

        // Anything else — including a variant a later `sqlparser` adds — is
        // refused rather than assumed. Guessing "write" here would be as wrong
        // as guessing "read": both hide the fact that nobody classified it.
        other => {
            return Verdict::Refused {
                reason: RefusalReason::Unrecognised {
                    statement: statement_label(other),
                },
            }
        }
    };
    Verdict::Write { kind }
}

/// Whether an `EXPLAIN` runs the statement it wraps.
///
/// `sqlparser` records `EXPLAIN ANALYZE ...` and `EXPLAIN (ANALYZE) ...` in two
/// different places: the bare form sets the `analyze` field, while the
/// parenthesised option form leaves that field **false** and puts `ANALYZE` in
/// `options`. Reading only the field is how `EXPLAIN (ANALYZE) INSERT ...`
/// executed while the guard reported a read.
///
/// The `arg` of an option is deliberately ignored, so `(ANALYZE false)` — which
/// Postgres does not execute — is still reported as executing. That errs towards
/// calling a read a write, which the writes-allowed setting can lift; the
/// opposite error cannot be lifted by anything.
///
/// An option name on neither list is **not** assumed harmless: it is reported as
/// unrecognised, so an option a later engine adds cannot silently carry
/// execution past this check.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ExplainEffect {
    PlanOnly,
    Executes,
    Unknown { option: String },
}

/// `EXPLAIN` options that change only what the plan *reports*, never whether the
/// wrapped statement runs (Postgres' full set, minus `ANALYZE`).
const PLAN_ONLY_EXPLAIN_OPTIONS: &[&str] = &[
    "VERBOSE",
    "COSTS",
    "SETTINGS",
    "GENERIC_PLAN",
    "BUFFERS",
    "WAL",
    "TIMING",
    "SUMMARY",
    "MEMORY",
    "SERIALIZE",
    "FORMAT",
];

fn explain_effect(analyze: bool, options: Option<&[UtilityOption]>) -> ExplainEffect {
    if analyze {
        return ExplainEffect::Executes;
    }
    let mut unknown: Option<String> = None;
    for option in options.unwrap_or_default() {
        let name = option.name.value.as_str();
        if name.eq_ignore_ascii_case("ANALYZE") {
            return ExplainEffect::Executes;
        }
        if unknown.is_none()
            && !PLAN_ONLY_EXPLAIN_OPTIONS
                .iter()
                .any(|entry| entry.eq_ignore_ascii_case(name))
        {
            unknown = Some(name.to_string());
        }
    }
    match unknown {
        Some(option) => ExplainEffect::Unknown { option },
        None => ExplainEffect::PlanOnly,
    }
}

/// A short label for an unrecognised statement: the leading words of the
/// re-rendered statement, so the refusal names what it saw.
fn statement_label(statement: &Statement) -> String {
    let rendered = statement.to_string();
    let label: Vec<&str> = rendered.split_whitespace().take(2).collect();
    if label.is_empty() {
        "(unknown statement)".to_string()
    } else {
        label.join(" ")
    }
}

/// The write hiding inside a query, if any: a `SELECT ... INTO`, or a
/// data-modifying CTE (`WITH x AS (DELETE ... RETURNING *) SELECT * FROM x`),
/// at any depth reachable from the query body.
fn query_write_kind(query: &Query) -> Option<&'static str> {
    if let Some(with) = &query.with {
        for cte in &with.cte_tables {
            if let Some(kind) = query_write_kind(&cte.query) {
                return Some(kind);
            }
        }
    }
    set_expr_write_kind(&query.body)
}

fn set_expr_write_kind(body: &SetExpr) -> Option<&'static str> {
    match body {
        SetExpr::Select(select) => select.into.as_ref().map(|_| "SELECT ... INTO"),
        SetExpr::Query(query) => query_write_kind(query),
        SetExpr::SetOperation { left, right, .. } => {
            set_expr_write_kind(left).or_else(|| set_expr_write_kind(right))
        }
        SetExpr::Insert(_) => Some("INSERT"),
        SetExpr::Update(_) => Some("UPDATE"),
        SetExpr::Delete(_) => Some("DELETE"),
        SetExpr::Merge(_) => Some("MERGE"),
        SetExpr::Values(_) | SetExpr::Table(_) => None,
    }
}

/// The unquoted words of the batch, in order.
///
/// Tokenising rather than scanning raw text is what makes the keyword checks
/// above trustworthy: a comment is whitespace, a string literal is a literal,
/// and `[delete]` / `"delete"` carry a quote style — so none of them can be
/// mistaken for the keyword they spell.
fn bare_words(sql: &str, dialect: &dyn Dialect) -> Vec<String> {
    let Ok(tokens) = Tokenizer::new(dialect, sql).tokenize() else {
        return Vec::new();
    };
    tokens
        .into_iter()
        .filter_map(|token| match token {
            Token::Word(word) if word.quote_style.is_none() => Some(word.value),
            _ => None,
        })
        .collect()
}

fn denylist_entry(name: &str) -> Option<&'static str> {
    DENYLIST
        .iter()
        .find(|entry| entry.eq_ignore_ascii_case(name))
        .copied()
}

/// The first denylisted name the batch actually **calls**, if any.
///
/// Two rules, and only these two, make a name a call:
///
/// 1. it is immediately followed by `(` — `pg_read_file(...)`,
///    `OPENROWSET(...)`, a table function in a `FROM`; or
/// 2. it is part of the routine name of an `EXEC`/`EXECUTE` —
///    `EXEC sp_executesql N'...'`, `EXEC [master]..[xp_cmdshell] 'dir'`.
///
/// **Quoting is ignored on purpose, and that is the opposite of the rule
/// [`bare_words`] follows.** There, `[delete]` is an identifier and emphatically
/// not the `DELETE` keyword. Here the name *is* the identifier: quoting changes
/// how it resolves, never what it does, so `[xp_cmdshell]`, `"pg_read_file"` and
/// the backtick form all count. Keeping the two scans on the same rule is how
/// `EXEC [master]..[xp_cmdshell] 'dir'` passed as an ordinary write.
///
/// Two things are deliberately *not* matched, because neither runs anything: a
/// string literal spelling the name (the tokenizer keeps it a literal, and this
/// only ever inspects words), and a bare word in a value position — a column,
/// alias or table legitimately named `readfile` is not a call to it.
fn denylisted_call(sql: &str, dialect: &dyn Dialect) -> Option<&'static str> {
    let Ok(tokens) = Tokenizer::new(dialect, sql).tokenize() else {
        return None;
    };
    // Whitespace here includes comments, so a comment between the name and its
    // `(` cannot hide the call.
    let tokens: Vec<Token> = tokens
        .into_iter()
        .filter(|token| !matches!(token, Token::Whitespace(_)))
        .collect();

    let mut index = 0;
    while index < tokens.len() {
        let Token::Word(word) = &tokens[index] else {
            index += 1;
            continue;
        };
        let is_exec = word.quote_style.is_none()
            && (word.value.eq_ignore_ascii_case("EXEC")
                || word.value.eq_ignore_ascii_case("EXECUTE"));
        if is_exec {
            // The routine name is the run of identifier and `.` tokens after the
            // keyword. Every part is checked, not just the last: a denylisted
            // name never legitimately appears as a database or schema qualifier
            // either, and `[master]..[xp_cmdshell]` puts the interesting half at
            // an index that depends on how many qualifiers were elided.
            let mut next = index + 1;
            while next < tokens.len() {
                match &tokens[next] {
                    Token::Word(part) => {
                        if let Some(hit) = denylist_entry(&part.value) {
                            return Some(hit);
                        }
                    }
                    Token::Period => {}
                    _ => break,
                }
                next += 1;
            }
            index = next.max(index + 1);
            continue;
        }
        if matches!(tokens.get(index + 1), Some(Token::LParen)) {
            if let Some(hit) = denylist_entry(&word.value) {
                return Some(hit);
            }
        }
        index += 1;
    }
    None
}

fn hidden_keyword(words: &[String]) -> Option<String> {
    for (index, word) in words.iter().enumerate() {
        let Some(keyword) = READ_INCOMPATIBLE
            .iter()
            .find(|entry| entry.eq_ignore_ascii_case(word))
        else {
            continue;
        };
        // `SELECT ... FOR UPDATE` (and `FOR NO KEY UPDATE`) is a locking read,
        // not an update.
        if keyword.eq_ignore_ascii_case("UPDATE")
            && words[index.saturating_sub(3)..index]
                .iter()
                .any(|w| w.eq_ignore_ascii_case("FOR"))
        {
            continue;
        }
        return Some((*keyword).to_string());
    }
    None
}

/// The T-SQL `GO` separator: a line holding only `GO`, optionally followed by a
/// repeat count. It is read by the *client* and never sent, so a batch
/// containing one cannot be run as it stands — and it is refused by name rather
/// than silently stripped, because stripping it would run statements the user
/// wrote as separate batches as one.
fn batch_separator(sql: &str) -> Option<String> {
    for line in sql.lines() {
        let mut parts = line.split_whitespace();
        let Some(first) = parts.next() else { continue };
        if !first.eq_ignore_ascii_case("GO") {
            continue;
        }
        match (parts.next(), parts.next()) {
            (None, _) => return Some("GO".to_string()),
            (Some(count), None) if count.parse::<u32>().is_ok() => return Some("GO".to_string()),
            _ => continue,
        }
    }
    None
}

#[cfg(test)]
#[path = "guard_tests.rs"]
mod tests;
