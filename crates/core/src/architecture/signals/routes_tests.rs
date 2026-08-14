//! Tests for [`super::routes`].
//!
//! Every test builds a real temporary workspace, runs the real
//! [`crate::workspace::scan`] and the real [`crate::symbols::index::build`] over
//! it, and asks the producer for signals. Hand-constructing a
//! [`SymbolIndex`](crate::symbols::index::SymbolIndex) would test the
//! fabrication: the producer's whole job is to line route syntax up with the
//! classes that index actually recorded, and a fake index is exactly where a
//! wrong assumption about `declaration()` would hide.
//!
//! Most of the tests below are named for what the producer *refuses*. That is
//! the point of this file. Emitting `GET /api/orders` for a route that really is
//! `GET /api/v1/orders` is worse than emitting nothing at all, because a route
//! list is precisely the sort of output a reader will act on without checking.

use super::framework::{admit, ComponentKind, Evidence, Signal, Strength};
use super::routes::*;
use crate::symbols::index::{self, SymbolIndex};
use crate::workspace::{scan, Workspace};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const WEB_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk.Web">
  <PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net8.0</TargetFramework></PropertyGroup>
</Project>"#;

const TEST_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><TargetFramework>net8.0</TargetFramework><IsTestProject>true</IsTestProject></PropertyGroup>
</Project>"#;

fn workspace_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, contents).unwrap();
    }
    dir
}

/// The index is built from [`Workspace::root`] rather than from the temporary
/// directory's own path, because [`scan`] canonicalises the root it was given
/// and the project directories it records are under the canonical form. On
/// Windows the two spellings differ — the temporary directory arrives with a
/// short `PROGRA~1`-style component — and an index built from the uncanonical
/// one attributes every file to no project at all. The application has the same
/// obligation, and gets it right for the same reason: it holds a `Workspace`
/// and passes its `root` on.
fn scanned(files: &[(&str, &str)]) -> (tempfile::TempDir, Workspace, SymbolIndex) {
    let dir = workspace_with(files);
    let ws = scan(dir.path()).unwrap();
    let idx = index::build(&ws.root, &ws.projects);
    (dir, ws, idx)
}

/// Run the producer over a workspace made of the given files.
fn routes_of(files: &[(&str, &str)]) -> RouteScan {
    let (dir, ws, idx) = scanned(files);
    let found = route_signals(&ws.root, &ws.projects, &idx);
    drop(dir);
    found
}

/// A workspace holding one web project plus the given extra files, all placed
/// under `src/Orders.Api/`.
fn api_with(extra: &[(&str, &str)]) -> RouteScan {
    let mut files = vec![("src/Orders.Api/Orders.Api.csproj", WEB_CSPROJ)];
    files.extend_from_slice(extra);
    routes_of(&files)
}

/// The route lines a scan produced, in order.
fn details(found: &RouteScan) -> Vec<String> {
    found
        .signals
        .iter()
        .map(|s| s.detail.clone().unwrap_or_default())
        .collect()
}

fn warnings_mentioning(found: &RouteScan, needle: &str) -> Vec<String> {
    found
        .warnings
        .iter()
        .filter(|w| w.contains(needle))
        .cloned()
        .collect()
}

/// A controller file body wrapped in the usual namespace, so every fixture
/// exercises the same shape the index really sees.
fn controller(attributes: &str, class: &str, body: &str) -> String {
    format!("namespace Orders.Api.Controllers;\n\n{attributes}public class {class} : ControllerBase\n{{\n{body}}}\n")
}

const LIST_ACTION: &str = "    [HttpGet]\n    public IActionResult List() => Ok();\n";

// ---------------------------------------------------------------------------
// Controllers: what is admitted
// ---------------------------------------------------------------------------

#[test]
fn a_controller_with_an_api_controller_attribute_produces_one_route_per_action() {
    let found = api_with(&[(
        "src/Orders.Api/Controllers/OrdersController.cs",
        &controller(
            "[ApiController]\n[Route(\"api/orders\")]\n",
            "OrdersController",
            "    [HttpGet]\n    public IActionResult List() => Ok();\n\n    \
             [HttpGet(\"{id}\")]\n    public IActionResult Get(int id) => Ok();\n\n    \
             [HttpPost]\n    public IActionResult Create() => Ok();\n",
        ),
    )]);

    assert_eq!(
        details(&found),
        vec![
            "GET /api/orders".to_string(),
            "GET /api/orders/{id}".to_string(),
            "POST /api/orders".to_string(),
        ],
        "warnings were {:?}",
        found.warnings
    );
}

