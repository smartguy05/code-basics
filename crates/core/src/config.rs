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

use crate::inspect::model::{Caps, InspectorConfig};
use crate::model::{ConfigSource, RunConfig};
use crate::workspace::Workspace;

/// Directory holding this app's per-workspace state.
pub const CONFIG_DIR: &str = ".code-basics";
pub const CONFIG_FILE: &str = "config.json";
/// Where test report files are written. Inside the config directory so a
/// single `.gitignore` entry covers everything.
pub const RESULTS_DIR: &str = "results";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConfig {
    /// Schema version, so a future format change can migrate rather than fail.
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub configs: Vec<RunConfig>,
    /// Ids of configurations the user starred. Favourites sort before
    /// everything else in the UI.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub favorites: Vec<String>,
    /// Preferred ordering, as config ids. Ids listed here sort by their
    /// position; anything unlisted keeps its name order after them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<String>,
    /// Evaluate .NET projects with `dotnet msbuild -getProperty` during a scan
    /// instead of reading them as XML.
    ///
    /// Off by default: it costs a process launch per project and needs the SDK
    /// installed. Worth turning on for repositories that set properties behind
    /// MSBuild conditions or in imported `.props` files, which the XML scan
    /// cannot see. See `crate::adapters::msbuild`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub msbuild_evaluation: bool,
    /// Object-inspector settings for this workspace.
    ///
    /// Absent unless the user configured something, the same way
    /// `msbuild_evaluation` stays out of the file until it is turned on. The
    /// section is opt-in because it is the only thing that can enable crash
    /// dump capture, and a dump is a verbatim copy of process memory —
    /// connection strings, tokens, customer records — landing in a directory
    /// next to a repository this file is shared through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inspector: Option<InspectorConfig>,
}

fn default_version() -> u32 {
    1
}

/// How many crash dumps to keep when the workspace has not said otherwise.
///
/// Three is enough to compare a repeated crash against its two predecessors,
/// which is the case that actually needs history. The count alone cannot bound
/// disk use, though: a dump of a trivial console app measured 9.3 MB and a real
/// application's runs to hundreds of megabytes, so the byte budget below is the
/// limit that genuinely binds.
pub const DEFAULT_KEEP_DUMPS: u32 = 3;

/// Total size budget for the dumps directory, in megabytes.
///
/// 2 GB is small enough to be an unremarkable amount of a development machine's
/// disk, and large enough to hold three dumps of a mid-sized service. For any
/// substantial application this is what prunes first — three 700 MB dumps
/// already exceed it — which is the intended ordering: bytes are the resource
/// that runs out, not files.
pub const DEFAULT_MAX_DUMP_MEGABYTES: u64 = 2048;

impl WorkspaceConfig {
    /// Whether this workspace has opted into writing crash dumps.
    ///
    /// False whenever the section is absent. Nothing infers this from anything
    /// else: capture only happens because someone asked for it in writing.
    pub fn dump_capture_enabled(&self) -> bool {
        self.inspector.as_ref().is_some_and(|i| i.capture_dumps)
    }

    /// The limits to walk an object graph under: whatever the workspace
    /// configured, otherwise the built-in defaults.
    pub fn inspector_caps(&self) -> Caps {
        self.inspector
            .as_ref()
            .and_then(|i| i.caps)
            .unwrap_or_default()
    }

    /// How many dumps to retain, newest first.
    pub fn keep_dumps(&self) -> u32 {
        self.inspector
            .as_ref()
            .and_then(|i| i.keep_dumps)
            .unwrap_or(DEFAULT_KEEP_DUMPS)
    }

    /// The dumps directory's size budget, in megabytes.
    pub fn max_dump_megabytes(&self) -> u64 {
        self.inspector
            .as_ref()
            .and_then(|i| i.max_dump_megabytes)
            .unwrap_or(DEFAULT_MAX_DUMP_MEGABYTES)
    }
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            version: default_version(),
            configs: Vec::new(),
            favorites: Vec::new(),
            order: Vec::new(),
            msbuild_evaluation: false,
            inspector: None,
        }
    }
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
        return Ok(WorkspaceConfig::default());
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    serde_json::from_str(&content)
        .with_context(|| format!("{} is not valid configuration JSON", path.display()))
}

/// Everything under `.code-basics/` that must never be committed.
///
/// `results/` is regenerated on every run; `changelists.json` records one
/// person's work-in-progress grouping and would be noise for everyone else.
/// `intents/` is a log of what a coding agent did during one person's session,
/// which is both personal and large.
const IGNORED: &[&str] = &[
    "results/",
    crate::changelists::CHANGELISTS_FILE,
    "intents/",
    // Object captures: regenerated on demand, full of one machine's absolute
    // paths, and a verbatim copy of whatever data the process was holding.
    "inspect/",
    // Crash dumps are process memory — connection strings, tokens, whatever
    // the application had in flight. These must never reach a shared history.
    "dumps/",
];

