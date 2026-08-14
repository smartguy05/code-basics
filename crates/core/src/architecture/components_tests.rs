//! Tests for [`super::components`].
//!
//! Every test builds a real temporary workspace, runs the real
//! [`crate::workspace::scan`] and the real [`crate::symbols::index::build`]
//! over it, and asserts on the [`ArchGraph`] the assembly produced — for the
//! same reason [`super::graph_tests`] and the producers' own suites do it:
//! this file's whole job is to line three producers' output up with what the
//! scan found, so a test that fabricated either side would be testing the
//! fabrication.
//!
//! The assertions are deliberately made on the *graph*, never on the signal
//! list. What a producer emits is not what a user sees — a MEDIUM signal with
//! no HIGH counterpart is emitted and then refused — and a test that stopped
//! at the signals could pass while the diagram stayed empty or while a box
//! appeared that the grading rule forbids.

use std::collections::BTreeSet;

use super::components::component_graph;
use super::graph::{ArchGraph, ArchKind, ArchNode, EdgeKind};
use crate::symbols::index::{build, SymbolIndex};
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

fn scanned(files: &[(&str, &str)]) -> (tempfile::TempDir, Workspace, SymbolIndex) {
    let dir = workspace_with(files);
    let ws = scan(dir.path()).unwrap();
    let index = build(dir.path(), &ws.projects);
    (dir, ws, index)
}

/// The graph, with the [`tempfile::TempDir`] kept alive for the test's length.
fn mapped(files: &[(&str, &str)]) -> (tempfile::TempDir, ArchGraph) {
    let (dir, ws, index) = scanned(files);
    let graph = component_graph(&ws, &index);
    (dir, graph)
}

fn lib_csproj(packages: &[&str]) -> String {
    sdk_csproj("Microsoft.NET.Sdk", packages)
}

fn web_csproj(packages: &[&str]) -> String {
    sdk_csproj("Microsoft.NET.Sdk.Web", packages)
}

fn sdk_csproj(sdk: &str, packages: &[&str]) -> String {
    let items: String = packages
        .iter()
        .map(|p| format!("    <PackageReference Include=\"{p}\" Version=\"1.0.0\" />\n"))
        .collect();
    format!(
        "<Project Sdk=\"{sdk}\">\n  <PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup>\n  <ItemGroup>\n{items}  </ItemGroup>\n</Project>"
    )
}

fn launch_settings(url: &str) -> String {
    format!(
        r#"{{
  "profiles": {{
    "http": {{ "commandName": "Project", "applicationUrl": "{url}" }}
  }}
}}"#
    )
}

fn nodes_of(graph: &ArchGraph, kind: ArchKind) -> Vec<&ArchNode> {
    graph.nodes.iter().filter(|n| n.kind == kind).collect()
}

fn labels_of(graph: &ArchGraph, kind: ArchKind) -> Vec<&str> {
    nodes_of(graph, kind)
        .into_iter()
        .map(|n| n.label.as_str())
        .collect()
}

fn edges_of(graph: &ArchGraph, kind: EdgeKind) -> Vec<(&str, &str)> {
    graph
        .edges
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect()
}

fn id_of<'a>(graph: &'a ArchGraph, label: &str) -> &'a str {
    graph
        .nodes
        .iter()
        .find(|n| n.label == label)
        .unwrap_or_else(|| panic!("no node labelled {label} in {:?}", labels(graph)))
        .id
        .as_str()
}

fn labels(graph: &ArchGraph) -> Vec<&str> {
    graph.nodes.iter().map(|n| n.label.as_str()).collect()
}

/// Whether any warning mentions every one of these fragments.
fn warned_about(graph: &ArchGraph, fragments: &[&str]) -> bool {
    graph
        .warnings
        .iter()
        .any(|w| fragments.iter().all(|f| w.contains(f)))
}

/// Every string a consumer can reach from a component map, concatenated.
///
/// The three exits are all here on purpose. The `Debug` form covers every field
/// of every node, edge and warning; [`super::mermaid::render`] is what gets
/// written into `.code-basics/diagrams/` and committed, and it copies the
/// warnings in as `%%` comments; and the serde JSON is what crosses IPC into
/// the frontend. A leak-sweep that checked only one of them is how a credential
/// stayed in the exported diagram while the node labels looked clean — so the
/// assertion below is deliberately over the whole surface rather than over the
/// one field a fix happened to touch.
fn everything_reachable(graph: &ArchGraph) -> String {
    format!(
        "{graph:?}\n{}\n{}",
        super::mermaid::render(graph),
        serde_json::to_string(graph).unwrap()
    )
}

