//! The erosion rule model, its TOML loader, and the built-in set.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;

/// The kind of weakening a rule detects. Crosses IPC on every [`super::scan::ErosionFlag`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ErosionCategory {
    /// An assertion that was removed.
    DeletedAssertion,
    /// A test newly marked to skip.
    IgnoredTest,
    /// A newly broadened or emptied exception handler.
    WidenedCatch,
    /// A guard against null/None that was removed.
    RemovedNullCheck,
    /// A newly introduced unsafe cast, non-null assertion, or panic path.
    UnsafeCast,
    /// A stub, TODO, or not-implemented left in a production path.
    LeftoverStub,
    /// A timeout, retry, or validation that was removed.
    RemovedSafeguard,
    /// A log line that was removed.
    DroppedLog,
}

/// Which side of the diff a rule inspects.
///
/// The single most important correctness lever: a deleted-assertion rule reads
/// *removed* lines, an introduced-weakness rule reads *added* ones. Never
/// context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleSide {
    Added,
    Removed,
}

/// One rule: a regex against one side of the diff, for files of one kind.
///
/// Loaded from TOML; the built-in set is the same shape. Not sent over IPC —
/// only the flags it produces are.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErosionRule {
    pub id: String,
    pub category: ErosionCategory,
    pub side: RuleSide,
    /// A regular expression matched against a changed line's content.
    pub pattern: String,
    /// What to show the reviewer when this fires.
    pub message: String,
    /// File extensions this rule applies to (e.g. `.cs`). Empty means every file.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Skip files that look like tests — for rules about production paths
    /// (`TODO`, stubs) where a test fixture is a false positive.
    #[serde(default)]
    pub prod_only: bool,
}

/// A rule whose regex has been compiled and is ready to match.
pub struct CompiledRule {
    pub rule: ErosionRule,
    pub re: Regex,
}

/// The TOML file shape: a list of `[[rule]]` tables.
#[derive(Debug, Deserialize)]
struct RuleFile {
    #[serde(default)]
    rule: Vec<ErosionRule>,
}

/// Parse rules from one TOML document.
pub fn parse(toml_text: &str) -> Result<Vec<ErosionRule>> {
    let file: RuleFile = toml::from_str(toml_text).context("erosion rules are not valid TOML")?;

    for rule in &file.rule {
        anyhow::ensure!(!rule.id.trim().is_empty(), "an erosion rule needs an id");
        anyhow::ensure!(
            !rule.pattern.trim().is_empty(),
            "erosion rule `{}` needs a pattern",
            rule.id
        );
    }

    Ok(file.rule)
}

/// Load every `*.toml` rule file in a directory.
///
/// A malformed file is skipped with its error returned alongside the rules that
/// loaded, so one bad file cannot disable the rest.
pub fn load_dir(dir: &Path) -> (Vec<ErosionRule>, Vec<String>) {
    let mut rules = Vec::new();
    let mut errors = Vec::new();

    let Ok(entries) = std::fs::read_dir(dir) else {
        return (rules, errors);
    };

    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
        .collect();
    paths.sort();

    for path in paths {
        match std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|c| parse(&c))
        {
            Ok(mut parsed) => rules.append(&mut parsed),
            Err(e) => errors.push(format!("{}: {e}", path.display())),
        }
    }

    (rules, errors)
}

/// Where a workspace's own erosion rules live.
pub fn rules_dir(workspace_root: &Path) -> PathBuf {
    crate::config::config_dir(workspace_root).join("erosion")
}

/// The built-in rules followed by the workspace's own, which extend them.
pub fn all_rules(workspace_root: &Path) -> (Vec<ErosionRule>, Vec<String>) {
    let mut rules = builtin_rules();
    let (mut user, errors) = load_dir(&rules_dir(workspace_root));
    rules.append(&mut user);
    (rules, errors)
}

/// Compile rules, reporting any whose regex will not compile rather than
/// failing the scan.
pub fn compile(rules: &[ErosionRule]) -> (Vec<CompiledRule>, Vec<String>) {
    let mut compiled = Vec::new();
    let mut warnings = Vec::new();

    for rule in rules {
        match Regex::new(&rule.pattern) {
            Ok(re) => compiled.push(CompiledRule {
                rule: rule.clone(),
                re,
            }),
            Err(e) => warnings.push(format!(
                "erosion rule `{}` has an invalid pattern and was skipped: {e}",
                rule.id
            )),
        }
    }

    (compiled, warnings)
}

/// Convenience constructor for the built-in set.
fn r(
    id: &str,
    category: ErosionCategory,
    side: RuleSide,
    pattern: &str,
    message: &str,
    extensions: &[&str],
    prod_only: bool,
) -> ErosionRule {
    ErosionRule {
        id: id.into(),
        category,
        side,
        pattern: pattern.into(),
        message: message.into(),
        extensions: extensions.iter().map(|s| s.to_string()).collect(),
        prod_only,
    }
}