/// Make sure `.code-basics/.gitignore` lists everything local.
///
/// Entries are appended rather than the file rewritten: a workspace created
/// before an entry existed still needs it, and anything the user added by hand
/// has to survive.
pub fn ensure_gitignore(dir: &Path) -> Result<()> {
    let path = dir.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let missing: Vec<&str> = IGNORED
        .iter()
        .copied()
        .filter(|entry| !existing.lines().any(|line| line.trim() == *entry))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    for entry in missing {
        updated.push_str(entry);
        updated.push('\n');
    }

    std::fs::write(&path, updated).with_context(|| format!("failed to write {}", path.display()))
}

/// Write the configuration file, creating the directory if needed.
pub fn save(root: &Path, config: &WorkspaceConfig) -> Result<()> {
    let dir = config_dir(root);
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;

    ensure_gitignore(&dir)?;

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

/// Layer a saved configuration file onto a freshly scanned workspace: merge
/// the configs, carry over favourites and ordering, and sort the result the
/// way the UI lists it.
pub fn apply(workspace: &mut Workspace, saved: WorkspaceConfig) {
    workspace.configs = merge(std::mem::take(&mut workspace.configs), saved.configs);
    sort_configs(&mut workspace.configs, &saved.favorites, &saved.order);
    workspace.favorites = saved.favorites;
    workspace.order = saved.order;
}

/// Sort configurations the way the UI lists them: favourites first, then by
/// position in the saved order, then by name for anything the user never
/// arranged. The sort is stable, so ids missing from both lists keep the name
/// order [`merge`] established.
pub fn sort_configs(configs: &mut [RunConfig], favorites: &[String], order: &[String]) {
    let position = |id: &str| order.iter().position(|o| o == id).unwrap_or(usize::MAX);
    let favorite = |id: &str| !favorites.iter().any(|f| f == id);

    configs.sort_by(|a, b| {
        favorite(&a.id)
            .cmp(&favorite(&b.id))
            .then(position(&a.id).cmp(&position(&b.id)))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.id.cmp(&b.id))
    });
}

/// Star or unstar a configuration and persist the change.
pub fn set_favorite(root: &Path, id: &str, favorite: bool) -> Result<WorkspaceConfig> {
    let mut file = load(root)?;
    file.favorites.retain(|f| f != id);
    if favorite {
        file.favorites.push(id.to_string());
    }
    save(root, &file)?;
    Ok(file)
}

