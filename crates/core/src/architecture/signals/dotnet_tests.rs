//! Tests for [`super::dotnet`].
//!
//! Every test builds a real temporary workspace and runs the real
//! [`crate::workspace::scan`] over it, for the same reason
//! [`super::super::graph_tests`] does: this producer's whole job is to line
//! strings in files up with what the scan found, so a test that fabricated the
//! scan's output would be testing the fabrication.
//!
//! Most assertions are made against [`admit`] rather than against the raw
//! signal list. What a producer *emits* is not what a user sees — a MEDIUM
//! signal with no HIGH counterpart is emitted and then refused — so a test that
//! stopped at the signal list could pass while the diagram stayed empty, or
//! while a box appeared that the grading rule forbids.

use super::dotnet::*;
use super::framework::{admit, Admitted, ComponentKind, DiscardReason, Strength};
use crate::workspace::{scan, Workspace};

// ---------------------------------------------------------------------------
// Fixtures
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

fn scanned(files: &[(&str, &str)]) -> (tempfile::TempDir, Workspace) {
    let dir = workspace_with(files);
    let ws = scan(dir.path()).unwrap();
    (dir, ws)
}

/// The producer's raw output. The [`tempfile::TempDir`] is returned so the
/// caller keeps the workspace alive for the length of the test.
fn produced(files: &[(&str, &str)]) -> (tempfile::TempDir, DotnetSignals) {
    let (dir, ws) = scanned(files);
    let out = signals(&ws);
    (dir, out)
}

/// The producer's output after the gate — what a user would actually see.
fn admitted(files: &[(&str, &str)]) -> (tempfile::TempDir, DotnetSignals, Admitted) {
    let (dir, out) = produced(files);
    let gated = admit(out.signals.clone());
    (dir, out, gated)
}

fn csproj(properties: &str, packages: &[&str]) -> String {
    let items: String = packages
        .iter()
        .map(|p| format!("    <PackageReference Include=\"{p}\" Version=\"1.0.0\" />\n"))
        .collect();
    format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup><TargetFramework>net8.0</TargetFramework>{properties}</PropertyGroup>\n  <ItemGroup>\n{items}  </ItemGroup>\n</Project>"
    )
}

fn web_csproj(packages: &[&str]) -> String {
    let items: String = packages
        .iter()
        .map(|p| format!("    <PackageReference Include=\"{p}\" Version=\"1.0.0\" />\n"))
        .collect();
    format!(
        "<Project Sdk=\"Microsoft.NET.Sdk.Web\">\n  <PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup>\n  <ItemGroup>\n{items}  </ItemGroup>\n</Project>"
    )
}

fn launch_settings(url: &str) -> String {
    format!(
        r#"{{
  "profiles": {{
    "http": {{
      "commandName": "Project",
      "applicationUrl": "{url}"
    }}
  }}
}}"#
    )
}

fn labels(gated: &Admitted, kind: ComponentKind) -> Vec<String> {
    gated
        .components
        .iter()
        .filter(|c| c.kind == kind)
        .map(|c| c.label.clone())
        .collect()
}

fn details(gated: &Admitted) -> Vec<String> {
    gated
        .components
        .iter()
        .flat_map(|c| c.details.iter().map(|d| d.text.clone()))
        .collect()
}

/// The enrichment text attached to *one named box*.
///
/// [`details`] flattens every component's details into one list, which is
/// enough to ask whether something was recorded and useless for asking what it
/// was recorded *against* — and the pairing is exactly what the connection
/// string tests below are about.
fn details_of(gated: &Admitted, label: &str) -> Vec<String> {
    gated
        .components
        .iter()
        .filter(|c| c.label == label)
        .flat_map(|c| c.details.iter().map(|d| d.text.clone()))
        .collect()
}

/// The evidence of every signal whose receipt is a source file.
///
/// The `AddHttpClient` and Aspire rules are the only ones that cite `.cs`, so
/// this is what the comment tests assert is empty.
fn cited_source_files(out: &DotnetSignals) -> Vec<String> {
    out.signals
        .iter()
        .filter(|s| s.evidence.path.to_string_lossy().ends_with(".cs"))
        .map(|s| format!("{:?}", s.evidence))
        .collect()
}

/// Every string a caller of this producer can reach, flattened.
///
/// The leak tests assert over this rather than over labels alone: an excerpt, a
/// detail or a producer warning would carry a secret into an exported diagram
/// exactly as effectively as a label would.
fn everything_visible(out: &DotnetSignals, gated: &Admitted) -> String {
    format!(
        "{out:?} {gated:?} {:?} {:?}",
        gated.edges(),
        gated.warnings()
    )
}

// ---------------------------------------------------------------------------
// HIGH: data clients from <PackageReference>
// ---------------------------------------------------------------------------

