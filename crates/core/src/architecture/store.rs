//! Diagrams on disk: where they live, how they say where they came from, and
//! what happens when a person edits one.
//!
//! A diagram is a markdown file with a small block of front matter. The body is
//! whatever renders it — today [`super::mermaid`] — and this module never looks
//! inside it. What this module owns is the *provenance*: a reader has to be
//! able to tell a diagram derived from the manifests from one a language model
//! proposed from one a person drew, because those three are trusted very
//! differently (see [`super::graph::Derivation`]).
//!
//! # Why provenance lives in front matter
//!
//! The obvious alternative is a `%%` comment in the Mermaid body, which is
//! cheaper and invisible when rendered. It was rejected because a comment is
//! part of the text a person edits: it can be deleted by accident, reflowed by
//! a formatter, or copied verbatim onto a different diagram. The requirement
//! here is stronger than "usually says" — a diagram must *always* be able to
//! say how it was produced, and an unlabelled arrow is exactly the confidently
//! wrong claim the rest of this feature is built to avoid. Front matter is a
//! separate region with a parser that either understands it or refuses to.
//!
//! # Two directories, one of them gitignored
//!
//! [`DIAGRAMS_DIR`] holds the diagrams a team keeps: anything a person drew or
//! an agent inferred and a person accepted. It is committed, the same way
//! `config.json` and `adapters/` under `.code-basics/` are committed on
//! purpose. [`DERIVED_DIR`] holds diagrams recomputed deterministically from
//! the manifests; committing those would put a regenerated file in everyone's
//! diff for every refactor while telling them nothing the manifests do not
//! already say, so it is listed in [`crate::config`]'s ignore set along with
//! [`PROMPTS_DIR`]. Which directory a diagram lands in is not a parameter: it
//! follows from its [`DiagramDerivation`], so the two can never disagree.
//!
//! # No YAML
//!
//! The front matter is parsed by hand, line by line, against a fixed key set.
//! Pulling in a YAML parser to read six keys would buy anchors, block scalars
//! and type coercion this format does not want, and would answer a malformed
//! file with a guess. The hand parser understands [`KNOWN_KEYS`] and nothing
//! else, and a file it cannot understand is reported as
//! [`DiagramDerivation::User`] with a warning rather than rejected outright —
//! refusing to show a person their own file is worse than showing it
//! unlabelled.
//!
//! The cost of that strictness is deliberate and worth naming: a diagram
//! written by a *later* version of this format, carrying a key this version has
//! never heard of, reads as an unlabelled user diagram here. That is the
//! abstain rule applied honestly — an unknown key may change the meaning of the
//! ones beside it, and this code cannot know that it does not. It is also why
//! the key set is kept as small as it is.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

/// The committed diagram directory, relative to [`crate::config::CONFIG_DIR`].
pub const DIAGRAMS_DIR: &str = "diagrams";

/// The regenerated diagram directory, relative to [`DIAGRAMS_DIR`].
pub const DERIVED_DIR: &str = "derived";

/// Where prompts used to infer a diagram are kept, relative to
/// [`DIAGRAMS_DIR`].
pub const PROMPTS_DIR: &str = ".prompts";

/// The value of the `code-basics` front matter key this module writes and is
/// willing to read.
pub const FORMAT_VERSION: &str = "v1";

/// Every front matter key this version understands.
pub const KNOWN_KEYS: &[&str] = &[
    "code-basics",
    "level",
    "derivation",
    "agent",
    "generated",
    "sourceCommit",
    "edited",
];

/// Where a stored diagram came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DiagramDerivation {
    /// Recomputed from the manifests by [`super::graph::project_graph`].
    Derived,
    /// Proposed by a coding agent, named so a reader can weigh it.
    Inferred { agent: String },
    /// Drawn by a person — or a file whose provenance could not be read.
    User,
}