/// Assert that none of `secrets` appears anywhere a consumer can see.
#[track_caller]
fn leaks_nothing(graph: &ArchGraph, secrets: &[&str]) {
    let everything = everything_reachable(graph);
    for secret in secrets {
        assert!(
            !everything.contains(secret),
            "{secret:?} reached a string exported from the component map:\n{everything}"
        );
    }
}

// ---------------------------------------------------------------------------
// Level 1 and level 2 are different questions
// ---------------------------------------------------------------------------

#[test]
fn a_workspace_with_no_high_signals_produces_an_empty_map_rather_than_a_project_map() {
    // Two ordinary class libraries, one referencing the other. The *project*
    // map draws two boxes and an arrow for this. The component map must draw
    // nothing: neither project serves anything and neither declares a store,
    // so this system has no components that were written down. Falling back to
    // the project map here would answer a question nobody asked while looking
    // exactly like an answer to the one they did.
    let (_dir, graph) = mapped(&[
        (
            "src/Core/Core.csproj",
            &sdk_csproj("Microsoft.NET.Sdk", &["Newtonsoft.Json"]),
        ),
        ("src/Util/Util.csproj", &lib_csproj(&[])),
    ]);

    assert!(
        graph.nodes.is_empty(),
        "a workspace with no declared component must produce no boxes, got {:?}",
        labels(&graph)
    );
    assert!(graph.edges.is_empty(), "{:?}", graph.edges);
}

#[test]
fn an_empty_map_is_still_derived_at_the_current_scanner_version() {
    // The emptiness is an answer, not an absence of one, so it has to carry
    // the same provenance a populated map does — otherwise a stored empty
    // graph is indistinguishable from a graph nothing ever ran over.
    let (_dir, graph) = mapped(&[("src/Util/Util.csproj", &lib_csproj(&[]))]);

    assert_eq!(
        graph.derivation,
        super::graph::Derivation::Derived {
            scanner: super::graph::SCANNER_VERSION
        }
    );
}

// ---------------------------------------------------------------------------
// Services
// ---------------------------------------------------------------------------

#[test]
fn a_web_project_becomes_a_service_node_and_an_ordinary_library_becomes_nothing() {
    let (_dir, graph) = mapped(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        ("src/Orders.Core/Orders.Core.csproj", &lib_csproj(&[])),
    ]);

    assert_eq!(labels_of(&graph, ArchKind::Service), ["Orders.Api"]);
    assert_eq!(
        labels(&graph),
        ["Orders.Api"],
        "the class library declared nothing and must not be drawn"
    );
}