#[test]
fn a_package_reference_creates_a_database_component_labelled_with_the_provider() {
    let (_dir, _out, gated) =
        admitted(&[("src/Orders.Api/Orders.Api.csproj", &csproj("", &["Npgsql"]))]);

    assert_eq!(labels(&gated, ComponentKind::Database), ["PostgreSQL"]);
    assert_eq!(
        gated.edges().len(),
        1,
        "one declared client is one edge: {:?}",
        gated.edges()
    );
}

#[test]
fn every_mapped_data_client_package_produces_its_documented_provider_label() {
    let cases: &[(&str, ComponentKind, &str)] = &[
        (
            "Microsoft.EntityFrameworkCore.SqlServer",
            ComponentKind::Database,
            "SQL Server",
        ),
        (
            "Microsoft.Data.SqlClient",
            ComponentKind::Database,
            "SQL Server",
        ),
        ("Npgsql", ComponentKind::Database, "PostgreSQL"),
        (
            "Npgsql.EntityFrameworkCore.PostgreSQL",
            ComponentKind::Database,
            "PostgreSQL",
        ),
        (
            "Pomelo.EntityFrameworkCore.MySql",
            ComponentKind::Database,
            "MySQL",
        ),
        ("MySql.Data", ComponentKind::Database, "MySQL"),
        (
            "Microsoft.EntityFrameworkCore.Sqlite",
            ComponentKind::Database,
            "SQLite",
        ),
        ("MongoDB.Driver", ComponentKind::Database, "MongoDB"),
        ("StackExchange.Redis", ComponentKind::Cache, "Redis"),
        ("RabbitMQ.Client", ComponentKind::MessageQueue, "RabbitMQ"),
        (
            "Azure.Messaging.ServiceBus",
            ComponentKind::MessageQueue,
            "Azure Service Bus",
        ),
        ("Confluent.Kafka", ComponentKind::MessageQueue, "Kafka"),
    ];

    for (package, kind, label) in cases {
        let (_dir, _out, gated) = admitted(&[("src/App/App.csproj", &csproj("", &[package]))]);
        assert_eq!(
            labels(&gated, *kind),
            [label.to_string()],
            "{package} should have produced one {label} component"
        );
    }
}

#[test]
fn a_package_reference_never_names_the_database_instance() {
    let (_dir, out, gated) = admitted(&[
        ("src/Orders.Api/Orders.Api.csproj", &csproj("", &["Npgsql"])),
        (
            "src/Orders.Api/appsettings.json",
            r#"{
  "ConnectionStrings": {
    "Orders": "Host=db.internal;Port=6789;Database=ordersdb;Username=svc_orders;Password=hunter2"
  }
}"#,
        ),
    ]);

    assert_eq!(labels(&gated, ComponentKind::Database), ["PostgreSQL"]);

    let visible = everything_visible(&out, &gated);
    for forbidden in ["db.internal", "6789", "ordersdb", "svc_orders", "hunter2"] {
        assert!(
            !visible.contains(forbidden),
            "'{forbidden}' escaped the parser into: {visible}"
        );
    }
}

#[test]
fn each_package_reference_is_cited_at_its_own_line() {
    let (_dir, _out, gated) = admitted(&[(
        "src/App/App.csproj",
        &csproj("", &["Npgsql.EntityFrameworkCore.PostgreSQL", "Npgsql"]),
    )]);

    let usages = &gated
        .components
        .iter()
        .find(|c| c.label == "PostgreSQL")
        .expect("the two packages are one technology and share one box")
        .usages;

    assert_eq!(
        usages.len(),
        2,
        "two declarations collapsed into one receipt: {usages:?}"
    );
    assert_ne!(
        usages[0].evidence.line, usages[1].evidence.line,
        "both packages were cited at the same line: {usages:?}"
    );
}

#[test]
fn an_unmapped_package_reference_produces_nothing_at_all() {
    let (_dir, out, gated) = admitted(&[(
        "src/App/App.csproj",
        &csproj("", &["Newtonsoft.Json", "Serilog"]),
    )]);

    assert!(gated.components.is_empty(), "{:?}", gated.components);
    assert!(
        out.warnings.is_empty(),
        "an ordinary third-party package is not a refusal worth reporting: {:?}",
        out.warnings
    );
}

#[test]
fn a_package_name_that_merely_shares_a_prefix_word_is_not_matched() {
    let (_dir, _out, gated) = admitted(&[(
        "src/App/App.csproj",
        &csproj("", &["NpgsqlRest", "MongoDB.Analyzer"]),
    )]);

    assert!(
        gated.components.is_empty(),
        "a prefix that does not end on a name boundary matched anyway: {:?}",
        gated.components
    );
}

