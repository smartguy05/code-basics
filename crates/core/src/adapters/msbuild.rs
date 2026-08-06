//! Optional, accurate MSBuild evaluation.
//!
//! Project detection is normally a shallow XML scan (see
//! [`crate::adapters::dotnet::parse_project_file`]), which keeps opening a
//! workspace instant and works with no SDK installed. The cost is that MSBuild
//! itself is never run, so `Condition="..."`, `$(Property)` references and
//! imported `.props` files other than `Directory.Build.*` are invisible — the
//! shallow scan simply takes the last literal value it sees.
//!
//! For workspaces where that is not good enough, this module asks the SDK for
//! the real answer:
//!
//! ```text
//! dotnet msbuild <project> -getProperty:OutputType -getProperty:TargetFramework ...
//! ```
//!
//! Requesting more than one property makes the SDK emit JSON, which is why the
//! full set is always requested even when one value would do.
//!
//! This is opt-in per workspace (`msbuildEvaluation` in
//! `.code-basics/config.json`) because it is *slow*: one process launch per
//! project, each of which evaluates imports and may restore. Every failure
//! mode — no SDK, an unrestorable project, a timeout — degrades back to the
//! shallow scan rather than failing the workspace.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use crate::adapters::dotnet::ProjectFile;

/// The properties worth asking MSBuild for: exactly those the shallow scan
/// reads, so the two can be compared and merged field for field.
pub const PROPERTIES: &[&str] = &[
    "OutputType",
    "TargetFramework",
    "TargetFrameworks",
    "Configurations",
    "IsTestProject",
    "UserSecretsId",
    "UseMaui",
    "IsAspireHost",
    "TestingPlatformDotnetTestSupport",
    "EnableMSTestRunner",
];

/// The command line used to evaluate a project.
///
/// Split out so it can be asserted on without running the SDK.
pub fn command_args(project: &Path) -> Vec<String> {
    let mut args = vec![
        "msbuild".to_string(),
        project.display().to_string(),
        "-nologo".to_string(),
    ];
    args.extend(PROPERTIES.iter().map(|p| format!("-getProperty:{p}")));
    args
}

/// Read the `{"Properties": {...}}` document `-getProperty` writes.
///
/// Empty values are dropped: MSBuild reports an unset property as `""`, which
/// must not overwrite something the shallow scan did find.
pub fn parse_output(stdout: &str) -> BTreeMap<String, String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return BTreeMap::new();
    };
    let Some(properties) = value.get("Properties").and_then(|p| p.as_object()) else {
        return BTreeMap::new();
    };

    properties
        .iter()
        .filter_map(|(key, value)| {
            let text = value.as_str()?.trim();
            (!text.is_empty()).then(|| (key.clone(), text.to_string()))
        })
        .collect()
}

/// Overlay evaluated properties onto a shallow-scanned project.
///
/// MSBuild's answer wins wherever it has one, because it accounts for the
/// conditions and imports the scan cannot see. Package references are left
/// alone: they are items rather than properties, and `-getProperty` does not
/// return them.
pub fn apply(project: &mut ProjectFile, evaluated: &BTreeMap<String, String>) {
    let truthy = |key: &str| evaluated.get(key).map(|v| v.eq_ignore_ascii_case("true"));
    let list = |raw: &str| -> Vec<String> {
        raw.split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    };

    if let Some(output_type) = evaluated.get("OutputType") {
        project.output_type = Some(output_type.clone());
    }
    if let Some(user_secrets) = evaluated.get("UserSecretsId") {
        project.user_secrets_id = Some(user_secrets.clone());
    }
    if let Some(configurations) = evaluated.get("Configurations") {
        project.configurations = list(configurations);
    }

    // `TargetFrameworks` wins over `TargetFramework`: a multi-targeted project
    // reports both, with the singular holding whichever one this evaluation
    // happened to pick.
    match (
        evaluated.get("TargetFrameworks").map(|v| list(v)),
        evaluated.get("TargetFramework"),
    ) {
        (Some(many), _) if !many.is_empty() => project.target_frameworks = many,
        (_, Some(one)) => project.target_frameworks = vec![one.clone()],
        _ => {}
    }

    if let Some(value) = truthy("IsTestProject") {
        project.is_test_project = Some(value);
    }
    if let Some(value) = truthy("UseMaui") {
        project.use_maui = Some(value);
    }
    if let Some(value) = truthy("IsAspireHost") {
        project.is_aspire_host = Some(value);
    }
    if let Some(value) = truthy("TestingPlatformDotnetTestSupport") {
        project.testing_platform_support = Some(value);
    }
    if let Some(value) = truthy("EnableMSTestRunner") {
        project.enable_mtp_runner = Some(value);
    }
}

/// Ask the SDK to evaluate a project, returning `None` if it cannot.
///
/// Never propagates an error: a workspace that opts into evaluation but has no
/// SDK installed should still open, just with shallow-scanned projects.
pub fn evaluate(project: &Path) -> Option<BTreeMap<String, String>> {
    let output = Command::new("dotnet")
        .args(command_args(project))
        .current_dir(project.parent()?)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let properties = parse_output(&stdout);
    (!properties.is_empty()).then_some(properties)
}
