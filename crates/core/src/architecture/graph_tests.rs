//! Tests for [`super::graph`].
//!
//! Every test builds a real temporary workspace and runs the real
//! [`crate::workspace::scan`] over it, rather than hand-constructing
//! [`Project`](crate::model::Project) values. The graph's whole job is to line
//! reference strings up with what the scan produced, so a test that fabricates
//! the scan's output would be testing the fabrication.

use std::path::Path;

use super::graph::*;
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

const LIB_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup>
</Project>"#;

/// An executable project referencing whatever `Include` is substituted in.
fn app_csproj(includes: &[&str]) -> String {
    let items: String = includes
        .iter()
        .map(|i| format!("    <ProjectReference Include=\"{i}\" />\n"))
        .collect();
    format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  \
         <PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net8.0</TargetFramework></PropertyGroup>\n  \
         <ItemGroup>\n{items}  </ItemGroup>\n</Project>"
    )
}

fn id_of<'a>(ws: &'a Workspace, name: &str) -> &'a str {
    &ws.projects
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("no scanned project named {name}"))
        .id
}

fn edges_of(graph: &ArchGraph, kind: EdgeKind) -> Vec<(&str, &str)> {
    graph
        .edges
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect()
}

// ---------------------------------------------------------------------------
// The IPC contract
// ---------------------------------------------------------------------------

#[test]
fn an_arch_graph_serialises_with_the_keys_the_ui_reads() {
    let graph = ArchGraph {
        nodes: vec![ArchNode {
            id: "a".into(),
            label: "A".into(),
            kind: ArchKind::Project,
            project_id: Some("a".into()),
            path: Some("src/A/A.csproj".into()),
            ecosystem: Some("dotnet".into()),
        }],
        edges: vec![ArchEdge {
            from: "a".into(),
            to: "b".into(),
            kind: EdgeKind::ProjectReference,
            label: None,
        }],
        warnings: vec!["something".into()],
        derivation: Derivation::Derived { scanner: 1 },
    };

    let json = serde_json::to_value(&graph).unwrap();

    let mut keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["derivation", "edges", "nodes", "warnings"],
        "src/ipc/types.ts mirrors ArchGraph by hand — update it with this test"
    );

    let mut node_keys: Vec<String> = json["nodes"][0]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    node_keys.sort();
    assert_eq!(
        node_keys,
        ["ecosystem", "id", "kind", "label", "path", "projectId"],
        "src/ipc/types.ts mirrors ArchNode by hand — update it with this test"
    );

    let mut edge_keys: Vec<String> = json["edges"][0]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    edge_keys.sort();
    assert_eq!(
        edge_keys,
        ["from", "kind", "label", "to"],
        "src/ipc/types.ts mirrors ArchEdge by hand — update it with this test"
    );

    assert_eq!(
        json["nodes"][0]["kind"], "project",
        "src/ipc/types.ts spells ArchKind in camelCase — update it with this test"
    );
    assert_eq!(
        json["edges"][0]["kind"], "projectReference",
        "src/ipc/types.ts spells EdgeKind in camelCase — update it with this test"
    );

    // Every variant, not just the two above. `types.ts` is a hand-written
    // mirror with no codegen behind it, so an added variant that nothing
    // asserts on is exactly the change that ships a union the UI cannot
    // narrow — and `serde(rename_all = "camelCase")` on a one-word variant
    // (`project`, `external`, `contains`) is a no-op, which makes it easy to
    // believe the rename is doing something it is not.
    let kinds: Vec<serde_json::Value> = [
        ArchKind::Project,
        ArchKind::Solution,
        ArchKind::SolutionFolder,
        ArchKind::External,
        ArchKind::Service,
        ArchKind::DataStore,
    ]
    .iter()
    .map(|kind| serde_json::to_value(kind).unwrap())
    .collect();
    assert_eq!(
        kinds,
        [
            "project",
            "solution",
            "solutionFolder",
            "external",
            "service",
            "dataStore"
        ],
        "src/ipc/types.ts spells the ArchKind union by hand — update it with this test"
    );

    let edge_kinds: Vec<serde_json::Value> = [
        EdgeKind::ProjectReference,
        EdgeKind::PackageDependency,
        EdgeKind::Contains,
        EdgeKind::DataAccess,
        EdgeKind::ServiceCall,
    ]
    .iter()
    .map(|kind| serde_json::to_value(kind).unwrap())
    .collect();
    assert_eq!(
        edge_kinds,
        [
            "projectReference",
            "packageDependency",
            "contains",
            "dataAccess",
            "serviceCall"
        ],
        "src/ipc/types.ts spells the EdgeKind union by hand — update it with this test"
    );
    assert_eq!(
        json["derivation"],
        serde_json::json!({ "derived": { "scanner": 1 } }),
        "src/ipc/types.ts mirrors Derivation by hand — update it with this test"
    );

    let inferred = serde_json::to_value(Derivation::Inferred {
        agent: "claude".into(),
    })
    .unwrap();
    assert_eq!(
        inferred,
        serde_json::json!({ "inferred": { "agent": "claude" } }),
        "src/ipc/types.ts mirrors Derivation by hand — update it with this test"
    );
    assert_eq!(
        serde_json::to_value(Derivation::User).unwrap(),
        serde_json::json!("user"),
        "src/ipc/types.ts mirrors Derivation by hand — update it with this test"
    );
}

// ---------------------------------------------------------------------------
// .NET project references
// ---------------------------------------------------------------------------

#[test]
fn dotnet_project_references_become_edges() {
    let (_dir, ws) = scanned(&[
        ("src/App/App.csproj", &app_csproj(&[r"..\Lib\Lib.csproj"])),
        ("src/Lib/Lib.csproj", LIB_CSPROJ),
    ]);

    let graph = project_graph(&ws);

    assert_eq!(
        edges_of(&graph, EdgeKind::ProjectReference),
        vec![(id_of(&ws, "App"), id_of(&ws, "Lib"))]
    );
    assert!(
        graph.warnings.is_empty(),
        "a reference that resolved cleanly warns about nothing: {:?}",
        graph.warnings
    );
}

#[test]
fn a_project_reference_pointing_outside_the_workspace_is_reported_not_dropped() {
    // A silently lost arrow is the failure mode to avoid: the diagram would
    // look complete and be wrong.
    let (_dir, ws) = scanned(&[(
        "src/App/App.csproj",
        &app_csproj(&[r"..\..\..\Shared\Shared.csproj"]),
    )]);

    let graph = project_graph(&ws);

    let external: Vec<&ArchNode> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == ArchKind::External)
        .collect();
    assert_eq!(external.len(), 1, "nodes: {:?}", graph.nodes);
    assert_eq!(external[0].label, "Shared");
    assert!(external[0].project_id.is_none());

    let app = id_of(&ws, "App");
    assert_eq!(
        edges_of(&graph, EdgeKind::ProjectReference),
        vec![(app, external[0].id.as_str())]
    );

    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains(r"..\..\..\Shared\Shared.csproj"),
        "the warning must quote the path as written: {}",
        graph.warnings[0]
    );
}

#[test]
fn a_project_reference_that_resolves_to_no_scanned_project_is_reported() {
    let (_dir, ws) = scanned(&[
        // `Lbi` is a typo for `Lib`, inside the workspace but pointing nowhere.
        ("src/App/App.csproj", &app_csproj(&[r"..\Lbi\Lib.csproj"])),
        ("src/Lib/Lib.csproj", LIB_CSPROJ),
    ]);

    let graph = project_graph(&ws);

    assert!(
        edges_of(&graph, EdgeKind::ProjectReference).is_empty(),
        "an unresolved reference must not invent an edge"
    );
    assert!(
        !graph.nodes.iter().any(|n| n.kind == ArchKind::External),
        "a dangling path inside the scanned area is a broken manifest, not an \
         external component: {:?}",
        graph.nodes
    );
    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains(r"..\Lbi\Lib.csproj"),
        "the warning must quote the path as written: {}",
        graph.warnings[0]
    );
}

#[test]
fn a_project_reference_written_with_either_separator_resolves_the_same_way() {
    for include in [r"..\Lib\Lib.csproj", "../Lib/Lib.csproj"] {
        let (_dir, ws) = scanned(&[
            ("src/App/App.csproj", &app_csproj(&[include])),
            ("src/Lib/Lib.csproj", LIB_CSPROJ),
        ]);

        let graph = project_graph(&ws);
        assert_eq!(
            edges_of(&graph, EdgeKind::ProjectReference),
            vec![(id_of(&ws, "App"), id_of(&ws, "Lib"))],
            "{include} should resolve"
        );
    }
}