#[test]
fn an_inmemory_provider_is_not_a_database() {
    let (_dir, out, gated) = admitted(&[(
        "tests/App.Tests/App.Tests.csproj",
        &csproj("", &["Microsoft.EntityFrameworkCore.InMemory"]),
    )]);

    assert!(
        gated.components.is_empty(),
        "the in-memory provider was drawn as a component: {:?}",
        gated.components
    );
    assert!(
        out.warnings
            .iter()
            .any(|w| w.contains("Microsoft.EntityFrameworkCore.InMemory")),
        "the refusal was silent: {:?}",
        out.warnings
    );
}

#[test]
fn a_testcontainers_package_is_not_a_component() {
    let (_dir, out, gated) = admitted(&[(
        "tests/App.Tests/App.Tests.csproj",
        &csproj("", &["Testcontainers.PostgreSql"]),
    )]);

    assert!(
        gated.components.is_empty(),
        "a test-run container was drawn as a component of the system: {:?}",
        gated.components
    );
    assert!(
        out.warnings
            .iter()
            .any(|w| w.contains("Testcontainers.PostgreSql")),
        "the refusal was silent: {:?}",
        out.warnings
    );
}

#[test]
fn a_data_client_in_a_test_project_is_still_a_declared_fact() {
    let (_dir, _out, gated) = admitted(&[(
        "tests/Orders.Tests/Orders.Tests.csproj",
        &csproj("<IsTestProject>true</IsTestProject>", &["Npgsql"]),
    )]);

    assert_eq!(
        labels(&gated, ComponentKind::Database),
        ["PostgreSQL"],
        "the same declaration was read differently because of who made it"
    );
}

#[test]
fn two_projects_declaring_one_provider_share_one_box_with_two_edges() {
    let (_dir, _out, gated) = admitted(&[
        ("src/A/A.csproj", &csproj("", &["Npgsql"])),
        ("src/B/B.csproj", &csproj("", &["Npgsql"])),
    ]);

    assert_eq!(labels(&gated, ComponentKind::Database), ["PostgreSQL"]);
    assert_eq!(gated.edges().len(), 2, "{:?}", gated.edges());
}

// ---------------------------------------------------------------------------
// HIGH: HTTP services
// ---------------------------------------------------------------------------

