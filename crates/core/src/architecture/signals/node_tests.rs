//! Tests for [`super::node`].
//!
//! Every test drives the real entry point over a real directory tree, because
//! almost every rule in that producer is a rule about what is *on disk* — a
//! route is a filename, a framework is a manifest entry, and an abstention is
//! usually triggered by a second file existing. Testing the helpers in
//! isolation would pass while the producer read the wrong directory.
//!
//! Where a test asserts what is *drawn*, it runs the signals through
//! [`admit`](super::framework::admit) rather than inspecting them directly.
//! The gate is what decides, and a producer that emits a beautiful signal the
//! gate refuses has produced nothing.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::model::{Project, ProjectKind};

use super::framework::{admit, Admitted, ComponentKind, Strength};
use super::node::{signals, NodeSignals};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("a parent")).expect("create dirs");
    fs::write(path, contents).expect("write fixture");
}

/// A Node project rooted at `relative_dir` inside `root`.
fn project(root: &Path, relative_dir: &str) -> Project {
    let dir = if relative_dir.is_empty() {
        root.to_path_buf()
    } else {
        root.join(relative_dir)
    };
    Project {
        id: if relative_dir.is_empty() {
            "root".to_string()
        } else {
            relative_dir.replace('/', "-")
        },
        name: relative_dir
            .rsplit('/')
            .next()
            .unwrap_or("root")
            .to_string(),
        manifest_path: dir.join("package.json"),
        dir,
        ecosystem: "node".to_string(),
        kind: ProjectKind::Executable,
        frameworks: Vec::new(),
        configurations: Vec::new(),
        is_test_project: false,
        test_runner: None,
        unreadable: None,
    }
}

fn run(root: &Path, relative_dir: &str) -> NodeSignals {
    signals(root, &project(root, relative_dir))
}

/// `(kind, label)` for every component the gate admitted, in id order.
fn drawn(admitted: &Admitted) -> Vec<(ComponentKind, String)> {
    admitted
        .components
        .iter()
        .map(|c| (c.kind, c.label.clone()))
        .collect()
}

/// Every detail text on the component with the given label.
fn details(admitted: &Admitted, label: &str) -> Vec<String> {
    admitted
        .components
        .iter()
        .filter(|c| c.label == label)
        .flat_map(|c| c.details.iter().map(|d| d.text.clone()))
        .collect()
}

fn admitted_from(found: &NodeSignals) -> Admitted {
    admit(found.signals.clone())
}

fn mentions(haystack: &[String], needle: &str) -> bool {
    haystack.iter().any(|line| line.contains(needle))
}

// ---------------------------------------------------------------------------
// Data clients declared in a manifest
// ---------------------------------------------------------------------------

#[test]
fn a_declared_postgres_driver_creates_a_database_component() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "services/orders/package.json",
        r#"{ "name": "orders", "dependencies": { "pg": "^8.11.3" } }"#,
    );

    let found = run(tmp.path(), "services/orders");
    let admitted = admitted_from(&found);

    assert_eq!(
        drawn(&admitted),
        vec![(ComponentKind::Database, "PostgreSQL".to_string())],
        "a declared pg dependency must draw PostgreSQL, got {:?}",
        drawn(&admitted)
    );
    assert_eq!(admitted.edges().len(), 1, "one project, one edge");
    let usage = &admitted.components[0].usages[0];
    assert_eq!(
        usage.evidence.path,
        Path::new("services/orders/package.json"),
        "evidence must cite the manifest, workspace-relative"
    );
    assert!(
        usage.evidence.excerpt.contains("\"pg\""),
        "the excerpt must be the line a reader can go and check, got {:?}",
        usage.evidence.excerpt
    );
    assert_eq!(
        usage.evidence.line,
        Some(1),
        "the dependency line number must be reported"
    );
}

#[test]
fn redis_is_a_cache_and_kafkajs_is_a_queue_not_two_databases() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "app/package.json",
        r#"{
  "name": "app",
  "dependencies": {
    "ioredis": "5.4.1",
    "kafkajs": "2.2.4"
  }
}"#,
    );

    let admitted = admitted_from(&run(tmp.path(), "app"));

    assert_eq!(
        drawn(&admitted),
        vec![
            (ComponentKind::Cache, "Redis".to_string()),
            (ComponentKind::MessageQueue, "Kafka".to_string()),
        ],
        "got {:?}",
        drawn(&admitted)
    );
}