#[test]
fn a_route_attribute_alone_is_enough_to_make_a_class_a_controller() {
    let found = api_with(&[(
        "src/Orders.Api/Controllers/OrdersController.cs",
        &controller("[Route(\"api/orders\")]\n", "OrdersController", LIST_ACTION),
    )]);

    assert_eq!(details(&found), vec!["GET /api/orders".to_string()]);
}

#[test]
fn the_controller_token_is_substituted_with_the_class_name_minus_its_suffix() {
    let found = api_with(&[(
        "src/Orders.Api/Controllers/OrdersController.cs",
        &controller(
            "[ApiController]\n[Route(\"api/[controller]\")]\n",
            "OrdersController",
            LIST_ACTION,
        ),
    )]);

    assert_eq!(details(&found), vec!["GET /api/Orders".to_string()]);
}

#[test]
fn an_action_route_beginning_with_a_slash_replaces_the_class_route() {
    let found = api_with(&[(
        "src/Orders.Api/Controllers/OrdersController.cs",
        &controller(
            "[ApiController]\n[Route(\"api/orders\")]\n",
            "OrdersController",
            "    [HttpGet(\"/health/orders\")]\n    public IActionResult Health() => Ok();\n",
        ),
    )]);

    assert_eq!(details(&found), vec!["GET /health/orders".to_string()]);
}

#[test]
fn a_named_route_argument_is_not_a_template_and_leaves_the_class_route_alone() {
    let found = api_with(&[(
        "src/Orders.Api/Controllers/OrdersController.cs",
        &controller(
            "[ApiController]\n[Route(\"api/orders\")]\n",
            "OrdersController",
            "    [HttpGet(Name = \"ListOrders\")]\n    public IActionResult List() => Ok();\n",
        ),
    )]);

    assert_eq!(details(&found), vec!["GET /api/orders".to_string()]);
}

// ---------------------------------------------------------------------------
// Controllers: what is refused
// ---------------------------------------------------------------------------

#[test]
fn a_controller_without_an_attribute_is_not_a_controller() {
    let found = api_with(&[(
        "src/Orders.Api/Controllers/OrdersController.cs",
        &controller("", "OrdersController", LIST_ACTION),
    )]);

    assert!(
        found.signals.is_empty(),
        "a naming convention alone created routes: {:?}",
        details(&found)
    );
    assert!(
        !warnings_mentioning(&found, "OrdersController").is_empty(),
        "the refusal was silent: {:?}",
        found.warnings
    );
}

#[test]
fn a_commented_out_attribute_is_not_an_attribute() {
    let found = api_with(&[(
        "src/Orders.Api/Controllers/OrdersController.cs",
        &controller(
            "// [ApiController]\n// [Route(\"api/orders\")]\n",
            "OrdersController",
            LIST_ACTION,
        ),
    )]);

    assert!(
        found.signals.is_empty(),
        "a commented-out attribute was read as live: {:?}",
        details(&found)
    );
}

#[test]
fn a_route_with_an_unknown_token_is_not_resolved() {
    let found = api_with(&[(
        "src/Orders.Api/Controllers/OrdersController.cs",
        &controller(
            "[ApiController]\n[Route(\"api/[area]/[controller]\")]\n",
            "OrdersController",
            LIST_ACTION,
        ),
    )]);

    assert!(
        found.signals.is_empty(),
        "an unresolved token was guessed at: {:?}",
        details(&found)
    );
    assert!(
        !warnings_mentioning(&found, "api/[area]/[controller]").is_empty(),
        "the unresolved template was not reported verbatim: {:?}",
        found.warnings
    );
}

#[test]
fn an_action_route_with_an_unknown_token_is_not_resolved() {
    let found = api_with(&[(
        "src/Orders.Api/Controllers/OrdersController.cs",
        &controller(
            "[ApiController]\n[Route(\"api/orders\")]\n",
            "OrdersController",
            "    [HttpGet(\"[action]\")]\n    public IActionResult List() => Ok();\n",
        ),
    )]);

    assert!(
        found.signals.is_empty(),
        "an [action] token was substituted: {:?}",
        details(&found)
    );
}