#[test]
fn a_web_sdk_project_is_an_http_service() {
    let (_dir, _out, gated) = admitted(&[("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[]))]);

    assert_eq!(labels(&gated, ComponentKind::HttpService), ["Orders.Api"]);
}

#[test]
fn an_aspire_host_is_an_http_service() {
    let (_dir, _out, gated) = admitted(&[(
        "AppHost/AppHost.csproj",
        &csproj("<IsAspireHost>true</IsAspireHost>", &[]),
    )]);

    assert_eq!(labels(&gated, ComponentKind::HttpService), ["AppHost"]);
}

#[test]
fn a_library_project_is_not_an_http_service() {
    let (_dir, _out, gated) = admitted(&[("src/Lib/Lib.csproj", &csproj("", &[]))]);

    assert!(gated.components.is_empty(), "{:?}", gated.components);
}

#[test]
fn an_application_url_attaches_to_the_service_the_sdk_declared_naming_the_profile_not_the_url() {
    // This test used to assert the opposite of its second half — that the url
    // *did* reach the detail. That assertion was the leak: a detail is printed
    // verbatim by `components::cross_project_notes`, and an `applicationUrl`
    // takes `user:password@host`. What the signal is for is saying *this
    // project has a launch binding, recorded here*; the profile name and the
    // cited line carry that, and the url adds nothing the reader cannot get by
    // opening the file the evidence names.
    let (_dir, _out, gated) = admitted(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        (
            "src/Orders.Api/Properties/launchSettings.json",
            &launch_settings("https://localhost:7080"),
        ),
    ]);

    assert_eq!(labels(&gated, ComponentKind::HttpService), ["Orders.Api"]);
    assert!(
        details(&gated).iter().any(|d| d.contains("launch profile")),
        "the launch profile did not reach the service it belongs to: {:?}",
        details(&gated)
    );
    assert!(
        !format!("{gated:?}").contains("7080"),
        "the url must not survive anywhere in the graded result: {gated:?}"
    );
}

#[test]
fn an_application_url_never_creates_a_service_no_sdk_declared() {
    let (_dir, _out, gated) = admitted(&[
        ("src/Lib/Lib.csproj", &csproj("", &[])),
        (
            "src/Lib/Properties/launchSettings.json",
            &launch_settings("https://localhost:7080"),
        ),
    ]);

    assert!(
        gated.components.is_empty(),
        "a launch profile brought a service into existence: {:?}",
        gated.components
    );
    assert!(
        gated
            .discarded
            .iter()
            .any(|d| d.reason == DiscardReason::MediumWithoutHigh),
        "the refusal was not counted: {:?}",
        gated.discarded
    );
}

// ---------------------------------------------------------------------------
// The Aspire generated class name
// ---------------------------------------------------------------------------

/// Pinned against the real transform, not against a belief about it.
///
/// Every expectation below was produced by running the SDK's own regex on this
/// machine — see the doc comment on [`aspire_class_name`] for the command and
/// the source of the pattern.
#[test]
fn the_aspire_generated_class_name_transform_matches_the_sdk_regex() {
    let cases: &[(&str, &str)] = &[
        ("Orders.Api", "Orders_Api"),
        ("web-frontend", "web_frontend"),
        ("1Service", "_1Service"),
        ("Orders.1Api", "Orders__1Api"),
        ("A B", "A_B"),
        ("Orders_Api", "Orders_Api"),
        ("a.b.c", "a_b_c"),
        ("Foo..Bar", "Foo__Bar"),
        ("9", "_9"),
        (".9x", "__9x"),
        ("Order$Api", "Order_Api"),
    ];

    for (stem, expected) in cases {
        assert_eq!(
            aspire_class_name(stem).as_deref(),
            Some(*expected),
            "the generated class name for {stem} is wrong"
        );
    }
}

#[test]
fn a_non_ascii_project_name_has_no_generated_class_name_this_module_will_claim() {
    assert_eq!(
        aspire_class_name("Café"),
        None,
        "a Unicode name was transformed by ASCII rules"
    );
    assert_eq!(aspire_class_name(""), None);
}

#[test]
fn an_aspire_add_project_enriches_the_referenced_service() {
    let (_dir, _out, gated) = admitted(&[
        (
            "AppHost/AppHost.csproj",
            &csproj("<IsAspireHost>true</IsAspireHost>", &[]),
        ),
        (
            "AppHost/Program.cs",
            "var builder = DistributedApplication.CreateBuilder(args);\n\
             builder.AddProject<Projects.Orders_Api>(\"orders\");\n\
             builder.Build().Run();\n",
        ),
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
    ]);

    assert!(
        details(&gated).iter().any(|d| d.contains("Aspire")),
        "the app host's own reference did not reach the service: {:?}",
        details(&gated)
    );
}

/// A hyphenated project name resolves end-to-end, and the near-miss directory
/// does not steal it.
///
/// This pins the whole path from `Projects.web_frontend` in the app host down to
/// the `web-frontend.csproj` the scan found: `aspire_class_name` maps the `-` to
/// `_` (the SDK's `\W` rule), and the match is decided on the *transformed
/// stem*, so the sibling `webfrontend.csproj` — whose transform is `webfrontend`
/// and does not equal `web_frontend` — must not be enriched. Asserting on
/// [`details_of`] rather than the flattened [`details`] is the point: the decoy
/// is only a decoy if the test can tell *which* box the Aspire detail landed on.
#[test]
fn an_aspire_add_project_resolves_a_hyphenated_project_name() {
    let (_dir, _out, gated) = admitted(&[
        (
            "AppHost/AppHost.csproj",
            &csproj("<IsAspireHost>true</IsAspireHost>", &[]),
        ),
        (
            "AppHost/Program.cs",
            "var builder = DistributedApplication.CreateBuilder(args);\n\
             builder.AddProject<Projects.web_frontend>(\"web\");\n\
             builder.Build().Run();\n",
        ),
        ("src/web-frontend/web-frontend.csproj", &web_csproj(&[])),
        // The decoy: its stem transforms to `webfrontend`, not `web_frontend`,
        // so it must never be chosen for `Projects.web_frontend`.
        ("src/webfrontend/webfrontend.csproj", &web_csproj(&[])),
    ]);

    assert!(
        details_of(&gated, "web-frontend")
            .iter()
            .any(|d| d.contains("Aspire")),
        "the hyphenated project was not resolved end-to-end: {:?}",
        details_of(&gated, "web-frontend")
    );
    assert!(
        details_of(&gated, "webfrontend").is_empty(),
        "the near-miss directory was enriched by the app host reference: {:?}",
        details_of(&gated, "webfrontend")
    );
}

/// Two projects whose file stems collide *after* the transform are not
/// disambiguated by guesswork.
///
/// `Orders.Api.csproj` and `Orders_Api.csproj` both transform to `Orders_Api`,
/// so `Projects.Orders_Api` matches both. The `many` arm refuses to attribute
/// the reference to either and warns instead — a wrong arrow is worse than none.
#[test]
fn an_aspire_add_project_ambiguous_after_the_transform_is_not_attributed() {
    let (_dir, out, gated) = admitted(&[
        (
            "AppHost/AppHost.csproj",
            &csproj("<IsAspireHost>true</IsAspireHost>", &[]),
        ),
        (
            "AppHost/Program.cs",
            "builder.AddProject<Projects.Orders_Api>(\"orders\");\n",
        ),
        ("src/OrdersApi/Orders.Api.csproj", &web_csproj(&[])),
        (
            "src/OrdersApiUnderscore/Orders_Api.csproj",
            &web_csproj(&[]),
        ),
    ]);

    assert!(
        !details(&gated).iter().any(|d| d.contains("Aspire")),
        "an ambiguous reference was attributed to a guessed project: {:?}",
        details(&gated)
    );
    assert!(
        out.warnings
            .iter()
            .any(|w| w.contains("Orders_Api") && w.contains("2 scanned projects")),
        "the collision was not reported as a two-way ambiguity: {:?}",
        out.warnings
    );
}

#[test]
fn an_aspire_add_project_naming_no_scanned_project_is_a_warning_and_not_a_component() {
    let (_dir, out, gated) = admitted(&[
        (
            "AppHost/AppHost.csproj",
            &csproj("<IsAspireHost>true</IsAspireHost>", &[]),
        ),
        (
            "AppHost/Program.cs",
            "builder.AddProject<Projects.Billing_Api>(\"billing\");\n",
        ),
    ]);

    assert_eq!(
        labels(&gated, ComponentKind::HttpService),
        ["AppHost"],
        "a component was invented for an identifier that resolved to nothing"
    );
    assert!(
        out.warnings.iter().any(|w| w.contains("Billing_Api")),
        "an unresolved app host reference was dropped silently: {:?}",
        out.warnings
    );
}

#[test]
fn a_commented_out_add_project_is_not_a_signal() {
    let (_dir, out, gated) = admitted(&[
        (
            "AppHost/AppHost.csproj",
            &csproj("<IsAspireHost>true</IsAspireHost>", &[]),
        ),
        (
            "AppHost/Program.cs",
            "// builder.AddProject<Projects.Orders_Api>(\"orders\");\n\
             /* builder.AddProject<Projects.Orders_Api>(\"orders\"); */\n",
        ),
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
    ]);

    assert!(
        details(&gated).is_empty(),
        "a commented-out registration was read as a live one: {:?}",
        details(&gated)
    );
    assert!(
        !out.warnings.iter().any(|w| w.contains("Orders_Api")),
        "a comment was reported as an unresolved reference: {:?}",
        out.warnings
    );
    // Asserted on the raw output as well as on the gate's: the gate would
    // refuse an excerpt beginning `//` on its own (`NotADeclaration`), so a
    // test that only looked at components would pass with the comment scanner
    // removed entirely.
    assert!(
        cited_source_files(&out).is_empty(),
        "the producer emitted a signal from commented-out source: {:?}",
        cited_source_files(&out)
    );
}

// ---------------------------------------------------------------------------
// MEDIUM: connectionStrings keys
// ---------------------------------------------------------------------------

#[test]
fn a_connection_string_key_enriches_the_only_database_the_project_declares() {
    let (_dir, _out, gated) = admitted(&[
        ("src/Orders.Api/Orders.Api.csproj", &csproj("", &["Npgsql"])),
        (
            "src/Orders.Api/appsettings.json",
            r#"{
  "ConnectionStrings": {
    "Orders": "Host=db.internal;Username=svc;Password=hunter2"
  }
}"#,
        ),
    ]);

    assert_eq!(labels(&gated, ComponentKind::Database), ["PostgreSQL"]);
    assert!(
        details(&gated).iter().any(|d| d.contains("Orders")),
        "the key the author chose did not reach the database it names: {:?}",
        details(&gated)
    );
}

#[test]
fn a_connection_string_key_never_creates_a_database() {
    let (_dir, out, gated) = admitted(&[
        ("src/App/App.csproj", &csproj("", &[])),
        (
            "src/App/appsettings.json",
            r#"{"ConnectionStrings": {"Orders": "Host=db.internal;Password=hunter2"}}"#,
        ),
    ]);

    assert!(
        gated.components.is_empty(),
        "a connection string created a database nothing declared: {:?}",
        gated.components
    );
    assert!(
        out.warnings.iter().any(|w| w.contains("Orders")),
        "the refusal was silent: {:?}",
        out.warnings
    );
}

#[test]
fn a_connection_string_key_is_not_attributed_when_two_providers_are_declared() {
    let (_dir, out, gated) = admitted(&[
        (
            "src/App/App.csproj",
            &csproj("", &["Npgsql", "MongoDB.Driver"]),
        ),
        (
            "src/App/appsettings.json",
            r#"{"ConnectionStrings": {"Orders": "Host=db.internal;Password=hunter2"}}"#,
        ),
    ]);

    assert!(
        details(&gated).is_empty(),
        "a key was attached to one of two providers by guesswork: {:?}",
        details(&gated)
    );
    assert!(
        out.warnings.iter().any(|w| w.contains("Orders")),
        "the ambiguity was not reported: {:?}",
        out.warnings
    );
}

/// The audit's exact scenario: one project, two clients of *different kinds*.
///
/// The candidate set used to be the declared **databases** only, so a project
/// referencing `Npgsql` and `StackExchange.Redis` had exactly one database and
/// every key in the file was attached to it — producing the pairing
/// "PostgreSQL — connection string 'Redis'", which is false about the one thing
/// a connection string is for.
#[test]
fn a_connection_string_key_never_lands_on_a_provider_when_another_kind_of_client_is_declared() {
    let (_dir, out, gated) = admitted(&[
        (
            "src/Orders.Api/Orders.Api.csproj",
            &csproj("", &["Npgsql", "StackExchange.Redis"]),
        ),
        (
            "src/Orders.Api/appsettings.json",
            r#"{
  "ConnectionStrings": {
    "Orders": "Host=db.internal;Password=hunter2",
    "Redis": "redis.internal:6379,abortConnect=false"
  }
}"#,
        ),
    ]);

    assert_eq!(
        labels(&gated, ComponentKind::Database),
        ["PostgreSQL"],
        "the fixture has to draw both boxes or this test passes for the wrong reason"
    );
    assert_eq!(labels(&gated, ComponentKind::Cache), ["Redis"]);

    assert_eq!(
        details_of(&gated, "PostgreSQL"),
        Vec::<String>::new(),
        "a key was attached to a provider the project has another client for"
    );
    assert_eq!(
        details_of(&gated, "Redis"),
        ["connection string 'Redis'"],
        "the key naming its own provider is the one pairing the files state"
    );
    assert!(
        out.warnings.iter().any(|w| w.contains("'Orders'")),
        "the key that could not be placed was dropped silently: {:?}",
        out.warnings
    );
}

