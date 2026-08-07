//! Tests for optional MSBuild evaluation.
//!
//! The parsing and merging tests run everywhere. The one test that actually
//! launches the SDK skips itself when `dotnet` is absent, so the suite stays
//! runnable on a machine with no .NET installed.

use std::collections::BTreeMap;
use std::path::Path;

use super::dotnet::{parse_project_file, ProjectFile};
use super::msbuild::*;

/// The exact document `dotnet msbuild -getProperty:A -getProperty:B` writes,
/// captured from the .NET 10 SDK.
const OUTPUT: &str = r#"{
  "Properties": {
    "OutputType": "Exe",
    "TargetFramework": "net10.0",
    "TargetFrameworks": "",
    "Configurations": "Debug;Release;Staging",
    "IsTestProject": "true",
    "UserSecretsId": "",
    "UseMaui": "",
    "IsAspireHost": "",
    "TestingPlatformDotnetTestSupport": "",
    "EnableMSTestRunner": ""
  }
}"#;

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn requests_every_property_the_shallow_scan_reads() {
    let args = command_args(Path::new("/repo/src/App/App.csproj"));

    assert_eq!(args[0], "msbuild");
    assert!(args.iter().any(|a| a.contains("App.csproj")));
    for property in PROPERTIES {
        assert!(
            args.iter()
                .any(|a| a == &format!("-getProperty:{property}")),
            "{property} must be requested"
        );
    }
}

#[test]
fn asks_for_more_than_one_property_so_the_sdk_emits_json() {
    // A single `-getProperty` prints a bare value instead of a JSON document,
    // and `parse_output` only understands the document.
    let requested = command_args(Path::new("a.csproj"))
        .iter()
        .filter(|a| a.starts_with("-getProperty:"))
        .count();
    assert!(requested > 1);
}

#[test]
fn reads_the_property_document() {
    let properties = parse_output(OUTPUT);

    assert_eq!(
        properties.get("OutputType").map(String::as_str),
        Some("Exe")
    );
    assert_eq!(
        properties.get("TargetFramework").map(String::as_str),
        Some("net10.0")
    );
}

#[test]
fn unset_properties_are_dropped_rather_than_stored_empty() {
    // MSBuild reports an unset property as "", which must not overwrite a
    // value the XML scan did find.
    let properties = parse_output(OUTPUT);
    assert!(!properties.contains_key("UserSecretsId"));
    assert!(!properties.contains_key("TargetFrameworks"));
}

#[test]
fn unparseable_output_yields_nothing() {
    assert!(parse_output("").is_empty());
    assert!(
        parse_output("Exe").is_empty(),
        "a bare single-property value is not a document"
    );
    assert!(parse_output(r#"{"NotProperties": {}}"#).is_empty());
}

#[test]
fn evaluated_properties_win_over_the_shallow_scan() {
    // The whole point: a value behind an MSBuild condition is invisible to the
    // XML scan, which takes the last literal it sees.
    let mut project = parse_project_file(
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup>
             <OutputType>Library</OutputType>
             <TargetFramework>net6.0</TargetFramework>
           </PropertyGroup></Project>"#,
    );
    assert_eq!(project.output_type.as_deref(), Some("Library"));

    apply(&mut project, &parse_output(OUTPUT));

    assert_eq!(project.output_type.as_deref(), Some("Exe"));
    assert_eq!(project.target_frameworks, vec!["net10.0"]);
    assert_eq!(project.configurations, vec!["Debug", "Release", "Staging"]);
    assert_eq!(project.is_test_project, Some(true));
}

#[test]
fn multi_targeting_wins_over_the_single_framework_property() {
    // A multi-targeted project reports both, with TargetFramework holding
    // whichever one this evaluation happened to pick.
    let mut project = ProjectFile::default();
    apply(
        &mut project,
        &map(&[
            ("TargetFramework", "net8.0"),
            ("TargetFrameworks", "net8.0;net9.0"),
        ]),
    );

    assert_eq!(project.target_frameworks, vec!["net8.0", "net9.0"]);
}

#[test]
fn package_references_survive_evaluation() {
    // `-getProperty` returns properties, not items, so the package list can
    // only ever come from the XML — and runner classification depends on it.
    let mut project = parse_project_file(
        r#"<Project Sdk="Microsoft.NET.Sdk"><ItemGroup>
             <PackageReference Include="xunit.v3" Version="1.0.0" />
           </ItemGroup></Project>"#,
    );

    apply(&mut project, &parse_output(OUTPUT));

    assert_eq!(project.package_references, vec!["xunit.v3"]);
}

#[test]
fn an_empty_evaluation_changes_nothing() {
    let original = parse_project_file(
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup>
             <OutputType>Exe</OutputType>
           </PropertyGroup></Project>"#,
    );
    let mut project = original.clone();

    apply(&mut project, &BTreeMap::new());

    assert_eq!(project, original);
}

#[test]
fn evaluating_a_project_that_does_not_exist_fails_softly() {
    // A workspace that opts into evaluation must still open when a project
    // cannot be evaluated.
    assert!(evaluate(Path::new("/nonexistent/App.csproj")).is_none());
}

#[test]
fn evaluates_a_real_project_when_the_sdk_is_available() {
    let Ok(probe) = std::process::Command::new("dotnet")
        .arg("--version")
        .output()
    else {
        eprintln!("skipped: no dotnet on PATH");
        return;
    };
    if !probe.status.success() {
        eprintln!("skipped: dotnet is not usable");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("App.csproj");
    std::fs::write(
        &project,
        r#"<Project Sdk="Microsoft.NET.Sdk">
             <PropertyGroup>
               <TargetFramework>net8.0</TargetFramework>
               <Configurations>Debug;Release;Staging</Configurations>
             </PropertyGroup>
             <!-- Invisible to the XML scan: only a real evaluation resolves it. -->
             <PropertyGroup Condition="'$(Configuration)' == 'Debug'">
               <OutputType>Exe</OutputType>
             </PropertyGroup>
           </Project>"#,
    )
    .unwrap();

    let Some(properties) = evaluate(&project) else {
        eprintln!("skipped: the SDK could not evaluate the probe project");
        return;
    };

    assert_eq!(
        properties.get("Configurations").map(String::as_str),
        Some("Debug;Release;Staging")
    );
    assert_eq!(
        properties.get("OutputType").map(String::as_str),
        Some("Exe"),
        "a condition the XML scan cannot evaluate must resolve here"
    );
}