#[test]
fn two_projects_declaring_the_same_driver_share_one_box_with_two_edges() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "a/package.json",
        r#"{ "name": "a", "dependencies": { "pg": "8" } }"#,
    );
    write(
        tmp.path(),
        "b/package.json",
        r#"{ "name": "b", "dependencies": { "postgres": "3" } }"#,
    );

    let mut all = run(tmp.path(), "a").signals;
    all.extend(run(tmp.path(), "b").signals);
    let admitted = admit(all);

    assert_eq!(
        drawn(&admitted),
        vec![(ComponentKind::Database, "PostgreSQL".to_string())],
        "two drivers for one engine are one technology, got {:?}",
        drawn(&admitted)
    );
    assert_eq!(admitted.edges().len(), 2, "one edge per declaring project");
}

#[test]
fn a_driver_in_dev_dependencies_only_never_creates_a_component() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "app/package.json",
        r#"{ "name": "app", "devDependencies": { "better-sqlite3": "9" } }"#,
    );

    let found = run(tmp.path(), "app");
    let admitted = admitted_from(&found);

    assert!(
        admitted.components.is_empty(),
        "a devDependency is a toolchain entry, not a deployed database, got {:?}",
        drawn(&admitted)
    );
    assert!(
        found.signals.iter().all(|s| s.strength == Strength::Medium),
        "a dev-only driver must be emitted at MEDIUM if at all"
    );
    assert!(
        mentions(&admitted.warnings(), "medium-without-high"),
        "the refusal must be counted, got {:?}",
        admitted.warnings()
    );
}

#[test]
fn a_dev_dependency_enriches_a_box_another_project_declared_but_adds_no_edge() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "api/package.json",
        r#"{ "name": "api", "dependencies": { "pg": "8" } }"#,
    );
    write(
        tmp.path(),
        "tests/package.json",
        r#"{ "name": "tests", "devDependencies": { "pg": "8" } }"#,
    );

    let mut all = run(tmp.path(), "api").signals;
    all.extend(run(tmp.path(), "tests").signals);
    let admitted = admit(all);

    assert_eq!(
        admitted.edges().len(),
        1,
        "only the runtime dependency earns an arrow, got {:?}",
        admitted.edges()
    );
    assert!(
        mentions(&details(&admitted, "PostgreSQL"), "devDependencies"),
        "the dev usage must still be visible as enrichment, got {:?}",
        details(&admitted, "PostgreSQL")
    );
}

#[test]
fn a_types_package_is_never_read_as_the_runtime_package_it_types() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "app/package.json",
        r#"{ "name": "app", "dependencies": { "@types/pg": "8.11.6", "pg": "8.11.3" } }"#,
    );

    let admitted = admitted_from(&run(tmp.path(), "app"));

    assert_eq!(
        drawn(&admitted),
        vec![(ComponentKind::Database, "PostgreSQL".to_string())],
        "only the runtime client is a database; its declaration files are not"
    );
    assert_eq!(
        admitted.components[0].usages.len(),
        1,
        "@types/pg must not be counted as a second usage, got {:?}",
        admitted.components[0].usages
    );
}

#[test]
fn a_package_json_that_does_not_parse_yields_nothing_rather_than_a_panic() {
    let tmp = TempDir::new().expect("tempdir");
    write(tmp.path(), "app/package.json", "{ this is not json");

    let found = run(tmp.path(), "app");

    assert!(found.signals.is_empty());
    assert!(
        mentions(&found.warnings, "could not be read"),
        "an unreadable manifest must be reported, got {:?}",
        found.warnings
    );
}

#[test]
fn a_project_from_another_ecosystem_is_not_read_at_all() {
    let tmp = TempDir::new().expect("tempdir");
    write(tmp.path(), "app/package.json", r#"{ "dependencies": {} }"#);
    let mut dotnet = project(tmp.path(), "app");
    dotnet.ecosystem = "dotnet".to_string();
    dotnet.manifest_path = tmp.path().join("app/App.csproj");

    let found = signals(tmp.path(), &dotnet);

    assert!(found.signals.is_empty());
    assert!(found.warnings.is_empty());
}

// ---------------------------------------------------------------------------
// The ORM problem
// ---------------------------------------------------------------------------

#[test]
fn an_orm_alone_says_a_database_exists_without_claiming_to_name_it() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "app/package.json",
        r#"{ "name": "app", "dependencies": { "@prisma/client": "5.14.0" } }"#,
    );

    let admitted = admitted_from(&run(tmp.path(), "app"));

    assert_eq!(
        drawn(&admitted),
        vec![(ComponentKind::Database, "Database (via Prisma)".to_string())],
        "an ORM proves a database and not which one, got {:?}",
        drawn(&admitted)
    );
}

