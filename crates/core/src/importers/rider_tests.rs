//! Tests for the Rider importer.
//!
//! The XML samples are shaped the way Rider actually writes them. Since
//! JetBrains publishes no schema, these double as the record of what the
//! importer was written against.

use std::path::{Path, PathBuf};

use super::rider::*;
use crate::model::{ConfigSource, RunKind};

const DOTNET_PROJECT: &str = r#"<component name="ProjectRunConfigurationManager">
  <configuration default="false" name="Api" type="DotNetProject" factoryName=".NET Project">
    <option name="EXE_PATH" value="$PROJECT_DIR$/src/Api/bin/Debug/net8.0/Api.dll" />
    <option name="PROGRAM_PARAMETERS" value="--verbose --name &quot;my api&quot;" />
    <option name="WORKING_DIRECTORY" value="$PROJECT_DIR$/src/Api" />
    <option name="PASS_PARENT_ENVS" value="1" />
    <option name="USE_EXTERNAL_CONSOLE" value="0" />
    <option name="USE_MONO" value="0" />
    <option name="PROJECT_PATH" value="$PROJECT_DIR$/src/Api/Api.csproj" />
    <option name="PROJECT_KIND" value="DotNetCore" />
    <option name="PROJECT_TFM" value="net8.0" />
    <envs>
      <env name="ASPNETCORE_ENVIRONMENT" value="Development" />
      <env name="LOG_LEVEL" value="debug" />
    </envs>
    <method v="2" />
  </configuration>
</component>"#;

const LAUNCH_SETTINGS: &str = r#"<component name="ProjectRunConfigurationManager">
  <configuration default="false" name="Api: https" type="DotNetLaunchSettings" factoryName=".NET Launch Settings Profile">
    <option name="LAUNCH_PROFILE_PROJECT_FILE_PATH" value="$PROJECT_DIR$/src/Api/Api.csproj" />
    <option name="LAUNCH_PROFILE_TFM" value="net8.0" />
    <option name="LAUNCH_PROFILE_NAME" value="https" />
    <option name="USE_EXTERNAL_CONSOLE" value="0" />
    <method v="2" />
  </configuration>
</component>"#;

const NPM: &str = r#"<component name="ProjectRunConfigurationManager">
  <configuration default="false" name="web: dev" type="js.build_tools.npm" nameIsGenerated="true">
    <package-json value="$PROJECT_DIR$/apps/web/package.json" />
    <command value="run" />
    <scripts>
      <script value="dev" />
    </scripts>
    <node-interpreter value="project" />
    <envs>
      <env name="PORT" value="3001" />
    </envs>
    <method v="2" />
  </configuration>
</component>"#;

const DOCKER: &str = r#"<component name="ProjectRunConfigurationManager">
  <configuration default="false" name="compose" type="docker-deploy" factoryName="docker-compose.yml">
    <deployment type="docker-compose.yml" />
    <method v="2" />
  </configuration>
</component>"#;