#[test]
fn dot_segments_in_a_project_reference_are_resolved_lexically() {
    let (_dir, ws) = scanned(&[
        (
            "src/App/App.csproj",
            &app_csproj(&[r".\..\.\Lib\..\Lib\Lib.csproj"]),
        ),
        ("src/Lib/Lib.csproj", LIB_CSPROJ),
    ]);

    let graph = project_graph(&ws);
    assert_eq!(
        edges_of(&graph, EdgeKind::ProjectReference),
        vec![(id_of(&ws, "App"), id_of(&ws, "Lib"))]
    );
}

#[test]
fn a_project_reference_is_resolved_without_the_target_needing_to_exist() {
    // `fs::canonicalize` would fail here. That case — a reference to a file
    // that is not on disk — is exactly the one worth reporting, so resolution
    // must never depend on the target existing.
    let (_dir, ws) = scanned(&[(
        "src/App/App.csproj",
        &app_csproj(&[r"..\Missing\Missing.csproj"]),
    )]);

    let graph = project_graph(&ws);
    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
}

#[test]
fn a_project_reference_differing_only_in_casing_is_unresolved_and_says_so() {
    // Matching case-insensitively would be a guess: it is right on NTFS and
    // wrong on a case-sensitive filesystem, and the graph cannot tell which it
    // is looking at. Naming the near miss in the warning is strictly better
    // than either guess.
    let (_dir, ws) = scanned(&[
        ("src/App/App.csproj", &app_csproj(&[r"..\lib\lib.csproj"])),
        ("src/Lib/Lib.csproj", LIB_CSPROJ),
    ]);

    let graph = project_graph(&ws);

    assert!(edges_of(&graph, EdgeKind::ProjectReference).is_empty());
    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains("casing"),
        "the near miss must be named: {}",
        graph.warnings[0]
    );
}

#[test]
fn a_drive_rooted_project_reference_is_not_reinterpreted_as_project_relative() {
    // `\Shared\Shared.csproj` is legal MSBuild meaning "the root of the current
    // drive". Dropping the leading separator and joining it onto the referring
    // project's directory silently draws an arrow at a *different* project that
    // happens to sit at that relative position — the worst outcome this module
    // has, because the forged path really is inside the root and every guard
    // downstream believes it.
    let (_dir, ws) = scanned(&[
        (
            "src/App/App.csproj",
            &app_csproj(&[r"\Shared\Shared.csproj"]),
        ),
        ("src/App/Shared/Shared.csproj", LIB_CSPROJ),
        ("Shared/Shared.csproj", LIB_CSPROJ),
    ]);

    let graph = project_graph(&ws);

    let app = id_of(&ws, "App");
    let decoy = ws
        .projects
        .iter()
        .find(|p| p.id.contains("src-App-Shared"))
        .unwrap_or_else(|| {
            panic!(
                "the decoy project must have been scanned: {:?}",
                ws.projects
            )
        });
    assert!(
        !edges_of(&graph, EdgeKind::ProjectReference).contains(&(app, decoy.id.as_str())),
        "a rooted reference must not resolve against the referring project: {:?}",
        graph.edges
    );

    let external: Vec<&ArchNode> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == ArchKind::External)
        .collect();
    assert_eq!(
        external.len(),
        1,
        "a rooted reference names something the workspace cannot locate, so it \
         is drawn as external: {:?}",
        graph.nodes
    );
    assert_eq!(external[0].label, "Shared");
    assert_eq!(
        edges_of(&graph, EdgeKind::ProjectReference),
        vec![(app, external[0].id.as_str())]
    );

    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains(r"\Shared\Shared.csproj"),
        "the warning must quote the path as written: {}",
        graph.warnings[0]
    );
}

#[test]
fn a_posix_absolute_project_reference_is_not_reinterpreted_as_project_relative() {
    let (_dir, ws) = scanned(&[
        ("src/App/App.csproj", &app_csproj(&["/Lib/Lib.csproj"])),
        ("src/App/Lib/Lib.csproj", LIB_CSPROJ),
    ]);

    let graph = project_graph(&ws);

    let decoy = ws
        .projects
        .iter()
        .find(|p| p.id.contains("src-App-Lib"))
        .unwrap()
        .id
        .as_str();
    assert!(
        !edges_of(&graph, EdgeKind::ProjectReference).contains(&(id_of(&ws, "App"), decoy)),
        "edges: {:?}",
        graph.edges
    );
    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
}

#[test]
fn a_rooted_project_reference_carries_no_fabricated_path_on_its_external_node() {
    // `PathBuf::push("C:")` throws away everything accumulated so far, so the
    // old code emitted a `../../../..`-prefixed path whose depth was a function
    // of where the temporary directory happened to sit on disk. `ArchNode.path`
    // promises a location that survives being opened on another machine; there
    // is no such location for a drive-absolute reference, so there must be no
    // path at all.
    let (_dir, ws) = scanned(&[(
        "src/App/App.csproj",
        &app_csproj(&[r"C:\Shared\Shared.csproj"]),
    )]);

    let graph = project_graph(&ws);

    let external = graph
        .nodes
        .iter()
        .find(|n| n.kind == ArchKind::External)
        .unwrap_or_else(|| panic!("nodes: {:?}", graph.nodes));
    assert_eq!(
        external.path, None,
        "a rooted reference has no workspace-relative location to report"
    );
    assert!(
        !external.id.contains(".."),
        "the id must not encode a depth that depends on where the workspace \
         sits on disk: {}",
        external.id
    );
    assert_eq!(external.label, "Shared");
}

// ---------------------------------------------------------------------------
// Node dependencies
// ---------------------------------------------------------------------------

const ROOT_PKG: &str = r#"{ "name": "root", "private": true, "workspaces": ["packages/*"] }"#;

#[test]
fn a_dependency_matching_a_sibling_package_name_becomes_an_edge() {
    let (_dir, ws) = scanned(&[
        ("package.json", ROOT_PKG),
        (
            "packages/web/package.json",
            r#"{ "name": "web", "scripts": { "dev": "vite" },
                 "dependencies": { "@acme/ui": "workspace:*" } }"#,
        ),
        (
            "packages/ui/package.json",
            r#"{ "name": "@acme/ui", "scripts": { "build": "tsc" } }"#,
        ),
    ]);

    let graph = project_graph(&ws);

    assert_eq!(
        edges_of(&graph, EdgeKind::PackageDependency),
        vec![(id_of(&ws, "web"), id_of(&ws, "@acme/ui"))]
    );
}

#[test]
fn an_external_dependency_never_becomes_a_node() {
    // Third-party packages are not architecture. `react` is a fact about the
    // lockfile, and drawing it would bury the handful of edges that matter
    // under hundreds that do not.
    let (_dir, ws) = scanned(&[(
        "packages/web/package.json",
        r#"{ "name": "web", "scripts": { "dev": "vite" },
             "dependencies": { "react": "^19.0.0" } }"#,
    )]);

    let graph = project_graph(&ws);

    assert!(edges_of(&graph, EdgeKind::PackageDependency).is_empty());
    assert!(
        graph.nodes.iter().all(|n| n.label != "react"),
        "nodes: {:?}",
        graph.nodes
    );
    assert!(
        graph.warnings.is_empty(),
        "an ordinary third-party dependency is not a problem to report: {:?}",
        graph.warnings
    );
}

#[test]
fn a_dependency_matching_a_dotnet_project_name_is_not_an_edge() {
    // Cross-ecosystem name collision is exactly the kind of coincidence that
    // produces a confidently wrong arrow.
    let (_dir, ws) = scanned(&[
        (
            "packages/web/package.json",
            r#"{ "name": "web", "scripts": { "dev": "vite" },
                 "dependencies": { "Lib": "^1.0.0" } }"#,
        ),
        ("src/Lib/Lib.csproj", LIB_CSPROJ),
    ]);

    let graph = project_graph(&ws);

    assert!(
        edges_of(&graph, EdgeKind::PackageDependency).is_empty(),
        "a npm dependency named `Lib` says nothing about a .NET project \
         named `Lib`"
    );
}

#[test]
fn a_dependency_matching_two_projects_with_the_same_package_name_is_reported() {
    // Two packages declaring one `name` is a real defect, and there is no
    // basis for picking either, so the edge is abandoned rather than guessed.
    let (_dir, ws) = scanned(&[
        (
            "packages/web/package.json",
            r#"{ "name": "web", "scripts": { "dev": "vite" },
                 "dependencies": { "@acme/ui": "workspace:*" } }"#,
        ),
        (
            "packages/ui/package.json",
            r#"{ "name": "@acme/ui", "scripts": { "build": "tsc" } }"#,
        ),
        (
            "vendor/ui/package.json",
            r#"{ "name": "@acme/ui", "scripts": { "build": "tsc" } }"#,
        ),
    ]);

    let graph = project_graph(&ws);

    assert!(edges_of(&graph, EdgeKind::PackageDependency).is_empty());
    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains("@acme/ui"),
        "the warning must name the dependency: {}",
        graph.warnings[0]
    );
}

