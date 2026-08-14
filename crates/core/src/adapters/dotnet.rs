//! The .NET ecosystem adapter.
//!
//! # The VSTest / Microsoft.Testing.Platform split
//!
//! `dotnet test` has two entirely separate execution paths, and telling them
//! apart is the single most important thing this module does.
//!
//! * **VSTest** — the classic path. Takes `--logger "trx;LogFileName=..."`.
//! * **Microsoft.Testing.Platform (MTP)** — the newer path. Takes
//!   `-- --report-trx --report-trx-filename ...`, and **silently ignores**
//!   VSTest's `--logger`.
//!
//! Because MTP ignores the flags rather than rejecting them, getting this
//! wrong does not produce an error. The run appears to succeed, exits zero,
//! and simply leaves no report behind — which surfaces to the user as "the
//! tests ran but nothing appeared". That is why detection has its own
//! extensive tests below, and why a missing report is reported with a message
//! that names this as the likely cause.
//!
//! Both paths can emit TRX, so a single parser serves both.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::model::{
    ConfigSource, Invocation, ProjectKind, ReportFormat, ReportSpec, RunConfig, RunKind, TestRunner,
};

/// Values read out of a `.csproj` / `.fsproj` or a `Directory.Build.props`.
///
/// Every field is optional because MSBuild properties can be set in any of
/// several files that layer on top of each other.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProjectFile {
    /// The `Sdk` attribute of the `<Project>` element.
    pub sdk: Option<String>,
    /// Nested `<Sdk Name="..." />` imports, which is how Aspire's app host SDK
    /// and similar workloads layer on top of `Microsoft.NET.Sdk`.
    pub sdk_imports: Vec<String>,
    pub output_type: Option<String>,
    pub target_frameworks: Vec<String>,
    /// `<Configurations>` — the build configurations the project declares.
    /// Empty means the MSBuild default of `Debug;Release`.
    pub configurations: Vec<String>,
    pub is_test_project: Option<bool>,
    /// `<UseMaui>` — a .NET MAUI application, which is launchable even though
    /// some templates leave `<OutputType>` to the workload.
    pub use_maui: Option<bool>,
    /// `<IsAspireHost>` — an Aspire app host, likewise launchable.
    pub is_aspire_host: Option<bool>,
    /// `<TestingPlatformDotnetTestSupport>` — an explicit opt in to MTP.
    pub testing_platform_support: Option<bool>,
    /// `<EnableMSTestRunner>` / `<UseMicrosoftTestingPlatformRunner>` — the
    /// per-framework ways of switching MSTest and NUnit onto MTP.
    pub enable_mtp_runner: Option<bool>,
    /// `<UserSecretsId>` — names the secrets store under the user profile.
    pub user_secrets_id: Option<String>,
    pub package_references: Vec<String>,
    /// `<ProjectReference Include="..." />` — the raw `Include` attribute of
    /// every project reference, **exactly as written in the file**.
    ///
    /// Deliberately not normalised: separators are left as the author typed
    /// them (`..\Lib\Lib.csproj` on Windows, `../Lib/Lib.csproj` elsewhere),
    /// `..` segments are not resolved and nothing is made absolute. Resolving
    /// a reference to a project on disk is a separate stage that needs the
    /// referencing file's directory, which this parser does not have — and
    /// when the resolution fails, the only useful thing to show the user is
    /// the string they actually wrote. Rewriting it here would destroy that.
    pub project_references: Vec<String>,
}

impl ProjectFile {
    /// Case-insensitive test for a package reference, optionally by prefix.
    pub fn references(&self, name: &str) -> bool {
        self.package_references
            .iter()
            .any(|p| p.eq_ignore_ascii_case(name))
    }

    pub fn references_prefix(&self, prefix: &str) -> bool {
        let prefix = prefix.to_ascii_lowercase();
        self.package_references
            .iter()
            .any(|p| p.to_ascii_lowercase().starts_with(&prefix))
    }
}