/// The rules that ship with the app, per ecosystem.
///
/// Chosen for high signal: each is a move an agent makes to reach green, and a
/// removed line the reviewer would otherwise skim past. Log *downgrade*
/// detection (Error → Debug) needs pairing removed and added lines and is left
/// out of this first set; only removed log lines are flagged.
pub fn builtin_rules() -> Vec<ErosionRule> {
    use ErosionCategory::*;
    use RuleSide::*;

    const CS: &[&str] = &[".cs"];
    const TS: &[&str] = &[".ts", ".tsx", ".js", ".jsx"];
    const RS: &[&str] = &[".rs"];

    vec![
        // -- .NET ----------------------------------------------------------
        r("cs-deleted-assert", DeletedAssertion, Removed, r"\bAssert\.", "A .NET assertion was removed.", CS, false),
        r("cs-ignored-test", IgnoredTest, Added, r"\[\s*Ignore", "A test was marked [Ignore].", CS, false),
        r("cs-skip-fact", IgnoredTest, Added, r"\bSkip\s*=", "A test was marked with Skip=.", CS, false),
        r("cs-wide-catch", WidenedCatch, Added, r"catch\s*\(\s*(System\.)?Exception", "A catch was widened to Exception.", CS, false),
        r("cs-empty-catch", WidenedCatch, Added, r"catch\s*\{", "An exception is caught and swallowed.", CS, false),
        r("cs-removed-null-guard", RemovedNullCheck, Removed, r"ArgumentNullException|==\s*null|!=\s*null", "A null guard was removed.", CS, false),
        r("cs-pragma-disable", UnsafeCast, Added, r"#pragma warning disable", "A compiler warning was disabled.", CS, false),
        r("cs-not-implemented", LeftoverStub, Added, r"NotImplementedException", "A NotImplementedException was left behind.", CS, true),
        r("cs-todo", LeftoverStub, Added, r"//\s*(TODO|FIXME)\b", "A TODO/FIXME was left in the code.", CS, true),
        r("cs-removed-safeguard", RemovedSafeguard, Removed, r"Timeout|CancellationToken|Polly|\bretry", "A timeout, retry, or cancellation was removed.", CS, false),
        r("cs-dropped-log", DroppedLog, Removed, r"_logger\.|ILogger|\bLog\.(Info|Warn|Error|Debug)", "A log line was removed.", CS, false),
        // -- TypeScript / JavaScript --------------------------------------
        r("ts-deleted-assert", DeletedAssertion, Removed, r"\bexpect\(|\bassert\b", "An assertion was removed.", TS, false),
        r("ts-skip-test", IgnoredTest, Added, r"\.skip\(|\bxit\(|\bxdescribe\(|it\.todo\b", "A test was skipped.", TS, false),
        r("ts-empty-catch", WidenedCatch, Added, r"catch\s*(\([^)]*\))?\s*\{\s*\}", "An error is caught and swallowed.", TS, false),
        r("ts-as-any", UnsafeCast, Added, r"\bas any\b", "A value was cast to any.", TS, false),
        r("ts-ts-ignore", UnsafeCast, Added, r"@ts-ignore|@ts-expect-error", "A type error was suppressed.", TS, false),
        r("ts-removed-null-guard", RemovedNullCheck, Removed, r"===?\s*null|!==?\s*null|===?\s*undefined", "A null/undefined guard was removed.", TS, false),
        r("ts-not-implemented", LeftoverStub, Added, r#"throw new Error\(\s*['"]not implemented"#, "A not-implemented stub was left behind.", TS, true),
        r("ts-todo", LeftoverStub, Added, r"//\s*(TODO|FIXME)\b", "A TODO/FIXME was left in the code.", TS, true),
        r("ts-removed-safeguard", RemovedSafeguard, Removed, r"AbortSignal|\btimeout\b|\bretry\b", "A timeout or retry was removed.", TS, false),
        r("ts-dropped-log", DroppedLog, Removed, r"console\.(log|warn|error|info)|logger\.", "A log line was removed.", TS, false),
        // -- Rust ----------------------------------------------------------
        r("rs-deleted-assert", DeletedAssertion, Removed, r"\bassert(_eq|_ne)?!", "An assertion was removed.", RS, false),
        r("rs-ignore", IgnoredTest, Added, r"#\[ignore\]", "A test was marked #[ignore].", RS, false),
        r("rs-unwrap", UnsafeCast, Added, r"\.unwrap\(\)|\.expect\(", "A panic path (unwrap/expect) was introduced.", RS, false),
        r("rs-unsafe", UnsafeCast, Added, r"\bunsafe\s*\{", "An unsafe block was introduced.", RS, false),
        r("rs-todo-macro", LeftoverStub, Added, r"\btodo!\(|\bunimplemented!\(", "A todo!/unimplemented! was left behind.", RS, true),
        r("rs-todo", LeftoverStub, Added, r"//\s*(TODO|FIXME)\b", "A TODO/FIXME was left in the code.", RS, true),
        r("rs-dropped-log", DroppedLog, Removed, r"tracing::(info|warn|error|debug)|log::(info|warn|error|debug)", "A log line was removed.", RS, false),
    ]
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