/// The matching rule is identity, not resemblance.
///
/// `RedisCache` is what a great many `appsettings.json` files call the Redis
/// connection string, and attaching it would be right most of the time — which
/// is the argument this module refuses everywhere else. A prefix match would
/// also hand `MongoDB` the key `Mongo`, and then `SqlServerReadReplica` to
/// something, and the rule stops being checkable.
#[test]
fn a_connection_string_key_that_merely_resembles_a_provider_is_attached_to_nothing() {
    let (_dir, out, gated) = admitted(&[
        (
            "src/Orders.Api/Orders.Api.csproj",
            &csproj("", &["Npgsql", "StackExchange.Redis"]),
        ),
        (
            "src/Orders.Api/appsettings.json",
            r#"{"ConnectionStrings": {"RedisCache": "redis.internal:6379"}}"#,
        ),
    ]);

    assert!(
        details(&gated).is_empty(),
        "a key was placed by resemblance: {:?}",
        details(&gated)
    );
    assert!(
        out.warnings.iter().any(|w| w.contains("'RedisCache'")),
        "the refusal was silent: {:?}",
        out.warnings
    );
}

/// The recovery clause, on its own, with only one client declared.
///
/// A single declared store already places every key by count, so this fixture
/// exists to pin the *other* half: punctuation and case in the provider label
/// do not have to be reproduced by the author for the names to be the same name.
#[test]
fn a_connection_string_key_names_its_provider_across_case_and_punctuation() {
    let (_dir, _out, gated) = admitted(&[
        (
            "src/Orders.Api/Orders.Api.csproj",
            &csproj("", &["Microsoft.Data.SqlClient", "StackExchange.Redis"]),
        ),
        (
            "src/Orders.Api/appsettings.json",
            r#"{"ConnectionStrings": {"sqlserver": "Server=.;Database=x"}}"#,
        ),
    ]);

    assert_eq!(
        details_of(&gated, "SQL Server"),
        ["connection string 'sqlserver'"],
        "'sqlserver' and 'SQL Server' are the same name written two ways: {:?}",
        details(&gated)
    );
}