#[test]
fn a_service_node_carries_the_same_id_path_and_ecosystem_the_project_map_gives_it() {
    // The two maps are read by one UI. A project that is the same project in
    // both must be the same node in both, or moving between them needs a
    // lookup table that does not exist.
    let (dir, ws, index) = scanned(&[("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[]))]);
    let components = component_graph(&ws, &index);
    let projects = super::graph::project_graph(&ws);
    drop(dir);

    let service = &components.nodes[0];
    let project = projects
        .nodes
        .iter()
        .find(|n| n.label == "Orders.Api")
        .unwrap();

    assert_eq!(service.id, project.id);
    assert_eq!(service.project_id, project.project_id);
    assert_eq!(service.path, project.path);
    assert_eq!(service.ecosystem, project.ecosystem);
    assert_ne!(
        service.kind, project.kind,
        "the kind is the one field that is allowed to differ"
    );
}

#[test]
fn a_node_project_serving_http_becomes_a_service_node() {
    let (_dir, graph) = mapped(&[(
        "apps/web/package.json",
        r#"{ "name": "billing-api", "dependencies": { "express": "^4.0.0" } }"#,
    )]);

    // The label is the scan's project name, which for a `package.json` is the
    // `name` field rather than the directory — so this is `billing-api`, not
    // `web`. The component map deliberately does not re-derive it: a project
    // that is the same project in both maps must be labelled the same in both.
    assert_eq!(labels_of(&graph, ArchKind::Service), ["billing-api"]);
    assert_eq!(
        graph.nodes[0].ecosystem.as_deref(),
        Some("node"),
        "{:?}",
        graph.nodes
    );
}

#[test]
fn a_project_that_is_both_a_service_and_a_data_client_is_drawn_once_as_a_service() {
    let (_dir, graph) = mapped(&[("src/Orders.Api/Orders.Api.csproj", &web_csproj(&["Npgsql"]))]);

    assert_eq!(labels_of(&graph, ArchKind::Service), ["Orders.Api"]);
    assert!(
        nodes_of(&graph, ArchKind::Project).is_empty(),
        "a service must not also appear as a plain project: {:?}",
        graph.nodes
    );
}

// ---------------------------------------------------------------------------
// Data stores
// ---------------------------------------------------------------------------

#[test]
fn a_package_reference_to_a_database_client_becomes_a_store_labelled_by_provider() {
    let (_dir, graph) = mapped(&[("src/Orders.Api/Orders.Api.csproj", &web_csproj(&["Npgsql"]))]);

    assert_eq!(
        labels_of(&graph, ArchKind::DataStore),
        ["PostgreSQL"],
        "the box is the provider the package names and nothing else"
    );
}

#[test]
fn a_declared_client_is_joined_to_its_store_by_a_data_access_edge() {
    let (_dir, graph) = mapped(&[(
        "src/Orders.Api/Orders.Api.csproj",
        &web_csproj(&["StackExchange.Redis"]),
    )]);

    let service = id_of(&graph, "Orders.Api");
    let store = id_of(&graph, "Redis");
    assert_eq!(edges_of(&graph, EdgeKind::DataAccess), [(service, store)]);
}

#[test]
fn two_projects_declaring_the_same_provider_share_one_store_with_two_edges() {
    // One box, because the box is the technology: "PostgreSQL is spoken here".
    // Two boxes would assert two distinct databases, which neither manifest
    // says.
    let (_dir, graph) = mapped(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&["Npgsql"])),
        (
            "src/Billing.Api/Billing.Api.csproj",
            &web_csproj(&["Npgsql.EntityFrameworkCore.PostgreSQL"]),
        ),
    ]);

    assert_eq!(labels_of(&graph, ArchKind::DataStore), ["PostgreSQL"]);
    assert_eq!(edges_of(&graph, EdgeKind::DataAccess).len(), 2);
}

#[test]
fn a_class_library_that_speaks_a_protocol_is_drawn_as_a_project_not_as_a_service() {
    // The reference is a declared fact and earns the store its box, so the
    // library has to be drawn or the arrow comes from nowhere. It is not a
    // service: nothing in that project file says it listens on anything.
    let (_dir, graph) = mapped(&[(
        "src/Orders.Data/Orders.Data.csproj",
        &lib_csproj(&["MongoDB.Driver"]),
    )]);

    assert_eq!(labels_of(&graph, ArchKind::Project), ["Orders.Data"]);
    assert!(
        nodes_of(&graph, ArchKind::Service).is_empty(),
        "{:?}",
        graph.nodes
    );
    assert_eq!(edges_of(&graph, EdgeKind::DataAccess).len(), 1);
}

#[test]
fn a_data_store_node_never_carries_a_project_a_path_or_an_ecosystem() {
    // Every one of these would be a claim about where the store lives, which
    // no file in the workspace makes. The obvious slip is to copy the
    // declaring project's details onto it.
    let (_dir, graph) = mapped(&[(
        "src/Orders.Api/Orders.Api.csproj",
        &web_csproj(&["Confluent.Kafka"]),
    )]);

    let store = nodes_of(&graph, ArchKind::DataStore)[0];
    assert_eq!(store.label, "Kafka");
    assert_eq!(store.project_id, None);
    assert_eq!(store.path, None);
    assert_eq!(store.ecosystem, None);
    assert!(
        store.id.starts_with("component:"),
        "a store id must not be able to collide with a project id: {}",
        store.id
    );
}

#[test]
fn a_node_workspace_data_client_earns_a_store_the_same_way_a_dotnet_one_does() {
    let (_dir, graph) = mapped(&[(
        "apps/api/package.json",
        r#"{ "name": "api", "dependencies": { "fastify": "^4.0.0", "ioredis": "^5.0.0" } }"#,
    )]);

    assert_eq!(labels_of(&graph, ArchKind::DataStore), ["Redis"]);
    assert_eq!(edges_of(&graph, EdgeKind::DataAccess).len(), 1);
}

// ---------------------------------------------------------------------------
// MEDIUM may never create
// ---------------------------------------------------------------------------

