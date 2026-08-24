//! Tests for [`super::framework`].
//!
//! These are the standing prohibitions for the whole phase, not just for this
//! file. The producers in [`super::dotnet`], [`super::node`] and
//! [`super::routes`] are written independently and cannot be relied upon to
//! each re-derive the same discipline, so the rules are pinned here — against
//! [`admit`], the gate they all pass through — and a producer that tries to
//! break one fails these tests rather than shipping.

use super::framework::*;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn manifest(path: &str, line: u32, excerpt: &str) -> Evidence {
    Evidence::new(path, Some(line), excerpt)
}

fn npgsql(project: &str) -> Signal {
    Signal::high(
        ComponentKind::Database,
        "PostgreSQL",
        project,
        manifest(
            "src/Orders.Api/Orders.Api.csproj",
            12,
            r#"<PackageReference Include="Npgsql" Version="8.0.3" />"#,
        ),
    )
}

/// Everything a caller of [`admit`] can see, flattened into one string.
///
/// Used by the leak tests: asserting a secret is absent from the labels is not
/// enough, because an excerpt, a detail or a warning would carry it into an
/// exported diagram just as effectively. This renders every reachable field,
/// including the derived [`Admitted::edges`] and [`Admitted::warnings`], so a
/// new field added later is covered the moment it is `Debug`.
fn everything_visible(admitted: &Admitted) -> String {
    format!(
        "{admitted:?} {:?} {:?}",
        admitted.edges(),
        admitted.warnings()
    )
}

// ---------------------------------------------------------------------------
// The grading rule
// ---------------------------------------------------------------------------

#[test]
fn a_medium_signal_alone_never_creates_a_component() {
    let admitted = admit(vec![Signal::medium(
        ComponentKind::Database,
        "PostgreSQL",
        "src-Orders.Api-Orders.Api.csproj",
        manifest(
            "src/Orders.Api/appsettings.json",
            4,
            "\"Orders\": <value not read>",
        ),
    )]);

    assert!(
        admitted.components.is_empty(),
        "a supporting signal brought a component into existence: {:?}",
        admitted.components
    );
    assert!(admitted.edges().is_empty());
}

#[test]
fn a_medium_signal_enriches_a_component_a_high_signal_created() {
    let admitted = admit(vec![
        npgsql("orders"),
        Signal::medium(
            ComponentKind::Database,
            "PostgreSQL",
            "orders",
            manifest(
                "src/Orders.Api/appsettings.json",
                4,
                "\"Orders\": <value not read>",
            ),
        )
        .with_detail("connection string key: Orders"),
    ]);

    let [component] = admitted.components.as_slice() else {
        panic!(
            "expected exactly one component, got {:?}",
            admitted.components
        );
    };
    assert_eq!(component.label, "PostgreSQL");
    assert_eq!(component.usages.len(), 1, "the MEDIUM signal added a usage");
    assert_eq!(
        component
            .details
            .iter()
            .map(|d| d.text.as_str())
            .collect::<Vec<_>>(),
        vec!["connection string key: Orders"]
    );
    assert!(
        admitted.discarded.is_empty(),
        "nothing should have been refused: {:?}",
        admitted.discarded
    );
}

#[test]
fn a_discarded_medium_signal_is_counted_not_silent() {
    let admitted = admit(vec![Signal::medium(
        ComponentKind::Cache,
        "Redis",
        "orders",
        manifest(
            "src/Orders.Api/appsettings.json",
            9,
            "\"Cache\": <value not read>",
        ),
    )]);

    assert_eq!(admitted.discarded.len(), 1, "the refusal was not counted");
    assert_eq!(
        admitted.discarded[0].reason,
        DiscardReason::MediumWithoutHigh
    );
    let warnings = admitted.warnings();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].contains("Redis") && warnings[0].contains("src/Orders.Api/appsettings.json"),
        "a warning must name what was seen and where, so a user can check it: {warnings:?}"
    );
}