#[test]
fn an_appsettings_file_that_is_not_json_is_reported_rather_than_guessed_at() {
    let (_dir, out, _gated) = admitted(&[
        ("src/App/App.csproj", &csproj("", &["Npgsql"])),
        ("src/App/appsettings.json", "{ this is not json"),
    ]);

    assert!(
        out.warnings.iter().any(|w| w.contains("appsettings.json")),
        "an unreadable configuration file was passed over in silence: {:?}",
        out.warnings
    );
}

// ---------------------------------------------------------------------------
// MEDIUM: AddHttpClient
// ---------------------------------------------------------------------------

#[test]
fn an_httpclient_with_no_resolvable_base_address_yields_no_edge() {
    let (_dir, out, gated) = admitted(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        (
            "src/Orders.Api/Program.cs",
            "builder.Services.AddHttpClient(\"billing\");\n",
        ),
        ("src/Billing.Api/Billing.Api.csproj", &web_csproj(&[])),
    ]);

    assert_eq!(
        gated.edges().len(),
        2,
        "the only edges should be the two services' own declarations: {:?}",
        gated.edges()
    );
    assert!(
        details(&gated).is_empty(),
        "a named client with no address enriched something: {:?}",
        details(&gated)
    );
    assert!(
        out.warnings.iter().any(|w| w.contains("AddHttpClient")),
        "an unresolved client was dropped silently: {:?}",
        out.warnings
    );
}