#[test]
fn a_dev_only_data_client_creates_no_store_and_is_reported_as_refused() {
    // A `devDependencies` entry is a MEDIUM signal. With no HIGH signal naming
    // the same component there is nothing for it to enrich, so it must produce
    // no box, no arrow, and a warning saying it was seen.
    let (_dir, graph) = mapped(&[(
        "apps/api/package.json",
        r#"{ "name": "api", "dependencies": { "express": "^4.0.0" }, "devDependencies": { "ioredis": "^5.0.0" } }"#,
    )]);

    assert!(
        nodes_of(&graph, ArchKind::DataStore).is_empty(),
        "a devDependency must not create a component: {:?}",
        graph.nodes
    );
    assert!(
        warned_about(&graph, &["Redis", "medium-without-high"]),
        "the refusal must be visible: {:?}",
        graph.warnings
    );
}

#[test]
fn a_route_never_brings_a_service_into_existence() {
    // A controller in a project whose SDK does not say it serves HTTP. The
    // route producer emits MEDIUM signals for it; nothing may come of them.
    const CONTROLLER: &str = "using Microsoft.AspNetCore.Mvc;\n\
         [Route(\"api/orders\")]\n\
         public class OrdersController : ControllerBase\n{\n\
         \x20   [HttpGet]\n\
         \x20   public string Get() => \"ok\";\n}\n";

    let (_dir, graph) = mapped(&[
        ("src/Orders.Core/Orders.Core.csproj", &lib_csproj(&[])),
        ("src/Orders.Core/OrdersController.cs", CONTROLLER),
    ]);

    assert!(
        graph.nodes.is_empty(),
        "a route list must not be able to declare a service: {:?}",
        labels(&graph)
    );

    // The same controller under a web SDK. This half is what proves the first
    // half is not passing because the fixture is inert: the routes are found
    // either way, and only the project file decides whether a box appears.
    let (_dir, web) = mapped(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        ("src/Orders.Api/OrdersController.cs", CONTROLLER),
    ]);

    assert_eq!(labels_of(&web, ArchKind::Service), ["Orders.Api"]);
}

#[test]
fn a_matched_base_address_is_reported_as_a_note_and_never_drawn_as_an_arrow() {
    // The strongest cross-project inference the phase can make: a literal
    // `BaseAddress` matching exactly one other project's `applicationUrl`.
    // Both halves are strings an author wrote, and it is still refused as an
    // edge, because the line it was read from is a `.cs` file rather than a
    // manifest. The fact is reported instead of being drawn.
    let (_dir, graph) = mapped(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        (
            "src/Orders.Api/Properties/launchSettings.json",
            &launch_settings("http://localhost:5101"),
        ),
        ("src/Orders.Web/Orders.Web.csproj", &web_csproj(&[])),
        (
            "src/Orders.Web/Program.cs",
            "var b = WebApplication.CreateBuilder(args);\n\
             b.Services.AddHttpClient(\"orders\", c =>\n{\n\
             \x20   c.BaseAddress = new Uri(\"http://localhost:5101\");\n});\n",
        ),
    ]);

    let mut services = labels_of(&graph, ArchKind::Service);
    services.sort();
    assert_eq!(services, ["Orders.Api", "Orders.Web"]);
    assert!(
        graph.edges.is_empty(),
        "a supporting signal must never bring an edge into existence: {:?}",
        graph.edges
    );
    assert!(
        warned_about(
            &graph,
            &[
                "called over HTTP by Orders.Web",
                "Orders.Api",
                "note rather than an arrow"
            ],
        ),
        "the call has to be reported since it is not drawn: {:?}",
        graph.warnings
    );
}

/// One warning list, one vocabulary — the one printed on the boxes.
///
/// The producers' warnings have always named `Orders.Web`, the project's
/// display name, because they hold the [`crate::model::Project`]. This file's
/// notes named `src-Orders.Web-Orders.Web.csproj`, the scan's internal id,
/// because a [`super::signals::framework::Detail`] carries only that. Both land
/// in `ArchGraph::warnings` side by side, and the id is the half a reader
/// cannot match to anything on the diagram — it is not drawn, not labelled, and
/// not a path they can open.
#[test]
fn a_cross_project_note_names_the_project_the_way_the_diagram_labels_it() {
    let (_dir, graph) = mapped(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        (
            "src/Orders.Api/Properties/launchSettings.json",
            &launch_settings("http://localhost:5102"),
        ),
        ("src/Orders.Web/Orders.Web.csproj", &web_csproj(&[])),
        (
            "src/Orders.Web/Program.cs",
            "var b = WebApplication.CreateBuilder(args);\n\
             b.Services.AddHttpClient(\"orders\", c =>\n{\n\
             \x20   c.BaseAddress = new Uri(\"http://localhost:5102\");\n});\n",
        ),
    ]);

    assert!(
        warned_about(&graph, &["Orders.Web: 'called over HTTP by Orders.Web'"]),
        "the note has to open with the name on the box: {:?}",
        graph.warnings
    );
    assert!(
        !graph
            .warnings
            .iter()
            .any(|w| w.contains("src-Orders.Web-Orders.Web.csproj")),
        "a raw project id reached a warning a person reads: {:?}",
        graph.warnings
    );
}