#[test]
fn a_non_literal_route_argument_is_not_resolved() {
    let found = api_with(&[(
        "src/Orders.Api/Controllers/OrdersController.cs",
        &controller(
            "[ApiController]\n[Route(RouteConstants.Orders)]\n",
            "OrdersController",
            LIST_ACTION,
        ),
    )]);

    assert!(
        found.signals.is_empty(),
        "a constant was read as if it were a literal: {:?}",
        details(&found)
    );
    assert!(
        !warnings_mentioning(&found, "OrdersController.cs").is_empty(),
        "the refusal was silent: {:?}",
        found.warnings
    );
}

#[test]
fn two_class_level_route_attributes_are_ambiguous_and_abstain() {
    let found = api_with(&[(
        "src/Orders.Api/Controllers/OrdersController.cs",
        &controller(
            "[ApiController]\n[Route(\"api/orders\")]\n[Route(\"api/v2/orders\")]\n",
            "OrdersController",
            LIST_ACTION,
        ),
    )]);

    assert!(
        found.signals.is_empty(),
        "one of two class routes was picked: {:?}",
        details(&found)
    );
    assert!(
        !warnings_mentioning(&found, "OrdersController.cs").is_empty(),
        "the ambiguity was not reported: {:?}",
        found.warnings
    );
}

#[test]
fn a_class_not_named_controller_is_never_examined() {
    let found = api_with(&[(
        "src/Orders.Api/Endpoints/OrderEndpoints.cs",
        &controller(
            "[ApiController]\n[Route(\"api/orders\")]\n",
            "OrderEndpoints",
            LIST_ACTION,
        ),
    )]);

    assert!(found.signals.is_empty(), "{:?}", details(&found));
    assert!(
        found.warnings.is_empty(),
        "a class that is not a candidate produced a warning: {:?}",
        found.warnings
    );
}

#[test]
fn a_controller_in_a_test_project_is_not_a_route() {
    let found = routes_of(&[
        ("src/Orders.Api/Orders.Api.csproj", WEB_CSPROJ),
        ("tests/Orders.Tests/Orders.Tests.csproj", TEST_CSPROJ),
        (
            "tests/Orders.Tests/FakeOrdersController.cs",
            &controller(
                "[ApiController]\n[Route(\"api/fake\")]\n",
                "FakeOrdersController",
                LIST_ACTION,
            ),
        ),
    ]);

    assert!(
        found.signals.is_empty(),
        "a test fixture became an endpoint of the system: {:?}",
        details(&found)
    );
    assert!(
        !warnings_mentioning(&found, "test project").is_empty(),
        "the test project's candidates were dropped silently: {:?}",
        found.warnings
    );
}

#[test]
fn an_abstract_controller_declares_no_endpoints_even_when_it_carries_both_attributes() {
    let found = api_with(&[(
        "src/Orders.Api/Controllers/BaseController.cs",
        "namespace Orders.Api.Controllers;\n\n[ApiController]\n[Route(\"api/[controller]\")]\n\
         public abstract class BaseController : ControllerBase\n{\n    [HttpGet(\"ping\")]\n    \
         public IActionResult Ping() => Ok();\n}\n",
    )]);

    assert!(
        found.signals.is_empty(),
        "an abstract controller, which the framework never registers, became an endpoint: {:?}",
        details(&found)
    );
    assert!(
        !warnings_mentioning(&found, "BaseController").is_empty(),
        "the refusal was silent: {:?}",
        found.warnings
    );
}

#[test]
fn a_concrete_controller_that_inherits_its_routes_from_an_abstract_base_is_not_resolved() {
    let found = api_with(&[
        (
            "src/Orders.Api/Controllers/BaseController.cs",
            "namespace Orders.Api.Controllers;\n\n[ApiController]\n[Route(\"api/[controller]\")]\n\
             public abstract class BaseController : ControllerBase\n{\n    [HttpGet(\"ping\")]\n    \
             public IActionResult Ping() => Ok();\n}\n",
        ),
        (
            "src/Orders.Api/Controllers/DerivedController.cs",
            "namespace Orders.Api.Controllers;\n\n\
             public class DerivedController : BaseController\n{\n}\n",
        ),
    ]);

    assert!(
        found.signals.is_empty(),
        "a route inherited from a base class was named without reading the base: {:?}",
        details(&found)
    );
    assert!(
        !warnings_mentioning(&found, "DerivedController").is_empty(),
        "the derived controller was dropped silently: {:?}",
        found.warnings
    );
}