#[test]
fn a_package_that_declares_no_name_is_never_matched_by_its_directory_name() {
    // The scan falls a nameless `package.json` back to its directory name so
    // the project has something to show in the sidebar. That fallback is not a
    // package name: a package with no `name` cannot be depended on by name at
    // all, so every match against it is false by construction — and `config`
    // here is a real, widely used npm package.
    let (_dir, ws) = scanned(&[
        (
            "apps/api/package.json",
            r#"{ "name": "api", "scripts": { "dev": "node ." },
                 "dependencies": { "config": "^3.3.9" } }"#,
        ),
        (
            "config/package.json",
            r#"{ "type": "module", "scripts": { "lint": "eslint ." } }"#,
        ),
    ]);

    let graph = project_graph(&ws);

    assert!(
        edges_of(&graph, EdgeKind::PackageDependency).is_empty(),
        "edges: {:?}",
        graph.edges
    );
    assert!(
        graph.warnings.is_empty(),
        "an ordinary third-party dependency is not a problem to report: {:?}",
        graph.warnings
    );
}

#[test]
fn an_unreadable_package_json_is_reported_rather_than_silently_losing_its_edges() {
    // The graph is derived on demand precisely because manifests are edited
    // while the workspace stays open, so a manifest that has just gone missing
    // is the designed-for case rather than a race. Dropping every edge of that
    // project without a word leaves a diagram that looks complete.
    let (dir, ws) = scanned(&[
        (
            "apps/web/package.json",
            r#"{ "name": "web", "scripts": { "dev": "vite" },
                 "dependencies": { "lib": "workspace:*" } }"#,
        ),
        (
            "libs/lib/package.json",
            r#"{ "name": "lib", "scripts": { "build": "tsc" } }"#,
        ),
    ]);
    std::fs::remove_file(dir.path().join("apps/web/package.json")).unwrap();

    let graph = project_graph(&ws);

    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains("read") && graph.warnings[0].contains("apps/web/package.json"),
        "the warning must say it could not be read and name the file: {}",
        graph.warnings[0]
    );
}

#[test]
fn an_unparseable_package_json_is_reported_as_unparseable_not_as_unreadable() {
    // The two need different fixes from the user, so the warning has to tell
    // them apart.
    let (_dir, ws) = scanned(&[
        (
            "apps/web/package.json",
            r#"{ "name": "web", "scripts": { "dev": "vite" },
                 "dependencies": { "lib": "workspace:*" } }"#,
        ),
        (
            "libs/lib/package.json",
            r#"{ "name": "lib", "scripts": { "build": "tsc" } }"#,
        ),
    ]);
    // A stray trailing comma — the commonest way a hand-edited manifest breaks.
    std::fs::write(
        ws.root.join("apps/web/package.json"),
        r#"{ "name": "web", "scripts": { "dev": "vite" },
             "dependencies": { "lib": "workspace:*" }, }"#,
    )
    .unwrap();

    let graph = project_graph(&ws);

    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains("parse") && graph.warnings[0].contains("apps/web/package.json"),
        "the warning must say it could not be parsed and name the file: {}",
        graph.warnings[0]
    );
}

#[test]
fn a_workspace_protocol_dependency_matching_no_project_is_reported() {
    // Skipping a third-party dependency is deliberate. `workspace:*` is an
    // unambiguous declaration that the dependency is meant to be local, so a
    // `workspace:` specifier matching nothing is a real miss — and the
    // specifier saying so is already in hand.
    let (_dir, ws) = scanned(&[(
        "packages/web/package.json",
        r#"{ "name": "web", "scripts": { "dev": "vite" },
             "dependencies": { "@acme/missing": "workspace:*", "react": "^19.0.0" } }"#,
    )]);

    let graph = project_graph(&ws);

    assert!(edges_of(&graph, EdgeKind::PackageDependency).is_empty());
    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains("@acme/missing") && graph.warnings[0].contains("workspace:*"),
        "the warning must name the dependency and its specifier: {}",
        graph.warnings[0]
    );
}

#[test]
fn a_pnpm_workspace_draws_containment_from_its_packages_globs() {
    // The commonest monorepo layout in the JS ecosystem keeps its member globs
    // in `pnpm-workspace.yaml`, not in `package.json`. Reading that file is the
    // honest fix: a pnpm workspace yields a container holding the members its
    // own `packages:` list expands to.
    let (_dir, ws) = scanned(&[
        ("pnpm-workspace.yaml", "packages:\n  - 'packages/*'\n"),
        (
            "packages/web/package.json",
            r#"{ "name": "web", "scripts": { "dev": "vite" } }"#,
        ),
    ]);

    let graph = project_graph(&ws);

    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.id == "workspace:pnpm-workspace.yaml" && n.kind == ArchKind::Solution),
        "a pnpm workspace container node must exist: {:?}",
        graph.nodes
    );
    assert!(
        edges_of(&graph, EdgeKind::Contains)
            .contains(&("workspace:pnpm-workspace.yaml", id_of(&ws, "web"))),
        "the container must hold the member its packages glob expands to: {:?}",
        graph.edges
    );
}

#[test]
fn a_workspaces_key_pnpm_ignores_is_never_drawn_as_containment() {
    // Verified against pnpm 10.14.0 on this machine: with a `pnpm-workspace.yaml`
    // present, `pnpm list -r` returned only the members that file lists, and the
    // `packages/*` glob in `package.json` contributed nothing — pnpm prints
    // "The \"workspaces\" field in package.json is not supported by pnpm".
    //
    // So membership is drawn from `pnpm-workspace.yaml`: the container is
    // `workspace:pnpm-workspace.yaml`, never `workspace:package.json`, and it
    // holds exactly the members that file's globs expand to. The ignored
    // `workspaces` key is reported so a reader knows it went unused.
    let (_dir, ws) = scanned(&[
        (
            "package.json",
            r#"{ "name": "acme-monorepo", "private": true, "workspaces": ["packages/*"] }"#,
        ),
        (
            "pnpm-workspace.yaml",
            "packages:\n  - 'packages/*'\n  - 'tools/*'\n",
        ),
        (
            "packages/web/package.json",
            r#"{ "name": "web", "scripts": { "dev": "vite" } }"#,
        ),
        (
            "tools/cli/package.json",
            r#"{ "name": "@acme/cli", "scripts": { "build": "tsc" } }"#,
        ),
    ]);

    let graph = project_graph(&ws);

    assert!(
        !graph.nodes.iter().any(|n| n.id == "workspace:package.json"),
        "the container's own label must not come from the ignored file: {:?}",
        graph.nodes
    );

    let contains = edges_of(&graph, EdgeKind::Contains);
    assert!(
        contains.contains(&("workspace:pnpm-workspace.yaml", id_of(&ws, "web")))
            && contains.contains(&("workspace:pnpm-workspace.yaml", id_of(&ws, "@acme/cli"))),
        "the pnpm container must hold the members its own globs expand to: {:?}",
        graph.edges
    );

    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    let warning = &graph.warnings[0];
    assert!(
        warning.contains("pnpm-workspace.yaml") && warning.contains("package.json"),
        "the warning must name both files, so a reader knows which list was \
         ignored and which supplied the members: {warning}"
    );
    assert!(
        warning.contains("packages/*"),
        "the warning must quote the ignored patterns: {warning}"
    );
}

#[test]
fn a_pnpm_workspace_expands_multiple_globs_and_excludes_non_node_dirs() {
    // Multiple `packages:` entries all contribute, and a directory that is not
    // a node project is never swept in even when a glob would match its path.
    let (_dir, ws) = scanned(&[
        (
            "pnpm-workspace.yaml",
            "packages:\n  - 'packages/*'\n  - 'apps/*'\n",
        ),
        (
            "packages/web/package.json",
            r#"{ "name": "web", "scripts": { "dev": "vite" } }"#,
        ),
        (
            "apps/api/package.json",
            r#"{ "name": "@acme/api", "scripts": { "start": "node ." } }"#,
        ),
        // A .NET project under a matched glob path: matched by the pattern but
        // not a node project, so it is not a member.
        (
            "packages/svc/svc.csproj",
            "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>",
        ),
    ]);

    let graph = project_graph(&ws);

    let contains = edges_of(&graph, EdgeKind::Contains);
    assert!(
        contains.contains(&("workspace:pnpm-workspace.yaml", id_of(&ws, "web")))
            && contains.contains(&("workspace:pnpm-workspace.yaml", id_of(&ws, "@acme/api"))),
        "both globs must contribute their node members: {:?}",
        graph.edges
    );
    assert!(
        !contains.contains(&("workspace:pnpm-workspace.yaml", id_of(&ws, "svc"))),
        "a non-node directory must never be swept into a node workspace container: {:?}",
        graph.edges
    );
}