#[test]
fn a_note_about_a_project_never_repeats_the_address_it_was_read_from() {
    // The excerpt behind that note is a literal `host:port`. Naming the file
    // is what makes the claim checkable; copying the address into a warning
    // that gets exported is not part of the job.
    let (_dir, graph) = mapped(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        (
            "src/Orders.Api/Properties/launchSettings.json",
            &launch_settings("http://localhost:5199"),
        ),
        ("src/Orders.Web/Orders.Web.csproj", &web_csproj(&[])),
        (
            "src/Orders.Web/Program.cs",
            "b.Services.AddHttpClient(\"orders\", c =>\n{\n\
             \x20   c.BaseAddress = new Uri(\"http://localhost:5199\");\n});\n",
        ),
    ]);

    for warning in &graph.warnings {
        assert!(
            !warning.contains("5199"),
            "a warning repeated an address it read: {warning}"
        );
    }
}

#[test]
fn a_projects_own_supporting_details_are_not_reported_as_missing_arrows() {
    // A route list and a launch profile enrich a box the same project already
    // earned. Nothing about the picture changes whether they are there, so
    // reporting them would bury the refusals a reader has to see.
    let (_dir, graph) = mapped(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&[])),
        (
            "src/Orders.Api/Properties/launchSettings.json",
            &launch_settings("http://localhost:5001"),
        ),
    ]);

    assert_eq!(
        labels_of(&graph, ArchKind::Service),
        ["Orders.Api"],
        "the fixture has to draw the box the detail attached to, or the \
         assertion below passes for the wrong reason"
    );
    assert!(
        !warned_about(&graph, &["note rather than an arrow"]),
        "a project's own enrichment is not a missing arrow: {:?}",
        graph.warnings
    );
}

// ---------------------------------------------------------------------------
// Everything refused is counted
// ---------------------------------------------------------------------------

#[test]
fn a_producer_refusal_reaches_the_graphs_warnings() {
    // `Microsoft.EntityFrameworkCore.InMemory` is refused by the .NET producer
    // itself, before the gate ever sees it. Without this the refusal would be
    // invisible, which is the one outcome the phase rules out.
    let (_dir, graph) = mapped(&[(
        "src/Orders.Api/Orders.Api.csproj",
        &web_csproj(&["Microsoft.EntityFrameworkCore.InMemory"]),
    )]);

    assert!(
        nodes_of(&graph, ArchKind::DataStore).is_empty(),
        "{:?}",
        graph.nodes
    );
    assert!(warned_about(&graph, &["InMemory"]), "{:?}", graph.warnings);
}

#[test]
fn a_gate_refusal_and_a_producer_refusal_both_reach_the_same_list() {
    let (_dir, graph) = mapped(&[
        (
            "src/Orders.Api/Orders.Api.csproj",
            &web_csproj(&["Testcontainers.PostgreSql"]),
        ),
        (
            "apps/api/package.json",
            r#"{ "name": "api", "devDependencies": { "kafkajs": "^2.0.0" } }"#,
        ),
    ]);

    assert!(
        warned_about(&graph, &["Testcontainers"]),
        "the producer's refusal is missing: {:?}",
        graph.warnings
    );
    assert!(
        warned_about(&graph, &["medium-without-high"]),
        "the gate's refusal is missing: {:?}",
        graph.warnings
    );
}

