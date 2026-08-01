//! The workspace configuration file, `.code-basics/config.json`.
//!
//! Checked into the repository so run configurations are shared the way
//! Rider's `.run/` directory is. Auto-detected configurations are *not*
//! written here — only what the user creates or imports — so the file stays
//! small and re-detection keeps working as projects change.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::model::{ConfigSource, RunConfig};

/// Directory holding this app's per-workspace state.
pub const CONFIG_DIR: &str = ".code-basics";
pub const CONFIG_FILE: &str = "config.json";
/// Where test report files are written. Inside the config directory so a
/// single `.gitignore` entry covers everything.
pub const RESULTS_DIR: &str = "results";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConfig {
    /// Schema version, so a future format change can migrate rather than fail.
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub configs: Vec<RunConfig>,
}

fn default_version() -> u32 {
    1
}

pub fn config_dir(root: &Path) -> PathBuf {
    root.join(CONFIG_DIR)
}

pub fn config_path(root: &Path) -> PathBuf {
    config_dir(root).join(CONFIG_FILE)
}

pub fn results_dir(root: &Path) -> PathBuf {
    config_dir(root).join(RESULTS_DIR)
}

/// Load the configuration file, returning an empty configuration when absent.
pub fn load(root: &Path) -> Result<WorkspaceConfig> {
    let path = config_path(root);
    if !path.exists() {
        return Ok(WorkspaceConfig { version: 1, configs: Vec::new() });
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    serde_json::from_str(&content)
        .with_context(|| format!("{} is not valid configuration JSON", path.display()))
}

/// Write the configuration file, creating the directory if needed.
pub fn save(root: &Path, config: &WorkspaceConfig) -> Result<()> {
    let dir = config_dir(root);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create {}", dir.display()))?;

    // Results are regenerated on every run and must never be committed.
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, format!("{RESULTS_DIR}/\n"))
            .with_context(|| format!("failed to write {}", ignore.display()))?;
    }

    let path = config_path(root);
    let json = serde_json::to_string_pretty(config).context("failed to serialise configuration")?;

    std::fs::write(&path, format!("{json}\n"))
        .with_context(|| format!("failed to write {}", path.display()))
}

/// Combine detected configurations with saved ones.
///
/// A saved configuration replaces a detected one with the same id, so editing
/// a detected configuration makes it stick without creating a duplicate.
/// Everything else is additive.
pub fn merge(detected: Vec<RunConfig>, saved: Vec<RunConfig>) -> Vec<RunConfig> {
    let mut out: Vec<RunConfig> = Vec::with_capacity(detected.len() + saved.len());

    for config in detected {
        if !saved.iter().any(|s| s.id == config.id) {
            out.push(config);
        }
    }
    out.extend(saved);

    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    out
}

/// Add or replace a configuration and persist it.
pub fn upsert(root: &Path, config: RunConfig) -> Result<WorkspaceConfig> {
    let mut file = load(root)?;

    // A detected configuration being edited becomes a user configuration, or
    // the next scan would overwrite the change.
    let mut config = config;
    if config.source == ConfigSource::Detected {
        config.source = ConfigSource::UserFile;
    }

    match file.configs.iter_mut().find(|c| c.id == config.id) {
        Some(existing) => *existing = config,
        None => file.configs.push(config),
    }

    save(root, &file)?;
    Ok(file)
}

