//! Tests for [`super::mermaid`].
//!
//! Rendering is pure — it reads a graph and returns a string — so these tests
//! build [`ArchGraph`] values directly rather than scanning a workspace. That
//! is the opposite of `graph_tests.rs`, deliberately: the graph's job is to
//! line up with what is on disk, and the renderer's job is to draw whatever it
//! is handed, including graphs no scan would ever produce.

use super::graph::*;
use super::mermaid::*;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn node(id: &str, label: &str, kind: ArchKind) -> ArchNode {
    ArchNode {
        id: id.into(),
        label: label.into(),
        kind,
        project_id: match kind {
            ArchKind::Project => Some(id.into()),
            _ => None,
        },
        path: None,
        ecosystem: None,
    }
}

fn edge(from: &str, to: &str, kind: EdgeKind) -> ArchEdge {
    ArchEdge {
        from: from.into(),
        to: to.into(),
        kind,
        label: None,
    }
}

fn graph(nodes: Vec<ArchNode>, edges: Vec<ArchEdge>) -> ArchGraph {
    ArchGraph {
        nodes,
        edges,
        warnings: Vec::new(),
        derivation: Derivation::Derived {
            scanner: SCANNER_VERSION,
        },
    }
}

/// Every line of `source`, with the leading indentation removed, so a test can
/// assert on a statement without also asserting on its nesting depth.
fn statements(source: &str) -> Vec<&str> {
    source.lines().map(str::trim).collect()
}

/// `source` with the legend removed.
///
/// The legend is a fixed key appended after everything the graph produced, so
/// a test about *the graph's* boxes and arrows would otherwise have to count
/// the key's as well and would stop saying what it means. The legend is pinned
/// exactly by its own tests instead.
fn without_legend(source: &str) -> String {
    match source.find("    subgraph legend[") {
        Some(at) => source[..at].to_string(),
        None => source.to_string(),
    }
}

/// The legend alone, so a test can assert on it without the diagram.
fn legend_of(source: &str) -> String {
    match source.find("    subgraph legend[") {
        Some(at) => source[at..].to_string(),
        None => String::new(),
    }
}