#[test]
fn an_httpclient_base_address_bound_to_configuration_yields_no_signal() {
    let (_dir, out, gated) = admitted(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        (
            "src/Orders.Api/Program.cs",
            "builder.Services.AddHttpClient(\"billing\", c =>\n\
             {\n\
             c.BaseAddress = new Uri(builder.Configuration[\"Services:Billing\"]!);\n\
             });\n",
        ),
        ("src/Billing.Api/Billing.Api.csproj", &web_csproj(&[])),
    ]);

    assert!(
        details(&gated).is_empty(),
        "a configuration-bound address was resolved anyway: {:?}",
        details(&gated)
    );
    assert!(
        out.warnings.iter().any(|w| w.contains("AddHttpClient")),
        "{:?}",
        out.warnings
    );
}

#[test]
fn an_httpclient_base_address_matching_an_application_url_enriches_that_service() {
    let (_dir, _out, gated) = admitted(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        (
            "src/Orders.Api/Program.cs",
            "builder.Services.AddHttpClient(\"billing\", c =>\n\
             {\n\
             c.BaseAddress = new Uri(\"https://localhost:7080\");\n\
             });\n",
        ),
        ("src/Billing.Api/Billing.Api.csproj", &web_csproj(&[])),
        (
            "src/Billing.Api/Properties/launchSettings.json",
            &launch_settings("https://localhost:7080"),
        ),
    ]);

    let details = details(&gated);
    assert!(
        details.iter().any(|d| d.contains("Orders.Api")),
        "the caller was not recorded against the service it calls: {details:?}"
    );
}

#[test]
fn an_httpclient_base_address_matching_nothing_creates_no_service() {
    let (_dir, _out, gated) = admitted(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        (
            "src/Orders.Api/Program.cs",
            "builder.Services.AddHttpClient(\"billing\", c =>\n\
             {\n\
             c.BaseAddress = new Uri(\"https://billing.example.com\");\n\
             });\n",
        ),
    ]);

    assert_eq!(
        labels(&gated, ComponentKind::HttpService),
        ["Orders.Api"],
        "an address that matched no project invented a service: {:?}",
        gated.components
    );
}

#[test]
fn a_wildcard_application_url_binding_matches_no_client() {
    let (_dir, _out, gated) = admitted(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        (
            "src/Orders.Api/Program.cs",
            "builder.Services.AddHttpClient(\"billing\", c =>\n\
             {\n\
             c.BaseAddress = new Uri(\"http://localhost:5080\");\n\
             });\n",
        ),
        ("src/Billing.Api/Billing.Api.csproj", &web_csproj(&[])),
        (
            "src/Billing.Api/Properties/launchSettings.json",
            &launch_settings("http://+:5080"),
        ),
    ]);

    assert!(
        details(&gated).iter().all(|d| !d.contains("Orders.Api")),
        "a wildcard binding was matched against a specific host: {:?}",
        details(&gated)
    );
}

#[test]
fn a_commented_out_registration_is_not_a_signal() {
    let (_dir, out, gated) = admitted(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        (
            "src/Orders.Api/Program.cs",
            "// builder.Services.AddHttpClient(\"billing\", c => c.BaseAddress = new Uri(\"https://localhost:7080\"));\n",
        ),
        ("src/Billing.Api/Billing.Api.csproj", &web_csproj(&[])),
        (
            "src/Billing.Api/Properties/launchSettings.json",
            &launch_settings("https://localhost:7080"),
        ),
    ]);

    assert!(
        details(&gated).iter().all(|d| !d.contains("Orders.Api")),
        "a commented-out registration was read as a live one: {:?}",
        details(&gated)
    );
    assert!(
        out.warnings.is_empty(),
        "a comment was reported as an unresolved registration: {:?}",
        out.warnings
    );
    // See the note in `a_commented_out_add_project_is_not_a_signal`: the gate
    // would catch this one too, and this producer must not rely on it.
    assert!(
        cited_source_files(&out).is_empty(),
        "the producer emitted a signal from commented-out source: {:?}",
        cited_source_files(&out)
    );
}