#[test]
fn an_orm_beside_a_named_driver_does_not_add_a_second_database_box() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "app/package.json",
        r#"{ "name": "app", "dependencies": { "@prisma/client": "5", "pg": "8" } }"#,
    );

    let found = run(tmp.path(), "app");
    let admitted = admitted_from(&found);

    assert_eq!(
        drawn(&admitted),
        vec![(ComponentKind::Database, "PostgreSQL".to_string())],
        "the driver names the engine the ORM would not, got {:?}",
        drawn(&admitted)
    );
    assert!(
        mentions(&found.warnings, "Prisma"),
        "folding the ORM into the named engine must be said out loud, got {:?}",
        found.warnings
    );
}

#[test]
fn the_prisma_schema_provider_enriches_the_orm_box_rather_than_creating_one() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "app/package.json",
        r#"{ "name": "app", "dependencies": { "@prisma/client": "5" } }"#,
    );
    write(
        tmp.path(),
        "app/prisma/schema.prisma",
        "generator client {\n  provider = \"prisma-client-js\"\n}\n\ndatasource db {\n  provider = \"postgresql\"\n  url      = env(\"DATABASE_URL\")\n}\n",
    );

    let admitted = admitted_from(&run(tmp.path(), "app"));

    assert_eq!(
        drawn(&admitted),
        vec![(ComponentKind::Database, "Database (via Prisma)".to_string())],
        "the schema file is not a manifest, so it may not mint a PostgreSQL box, got {:?}",
        drawn(&admitted)
    );
    assert!(
        mentions(&details(&admitted, "Database (via Prisma)"), "PostgreSQL"),
        "the declared engine must still reach the reader as enrichment, got {:?}",
        details(&admitted, "Database (via Prisma)")
    );
}

#[test]
fn a_prisma_generator_provider_is_never_mistaken_for_the_datasource_engine() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "app/package.json",
        r#"{ "name": "app", "dependencies": { "@prisma/client": "5" } }"#,
    );
    write(
        tmp.path(),
        "app/prisma/schema.prisma",
        "generator client {\n  provider = \"prisma-client-js\"\n}\n\ndatasource db {\n  provider = \"mysql\"\n}\n",
    );

    let found = run(tmp.path(), "app");
    let admitted = admitted_from(&found);

    assert_eq!(
        details(&admitted, "Database (via Prisma)"),
        vec!["engine: MySQL".to_string()],
        "the generator block has a provider too, and it names a code generator"
    );
}

#[test]
fn a_prisma_provider_read_from_an_environment_variable_is_abstained_from() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "app/package.json",
        r#"{ "name": "app", "dependencies": { "@prisma/client": "5" } }"#,
    );
    write(
        tmp.path(),
        "app/prisma/schema.prisma",
        "datasource db {\n  provider = env(\"DB_PROVIDER\")\n}\n",
    );

    let found = run(tmp.path(), "app");
    let admitted = admitted_from(&found);

    assert!(
        details(&admitted, "Database (via Prisma)").is_empty(),
        "a provider chosen at deploy time is not a declared engine, got {:?}",
        details(&admitted, "Database (via Prisma)")
    );
    assert!(
        mentions(&found.warnings, "schema.prisma"),
        "the abstention must name the file it declined to read, got {:?}",
        found.warnings
    );
}

// ---------------------------------------------------------------------------
// File-system routing
// ---------------------------------------------------------------------------

fn next_app(root: &Path) {
    write(
        root,
        "web/package.json",
        r#"{ "name": "web", "dependencies": { "next": "14.2.3" } }"#,
    );
}

#[test]
fn a_next_dependency_makes_the_project_an_http_service_and_routes_enrich_it() {
    let tmp = TempDir::new().expect("tempdir");
    next_app(tmp.path());
    write(
        tmp.path(),
        "web/app/users/route.ts",
        "export async function GET() {}\n",
    );

    let admitted = admitted_from(&run(tmp.path(), "web"));

    assert_eq!(
        drawn(&admitted),
        vec![(ComponentKind::HttpService, "web".to_string())],
        "got {:?}",
        drawn(&admitted)
    );
    assert!(
        details(&admitted, "web").contains(&"/users".to_string()),
        "got {:?}",
        details(&admitted, "web")
    );
}

#[test]
fn an_app_directory_without_a_next_dependency_is_just_a_directory() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "web/package.json",
        r#"{ "name": "web", "dependencies": { "lodash": "4" } }"#,
    );
    write(
        tmp.path(),
        "web/app/users/route.ts",
        "export async function GET() {}\n",
    );

    let found = run(tmp.path(), "web");

    assert!(
        found.signals.is_empty(),
        "the convention only means something where the framework is declared, got {:?}",
        found.signals
    );
}

#[test]
fn a_next_route_group_does_not_appear_in_the_url() {
    let tmp = TempDir::new().expect("tempdir");
    next_app(tmp.path());
    write(
        tmp.path(),
        "web/app/(marketing)/pricing/route.ts",
        "export async function GET() {}\n",
    );

    let admitted = admitted_from(&run(tmp.path(), "web"));

    assert_eq!(
        details(&admitted, "web"),
        vec!["/pricing".to_string()],
        "a route group is organisation, not a URL segment"
    );
}