fn root() -> PathBuf {
    PathBuf::from("/repo")
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

#[test]
fn reads_name_type_options_and_environment() {
    let configs = parse(DOTNET_PROJECT);
    assert_eq!(configs.len(), 1);

    let c = &configs[0];
    assert_eq!(c.name, "Api");
    assert_eq!(c.kind, "DotNetProject");
    assert_eq!(c.options.get("PROJECT_TFM").map(String::as_str), Some("net8.0"));
    assert_eq!(c.env.get("LOG_LEVEL").map(String::as_str), Some("debug"));
}

#[test]
fn reads_npm_script_and_package_json_elements() {
    let configs = parse(NPM);
    let c = &configs[0];

    assert_eq!(c.scripts, vec!["dev"]);
    assert_eq!(
        c.options.get("PACKAGE_JSON").map(String::as_str),
        Some("$PROJECT_DIR$/apps/web/package.json")
    );
    assert_eq!(c.options.get("NPM_COMMAND").map(String::as_str), Some("run"));
}

#[test]
fn reads_several_configurations_from_one_file() {
    let combined = format!(
        "<component name=\"ProjectRunConfigurationManager\">{}{}</component>",
        DOTNET_PROJECT
            .trim_start_matches("<component name=\"ProjectRunConfigurationManager\">")
            .trim_end_matches("</component>"),
        LAUNCH_SETTINGS
            .trim_start_matches("<component name=\"ProjectRunConfigurationManager\">")
            .trim_end_matches("</component>"),
    );
    assert_eq!(parse(&combined).len(), 2);
}

#[test]
fn malformed_xml_yields_whatever_was_understood() {
    // A truncated file should still surface the configurations it did contain
    // rather than importing nothing.
    let truncated = DOTNET_PROJECT.trim_end_matches("</component>");
    let configs = parse(truncated);
    assert_eq!(configs.len(), 1);
}

#[test]
fn an_empty_file_yields_nothing() {
    assert!(parse("").is_empty());
    assert!(parse("<component />").is_empty());
}

// ---------------------------------------------------------------------------
// Macro expansion
// ---------------------------------------------------------------------------

#[test]
fn expands_the_project_directory_macro() {
    assert_eq!(
        expand_macros("$PROJECT_DIR$/src/Api", Path::new("/repo")),
        "/repo/src/Api"
    );
}

#[test]
fn leaves_unknown_text_alone() {
    assert_eq!(expand_macros("plain/path", Path::new("/repo")), "plain/path");
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

#[test]
fn converts_a_dotnet_project_configuration() {
    let parsed = parse(DOTNET_PROJECT);
    let config = convert(&parsed[0], &root()).unwrap();

    assert_eq!(config.name, "Api");
    assert_eq!(config.kind, RunKind::App);
    assert_eq!(config.ecosystem, "dotnet");
    assert_eq!(config.source, ConfigSource::RiderImport);
    assert_eq!(config.framework.as_deref(), Some("net8.0"));
    assert_eq!(config.env.get("ASPNETCORE_ENVIRONMENT").map(String::as_str), Some("Development"));
}

#[test]
fn stores_paths_relative_to_the_workspace_so_they_stay_portable() {
    // An absolute path would break for every other person on the team.
    let parsed = parse(DOTNET_PROJECT);
    let config = convert(&parsed[0], &root()).unwrap();

    assert_eq!(config.project, Some(PathBuf::from("src/Api/Api.csproj")));
    assert_eq!(config.cwd, Some(PathBuf::from("src/Api")));
}

#[test]
fn splits_quoted_program_arguments() {
    let parsed = parse(DOTNET_PROJECT);
    let config = convert(&parsed[0], &root()).unwrap();

    assert_eq!(config.args, vec!["--verbose", "--name", "my api"]);
}

#[test]
fn converts_a_launch_settings_configuration() {
    let parsed = parse(LAUNCH_SETTINGS);
    let config = convert(&parsed[0], &root()).unwrap();

    assert_eq!(config.launch_profile.as_deref(), Some("https"));
    assert_eq!(config.project, Some(PathBuf::from("src/Api/Api.csproj")));
    assert!(config.warnings.is_empty());
}

#[test]
fn converts_an_npm_configuration_to_its_package_directory() {
    let parsed = parse(NPM);
    let config = convert(&parsed[0], &root()).unwrap();

    assert_eq!(config.ecosystem, "node");
    assert_eq!(config.script.as_deref(), Some("dev"));
    // The project is the directory, not the package.json itself.
    assert_eq!(config.project, Some(PathBuf::from("apps/web")));
    assert_eq!(config.env.get("PORT").map(String::as_str), Some("3001"));
}

#[test]
fn an_npm_test_script_becomes_a_test_configuration() {
    let xml = NPM.replace(r#"<script value="dev" />"#, r#"<script value="test" />"#);
    let parsed = parse(&xml);
    let config = convert(&parsed[0], &root()).unwrap();

    assert_eq!(config.kind, RunKind::Test);
}

#[test]
fn configuration_types_this_app_cannot_launch_are_not_converted() {
    // Offering a button that cannot work is worse than omitting it.
    let parsed = parse(DOCKER);
    assert!(convert(&parsed[0], &root()).is_none());
}

// ---------------------------------------------------------------------------
// Warnings — the review step depends on these
// ---------------------------------------------------------------------------

#[test]
fn warns_when_no_project_path_was_recorded() {
    let xml = DOTNET_PROJECT.replace(
        r#"<option name="PROJECT_PATH" value="$PROJECT_DIR$/src/Api/Api.csproj" />"#,
        "",
    );
    let parsed = parse(&xml);
    let config = convert(&parsed[0], &root()).unwrap();

    assert!(config.project.is_none());
    assert!(config.warnings.iter().any(|w| w.contains("project path")));
}

#[test]
fn warns_about_mono_configurations() {
    let xml = DOTNET_PROJECT.replace(
        r#"<option name="USE_MONO" value="0" />"#,
        r#"<option name="USE_MONO" value="1" />"#,
    );
    let parsed = parse(&xml);
    let config = convert(&parsed[0], &root()).unwrap();

    assert!(config.warnings.iter().any(|w| w.contains("Mono")));
}

#[test]
fn warns_when_rider_used_an_external_console() {
    let xml = DOTNET_PROJECT.replace(
        r#"<option name="USE_EXTERNAL_CONSOLE" value="0" />"#,
        r#"<option name="USE_EXTERNAL_CONSOLE" value="1" />"#,
    );
    let parsed = parse(&xml);
    let config = convert(&parsed[0], &root()).unwrap();

    assert!(config.warnings.iter().any(|w| w.contains("external console")));
}

#[test]
fn a_clean_configuration_imports_without_warnings() {
    let parsed = parse(DOTNET_PROJECT);
    let config = convert(&parsed[0], &root()).unwrap();

    assert!(config.warnings.is_empty(), "got {:?}", config.warnings);
}

// ---------------------------------------------------------------------------
// Scanning a workspace
// ---------------------------------------------------------------------------

fn workspace_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, contents).unwrap();
    }
    dir
}