#[test]
fn a_pnpm_glob_that_matches_nothing_draws_no_container() {
    // Mirrors npm's silence: a `packages:` glob that expands to no discovered
    // project invents neither a container node nor a warning.
    let (_dir, ws) = scanned(&[
        ("pnpm-workspace.yaml", "packages:\n  - 'services/*'\n"),
        (
            "packages/web/package.json",
            r#"{ "name": "web", "scripts": { "dev": "vite" } }"#,
        ),
    ]);

    let graph = project_graph(&ws);

    assert!(
        !graph
            .nodes
            .iter()
            .any(|n| n.id == "workspace:pnpm-workspace.yaml"),
        "a glob matching no project must draw no container: {:?}",
        graph.nodes
    );
    assert!(
        edges_of(&graph, EdgeKind::Contains).is_empty(),
        "a glob matching no project must draw no containment: {:?}",
        graph.edges
    );
    assert!(
        graph.warnings.is_empty(),
        "a glob matching nothing is silence, not a warning: {:?}",
        graph.warnings
    );
}

#[test]
fn an_unparseable_pnpm_workspace_yaml_abstains_with_a_named_warning() {
    // Not parsing YAML is defensible; silently pretending a broken file names no
    // members is not. A file that cannot be read for its `packages:` globs
    // abstains — no container invented — and says so, naming the file.
    let (_dir, ws) = scanned(&[
        // A tab where YAML forbids one: unparseable as a mapping.
        ("pnpm-workspace.yaml", "packages:\n\t- oops\n"),
        (
            "packages/web/package.json",
            r#"{ "name": "web", "scripts": { "dev": "vite" } }"#,
        ),
    ]);

    let graph = project_graph(&ws);

    assert!(
        !graph
            .nodes
            .iter()
            .any(|n| n.id == "workspace:pnpm-workspace.yaml"),
        "no container may be invented from a file that could not be parsed: {:?}",
        graph.nodes
    );
    assert!(
        edges_of(&graph, EdgeKind::Contains).is_empty(),
        "no containment may be drawn from a file that could not be parsed: {:?}",
        graph.edges
    );
    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains("pnpm-workspace.yaml"),
        "the warning must name the file that could not be read: {}",
        graph.warnings[0]
    );
}

// ---------------------------------------------------------------------------
// Manifests the scan could not read
// ---------------------------------------------------------------------------

#[test]
fn a_package_json_that_was_already_broken_before_the_scan_is_reported_not_invisible() {
    // The existing unparseable/unreadable tests scan a *valid* tree and break
    // the file afterwards, so the project exists and its warning comes from
    // `node_dependencies`. The case a user actually hits is a manifest that was
    // broken before the workspace was ever opened. The scan now keeps such a
    // project and records why it could not be read, so the component has a box
    // — but it has no edges in either direction, and a diagram that looks
    // complete while quietly missing arrows is the failure this module exists
    // to prevent. The reason the scan recorded is what makes the warning
    // actionable, so it must be quoted rather than paraphrased.
    let (_dir, ws) = scanned(&[
        (
            "packages/web/package.json",
            r#"{ "name": "web", "scripts": { "dev": "vite" } }"#,
        ),
        // A stray trailing comma, present at scan time.
        (
            "packages/broken/package.json",
            r#"{ "name": "@acme/broken", "dependencies": { "@acme/ui": "workspace:*" }, }"#,
        ),
    ]);

    let recorded = ws
        .projects
        .iter()
        .find(|p| p.dir.ends_with("broken"))
        .unwrap_or_else(|| panic!("the scan must keep the project: {:?}", ws.projects))
        .unreadable
        .clone()
        .expect("the scan must record why it could not be read");

    let graph = project_graph(&ws);

    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.path.as_deref() == Some(Path::new("packages/broken/package.json"))),
        "the component is no longer invisible — it has a box: {:?}",
        graph.nodes
    );
    assert!(
        graph.edges.is_empty(),
        "nothing may be derived from a manifest that would not parse: {:?}",
        graph.edges
    );

    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    let warning = &graph.warnings[0];
    assert!(
        warning.contains("packages/broken/package.json"),
        "the warning must name the file: {warning}"
    );
    assert!(
        warning.contains(&recorded),
        "the warning must quote the reason the scan recorded, verbatim — that \
         is the only part a user can act on: {warning}"
    );
    assert!(
        recorded.contains("line"),
        "and that reason is only actionable because it names a location: {recorded}"
    );
}

#[test]
fn a_broken_root_package_json_is_reported_rather_than_leaving_a_silent_workspace() {
    // Same root cause, worse consequence: the root manifest is also where the
    // workspace globs live, so a broken one loses every containment edge as
    // well as all of its own.
    let (_dir, ws) = scanned(&[
        (
            "package.json",
            r#"{ "name": "root", "workspaces": ["packages/*"], }"#,
        ),
        (
            "packages/web/package.json",
            r#"{ "name": "web", "scripts": { "dev": "vite" } }"#,
        ),
    ]);

    let graph = project_graph(&ws);

    assert!(
        edges_of(&graph, EdgeKind::Contains).is_empty(),
        "the membership list was in the file that would not parse: {:?}",
        graph.edges
    );
    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains("package.json") && graph.warnings[0].contains("line 1"),
        "a broken root manifest must not be silent, and the warning must carry \
         the parse error that locates the mistake: {}",
        graph.warnings[0]
    );
}

#[test]
fn a_crate_whose_manifest_the_scan_could_not_read_is_reported_with_the_reason_it_recorded() {
    // One rule, not one per ecosystem: the scan records why on the project, and
    // this module reads that field rather than re-deciding the question per
    // manifest format.
    let (_dir, ws) = scanned(&[
        ("crates/ok/Cargo.toml", "[package]\nname = \"ok\"\n"),
        ("crates/broken/Cargo.toml", "[package\nname = \"broken\"\n"),
    ]);

    let recorded = ws
        .projects
        .iter()
        .find(|p| p.dir.ends_with("broken"))
        .unwrap_or_else(|| panic!("the scan must keep the crate: {:?}", ws.projects))
        .unreadable
        .clone()
        .expect("the scan must record why it could not be read");

    let graph = project_graph(&ws);

    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains("crates/broken/Cargo.toml")
            && graph.warnings[0].contains(&recorded),
        "the warning must name the file and quote the recorded reason: {}",
        graph.warnings[0]
    );
}

#[test]
fn a_workspace_root_that_declares_no_scripts_is_not_reported_as_broken() {
    // `scan_node_project` also returns nothing for a perfectly valid monorepo
    // root with no scripts of its own. That is a deliberate omission, not a
    // defect, so it must not produce a warning — otherwise every monorepo opens
    // with a complaint about its own root.
    let (_dir, ws) = scanned(&[
        ("package.json", ROOT_PKG),
        (
            "packages/web/package.json",
            r#"{ "name": "web", "scripts": { "dev": "vite" } }"#,
        ),
    ]);

    let graph = project_graph(&ws);

    assert!(
        graph.warnings.is_empty(),
        "a valid, script-less workspace root is not a problem to report: {:?}",
        graph.warnings
    );
}

#[test]
fn a_project_reference_listed_twice_produces_one_edge() {
    let (_dir, ws) = scanned(&[
        (
            "src/App/App.csproj",
            &app_csproj(&[r"..\Lib\Lib.csproj", r"..\Lib\Lib.csproj"]),
        ),
        ("src/Lib/Lib.csproj", LIB_CSPROJ),
    ]);

    let graph = project_graph(&ws);

    assert_eq!(
        edges_of(&graph, EdgeKind::ProjectReference),
        vec![(id_of(&ws, "App"), id_of(&ws, "Lib"))]
    );
}

#[test]
fn workspace_globs_are_expanded_to_member_projects() {
    let (_dir, ws) = scanned(&[
        ("package.json", ROOT_PKG),
        (
            "packages/web/package.json",
            r#"{ "name": "web", "scripts": { "dev": "vite" } }"#,
        ),
        (
            "tools/gen/package.json",
            r#"{ "name": "gen", "scripts": { "gen": "node ." } }"#,
        ),
    ]);

    let graph = project_graph(&ws);

    let root = graph
        .nodes
        .iter()
        .find(|n| n.kind == ArchKind::Solution)
        .unwrap_or_else(|| panic!("a workspace root is a grouping node: {:?}", graph.nodes));

    assert_eq!(
        edges_of(&graph, EdgeKind::Contains),
        vec![(root.id.as_str(), id_of(&ws, "web"))],
        "`tools/gen` does not match `packages/*` and is not a member"
    );
}

// ---------------------------------------------------------------------------
// Cargo
// ---------------------------------------------------------------------------