/// The other half of `a_cross_project_note_names_the_project_the_way_the_
/// diagram_labels_it`, which fixed this file's own notes and left the gate's.
///
/// [`super::signals::framework::admit`]'s refusals land in the same
/// `ArchGraph::warnings` list as the producers' — they are relayed by
/// [`super::components::component_graph`] — and they opened with the
/// [`crate::model::Project::id`] the signal carried, so the list still read in
/// two vocabularies. The gate has no way to translate: a signal carries an id
/// and nothing else, and only the assembly step holds the [`Workspace`]. So the
/// translation belongs at the relay, which is what this pins.
#[test]
fn a_gate_refusal_names_the_project_the_way_the_diagram_labels_it() {
    // A launch profile on a project that is *not* a web SDK emits a MEDIUM
    // HttpService signal with no HIGH behind it, so the gate refuses it — and
    // the refusal carried `src-Orders.Api-Orders.Api.csproj`, which is drawn
    // nowhere, labels nothing and is not even a path the reader can open.
    let (_dir, graph) = mapped(&[
        ("src/Orders.Api/Orders.Api.csproj", &lib_csproj(&[])),
        (
            "src/Orders.Api/Properties/launchSettings.json",
            &launch_settings("http://localhost:5080"),
        ),
    ]);

    assert!(
        warned_about(&graph, &["medium-without-high"]),
        "the fixture has to produce a gate refusal, or the assertions below \
         pass for the wrong reason: {:?}",
        graph.warnings
    );
    assert!(
        warned_about(&graph, &["Orders.Api: ", "medium-without-high"]),
        "the gate's refusal has to open with the name on the box: {:?}",
        graph.warnings
    );
    assert!(
        !graph
            .warnings
            .iter()
            .any(|w| w.contains("src-Orders.Api-Orders.Api.csproj")),
        "a raw scan id reached a warning a person reads: {:?}",
        graph.warnings
    );
}

#[test]
fn a_connection_string_value_never_reaches_the_graph() {
    let (_dir, graph) = mapped(&[
        ("src/Orders.Api/Orders.Api.csproj", &web_csproj(&["Npgsql"])),
        (
            "src/Orders.Api/appsettings.json",
            r#"{ "ConnectionStrings": { "Orders": "Host=db.internal;Port=5432;Password=hunter2" } }"#,
        ),
    ]);

    // The store must be there, or this test proves only that an empty graph
    // leaks nothing.
    assert_eq!(labels_of(&graph, ArchKind::DataStore), ["PostgreSQL"]);

    let everything: String = graph
        .nodes
        .iter()
        .map(|n| format!("{} {}", n.id, n.label))
        .chain(graph.warnings.iter().cloned())
        .collect::<Vec<_>>()
        .join("\n");

    for secret in ["hunter2", "db.internal", "5432"] {
        assert!(
            !everything.contains(secret),
            "{secret:?} reached the graph:\n{everything}"
        );
    }
}

// ---------------------------------------------------------------------------
// Credentials, on every path out of the map
// ---------------------------------------------------------------------------

#[test]
fn a_credentialed_launch_profile_url_reaches_no_string_the_component_map_exports() {
    // `launchSettings.json` is a checked-in file and people do put credentials
    // in its `applicationUrl`. The launch-profile signal is MEDIUM and carries
    // that url as its detail, and a MEDIUM detail whose project did not earn
    // the box is printed verbatim by `cross_project_notes` — so the url lands
    // in `ArchGraph::warnings` and from there in the exported mermaid.
    //
    // Reaching it needs two scanned projects sharing a `Project::name`, which
    // is what makes the fixture look contrived and is entirely ordinary in a
    // solution with a `samples/` copy: the component is keyed on the name, the
    // web project earns it, and the sample's launch profile enriches a box it
    // does not own.
    let (_dir, graph) = mapped(&[
        ("src/Foo/Foo.csproj", &web_csproj(&[])),
        ("samples/Foo/Foo.csproj", &lib_csproj(&[])),
        (
            "samples/Foo/Properties/launchSettings.json",
            &launch_settings("https://launchuser:launchpass77@launch-host.corp.internal:9443"),
        ),
    ]);

    // Without the box this test would prove only that an empty graph leaks
    // nothing.
    assert_eq!(labels_of(&graph, ArchKind::Service), ["Foo"]);
    leaks_nothing(
        &graph,
        &[
            "launchpass77",
            "launchuser",
            "launch-host.corp.internal",
            "9443",
        ],
    );
}

#[test]
fn a_cross_project_note_names_the_file_and_never_quotes_the_text_it_read() {
    // The claim `cross_project_notes` documents about itself. The note must
    // still be there — a refusal nobody is told about is the outcome the phase
    // rules out — and it must locate the evidence by path and line rather than
    // by copying it.
    let (_dir, graph) = mapped(&[
        ("src/Foo/Foo.csproj", &web_csproj(&[])),
        ("samples/Foo/Foo.csproj", &lib_csproj(&[])),
        (
            "samples/Foo/Properties/launchSettings.json",
            &launch_settings("https://admin:s3cr3t-pw@internal-host.example:8443"),
        ),
    ]);

    assert!(
        warned_about(
            &graph,
            &["launchSettings.json", "note rather than an arrow"]
        ),
        "the refused enrichment still has to be reported: {:?}",
        graph.warnings
    );
    leaks_nothing(
        &graph,
        &["s3cr3t-pw", "admin:", "internal-host.example", "8443"],
    );
}