#[test]
fn a_next_dynamic_segment_keeps_the_spelling_the_author_wrote() {
    let tmp = TempDir::new().expect("tempdir");
    next_app(tmp.path());
    write(
        tmp.path(),
        "web/app/users/[id]/posts/[...rest]/route.ts",
        "export async function GET() {}\n",
    );

    let admitted = admitted_from(&run(tmp.path(), "web"));

    assert_eq!(
        details(&admitted, "web"),
        vec!["/users/[id]/posts/[...rest]".to_string()],
        "translating the segment into another framework's syntax would invent a spelling"
    );
}

#[test]
fn a_next_optional_catch_all_segment_keeps_the_spelling_the_author_wrote() {
    let tmp = TempDir::new().expect("tempdir");
    next_app(tmp.path());
    write(
        tmp.path(),
        "web/app/docs/[[...all]]/route.ts",
        "export async function GET() {}\n",
    );

    let admitted = admitted_from(&run(tmp.path(), "web"));

    assert_eq!(
        details(&admitted, "web"),
        vec!["/docs/[[...all]]".to_string()],
        "the optional catch-all is written out as the author wrote it, not expanded"
    );
}

#[test]
fn a_next_interception_marker_is_abstained_from_rather_than_guessed_at() {
    let tmp = TempDir::new().expect("tempdir");
    next_app(tmp.path());
    write(
        tmp.path(),
        "web/app/feed/(.)photo/route.ts",
        "export async function GET() {}\n",
    );

    let found = run(tmp.path(), "web");
    let admitted = admitted_from(&found);

    assert!(
        details(&admitted, "web").is_empty(),
        "the URL of an intercepting route depends on the route it intercepts, got {:?}",
        details(&admitted, "web")
    );
    assert!(
        mentions(&found.warnings, "(.)photo"),
        "the abstention must name what it declined, got {:?}",
        found.warnings
    );
}

#[test]
fn a_next_parallel_route_slot_is_abstained_from_rather_than_guessed_at() {
    let tmp = TempDir::new().expect("tempdir");
    next_app(tmp.path());
    write(
        tmp.path(),
        "web/app/@modal/photo/route.ts",
        "export async function GET() {}\n",
    );

    let found = run(tmp.path(), "web");
    let admitted = admitted_from(&found);

    assert!(
        details(&admitted, "web").is_empty(),
        "got {:?}",
        details(&admitted, "web")
    );
    assert!(
        mentions(&found.warnings, "@modal"),
        "the abstention must name what it declined, got {:?}",
        found.warnings
    );
}

#[test]
fn a_next_private_folder_is_not_a_route_at_all() {
    let tmp = TempDir::new().expect("tempdir");
    next_app(tmp.path());
    write(
        tmp.path(),
        "web/app/_internal/route.ts",
        "export async function GET() {}\n",
    );
    write(
        tmp.path(),
        "web/app/public/route.ts",
        "export async function GET() {}\n",
    );

    let admitted = admitted_from(&run(tmp.path(), "web"));

    assert_eq!(
        details(&admitted, "web"),
        vec!["/public".to_string()],
        "a leading underscore is Next's own opt-out; its sibling is still a route"
    );
}

#[test]
fn the_src_directory_holds_the_same_routers_as_the_project_root() {
    let tmp = TempDir::new().expect("tempdir");
    next_app(tmp.path());
    write(
        tmp.path(),
        "web/src/app/health/route.ts",
        "export async function GET() {}\n",
    );

    let admitted = admitted_from(&run(tmp.path(), "web"));

    assert_eq!(details(&admitted, "web"), vec!["/health".to_string()]);
}

#[test]
fn the_pages_router_api_directory_is_read_as_routes() {
    let tmp = TempDir::new().expect("tempdir");
    next_app(tmp.path());
    write(
        tmp.path(),
        "web/pages/api/users/index.ts",
        "export default h;\n",
    );
    write(
        tmp.path(),
        "web/pages/api/users/[id].ts",
        "export default h;\n",
    );
    write(tmp.path(), "web/pages/about.tsx", "export default Page;\n");

    let admitted = admitted_from(&run(tmp.path(), "web"));
    let mut routes = details(&admitted, "web");
    routes.sort();

    assert_eq!(
        routes,
        vec!["/api/users".to_string(), "/api/users/[id]".to_string()],
        "only pages/api is an HTTP endpoint; a page is not"
    );
}