/// A `[workspace]` with no `[package]` — the shape this repository's own root
/// manifest has, and the reason the containment rule cannot work from scanned
/// projects alone: the scan deliberately produces no project for one.
const VIRTUAL_ROOT: &str = "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n";

/// The declarative adapter shipped in `examples/adapters/cargo-nextest.toml`,
/// which detects `Cargo.toml` and is therefore the one way a project whose
/// manifest path ends in `Cargo.toml` can have an ecosystem that is not
/// `cargo`.
const CARGO_NEXTEST_ADAPTER: &str = r#"
id = "cargo-nextest"
name = "cargo nextest"
detect = ["Cargo.toml"]

[test]
program = "cargo"
args = ["nextest", "run"]
report_format = "junitXml"
"#;

#[test]
fn a_cargo_path_dependency_resolving_to_a_scanned_crate_becomes_a_reference_edge() {
    // The arrow this whole round exists for: `src-tauri` -> `crates/core` in
    // this repository is exactly this shape, a `path` dependency climbing out
    // of one member directory and back down into another.
    let (_dir, ws) = scanned(&[
        (
            "src-tauri/Cargo.toml",
            "[package]\nname = \"cb-app\"\n\n[dependencies]\ncb-core = { path = \"../crates/core\" }\n",
        ),
        ("crates/core/Cargo.toml", "[package]\nname = \"cb-core\"\n"),
    ]);

    let graph = project_graph(&ws);

    assert_eq!(
        edges_of(&graph, EdgeKind::ProjectReference),
        vec![(id_of(&ws, "cb-app"), id_of(&ws, "cb-core"))],
        "edges: {:?}",
        graph.edges
    );
    assert!(
        graph.warnings.is_empty(),
        "a dependency that resolved is not a problem to report: {:?}",
        graph.warnings
    );
}

#[test]
fn a_cargo_path_dependency_is_never_matched_against_a_project_from_another_ecosystem() {
    // A declarative adapter detecting `Cargo.toml` is the one way a project can
    // carry that manifest path without being a crate: the scan skips a virtual
    // manifest, so nothing built-in claims the root directory and the adapter
    // does. Resolving a path dependency onto it would draw an arrow at a
    // *configuration source* and call it a crate.
    let (_dir, ws) = scanned(&[
        (
            ".code-basics/adapters/cargo-nextest.toml",
            CARGO_NEXTEST_ADAPTER,
        ),
        ("Cargo.toml", VIRTUAL_ROOT),
        (
            "crates/app/Cargo.toml",
            "[package]\nname = \"app\"\n\n[dependencies]\nroot = { path = \"../..\" }\n",
        ),
    ]);
    assert!(
        ws.projects
            .iter()
            .any(|p| p.dir == ws.root && p.ecosystem != "cargo"),
        "the fixture only means anything while another ecosystem really does \
         claim the root Cargo.toml: {:?}",
        ws.projects
            .iter()
            .map(|p| (&p.name, &p.ecosystem))
            .collect::<Vec<_>>()
    );

    let graph = project_graph(&ws);

    assert!(
        edges_of(&graph, EdgeKind::ProjectReference).is_empty(),
        "a cargo lookup may not reach across ecosystems: {:?}",
        graph.edges
    );
    assert!(
        graph.warnings.iter().any(|w| w.contains("../..")),
        "the unresolved dependency must still be reported by the path as \
         written: {:?}",
        graph.warnings
    );
}

#[test]
fn a_cargo_path_dependency_pointing_outside_the_workspace_becomes_an_external_component() {
    let (_dir, ws) = scanned(&[(
        "crates/app/Cargo.toml",
        "[package]\nname = \"app\"\n\n[dependencies]\nshared = { path = \"../../../shared\" }\n",
    )]);

    let graph = project_graph(&ws);

    let external = graph
        .nodes
        .iter()
        .find(|n| n.kind == ArchKind::External)
        .unwrap_or_else(|| {
            panic!(
                "a crate outside the workspace is external: {:?}",
                graph.nodes
            )
        });
    assert_eq!(
        external.label, "shared",
        "the box is labelled with the crate's directory, not with `Cargo`"
    );
    assert_eq!(
        edges_of(&graph, EdgeKind::ProjectReference),
        vec![(id_of(&ws, "app"), external.id.as_str())]
    );

    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains("../../../shared"),
        "the warning must quote the path exactly as the manifest spells it, so \
         it can be grepped for: {}",
        graph.warnings[0]
    );
}

#[test]
fn a_cargo_path_dependency_matching_no_crate_is_reported_rather_than_dropped() {
    // Inside the scanned area and matching nothing is a broken manifest, not a
    // component the diagram is missing — so no external box is invented for it,
    // because that box would assert something exists where nothing does.
    let (_dir, ws) = scanned(&[(
        "crates/app/Cargo.toml",
        "[package]\nname = \"app\"\n\n[dependencies]\nghost = { path = \"../ghost\" }\n",
    )]);

    let graph = project_graph(&ws);

    assert!(graph.edges.is_empty(), "edges: {:?}", graph.edges);
    assert!(
        !graph.nodes.iter().any(|n| n.kind == ArchKind::External),
        "nothing exists at that path, so no box may claim it does: {:?}",
        graph.nodes
    );
    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains("../ghost") && graph.warnings[0].contains("ghost"),
        "the warning must name the dependency and the path as written: {}",
        graph.warnings[0]
    );
}

#[test]
fn a_rooted_cargo_path_dependency_is_never_reinterpreted_relative_to_the_workspace() {
    // `/shared/lib` means the root of the current drive. Joining it onto the
    // referring crate's directory forges a path that lands *inside* the
    // workspace and draws a confident arrow at whichever crate happens to sit
    // there — and the forged path passes the `starts_with(root)` guard
    // honestly, so everything downstream believes it.
    let (_dir, ws) = scanned(&[
        (
            "crates/app/Cargo.toml",
            "[package]\nname = \"app\"\n\n[dependencies]\nlib = { path = \"/shared/lib\" }\n",
        ),
        ("shared/lib/Cargo.toml", "[package]\nname = \"the-lib\"\n"),
    ]);

    let graph = project_graph(&ws);

    let external = graph
        .nodes
        .iter()
        .find(|n| n.kind == ArchKind::External)
        .unwrap_or_else(|| panic!("an absolute path is not locatable: {:?}", graph.nodes));
    assert_eq!(
        edges_of(&graph, EdgeKind::ProjectReference),
        vec![(id_of(&ws, "app"), external.id.as_str())],
        "the arrow must not land on the crate that happens to sit at that \
         relative path: {:?}",
        graph.edges
    );
    assert_eq!(
        external.path, None,
        "the only honest path here would be the machine-specific string the \
         manifest names, which is what `ArchNode::path` promises not to be"
    );
    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains("/shared/lib") && graph.warnings[0].contains("absolute"),
        "the warning must quote the path and say why it was not located: {}",
        graph.warnings[0]
    );
}

#[test]
fn a_cargo_path_dependency_differing_only_in_casing_is_reported_rather_than_matched() {
    // Matching case-insensitively would be right on NTFS and wrong on a
    // case-sensitive filesystem, and this code cannot tell which one it is
    // looking at. Naming the near miss is strictly more useful than either
    // guess.
    let (_dir, ws) = scanned(&[
        (
            "crates/app/Cargo.toml",
            "[package]\nname = \"app\"\n\n[dependencies]\nthe-lib = { path = \"../Lib\" }\n",
        ),
        ("crates/lib/Cargo.toml", "[package]\nname = \"the-lib\"\n"),
    ]);

    let graph = project_graph(&ws);

    assert!(graph.edges.is_empty(), "edges: {:?}", graph.edges);
    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains("../Lib") && graph.warnings[0].contains("casing"),
        "the warning must quote the path and name the near miss: {}",
        graph.warnings[0]
    );
    assert!(
        graph.warnings[0].contains("crates/lib"),
        "the near miss is only actionable if the candidate is named: {}",
        graph.warnings[0]
    );
}

#[test]
fn a_dev_dependency_and_a_build_dependency_are_drawn_like_any_other_path_dependency() {
    // A test-support crate half the workspace depends on is part of the
    // architecture, and dropping those edges would hide it. The three kinds
    // collapse onto one arrow because `EdgeKind` has no way to say "test only"
    // — see the note on `cargo_dependencies`.
    let (_dir, ws) = scanned(&[
        (
            "crates/app/Cargo.toml",
            "[package]\nname = \"app\"\n\n[dev-dependencies]\nharness = { path = \"../harness\" }\n\n\
             [build-dependencies]\ncodegen = { path = \"../codegen\" }\n",
        ),
        ("crates/harness/Cargo.toml", "[package]\nname = \"harness\"\n"),
        ("crates/codegen/Cargo.toml", "[package]\nname = \"codegen\"\n"),
    ]);

    let graph = project_graph(&ws);

    let mut refs = edges_of(&graph, EdgeKind::ProjectReference);
    refs.sort_unstable();
    let mut expected = vec![
        (id_of(&ws, "app"), id_of(&ws, "harness")),
        (id_of(&ws, "app"), id_of(&ws, "codegen")),
    ];
    expected.sort_unstable();
    assert_eq!(refs, expected);
}