#[test]
fn a_gate_refusal_over_a_value_shaped_label_never_echoes_the_label() {
    // A project whose `package.json` name is a whole credentialed url. The
    // gate refuses the signal for `label-looks-like-a-value` — the reason that
    // exists precisely because the label *is* a value — so the warning that
    // reports the refusal must not repeat it.
    let (_dir, graph) = mapped(&[(
        "apps/api/package.json",
        r#"{ "name": "https://deploy:deploypw42@registry.corp.internal:8443", "dependencies": { "express": "^4.0.0" } }"#,
    )]);

    assert!(
        warned_about(&graph, &["label-looks-like-a-value"]),
        "the fixture must actually reach that refusal: {:?}",
        graph.warnings
    );
    leaks_nothing(
        &graph,
        &["deploypw42", "registry.corp.internal", "deploy:", "8443"],
    );
}

#[test]
fn a_connection_string_key_shaped_like_a_host_is_described_rather_than_quoted() {
    // The key is the author's own label and is normally safe to echo, which is
    // why `nameable` echoes it. But `nameable` guarded a weaker set of shapes
    // than the gate's own `label_looks_like_a_value` did, so a `host:port` key
    // passed it and landed in an exported diagram. Two database clients so the
    // key is refused and the warning is the one that quotes it.
    let (_dir, graph) = mapped(&[
        (
            "src/App/App.csproj",
            &web_csproj(&["Npgsql", "MongoDB.Driver"]),
        ),
        (
            "src/App/appsettings.json",
            r#"{ "ConnectionStrings": { "redis-prod.corp.internal:6380": "irrelevant" } }"#,
        ),
    ]);

    // "data store clients" rather than "database clients": the count the
    // producer applies now spans caches and queues too, because counting only
    // databases is what let a key called `Redis` be attached to PostgreSQL.
    assert!(
        warned_about(
            &graph,
            &["connection string", "declares 2 data store clients"]
        ),
        "the fixture must reach the warning that names the key: {:?}",
        graph.warnings
    );
    leaks_nothing(&graph, &["redis-prod.corp.internal", "6380"]);
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

#[test]
fn the_map_does_not_depend_on_the_order_the_projects_were_scanned_in() {
    // Three producers feed this. If any of them let collection order reach the
    // output, the diagram reshuffles between runs and its git diff becomes
    // unreadable — and two versions of a repository stop being comparable.
    let files: &[(&str, &str)] = &[
        (
            "src/Orders.Api/Orders.Api.csproj",
            &web_csproj(&["Npgsql", "StackExchange.Redis"]),
        ),
        (
            "src/Billing.Api/Billing.Api.csproj",
            &web_csproj(&["Npgsql", "RabbitMQ.Client"]),
        ),
        (
            "src/Orders.Data/Orders.Data.csproj",
            &lib_csproj(&["MongoDB.Driver"]),
        ),
        (
            "apps/web/package.json",
            r#"{ "name": "web", "dependencies": { "next": "^14.0.0", "pg": "^8.0.0" } }"#,
        ),
    ];

    let (dir, mut ws, index) = scanned(files);
    let forwards = component_graph(&ws, &index);

    ws.projects.reverse();
    let backwards = component_graph(&ws, &index);

    ws.projects.rotate_left(2);
    let rotated = component_graph(&ws, &index);
    drop(dir);

    assert_eq!(forwards, backwards);
    assert_eq!(forwards, rotated);
    assert!(!forwards.nodes.is_empty(), "the fixture drew nothing");
}

#[test]
fn nodes_are_sorted_by_id_and_edges_never_repeat() {
    let (_dir, graph) = mapped(&[
        (
            "src/Orders.Api/Orders.Api.csproj",
            &web_csproj(&["Npgsql", "Microsoft.Data.SqlClient"]),
        ),
        (
            "src/Billing.Api/Billing.Api.csproj",
            &web_csproj(&["Npgsql"]),
        ),
    ]);

    let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "ArchGraph documents nodes as sorted by id");

    let unique: BTreeSet<_> = graph
        .edges
        .iter()
        .map(|e| (&e.from, &e.to, e.kind))
        .collect();
    assert_eq!(unique.len(), graph.edges.len(), "{:?}", graph.edges);
}