/// Replace the preferred ordering and persist it.
pub fn set_order(root: &Path, order: Vec<String>) -> Result<WorkspaceConfig> {
    let mut file = load(root)?;
    file.order = order;
    save(root, &file)?;
    Ok(file)
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
    use crate::model::{RunConfig, RunKind};

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
        let mut file = WorkspaceConfig::default();
        let mut c = config("api:run", "Api", ConfigSource::UserFile);
        c.args = vec!["--verbose".into()];
        c.env.insert("KEY".into(), "value".into());
        file.configs.push(c);

        save(dir.path(), &file).unwrap();
        let reloaded = load(dir.path()).unwrap();

        assert_eq!(reloaded.configs.len(), 1);
        assert_eq!(reloaded.configs[0].args, vec!["--verbose"]);
        assert_eq!(
            reloaded.configs[0].env.get("KEY").map(String::as_str),
            Some("value")
        );
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

    // -- .gitignore, which is written into a directory the user shares -------

    #[test]
    fn the_ignore_file_is_created_listing_everything_local() {
        let dir = tempfile::tempdir().unwrap();
        ensure_gitignore(dir.path()).unwrap();

        let ignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        for entry in IGNORED {
            assert!(
                ignore.lines().any(|l| l.trim() == *entry),
                "{entry} missing from: {ignore}"
            );
        }
    }

    /// A dump is a verbatim copy of process memory and a changelist is one
    /// person's work in progress. Neither may ever reach a shared history, so
    /// the exact entries are pinned rather than merely counted.
    #[test]
    fn the_ignore_file_covers_dumps_captures_and_changelists_by_name() {
        let dir = tempfile::tempdir().unwrap();
        ensure_gitignore(dir.path()).unwrap();

        let ignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        let lines: Vec<&str> = ignore.lines().map(str::trim).collect();

        assert!(lines.contains(&"dumps/"));
        assert!(lines.contains(&"inspect/"));
        assert!(lines.contains(&"intents/"));
        assert!(lines.contains(&"results/"));
        assert!(lines.contains(&crate::changelists::CHANGELISTS_FILE));
    }

    /// Entries are appended, never rewritten: anything a person added by hand
    /// has to survive.
    #[test]
    fn hand_written_ignore_entries_survive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".gitignore");
        std::fs::write(&path, "# mine\nscratch/\n").unwrap();

        ensure_gitignore(dir.path()).unwrap();

        let ignore = std::fs::read_to_string(&path).unwrap();
        assert!(ignore.contains("# mine"));
        assert!(ignore.lines().any(|l| l.trim() == "scratch/"));
        assert!(ignore.lines().any(|l| l.trim() == "dumps/"));
    }

    /// A workspace created before an entry existed still needs it.
    #[test]
    fn only_the_missing_entries_are_added() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".gitignore");
        std::fs::write(&path, "results/\n").unwrap();

        ensure_gitignore(dir.path()).unwrap();

        let ignore = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            ignore.lines().filter(|l| l.trim() == "results/").count(),
            1,
            "an entry already present must not be duplicated: {ignore}"
        );
        assert!(ignore.lines().any(|l| l.trim() == "intents/"));
    }

    #[test]
    fn ensuring_the_ignore_file_twice_changes_nothing_the_second_time() {
        let dir = tempfile::tempdir().unwrap();
        ensure_gitignore(dir.path()).unwrap();
        let first = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();

        ensure_gitignore(dir.path()).unwrap();
        let second = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();

        assert_eq!(first, second);
    }

    /// Appending to a file whose last line has no newline must not glue two
    /// entries into one unusable pattern.
    #[test]
    fn an_ignore_file_with_no_trailing_newline_is_not_run_together() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".gitignore");
        std::fs::write(&path, "scratch/").unwrap();

        ensure_gitignore(dir.path()).unwrap();

        let ignore = std::fs::read_to_string(&path).unwrap();
        assert!(
            ignore.lines().any(|l| l.trim() == "scratch/"),
            "got: {ignore}"
        );
        assert!(ignore.lines().any(|l| l.trim() == "results/"));
        assert!(!ignore.contains("scratch/results/"), "got: {ignore}");
    }

    #[test]
    fn a_saved_configuration_replaces_a_detected_one_with_the_same_id() {
        let detected = vec![
            config("api:run", "Api (detected)", ConfigSource::Detected),
            config("web:run", "Web", ConfigSource::Detected),
        ];
        let saved = vec![config(
            "api:run",
            "Api (customised)",
            ConfigSource::UserFile,
        )];

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
        upsert(
            dir.path(),
            config("api:run", "First", ConfigSource::UserFile),
        )
        .unwrap();
        upsert(
            dir.path(),
            config("api:run", "Second", ConfigSource::UserFile),
        )
        .unwrap();

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
        assert!(
            !remove(dir.path(), "api:run").unwrap(),
            "removing twice is a no-op"
        );
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
    fn favourites_sort_first_then_the_saved_order_then_names() {
        let mut configs = vec![
            config("a", "Alpha", ConfigSource::Detected),
            config("b", "Beta", ConfigSource::Detected),
            config("c", "Gamma", ConfigSource::Detected),
            config("d", "Delta", ConfigSource::Detected),
        ];

        let favorites = vec!["c".to_string()];
        let order = vec!["b".to_string(), "a".to_string()];
        sort_configs(&mut configs, &favorites, &order);

        let ids: Vec<&str> = configs.iter().map(|c| c.id.as_str()).collect();
        // c is starred; b before a per the order; d never arranged, so last.
        assert_eq!(ids, ["c", "b", "a", "d"]);
    }

    #[test]
    fn favourites_respect_the_saved_order_among_themselves() {
        let mut configs = vec![
            config("a", "Alpha", ConfigSource::Detected),
            config("b", "Beta", ConfigSource::Detected),
        ];

        let favorites = vec!["a".to_string(), "b".to_string()];
        let order = vec!["b".to_string(), "a".to_string()];
        sort_configs(&mut configs, &favorites, &order);

        let ids: Vec<&str> = configs.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            ["b", "a"],
            "the order list decides ties between favourites"
        );
    }

    #[test]
    fn set_favorite_toggles_and_persists() {
        let dir = tempfile::tempdir().unwrap();

        let saved = set_favorite(dir.path(), "api:run", true).unwrap();
        assert_eq!(saved.favorites, ["api:run"]);
        assert_eq!(load(dir.path()).unwrap().favorites, ["api:run"]);

        let saved = set_favorite(dir.path(), "api:run", false).unwrap();
        assert!(saved.favorites.is_empty());
    }

    #[test]
    fn starring_twice_does_not_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        set_favorite(dir.path(), "api:run", true).unwrap();
        let saved = set_favorite(dir.path(), "api:run", true).unwrap();

        assert_eq!(saved.favorites, ["api:run"]);
    }

    #[test]
    fn set_order_replaces_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        set_order(dir.path(), vec!["a".into(), "b".into()]).unwrap();
        set_order(dir.path(), vec!["b".into(), "a".into()]).unwrap();

        assert_eq!(load(dir.path()).unwrap().order, ["b", "a"]);
    }

    #[test]
    fn empty_favourites_and_order_stay_out_of_the_file() {
        // The config file is checked in; noise keys would show up in diffs.
        let json = serde_json::to_value(WorkspaceConfig::default()).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort();

        assert_eq!(keys, ["configs", "version"]);
    }

    #[test]
    fn an_untouched_inspector_section_stays_out_of_the_file() {
        // Same reason as favourites — but with teeth: this file is shared with
        // the team, and a `captureDumps` key appearing in it as a side effect
        // of saving a run configuration would be an invitation to flip it on.
        let dir = tempfile::tempdir().unwrap();
        upsert(dir.path(), config("api:run", "Api", ConfigSource::UserFile)).unwrap();

        let written = std::fs::read_to_string(config_path(dir.path())).unwrap();
        assert!(!written.contains("inspector"), "got {written}");
        assert!(!written.contains("captureDumps"), "got {written}");
    }

    #[test]
    fn a_config_without_an_inspector_section_has_capture_disabled() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(config_dir(dir.path())).unwrap();
        std::fs::write(config_path(dir.path()), r#"{"version":1,"configs":[]}"#).unwrap();

        let loaded = load(dir.path()).unwrap();
        assert!(loaded.inspector.is_none());
        assert!(!loaded.dump_capture_enabled());
        assert_eq!(loaded.inspector_caps(), Caps::default());
        assert_eq!(loaded.keep_dumps(), DEFAULT_KEEP_DUMPS);
        assert_eq!(loaded.max_dump_megabytes(), DEFAULT_MAX_DUMP_MEGABYTES);
    }

    #[test]
    fn an_inspector_section_present_but_not_opted_in_still_disables_capture() {
        // Configuring caps or retention must not imply consent to dump memory.
        let file = WorkspaceConfig {
            inspector: Some(InspectorConfig {
                keep_dumps: Some(10),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(!file.dump_capture_enabled());
        assert_eq!(file.keep_dumps(), 10);
    }

    #[test]
    fn inspector_settings_survive_a_save_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let file = WorkspaceConfig {
            inspector: Some(InspectorConfig {
                capture_dumps: true,
                caps: Some(Caps {
                    max_depth: 2,
                    ..Caps::default()
                }),
                max_dump_megabytes: Some(512),
                ..Default::default()
            }),
            ..Default::default()
        };
        save(dir.path(), &file).unwrap();

        let reloaded = load(dir.path()).unwrap();
        assert!(reloaded.dump_capture_enabled());
        assert_eq!(reloaded.inspector_caps().max_depth, 2);
        assert_eq!(reloaded.max_dump_megabytes(), 512);
        // Unset within a present section still falls back, not to zero.
        assert_eq!(reloaded.keep_dumps(), DEFAULT_KEEP_DUMPS);
    }

    #[test]
    fn a_partial_caps_section_does_not_stop_the_workspace_opening() {
        // `load` failing is not a local failure: `open_workspace` propagates it,
        // so a hand-written subset of the limits would lock the user out of the
        // repository until they edited the JSON back.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(config_dir(dir.path())).unwrap();
        std::fs::write(
            config_path(dir.path()),
            r#"{"version":1,"configs":[],"inspector":{"caps":{"maxDepth":2}}}"#,
        )
        .unwrap();

        let loaded = load(dir.path()).expect("a partial caps section must still load");

        assert_eq!(loaded.inspector_caps().max_depth, 2);
        // The keys that were not written keep the built-in defaults rather than
        // collapsing to zero, which would elide everything.
        assert_eq!(
            loaded.inspector_caps().max_children,
            Caps::default().max_children
        );
        assert_eq!(loaded.inspector_caps().max_nodes, Caps::default().max_nodes);
    }

    #[test]
    fn results_live_inside_the_config_directory() {
        let root = Path::new("/repo");
        assert_eq!(results_dir(root), Path::new("/repo/.code-basics/results"));
    }
}