#[test]
fn an_httpclient_never_produces_a_high_signal_from_a_source_file() {
    let (_dir, out, _gated) = admitted(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        (
            "src/Orders.Api/Program.cs",
            "builder.Services.AddHttpClient(\"billing\", c =>\n\
             {\n\
             c.BaseAddress = new Uri(\"https://localhost:7080\");\n\
             });\n",
        ),
        ("src/Billing.Api/Billing.Api.csproj", &web_csproj(&[])),
        (
            "src/Billing.Api/Properties/launchSettings.json",
            &launch_settings("https://localhost:7080"),
        ),
    ]);

    for signal in &out.signals {
        if signal.evidence.path.to_string_lossy().ends_with(".cs") {
            assert_eq!(
                signal.strength,
                Strength::Medium,
                "a source file was cited as a declared fact: {signal:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Standing properties of the producer
// ---------------------------------------------------------------------------

/// The module documentation promises a cap produces a warning rather than a
/// quietly shorter answer; this is that promise executed.
#[test]
fn a_source_file_over_the_byte_cap_is_reported_rather_than_silently_skipped() {
    let mut huge = String::from(
        "builder.Services.AddHttpClient(\"billing\");
",
    );
    huge.push_str(
        &"// padding
"
        .repeat(60_000),
    );

    let (_dir, out, _gated) = admitted(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        ("src/Orders.Api/Huge.cs", &huge),
    ]);

    assert!(
        out.warnings.iter().any(|w| w.contains("Huge.cs")),
        "an unread file was passed over in silence: {:?}",
        out.warnings
    );
    assert!(
        !out.warnings.iter().any(|w| w.contains("AddHttpClient")),
        "a file that was never read still produced findings: {:?}",
        out.warnings
    );
}

#[test]
fn a_producer_warning_never_repeats_a_value_it_read() {
    let (_dir, out, _gated) = admitted(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        (
            "src/Orders.Api/appsettings.json",
            r#"{"ConnectionStrings": {"Orders": "Host=db.internal;Password=hunter2"}}"#,
        ),
        (
            "src/Orders.Api/Program.cs",
            "builder.Services.AddHttpClient(\"billing\", c =>\n\
             {\n\
             c.BaseAddress = new Uri(\"https://billing.internal:9443\");\n\
             });\n",
        ),
    ]);

    let warnings = format!("{:?}", out.warnings);
    for forbidden in ["db.internal", "hunter2", "billing.internal", "9443"] {
        assert!(
            !warnings.contains(forbidden),
            "'{forbidden}' was read out of a file and repeated in a warning: {warnings}"
        );
    }
}

#[test]
fn a_project_that_is_not_dotnet_is_ignored_entirely() {
    let (_dir, out, gated) = admitted(&[(
        "web/package.json",
        r#"{"name": "web", "dependencies": {"pg": "^8.0.0"}}"#,
    )]);

    assert!(gated.components.is_empty(), "{:?}", gated.components);
    assert!(out.signals.is_empty(), "{:?}", out.signals);
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn the_same_workspace_produces_the_same_signals_twice() {
    let api = web_csproj(&["Npgsql"]);
    let (_dir, ws) = scanned(&[
        ("src/Orders.Api/Orders.Api.csproj", &api),
        (
            "src/Orders.Api/appsettings.json",
            r#"{"ConnectionStrings": {"Orders": "Host=x;Password=y"}}"#,
        ),
        ("src/Billing.Api/Billing.Api.csproj", &api),
    ]);

    let first = signals(&ws);
    let second = signals(&ws);

    assert_eq!(first, second, "the producer is not deterministic");
}

#[test]
fn an_unreadable_project_file_is_reported_rather_than_skipped() {
    let (dir, ws) = scanned(&[("src/App/App.csproj", &csproj("", &["Npgsql"]))]);
    // Replace the manifest with a directory of the same name: the scan already
    // happened, so the project is known and its file can no longer be read.
    let manifest = dir.path().join("src/App/App.csproj");
    std::fs::remove_file(&manifest).unwrap();
    std::fs::create_dir(&manifest).unwrap();

    let out = signals(&ws);

    assert!(out.signals.is_empty(), "{:?}", out.signals);
    assert!(
        out.warnings.iter().any(|w| w.contains("App.csproj")),
        "a manifest that could not be read was passed over: {:?}",
        out.warnings
    );
}

/// Pins the assumption the module documentation makes about
/// [`crate::symbols`], in the module that would be wrong if it changed.
///
/// The producer is told to consume the symbol index rather than re-scan source.
/// It does not, and the stated reason is that the index cannot hold these
/// lines: they are calls, and the index only records declarations. If the
/// declaration heuristic ever grew to name a call, this test fails and that
/// paragraph of the module doc becomes false — which is the point of it.
#[test]
fn the_symbol_index_holds_no_declaration_for_the_lines_this_producer_reads() {
    for line in [
        "builder.Services.AddHttpClient(\"billing\", c => c.BaseAddress = new Uri(\"http://x\"));",
        "builder.AddProject<Projects.Orders_Api>(\"orders\");",
        "    c.BaseAddress = new Uri(\"https://localhost:7080\");",
    ] {
        assert_eq!(
            crate::symbols::declarations::declaration(line),
            None,
            "the symbol index would now hold {line}, so this producer could consume it"
        );
    }
}