#[test]
fn imports_from_the_run_directory() {
    let dir = workspace_with(&[(".run/Api.run.xml", DOTNET_PROJECT)]);
    let result = import(dir.path());

    assert_eq!(result.configs.len(), 1);
    assert_eq!(result.configs[0].name, "Api");
}

#[test]
fn imports_from_the_idea_run_configurations_directory() {
    let dir = workspace_with(&[(".idea/runConfigurations/Api.xml", DOTNET_PROJECT)]);
    let result = import(dir.path());

    assert_eq!(result.configs.len(), 1);
}

#[test]
fn imports_from_a_nested_idea_project_directory() {
    // Rider nests these under .idea/<solution>.idea/ in some layouts.
    let dir = workspace_with(&[(".idea/.idea.Api/runConfigurations/Api.xml", DOTNET_PROJECT)]);
    let result = import(dir.path());

    assert_eq!(result.configs.len(), 1);
}

#[test]
fn records_what_it_skipped_so_the_user_knows_it_was_seen() {
    let dir = workspace_with(&[
        (".run/Api.run.xml", DOTNET_PROJECT),
        (".run/compose.run.xml", DOCKER),
    ]);
    let result = import(dir.path());

    assert_eq!(result.configs.len(), 1);
    assert_eq!(result.skipped, vec![("compose".to_string(), "docker-deploy".to_string())]);
}

#[test]
fn a_workspace_with_no_rider_configurations_imports_nothing() {
    let dir = workspace_with(&[("src/main.rs", "fn main() {}")]);
    let result = import(dir.path());

    assert!(result.configs.is_empty());
    assert!(result.skipped.is_empty());
}

#[test]
fn non_xml_files_are_ignored() {
    let dir = workspace_with(&[(".run/README.md", "not a configuration")]);
    assert!(import(dir.path()).configs.is_empty());
}

#[test]
fn imported_configurations_are_ordered_by_name() {
    let dir = workspace_with(&[
        (".run/z.run.xml", &DOTNET_PROJECT.replace(r#"name="Api""#, r#"name="Zebra""#)),
        (".run/a.run.xml", &DOTNET_PROJECT.replace(r#"name="Api""#, r#"name="Alpha""#)),
    ]);
    let result = import(dir.path());

    let names: Vec<&str> = result.configs.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["Alpha", "Zebra"]);
}

#[test]
fn imported_ids_are_distinct_per_configuration_name() {
    let dir = workspace_with(&[
        (".run/a.run.xml", &DOTNET_PROJECT.replace(r#"name="Api""#, r#"name="Api: http""#)),
        (".run/b.run.xml", &DOTNET_PROJECT.replace(r#"name="Api""#, r#"name="Api: https""#)),
    ]);
    let result = import(dir.path());

    assert_ne!(result.configs[0].id, result.configs[1].id);
}
