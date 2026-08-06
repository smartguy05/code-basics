//! Reading .NET solution files.
//!
//! A solution is the .NET analogue of a Node workspace: it says which projects
//! belong together and how they are grouped. Nothing here decides how to *run*
//! anything — the project adapters already do that — so a solution is used
//! purely to group what the scan found.
//!
//! Two formats exist and both are supported:
//!
//! * **`.sln`** — the classic tab-indented text format, still what most
//!   repositories have.
//! * **`.slnx`** — the XML replacement introduced with Visual Studio 17.13 and
//!   the .NET 9 SDK.
//!
//! Parsing is deliberately tolerant: an unreadable or partially understood
//! solution should cost the grouping, never the scan.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

/// The GUID marking a solution folder rather than a buildable project.
const SOLUTION_FOLDER_TYPE: &str = "2150E333-8FDC-42A3-9474-1AB1AEA671C7";

/// A solution and the projects it contains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Solution {
    pub name: String,
    /// Path to the solution file, relative to the workspace root.
    pub path: PathBuf,
    pub projects: Vec<SolutionProject>,
}

/// One project entry inside a solution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SolutionProject {
    pub name: String,
    /// Path to the project file, relative to the workspace root, with `\`
    /// normalised to `/` so it compares equal to what the scan produces.
    pub path: PathBuf,
    /// The solution folder this project sits in, as a `/`-joined path. `None`
    /// for projects at the solution root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

/// Whether a file name is a solution this module can read.
pub fn is_solution_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("sln") | Some("slnx")
    )
}

/// Parse a solution file whose contents have already been read.
///
/// `relative_dir` is the solution's own directory relative to the workspace
/// root; project paths inside a solution are relative to it.
pub fn parse(name: &str, content: &str, relative_dir: &Path, is_xml: bool) -> Vec<SolutionProject> {
    if is_xml {
        parse_slnx(content, relative_dir)
    } else {
        parse_sln(content, relative_dir)
    }
    .into_iter()
    .filter(|p| {
        // Solution folders and unreadable entries contribute nothing.
        !p.name.is_empty() && p.path.extension().is_some()
    })
    .map(|mut p| {
        if p.name.is_empty() {
            p.name = name.to_string();
        }
        p
    })
    .collect()
}

/// Join a solution-relative path onto the solution's directory, normalising
/// Windows separators so the result matches the scan's own relative paths.
fn resolve(relative_dir: &Path, raw: &str) -> PathBuf {
    let normalised = raw.replace('\\', "/");
    let mut out = PathBuf::new();
    for component in relative_dir.components() {
        out.push(component);
    }
    for part in normalised.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Classic .sln
// ---------------------------------------------------------------------------

/// Parse the tab-indented `.sln` format.
///
/// Only two constructs matter: the `Project(...) = ...` header lines, and the
/// `NestedProjects` section that maps a project's GUID onto the GUID of the
/// solution folder holding it.
fn parse_sln(content: &str, relative_dir: &Path) -> Vec<SolutionProject> {
    struct Entry {
        name: String,
        path: String,
        is_folder: bool,
        parent: Option<String>,
    }

    let mut entries: BTreeMap<String, Entry> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut in_nested = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("GlobalSection(NestedProjects)") {
            in_nested = true;
            continue;
        }
        if trimmed.starts_with("EndGlobalSection") {
            in_nested = false;
            continue;
        }

        if in_nested {
            // `{child} = {parent}`
            if let Some((child, parent)) = trimmed.split_once('=') {
                let child = normalise_guid(child);
                let parent = normalise_guid(parent);
                if let Some(entry) = entries.get_mut(&child) {
                    entry.parent = Some(parent);
                }
            }
            continue;
        }

        let Some(rest) = trimmed.strip_prefix("Project(") else {
            continue;
        };
        // Project("{type}") = "name", "path", "{guid}"
        let Some((type_part, tail)) = rest.split_once(')') else {
            continue;
        };
        let Some((_, fields)) = tail.split_once('=') else {
            continue;
        };

        let quoted: Vec<&str> = fields
            .split('"')
            .enumerate()
            .filter_map(|(i, s)| (i % 2 == 1).then_some(s))
            .collect();
        if quoted.len() < 3 {
            continue;
        }

        let guid = normalise_guid(quoted[2]);
        let is_folder = normalise_guid(type_part).eq_ignore_ascii_case(SOLUTION_FOLDER_TYPE);

        order.push(guid.clone());
        entries.insert(
            guid,
            Entry {
                name: quoted[0].to_string(),
                path: quoted[1].to_string(),
                is_folder,
                parent: None,
            },
        );
    }

    // Walk each project's parent chain to build its solution-folder path.
    let folder_path = |mut guid: Option<String>| -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        // A malformed solution could describe a cycle; the entry count bounds it.
        for _ in 0..entries.len() {
            let Some(current) = guid else { break };
            let Some(entry) = entries.get(&current) else {
                break;
            };
            if !entry.is_folder {
                break;
            }
            parts.push(entry.name.clone());
            guid = entry.parent.clone();
        }
        if parts.is_empty() {
            return None;
        }
        parts.reverse();
        Some(parts.join("/"))
    };

    order
        .iter()
        .filter_map(|guid| {
            let entry = entries.get(guid)?;
            if entry.is_folder {
                return None;
            }
            Some(SolutionProject {
                name: entry.name.clone(),
                path: resolve(relative_dir, &entry.path),
                folder: folder_path(entry.parent.clone()),
            })
        })
        .collect()
}