#[test]
fn a_sveltekit_server_endpoint_is_read_as_a_route() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "site/package.json",
        r#"{ "name": "site", "dependencies": { "@sveltejs/kit": "2.5.0" } }"#,
    );
    write(
        tmp.path(),
        "site/src/routes/(app)/orders/[id]/+server.ts",
        "export const GET = () => {};\n",
    );

    let admitted = admitted_from(&run(tmp.path(), "site"));

    assert_eq!(details(&admitted, "site"), vec!["/orders/[id]".to_string()]);
}

#[test]
fn a_nuxt_server_api_file_carries_the_method_its_filename_declares() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "site/package.json",
        r#"{ "name": "site", "dependencies": { "nuxt": "3.12.0" } }"#,
    );
    write(
        tmp.path(),
        "site/server/api/orders/[id].get.ts",
        "export default defineEventHandler(() => {});\n",
    );
    write(
        tmp.path(),
        "site/server/api/health.ts",
        "export default defineEventHandler(() => {});\n",
    );

    let admitted = admitted_from(&run(tmp.path(), "site"));
    let mut routes = details(&admitted, "site");
    routes.sort();

    assert_eq!(
        routes,
        vec![
            "/api/health".to_string(),
            "GET /api/orders/[id]".to_string()
        ],
        "the method is stated only where the filename states it"
    );
}

#[test]
fn a_base_path_in_the_framework_config_abstains_from_the_whole_route_list() {
    let tmp = TempDir::new().expect("tempdir");
    next_app(tmp.path());
    write(
        tmp.path(),
        "web/app/users/route.ts",
        "export async function GET() {}\n",
    );
    write(
        tmp.path(),
        "web/next.config.js",
        "module.exports = { basePath: '/dashboard' };\n",
    );

    let found = run(tmp.path(), "web");
    let admitted = admitted_from(&found);

    assert_eq!(
        drawn(&admitted),
        vec![(ComponentKind::HttpService, "web".to_string())],
        "the service is still declared; only its paths are unknown"
    );
    assert!(
        details(&admitted, "web").is_empty(),
        "every listed path would be missing its prefix, got {:?}",
        details(&admitted, "web")
    );
    assert!(
        mentions(&found.warnings, "basePath"),
        "got {:?}",
        found.warnings
    );
}

#[test]
fn a_commented_out_base_path_does_not_trigger_the_abstention() {
    let tmp = TempDir::new().expect("tempdir");
    next_app(tmp.path());
    write(
        tmp.path(),
        "web/app/users/route.ts",
        "export async function GET() {}\n",
    );
    write(
        tmp.path(),
        "web/next.config.js",
        "module.exports = {\n  // basePath: '/dashboard',\n};\n",
    );

    let admitted = admitted_from(&run(tmp.path(), "web"));

    assert_eq!(details(&admitted, "web"), vec!["/users".to_string()]);
}

#[test]
fn a_framework_in_dev_dependencies_only_does_not_make_the_project_a_service() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "web/package.json",
        r#"{ "name": "web", "devDependencies": { "next": "14" } }"#,
    );
    write(
        tmp.path(),
        "web/app/users/route.ts",
        "export async function GET() {}\n",
    );

    let admitted = admitted_from(&run(tmp.path(), "web"));

    assert!(admitted.components.is_empty(), "got {:?}", drawn(&admitted));
}

// ---------------------------------------------------------------------------
// Route registrations in source — MEDIUM, and never load-bearing
// ---------------------------------------------------------------------------

fn express_app(root: &Path) {
    write(
        root,
        "api/package.json",
        r#"{ "name": "billing-api", "dependencies": { "express": "4.19.2" } }"#,
    );
}

#[test]
fn an_express_route_with_a_literal_path_enriches_the_declared_service() {
    let tmp = TempDir::new().expect("tempdir");
    express_app(tmp.path());
    write(
        tmp.path(),
        "api/src/server.js",
        "const app = express();\napp.get('/users', listUsers);\napp.post(\"/users\", create);\n",
    );

    let found = run(tmp.path(), "api");
    let admitted = admitted_from(&found);
    let mut routes = details(&admitted, "billing-api");
    routes.sort();

    assert_eq!(
        routes,
        vec!["GET /users".to_string(), "POST /users".to_string()],
        "got {routes:?}"
    );
    assert!(
        found
            .signals
            .iter()
            .filter(|s| s.detail.as_deref() == Some("GET /users"))
            .all(|s| s.strength == Strength::Medium),
        "a route read out of source is never a declared fact"
    );
}

#[test]
fn a_route_registration_alone_never_creates_a_service() {
    let tmp = TempDir::new().expect("tempdir");
    express_app(tmp.path());
    write(
        tmp.path(),
        "api/src/server.js",
        "app.get('/users', listUsers);\n",
    );

    let found = run(tmp.path(), "api");
    let routes_only: Vec<_> = found
        .signals
        .into_iter()
        .filter(|s| s.strength == Strength::Medium)
        .collect();
    let admitted = admit(routes_only);

    assert!(
        admitted.components.is_empty(),
        "without the manifest signal there is nothing to enrich, got {:?}",
        drawn(&admitted)
    );
}

