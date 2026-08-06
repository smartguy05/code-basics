//! Importing JetBrains Rider run configurations.
//!
//! # Best effort, by design
//!
//! JetBrains does not publish a stable schema for these files. The element and
//! option names below are what Rider writes in practice, but they vary by
//! configuration type, plugin and version, and nothing guarantees they will
//! keep doing so.
//!
//! So this importer never silently converts. Everything it cannot translate is
//! recorded in [`RunConfig::warnings`], and the UI shows the results as a
//! review step before anything is saved. After import the app's own
//! `.code-basics/config.json` is the source of truth — this is a migration,
//! not a live binding.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::model::{ConfigSource, RunConfig, RunKind};

/// A configuration as it appears in the XML, before translation.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RiderConfiguration {
    pub name: String,
    /// Rider's configuration type, e.g. `DotNetProject`.
    pub kind: String,
    pub options: BTreeMap<String, String>,
    pub env: BTreeMap<String, String>,
    /// npm configurations record the script separately from the options.
    pub scripts: Vec<String>,
    /// Names of the member configurations a compound configuration launches.
    pub to_run: Vec<String>,
}

impl RiderConfiguration {
    fn option(&self, name: &str) -> Option<&str> {
        self.options.get(name).map(String::as_str).filter(|v| !v.is_empty())
    }
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if a.key.local_name().as_ref() == name.as_bytes() {
            a.unescape_value().ok().map(|v| v.into_owned())
        } else {
            None
        }
    })
}