#[test]
fn two_high_signals_naming_the_same_component_produce_one_node() {
    let admitted = admit(vec![
        npgsql("orders"),
        Signal::high(
            ComponentKind::Database,
            // Different spelling, same technology: the identity key is folded.
            "postgresql",
            "billing",
            manifest(
                "src/Billing.Api/Billing.Api.csproj",
                7,
                r#"<PackageReference Include="Npgsql" Version="8.0.3" />"#,
            ),
        ),
    ]);

    let [component] = admitted.components.as_slice() else {
        panic!(
            "two usages of one technology must be one box, got {:?}",
            admitted.components
        );
    };
    assert_eq!(
        component.usages.len(),
        2,
        "the box is shared, but each usage keeps its own receipt: {:?}",
        component.usages
    );
    assert_eq!(
        component
            .usages
            .iter()
            .map(|u| u.project_id.as_str())
            .collect::<Vec<_>>(),
        vec!["billing", "orders"]
    );
    assert_eq!(admitted.edges().len(), 2, "one usage is one edge");
    // The displayed spelling is the smallest one seen, so it cannot depend on
    // which producer ran first.
    assert_eq!(component.label, "PostgreSQL");
}

#[test]
fn a_component_of_a_different_kind_with_the_same_label_is_a_different_node() {
    let admitted = admit(vec![
        Signal::high(
            ComponentKind::Cache,
            "Redis",
            "orders",
            manifest(
                "a/a.csproj",
                1,
                r#"<PackageReference Include="StackExchange.Redis" />"#,
            ),
        ),
        Signal::high(
            ComponentKind::MessageQueue,
            "Redis",
            "orders",
            manifest(
                "a/a.csproj",
                2,
                r#"<PackageReference Include="Redis.OM" />"#,
            ),
        ),
    ]);

    assert_eq!(
        admitted.components.len(),
        2,
        "identity is kind and label together: {:?}",
        admitted.components
    );
}

#[test]
fn the_order_signals_arrive_in_does_not_change_the_result() {
    let signals = vec![
        npgsql("orders"),
        Signal::high(
            ComponentKind::Cache,
            "Redis",
            "billing",
            manifest(
                "src/Billing.Api/Billing.Api.csproj",
                3,
                r#"<PackageReference Include="StackExchange.Redis" />"#,
            ),
        ),
        Signal::medium(
            ComponentKind::Database,
            "PostgreSQL",
            "billing",
            manifest(
                "src/Billing.Api/appsettings.json",
                4,
                "\"Billing\": <value not read>",
            ),
        )
        .with_detail("connection string key: Billing"),
        Signal::medium(
            ComponentKind::MessageQueue,
            "RabbitMQ",
            "orders",
            manifest(
                "src/Orders.Api/appsettings.json",
                8,
                "\"Bus\": <value not read>",
            ),
        ),
    ];

    let forward = admit(signals.clone());
    let mut reversed = signals.clone();
    reversed.reverse();
    let backward = admit(reversed);

    assert_eq!(forward, backward);
    assert_eq!(forward.edges(), backward.edges());
    assert_eq!(forward.warnings(), backward.warnings());

    // …and every rotation, since three producers can interleave arbitrarily.
    for rotation in 1..signals.len() {
        let mut rotated = signals.clone();
        rotated.rotate_left(rotation);
        assert_eq!(admit(rotated), forward, "rotation by {rotation} differed");
    }
}

#[test]
fn an_empty_signal_set_produces_nothing_and_no_warning() {
    let admitted = admit(Vec::new());

    assert!(admitted.components.is_empty());
    assert!(admitted.discarded.is_empty());
    assert!(admitted.edges().is_empty());
    assert!(
        admitted.warnings().is_empty(),
        "a workspace with nothing to say must not produce noise: {:?}",
        admitted.warnings()
    );
}

// ---------------------------------------------------------------------------
// The standing prohibitions
// ---------------------------------------------------------------------------