#[test]
fn every_edge_names_two_nodes_the_graph_actually_contains() {
    // An arrow to a box that is not there is a claim about nothing, and the
    // renderer would drop it and say so. It must never be produced here.
    let (_dir, graph) = mapped(&[
        (
            "src/Orders.Data/Orders.Data.csproj",
            &lib_csproj(&["Npgsql"]),
        ),
        (
            "apps/api/package.json",
            r#"{ "name": "api", "dependencies": { "koa": "^2.0.0", "amqplib": "^0.10.0" } }"#,
        ),
    ]);

    let ids: BTreeSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    for edge in &graph.edges {
        assert!(ids.contains(edge.from.as_str()), "{edge:?} in {ids:?}");
        assert!(ids.contains(edge.to.as_str()), "{edge:?} in {ids:?}");
    }
    assert_eq!(graph.edges.len(), 2, "{:?}", graph.edges);
}

// ---------------------------------------------------------------------------
// The renderer has to be able to draw it
// ---------------------------------------------------------------------------

#[test]
fn a_component_map_renders_to_mermaid_its_own_validator_accepts() {
    let (_dir, graph) = mapped(&[
        (
            "src/Orders.Api/Orders.Api.csproj",
            &web_csproj(&["Npgsql", "StackExchange.Redis"]),
        ),
        (
            "src/Orders.Data/Orders.Data.csproj",
            &lib_csproj(&["MongoDB.Driver"]),
        ),
    ]);

    let source = super::mermaid::render(&graph);
    assert_eq!(super::mermaid::validate(&source), Ok(()), "{source}");
    assert!(
        source.contains("[(\""),
        "a data store must be drawn as a cylinder:\n{source}"
    );
}

/// Every warning names a project the way the diagram does.
///
/// A `Project::id` is a workspace-relative path with its separators replaced
/// by `-`, so `src-Api-Api.csproj` is a string that appears on no box and in
/// no file. A user reading `src-Api-Api.csproj: the controller …` has nothing
/// to map it back to, and the same warning list already spoke both dialects at
/// once — the grading gate's refusals were relayed through the display name
/// while the route and node producers prefixed the raw id.
///
/// This is pinned rather than left to review because it drifted twice: the
/// first pass fixed `cross_project_notes` and left the producers, and the
/// producers were fixed one file at a time. The assertion is mechanical — no
/// warning may contain any scanned project's id — so a new producer that
/// reaches for `project.id` fails here rather than at a reader's expense.
#[test]
fn no_warning_names_a_project_by_its_raw_id() {
    let (_dir, ws, index) = scanned(&[
        (
            "src/Api/Api.csproj",
            r#"<Project Sdk="Microsoft.NET.Sdk.Web"><ItemGroup>
               <PackageReference Include="Npgsql" />
               <PackageReference Include="StackExchange.Redis" />
               </ItemGroup></Project>"#,
        ),
        (
            "src/Api/Controllers/BaseController.cs",
            "[ApiController]\n[Route(\"api/[controller]\")]\npublic abstract class BaseController\n{\n}\n",
        ),
        (
            "src/Api/appsettings.json",
            r#"{ "ConnectionStrings": { "Orders": "Host=h;Database=d" } }"#,
        ),
        (
            "tests/ApiTests/ApiTests.csproj",
            r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup>
               <IsTestProject>true</IsTestProject></PropertyGroup></Project>"#,
        ),
        (
            "tests/ApiTests/T.cs",
            "[ApiController]\n[Route(\"api/t\")]\npublic class TController { [HttpGet] public void G() {} }\n",
        ),
        ("web/package.json", r#"{ "name": "web", "dependencies": { "express": "4" } }"#),
        ("packages/broken/package.json", "{ \"name\": \"broken\", }"),
    ]);

    let graph = component_graph(&ws, &index);
    let ids: Vec<&str> = ws.projects.iter().map(|p| p.id.as_str()).collect();

    // The fixture is only worth anything if it actually produced refusals.
    assert!(
        !graph.warnings.is_empty(),
        "the fixture stopped producing warnings, so this test proves nothing"
    );

    for warning in &graph.warnings {
        for id in &ids {
            // A handful of warnings are *about* an id collision and quote the
            // id as their subject; those also give the real paths, and they
            // say so in their text.
            if warning.contains("project id") || warning.contains("share that id") {
                continue;
            }
            assert!(
                !warning.contains(id),
                "a warning names the project by its raw id '{id}', which appears on no box \
                 and in no file:\n  {warning}"
            );
        }
    }
}