/// The one case where a derived class is read: it carries its own attributes,
/// so the ordinary rules apply to it and the routes it declares *itself* are
/// real. What it inherits from the base is not added, and that under-report is
/// the deliberate half — the framework really does register `api/derived/ping`
/// here, and naming it would mean resolving `: BaseController` to a declaration.
/// The base's own refusal is what tells the reader the list is short.
#[test]
fn a_derived_controller_declares_the_routes_it_carries_itself_and_none_it_inherits() {
    let found = api_with(&[
        (
            "src/Orders.Api/Controllers/BaseController.cs",
            "namespace Orders.Api.Controllers;\n\n[ApiController]\n[Route(\"api/[controller]\")]\n\
             public abstract class BaseController : ControllerBase\n{\n    [HttpGet(\"ping\")]\n    \
             public IActionResult Ping() => Ok();\n}\n",
        ),
        (
            "src/Orders.Api/Controllers/DerivedController.cs",
            "namespace Orders.Api.Controllers;\n\n[ApiController]\n[Route(\"api/derived\")]\n\
             public class DerivedController : BaseController\n{\n    [HttpGet(\"own\")]\n    \
             public IActionResult Own() => Ok();\n}\n",
        ),
    ]);

    assert_eq!(
        details(&found),
        vec!["GET /api/derived/own".to_string()],
        "an inherited action was named, or the class's own one was lost: warnings {:?}",
        found.warnings
    );
    assert!(
        !warnings_mentioning(&found, "BaseController").is_empty(),
        "nothing told the reader that the base's actions were not resolved: {:?}",
        found.warnings
    );
}

#[test]
fn a_class_that_merely_has_abstract_inside_its_name_is_still_a_controller() {
    let found = api_with(&[(
        "src/Orders.Api/Controllers/AbstractionsController.cs",
        &controller(
            "[ApiController]\n[Route(\"api/abstractions\")]\n",
            "AbstractionsController",
            LIST_ACTION,
        ),
    )]);

    assert_eq!(details(&found), vec!["GET /api/abstractions".to_string()]);
}

#[test]
fn an_action_marked_non_action_is_not_a_route_although_its_siblings_are() {
    let found = api_with(&[(
        "src/Orders.Api/Controllers/RController.cs",
        &controller(
            "[ApiController]\n[Route(\"api/r\")]\n",
            "RController",
            "    [HttpGet(\"ok\")]\n    [NonAction]\n    public IActionResult B() => Ok();\n\n    \
             [HttpGet(\"real\")]\n    public IActionResult C() => Ok();\n",
        ),
    )]);

    assert_eq!(
        details(&found),
        vec!["GET /api/r/real".to_string()],
        "warnings were {:?}",
        found.warnings
    );
}

#[test]
fn a_non_action_attribute_on_the_method_s_own_line_still_suppresses_the_route() {
    let found = api_with(&[(
        "src/Orders.Api/Controllers/RController.cs",
        &controller(
            "[ApiController]\n[Route(\"api/r\")]\n",
            "RController",
            "    [HttpGet(\"ok\")]\n    [NonAction] public IActionResult B() => Ok();\n",
        ),
    )]);

    assert!(
        found.signals.is_empty(),
        "a method the author excluded from routing became an endpoint: {:?}",
        details(&found)
    );
}

// ---------------------------------------------------------------------------
// Minimal APIs
// ---------------------------------------------------------------------------

const PROGRAM_HEAD: &str =
    "var builder = WebApplication.CreateBuilder(args);\nvar app = builder.Build();\n\n";

#[test]
fn a_literal_minimal_api_registration_is_a_route() {
    let found = api_with(&[(
        "src/Orders.Api/Program.cs",
        &format!(
            "{PROGRAM_HEAD}app.MapGet(\"/orders\", () => Results.Ok());\n\
             app.MapPost(\"/orders\", () => Results.Ok());\n\
             app.MapDelete(\"/orders/{{id}}\", (int id) => Results.NoContent());\n\napp.Run();\n"
        ),
    )]);

    assert_eq!(
        details(&found),
        vec![
            "DELETE /orders/{id}".to_string(),
            "GET /orders".to_string(),
            "POST /orders".to_string(),
        ],
        "warnings were {:?}",
        found.warnings
    );
}