/// Extract the handful of MSBuild properties and package references we care
/// about.
///
/// This is deliberately a shallow scan rather than an MSBuild evaluation: we
/// only need enough to classify a project, and evaluating MSBuild properly
/// would mean shipping MSBuild.
///
/// # `Condition` is not evaluated
///
/// The scan walks elements and ignores every `Condition` attribute, on the
/// item and on its enclosing `<ItemGroup>` alike. A `<ProjectReference>` that
/// applies to only one target framework, or only one configuration, is
/// therefore reported unconditionally — the same known limitation the
/// `<PackageReference>` handling above already has. This is not fixable from
/// here: `adapters/msbuild.rs` explains that `-getProperty` returns evaluated
/// *properties*, not items, so even the optional MSBuild evaluation pass
/// cannot supply a conditioned item list. Anything that consumes these
/// strings must treat them as "declared somewhere in the file", not as "in
/// effect for the framework you are building".
///
/// # References can be declared in `Directory.Build.props`, and are not merged
///
/// `workspace.rs::inherited_props` parses each `Directory.Build.props` and
/// `Directory.Build.targets` above a project through *this same function*, so
/// an `<ItemGroup><ProjectReference/></ItemGroup>` in a props file does land in
/// that props file's `project_references`. Nothing merges those into the
/// project's own list: `workspace.rs::scan_dotnet_project` keeps `parsed` and `props` separate and
/// only the explicit helpers (`is_test_project`, `project_kind`,
/// `configurations`, `classify_runner`) consult both. So a reference inherited
/// from a props file is currently invisible to a caller reading
/// `parsed.project_references`. That is left alone on purpose — the `Include`
/// of a props-file reference is relative to the props file's directory, not to
/// the project's, so merging the two lists without also carrying each one's
/// base directory would produce paths that resolve to the wrong place, and a
/// wrong edge is far worse than a missing one.
pub fn parse_project_file(xml: &str) -> ProjectFile {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut out = ProjectFile::default();
    let mut current: Option<String> = None;
    let mut text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if name.eq_ignore_ascii_case("PackageReference") {
                    if let Some(include) = attr_value(&e, "Include") {
                        out.package_references.push(include);
                    }
                }
                if name.eq_ignore_ascii_case("ProjectReference") {
                    if let Some(include) = attr_value(&e, "Include") {
                        out.project_references.push(include);
                    }
                }
                if name.eq_ignore_ascii_case("Project") {
                    out.sdk = attr_value(&e, "Sdk");
                }
                if name.eq_ignore_ascii_case("Sdk") {
                    if let Some(sdk) = attr_value(&e, "Name") {
                        out.sdk_imports.push(sdk);
                    }
                }
                current = Some(name);
                text.clear();
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if name.eq_ignore_ascii_case("PackageReference") {
                    if let Some(include) = attr_value(&e, "Include") {
                        out.package_references.push(include);
                    }
                }
                if name.eq_ignore_ascii_case("ProjectReference") {
                    if let Some(include) = attr_value(&e, "Include") {
                        out.project_references.push(include);
                    }
                }
                if name.eq_ignore_ascii_case("Sdk") {
                    if let Some(sdk) = attr_value(&e, "Name") {
                        out.sdk_imports.push(sdk);
                    }
                }
            }
            Ok(Event::Text(t)) => {
                if current.is_some() {
                    text.push_str(&t.unescape().unwrap_or_default());
                }
            }
            Ok(Event::End(_)) => {
                if let Some(name) = current.take() {
                    let value = text.trim();
                    if !value.is_empty() {
                        apply_property(&mut out, &name, value);
                    }
                }
                text.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    out
}

fn attr_value(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if a.key
            .local_name()
            .as_ref()
            .eq_ignore_ascii_case(name.as_bytes())
        {
            a.unescape_value().ok().map(|v| v.into_owned())
        } else {
            None
        }
    })
}

fn apply_property(out: &mut ProjectFile, name: &str, value: &str) {
    let truthy = || value.eq_ignore_ascii_case("true");

    if name.eq_ignore_ascii_case("OutputType") {
        out.output_type = Some(value.to_string());
    } else if name.eq_ignore_ascii_case("TargetFramework") {
        out.target_frameworks = vec![value.to_string()];
    } else if name.eq_ignore_ascii_case("TargetFrameworks") {
        out.target_frameworks = value
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
    } else if name.eq_ignore_ascii_case("Configurations") {
        out.configurations = value
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
    } else if name.eq_ignore_ascii_case("IsTestProject") {
        out.is_test_project = Some(truthy());
    } else if name.eq_ignore_ascii_case("UseMaui") {
        out.use_maui = Some(truthy());
    } else if name.eq_ignore_ascii_case("IsAspireHost") {
        out.is_aspire_host = Some(truthy());
    } else if name.eq_ignore_ascii_case("TestingPlatformDotnetTestSupport") {
        out.testing_platform_support = Some(truthy());
    } else if name.eq_ignore_ascii_case("EnableMSTestRunner")
        || name.eq_ignore_ascii_case("UseMicrosoftTestingPlatformRunner")
    {
        out.enable_mtp_runner = Some(truthy());
    } else if name.eq_ignore_ascii_case("UserSecretsId") {
        out.user_secrets_id = Some(value.to_string());
    }
}