/// The most dangerous thing this phase could do: a diagram is exported and
/// shared, so a credential that reaches a node label leaves the machine.
///
/// The fixture is a real `appsettings.json` written to disk and read back, so
/// the test is over the text a producer would actually be holding rather than
/// over a string invented to pass.
#[test]
fn a_connection_string_value_never_reaches_the_graph() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("appsettings.json");
    std::fs::write(
        &path,
        r#"{
  "ConnectionStrings": {
    "Orders": "Host=db.internal;Port=5432;Database=orders_prod;Username=svc_orders;Password=hunter2;SslMode=Require",
    "Cache": "redis.internal:6379,password=s3cr3t,ssl=True"
  },
  "Logging": { "LogLevel": { "Default": "Information" } }
}"#,
    )
    .unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    let orders_line = text
        .lines()
        .find(|line| line.contains("\"Orders\""))
        .unwrap()
        .trim()
        .to_string();
    let orders_value = orders_line
        .split_once(": ")
        .unwrap()
        .1
        .trim_end_matches(',')
        .trim_matches('"')
        .to_string();

    let admitted = admit(vec![
        // A careless producer quoting the line verbatim.
        Signal::high(
            ComponentKind::Database,
            "Orders",
            "orders",
            manifest("src/Orders.Api/appsettings.json", 3, &orders_line),
        ),
        // A worse one using the value itself as the label.
        Signal::high(
            ComponentKind::Database,
            &orders_value,
            "orders",
            manifest("src/Orders.Api/appsettings.json", 3, "declared"),
        ),
        // One smuggling it through the enrichment field.
        Signal::medium(
            ComponentKind::Database,
            "Orders",
            "orders",
            manifest(
                "src/Orders.Api/appsettings.json",
                3,
                "\"Orders\": <value not read>",
            ),
        )
        .with_detail(orders_value.clone()),
        // And the sanctioned form: the key is the author's own label, the
        // value is never read.
        Signal::high(
            ComponentKind::Database,
            "Orders",
            "orders",
            Evidence::elided_value("src/Orders.Api/appsettings.json", Some(3), "\"Orders\""),
        ),
    ]);

    let visible = everything_visible(&admitted);
    for secret in [
        "hunter2",
        "db.internal",
        "5432",
        "orders_prod",
        "svc_orders",
        "SslMode",
        "Require",
    ] {
        assert!(
            !visible.contains(secret),
            "'{secret}' escaped into the graph: {visible}"
        );
    }

    // The key alone is a permissible label, so the honest signal survives.
    let [component] = admitted.components.as_slice() else {
        panic!("the sanctioned signal should still produce a box: {admitted:?}");
    };
    assert_eq!(component.label, "Orders");

    assert_eq!(
        admitted.discarded.len(),
        3,
        "every refusal must be counted: {:?}",
        admitted.discarded
    );
    assert!(admitted
        .discarded
        .iter()
        .all(|d| d.reason == DiscardReason::SecretValue));
    assert!(
        admitted.discarded.iter().all(|d| d.label.is_none()),
        "a refused secret must not be echoed back in the refusal"
    );
}

#[test]
fn a_bare_host_and_port_is_refused_as_a_label_even_without_a_key() {
    let admitted = admit(vec![Signal::high(
        ComponentKind::Cache,
        "redis.internal:6379",
        "orders",
        manifest("src/Orders.Api/appsettings.json", 4, "declared"),
    )]);

    assert!(admitted.components.is_empty());
    assert_eq!(
        admitted.discarded[0].reason,
        DiscardReason::LabelLooksLikeAValue
    );
}