/// Parse a `.run.xml` file into the configurations it declares.
pub fn parse(xml: &str) -> Vec<RiderConfiguration> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut out: Vec<RiderConfiguration> = Vec::new();
    let mut current: Option<RiderConfiguration> = None;

    loop {
        let event = match reader.read_event() {
            Ok(e) => e,
            // A malformed file yields whatever was understood so far rather
            // than nothing at all.
            Err(_) => break,
        };

        match event {
            Event::Start(e) if e.local_name().as_ref() == b"configuration" => {
                current = Some(RiderConfiguration {
                    name: attr(&e, "name").unwrap_or_default(),
                    kind: attr(&e, "type").unwrap_or_default(),
                    ..Default::default()
                });
            }

            // A configuration with no options at all is self-closing, and so
            // never produces the End event that would otherwise record it.
            Event::Empty(e) if e.local_name().as_ref() == b"configuration" => {
                out.push(RiderConfiguration {
                    name: attr(&e, "name").unwrap_or_default(),
                    kind: attr(&e, "type").unwrap_or_default(),
                    ..Default::default()
                });
            }

            // The remaining elements of interest appear in both forms.
            Event::Start(e) | Event::Empty(e) => {
                match e.local_name().as_ref() {
                    b"option" => {
                        if let Some(config) = current.as_mut() {
                            if let (Some(name), Some(value)) = (attr(&e, "name"), attr(&e, "value"))
                            {
                                config.options.insert(name, value);
                            }
                        }
                    }
                    b"env" => {
                        if let Some(config) = current.as_mut() {
                            if let (Some(name), Some(value)) = (attr(&e, "name"), attr(&e, "value"))
                            {
                                config.env.insert(name, value);
                            }
                        }
                    }
                    // npm configurations record these as their own elements.
                    b"script" => {
                        if let Some(config) = current.as_mut() {
                            if let Some(value) = attr(&e, "value") {
                                config.scripts.push(value);
                            }
                        }
                    }
                    b"package-json" => {
                        if let Some(config) = current.as_mut() {
                            if let Some(value) = attr(&e, "value") {
                                config.options.insert("PACKAGE_JSON".into(), value);
                            }
                        }
                    }
                    // Compound configurations list their members this way.
                    b"toRun" => {
                        if let Some(config) = current.as_mut() {
                            if let Some(name) = attr(&e, "name") {
                                config.to_run.push(name);
                            }
                        }
                    }
                    b"command" => {
                        if let Some(config) = current.as_mut() {
                            if let Some(value) = attr(&e, "value") {
                                config.options.insert("NPM_COMMAND".into(), value);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) if e.local_name().as_ref() == b"configuration" => {
                if let Some(config) = current.take() {
                    out.push(config);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    out
}

/// Expand the path macros Rider writes into its configuration files.
///
/// `$PROJECT_DIR$` is by far the most common; the rest appear in
/// hand-edited files and shared team configurations.
pub fn expand_macros(value: &str, workspace_root: &Path) -> String {
    let mut out = value.replace("$PROJECT_DIR$", &workspace_root.display().to_string());

    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        out = out.replace("$USER_HOME$", &home.to_string_lossy());
    }
    out
}

/// Make an absolute path relative to the workspace, so the imported
/// configuration stays portable when checked in.
fn relativise(value: &str, workspace_root: &Path) -> PathBuf {
    let expanded = expand_macros(value, workspace_root);
    let path = PathBuf::from(&expanded);

    path.strip_prefix(workspace_root)
        .map(Path::to_path_buf)
        .unwrap_or(path)
}

fn id_for(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    format!("rider:{}", slug.trim_matches('-'))
}

/// Translate one Rider configuration into ours.
///
/// Returns `None` for configuration types this app cannot launch — Docker
/// Compose, IIS Express, remote debugging and so on — since offering a button
/// that cannot work is worse than omitting it.
pub fn convert(config: &RiderConfiguration, workspace_root: &Path) -> Option<RunConfig> {
    match config.kind.as_str() {
        "DotNetProject" | "DotNetExe" => Some(convert_dotnet(config, workspace_root)),
        // Rider has written both type names for launch-settings profiles.
        "DotNetLaunchSettings" | "LaunchSettings" => {
            Some(convert_launch_settings(config, workspace_root))
        }
        "js.build_tools.npm" => Some(convert_npm(config, workspace_root)),
        "CompoundRunConfigurationType" => Some(convert_compound(config)),
        // Unit test sessions. Matched on a substring rather than a literal
        // because JetBrains publishes no schema and has used more than one
        // name for a single concept before (see the two launch-settings types
        // above); anything containing "UnitTest" is a test session.
        kind if kind.to_ascii_lowercase().contains("unittest") => {
            Some(convert_unit_test(config, workspace_root))
        }
        _ => None,
    }
}

fn base(config: &RiderConfiguration, ecosystem: &str, kind: RunKind) -> RunConfig {
    let mut out = RunConfig::new(
        id_for(&config.name),
        config.name.clone(),
        kind,
        ecosystem,
        ConfigSource::RiderImport,
    );
    out.env = config.env.clone();
    out
}

fn convert_dotnet(config: &RiderConfiguration, root: &Path) -> RunConfig {
    let mut out = base(config, "dotnet", RunKind::App);

    out.project = config.option("PROJECT_PATH").map(|p| relativise(p, root));
    out.framework = config.option("PROJECT_TFM").map(str::to_string);
    out.cwd = config.option("WORKING_DIRECTORY").map(|p| relativise(p, root));

    if let Some(args) = config.option("PROGRAM_PARAMETERS") {
        out.args = crate::adapters::dotnet::split_args(&expand_macros(args, root));
    }

    if out.project.is_none() {
        // Without a project path there is nothing for `dotnet run` to build.
        // Keeping the configuration visible with a warning is more useful
        // than dropping it, since the user can point it at a project.
        out.warnings.push(
            "Rider did not record a project path for this configuration. \
             Choose a project before running it."
                .to_string(),
        );
    }

    if config.option("USE_MONO").is_some_and(|v| v == "1") {
        out.warnings
            .push("This configuration targets Mono, which this app does not launch.".to_string());
    }
    if config.option("USE_EXTERNAL_CONSOLE").is_some_and(|v| v == "1") {
        out.warnings.push(
            "Rider was set to use an external console. Output will appear in this app instead."
                .to_string(),
        );
    }
    if config.option("RUNTIME_ARGUMENTS").is_some() {
        out.warnings.push(
            "Runtime arguments were set in Rider and have not been imported.".to_string(),
        );
    }

    out
}

/// Translate a Rider unit test session into a test configuration.
///
/// Rider stores far more than we can act on — an explicit list of the tests in
/// the session, a filter expression, a target framework per assembly. What
/// transfers is the project to run and the framework to run it under; the
/// session's test selection does not, because a test run here is expressed as
/// "run this project's tests", with filtering applied afterwards from the
/// results tree.
fn convert_unit_test(config: &RiderConfiguration, root: &Path) -> RunConfig {
    let mut out = base(config, "dotnet", RunKind::Test);

    // Rider is inconsistent about which option carries the project, so the
    // known spellings are tried in turn rather than assuming one.
    out.project = ["PROJECT_PATH", "PROJECT_FILE_PATH", "TEST_PROJECT_PATH"]
        .iter()
        .find_map(|key| config.option(key))
        .map(|p| relativise(p, root));
    out.framework = ["PROJECT_TFM", "TFM", "TARGET_FRAMEWORK"]
        .iter()
        .find_map(|key| config.option(key))
        .map(str::to_string);

    if out.project.is_none() {
        out.warnings.push(
            "Rider did not record a project path for this test session. \
             Choose the test project before running it."
                .to_string(),
        );
    }

    // Rider sessions can pin an explicit set of tests. We always run the whole
    // project and let the user re-run failures from the results tree, so say so
    // rather than quietly running more tests than the session named.
    out.warnings.push(
        "Imported as a whole-project test run. Any specific tests this Rider \
         session selected are not carried over — run it, then use \"Re-run failed\" \
         or the filter box to narrow the results."
            .to_string(),
    );

    out
}

fn convert_launch_settings(config: &RiderConfiguration, root: &Path) -> RunConfig {
    let mut out = base(config, "dotnet", RunKind::App);

    out.project = config
        .option("LAUNCH_PROFILE_PROJECT_FILE_PATH")
        .map(|p| relativise(p, root));
    out.launch_profile = config.option("LAUNCH_PROFILE_NAME").map(str::to_string);
    out.framework = config.option("LAUNCH_PROFILE_TFM").map(str::to_string);

    if out.launch_profile.is_none() {
        out.warnings.push(
            "This configuration uses a launch profile, but Rider did not record its name."
                .to_string(),
        );
    }

    out
}

fn convert_npm(config: &RiderConfiguration, root: &Path) -> RunConfig {
    let script = config.scripts.first().cloned();
    let is_test = script.as_deref().is_some_and(|s| s.starts_with("test"));

    let mut out = base(
        config,
        "node",
        if is_test { RunKind::Test } else { RunKind::App },
    );

    // The project is the directory containing package.json.
    out.project = config.option("PACKAGE_JSON").map(|p| {
        let relative = relativise(p, root);
        relative.parent().map(Path::to_path_buf).unwrap_or(relative)
    });
    out.script = script;

    if out.script.is_none() {
        out.warnings
            .push("No npm script was recorded for this configuration.".to_string());
    }
    if config
        .option("NPM_COMMAND")
        .is_some_and(|c| c != "run" && c != "run-script")
    {
        out.warnings.push(format!(
            "Rider ran `npm {}`, which is not a script; imported as a script anyway.",
            config.option("NPM_COMMAND").unwrap_or_default()
        ));
    }

    out
}

fn convert_compound(config: &RiderConfiguration) -> RunConfig {
    let mut out = base(config, "compound", RunKind::App);

    // Members are recorded by Rider display name for now;
    // [`resolve_compounds`] rewrites them into config ids once the full set of
    // available configurations is known.
    out.compound = config.to_run.clone();

    if out.compound.is_empty() {
        out.warnings
            .push("This compound configuration lists nothing to run.".to_string());
    }
    out
}

/// Rewrite compound members from Rider display names into config ids.
///
/// A member is looked up, in order, among the other imported configurations
/// (by Rider name), among `existing` by exact name, and finally — since Rider
/// names launch-profile configurations `Project: profile` while detection
/// names them `Project (profile)` — among `existing` by project file stem and
/// launch profile. Members that resolve nowhere are dropped with a warning, so
/// the review step shows exactly what the compound will launch.
pub fn resolve_compounds(imported: &mut [RunConfig], existing: &[RunConfig]) {
    let by_rider_name: BTreeMap<String, String> = imported
        .iter()
        .filter(|c| c.compound.is_empty())
        .map(|c| (c.name.clone(), c.id.clone()))
        .collect();

    let targets_project = |c: &RunConfig, project: &str| {
        c.project
            .as_deref()
            .and_then(Path::file_stem)
            .is_some_and(|stem| stem == project)
    };

    let resolve = |member: &str| -> Option<String> {
        if let Some(id) = by_rider_name.get(member) {
            return Some(id.clone());
        }
        if let Some(found) = existing.iter().find(|c| c.name == member) {
            return Some(found.id.clone());
        }
        // `Project: profile` — how Rider names launch-profile configurations.
        if let Some((project, profile)) = member.split_once(": ") {
            return existing
                .iter()
                .find(|c| c.launch_profile.as_deref() == Some(profile) && targets_project(c, project))
                .map(|c| c.id.clone());
        }
        // A bare project name — how Rider references a plain project run.
        // Prefer the Debug configuration detection generates for executables.
        existing
            .iter()
            .filter(|c| c.kind == RunKind::App && c.launch_profile.is_none())
            .filter(|c| targets_project(c, member))
            .min_by_key(|c| c.build_configuration.as_deref() != Some("Debug"))
            .map(|c| c.id.clone())
    };

    for config in imported.iter_mut().filter(|c| !c.compound.is_empty()) {
        let mut resolved = Vec::new();
        for member in std::mem::take(&mut config.compound) {
            match resolve(&member) {
                Some(id) => resolved.push(id),
                None => config.warnings.push(format!(
                    "`{member}` could not be matched to any configuration and was dropped. \
                     Import or create it, then re-import this compound."
                )),
            }
        }
        if resolved.is_empty() {
            config
                .warnings
                .push("None of the members resolved, so this launches nothing.".to_string());
        }
        config.compound = resolved;
    }
}

/// The result of scanning a workspace for Rider configurations.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ImportResult {
    /// Configurations translated successfully, some carrying warnings.
    pub configs: Vec<RunConfig>,
    /// Names and types that were recognised but cannot be launched here.
    pub skipped: Vec<(String, String)>,
}

/// Find and convert every Rider configuration in a workspace.
///
/// Reads both `.run/` (the "store as project file" location) and
/// `.idea/**/runConfigurations/` (the older shared location).
pub fn import(workspace_root: &Path) -> ImportResult {
    let mut result = ImportResult::default();

    for dir in candidate_directories(workspace_root) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("xml") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };

            for config in parse(&content) {
                match convert(&config, workspace_root) {
                    Some(converted) => result.configs.push(converted),
                    None => result.skipped.push((config.name, config.kind)),
                }
            }
        }
    }

    result.configs.sort_by(|a, b| a.name.cmp(&b.name));
    result.skipped.sort();
    result
}

fn candidate_directories(root: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![root.join(".run")];

    // .idea may be a directory of projects, so look one level down too.
    let idea = root.join(".idea");
    dirs.push(idea.join("runConfigurations"));

    if let Ok(entries) = std::fs::read_dir(&idea) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                dirs.push(entry.path().join("runConfigurations"));
            }
        }
    }

    dirs
}