/// The runner selection recorded in a `dotnet.config`.
///
/// The .NET 10 SDK reads `[dotnet.test:runner] name = "Microsoft.Testing.Platform"`
/// from a `dotnet.config` beside the solution. When present it overrides
/// everything else, so it is checked first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfiguredRunner {
    MicrosoftTestingPlatform,
    VsTest,
}

/// Read the runner selection out of a `dotnet.config`.
///
/// Parsed as INI rather than TOML: the section name `dotnet.test:runner`
/// contains a colon, which is not a legal TOML bare key.
pub fn parse_dotnet_config(content: &str) -> Option<ConfiguredRunner> {
    let mut in_runner_section = false;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        if let Some(section) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_runner_section = section.trim().eq_ignore_ascii_case("dotnet.test:runner");
            continue;
        }

        if in_runner_section {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim().eq_ignore_ascii_case("name") {
                    let value = value.trim().trim_matches(['"', '\'']);
                    return Some(
                        if value.eq_ignore_ascii_case("Microsoft.Testing.Platform") {
                            ConfiguredRunner::MicrosoftTestingPlatform
                        } else {
                            ConfiguredRunner::VsTest
                        },
                    );
                }
            }
        }
    }
    None
}

/// Decide whether a project contains tests at all.
pub fn is_test_project(project: &ProjectFile, inherited: &[ProjectFile]) -> bool {
    if let Some(explicit) = project
        .is_test_project
        .or_else(|| inherited.iter().rev().find_map(|p| p.is_test_project))
    {
        return explicit;
    }

    let all = std::iter::once(project).chain(inherited.iter());
    all.into_iter().any(|p| {
        p.references("Microsoft.NET.Test.Sdk")
            || p.references_prefix("xunit")
            || p.references_prefix("nunit")
            || p.references_prefix("MSTest")
            || p.references_prefix("TUnit")
            || p.references_prefix("Microsoft.Testing.Platform")
    })
}

/// Decide which of the two `dotnet test` paths a project uses.
///
/// Precedence, highest first:
/// 1. An explicit `dotnet.config` runner selection.
/// 2. An explicit `<TestingPlatformDotnetTestSupport>` property.
/// 3. An explicit per-framework runner switch (`<EnableMSTestRunner>`).
/// 4. Package evidence: an MTP-native framework, or MTP extensions without
///    `Microsoft.NET.Test.Sdk` — which is what VSTest requires.
///
/// Anything else is assumed to be VSTest, since that remains the default.
pub fn classify_runner(
    project: &ProjectFile,
    inherited: &[ProjectFile],
    configured: Option<ConfiguredRunner>,
) -> TestRunner {
    match configured {
        Some(ConfiguredRunner::MicrosoftTestingPlatform) => {
            return TestRunner::MicrosoftTestingPlatform
        }
        Some(ConfiguredRunner::VsTest) => return TestRunner::VsTest,
        None => {}
    }

    let layered: Vec<&ProjectFile> = std::iter::once(project).chain(inherited.iter()).collect();

    // An explicit property wins over any amount of package guesswork. Nearest
    // file first, so a project overrides Directory.Build.props.
    if let Some(explicit) = layered.iter().find_map(|p| p.testing_platform_support) {
        return if explicit {
            TestRunner::MicrosoftTestingPlatform
        } else {
            TestRunner::VsTest
        };
    }
    if let Some(explicit) = layered.iter().find_map(|p| p.enable_mtp_runner) {
        return if explicit {
            TestRunner::MicrosoftTestingPlatform
        } else {
            TestRunner::VsTest
        };
    }

    // xunit.v3 and TUnit run on MTP natively.
    let mtp_native = layered
        .iter()
        .any(|p| p.references_prefix("xunit.v3") || p.references_prefix("TUnit"));
    if mtp_native {
        return TestRunner::MicrosoftTestingPlatform;
    }

    // MTP extensions present *and* no VSTest host: only MTP can run this.
    let has_mtp_packages = layered.iter().any(|p| {
        p.references_prefix("Microsoft.Testing.Platform")
            || p.references_prefix("Microsoft.Testing.Extensions")
    });
    let has_vstest_host = layered
        .iter()
        .any(|p| p.references("Microsoft.NET.Test.Sdk"));
    if has_mtp_packages && !has_vstest_host {
        return TestRunner::MicrosoftTestingPlatform;
    }

    TestRunner::VsTest
}