#[test]
fn a_label_refused_for_being_a_value_is_never_echoed_by_the_warning_that_refuses_it() {
    // The reason that exists precisely because the label *is* a value. Echoing
    // it puts the thing the gate just refused into `ArchGraph::warnings`, and
    // from there into the exported mermaid — the exact leak the refusal was
    // supposed to prevent, with the tool's own explanation attached.
    let admitted = admit(vec![
        Signal::high(
            ComponentKind::Cache,
            "https://cacheuser:cachepw88@cache.corp.internal:6380",
            "orders",
            manifest("src/Orders.Api/appsettings.json", 4, "declared"),
        ),
        Signal::high(
            ComponentKind::Cache,
            "redis-prod.corp.internal:6380",
            "orders",
            manifest("src/Orders.Api/appsettings.json", 5, "declared"),
        ),
    ]);

    assert!(admitted
        .discarded
        .iter()
        .all(|d| d.reason == DiscardReason::LabelLooksLikeAValue));
    assert!(
        admitted.discarded.iter().all(|d| d.label.is_none()),
        "a label refused for being a value must not be repeated: {:?}",
        admitted.discarded
    );

    let visible = everything_visible(&admitted);
    for secret in [
        "cachepw88",
        "cacheuser",
        "cache.corp.internal",
        "redis-prod.corp.internal",
        "6380",
    ] {
        assert!(
            !visible.contains(secret),
            "{secret:?} survived the refusal that named it:\n{visible}"
        );
    }
    assert_eq!(
        admitted.warnings().len(),
        2,
        "the refusals still have to be reported, one per line refused: {:?}",
        admitted.warnings()
    );
}

#[test]
fn a_value_shaped_detail_is_refused_the_same_way_a_value_shaped_label_is() {
    // The gate screened `label` for value shapes and left `detail` to the
    // producers' own discipline, so a producer that interpolated a url into
    // its enrichment text walked straight through. `detail` is printed
    // verbatim by `components::cross_project_notes`, so it is exactly as
    // exported as the label is.
    let admitted = admit(vec![
        Signal::high(
            ComponentKind::HttpService,
            "Orders.Api",
            "orders-api",
            manifest("src/Orders.Api/Orders.Api.csproj", 1, "<Project Sdk=\"x\">"),
        ),
        Signal::medium(
            ComponentKind::HttpService,
            "Orders.Api",
            "orders-web",
            manifest(
                "src/Orders.Web/Properties/launchSettings.json",
                5,
                "declared",
            ),
        )
        .with_detail("https://launchuser:launchpw99@launch-host.corp.internal:9443"),
    ]);

    assert_eq!(
        admitted.components.len(),
        1,
        "the HIGH signal still earns its box: {:?}",
        admitted.components
    );
    assert!(
        admitted.components[0].details.is_empty(),
        "a value-shaped detail must not be attached: {:?}",
        admitted.components[0].details
    );
    assert_eq!(
        admitted.discarded.len(),
        1,
        "and the refusal must be counted: {:?}",
        admitted.discarded
    );

    let visible = everything_visible(&admitted);
    for secret in [
        "launchpw99",
        "launchuser",
        "launch-host.corp.internal",
        "9443",
    ] {
        assert!(
            !visible.contains(secret),
            "{secret:?} reached a caller of admit:\n{visible}"
        );
    }
}

#[test]
fn no_component_is_created_from_a_name_similarity() {
    // Two projects sharing a prefix, each declaring its own component. A shared
    // prefix is a naming convention; nothing here may merge them, relate them,
    // or invent a third box above them.
    let admitted = admit(vec![
        Signal::high(
            ComponentKind::HttpService,
            "Orders.Api",
            "orders-api",
            manifest(
                "src/Orders.Api/Orders.Api.csproj",
                1,
                "<OutputType>Exe</OutputType>",
            ),
        ),
        Signal::high(
            ComponentKind::HttpService,
            "Orders.Worker",
            "orders-worker",
            manifest(
                "src/Orders.Worker/Orders.Worker.csproj",
                1,
                "<OutputType>Exe</OutputType>",
            ),
        ),
    ]);

    assert_eq!(
        admitted.components.len(),
        2,
        "a shared prefix must not merge two components: {:?}",
        admitted.components
    );
    let edges = admitted.edges();
    assert_eq!(edges.len(), 2, "each project uses only its own: {edges:?}");
    assert!(
        edges
            .iter()
            .all(|e| e.component_id.contains(&e.project_id.replace('-', "."))),
        "an edge appeared between two similarly named projects: {edges:?}"
    );
}