#[test]
fn cargo_workspace_members_are_matched_against_discovered_crates_not_the_filesystem() {
    let (_dir, ws) = scanned(&[
        ("Cargo.toml", VIRTUAL_ROOT),
        ("crates/a/Cargo.toml", "[package]\nname = \"a\"\n"),
        ("crates/b/Cargo.toml", "[package]\nname = \"b\"\n"),
        // Two segments deep: `*` stops at a separator, the way cargo, npm and
        // pnpm all mean it, so this is not a member.
        ("crates/a/vendor/c/Cargo.toml", "[package]\nname = \"c\"\n"),
        // Outside the pattern entirely.
        ("tools/d/Cargo.toml", "[package]\nname = \"d\"\n"),
    ]);

    let graph = project_graph(&ws);

    let root = graph
        .nodes
        .iter()
        .find(|n| n.kind == ArchKind::Solution)
        .unwrap_or_else(|| {
            panic!(
                "a cargo workspace root is a grouping node: {:?}",
                graph.nodes
            )
        });
    assert_eq!(root.ecosystem.as_deref(), Some("cargo"));
    assert_eq!(root.path.as_deref(), Some(Path::new("Cargo.toml")));

    let mut contains = edges_of(&graph, EdgeKind::Contains);
    contains.sort_unstable();
    let mut expected = vec![
        (root.id.as_str(), id_of(&ws, "a")),
        (root.id.as_str(), id_of(&ws, "b")),
    ];
    expected.sort_unstable();
    assert_eq!(
        contains, expected,
        "`crates/a/vendor/c` and `tools/d` are not members of `crates/*`"
    );
}

#[test]
fn a_cargo_workspace_exclude_removes_a_directory_the_member_glob_matched() {
    // `members = ["crates/*"]` with `exclude = ["crates/legacy"]` matches a
    // directory that is explicitly *not* a member, and drawing it inside the
    // container would state the opposite of what the manifest says.
    let (_dir, ws) = scanned(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/legacy\"]\n",
        ),
        ("crates/a/Cargo.toml", "[package]\nname = \"a\"\n"),
        ("crates/legacy/Cargo.toml", "[package]\nname = \"legacy\"\n"),
    ]);

    let graph = project_graph(&ws);

    let root = graph
        .nodes
        .iter()
        .find(|n| n.kind == ArchKind::Solution)
        .unwrap();
    assert_eq!(
        edges_of(&graph, EdgeKind::Contains),
        vec![(root.id.as_str(), id_of(&ws, "a"))],
        "edges: {:?}",
        graph.edges
    );
    assert!(
        graph.nodes.iter().any(|n| n.label == "legacy"),
        "an excluded crate is still a crate and keeps its own box: {:?}",
        graph.nodes
    );
}

#[test]
fn a_cargo_member_pattern_that_matched_no_discovered_crate_is_reported() {
    // A membership list naming something the scan never found is the same
    // "looks complete, quietly missing" failure a dropped reference is.
    let (_dir, ws) = scanned(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\", \"ghost\"]\n",
        ),
        ("crates/a/Cargo.toml", "[package]\nname = \"a\"\n"),
    ]);

    let graph = project_graph(&ws);

    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains("ghost"),
        "the warning must quote the pattern that matched nothing: {}",
        graph.warnings[0]
    );
    assert!(
        !graph.warnings[0].contains("crates/*"),
        "the pattern that did match must not be reported: {}",
        graph.warnings[0]
    );
}

#[test]
fn a_workspace_root_that_is_itself_a_crate_is_a_member_of_its_own_workspace() {
    // Cargo makes the root package a member automatically, and it is named by
    // no `members` entry — so it can only come from the `[package]` table
    // sitting beside the `[workspace]` one.
    let (_dir, ws) = scanned(&[
        (
            "Cargo.toml",
            "[package]\nname = \"root\"\n\n[workspace]\nmembers = [\"crates/*\"]\n",
        ),
        ("crates/a/Cargo.toml", "[package]\nname = \"a\"\n"),
    ]);

    let graph = project_graph(&ws);

    let container = graph
        .nodes
        .iter()
        .find(|n| n.kind == ArchKind::Solution)
        .unwrap_or_else(|| panic!("nodes: {:?}", graph.nodes));
    let mut contains = edges_of(&graph, EdgeKind::Contains);
    contains.sort_unstable();
    let mut expected = vec![
        (container.id.as_str(), id_of(&ws, "root")),
        (container.id.as_str(), id_of(&ws, "a")),
    ];
    expected.sort_unstable();
    assert_eq!(contains, expected);
    assert_ne!(
        container.id,
        id_of(&ws, "root"),
        "the file plays two roles and needs two ids"
    );
}

#[test]
fn a_cargo_manifest_broken_after_the_scan_is_reported_rather_than_losing_its_workspace() {
    // A virtual manifest is not a project, so nothing else in this module will
    // ever complain about it. Its `members` list is the only source of the
    // containment edges, and a broken one costs all of them at once.
    let (_dir, ws) = scanned(&[
        ("Cargo.toml", VIRTUAL_ROOT),
        ("crates/a/Cargo.toml", "[package]\nname = \"a\"\n"),
    ]);
    std::fs::write(ws.root.join("Cargo.toml"), "[workspace\nmembers = [\n").unwrap();

    let graph = project_graph(&ws);

    assert!(
        edges_of(&graph, EdgeKind::Contains).is_empty(),
        "edges: {:?}",
        graph.edges
    );
    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].contains("Cargo.toml") && graph.warnings[0].contains("parse"),
        "the warning must name the file and say it could not be parsed: {}",
        graph.warnings[0]
    );
}

// ---------------------------------------------------------------------------
// Solutions
// ---------------------------------------------------------------------------

fn sln(entries: &str) -> String {
    format!("Microsoft Visual Studio Solution File, Format Version 12.00\n{entries}")
}

const APP_AND_LIB: &str = r#"Project("{2150E333-8FDC-42A3-9474-1AB1AEA671C7}") = "src", "src", "{11111111-1111-1111-1111-111111111111}"
EndProject
Project("{9A19103F-16F7-4668-BE54-9A1E7A4F7556}") = "App", "src\App\App.csproj", "{33333333-3333-3333-3333-333333333333}"
EndProject
Project("{9A19103F-16F7-4668-BE54-9A1E7A4F7556}") = "Lib", "src\Lib\Lib.csproj", "{44444444-4444-4444-4444-444444444444}"
EndProject
Global
	GlobalSection(NestedProjects) = preSolution
		{33333333-3333-3333-3333-333333333333} = {11111111-1111-1111-1111-111111111111}
	EndGlobalSection
EndGlobal
"#;

#[test]
fn solution_folders_become_containment_not_dependency() {
    // A solution says which projects ship together, never which depends on
    // which.
    let (_dir, ws) = scanned(&[
        ("src/App/App.csproj", &app_csproj(&[])),
        ("src/Lib/Lib.csproj", LIB_CSPROJ),
        ("Repo.sln", &sln(APP_AND_LIB)),
    ]);

    let graph = project_graph(&ws);

    let solution = graph
        .nodes
        .iter()
        .find(|n| n.kind == ArchKind::Solution)
        .unwrap();
    let folder = graph
        .nodes
        .iter()
        .find(|n| n.kind == ArchKind::SolutionFolder)
        .unwrap_or_else(|| panic!("`src` is a solution folder: {:?}", graph.nodes));
    assert_eq!(folder.label, "src");

    let mut contains = edges_of(&graph, EdgeKind::Contains);
    contains.sort();
    let mut expected = vec![
        (solution.id.as_str(), folder.id.as_str()),
        (folder.id.as_str(), id_of(&ws, "App")),
        (solution.id.as_str(), id_of(&ws, "Lib")),
    ];
    expected.sort();
    assert_eq!(contains, expected);

    assert!(
        graph.edges.iter().all(|e| e.kind == EdgeKind::Contains),
        "a solution produces no dependency edges: {:?}",
        graph.edges
    );
}