/// Whether the TRX reporting extension is available for an MTP project.
///
/// Without `Microsoft.Testing.Extensions.TrxReport`, MTP accepts
/// `--report-trx` but produces nothing, so this is worth warning about before
/// the run rather than diagnosing afterwards.
pub fn has_trx_extension(project: &ProjectFile, inherited: &[ProjectFile]) -> bool {
    std::iter::once(project).chain(inherited.iter()).any(|p| {
        p.references_prefix("Microsoft.Testing.Extensions.TrxReport")
                // xunit.v3 and TUnit bundle TRX reporting in their main package.
                || p.references_prefix("xunit.v3")
                || p.references_prefix("TUnit")
    })
}

/// Classify a project, layering `Directory.Build.props` underneath it.
///
/// `OutputType` is frequently set once in a `Directory.Build.props` for a whole
/// folder of tools, so the inherited files are consulted before falling back to
/// SDK defaults.
pub fn project_kind(
    project: &ProjectFile,
    inherited: &[ProjectFile],
    is_test: bool,
) -> ProjectKind {
    if is_test {
        return ProjectKind::Test;
    }

    let layered: Vec<&ProjectFile> = std::iter::once(project).chain(inherited.iter()).collect();

    // Workload markers describe an application regardless of OutputType: MAUI
    // heads and Aspire app hosts are both launched with `dotnet run`.
    if layered
        .iter()
        .any(|p| p.use_maui == Some(true) || p.is_aspire_host == Some(true))
        || layered.iter().any(|p| {
            p.sdk_imports
                .iter()
                .any(|s| s.eq_ignore_ascii_case("Aspire.AppHost.Sdk"))
        })
    {
        return ProjectKind::Executable;
    }

    // Nearest file first, so a project overrides Directory.Build.props.
    match layered.iter().find_map(|p| p.output_type.as_deref()) {
        Some(t) if t.eq_ignore_ascii_case("Exe") || t.eq_ignore_ascii_case("WinExe") => {
            ProjectKind::Executable
        }
        Some(_) => ProjectKind::Library,
        // A project with no OutputType is a library unless its SDK produces an
        // executable by default — ASP.NET Core templates in particular never
        // write an OutputType.
        None => {
            if layered
                .iter()
                .any(|p| p.sdk.as_deref().is_some_and(sdk_defaults_to_executable))
            {
                ProjectKind::Executable
            } else {
                ProjectKind::Library
            }
        }
    }
}

/// SDKs whose projects build an executable without declaring `<OutputType>`.
///
/// `Microsoft.NET.Sdk.Razor` is deliberately absent: it is the Razor *class
/// library* SDK, and Blazor Server apps use the Web SDK instead.
fn sdk_defaults_to_executable(sdk: &str) -> bool {
    let sdk = sdk.to_ascii_lowercase();
    matches!(
        sdk.as_str(),
        "microsoft.net.sdk.web"
            | "microsoft.net.sdk.worker"
            | "microsoft.net.sdk.blazorwebassembly"
    )
}

/// The build configurations a project offers, nearest declaration winning.
///
/// `<Configurations>` replaces the MSBuild default of `Debug;Release` when a
/// project declares it, which is the only way a `Staging` or `QA` configuration
/// becomes visible without evaluating MSBuild.
pub fn configurations(project: &ProjectFile, inherited: &[ProjectFile]) -> Vec<String> {
    std::iter::once(project)
        .chain(inherited.iter())
        .find(|p| !p.configurations.is_empty())
        .map(|p| p.configurations.clone())
        .unwrap_or_else(|| vec!["Debug".to_string(), "Release".to_string()])
}

/// The configuration to use where a debug build is wanted.
///
/// Almost always `Debug`, but a project may declare a configuration set that
/// omits it entirely, in which case the first declared one has to serve.
fn debug_configuration(configurations: &[String]) -> String {
    configurations
        .iter()
        .find(|c| c.eq_ignore_ascii_case("Debug"))
        .or_else(|| configurations.first())
        .cloned()
        .unwrap_or_else(|| "Debug".to_string())
}

// ---------------------------------------------------------------------------
// launchSettings.json
// ---------------------------------------------------------------------------

/// One profile from `Properties/launchSettings.json`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchProfile {
    pub name: String,
    pub command_name: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub working_directory: Option<String>,
    pub application_url: Option<String>,
    /// Whether `dotnet run --launch-profile` can actually apply this profile.
    ///
    /// Only `Project` profiles can. IIS Express, Docker and Executable profiles
    /// describe a hosting model this app does not launch, but they are still
    /// reported so the UI can show why a project appears to have no profiles
    /// rather than leaving the list mysteriously empty.
    pub launchable: bool,
}

