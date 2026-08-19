//! Business-rule invariants, authored as markdown and injected as review
//! context.
//!
//! A *rule doc* is a plain `.md` file a team writes down: an invariant the code
//! must uphold ("money is always stored in minor units", "every public endpoint
//! authorises the caller"). Unlike an erosion rule — one regex against one side
//! of a diff — a rule doc is prose for a human *and* for a reviewing agent, so
//! it carries no pattern and matches nothing on its own. It is loaded and handed
//! to a review as context (see [`crate::review::compose_prompt`]) so the agent
//! judges the diff against the rules the team actually stated.
//!
//! The file format is the same front-matter-and-body shape the Enhancements
//! library uses, parsed through the very same [`split_front_matter`]: an
//! optional `---`-fenced `id`/`title` block, then the markdown body. A file with
//! no front matter is never dropped — it abstains to safe fallbacks (the file
//! stem for the id, the first heading or the stem for the title), exactly like a
//! bare instruction template.
//!
//! Authored rules are committed like erosion rules and declarative adapters, so
//! `rules/` is deliberately **not** in [`crate::config`]'s ignore list.
//!
//! All pure string and filesystem work, tested headlessly in `rules_tests.rs`.
//!
//! [`split_front_matter`]: crate::enhancements::split_front_matter

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::enhancements::split_front_matter;

/// One business-rule invariant, parsed from a markdown file.
///
/// Crosses IPC on [`crate::rules`]'s `list_rules` command, so its keys are pinned
/// by a test and mirrored by hand in `src/ipc/types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RuleDoc {
    /// Stable slug: the front-matter `id`, or the file stem when absent.
    pub id: String,
    /// Human label: the front-matter `title`, else the first `#` heading, else
    /// the file stem. Never empty.
    pub title: String,
    /// The markdown body (front matter stripped), the text handed to a review as
    /// context.
    pub body: String,
}

/// Everything [`load_rules`] found: the parsed docs and any file it could not
/// read.
///
/// Crosses IPC on the `list_rules` command, exactly like
/// [`crate::erosion::ErosionReport`], so its keys are pinned and mirrored in
/// `types.ts`. `warnings` is surfaced, never dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RulesReport {
    pub rules: Vec<RuleDoc>,
    pub warnings: Vec<String>,
}

/// Where a workspace's own rule docs live.
pub fn rules_dir(workspace_root: &Path) -> PathBuf {
    crate::config::config_dir(workspace_root).join("rules")
}

/// Parse one rule doc. `default_id` (normally the file stem) fills in for a
/// missing or blank `id:` key, so a bare markdown file is still usable.
pub fn parse_rule_doc(text: &str, default_id: &str) -> RuleDoc {
    let (front, body) = match split_front_matter(text) {
        Some((front, body)) => (Some(front), body),
        None => (None, text.to_string()),
    };
    let body = body.trim().to_string();

    let mut id = default_id.to_string();
    let mut title: Option<String> = None;
    if let Some(front) = front {
        for line in front.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "id" if !value.is_empty() => id = value.to_string(),
                "title" if !value.is_empty() => title = Some(value.to_string()),
                _ => {}
            }
        }
    }

    let title = title
        .or_else(|| first_heading(&body))
        .unwrap_or_else(|| default_id.to_string());

    RuleDoc { id, title, body }
}

/// The text of the first markdown heading in `body`, if any.
fn first_heading(body: &str) -> Option<String> {
    body.lines().find_map(|line| {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#') {
            return None;
        }
        let heading = trimmed.trim_start_matches('#').trim();
        (!heading.is_empty()).then(|| heading.to_string())
    })
}

/// Load every `*.md` rule doc in a directory, sorted deterministically.
///
/// A file that will not read is reported in the warnings alongside the docs that
/// loaded, so one bad file cannot hide the rest; a missing directory is an empty
/// list, not an error. Mirrors [`crate::erosion::rules::load_dir`].
pub fn load_rules(dir: &Path) -> (Vec<RuleDoc>, Vec<String>) {
    let mut rules = Vec::new();
    let mut warnings = Vec::new();

    let Ok(entries) = std::fs::read_dir(dir) else {
        return (rules, warnings);
    };

    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect();
    paths.sort();

    for path in paths {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("rule")
            .to_string();
        match std::fs::read_to_string(&path) {
            Ok(text) => rules.push(parse_rule_doc(&text, &stem)),
            Err(e) => warnings.push(format!("{}: {e}", path.display())),
        }
    }

    (rules, warnings)
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
