//! Parsing `.http` / `.rest` scenario files (VS Code REST Client / JetBrains
//! HTTP Client syntax).
//!
//! Pure text in, a structured scenario out — like [`crate::symbols::declarations`],
//! it touches nothing. The impure replay that fires these requests lives in
//! [`super::replay`]; keeping the parse separate is what lets the exotic corners
//! (handler scripts, unresolved variables) be *tested* as abstentions rather
//! than guessed at.
//!
//! Recognised:
//! * `###` request separators (with optional trailing name), `# @name foo`.
//! * `METHOD url [HTTP/1.1]`, header lines, a blank line, then the body.
//! * `@var = value` file variables and `{{var}}` substitution.
//! * `# @readiness METHOD url STATUS [timeout=30s] [interval=500ms]` — a
//!   convention this feature defines for the readiness probe.
//!
//! Deliberately *not* executed: `> {% … %}` response-handler scripts (there is
//! no JS engine here) are skipped and recorded as a warning, and an
//! unresolved `{{var}}` is left in place with a warning rather than guessed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Discover `.http` / `.rest` scenario files under `root`.
///
/// Walks through [`crate::workspace::source_walker`], so discovery honours the
/// exact same rules as the project scan and the symbol index — `SKIP_DIRS`
/// (`node_modules`, `.git`, `target`, `.code-basics`, …), the depth cap, and the
/// nested-checkout exclusion — rather than re-deriving them here where they
/// would drift. Extension matching is case-insensitive; the returned paths are
/// absolute and sorted.
pub fn discover_http_files(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = crate::workspace::source_walker(root)
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .filter(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case("http") || x.eq_ignore_ascii_case("rest"))
        })
        .collect();
    found.sort();
    found
}

/// One request to replay against both sides.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequestSpec {
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

/// How to tell the app is up before firing requests.
#[derive(Debug, Clone, PartialEq)]
pub struct Readiness {
    pub method: String,
    pub url: String,
    pub expect_status: u16,
    pub timeout: Duration,
    pub poll_interval: Duration,
}

/// A parsed `.http` file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HttpScenario {
    /// Workspace-relative path; set by the caller, empty from [`parse_http_file`].
    pub path: String,
    pub requests: Vec<HttpRequestSpec>,
    pub readiness: Option<Readiness>,
    pub variables: BTreeMap<String, String>,
    pub warnings: Vec<String>,
}