impl LaunchProfile {
    /// `dotnet run --launch-profile` only understands `Project` profiles; a
    /// profile with no `commandName` at all defaults to one.
    pub fn is_launchable(command_name: Option<&str>) -> bool {
        matches!(command_name, None | Some("Project"))
    }
}

/// Parse `launchSettings.json` into its profiles.
///
/// Every profile is returned, including the hosting models this app cannot
/// launch; [`LaunchProfile::launchable`] separates them.
pub fn parse_launch_settings(json: &str) -> Vec<LaunchProfile> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(profiles) = root.get("profiles").and_then(|p| p.as_object()) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for (name, body) in profiles {
        let command_name = body
            .get("commandName")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let launchable = LaunchProfile::is_launchable(command_name.as_deref());

        let args = body
            .get("commandLineArgs")
            .and_then(|v| v.as_str())
            .map(split_args)
            .unwrap_or_default();

        let env = body
            .get("environmentVariables")
            .and_then(|v| v.as_object())
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        out.push(LaunchProfile {
            name: name.clone(),
            command_name,
            args,
            env,
            working_directory: body
                .get("workingDirectory")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            application_url: body
                .get("applicationUrl")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            launchable,
        });
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Split a command line string into arguments, honouring double quotes.
pub fn split_args(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut has_content = false;

    for ch in raw.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                // A quoted empty string is still an argument.
                has_content = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_content {
                    out.push(std::mem::take(&mut current));
                    has_content = false;
                }
            }
            c => {
                current.push(c);
                has_content = true;
            }
        }
    }
    if has_content {
        out.push(current);
    }
    out
}

// ---------------------------------------------------------------------------
// Building invocations
// ---------------------------------------------------------------------------

/// Everything needed to turn a [`RunConfig`] into a command line.
pub struct BuildContext<'a> {
    pub workspace_root: &'a Path,
    /// Where report files should be written.
    pub results_dir: &'a Path,
    /// The runner in use, for test configurations.
    pub runner: Option<TestRunner>,
    /// Whether TRX reporting is available. Only meaningful under MTP.
    pub trx_extension_available: bool,
    /// Whether the project has a `Properties/launchSettings.json`, so running
    /// without a profile can warn about the environment it leaves behind.
    pub has_launch_settings: bool,
    /// Fully qualified names to restrict the run to, for "re-run failed".
    pub filter: Option<Vec<String>>,
    /// Where the runtime should write crash dumps, when the workspace has
    /// opted into capturing them. `None` — the default — arms nothing.
    ///
    /// This lives here rather than in [`crate::process`] alongside the colour
    /// defaults because it is neither ecosystem-agnostic nor cheap: only .NET
    /// understands `DOTNET_Dbg*`, the path is per-workspace state the
    /// supervisor has no business knowing, and a single crash writes hundreds
    /// of megabytes.
    pub dumps_dir: Option<&'a Path>,
    /// `inspector.env` from the workspace configuration: extra environment the
    /// workspace asked to apply to dump-capturing runs, typically to change
    /// `DOTNET_DbgMiniDumpType`. Ignored unless `dumps_dir` is set, because it
    /// exists to tune a capture and there is no capture to tune otherwise.
    pub dump_env: Option<&'a BTreeMap<String, String>>,
}

/// What a run is told when this workspace has opted into crash dumps.
///
/// `.code-basics/config.json` is shared through the repository, so the opt-in
/// is one person's edit and everyone else's runs. Without this the first a
/// teammate knows about it is hundreds of megabytes of their process memory on
/// disk, written by a tab they never opened.
const CAPTURE_ARMED_WARNING: &str =
    "Crash dump capture is on for this workspace (inspector.captureDumps in \
     .code-basics/config.json, which is shared through the repository). If this process dies \
     from an unhandled exception, the runtime writes a verbatim copy of its memory — \
     connection strings, tokens, whatever was in flight — into .code-basics/dumps/. Expect \
     hundreds of megabytes per crash.";