#[test]
fn a_missing_solution_member_says_its_path_has_been_normalised() {
    // Every other warning in this module quotes a path byte-for-byte as the
    // file spells it, so a reader can grep for it. This one cannot:
    // `solution::resolve`
    // replaces `\` with `/` before the graph ever sees the string, and the graph
    // may not reach back for the raw form. Keeping the mismatch silent would
    // send a reader grepping for `src/Ghost/Ghost.csproj` through a `.sln` that
    // says `src\Ghost\Ghost.csproj` and finding nothing, with no clue why.
    let ghost = r#"Project("{9A19103F-16F7-4668-BE54-9A1E7A4F7556}") = "Ghost", "src\Ghost\Ghost.csproj", "{55555555-5555-5555-5555-555555555555}"
EndProject
"#;
    let (_dir, ws) = scanned(&[
        ("src/Lib/Lib.csproj", LIB_CSPROJ),
        ("Repo.sln", &sln(ghost)),
    ]);

    let graph = project_graph(&ws);

    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    let warning = &graph.warnings[0];
    assert!(
        warning.contains("src/Ghost/Ghost.csproj"),
        "the warning must still name the member: {warning}"
    );
    assert!(
        warning.contains("normalis"),
        "the warning must say the separators were normalised, so a reader who \
         greps the solution file and finds nothing knows why: {warning}"
    );
}

#[test]
fn a_project_in_two_solutions_is_not_duplicated() {
    let (_dir, ws) = scanned(&[
        ("src/App/App.csproj", &app_csproj(&[])),
        ("src/Lib/Lib.csproj", LIB_CSPROJ),
        ("Repo.sln", &sln(APP_AND_LIB)),
        ("Other.sln", &sln(APP_AND_LIB)),
    ]);

    let graph = project_graph(&ws);

    let app = id_of(&ws, "App");
    assert_eq!(
        graph.nodes.iter().filter(|n| n.id == app).count(),
        1,
        "nodes: {:?}",
        graph.nodes
    );
    assert_eq!(
        graph
            .nodes
            .iter()
            .filter(|n| n.kind == ArchKind::Solution)
            .count(),
        2
    );
}

// ---------------------------------------------------------------------------
// Shape
// ---------------------------------------------------------------------------

#[test]
fn nodes_and_edges_are_ordered_deterministically() {
    // A diagram that reshuffles between runs produces a meaningless git diff.
    let files: Vec<(&str, String)> = vec![
        (
            "src/App/App.csproj",
            app_csproj(&[r"..\Lib\Lib.csproj", r"..\Zed\Zed.csproj"]),
        ),
        ("src/Lib/Lib.csproj", LIB_CSPROJ.to_string()),
        ("src/Zed/Zed.csproj", LIB_CSPROJ.to_string()),
        ("Repo.sln", sln(APP_AND_LIB)),
    ];
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(p, c)| (*p, c.as_str())).collect();
    let (_dir, ws) = scanned(&borrowed);

    let graph = project_graph(&ws);

    let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "nodes are sorted by id");

    let keyed: Vec<(&str, &str)> = graph
        .edges
        .iter()
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect();
    let mut sorted_edges = keyed.clone();
    sorted_edges.sort_unstable();
    assert_eq!(keyed, sorted_edges, "edges are sorted by endpoint");

    assert_eq!(
        graph,
        project_graph(&ws),
        "the same input yields the same graph"
    );
}

#[test]
fn an_empty_workspace_yields_an_empty_graph_with_no_warnings() {
    let dir = tempfile::tempdir().unwrap();
    let ws = scan(dir.path()).unwrap();

    let graph = project_graph(&ws);

    assert!(graph.nodes.is_empty());
    assert!(graph.edges.is_empty());
    assert!(graph.warnings.is_empty());
}

#[test]
fn the_graph_is_always_marked_as_derived_by_a_known_scanner_version() {
    let (_dir, ws) = scanned(&[("src/Lib/Lib.csproj", LIB_CSPROJ)]);

    assert_eq!(
        project_graph(&ws).derivation,
        Derivation::Derived {
            scanner: SCANNER_VERSION
        }
    );
}

#[test]
fn node_paths_are_relative_so_a_saved_graph_survives_a_move() {
    let (_dir, ws) = scanned(&[("src/Lib/Lib.csproj", LIB_CSPROJ)]);

    let graph = project_graph(&ws);
    let lib = &graph.nodes[0];

    assert_eq!(lib.path.as_deref(), Some(Path::new("src/Lib/Lib.csproj")));
    assert_eq!(lib.ecosystem.as_deref(), Some("dotnet"));
    assert_eq!(lib.project_id.as_deref(), Some(id_of(&ws, "Lib")));
}

// ---------------------------------------------------------------------------
// Colliding project ids
// ---------------------------------------------------------------------------

#[test]
fn two_projects_whose_ids_collide_each_keep_their_own_box_and_only_their_own_arrows() {
    // `Project::id` replaces both separators with `-`, so `src/a/App.csproj`
    // and `src-a/App.csproj` scan to the same id. Merging them loses a box and
    // hands the survivor an arrow the other project declared.
    let (_dir, ws) = scanned(&[
        ("src/a/App.csproj", &app_csproj(&["../Alpha/Alpha.csproj"])),
        ("src-a/App.csproj", &app_csproj(&["../Beta/Beta.csproj"])),
        ("src/Alpha/Alpha.csproj", LIB_CSPROJ),
        ("Beta/Beta.csproj", LIB_CSPROJ),
    ]);

    assert_eq!(
        ws.projects
            .iter()
            .filter(|p| p.id == "src-a-App.csproj")
            .count(),
        2,
        "the fixture only means anything while the scan really does collide"
    );

    let graph = project_graph(&ws);

    let boxes: Vec<&Path> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == ArchKind::Project)
        .filter_map(|n| n.path.as_deref())
        .collect();
    assert!(
        boxes.contains(&Path::new("src/a/App.csproj"))
            && boxes.contains(&Path::new("src-a/App.csproj")),
        "neither project may vanish, got {boxes:?}"
    );

    let alpha = graph
        .nodes
        .iter()
        .find(|n| n.path.as_deref() == Some(Path::new("src/a/App.csproj")))
        .unwrap();
    let beta = graph
        .nodes
        .iter()
        .find(|n| n.path.as_deref() == Some(Path::new("src-a/App.csproj")))
        .unwrap();
    assert_ne!(alpha.id, beta.id, "two boxes need two ids");

    let mut refs = edges_of(&graph, EdgeKind::ProjectReference);
    refs.sort_unstable();
    let mut expected = vec![
        (alpha.id.as_str(), id_of(&ws, "Alpha")),
        (beta.id.as_str(), id_of(&ws, "Beta")),
    ];
    expected.sort_unstable();
    assert_eq!(
        refs, expected,
        "each arrow belongs to the project that declared it"
    );

    assert!(
        graph
            .warnings
            .iter()
            .any(|w| w.contains("src/a/App.csproj") && w.contains("src-a/App.csproj")),
        "the collision must be reported by both real paths, got {:?}",
        graph.warnings
    );
}