/// The parsed front matter block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FrontMatter {
    pub level: Option<String>,
    pub derivation: DiagramDerivation,
    pub generated: Option<String>,
    pub source_commit: Option<String>,
    pub edited: bool,
}

/// One diagram file, as [`list`] reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagramFile {
    pub name: String,
    pub path: String,
    pub level: Option<String>,
    pub derivation: DiagramDerivation,
    pub generated: Option<String>,
    pub edited: bool,
    pub warning: Option<String>,
}

/// A diagram split into its provenance and its renderable body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDiagram {
    pub front: FrontMatter,
    pub body: String,
    pub warning: Option<String>,
}

// ---------------------------------------------------------------------------
// Locations
// ---------------------------------------------------------------------------

/// The committed diagram directory for a workspace.
pub fn dir(root: &Path) -> PathBuf {
    crate::config::config_dir(root).join(DIAGRAMS_DIR)
}

/// The regenerated diagram directory, which is gitignored.
pub fn derived_dir(root: &Path) -> PathBuf {
    dir(root).join(DERIVED_DIR)
}

/// The prompt directory, which is gitignored.
pub fn prompts_dir(root: &Path) -> PathBuf {
    dir(root).join(PROMPTS_DIR)
}

/// Where a diagram with this provenance belongs.
///
/// The directory is a function of the derivation rather than a parameter, so a
/// derived diagram cannot end up committed nor a hand-drawn one filed where the
/// next scan would overwrite it.
pub fn path(root: &Path, name: &str, derivation: &DiagramDerivation) -> Result<PathBuf> {
    let name = file_name(name)?;
    Ok(match derivation {
        DiagramDerivation::Derived => derived_dir(root).join(name),
        DiagramDerivation::Inferred { .. } | DiagramDerivation::User => dir(root).join(name),
    })
}

/// Turn a caller-supplied name into a single file name, or refuse it.
///
/// This takes a string that may have come from a diagram's own contents — a
/// name written by whatever produced the file — so it is treated as hostile,
/// and every check runs against the name *as given*, before the `.md` is
/// appended. Validating the extended name instead would have let `..` through
/// as the perfectly ordinary file `...md`, which is how the first version of
/// this function failed its own test.
///
/// Three checks, each catching what the others miss:
///
/// * No `/`, `\` or `:` anywhere, tested on the string rather than through
///   [`Path`]. `Path` is platform-dependent in exactly the wrong direction
///   here: `C:\Windows\System32` is a drive prefix plus components on Windows
///   and one perfectly legal file name on Linux, so a check that only asked
///   `Path` would pass a traversal through on the platform that happened to
///   disagree.
/// * Exactly one ordinary component, which rejects roots and prefixes on the
///   platform that does understand them.
/// * A name that is not made of dots alone, which rejects `.`, `..` and the
///   bare extension `.md` — none of them identify a diagram, and the first two
///   are the traversal the first check already caught, arriving unescorted.
fn file_name(name: &str) -> Result<String> {
    if name.is_empty() || name.trim() != name {
        bail!("a diagram name may not be empty or padded with spaces");
    }
    if name.contains(['/', '\\', ':']) {
        bail!("'{name}' is not a diagram name: it must be a single file name");
    }

    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(one)), None) if one == std::ffi::OsStr::new(name) => {}
        _ => bail!("'{name}' is not a diagram name: it must be a single file name"),
    }

    let stem = name.strip_suffix(".md").unwrap_or(name);
    if stem.trim_matches('.').is_empty() {
        bail!("'{name}' is not a diagram name: there is nothing in it to name a diagram");
    }

    Ok(match Path::new(name).extension() {
        Some(e) if e.eq_ignore_ascii_case("md") => name.to_string(),
        _ => format!("{name}.md"),
    })
}

// ---------------------------------------------------------------------------
// Front matter
// ---------------------------------------------------------------------------

