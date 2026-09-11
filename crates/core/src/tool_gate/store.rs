//! Reading and writing `code-basics/mcp-tools.json`.
//!
//! The filesystem seam for [`super`]. Modelled on [`crate::features::store`] —
//! the same user-global path resolution, the same tolerant load, the same atomic
//! `.tmp`+rename save — but with **no installer seed**: no installer writes
//! per-tool state, and the built-in default (every tool on) is a complete answer
//! on its own, so there is nothing for a seed to adopt.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::ToolGateFile;

/// The file name inside `code-basics/`.
pub const MCP_TOOLS_FILE: &str = "mcp-tools.json";

/// Where the tool-gate file lives: `<config>/code-basics/mcp-tools.json`.
///
/// `CB_MCP_TOOLS_PATH` overrides the whole path, matching `CB_FEATURES_PATH`.
/// Otherwise the base is `%APPDATA%`, then `$XDG_CONFIG_HOME`, then `~/.config`,
/// then the current directory — the same resolution order as
/// [`crate::features::store::features_path`], so the MCP servers (separate
/// processes) resolve the identical path the app writes.
pub fn mcp_tools_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CB_MCP_TOOLS_PATH") {
        return PathBuf::from(path);
    }

    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));

    base.join("code-basics").join(MCP_TOOLS_FILE)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Read the tool gate at `path`. A missing or unparseable file yields the default
/// [`ToolGateFile`] (every tool enabled) rather than an error — this is a
/// preference, not a security boundary, so a corrupt file must not stop a server
/// starting, and the permissive default matches "every tool ships on". The real
/// consent boundaries (SQL/Redis exposure, browser automation) are enforced
/// independently of this file.
pub fn load(path: &Path) -> ToolGateFile {
    let Ok(text) = std::fs::read_to_string(path) else {
        return ToolGateFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Read the tool gate at `path` only if the file is actually there, so a caller
/// can tell "no store yet" from "a store that happens to be empty".
pub fn load_existing(path: &Path) -> Option<ToolGateFile> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(serde_json::from_str(&text).unwrap_or_default())
}

/// A sibling of `path` with `suffix` appended to its file name.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// Write the tool gate to `path`, creating the parent directory if absent.
///
/// Atomic, like [`crate::features::store::save`]: the JSON goes to a sibling
/// `.tmp` and is renamed over the target, so a crash mid-write cannot leave a
/// truncated file that the tolerant [`load`] would read back as "no choices
/// made" and quietly clobber.
pub fn save(path: &Path, gate: &ToolGateFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(gate).context("failed to serialise mcp tool gate")?;
    let tmp = sibling(path, ".tmp");
    std::fs::write(&tmp, format!("{json}\n"))
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