#[test]
fn a_map_group_prefix_is_never_concatenated_onto_its_endpoints() {
    let found = api_with(&[(
        "src/Orders.Api/Program.cs",
        &format!(
            "{PROGRAM_HEAD}var group = app.MapGroup(\"/api/v1\");\n\
             group.MapGet(\"/orders\", () => Results.Ok());\n\napp.Run();\n"
        ),
    )]);

    assert!(
        !details(&found).iter().any(|d| d.contains("/api/v1")),
        "a group prefix was concatenated: {:?}",
        details(&found)
    );
    assert!(
        found.signals.is_empty(),
        "an endpoint was reported without the prefix it is really registered under: {:?}",
        details(&found)
    );
    assert!(
        !warnings_mentioning(&found, "MapGroup").is_empty(),
        "the group was not reported as unresolved: {:?}",
        found.warnings
    );
}

#[test]
fn a_map_group_in_one_file_stops_endpoints_being_resolved_in_every_other_file_of_that_project() {
    let found = api_with(&[
        (
            "src/Orders.Api/Program.cs",
            &format!("{PROGRAM_HEAD}app.MapGroup(\"/api/v1\").MapOrders();\n\napp.Run();\n"),
        ),
        (
            "src/Orders.Api/Endpoints/OrderEndpoints.cs",
            "namespace Orders.Api.Endpoints;\n\npublic static class OrderEndpoints\n{\n    \
             public static void MapOrders(this IEndpointRouteBuilder group)\n    {\n        \
             group.MapGet(\"/orders\", () => Results.Ok());\n        \
             group.MapPost(\"/orders\", () => Results.Ok());\n    }\n}\n",
        ),
    ]);

    assert!(
        found.signals.is_empty(),
        "endpoints registered in a sibling file were reported without the group prefix they are \
         really mounted under: {:?}",
        details(&found)
    );
    assert!(
        !warnings_mentioning(&found, "OrderEndpoints.cs").is_empty(),
        "the sibling file's endpoints were dropped without saying so: {:?}",
        found.warnings
    );
}

#[test]
fn a_method_that_receives_a_route_group_builder_stops_that_project_resolving_endpoints() {
    let found = api_with(&[(
        "src/Orders.Api/Endpoints/OrderEndpoints.cs",
        "namespace Orders.Api.Endpoints;\n\npublic static class OrderEndpoints\n{\n    \
         public static RouteGroupBuilder MapOrders(this RouteGroupBuilder group)\n    {\n        \
         group.MapGet(\"/orders\", () => Results.Ok());\n        return group;\n    }\n}\n",
    )]);

    assert!(
        found.signals.is_empty(),
        "an endpoint registered on a group parameter was reported at the prefix-free path: {:?}",
        details(&found)
    );
    assert!(
        !warnings_mentioning(&found, "OrderEndpoints.cs").is_empty(),
        "the refusal was silent: {:?}",
        found.warnings
    );
}

/// The `MapGroup` call and the endpoints it mounts need not even share a
/// project: a class library holding the extension method is the layout the
/// framework's own templates encourage. The type name in the method's signature
/// is what carries the refusal across that boundary.
#[test]
fn endpoints_in_a_library_that_takes_a_route_group_are_not_resolved_though_the_group_is_elsewhere()
{
    let found = routes_of(&[
        ("src/Orders.Api/Orders.Api.csproj", WEB_CSPROJ),
        (
            "src/Orders.Api/Program.cs",
            &format!("{PROGRAM_HEAD}app.MapGroup(\"/api/v1\").MapOrders();\n\napp.Run();\n"),
        ),
        (
            "src/Orders.Endpoints/Orders.Endpoints.csproj",
            "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup><TargetFramework>net8.0\
             </TargetFramework></PropertyGroup>\n</Project>",
        ),
        (
            "src/Orders.Endpoints/OrderEndpoints.cs",
            "namespace Orders.Endpoints;\n\npublic static class OrderEndpoints\n{\n    \
             public static RouteGroupBuilder MapOrders(this RouteGroupBuilder group)\n    {\n        \
             group.MapGet(\"/orders\", () => Results.Ok());\n        return group;\n    }\n}\n",
        ),
    ]);

    assert!(
        found.signals.is_empty(),
        "an endpoint was named at a path the calling project chose the prefix for: {:?}",
        details(&found)
    );
    assert!(
        !warnings_mentioning(&found, "OrderEndpoints.cs").is_empty(),
        "the refusal was silent: {:?}",
        found.warnings
    );
}