/// Every quoted string on a line, which for a legend line is its label.
///
/// The renderer escapes the only character that could end a quoted span early,
/// so pairing the quotes off in order is exact on this module's own output.
fn quoted(line: &str) -> Vec<String> {
    line.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

#[test]
fn a_graph_renders_as_a_flowchart_with_stable_ids() {
    let g = graph(
        vec![
            node("src-App-App.csproj", "App", ArchKind::Project),
            node("src-Lib-Lib.csproj", "Lib", ArchKind::Project),
        ],
        vec![edge(
            "src-App-App.csproj",
            "src-Lib-Lib.csproj",
            EdgeKind::ProjectReference,
        )],
    );

    assert_eq!(
        without_legend(&render(&g)),
        // The version is interpolated rather than spelled out: the fixture
        // stamps `SCANNER_VERSION` into the graph, so a literal here pins the
        // two together only until the derivation rules next change, and then
        // fails for a reason that has nothing to do with rendering.
        format!(
            "%% Derived by code-basics from the files on disk (scanner version \
             {SCANNER_VERSION}).\n\
             flowchart LR\n    \
             nsrc_2d_App_2d_App_2e_csproj[\"App\"]\n    \
             nsrc_2d_Lib_2d_Lib_2e_csproj[\"Lib\"]\n    \
             nsrc_2d_App_2d_App_2e_csproj --> nsrc_2d_Lib_2d_Lib_2e_csproj\n"
        )
    );
    assert_eq!(
        legend_of(&render(&g)),
        "    subgraph legend[\"Legend\"]\n        \
         legend_project[\"project in this workspace\"]\n        \
         legend_from[\"A\"] -->|\"project reference\"| legend_to[\"B\"]\n    \
         end\n",
        "the legend is part of the file and is pinned like the rest of it"
    );

    assert_eq!(render(&g), render(&g), "rendering is deterministic");
}

#[test]
fn two_ids_that_differ_only_in_a_character_mermaid_forbids_never_collapse() {
    // `Project::id` is a path with its separators replaced, so it is full of
    // characters a Mermaid identifier cannot contain. Stripping them would let
    // two genuinely different projects become one box — a silent, invisible
    // wrong answer, which is the worst outcome this module can produce.
    let ids = [
        "a-b", "a.b", "a_b", "a/b", "a b", "ab", "a:b", "a#b", "a@b", "a\\b", "a-b-", "-a-b",
    ];
    let g = graph(
        ids.iter()
            .map(|id| node(id, "same label", ArchKind::Project))
            .collect(),
        Vec::new(),
    );

    let source = without_legend(&render(&g));
    let declared: Vec<&str> = statements(&source)
        .into_iter()
        .filter(|l| l.contains("[\""))
        .collect();

    assert_eq!(
        declared.len(),
        ids.len(),
        "every id must survive as its own node:\n{source}"
    );
    let unique: std::collections::BTreeSet<&str> = declared.iter().copied().collect();
    assert_eq!(
        unique.len(),
        ids.len(),
        "two ids collapsed onto one Mermaid identifier:\n{source}"
    );
}

#[test]
fn a_label_containing_quotes_or_newlines_cannot_break_the_diagram() {
    // These are all real: `Acme.Web (Legacy)`, `R&D`, `Foo<T>`, names with
    // typographic quotes, and labels a user pasted a newline into.
    let awkward = [
        "say \"hello\"",
        "two\nlines",
        "tab\there",
        "Acme.Web (Legacy)",
        "R&D",
        "Foo<T> => Bar",
        "100% [done]",
        "a|b",
        "{braces}",
        "naïve — Ω 日本語",
        "%%{init: {\"securityLevel\": \"loose\"}}%%",
        "<script>alert(1)</script>",
        "onclick=alert(1)",
        "javascript:alert(1)",
        "",
    ];

    for label in awkward {
        let g = graph(vec![node("only", label, ArchKind::Project)], Vec::new());
        let source = render(&g);

        assert!(
            validate(&source).is_ok(),
            "label {label:?} produced source that does not validate: \
             {:?}\n{source}",
            validate(&source)
        );
        assert_eq!(
            source.lines().filter(|l| l.contains("nonly")).count(),
            1,
            "label {label:?} must stay on one line:\n{source}"
        );
    }
}

#[test]
fn an_empty_graph_renders_a_note_rather_than_an_empty_flowchart() {
    // `flowchart LR` with nothing under it renders as a blank rectangle, which
    // a reader takes for a broken diagram rather than for "nothing to show".
    let source = render(&graph(Vec::new(), Vec::new()));

    assert!(validate(&source).is_ok(), "{source}");
    assert!(
        source.to_lowercase().contains("no ") || source.to_lowercase().contains("nothing"),
        "an empty graph must say so in words:\n{source}"
    );
    assert!(
        statements(&source).iter().any(|l| l.contains("[\"")),
        "the note must be a node, not a bare flowchart header:\n{source}"
    );
}

#[test]
fn the_derivation_line_is_always_present() {
    for (derivation, expected) in [
        (Derivation::Derived { scanner: 1 }, "scanner version 1"),
        (
            Derivation::Inferred {
                agent: "claude".into(),
            },
            "claude",
        ),
        (Derivation::User, "hand"),
    ] {
        let mut g = graph(vec![node("a", "A", ArchKind::Project)], Vec::new());
        g.derivation = derivation.clone();

        let source = render(&g);
        let first = source.lines().next().unwrap();

        assert!(
            first.starts_with("%%"),
            "{derivation:?} must be stated in a comment on the first line: {first}"
        );
        assert!(
            first.contains(expected),
            "{derivation:?} must be named: {first}"
        );
        assert!(validate(&source).is_ok(), "{source}");
    }
}

#[test]
fn a_solution_folder_becomes_a_subgraph_and_every_subgraph_is_closed() {
    let g = graph(
        vec![
            node("solution:Repo.sln", "Repo", ArchKind::Solution),
            node("solution:Repo.sln#src", "src", ArchKind::SolutionFolder),
            node("src-App-App.csproj", "App", ArchKind::Project),
        ],
        vec![
            edge(
                "solution:Repo.sln",
                "solution:Repo.sln#src",
                EdgeKind::Contains,
            ),
            edge(
                "solution:Repo.sln#src",
                "src-App-App.csproj",
                EdgeKind::Contains,
            ),
        ],
    );

    let source = render(&g);
    let drawn = without_legend(&source);
    let lines = statements(&drawn);

    assert_eq!(
        lines.iter().filter(|l| l.starts_with("subgraph ")).count(),
        2,
        "both containers become subgraphs:\n{source}"
    );
    assert_eq!(
        lines.iter().filter(|l| **l == "end").count(),
        2,
        "every subgraph is closed:\n{source}"
    );
    assert!(
        !lines.iter().any(|l| l.contains("--o")),
        "containment realised as nesting is not also drawn as an arrow:\n{source}"
    );
    assert!(validate(&source).is_ok(), "{source}");
}

#[test]
fn a_container_holding_nothing_is_a_box_rather_than_an_empty_subgraph() {
    let g = graph(
        vec![node("solution:Repo.sln", "Repo", ArchKind::Solution)],
        Vec::new(),
    );

    let source = render(&g);

    assert!(
        !without_legend(&source).contains("subgraph"),
        "an empty subgraph draws as a titled void; a box is honest:\n{source}"
    );
    assert!(source.contains("\"Repo\""), "{source}");
    assert!(validate(&source).is_ok(), "{source}");
}

#[test]
fn a_project_claimed_by_two_containers_is_nested_once_and_the_second_claim_is_drawn() {
    // Mermaid can only nest a node in one subgraph. Dropping the second
    // containment would be a diagram that looks complete and is missing a
    // fact, so it is drawn as an arrow instead.
    let g = graph(
        vec![
            node("solution:A.sln", "A", ArchKind::Solution),
            node("solution:B.sln", "B", ArchKind::Solution),
            node("p", "P", ArchKind::Project),
        ],
        vec![
            edge("solution:A.sln", "p", EdgeKind::Contains),
            edge("solution:B.sln", "p", EdgeKind::Contains),
        ],
    );

    let source = render(&g);
    let drawn = without_legend(&source);
    let lines = statements(&drawn);

    assert_eq!(
        lines.iter().filter(|l| **l == "np[\"P\"]").count(),
        1,
        "the project is declared exactly once:\n{source}"
    );
    assert_eq!(
        lines.iter().filter(|l| l.starts_with("subgraph ")).count(),
        1,
        "only the container that nests it is a subgraph:\n{source}"
    );
    assert!(
        lines.iter().any(|l| l.contains("--o np")),
        "the second containment is drawn rather than lost:\n{source}"
    );
    assert!(validate(&source).is_ok(), "{source}");
}

#[test]
fn graph_warnings_are_rendered_as_comments_a_reader_of_the_file_can_see() {
    let mut g = graph(vec![node("a", "A", ArchKind::Project)], Vec::new());
    g.warnings = vec![
        r"App: project reference '..\Lbi\Lib.csproj' matches no project".into(),
        "a warning that %%{ looks like a directive".into(),
    ];

    let source = render(&g);

    assert!(
        source.contains(r"..\Lbi\Lib.csproj"),
        "the warning must reach the file verbatim:\n{source}"
    );
    assert!(
        validate(&source).is_ok(),
        "a warning must never be able to turn into a directive: {:?}\n{source}",
        validate(&source)
    );
}

#[test]
fn an_edge_naming_a_node_that_does_not_exist_is_dropped_and_reported() {
    let g = graph(
        vec![node("a", "A", ArchKind::Project)],
        vec![edge("a", "ghost", EdgeKind::ProjectReference)],
    );

    let source = render(&g);

    assert!(
        !statements(&without_legend(&source))
            .iter()
            .any(|l| l.contains("-->")),
        "an arrow to a box that does not exist is a claim about nothing:\n{source}"
    );
    assert!(
        source.contains("%%") && source.contains("ghost"),
        "the dropped edge must be visible in the file:\n{source}"
    );
    assert!(validate(&source).is_ok(), "{source}");
}

#[test]
fn each_edge_kind_is_drawn_with_its_own_arrow() {
    let g = graph(
        vec![
            node("a", "A", ArchKind::Project),
            node("b", "B", ArchKind::Project),
            node("c", "C", ArchKind::Project),
            node("d", "D", ArchKind::Project),
        ],
        vec![
            edge("a", "b", EdgeKind::ProjectReference),
            edge("a", "c", EdgeKind::PackageDependency),
            edge("a", "d", EdgeKind::Contains),
        ],
    );

    let source = render(&g);
    let drawn = without_legend(&source);
    let lines = statements(&drawn);

    assert!(lines.contains(&"na --> nb"), "{lines:?}");
    assert!(lines.contains(&"na -.-> nc"), "{lines:?}");
    assert!(lines.contains(&"na --o nd"), "{lines:?}");
}

#[test]
fn a_data_access_edge_is_drawn_with_an_arrow_no_other_edge_kind_uses() {
    // Four kinds, four arrows. If two shared one, the legend would explain a
    // symbol that means two different things — which is worse than no legend,
    // because the reader would believe it.
    let g = graph(
        vec![
            node("a", "A", ArchKind::Service),
            node("b", "B", ArchKind::Project),
            node("c", "C", ArchKind::Project),
            node("d", "D", ArchKind::Project),
            node(
                "component:database:postgresql",
                "PostgreSQL",
                ArchKind::DataStore,
            ),
        ],
        vec![
            edge("a", "b", EdgeKind::ProjectReference),
            edge("a", "c", EdgeKind::PackageDependency),
            edge("a", "d", EdgeKind::Contains),
            edge("a", "component:database:postgresql", EdgeKind::DataAccess),
        ],
    );

    let source = render(&g);
    let drawn = without_legend(&source);
    let lines = statements(&drawn);
    let store = "na ==> ncomponent_3a_database_3a_postgresql";

    assert!(lines.contains(&store), "{lines:?}");
    assert_eq!(
        lines.iter().filter(|l| l.contains("==>")).count(),
        1,
        "the data-access arrow must not be spelled the same as another kind: {lines:?}"
    );
    assert_eq!(validate(&source), Ok(()), "{source}");
}

#[test]
fn a_data_store_is_drawn_as_a_cylinder_and_a_service_is_not_drawn_as_a_plain_project() {
    // The shape is the only thing separating a box a reader can open from a
    // box that has no source behind it at all. Every kind therefore gets its
    // own, and the assertion is that all five differ rather than that any one
    // of them is a particular string.
    let g = graph(
        vec![
            node("p", "P", ArchKind::Project),
            node("s", "S", ArchKind::Service),
            node("component:cache:redis", "Redis", ArchKind::DataStore),
            node("external:../X/X.csproj", "X", ArchKind::External),
            node("solution:R.sln", "R", ArchKind::Solution),
        ],
        Vec::new(),
    );

    let source = render(&g);
    let body = without_legend(&source);
    let drawn = statements(&body);

    assert!(drawn.contains(&"np[\"P\"]"), "{drawn:?}");
    assert!(drawn.contains(&"ns(\"S\")"), "{drawn:?}");
    assert!(
        drawn.contains(&"ncomponent_3a_cache_3a_redis[(\"Redis\")]"),
        "a data store must be a cylinder: {drawn:?}"
    );

    // Node statements only: the derivation comment and the `flowchart LR`
    // header are lines too, and neither declares a box. `mermaid_id` prefixes
    // every node identifier with `n`, which is what makes that testable.
    let shapes: std::collections::BTreeSet<&str> = drawn
        .iter()
        .filter(|line| line.starts_with('n'))
        // Everything between the identifier and the label's opening quote is
        // the bracket run, which is the whole of what distinguishes a shape.
        .filter_map(|line| {
            let open = line.find(['[', '(', '{'])?;
            let quote = line.find('"')?;
            Some(&line[open..quote])
        })
        .collect();
    assert_eq!(
        shapes.len(),
        5,
        "five kinds were drawn and only {} shapes came out: {drawn:?}",
        shapes.len()
    );
    assert_eq!(validate(&source), Ok(()), "{source}");
}

#[test]
fn a_component_map_legend_explains_the_cylinder_and_the_arrow_into_it() {
    // A `.mmd` read on a pull request has nothing but the file. A cylinder
    // with no key is a shape; the row that names it is what turns it into
    // "this is not something in your repository".
    let g = graph(
        vec![
            node("s", "S", ArchKind::Service),
            node(
                "component:database:postgresql",
                "PostgreSQL",
                ArchKind::DataStore,
            ),
        ],
        vec![edge(
            "s",
            "component:database:postgresql",
            EdgeKind::DataAccess,
        )],
    );

    let legend = legend_of(&render(&g));

    assert!(legend.contains("legend_service(\""), "{legend}");
    assert!(legend.contains("legend_data_store[(\""), "{legend}");
    assert!(legend.contains("==>|\""), "{legend}");
    assert!(
        legend.to_lowercase().contains("serves http"),
        "the legend must say in words what the rounded box is:\n{legend}"
    );
    assert!(
        legend.to_lowercase().contains("declares"),
        "the legend must state the weaker claim the arrow actually makes, not \
         'uses':\n{legend}"
    );
    for absent in ["legend_project[", "legend_external", "-.->", "--o"] {
        assert!(
            !legend.contains(absent),
            "{absent:?} is not in this diagram and must not be in its legend:\n{legend}"
        );
    }
}

#[test]
fn an_edge_label_is_quoted_so_it_cannot_end_the_statement() {
    let mut g = graph(
        vec![
            node("a", "A", ArchKind::Project),
            node("b", "B", ArchKind::Project),
        ],
        vec![edge("a", "b", EdgeKind::ProjectReference)],
    );
    g.edges[0].label = Some("reads \"config\"".into());
    g.derivation = Derivation::User;

    let source = render(&g);

    assert!(source.contains("|\""), "the label is quoted:\n{source}");
    assert!(validate(&source).is_ok(), "{source}");
}

#[test]
fn a_containment_cycle_is_broken_rather_than_followed_forever() {
    // Nothing derived can produce this, but an agent-authored or user-edited
    // graph can, and a renderer that recurses into it never returns.
    let g = graph(
        vec![
            node("s1", "S1", ArchKind::Solution),
            node("s2", "S2", ArchKind::Solution),
        ],
        vec![
            edge("s1", "s2", EdgeKind::Contains),
            edge("s2", "s1", EdgeKind::Contains),
        ],
    );

    let source = render(&g);

    assert!(validate(&source).is_ok(), "{source}");
    assert!(
        source.contains("\"S1\"") && source.contains("\"S2\""),
        "{source}"
    );
}

// ---------------------------------------------------------------------------
// The legend
// ---------------------------------------------------------------------------

#[test]
fn a_diagram_using_every_symbol_carries_a_legend_that_explains_all_of_them() {
    // An exported `.mmd` is read far from this app, with no tooltip and no
    // sidebar to say that dotted means a package dependency. Without a key the
    // reader either guesses or treats every arrow as the same relationship,
    // and "A depends on B" and "A ships with B" are not the same claim.
    let g = graph(
        vec![
            node("solution:Repo.sln", "Repo", ArchKind::Solution),
            node("solution:Empty.sln", "Empty", ArchKind::Solution),
            node("app", "App", ArchKind::Project),
            node("lib", "Lib", ArchKind::Project),
            node(
                "external:../Shared/Shared.csproj",
                "Shared",
                ArchKind::External,
            ),
        ],
        vec![
            edge("solution:Repo.sln", "app", EdgeKind::Contains),
            edge("solution:Empty.sln", "app", EdgeKind::Contains),
            edge("app", "lib", EdgeKind::ProjectReference),
            edge(
                "app",
                "external:../Shared/Shared.csproj",
                EdgeKind::PackageDependency,
            ),
        ],
    );

    let source = render(&g);
    let legend = legend_of(&source);

    assert!(!legend.is_empty(), "no legend was rendered:\n{source}");
    for (symbol, meaning) in [
        ("legend_project[\"", "a plain box"),
        ("legend_external([\"", "the stadium shape"),
        ("legend_container[[\"", "the container shape"),
        ("-->|\"", "a solid arrow"),
        ("-.->|\"", "a dotted arrow"),
        ("--o|\"", "a containment arrow"),
    ] {
        assert!(
            legend.contains(symbol),
            "{meaning} is used by this diagram but {symbol:?} is not in the legend:\n{legend}"
        );
    }
    // A box drawn around boxes is the one symbol left out on purpose: it is
    // the only one the picture decodes by itself, and demonstrating it costs a
    // whole nested container inside the key. See `write_legend`.
    assert!(
        !legend.contains("legend_nesting"),
        "the key must not spend a nested subgraph on a convention the drawing \
         already carries:\n{legend}"
    );
    assert!(
        legend.to_lowercase().contains("package"),
        "the legend must say what a dotted arrow means in words:\n{legend}"
    );
    assert_eq!(
        validate(&source),
        Ok(()),
        "the legend must survive its own validator:\n{source}"
    );
}

#[test]
fn the_legend_explains_only_the_symbols_the_diagram_actually_uses() {
    // A key listing shapes that are not in the picture sends the reader
    // looking for them. Every entry below is absent because the graph has no
    // external node, no package dependency and no container.
    let g = graph(
        vec![
            node("app", "App", ArchKind::Project),
            node("lib", "Lib", ArchKind::Project),
        ],
        vec![edge("app", "lib", EdgeKind::ProjectReference)],
    );

    let legend = legend_of(&render(&g));

    assert!(legend.contains("legend_project["), "{legend}");
    assert!(legend.contains("-->|\""), "{legend}");
    for absent in [
        "legend_external",
        "legend_container",
        "legend_nesting",
        "-.->",
        "--o",
    ] {
        assert!(
            !legend.contains(absent),
            "{absent:?} is not in this diagram and must not be in its legend:\n{legend}"
        );
    }
}

#[test]
fn a_diagram_with_nothing_in_it_gets_no_legend() {
    let source = render(&graph(Vec::new(), Vec::new()));

    assert_eq!(legend_of(&source), "", "{source}");
}

#[test]
fn a_legend_with_only_one_entry_is_left_out_because_a_single_row_teaches_nothing() {
    // This repository's own diagram: three projects, no references between
    // them, so the key came out as one row — `a project in this workspace`
    // beside three plain boxes. A key is a table of contrasts, and a table
    // with one row has none; it explains a rectangle to somebody already
    // looking at three of them.
    let g = graph(
        vec![
            node("root", "code-basics", ArchKind::Project),
            node(
                "sidecar-fixtures-Crasher-Crasher.csproj",
                "Crasher",
                ArchKind::Project,
            ),
            node(
                "sidecar-inspector-Inspector.csproj",
                "Inspector",
                ArchKind::Project,
            ),
        ],
        Vec::new(),
    );

    let source = render(&g);

    assert_eq!(
        legend_of(&source),
        "",
        "one entry is not a key, it is clutter:\n{source}"
    );
    assert_eq!(
        statements(&source)
            .iter()
            .filter(|l| l.contains("[\""))
            .count(),
        3,
        "suppressing the key must not touch the diagram:\n{source}"
    );
    assert_eq!(validate(&source), Ok(()), "{source}");
}

#[test]
fn a_legend_telling_two_shapes_apart_is_kept_even_though_the_diagram_has_no_arrows() {
    // Two rows and no arrow. The shapes are the contrast: nothing about a
    // stadium says "outside this workspace", and a reader who does not know
    // that reads two projects where there is one project and one thing this
    // scan could not see into.
    let g = graph(
        vec![
            node("app", "App", ArchKind::Project),
            node(
                "external:../Shared/Shared.csproj",
                "Shared",
                ArchKind::External,
            ),
        ],
        Vec::new(),
    );

    let legend = legend_of(&render(&g));

    assert!(legend.contains("legend_project["), "{legend}");
    assert!(legend.contains("legend_external(["), "{legend}");
    for absent in ["-->", "-.->", "--o"] {
        assert!(
            !legend.contains(absent),
            "{absent:?} is not in this diagram and must not be in its legend:\n{legend}"
        );
    }
}

#[test]
fn a_legend_identifier_can_never_collide_with_a_node_identifier() {
    // `mermaid_id` prefixes every node with `n`, so nothing it produces can
    // begin with `legend`. A collision would put a real project inside the key
    // or overwrite an entry of it, and the diagram would look fine.
    let g = graph(
        vec![
            node("legend", "Legend", ArchKind::Project),
            node("legend_project", "Legend project", ArchKind::Project),
            node("legend_from", "Legend from", ArchKind::Project),
        ],
        vec![edge("legend", "legend_project", EdgeKind::ProjectReference)],
    );

    let source = render(&g);

    assert!(
        without_legend(&source).contains("nlegend[\"Legend\"]"),
        "{source}"
    );
    assert_eq!(
        source.matches("subgraph legend[").count(),
        1,
        "exactly one legend, and it is the rendered one:\n{source}"
    );
    assert_eq!(validate(&source), Ok(()), "{source}");
}

#[test]
fn two_projects_sharing_a_name_are_told_apart_by_their_paths() {
    // Two identically labelled boxes are a picture a reader merges into one,
    // which is the same silent wrong answer that `mermaid_id` exists to
    // prevent — and here the data is right, so only the drawing can fix it.
    let mut nodes = vec![
        node("src-App-App.csproj", "App", ArchKind::Project),
        node("test-App-App.csproj", "App", ArchKind::Project),
        node("src-Lib-Lib.csproj", "Lib", ArchKind::Project),
    ];
    nodes[0].path = Some("src/App/App.csproj".into());
    nodes[1].path = Some("test/App/App.csproj".into());

    let source = render(&graph(nodes, Vec::new()));
    let drawn = without_legend(&source);

    assert!(
        drawn.contains("[\"App (src/App/App.csproj)\"]"),
        "the colliding labels must be told apart:\n{source}"
    );
    assert!(
        drawn.contains("[\"App (test/App/App.csproj)\"]"),
        "the colliding labels must be told apart:\n{source}"
    );
    assert!(
        drawn.contains("[\"Lib\"]"),
        "a label nothing collides with is left alone:\n{source}"
    );
    assert_eq!(validate(&source), Ok(()), "{source}");
}

// ---------------------------------------------------------------------------
// Validation: fences
// ---------------------------------------------------------------------------

const FLOWCHART: &str = "flowchart LR\n    a[\"A\"]\n    b[\"B\"]\n    a --> b\n";

fn fenced(body: &str) -> String {
    format!("# Title\n\nSome prose.\n\n```mermaid\n{body}```\n")
}

fn rejection(source: &str) -> ValidationError {
    validate(source).expect_err(&format!("expected a rejection for:\n{source}"))
}

#[test]
fn a_whole_file_diagram_with_no_fence_is_accepted() {
    assert_eq!(validate(FLOWCHART), Ok(()));
}

#[test]
fn exactly_one_mermaid_fence_in_a_document_is_accepted() {
    assert_eq!(validate(&fenced(FLOWCHART)), Ok(()));
}

#[test]
fn two_mermaid_fences_are_rejected_at_the_line_of_the_second() {
    let source = format!("```mermaid\n{FLOWCHART}```\n\n```mermaid\n{FLOWCHART}```\n");

    let error = rejection(&source);

    assert_eq!(error.rule, ValidationRule::FenceCount);
    assert_eq!(error.line, 8, "{error}");
}

#[test]
fn an_unterminated_mermaid_fence_is_rejected_at_the_fence() {
    let error = rejection(&format!("intro\n\n```mermaid\n{FLOWCHART}"));

    assert_eq!(error.rule, ValidationRule::FenceCount);
    assert_eq!(error.line, 3, "{error}");
}

// ---------------------------------------------------------------------------
// Validation: diagram type
// ---------------------------------------------------------------------------

#[test]
fn every_allowlisted_diagram_type_is_accepted() {
    for header in [
        "flowchart LR",
        "flowchart TD",
        "graph TD",
        "sequenceDiagram",
        "classDiagram",
        "erDiagram",
        "stateDiagram-v2",
    ] {
        assert_eq!(validate(header), Ok(()), "{header} should be accepted");
    }
}

#[test]
fn a_diagram_type_outside_the_allowlist_is_rejected() {
    // The allowlist doubles as the CSP guard: these families pull renderer
    // bundles the spike did not clear, and `stateDiagram` v1 is not the one
    // that was tested.
    for header in [
        "mindmap",
        "architecture-beta",
        "gitGraph",
        "stateDiagram",
        "flowchart-elk LR",
        "journey",
        "quadrantChart",
        "notADiagram",
    ] {
        let error = rejection(header);
        assert_eq!(
            error.rule,
            ValidationRule::DiagramType,
            "{header} should be rejected as a diagram type"
        );
        assert!(error.detail.contains(header.split(' ').next().unwrap()));
    }
}

#[test]
fn the_diagram_type_is_read_past_leading_comments_and_blank_lines() {
    assert_eq!(validate("\n%% a comment\n\nflowchart LR\n"), Ok(()));
}

#[test]
fn a_diagram_with_no_statement_at_all_is_rejected() {
    let error = rejection("%% only a comment\n\n");

    assert_eq!(error.rule, ValidationRule::DiagramType);
}

// ---------------------------------------------------------------------------
// Validation: things that can execute or navigate
// ---------------------------------------------------------------------------

#[test]
fn a_click_call_binding_is_rejected() {
    let error = rejection("flowchart LR\n    a[\"A\"]\n    click a call doThing()\n");

    assert_eq!(error.rule, ValidationRule::ForbiddenDirective);
    assert_eq!(error.line, 3, "{error}");
}

#[test]
fn a_click_href_binding_is_rejected() {
    let error = rejection("flowchart LR\n    a[\"A\"]\n    click a href \"http://x\"\n");

    assert_eq!(error.rule, ValidationRule::ForbiddenDirective);
    assert_eq!(error.line, 3, "{error}");
}

#[test]
fn an_init_directive_is_rejected() {
    // A directive can set `securityLevel: loose` and re-enable `htmlLabels`,
    // undoing every other guard in this function.
    let error = rejection("%%{init: {\"securityLevel\": \"loose\"}}%%\nflowchart LR\n");

    assert_eq!(error.rule, ValidationRule::ForbiddenDirective);
    assert_eq!(error.line, 1, "{error}");
}

#[test]
fn markup_that_can_execute_or_navigate_is_rejected() {
    for (statement, line) in [
        ("    a[<script>alert(1)</script>]", 2),
        ("    a[<iframe src=x>]", 2),
        ("    a[go]\n    click a javascript:alert(1)", 3),
        ("    a[<img src=x onerror=alert(1)>]", 2),
        ("    a[<a href=http://x>go</a>]", 2),
    ] {
        let source = format!("flowchart LR\n{statement}\n");
        let error = rejection(&source);
        assert_eq!(
            error.rule,
            ValidationRule::ForbiddenDirective,
            "{statement} should be rejected"
        );
        assert_eq!(error.line, line, "{error}");
    }
}

#[test]
fn a_directive_hidden_behind_an_unbalanced_quote_is_still_rejected() {
    // Mermaid 11.16.1 finds directives with a regex run over the raw text
    // (`directiveRegex` in `src/diagram-api/regexes.ts`, applied by
    // `detectDirective`), which knows nothing about quoting. A single `"`
    // earlier on the line therefore hides the directive from a quote-aware
    // scan and from nothing else.
    let error =
        rejection("flowchart LR\n    A[\"] %%{init:{\"flowchart\":{\"htmlLabels\":true}}}%%\n");

    assert_eq!(error.rule, ValidationRule::ForbiddenDirective);
    assert_eq!(error.line, 2, "{error}");
}

#[test]
fn a_click_binding_hidden_behind_an_unbalanced_quote_is_still_rejected() {
    // A line whose quotes never close has no trustworthy "outside the quotes",
    // so the exemption that makes `href` inside a label inert is not granted.
    let error = rejection(
        "flowchart LR\n    A[\"a\"]\n    B[\"b\"]\n    A --> B\n    \" click A call cb()\n",
    );

    assert_eq!(error.rule, ValidationRule::ForbiddenDirective);
    assert_eq!(error.line, 5, "{error}");
}

#[test]
fn a_directive_outside_the_fence_is_rejected_at_the_line_it_sits_on() {
    // `store::parse` hands on the whole post-front-matter body and
    // `arch_read_diagram` hands on the whole file, so a directive in the prose
    // around the fence can still reach a renderer that scans raw text.
    for (source, line) in [
        (
            format!(
                "%%{{init:{{\"flowchart\":{{\"htmlLabels\":true}}}}}}%%\n\n{}",
                fenced(FLOWCHART)
            ),
            1,
        ),
        (
            format!(
                "{}\n%%{{init:{{\"flowchart\":{{\"htmlLabels\":true}}}}}}%%\n",
                fenced(FLOWCHART)
            ),
            12,
        ),
    ] {
        let error = rejection(&source);
        assert_eq!(error.rule, ValidationRule::ForbiddenDirective, "{source}");
        assert_eq!(error.line, line, "{error}\n{source}");
    }
}

#[test]
fn a_forbidden_word_inside_a_quoted_label_is_not_a_rejection() {
    // Mermaid never executes the inside of a quoted string, so rejecting a
    // project honestly named `<script>` would be a wrong answer in the other
    // direction — and this module's own output depends on it.
    assert_eq!(
        validate("flowchart LR\n    a[\"<script> href javascript: onerror=\"]\n"),
        Ok(())
    );
}

#[test]
fn a_forbidden_word_inside_a_comment_is_not_a_rejection() {
    // Warnings are copied verbatim out of manifests into `%%` comments, and
    // Mermaid does not parse a comment.
    assert_eq!(
        validate("flowchart LR\n%% reference to <script> href javascript:\n    a[\"A\"]\n"),
        Ok(())
    );
}

// ---------------------------------------------------------------------------
// Validation: structure
// ---------------------------------------------------------------------------

#[test]
fn an_unclosed_subgraph_is_rejected_at_the_line_that_opened_it() {
    let error = rejection("flowchart LR\n    subgraph s[\"S\"]\n        a[\"A\"]\n");

    assert_eq!(error.rule, ValidationRule::UnbalancedSubgraph);
    assert_eq!(error.line, 2, "{error}");
}

#[test]
fn an_end_with_no_subgraph_is_rejected_at_that_end() {
    let error = rejection("flowchart LR\n    a[\"A\"]\n    end\n");

    assert_eq!(error.rule, ValidationRule::UnbalancedSubgraph);
    assert_eq!(error.line, 3, "{error}");
}

#[test]
fn the_end_of_a_sequence_diagram_block_is_not_an_unbalanced_subgraph() {
    // `end` closes `loop`, `alt` and `opt` in a sequence diagram, which has no
    // `subgraph` at all. Counting them there would reject valid diagrams, and
    // a wrong rejection is as bad as a wrong acceptance.
    assert_eq!(
        validate("sequenceDiagram\n    loop every minute\n        A->>B: poll\n    end\n"),
        Ok(())
    );
}

#[test]
fn an_edge_endpoint_that_is_never_declared_is_rejected() {
    // In Mermaid a bare identifier silently becomes an empty box. In a diagram
    // that was generated or edited by an agent it is far more likely a typo,
    // and an unlabelled box is exactly the sort of thing a reader fills in
    // with a guess.
    let error = rejection("flowchart LR\n    a[\"A\"]\n    b[\"B\"]\n    a --> bb\n");

    assert_eq!(error.rule, ValidationRule::UndeclaredNode);
    assert_eq!(error.line, 4, "{error}");
    assert!(error.detail.contains("bb"), "{error}");
}

#[test]
fn a_node_declared_inside_a_subgraph_counts_as_declared() {
    assert_eq!(
        validate(
            "flowchart LR\n    subgraph s[\"S\"]\n        a[\"A\"]\n    end\n    b[\"B\"]\n    a --> b\n"
        ),
        Ok(())
    );
}

#[test]
fn a_subgraph_may_be_an_edge_endpoint() {
    assert_eq!(
        validate("flowchart LR\n    subgraph s[\"S\"]\n        a[\"A\"]\n    end\n    a --> s\n"),
        Ok(())
    );
}

#[test]
fn structural_rules_are_not_applied_to_diagram_types_that_do_not_have_them() {
    // A class diagram's `A --|> B` is not a flowchart edge and its members are
    // not node declarations. Reusing the flowchart parser there would reject
    // valid diagrams wholesale.
    assert_eq!(
        validate("classDiagram\n    class Animal\n    Animal <|-- Duck\n"),
        Ok(())
    );
    assert_eq!(
        validate("erDiagram\n    CUSTOMER ||--o{ ORDER : places\n"),
        Ok(())
    );
}

#[test]
fn line_numbers_are_reported_against_the_source_as_the_user_typed_it() {
    // The UI puts this beside an editor showing the whole document, so a line
    // number counted from the start of the fence would point at the wrong row.
    let error = rejection(&fenced("flowchart LR\n    a[\"A\"]\n    a --> ghost\n"));

    assert_eq!(error.rule, ValidationRule::UndeclaredNode);
    assert_eq!(error.line, 8, "{error}");
}

// ---------------------------------------------------------------------------
// The contract between the two halves
// ---------------------------------------------------------------------------

/// A label carrying every character that could end a shape, a string or a
/// comment early, plus a newline.
const AWKWARD_LABEL: &str =
    "\"quoted\" (paren) [bracket] {brace} <tag> 100% | ; -- --> %%{init}\nnewline";

/// The corpus both whole-output properties run over.
///
/// Shared rather than duplicated because the two properties are about the same
/// thing from two sides — what [`render`] emits must survive [`validate`], and
/// it must stay small enough to be worth emitting. A case added for one is a
/// case the other needs too, and two private lists would drift apart silently.
fn hostile_graphs() -> Vec<ArchGraph> {
    let awkward_label = AWKWARD_LABEL;
    vec![
        graph(Vec::new(), Vec::new()),
        graph(vec![node("a", "A", ArchKind::Project)], Vec::new()),
        graph(
            vec![node("a", awkward_label, ArchKind::Project)],
            Vec::new(),
        ),
        // Legend suppressed: one entry, so the last thing written before the
        // closing of the body is a node rather than the key's `end`.
        graph(
            vec![
                node("a", "A", ArchKind::Project),
                node("b", "B", ArchKind::Project),
                node("c", "C", ArchKind::Project),
            ],
            Vec::new(),
        ),
        // Legend kept on shapes alone: two entries, no arrow.
        graph(
            vec![
                node("a", "A", ArchKind::Project),
                node("external:x", "X", ArchKind::External),
            ],
            Vec::new(),
        ),
        graph(
            vec![
                node("solution:Repo.sln", awkward_label, ArchKind::Solution),
                node("solution:Repo.sln#src", "src", ArchKind::SolutionFolder),
                node("src-App-App.csproj", "App", ArchKind::Project),
                node("src-Lib-Lib.csproj", "Lib", ArchKind::Project),
                node(
                    "external:../Shared/Shared.csproj",
                    "Shared",
                    ArchKind::External,
                ),
            ],
            vec![
                edge(
                    "solution:Repo.sln",
                    "solution:Repo.sln#src",
                    EdgeKind::Contains,
                ),
                edge(
                    "solution:Repo.sln#src",
                    "src-App-App.csproj",
                    EdgeKind::Contains,
                ),
                edge(
                    "solution:Repo.sln",
                    "src-Lib-Lib.csproj",
                    EdgeKind::Contains,
                ),
                edge(
                    "src-App-App.csproj",
                    "src-Lib-Lib.csproj",
                    EdgeKind::ProjectReference,
                ),
                edge(
                    "src-App-App.csproj",
                    "external:../Shared/Shared.csproj",
                    EdgeKind::ProjectReference,
                ),
                edge("src-Lib-Lib.csproj", "nowhere", EdgeKind::PackageDependency),
            ],
        ),
        // A component map, with the awkward label on the shape whose brackets
        // nest (`[(…)]`) — the one place a stray quote or bracket in a
        // provider name could close the shape early.
        graph(
            vec![
                node("src-Api-Api.csproj", "Api", ArchKind::Service),
                node("src-Data-Data.csproj", "Data", ArchKind::Project),
                node(
                    "component:database:postgresql",
                    awkward_label,
                    ArchKind::DataStore,
                ),
                node("component:queue:kafka", "Kafka", ArchKind::DataStore),
            ],
            vec![
                edge(
                    "src-Api-Api.csproj",
                    "component:database:postgresql",
                    EdgeKind::DataAccess,
                ),
                edge(
                    "src-Data-Data.csproj",
                    "component:database:postgresql",
                    EdgeKind::DataAccess,
                ),
                edge(
                    "src-Api-Api.csproj",
                    "component:queue:kafka",
                    EdgeKind::DataAccess,
                ),
            ],
        ),
        // A component map with exactly one shape and one arrow — two legend
        // entries, so the key is kept, and the new symbols are the only ones
        // in it.
        graph(
            vec![
                node("src-Api-Api.csproj", "Api", ArchKind::Service),
                node("component:cache:redis", "Redis", ArchKind::DataStore),
            ],
            vec![edge(
                "src-Api-Api.csproj",
                "component:cache:redis",
                EdgeKind::DataAccess,
            )],
        ),
    ]
}

#[test]
fn everything_render_produces_passes_validate() {
    // If this fails, one of the two functions is wrong and the pair is useless:
    // a renderer whose output its own gatekeeper rejects cannot be shipped, and
    // a gatekeeper that waves through what the renderer would never emit is not
    // guarding anything.
    let awkward_label = AWKWARD_LABEL;

    for mut g in hostile_graphs() {
        for derivation in [
            Derivation::Derived { scanner: 1 },
            Derivation::Inferred {
                agent: awkward_label.into(),
            },
            Derivation::User,
        ] {
            g.derivation = derivation;
            g.warnings = vec![awkward_label.into(), "```mermaid".into()];

            let source = render(&g);
            assert_eq!(
                validate(&source),
                Ok(()),
                "render produced source its own validator rejects:\n{source}"
            );
        }
    }
}

#[test]
fn every_legend_render_produces_stays_smaller_than_the_diagram_it_explains() {
    // The defect this pins: on this repository the key was laid out as just
    // another subgraph and took roughly the top 40% of the canvas — larger than
    // the two boxes and one arrow it was annotating, with real content pushed
    // below the fold. Mermaid sizes a box from its label text and a subgraph
    // from its contents, so the three numbers below are the whole of what the
    // renderer controls: how many rows, how deep they nest, and how long the
    // text in them is. They are asserted as a property over every case the
    // validate property runs on, because a single example would only pin the
    // one graph somebody happened to look at.
    //
    // The bounds are deliberately loose — this is a floor under "annotation",
    // not a pixel budget — and the worst case in the corpus is a graph using
    // every shape and every arrow at once, which no derivation produces.
    const MAX_LABEL: usize = 26;
    const MAX_LINES: usize = 12;

    for g in hostile_graphs() {
        let source = render(&g);
        let legend = legend_of(&source);
        if legend.is_empty() {
            continue;
        }

        // A subgraph inside the legend is the single most expensive row it can
        // draw: a container's padding, around a box, inside the key. Whatever
        // it would have explained is not worth a nested box to say.
        assert_eq!(
            legend.matches("subgraph ").count(),
            1,
            "the legend must not nest a subgraph:\n{legend}"
        );
        assert!(
            legend.lines().count() <= MAX_LINES,
            "a {}-line key annotates nothing; it competes:\n{legend}",
            legend.lines().count()
        );
        for line in legend.lines() {
            for label in quoted(line) {
                assert!(
                    label.chars().count() <= MAX_LABEL,
                    "a key entry names a symbol, it does not define it — \
                     {:?} is {} characters:\n{legend}",
                    label,
                    label.chars().count()
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The IPC contract
// ---------------------------------------------------------------------------

#[test]
fn a_validation_error_serialises_with_the_keys_the_ui_reads() {
    let json = serde_json::to_value(ValidationError {
        rule: ValidationRule::DiagramType,
        line: 3,
        detail: "unknown".into(),
    })
    .unwrap();

    let mut keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["detail", "line", "rule"],
        "src/ipc/types.ts mirrors ValidationError by hand — update it with this test"
    );
    assert_eq!(
        json["rule"], "diagramType",
        "src/ipc/types.ts spells ValidationRule in camelCase — update it with this test"
    );
    assert_eq!(
        serde_json::to_value(ValidationRule::UnbalancedSubgraph).unwrap(),
        "unbalancedSubgraph"
    );
}