#[test]
fn a_template_literal_route_is_abstained_from_and_warned_about() {
    let tmp = TempDir::new().expect("tempdir");
    express_app(tmp.path());
    write(
        tmp.path(),
        "api/src/server.js",
        "app.get(`/users/${id}`, showUser);\n",
    );

    let found = run(tmp.path(), "api");
    let admitted = admitted_from(&found);

    assert!(
        details(&admitted, "billing-api").is_empty(),
        "got {:?}",
        details(&admitted, "billing-api")
    );
    assert!(
        mentions(&found.warnings, "server.js"),
        "got {:?}",
        found.warnings
    );
}

#[test]
fn a_route_path_held_in_a_variable_is_abstained_from_and_warned_about() {
    let tmp = TempDir::new().expect("tempdir");
    express_app(tmp.path());
    write(
        tmp.path(),
        "api/src/server.js",
        "app.get(USERS_PATH, listUsers);\n",
    );

    let found = run(tmp.path(), "api");

    assert!(
        found
            .signals
            .iter()
            .all(|s| s.detail.as_deref() != Some("GET /users")),
        "nothing may be invented from a variable"
    );
    assert!(
        mentions(&found.warnings, "server.js"),
        "got {:?}",
        found.warnings
    );
}

#[test]
fn a_commented_out_route_registration_is_not_a_route() {
    let tmp = TempDir::new().expect("tempdir");
    express_app(tmp.path());
    write(
        tmp.path(),
        "api/src/server.js",
        "// app.get('/legacy', legacy);\napp.get('/users', listUsers);\n",
    );

    let admitted = admitted_from(&run(tmp.path(), "api"));

    assert_eq!(
        details(&admitted, "billing-api"),
        vec!["GET /users".to_string()],
        "a commented registration says the opposite of what it looks like"
    );
}

#[test]
fn a_route_inside_a_block_comment_is_not_a_route() {
    let tmp = TempDir::new().expect("tempdir");
    express_app(tmp.path());
    write(
        tmp.path(),
        "api/src/server.js",
        "/*\napp.get('/legacy', legacy);\n*/\napp.get('/users', listUsers);\n",
    );

    let admitted = admitted_from(&run(tmp.path(), "api"));

    assert_eq!(
        details(&admitted, "billing-api"),
        vec!["GET /users".to_string()],
        "a block comment spans lines and a line scan has to track that"
    );
}

#[test]
fn a_mount_prefix_anywhere_in_the_project_abstains_from_every_route_in_it() {
    let tmp = TempDir::new().expect("tempdir");
    express_app(tmp.path());
    write(
        tmp.path(),
        "api/src/index.js",
        "app.use('/api/v2', usersRouter);\n",
    );
    write(
        tmp.path(),
        "api/src/users.js",
        "router.get('/users', listUsers);\n",
    );

    let found = run(tmp.path(), "api");
    let admitted = admitted_from(&found);

    assert!(
        details(&admitted, "billing-api").is_empty(),
        "'/users' is not the URL; '/api/v2/users' is, and joining them is not provable here, got {:?}",
        details(&admitted, "billing-api")
    );
    assert!(
        mentions(&found.warnings, "/api/v2"),
        "got {:?}",
        found.warnings
    );
}

#[test]
fn a_root_mount_does_not_trigger_the_prefix_abstention() {
    let tmp = TempDir::new().expect("tempdir");
    express_app(tmp.path());
    write(
        tmp.path(),
        "api/src/index.js",
        "app.use('/', express.static('public'));\napp.get('/users', listUsers);\n",
    );

    let admitted = admitted_from(&run(tmp.path(), "api"));

    assert_eq!(
        details(&admitted, "billing-api"),
        vec!["GET /users".to_string()],
        "mounting at the root adds nothing to any path"
    );
}

#[test]
fn a_mount_prefix_written_as_a_template_literal_abstains_from_every_route_in_the_project() {
    let tmp = TempDir::new().expect("tempdir");
    express_app(tmp.path());
    write(
        tmp.path(),
        "api/src/index.js",
        "const base = '/api';\napp.use(`${base}/v2`, usersRouter);\n",
    );
    write(
        tmp.path(),
        "api/src/users.js",
        "router.get('/users', listUsers);\n",
    );

    let found = run(tmp.path(), "api");
    let admitted = admitted_from(&found);

    assert!(
        details(&admitted, "billing-api").is_empty(),
        "the mount exists whether or not its text can be read, so '/users' is still not the URL, \
         got {:?}",
        details(&admitted, "billing-api")
    );
    assert!(
        mentions(&found.warnings, "src/index.js"),
        "the abstention has to name the line that caused it, got {:?}",
        found.warnings
    );
}