#[test]
fn a_project_with_no_route_group_anywhere_still_resolves_endpoints_across_several_files() {
    let found = api_with(&[
        (
            "src/Orders.Api/Program.cs",
            &format!("{PROGRAM_HEAD}app.MapGet(\"/orders\", () => Results.Ok());\n"),
        ),
        (
            "src/Orders.Api/Endpoints/HealthEndpoints.cs",
            "namespace Orders.Api.Endpoints;\n\npublic static class HealthEndpoints\n{\n    \
             public static void MapHealth(this IEndpointRouteBuilder app)\n    {\n        \
             app.MapGet(\"/live\", () => Results.Ok());\n    }\n}\n",
        ),
    ]);

    assert_eq!(
        details(&found),
        vec!["GET /live".to_string(), "GET /orders".to_string()],
        "warnings were {:?}",
        found.warnings
    );
}

#[test]
fn a_map_group_in_one_project_leaves_another_project_s_endpoints_alone() {
    let found = routes_of(&[
        ("src/Orders.Api/Orders.Api.csproj", WEB_CSPROJ),
        (
            "src/Orders.Api/Program.cs",
            &format!("{PROGRAM_HEAD}var group = app.MapGroup(\"/api/v1\");\n\napp.Run();\n"),
        ),
        ("src/Billing.Api/Billing.Api.csproj", WEB_CSPROJ),
        (
            "src/Billing.Api/Program.cs",
            &format!("{PROGRAM_HEAD}app.MapGet(\"/invoices\", () => Results.Ok());\n"),
        ),
    ]);

    assert_eq!(
        details(&found),
        vec!["GET /invoices".to_string()],
        "one project's group suppressed another project's endpoints, or invented one: {:?}",
        found.warnings
    );
}

#[test]
fn a_commented_out_map_call_is_not_a_route() {
    let found = api_with(&[(
        "src/Orders.Api/Program.cs",
        &format!(
            "{PROGRAM_HEAD}// app.MapGet(\"/removed\", () => Results.Ok());\n\
             app.MapGet(\"/live\", () => Results.Ok());\n\napp.Run();\n"
        ),
    )]);

    assert_eq!(details(&found), vec!["GET /live".to_string()]);
}

#[test]
fn a_map_call_inside_a_block_comment_is_not_a_route() {
    let found = api_with(&[(
        "src/Orders.Api/Program.cs",
        &format!(
            "{PROGRAM_HEAD}/*\napp.MapGet(\"/removed\", () => Results.Ok());\n*/\n\
             app.MapGet(\"/live\", () => Results.Ok());\n\napp.Run();\n"
        ),
    )]);

    assert_eq!(details(&found), vec!["GET /live".to_string()]);
}

#[test]
fn a_map_call_inside_a_string_literal_is_not_a_route() {
    let found = api_with(&[(
        "src/Orders.Api/Program.cs",
        &format!(
            "{PROGRAM_HEAD}var sample = \"app.MapGet(\\\"/fake\\\", handler)\";\n\
             app.MapGet(\"/live\", () => Results.Ok());\n\napp.Run();\n"
        ),
    )]);

    assert_eq!(details(&found), vec!["GET /live".to_string()]);
}

#[test]
fn a_non_literal_minimal_api_pattern_is_not_a_route() {
    let found = api_with(&[(
        "src/Orders.Api/Program.cs",
        &format!("{PROGRAM_HEAD}app.MapGet(Routes.Orders, () => Results.Ok());\n\napp.Run();\n"),
    )]);

    assert!(found.signals.is_empty(), "{:?}", details(&found));
    assert!(
        !warnings_mentioning(&found, "Program.cs").is_empty(),
        "the refusal was silent: {:?}",
        found.warnings
    );
}

#[test]
fn health_checks_hubs_grpc_services_and_razor_pages_are_not_rest_routes() {
    let found = api_with(&[(
        "src/Orders.Api/Program.cs",
        &format!(
            "{PROGRAM_HEAD}app.MapHealthChecks(\"/health\");\n\
             app.MapHub<ChatHub>(\"/chat\");\n\
             app.MapGrpcService<GreeterService>();\n\
             app.MapRazorPages();\n\napp.Run();\n"
        ),
    )]);

    assert!(
        found.signals.is_empty(),
        "a non-REST registration became a route: {:?}",
        details(&found)
    );
    for needle in [
        "MapHealthChecks",
        "MapHub",
        "MapGrpcService",
        "MapRazorPages",
    ] {
        assert!(
            !warnings_mentioning(&found, needle).is_empty(),
            "{needle} was dropped silently: {:?}",
            found.warnings
        );
    }
}