/// Split a diagram into its provenance and its body.
///
/// Never fails. Anything this parser does not fully understand is reported as
/// [`DiagramDerivation::User`] with the whole file as the body and a warning
/// naming what went wrong: the file still renders, because refusing to show a
/// person their own diagram is worse than showing it unlabelled, and the one
/// thing that must never happen — presenting an unreadable file as derived
/// fact — cannot, since the fallback is the least authoritative case there is.
pub fn parse(text: &str) -> ParsedDiagram {
    match split(text) {
        Some((header, body)) => match read_keys(header) {
            Ok(front) => ParsedDiagram {
                front,
                body: strip_one_blank_line(body).to_string(),
                warning: None,
            },
            Err(why) => unlabelled(text, why.to_string()),
        },
        None => unlabelled(
            text,
            "this file has no readable front matter, so it is shown as a user diagram".into(),
        ),
    }
}

fn unlabelled(text: &str, warning: String) -> ParsedDiagram {
    ParsedDiagram {
        front: FrontMatter {
            level: None,
            derivation: DiagramDerivation::User,
            generated: None,
            source_commit: None,
            edited: false,
        },
        body: text.to_string(),
        warning: Some(warning),
    }
}

/// Return the header block and everything after it, or `None` when the file
/// does not open with a terminated `---` block.
fn split(text: &str) -> Option<(&str, &str)> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;

    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim() == "---" {
            return Some((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

/// Drop the single blank line [`render`] writes between the header and the
/// body, and nothing more — blank lines the author put there are theirs.
fn strip_one_blank_line(body: &str) -> &str {
    body.strip_prefix("\r\n")
        .or_else(|| body.strip_prefix('\n'))
        .unwrap_or(body)
}

fn read_keys(header: &str) -> Result<FrontMatter> {
    let mut seen: Vec<(&str, String)> = Vec::new();

    for line in header.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            bail!("front matter line '{line}' is not a 'key: value' pair");
        };
        let (key, value) = (key.trim(), value.trim());

        let Some(known) = KNOWN_KEYS.iter().find(|k| **k == key) else {
            // Not "ignore what you do not recognise": an unknown key may change
            // what the keys beside it mean, and this version cannot know that
            // it does not.
            bail!("front matter key '{key}' is not one this version understands");
        };
        if value.is_empty() {
            bail!("front matter key '{key}' has no value");
        }
        if seen.iter().any(|(k, _)| *k == *known) {
            bail!("front matter key '{key}' appears twice with different values");
        }
        seen.push((known, value.to_string()));
    }

    let get = |key: &str| {
        seen.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.to_string())
    };

    match get("code-basics").as_deref() {
        Some(FORMAT_VERSION) => {}
        Some(other) => bail!("this diagram is in format '{other}', which this version cannot read"),
        None => bail!("front matter without a 'code-basics' version marker is not read"),
    }

    let agent = get("agent");
    let derivation = match (get("derivation").as_deref(), agent) {
        (Some("derived"), None) => DiagramDerivation::Derived,
        (Some("user"), None) => DiagramDerivation::User,
        // An agent's claim is only worth reading if the agent is named, and a
        // name attached to anything else describes a file no agent produced.
        (Some("inferred"), Some(agent)) => DiagramDerivation::Inferred { agent },
        (Some("inferred"), None) => bail!("an inferred diagram must name the agent that made it"),
        (Some(other), Some(_)) => bail!("'{other}' diagrams are not produced by an agent"),
        (Some(other), None) => bail!("'{other}' is not a derivation this version understands"),
        (None, _) => bail!("front matter without a 'derivation' says nothing about provenance"),
    };

    let edited = match get("edited").as_deref() {
        None => false,
        Some("true") => true,
        Some("false") => false,
        Some(other) => bail!("'edited: {other}' is not true or false"),
    };

    Ok(FrontMatter {
        level: get("level"),
        derivation,
        generated: get("generated"),
        source_commit: get("sourceCommit"),
        edited,
    })
}