/// Layer the crash-dump variables **underneath** a configuration's own
/// environment.
///
/// The same precedence [`crate::process`] uses for its colour defaults: a user
/// who sets `DOTNET_DbgMiniDumpType` themselves keeps it. `extend` overwrites,
/// so inserting the configuration's entries last is what makes the user win.
///
/// Three layers, weakest first: the built-in `DOTNET_Dbg*` defaults, then the
/// workspace's `inspector.env` (which exists precisely to change one of them),
/// then the run configuration's own environment.
fn dump_layered_env(config: &RunConfig, ctx: &BuildContext) -> BTreeMap<String, String> {
    let Some(dir) = ctx.dumps_dir else {
        return config.env.clone();
    };

    let mut env = crate::inspect::dumps::dump_env(dir);
    if let Some(workspace) = ctx.dump_env {
        env.extend(workspace.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    env.extend(config.env.iter().map(|(k, v)| (k.clone(), v.clone())));
    env
}

/// Build the `dotnet test` command line for a test configuration.
pub fn test_invocation(config: &RunConfig, ctx: &BuildContext) -> Invocation {
    let runner = ctx.runner.unwrap_or(TestRunner::VsTest);
    let report_path = ctx
        .results_dir
        .join(format!("{}.trx", sanitise(&config.id)));
    let report_name = report_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "report.trx".to_string());

    let mut args = vec!["test".to_string()];
    let mut warnings = Vec::new();

    if ctx.dumps_dir.is_some() {
        warnings.push(CAPTURE_ARMED_WARNING.to_string());
    }

    if let Some(project) = &config.project {
        args.push(ctx.workspace_root.join(project).display().to_string());
    }
    if let Some(configuration) = &config.build_configuration {
        args.push("-c".into());
        args.push(configuration.clone());
    }
    if let Some(framework) = &config.framework {
        args.push("-f".into());
        args.push(framework.clone());
    }

    match runner {
        TestRunner::MicrosoftTestingPlatform => {
            if !ctx.trx_extension_available {
                warnings.push(
                    "This project runs on Microsoft.Testing.Platform but does not reference \
                     Microsoft.Testing.Extensions.TrxReport. The run will succeed but produce \
                     no results. Add the package to see test results here."
                        .to_string(),
                );
            }
            // Everything after `--` is MTP's own command line. VSTest options
            // before it would be silently ignored.
            args.push("--".into());
            args.push("--report-trx".into());
            args.push("--report-trx-filename".into());
            args.push(report_name);
            args.push("--results-directory".into());
            args.push(ctx.results_dir.display().to_string());
            // MTP's default `Normal` verbosity prints only a run summary, so
            // the UI's live progress counter has nothing to read until the run
            // ends. `Detailed` prints a `passed TestName (5ms)` line as each
            // test finishes — the MTP equivalent of VSTest's console logger.
            args.push("--output".into());
            args.push("Detailed".into());

            if let Some(names) = &ctx.filter {
                if !names.is_empty() {
                    args.push("--filter".into());
                    args.push(vstest_filter(names));
                    warnings.push(
                        "Re-running only failed tests uses `--filter`, which is provided by the \
                         VSTest bridge. Runners that implement Microsoft.Testing.Platform \
                         natively may ignore it and run the full suite."
                            .to_string(),
                    );
                }
            }

            // MTP ignores every `--blame-*` option, so there is nothing to add
            // to the command line here. Its crash dumps come from a package
            // the project has to reference, which is the user's edit to make,
            // not ours.
            if ctx.dumps_dir.is_some() {
                warnings.push(
                    "Crash dump capture is on, but this project runs on \
                     Microsoft.Testing.Platform, which ignores VSTest's --blame options. \
                     To capture a dump when the test host crashes, add the \
                     Microsoft.Testing.Extensions.CrashDump package to the project and \
                     pass --crashdump."
                        .to_string(),
                );
            }
        }
        _ => {
            args.push("--logger".into());
            args.push(format!("trx;LogFileName={report_name}"));
            // The default console verbosity prints nothing per test; `normal`
            // prints a Passed/Failed line as each test finishes, which is what
            // the UI's live progress counter reads.
            args.push("--logger".into());
            args.push("console;verbosity=normal".into());
            args.push("--results-directory".into());
            args.push(ctx.results_dir.display().to_string());

            // The `DOTNET_Dbg*` variables only fire on an unhandled crash, and
            // a failing assertion is not one — the test host exits tidily, so
            // the run that most needs a dump produces none. Blame's crash
            // collector covers exactly that gap: it "collects a crash dump on
            // expected as well as unexpected test host exit".
            if ctx.dumps_dir.is_some() {
                args.push("--blame-crash-collect-always".into());
                // Blame writes its dump into `--results-directory`, under a
                // name of its own, so it never appears in the Objects tab.
                // Saying so is the difference between a user looking for it and
                // a user concluding the capture failed. It is still pruned:
                // `inspect::session` sweeps that directory under the same byte
                // budget.
                warnings.push(
                    "Crash dump capture is on, so this run also passes VSTest's \
                     --blame-crash-collect-always, which writes a dump when the test host \
                     exits for any reason. That dump lands in .code-basics/results/ under \
                     the collector's own name, not in .code-basics/dumps/, so it is not \
                     listed in the Objects tab — open it with your own debugger."
                        .to_string(),
                );
            }

            if let Some(names) = &ctx.filter {
                if !names.is_empty() {
                    args.push("--filter".into());
                    args.push(vstest_filter(names));
                }
            }
        }
    }

    args.extend(config.args.iter().cloned());

    Invocation {
        program: "dotnet".into(),
        args,
        cwd: resolve_cwd(config, ctx.workspace_root),
        env: dump_layered_env(config, ctx),
        report: Some(ReportSpec {
            path: report_path,
            format: ReportFormat::Trx,
        }),
        warnings,
    }
}

/// Build the `dotnet run` command line for an application configuration.
pub fn run_invocation(config: &RunConfig, ctx: &BuildContext) -> Invocation {
    let mut args = vec!["run".to_string()];
    let mut warnings = Vec::new();

    if ctx.dumps_dir.is_some() {
        warnings.push(CAPTURE_ARMED_WARNING.to_string());
    }

    if let Some(project) = &config.project {
        args.push("--project".into());
        args.push(ctx.workspace_root.join(project).display().to_string());
    }
    if let Some(configuration) = &config.build_configuration {
        args.push("-c".into());
        args.push(configuration.clone());
    }
    if let Some(framework) = &config.framework {
        args.push("-f".into());
        args.push(framework.clone());
    }

    match &config.launch_profile {
        Some(profile) => {
            args.push("--launch-profile".into());
            args.push(profile.clone());
        }
        // An explicit opt-out. Skipping the profile silently bites hard —
        // no ASPNETCORE_ENVIRONMENT=Development (so no user secrets), no
        // applicationUrl — so opting out gets a warning unless the config
        // sets the environment itself.
        None if config.ignore_launch_settings => {
            args.push("--no-launch-profile".into());

            if ctx.has_launch_settings
                && !config.env.contains_key("ASPNETCORE_ENVIRONMENT")
                && !config.env.contains_key("DOTNET_ENVIRONMENT")
            {
                warnings.push(
                    "This configuration ignores launchSettings.json, so its environment \
                     variables and applicationUrl are not applied — the app runs without \
                     ASPNETCORE_ENVIRONMENT=Development, which also disables .NET user \
                     secrets. Pick a launch profile, set the environment variables \
                     explicitly, or untick \"Ignore launchSettings.json\"."
                        .to_string(),
                );
            }
        }
        // No profile named: leave `dotnet run` to its own default — the first
        // Project profile in launchSettings.json, with its environment and
        // applicationUrl applied. This matches running from a terminal or an
        // IDE, which is what people expect a plain "run" to do.
        None => {}
    }

    if !config.args.is_empty() {
        args.push("--".into());
        args.extend(config.args.iter().cloned());
    }

    Invocation {
        program: "dotnet".into(),
        args,
        cwd: resolve_cwd(config, ctx.workspace_root),
        env: dump_layered_env(config, ctx),
        report: None,
        warnings,
    }
}

/// A build-system action on a project, as opposed to running it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum BuildAction {
    Build,
    /// A full non-incremental compile, like Rider's Rebuild.
    Rebuild,
    Clean,
}