#[test]
fn nothing_is_inferred_from_a_using_line() {
    let admitted = admit(vec![
        Signal::high(
            ComponentKind::Database,
            "PostgreSQL",
            "orders",
            manifest("src/Orders.Api/Db.cs", 1, "using Npgsql;"),
        ),
        Signal::medium(
            ComponentKind::Database,
            "PostgreSQL",
            "orders",
            manifest("src/Orders.Api/Db.cs", 2, "global using Npgsql;"),
        ),
    ]);

    assert!(
        admitted.components.is_empty(),
        "an import says a namespace resolves, not that the program uses it: {:?}",
        admitted.components
    );
    assert!(admitted
        .discarded
        .iter()
        .all(|d| d.reason == DiscardReason::NotADeclaration));
    assert_eq!(admitted.discarded.len(), 2);
}

#[test]
fn nothing_is_inferred_from_a_comment() {
    let admitted = admit(vec![
        Signal::high(
            ComponentKind::Cache,
            "Redis",
            "orders",
            manifest(
                "src/Orders.Api/Startup.cs",
                9,
                "// services.AddStackExchangeRedisCache();",
            ),
        ),
        Signal::high(
            ComponentKind::Cache,
            "Redis",
            "orders",
            manifest(
                "src/Orders.Api/Orders.Api.csproj",
                9,
                "<!-- <PackageReference Include=\"StackExchange.Redis\" /> -->",
            ),
        ),
    ]);

    assert!(
        admitted.components.is_empty(),
        "a commented-out registration looks identical to a live one and means the opposite"
    );
    assert_eq!(admitted.discarded.len(), 2);
    assert!(admitted
        .discarded
        .iter()
        .all(|d| d.reason == DiscardReason::NotADeclaration));
}

#[test]
fn a_high_signal_citing_a_source_file_is_refused_rather_than_demoted() {
    let admitted = admit(vec![Signal::high(
        ComponentKind::HttpService,
        "billing-api",
        "orders",
        manifest(
            "src/Orders.Api/Program.cs",
            22,
            "builder.Services.AddHttpClient(\"billing\");",
        ),
    )]);

    assert!(admitted.components.is_empty());
    assert_eq!(
        admitted.discarded[0].reason,
        DiscardReason::UnverifiableEvidence,
        "demoting it to MEDIUM would hide the producer's mistake instead of reporting it"
    );
}

#[test]
fn a_medium_signal_may_cite_a_source_file_because_it_only_enriches() {
    let admitted = admit(vec![
        Signal::high(
            ComponentKind::HttpService,
            "Orders.Api",
            "orders",
            manifest(
                "src/Orders.Api/Orders.Api.csproj",
                1,
                "<OutputType>Exe</OutputType>",
            ),
        ),
        Signal::medium(
            ComponentKind::HttpService,
            "Orders.Api",
            "orders",
            manifest(
                "src/Orders.Api/OrdersController.cs",
                14,
                "[Route(\"api/orders\")]",
            ),
        )
        .with_detail("GET api/orders"),
    ]);

    let [component] = admitted.components.as_slice() else {
        panic!("expected one component: {admitted:?}");
    };
    assert_eq!(
        component
            .details
            .iter()
            .map(|d| d.text.as_str())
            .collect::<Vec<_>>(),
        vec!["GET api/orders"]
    );
    assert!(admitted.discarded.is_empty());
}