/// Write front matter and body into the text that goes on disk.
///
/// Keys are emitted in a fixed order so that rewriting an unchanged diagram
/// produces no diff. A value that could not be read back — one carrying a line
/// break, or padding a line-based format would lose — is refused rather than
/// written: the file it produced would parse as *something*, and that something
/// would be a provenance claim nobody made.
pub fn render(front: &FrontMatter, body: &str) -> Result<String> {
    let mut out = String::from("---\n");
    let mut put = |key: &str, value: &str| -> Result<()> {
        if value.trim() != value || value.is_empty() || value.contains(['\n', '\r']) {
            bail!("'{key}' cannot be stored: '{value}' would not survive being read back");
        }
        out.push_str(key);
        out.push_str(": ");
        out.push_str(value);
        out.push('\n');
        Ok(())
    };

    put("code-basics", FORMAT_VERSION)?;
    if let Some(level) = &front.level {
        put("level", level)?;
    }
    match &front.derivation {
        DiagramDerivation::Derived => put("derivation", "derived")?,
        DiagramDerivation::User => put("derivation", "user")?,
        DiagramDerivation::Inferred { agent } => {
            put("derivation", "inferred")?;
            put("agent", agent)?;
        }
    }
    if front.edited {
        put("edited", "true")?;
    }
    if let Some(generated) = &front.generated {
        put("generated", generated)?;
    }
    if let Some(commit) = &front.source_commit {
        put("sourceCommit", commit)?;
    }

    out.push_str("---\n\n");
    out.push_str(body);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Every diagram in the workspace, committed ones first.
///
/// The order is committed-then-regenerated, each alphabetical, and it is part
/// of the contract: a list that reshuffles between calls makes a UI jump under
/// the user's cursor. A file whose front matter cannot be read is listed like
/// any other with its `warning` set — the alternative, quietly omitting it,
/// would leave a person looking for a diagram that is plainly on disk.
pub fn list(root: &Path) -> Result<Vec<DiagramFile>> {
    let mut out = Vec::new();
    collect(root, &dir(root), &mut out)?;
    collect(root, &derived_dir(root), &mut out)?;
    Ok(out)
}

fn collect(root: &Path, from: &Path, out: &mut Vec<DiagramFile>) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(from) else {
        // A directory that is not there is not an error: a workspace that has
        // never had a diagram is the normal case.
        return Ok(());
    };

    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("failed to list {}", from.display()))?;
        let path = entry.path();
        if !path.is_file()
            || !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            continue;
        }
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };

        // An unreadable file is described as far as it can be. Failing the
        // whole listing because of one of them would hide every other diagram.
        let parsed = std::fs::read_to_string(&path).map(|text| parse(&text));
        let (front, warning) = match parsed {
            Ok(parsed) => (parsed.front, parsed.warning),
            Err(e) => (
                unlabelled("", String::new()).front,
                Some(format!("{name} could not be read: {e}")),
            ),
        };

        found.push(DiagramFile {
            name,
            path: relative_slashes(root, &path),
            level: front.level,
            derivation: front.derivation,
            generated: front.generated,
            edited: front.edited,
            warning,
        });
    }

    found.sort_by(|a, b| a.name.cmp(&b.name));
    out.extend(found);
    Ok(())
}