/// Build the `dotnet build` / `dotnet clean` command line for a configuration.
pub fn build_action_invocation(
    config: &RunConfig,
    action: BuildAction,
    workspace_root: &Path,
) -> Invocation {
    let mut args = match action {
        BuildAction::Build => vec!["build".to_string()],
        BuildAction::Rebuild => vec!["build".to_string(), "--no-incremental".to_string()],
        BuildAction::Clean => vec!["clean".to_string()],
    };

    if let Some(project) = &config.project {
        args.push(workspace_root.join(project).display().to_string());
    }
    if let Some(configuration) = &config.build_configuration {
        args.push("-c".into());
        args.push(configuration.clone());
    }
    if let Some(framework) = &config.framework {
        args.push("-f".into());
        args.push(framework.clone());
    }

    Invocation {
        program: "dotnet".into(),
        args,
        cwd: resolve_cwd(config, workspace_root),
        env: config.env.clone(),
        report: None,
        warnings: Vec::new(),
    }
}

/// Build a VSTest filter expression restricting a run to specific tests.
fn vstest_filter(full_names: &[String]) -> String {
    full_names
        .iter()
        .map(|n| format!("FullyQualifiedName={}", strip_arguments(n)))
        .collect::<Vec<_>>()
        .join("|")
}

/// Drop a theory's argument list from a name.
///
/// `FullyQualifiedName` matches the method, not an individual data row, and
/// leaving `(value: 2)` on would match nothing at all.
fn strip_arguments(full_name: &str) -> &str {
    match full_name.find('(') {
        Some(i) => &full_name[..i],
        None => full_name,
    }
}