#[test]
fn a_mount_prefix_held_in_a_variable_abstains_from_every_route_in_the_project() {
    let tmp = TempDir::new().expect("tempdir");
    express_app(tmp.path());
    write(
        tmp.path(),
        "api/src/index.js",
        "const base = '/api';\napp.use(base, usersRouter);\n",
    );
    write(
        tmp.path(),
        "api/src/users.js",
        "router.get('/users', listUsers);\n",
    );

    let found = run(tmp.path(), "api");
    let admitted = admitted_from(&found);

    assert!(
        details(&admitted, "billing-api").is_empty(),
        "an unprefixed path is a confident wrong answer, got {:?}",
        details(&admitted, "billing-api")
    );
    assert!(
        mentions(&found.warnings, "src/index.js"),
        "got {:?}",
        found.warnings
    );
}

#[test]
fn a_mount_prefix_read_from_a_member_expression_abstains_from_every_route_in_the_project() {
    let tmp = TempDir::new().expect("tempdir");
    express_app(tmp.path());
    write(
        tmp.path(),
        "api/src/index.js",
        "app.use(config.base, usersRouter);\n",
    );
    write(
        tmp.path(),
        "api/src/users.js",
        "router.get('/users', listUsers);\n",
    );

    let found = run(tmp.path(), "api");
    let admitted = admitted_from(&found);

    assert!(
        details(&admitted, "billing-api").is_empty(),
        "got {:?}",
        details(&admitted, "billing-api")
    );
    assert!(
        mentions(&found.warnings, "src/index.js"),
        "got {:?}",
        found.warnings
    );
}

#[test]
fn middleware_registered_with_use_and_no_path_does_not_abstain_from_anything() {
    let tmp = TempDir::new().expect("tempdir");
    express_app(tmp.path());
    write(
        tmp.path(),
        "api/src/index.js",
        "app.use(express.json());\napp.use(cors());\napp.use(errorHandler);\n\
         app.get('/users', listUsers);\n",
    );

    let found = run(tmp.path(), "api");
    let admitted = admitted_from(&found);

    assert_eq!(
        details(&admitted, "billing-api"),
        vec!["GET /users".to_string()],
        "middleware takes no path, so it moves no route and must suppress nothing"
    );
    assert!(
        found.warnings.is_empty(),
        "there is nothing here that could not be read, got {:?}",
        found.warnings
    );
}

#[test]
fn a_two_argument_use_on_a_receiver_that_does_not_route_does_not_abstain_from_anything() {
    let tmp = TempDir::new().expect("tempdir");
    express_app(tmp.path());
    write(
        tmp.path(),
        "api/src/index.js",
        "i18n.use(backend.plugin, initOptions);\napp.get('/users', listUsers);\n",
    );

    let found = run(tmp.path(), "api");
    let admitted = admitted_from(&found);

    assert_eq!(
        details(&admitted, "billing-api"),
        vec!["GET /users".to_string()],
        "an identifier is not path-shaped, so a receiver that does not route is not a mount"
    );
    assert!(found.warnings.is_empty(), "got {:?}", found.warnings);
}

#[test]
fn a_fastify_register_prefix_held_in_a_variable_abstains_exactly_as_a_literal_one_does() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "api/package.json",
        r#"{ "name": "billing-api", "dependencies": { "fastify": "4.28.0" } }"#,
    );
    write(
        tmp.path(),
        "api/src/index.js",
        "fastify.register(routes, { prefix: base });\n",
    );
    write(
        tmp.path(),
        "api/src/routes.js",
        "fastify.get('/users', listUsers);\n",
    );

    let found = run(tmp.path(), "api");
    let admitted = admitted_from(&found);

    assert!(
        details(&admitted, "billing-api").is_empty(),
        "an unreadable prefix moves the routes just as a readable one does, got {:?}",
        details(&admitted, "billing-api")
    );
    assert!(
        mentions(&found.warnings, "src/index.js"),
        "got {:?}",
        found.warnings
    );
}

#[test]
fn a_fastify_register_with_no_prefix_option_does_not_abstain_from_anything() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "api/package.json",
        r#"{ "name": "billing-api", "dependencies": { "fastify": "4.28.0" } }"#,
    );
    write(
        tmp.path(),
        "api/src/index.js",
        "fastify.register(cors, { origin: true });\nfastify.get('/users', listUsers);\n",
    );

    let found = run(tmp.path(), "api");
    let admitted = admitted_from(&found);

    assert_eq!(
        details(&admitted, "billing-api"),
        vec!["GET /users".to_string()],
        "a plugin registered without a prefix moves nothing"
    );
    assert!(found.warnings.is_empty(), "got {:?}", found.warnings);
}