/// Parse a `.http` file's text. Never fails — malformed pieces become warnings.
pub fn parse_http_file(text: &str) -> HttpScenario {
    let mut scenario = HttpScenario::default();

    // Pass 1: file-level `@var = value` definitions.
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix('@') {
            if let Some((k, v)) = rest.split_once('=') {
                scenario
                    .variables
                    .insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }

    // Pass 2: split into blocks on `###`, parse each.
    let mut blocks: Vec<Vec<&str>> = vec![Vec::new()];
    for line in text.lines() {
        if line.trim_start().starts_with("###") {
            blocks.push(vec![line]);
        } else {
            blocks.last_mut().unwrap().push(line);
        }
    }
    let vars = scenario.variables.clone();
    for block in blocks {
        parse_block(&block, &vars, &mut scenario);
    }

    scenario
}

fn parse_block(lines: &[&str], vars: &BTreeMap<String, String>, scenario: &mut HttpScenario) {
    let mut name: Option<String> = None;
    let mut method_url: Option<(String, String)> = None;
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut body_lines: Vec<String> = Vec::new();
    let mut in_body = false;
    let mut in_script = false;

    for (i, raw) in lines.iter().enumerate() {
        let t = raw.trim();

        if i == 0 && t.starts_with("###") {
            let rest = t.trim_start_matches('#').trim();
            if !rest.is_empty() {
                name = Some(rest.to_string());
            }
            continue;
        }

        if in_script {
            if t.contains("%}") {
                in_script = false;
            }
            continue;
        }
        if t.starts_with("> {%") || t.starts_with(">{%") || t == ">" {
            scenario
                .warnings
                .push("skipped a response-handler script (`> {% … %}`): not executed".into());
            if !t.contains("%}") {
                in_script = true;
            }
            continue;
        }

        if in_body {
            body_lines.push((*raw).to_string());
            continue;
        }

        if t.is_empty() {
            if method_url.is_some() {
                in_body = true;
            }
            continue;
        }

        if t.starts_with('#') || t.starts_with("//") {
            let d = t.trim_start_matches(['#', '/']).trim();
            if let Some(n) = d.strip_prefix("@name") {
                let n = n.trim_start_matches('=').trim();
                if !n.is_empty() {
                    name = Some(n.to_string());
                }
            } else if let Some(r) = d.strip_prefix("@readiness") {
                match parse_readiness(r.trim(), vars, scenario) {
                    Ok(rd) => scenario.readiness = Some(rd),
                    Err(e) => scenario.warnings.push(e),
                }
            }
            continue;
        }

        // File-variable lines are collected in pass 1; ignore here.
        if t.starts_with('@') {
            continue;
        }

        if method_url.is_none() {
            method_url = Some(parse_request_line(t));
        } else if let Some((k, v)) = t.split_once(':') {
            headers.push((k.trim().to_string(), substitute(v.trim(), vars, scenario)));
        }
    }

    let Some((method, url)) = method_url else {
        return; // a comment/readiness-only block has no request
    };
    let url = substitute(&url, vars, scenario);

    // Drop trailing blank body lines.
    while body_lines
        .last()
        .map(|l| l.trim().is_empty())
        .unwrap_or(false)
    {
        body_lines.pop();
    }
    let body = if body_lines.is_empty() {
        None
    } else {
        Some(substitute(&body_lines.join("\n"), vars, scenario))
    };

    let name = name.unwrap_or_else(|| format!("request {}", scenario.requests.len() + 1));
    scenario.requests.push(HttpRequestSpec {
        name,
        method,
        url,
        headers,
        body,
    });
}

/// `METHOD url [HTTP/1.1]`, or a bare url (defaulting to GET).
fn parse_request_line(line: &str) -> (String, String) {
    let mut parts = line.split_whitespace();
    let first = parts.next().unwrap_or("").to_string();
    const METHODS: &[&str] = &[
        "GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "TRACE", "CONNECT",
    ];
    if METHODS.contains(&first.to_ascii_uppercase().as_str()) {
        let url = parts.next().unwrap_or("").to_string();
        (first.to_ascii_uppercase(), url)
    } else {
        (String::from("GET"), first)
    }
}

fn parse_readiness(
    rest: &str,
    vars: &BTreeMap<String, String>,
    scenario: &mut HttpScenario,
) -> Result<Readiness, String> {
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    if tokens.len() < 3 {
        return Err(format!(
            "@readiness needs METHOD url STATUS (got {:?}); readiness probe ignored",
            rest
        ));
    }
    let method = tokens[0].to_ascii_uppercase();
    let url = substitute(tokens[1], vars, scenario);
    let expect_status: u16 = tokens[2]
        .parse()
        .map_err(|_| format!("@readiness status `{}` is not a number", tokens[2]))?;

    let mut timeout = Duration::from_secs(30);
    let mut poll_interval = Duration::from_millis(500);
    for tok in &tokens[3..] {
        if let Some(v) = tok.strip_prefix("timeout=") {
            if let Some(d) = parse_duration(v) {
                timeout = d;
            }
        } else if let Some(v) = tok.strip_prefix("interval=") {
            if let Some(d) = parse_duration(v) {
                poll_interval = d;
            }
        }
    }
    Ok(Readiness {
        method,
        url,
        expect_status,
        timeout,
        poll_interval,
    })
}

/// `30s`, `500ms`, `2m`, or a bare number of seconds.
fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if let Some(v) = s.strip_suffix("ms") {
        v.parse().ok().map(Duration::from_millis)
    } else if let Some(v) = s.strip_suffix('s') {
        v.parse().ok().map(Duration::from_secs)
    } else if let Some(v) = s.strip_suffix('m') {
        v.parse::<u64>().ok().map(|m| Duration::from_secs(m * 60))
    } else {
        s.parse().ok().map(Duration::from_secs)
    }
}

/// Replace `{{var}}` from `vars`; an unresolved one is left in place with a
/// warning rather than guessed.
fn substitute(s: &str, vars: &BTreeMap<String, String>, scenario: &mut HttpScenario) -> String {
    if !s.contains("{{") {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find("}}") {
            let key = after[..end].trim();
            match vars.get(key) {
                Some(v) => out.push_str(v),
                None => {
                    out.push_str("{{");
                    out.push_str(&after[..end]);
                    out.push_str("}}");
                    scenario
                        .warnings
                        .push(format!("unresolved variable {{{{{key}}}}} left as-is"));
                }
            }
            rest = &after[end + 2..];
        } else {
            out.push_str("{{");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
#[path = "httpfile_tests.rs"]
mod tests;