fn resolve_cwd(config: &RunConfig, workspace_root: &Path) -> PathBuf {
    if let Some(cwd) = &config.cwd {
        return workspace_root.join(cwd);
    }
    // Default to the project's directory so relative paths in the app behave
    // the way they do when launched from an IDE.
    if let Some(project) = &config.project {
        let full = workspace_root.join(project);
        if let Some(parent) = full.parent() {
            return parent.to_path_buf();
        }
    }
    workspace_root.to_path_buf()
}

/// Make an id safe to use as a file name.
fn sanitise(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Create the standard set of configurations for a discovered project.
pub fn configs_for_project(
    project_id: &str,
    project_name: &str,
    relative_path: &Path,
    kind: ProjectKind,
    frameworks: &[String],
    configurations: &[String],
    launch_profiles: &[LaunchProfile],
) -> Vec<RunConfig> {
    let mut out = Vec::new();

    // A single-targeted project needs no `-f` at all; a multi-targeted one
    // cannot be run without choosing, so every framework gets its own
    // configuration rather than silently picking the first.
    let targets: Vec<Option<String>> = if frameworks.len() > 1 {
        frameworks.iter().cloned().map(Some).collect()
    } else {
        vec![None]
    };

    // Suffixes that keep ids unique and names readable once a project is
    // multi-targeted, and vanish entirely when it is not.
    let id_suffix = |framework: &Option<String>| match framework {
        Some(f) => format!(":{}", sanitise(f)),
        None => String::new(),
    };

    match kind {
        ProjectKind::Test => {
            // Debug only, like Rider's default test session. A Release test
            // run is a legitimate but rare want (and `#if !DEBUG` code paths
            // make it a trap — missing user secrets, managed-identity-only
            // branches); the config editor's Build configuration dropdown
            // offers the project's other configurations for anyone who needs
            // one.
            let configuration = debug_configuration(configurations);

            for framework in &targets {
                let label = match framework {
                    Some(f) => format!("{project_name} tests ({f})"),
                    None => format!("{project_name} tests"),
                };
                let mut config = RunConfig::new(
                    format!(
                        "{project_id}:test:{}{}",
                        configuration.to_lowercase(),
                        id_suffix(framework)
                    ),
                    label,
                    RunKind::Test,
                    "dotnet",
                    ConfigSource::Detected,
                );
                config.project = Some(relative_path.to_path_buf());
                config.build_configuration = Some(configuration.clone());
                config.framework = framework.clone();
                out.push(config);
            }
        }
        ProjectKind::Executable => {
            let debug = debug_configuration(configurations);

            // A configuration per launch profile, so the environment and URLs
            // the project already defines are preserved. Profiles describing a
            // hosting model we cannot start are skipped here but still reported
            // to the UI by `workspace::launch_profiles`.
            for profile in launch_profiles.iter().filter(|p| p.launchable) {
                for framework in &targets {
                    let label = match framework {
                        Some(f) => format!("{project_name} ({}, {f})", profile.name),
                        None => format!("{project_name} ({})", profile.name),
                    };
                    let mut config = RunConfig::new(
                        format!(
                            "{project_id}:run:{}{}",
                            sanitise(&profile.name),
                            id_suffix(framework)
                        ),
                        label,
                        RunKind::App,
                        "dotnet",
                        ConfigSource::Detected,
                    );
                    config.project = Some(relative_path.to_path_buf());
                    config.build_configuration = Some(debug.clone());
                    config.framework = framework.clone();
                    config.launch_profile = Some(profile.name.clone());
                    out.push(config);
                }
            }

            for configuration in configurations {
                for framework in &targets {
                    let label = match framework {
                        Some(f) => format!("{project_name} ({configuration}, {f})"),
                        None => format!("{project_name} ({configuration})"),
                    };
                    let mut config = RunConfig::new(
                        format!(
                            "{project_id}:run:{}{}",
                            configuration.to_lowercase(),
                            id_suffix(framework)
                        ),
                        label,
                        RunKind::App,
                        "dotnet",
                        ConfigSource::Detected,
                    );
                    config.project = Some(relative_path.to_path_buf());
                    config.build_configuration = Some(configuration.clone());
                    config.framework = framework.clone();
                    out.push(config);
                }
            }
        }
        // Libraries cannot be launched and have no tests to run.
        ProjectKind::Library | ProjectKind::Unknown => {}
    }

    out
}