#[test]
fn a_fastify_register_prefix_abstains_exactly_as_a_mount_does() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "api/package.json",
        r#"{ "name": "billing-api", "dependencies": { "fastify": "4.28.0" } }"#,
    );
    write(
        tmp.path(),
        "api/src/index.js",
        "fastify.register(routes, { prefix: '/v1' });\n",
    );
    write(
        tmp.path(),
        "api/src/routes.js",
        "fastify.get('/users', listUsers);\n",
    );

    let found = run(tmp.path(), "api");
    let admitted = admitted_from(&found);

    assert!(
        details(&admitted, "billing-api").is_empty(),
        "got {:?}",
        details(&admitted, "billing-api")
    );
    assert!(
        mentions(&found.warnings, "prefix"),
        "got {:?}",
        found.warnings
    );
}

#[test]
fn a_map_lookup_that_happens_to_take_a_path_is_not_a_route() {
    let tmp = TempDir::new().expect("tempdir");
    express_app(tmp.path());
    write(
        tmp.path(),
        "api/src/server.js",
        "const hit = cache.get('/users', fallback);\napp.get('/users', listUsers);\n",
    );

    let found = run(tmp.path(), "api");
    let admitted = admitted_from(&found);

    assert_eq!(
        details(&admitted, "billing-api"),
        vec!["GET /users".to_string()],
        "one registration, read once, from the receiver that routes"
    );
    assert!(
        !mentions(&found.warnings, "cache"),
        "a cache lookup is not even a candidate, so it must not be warned about either, got {:?}",
        found.warnings
    );
}

#[test]
fn nest_controllers_are_abstained_from_because_the_prefix_lives_on_the_class() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "api/package.json",
        r#"{ "name": "billing-api", "dependencies": { "@nestjs/core": "10.3.0" } }"#,
    );
    write(
        tmp.path(),
        "api/src/users.controller.ts",
        "@Controller('users')\nexport class UsersController {\n  @Get(':id')\n  find() {}\n}\n",
    );

    let found = run(tmp.path(), "api");
    let admitted = admitted_from(&found);

    assert_eq!(
        drawn(&admitted),
        vec![(ComponentKind::HttpService, "billing-api".to_string())],
        "the manifest still declares an HTTP service"
    );
    assert!(
        details(&admitted, "billing-api").is_empty(),
        "joining a class decorator to a method decorator is exactly the invention this refuses, got {:?}",
        details(&admitted, "billing-api")
    );
    assert!(
        mentions(&found.warnings, "NestJS"),
        "got {:?}",
        found.warnings
    );
}

// ---------------------------------------------------------------------------
// The standing prohibitions, checked against what this producer actually emits
// ---------------------------------------------------------------------------

#[test]
fn nothing_this_producer_emits_is_ever_refused_by_the_gate_as_malformed() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "api/package.json",
        r#"{
  "name": "billing-api",
  "dependencies": {
    "express": "4.19.2",
    "pg": "8.11.3",
    "ioredis": "5.4.1",
    "@prisma/client": "5.14.0"
  }
}"#,
    );
    write(
        tmp.path(),
        "api/src/server.js",
        "app.get('/users', listUsers);\n",
    );

    let found = run(tmp.path(), "api");
    let admitted = admitted_from(&found);

    assert_eq!(
        drawn(&admitted),
        vec![
            (ComponentKind::Cache, "Redis".to_string()),
            (ComponentKind::Database, "PostgreSQL".to_string()),
            (ComponentKind::HttpService, "billing-api".to_string()),
        ],
        "this fixture has to actually produce signals, or the assertion below is vacuous"
    );
    let unexpected: Vec<_> = admitted
        .discarded
        .iter()
        .map(|d| format!("{:?} {:?}", d.reason, d.label))
        .collect();
    assert!(
        unexpected.is_empty(),
        "every signal this producer emits should be admissible by construction, got {unexpected:?}"
    );
}

#[test]
fn no_dependency_version_range_is_ever_used_as_a_label() {
    let tmp = TempDir::new().expect("tempdir");
    write(
        tmp.path(),
        "api/package.json",
        r#"{ "name": "billing-api", "dependencies": { "pg": "git+ssh://git@host/pg.git#v8" } }"#,
    );

    let found = run(tmp.path(), "api");
    let visible = format!("{:?} {:?}", found.signals, found.warnings);

    assert_eq!(
        found.signals.len(),
        1,
        "the dependency must still be read, or this test proves nothing"
    );
    assert!(
        !visible.contains("git@host"),
        "a version specifier can be a credentialed URL and never belongs on a diagram, got {visible}"
    );
}