fn relative_slashes(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Read one diagram by name, exactly as it is on disk.
///
/// The committed copy wins over a regenerated one of the same name. That order
/// matches what [`write`] does when a person edits a derived diagram — it
/// promotes the file and deletes the regenerated copy — so the two can only
/// coexist if something outside this module put them there, and in that case
/// the one a person chose to keep is the one to show.
pub fn read(root: &Path, name: &str) -> Result<String> {
    let name = file_name(name)?;
    for candidate in [dir(root).join(&name), derived_dir(root).join(&name)] {
        if candidate.is_file() {
            return std::fs::read_to_string(&candidate)
                .with_context(|| format!("could not read {}", candidate.display()));
        }
    }
    bail!("there is no diagram called {name} in this workspace")
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Store a diagram with the provenance its author states.
///
/// This is the machine-authored path: the derivation is taken at face value
/// because the caller is the thing that derived or inferred it. A person's save
/// goes through [`write`] instead, which will not take provenance from the text
/// being saved.
pub fn write_authored(root: &Path, name: &str, front: &FrontMatter, body: &str) -> Result<PathBuf> {
    let target = path(root, name, &front.derivation)?;
    let text = render(front, body)?;
    put(&target, &text)?;
    Ok(target)
}

/// Save a person's edit of a diagram.
///
/// Two rules, both about provenance, both because a diagram's derivation is a
/// claim a reader will act on:
///
/// * **Provenance comes from the file already on disk, never from the text
///   being saved.** The editor shows the front matter, so without this anyone
///   could type `derivation: derived` and have their own drawing presented as a
///   fact read out of the manifests. The level, timestamp and commit *are*
///   taken from the saved text — those are description, not authority.
/// * **A change to the body is recorded.** An
///   [`Inferred`](DiagramDerivation::Inferred) diagram keeps its agent and
///   gains `edited: true`, because the arrows still came from that agent and a
///   person has since changed them; both halves matter to a reader. A
///   [`Derived`](DiagramDerivation::Derived) one is promoted to
///   [`User`](DiagramDerivation::User) and moved out of the regenerated
///   directory, since a file that is overwritten on every scan *and*
///   gitignored would throw the edit away twice over.
///
/// Saving text that matches the body already stored changes nothing and claims
/// no edit, so opening a diagram and pressing save cannot rewrite its history.
pub fn write(root: &Path, name: &str, contents: &str) -> Result<PathBuf> {
    let name = file_name(name)?;
    let incoming = parse(contents);

    let existing = [dir(root).join(&name), derived_dir(root).join(&name)]
        .into_iter()
        .find(|candidate| candidate.is_file());
    let stored = existing
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|text| parse(&text));

    let mut front = incoming.front.clone();
    if let Some(stored) = &stored {
        front.derivation = stored.front.derivation.clone();
        front.edited = stored.front.edited;

        if stored.body != incoming.body {
            match &front.derivation {
                DiagramDerivation::Inferred { .. } => front.edited = true,
                DiagramDerivation::Derived => {
                    front.derivation = DiagramDerivation::User;
                    front.edited = false;
                }
                DiagramDerivation::User => {}
            }
        }
    }

    let target = path(root, &name, &front.derivation)?;
    let text = render(&front, &incoming.body)?;
    put(&target, &text)?;

    if let Some(previous) = existing {
        if previous != target {
            std::fs::remove_file(&previous)
                .with_context(|| format!("could not remove {}", previous.display()))?;
        }
    }
    Ok(target)
}

/// Write a diagram file, creating what has to exist around it.
///
/// `std::fs::write` does not create parent directories, and the first diagram a
/// workspace ever stores is written into a `.code-basics/diagrams/` that does
/// not exist yet. The ignore entries are refreshed in the same call rather than
/// left to whichever other feature runs next, because [`DERIVED_DIR`] and
/// [`PROMPTS_DIR`] land inside a directory the user shares with their team and
/// the entry has to be there the moment the first file is.
fn put(target: &Path, text: &str) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    std::fs::write(target, text)
        .with_context(|| format!("could not write {}", target.display()))?;

    if let Some(config) = target.parent().and_then(config_dir_of) {
        crate::config::ensure_gitignore(&config)?;
    }
    Ok(())
}

/// Walk back up from a diagram's directory to the `.code-basics/` above it.
fn config_dir_of(from: &Path) -> Option<PathBuf> {
    let mut candidate = Some(from);
    while let Some(current) = candidate {
        if current.file_name() == Some(std::ffi::OsStr::new(crate::config::CONFIG_DIR)) {
            return Some(current.to_path_buf());
        }
        candidate = current.parent();
    }
    None
}