#[test]
fn a_dependency_between_two_node_projects_sharing_an_id_is_still_drawn() {
    // The self-reference guard used to compare ids, and two projects can share
    // one, so a genuine edge between `pkg/a` and `pkg-a` was discarded as a
    // package depending on itself.
    let (_dir, ws) = scanned(&[
        (
            "pkg/a/package.json",
            r#"{"name":"alpha","dependencies":{"beta":"workspace:*"}}"#,
        ),
        ("pkg-a/package.json", r#"{"name":"beta"}"#),
    ]);

    let graph = project_graph(&ws);
    let node_id = |path: &str| {
        graph
            .nodes
            .iter()
            .find(|n| n.path.as_deref() == Some(Path::new(path)))
            .unwrap_or_else(|| panic!("no node for {path}"))
            .id
            .clone()
    };

    assert_eq!(
        edges_of(&graph, EdgeKind::PackageDependency),
        [(
            node_id("pkg/a/package.json").as_str(),
            node_id("pkg-a/package.json").as_str()
        )],
        "alpha depends on beta, and they are different projects"
    );
}

// ---------------------------------------------------------------------------
// Relations this graph has no edge kind for
// ---------------------------------------------------------------------------
//
// The failure these cover is not a wrong arrow, it is a *missing* one that
// nothing announces. A Tauri desktop app is two projects wired together by a
// third file — `tauri.conf.json` names the frontend it bundles and the
// resources it ships — and no rule here used to open that file, so both halves
// of the app floated as unrelated boxes and the diagram looked complete. The
// module's own contract says everything that could not become an edge lands in
// `warnings` rather than vanishing; these pin that the contract applies to the
// module's own blind spot too.

/// A `tauri.conf.json` naming a frontend and a bundled resource.
fn tauri_conf(frontend: &str, resource: &str, before_build: Option<&str>) -> String {
    let build_command = match before_build {
        Some(command) => format!("\"beforeBuildCommand\": \"{command}\","),
        None => String::new(),
    };
    format!(
        "{{\n  \"productName\": \"app\",\n  \"build\": {{ {build_command} \"frontendDist\": \
         \"{frontend}\" }},\n  \"bundle\": {{ \"active\": true, \"resources\": {{ \"{resource}\": \
         \"inspector/\" }} }}\n}}"
    )
}

const APP_CARGO: &str = "[package]\nname = \"cb-app\"\nversion = \"0.1.0\"\n";

const FRONTEND_PKG: &str = r#"{ "name": "frontend", "scripts": { "dev": "vite" } }"#;

#[test]
fn a_tauri_config_beside_a_project_reports_the_relations_it_names_rather_than_dropping_them() {
    let conf = tauri_conf("../dist", "resources/inspector/", None);
    let (_dir, ws) = scanned(&[
        ("package.json", FRONTEND_PKG),
        ("src-tauri/Cargo.toml", APP_CARGO),
        ("src-tauri/tauri.conf.json", &conf),
    ]);

    let graph = project_graph(&ws);

    let warning = graph
        .warnings
        .iter()
        .find(|w| w.contains("src-tauri/tauri.conf.json"))
        .unwrap_or_else(|| {
            panic!(
                "the file the relations live in must be named: {:?}",
                graph.warnings
            )
        });

    for quoted in ["'../dist'", "'resources/inspector/'"] {
        assert!(
            warning.contains(quoted),
            "the value must be quoted exactly as the file spells it, so it can be \
             grepped for: {warning}"
        );
    }
    assert!(
        warning.contains("no edge kind") && warning.contains("not drawn"),
        "the warning must say these are relations and that they were not drawn — \
         that is the whole point of it: {warning}"
    );
    assert!(
        warning.starts_with("cb-app: "),
        "a warning about a project opens with the name on its box: {warning}"
    );
}

#[test]
fn a_tauri_config_creates_no_node_and_no_edge() {
    // An IPC relationship and a packaging relationship are different claims
    // from a compile-time reference. Drawing either as an arrow is exactly the
    // overreach this module exists to prevent, so the fix is a warning and the
    // graph itself must be identical to the one derived without the file.
    let projects: &[(&str, &str)] = &[
        ("package.json", FRONTEND_PKG),
        ("src-tauri/Cargo.toml", APP_CARGO),
    ];
    let (_bare, bare_ws) = scanned(projects);
    let bare = project_graph(&bare_ws);

    let conf = tauri_conf("../dist", "resources/inspector/", None);
    let mut with_conf = projects.to_vec();
    with_conf.push(("src-tauri/tauri.conf.json", conf.as_str()));
    let (_dir, ws) = scanned(&with_conf);
    let graph = project_graph(&ws);

    assert_eq!(graph.nodes, bare.nodes, "no node may be invented");
    assert_eq!(graph.edges, bare.edges, "no arrow may be invented");
    assert!(
        bare.warnings.is_empty() && graph.warnings.len() == 1,
        "the only difference is the warning: {:?} vs {:?}",
        bare.warnings,
        graph.warnings
    );
}

#[test]
fn a_tauri_build_command_is_named_but_its_text_is_never_quoted() {
    // `beforeBuildCommand` is a command line, which is the one field here that
    // can carry a credential. Same discipline the signals module applies: name
    // the key so the reader can go and read it, publish nothing from the right
    // of it.
    let conf = tauri_conf(
        "../dist",
        "resources/inspector/",
        Some("NPM_TOKEN=hunter2 pnpm build"),
    );
    let (_dir, ws) = scanned(&[
        ("package.json", FRONTEND_PKG),
        ("src-tauri/Cargo.toml", APP_CARGO),
        ("src-tauri/tauri.conf.json", &conf),
    ]);

    let joined = project_graph(&ws).warnings.join("\n");

    assert!(
        joined.contains("beforeBuildCommand"),
        "the key must be named, or the relation it stands for is silent: {joined}"
    );
    for secret in ["NPM_TOKEN", "hunter2", "pnpm build"] {
        assert!(
            !joined.contains(secret),
            "{secret:?} came from the right of a command line and reached a warning: {joined}"
        );
    }
}

#[test]
fn a_tauri_value_that_is_not_shaped_like_a_path_is_refused_rather_than_quoted() {
    // Nothing stops a `frontendDist` holding a url, and a url holds
    // credentials. The relation is still reported — the reader is told the key
    // is there and was not drawn — but the value it holds is not republished.
    let conf = tauri_conf(
        "https://user:hunter2@build.internal/dist",
        "resources/inspector/",
        None,
    );
    let (_dir, ws) = scanned(&[
        ("src-tauri/Cargo.toml", APP_CARGO),
        ("src-tauri/tauri.conf.json", &conf),
    ]);

    let joined = project_graph(&ws).warnings.join("\n");

    assert!(
        joined.contains("frontendDist") && joined.contains("not drawn"),
        "the relation must still be reported: {joined}"
    );
    for secret in ["hunter2", "build.internal", "user:"] {
        assert!(
            !joined.contains(secret),
            "{secret:?} was quoted out of a value that is not a path: {joined}"
        );
    }
}

#[test]
fn a_tauri_config_that_will_not_parse_is_reported_rather_than_silently_skipped() {
    // Silence here would be the exact failure the warning exists to fix: a file
    // known to carry relations, unread, with nothing said about it.
    let (_dir, ws) = scanned(&[
        ("src-tauri/Cargo.toml", APP_CARGO),
        ("src-tauri/tauri.conf.json", "{ \"build\": { , } }"),
    ]);

    let graph = project_graph(&ws);

    assert!(
        graph
            .warnings
            .iter()
            .any(|w| w.contains("src-tauri/tauri.conf.json") && w.contains("could not be parsed")),
        "a file known to declare relations that could not be read must be \
         reported: {:?}",
        graph.warnings
    );
}

#[test]
fn a_workspace_with_no_such_file_gains_no_blind_spot_warning() {
    // The warning is only worth having if it fires on evidence. A repository
    // with nothing of the kind stays as quiet as it was before.
    let (_dir, ws) = scanned(&[("src/Lib/Lib.csproj", LIB_CSPROJ)]);

    let graph = project_graph(&ws);
    assert!(graph.warnings.is_empty(), "{:?}", graph.warnings);
}

#[test]
fn a_tauri_config_declaring_none_of_the_keys_that_carry_relations_says_nothing() {
    // The claim is about *relations*, not about the file existing. A config
    // with no frontend, no resources and no build command names no relation, so
    // nothing went undrawn and there is nothing to report.
    let (_dir, ws) = scanned(&[
        ("src-tauri/Cargo.toml", APP_CARGO),
        (
            "src-tauri/tauri.conf.json",
            r#"{ "productName": "app", "version": "0.1.0" }"#,
        ),
    ]);

    let graph = project_graph(&ws);
    assert!(graph.warnings.is_empty(), "{:?}", graph.warnings);
}

#[test]
fn a_bundled_resource_list_is_quoted_whether_it_is_an_array_or_a_map() {
    // Tauri accepts both spellings for `bundle.resources`. Reading only one of
    // them would make the warning's presence depend on a formatting choice.
    let (_dir, ws) = scanned(&[
        ("src-tauri/Cargo.toml", APP_CARGO),
        (
            "src-tauri/tauri.conf.json",
            r#"{ "bundle": { "resources": ["resources/inspector/", "assets/*"] } }"#,
        ),
    ]);

    let joined = project_graph(&ws).warnings.join("\n");

    assert!(
        joined.contains("'resources/inspector/'") && joined.contains("'assets/*'"),
        "both entries of the array form must be quoted: {joined}"
    );
}

// ---------------------------------------------------------------------------
// One warning list, one vocabulary
// ---------------------------------------------------------------------------

#[test]
fn a_warning_about_a_project_opens_with_the_name_that_is_printed_on_its_box() {
    // Every warning this module raises about a project opens with
    // `Project::name` — the string the diagram draws — except the unreadable
    // manifest report, which opened with the path instead. One list in two
    // vocabularies makes a reader work out, line by line, which kind of string
    // they are looking at. The path is still in the message, because it is the
    // thing they have to open; it is just no longer the subject.
    let (_dir, ws) = scanned(&[(
        "packages/broken/package.json",
        r#"{ "name": "@acme/broken", "dependencies": {}, }"#,
    )]);

    let name = ws
        .projects
        .iter()
        .find(|p| p.unreadable.is_some())
        .unwrap_or_else(|| panic!("the scan must keep the broken project: {:?}", ws.projects))
        .name
        .clone();

    let graph = project_graph(&ws);

    assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
    assert!(
        graph.warnings[0].starts_with(&format!("{name}: ")),
        "the warning must open with the name on the box: {}",
        graph.warnings[0]
    );
    assert!(
        graph.warnings[0].contains("packages/broken/package.json"),
        "and must still name the file to open: {}",
        graph.warnings[0]
    );
}