#[test]
fn an_unrecognised_map_extension_method_is_ignored_without_a_warning() {
    let found = api_with(&[(
        "src/Orders.Api/Program.cs",
        &format!("{PROGRAM_HEAD}app.MapCarter();\napp.MapControllers();\n\napp.Run();\n"),
    )]);

    assert!(found.signals.is_empty(), "{:?}", details(&found));
    assert!(
        found.warnings.is_empty(),
        "an unknown Map* extension produced noise: {:?}",
        found.warnings
    );
}

#[test]
fn a_minimal_api_in_a_test_project_is_not_a_route() {
    let found = routes_of(&[
        ("src/Orders.Api/Orders.Api.csproj", WEB_CSPROJ),
        ("tests/Orders.Tests/Orders.Tests.csproj", TEST_CSPROJ),
        (
            "tests/Orders.Tests/Fixture.cs",
            &format!("{PROGRAM_HEAD}app.MapGet(\"/fake\", () => Results.Ok());\n"),
        ),
    ]);

    assert!(found.signals.is_empty(), "{:?}", details(&found));
    assert!(
        !warnings_mentioning(&found, "test project").is_empty(),
        "{:?}",
        found.warnings
    );
}

// ---------------------------------------------------------------------------
// What the signals are
// ---------------------------------------------------------------------------

#[test]
fn every_route_signal_is_medium_and_cites_the_line_it_was_read_from() {
    let found = api_with(&[(
        "src/Orders.Api/Program.cs",
        &format!("{PROGRAM_HEAD}app.MapGet(\"/orders\", () => Results.Ok());\n"),
    )]);

    let signal = found.signals.first().expect("no signal");
    assert_eq!(signal.strength, Strength::Medium);
    assert_eq!(signal.kind, ComponentKind::HttpService);
    assert_eq!(signal.label, "Orders.Api");
    assert_eq!(
        signal.evidence.path.to_string_lossy(),
        "src/Orders.Api/Program.cs"
    );
    assert_eq!(signal.evidence.line, Some(4));
    assert!(
        signal.evidence.excerpt.contains("MapGet"),
        "excerpt was {:?}",
        signal.evidence.excerpt
    );
}

#[test]
fn a_route_signal_enriches_a_service_only_when_a_high_signal_created_it() {
    let found = api_with(&[(
        "src/Orders.Api/Program.cs",
        &format!("{PROGRAM_HEAD}app.MapGet(\"/orders\", () => Results.Ok());\n"),
    )]);
    assert_eq!(found.signals.len(), 1);

    let alone = admit(found.signals.clone());
    assert!(
        alone.components.is_empty(),
        "a route list created a service: {:?}",
        alone.components
    );

    let project = found.signals[0].project_id.clone();
    let with_high = admit(
        std::iter::once(Signal::high(
            ComponentKind::HttpService,
            "Orders.Api",
            &project,
            Evidence::new(
                "src/Orders.Api/Orders.Api.csproj",
                Some(1),
                r#"<Project Sdk="Microsoft.NET.Sdk.Web">"#,
            ),
        ))
        .chain(found.signals.clone())
        .collect(),
    );
    assert_eq!(with_high.components.len(), 1);
    assert_eq!(
        with_high.components[0]
            .details
            .iter()
            .map(|d| d.text.clone())
            .collect::<Vec<_>>(),
        vec!["GET /orders".to_string()]
    );
}

#[test]
fn the_same_workspace_always_produces_the_same_signals_in_the_same_order() {
    let files: Vec<(&str, &str)> = vec![
        ("src/Orders.Api/Orders.Api.csproj", WEB_CSPROJ),
        (
            "src/Orders.Api/Program.cs",
            "var app = WebApplication.Create();\napp.MapGet(\"/b\", () => 1);\napp.MapGet(\"/a\", () => 1);\n",
        ),
    ];
    let first = routes_of(&files);
    let second = routes_of(&files);
    assert_eq!(first, second);
    assert_eq!(
        details(&first),
        vec!["GET /a".to_string(), "GET /b".to_string()]
    );
}