/// Remove a saved configuration.
///
/// Returns whether anything was removed. Detected configurations reappear on
/// the next scan, which is the intended behaviour: this only forgets an
/// override.
pub fn remove(root: &Path, id: &str) -> Result<bool> {
    let mut file = load(root)?;
    let before = file.configs.len();
    file.configs.retain(|c| c.id != id);

    let removed = file.configs.len() != before;
    if removed {
        save(root, &file)?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RunKind, RunConfig};

    fn config(id: &str, name: &str, source: ConfigSource) -> RunConfig {
        RunConfig::new(id, name, RunKind::App, "dotnet", source)
    }

    #[test]
    fn a_missing_file_loads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load(dir.path()).unwrap();

        assert_eq!(loaded.version, 1);
        assert!(loaded.configs.is_empty());
    }

    #[test]
    fn saves_and_reloads_configurations() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = WorkspaceConfig { version: 1, configs: vec![] };
        let mut c = config("api:run", "Api", ConfigSource::UserFile);
        c.args = vec!["--verbose".into()];
        c.env.insert("KEY".into(), "value".into());
        file.configs.push(c);

        save(dir.path(), &file).unwrap();
        let reloaded = load(dir.path()).unwrap();

        assert_eq!(reloaded.configs.len(), 1);
        assert_eq!(reloaded.configs[0].args, vec!["--verbose"]);
        assert_eq!(reloaded.configs[0].env.get("KEY").map(String::as_str), Some("value"));
    }

    #[test]
    fn saving_ignores_the_results_directory() {
        // Reports are rewritten on every run; committing them would be noise
        // in exactly the diff view this app exists to keep clean.
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &WorkspaceConfig::default()).unwrap();

        let ignore = std::fs::read_to_string(config_dir(dir.path()).join(".gitignore")).unwrap();
        assert!(ignore.contains("results/"));
    }

    #[test]
    fn a_saved_configuration_replaces_a_detected_one_with_the_same_id() {
        let detected = vec![
            config("api:run", "Api (detected)", ConfigSource::Detected),
            config("web:run", "Web", ConfigSource::Detected),
        ];
        let saved = vec![config("api:run", "Api (customised)", ConfigSource::UserFile)];

        let merged = merge(detected, saved);

        assert_eq!(merged.len(), 2, "the override must not create a duplicate");
        let api = merged.iter().find(|c| c.id == "api:run").unwrap();
        assert_eq!(api.name, "Api (customised)");
    }

    #[test]
    fn merging_keeps_configurations_that_only_exist_on_one_side() {
        let detected = vec![config("a", "A", ConfigSource::Detected)];
        let saved = vec![config("b", "B", ConfigSource::UserFile)];

        let merged = merge(detected, saved);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn editing_a_detected_configuration_makes_it_a_user_configuration() {
        // Otherwise the next scan would silently discard the edit.
        let dir = tempfile::tempdir().unwrap();
        upsert(dir.path(), config("api:run", "Api", ConfigSource::Detected)).unwrap();

        let saved = load(dir.path()).unwrap();
        assert_eq!(saved.configs[0].source, ConfigSource::UserFile);
    }

    #[test]
    fn upsert_replaces_rather_than_appending() {
        let dir = tempfile::tempdir().unwrap();
        upsert(dir.path(), config("api:run", "First", ConfigSource::UserFile)).unwrap();
        upsert(dir.path(), config("api:run", "Second", ConfigSource::UserFile)).unwrap();

        let saved = load(dir.path()).unwrap();
        assert_eq!(saved.configs.len(), 1);
        assert_eq!(saved.configs[0].name, "Second");
    }

    #[test]
    fn removes_a_saved_configuration() {
        let dir = tempfile::tempdir().unwrap();
        upsert(dir.path(), config("api:run", "Api", ConfigSource::UserFile)).unwrap();

        assert!(remove(dir.path(), "api:run").unwrap());
        assert!(load(dir.path()).unwrap().configs.is_empty());
        assert!(!remove(dir.path(), "api:run").unwrap(), "removing twice is a no-op");
    }

    #[test]
    fn malformed_configuration_is_an_error_naming_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(config_dir(dir.path())).unwrap();
        std::fs::write(config_path(dir.path()), "{ not json").unwrap();

        let err = load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("config.json"), "got {err}");
    }

    #[test]
    fn results_live_inside_the_config_directory() {
        let root = Path::new("/repo");
        assert_eq!(results_dir(root), Path::new("/repo/.code-basics/results"));
    }
}