/// Strip the braces and surrounding whitespace from a solution GUID.
fn normalise_guid(raw: &str) -> String {
    raw.trim()
        .trim_matches('"')
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .to_ascii_uppercase()
}

// ---------------------------------------------------------------------------
// .slnx
// ---------------------------------------------------------------------------

/// Parse the XML `.slnx` format.
///
/// `<Folder Name="/Src/">` elements nest, and `<Project Path="..." />` elements
/// sit either inside one or at the top level.
fn parse_slnx(content: &str, relative_dir: &Path) -> Vec<SolutionProject> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut out = Vec::new();
    let mut folders: Vec<String> = Vec::new();

    // `<Folder Name="/Src/Core/">` already carries the full path, so the stack
    // only needs the current innermost name.
    let folder_label = |folders: &[String]| -> Option<String> {
        let joined = folders
            .last()
            .map(|f| f.trim_matches('/').to_string())
            .unwrap_or_default();
        (!joined.is_empty()).then_some(joined)
    };

    loop {
        match reader.read_event() {
            event @ (Ok(Event::Start(_)) | Ok(Event::Empty(_))) => {
                // A `<Folder>` written as an empty element opens and closes in
                // one event, so only a Start may push onto the stack.
                let (e, opens_scope) = match event {
                    Ok(Event::Start(e)) => (e, true),
                    Ok(Event::Empty(e)) => (e, false),
                    _ => unreachable!("matched Start or Empty above"),
                };
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();

                if name.eq_ignore_ascii_case("Folder") {
                    if opens_scope {
                        folders.push(attribute(&e, "Name").unwrap_or_default());
                    }
                } else if name.eq_ignore_ascii_case("Project") {
                    if let Some(path) = attribute(&e, "Path") {
                        let resolved = resolve(relative_dir, &path);
                        let label = resolved
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        out.push(SolutionProject {
                            name: label,
                            path: resolved,
                            folder: folder_label(&folders),
                        });
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if name.eq_ignore_ascii_case("Folder") {
                    folders.pop();
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    out
}

fn attribute(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if a.key.local_name().as_ref().eq_ignore_ascii_case(name.as_bytes()) {
            a.unescape_value().ok().map(|v| v.into_owned())
        } else {
            None
        }
    })
}