#[test]
fn a_signal_without_a_label_or_a_project_is_refused_and_counted() {
    let admitted = admit(vec![
        Signal::high(
            ComponentKind::Database,
            "   ",
            "orders",
            manifest("a/a.csproj", 1, "declared"),
        ),
        Signal::high(
            ComponentKind::Database,
            "PostgreSQL",
            "",
            manifest("a/a.csproj", 1, "declared"),
        ),
        Signal::high(
            ComponentKind::Database,
            "PostgreSQL",
            "orders",
            Evidence::new("", None, "declared"),
        ),
    ]);

    assert!(admitted.components.is_empty());
    assert_eq!(admitted.discarded.len(), 3);
    assert!(admitted
        .discarded
        .iter()
        .all(|d| d.reason == DiscardReason::Incomplete));
}

#[test]
fn every_usage_becomes_exactly_one_edge_and_repeats_collapse() {
    let admitted = admit(vec![npgsql("orders"), npgsql("orders"), npgsql("billing")]);

    let edges = admitted.edges();
    assert_eq!(
        edges.len(),
        2,
        "the same declaration seen twice is one edge: {edges:?}"
    );
    assert!(edges
        .iter()
        .all(|e| e.component_id == "component:database:postgresql"));
}

#[test]
fn a_component_id_cannot_collide_with_a_project_or_solution_id() {
    let admitted = admit(vec![npgsql("orders")]);

    let id = &admitted.components[0].id;
    assert!(id.starts_with("component:"));
    assert!(
        !id.starts_with("solution:") && !id.starts_with("external:") && !id.starts_with("project:")
    );
}

#[test]
fn a_word_containing_a_connection_string_key_is_not_mistaken_for_one() {
    // `report` contains `port`, `passwordless` contains `password`, and neither
    // is an assignment. Over-refusing is safe but silently drops real edges, so
    // the boundary is pinned in both directions.
    let admitted = admit(vec![Signal::high(
        ComponentKind::Database,
        "Reporting",
        "orders",
        manifest(
            "src/Reporting/Reporting.csproj",
            4,
            r#"<PackageReference Include="Npgsql" /> <!-- passwordless auth -->"#.trim_start(),
        ),
    )]);

    assert_eq!(
        admitted.components.len(),
        1,
        "a false positive here silently deletes a real edge: {:?}",
        admitted.discarded
    );
}

// ---------------------------------------------------------------------------
// Service calls
// ---------------------------------------------------------------------------

#[test]
fn a_high_call_citing_a_declaration_file_is_admitted_as_a_service_call() {
    // A call signal names two projects rather than a component, so it must
    // never build a box; it survives as an `AdmittedCall` and nothing else.
    // Its evidence cites the callee's `launchSettings.json`, a declaration
    // file, so the HIGH screen passes.
    let admitted = admit(vec![Signal::call(
        "orders-web",
        "orders-api",
        Evidence::elided_value(
            "src/Orders.Api/Properties/launchSettings.json",
            Some(4),
            "applicationUrl",
        ),
    )]);

    assert!(
        admitted.components.is_empty(),
        "a call must never bring a component into existence: {:?}",
        admitted.components
    );
    assert_eq!(admitted.service_calls().len(), 1, "{:?}", admitted);
    let call = &admitted.service_calls()[0];
    assert_eq!(call.from_project, "orders-web");
    assert_eq!(call.to_project, "orders-api");
}

#[test]
fn a_call_citing_a_source_file_is_refused_as_unverifiable() {
    // Anchoring the call on a `.cs` line is exactly the lie the gate exists to
    // catch: HIGH means "the author wrote this down", and a source line is not
    // that. It is refused, counted, and creates nothing.
    let admitted = admit(vec![Signal::call(
        "orders-web",
        "orders-api",
        Evidence::new(
            "src/Orders.Web/Program.cs",
            Some(3),
            "c.BaseAddress = new Uri(\"http://localhost:5101\");",
        ),
    )]);

    assert!(admitted.service_calls().is_empty(), "{:?}", admitted);
    assert!(
        admitted
            .discarded
            .iter()
            .any(|d| d.reason == DiscardReason::UnverifiableEvidence),
        "the .cs-anchored call was not refused for the right reason: {:?}",
        admitted.discarded
    );
}
