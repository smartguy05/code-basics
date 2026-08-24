//! The component map: which services this workspace runs, which data stores
//! they declare they speak to, and nothing that was not written down.
//!
//! [`component_graph`] is the assembly step for [`super::signals`]. The three
//! producers there read manifests, configuration and source and emit
//! candidates; [`framework::admit`] grades them; this file turns what survived
//! into the same [`ArchGraph`] the project map uses, so one renderer, one
//! store and one IPC type serve both.
//!
//! # This is a different question from [`super::graph::project_graph`]
//!
//! The project map answers *what is in this repository* — every project the
//! scan found, and the references between them. The component map answers
//! *what does this system consist of at run time* — the things that listen on
//! a port and the things they connect to. They overlap on the boxes and agree
//! on nothing else, and the most important consequence is stated first because
//! it is the one that would otherwise be got wrong:
//!
//! **A workspace with no HIGH signals produces an empty component map.** Not a
//! project map, not a diagram of every `.csproj` with no arrows — nothing. A
//! repository of class libraries has no components, and the honest picture of
//! that is a blank one. Falling back to the project map when the component map
//! came out empty would answer a question the user did not ask while looking
//! exactly like an answer to the one they did, which is the single worst
//! outcome available here: the reader has no way to tell which of the two
//! questions the picture in front of them is about.
//! `a_workspace_with_no_high_signals_produces_an_empty_map_rather_than_a_project_map`
//! pins it.
//!
//! # What may become a node, and what may become an edge
//!
//! Restating [`super`]'s rule in the terms this file works in, because this is
//! the file where a slip would be invisible:
//!
//! * A **service node** exists because a HIGH [`ComponentKind::HttpService`]
//!   signal named the project that emitted it. In practice: a
//!   `Microsoft.NET.Sdk.Web` project file, an Aspire app host, or a
//!   `package.json` with an HTTP framework in `dependencies`.
//! * A **data store node** exists because a HIGH signal of any other kind named
//!   it. In practice: a `<PackageReference>` or a `dependencies` entry naming a
//!   client library whose name states the protocol.
//! * A **project node** exists because a project declared a data store and is
//!   not itself a service. A class library that references `Npgsql` has said
//!   something true about the system, and the alternative — dropping it — would
//!   leave the store box with an arrow coming from nowhere or, worse, with no
//!   arrow at all and no explanation.
//! * An **edge** exists only between a project and a data store, and only
//!   because that project's own manifest names that store's client.
//!
//! Everything else that was seen is a warning. Nothing here reads a
//! [`framework::Detail`] as a reason to draw anything.
//!
//! # The service → service arrow, and the one node rule it obeys
//!
//! There is a project → project arrow — [`EdgeKind::ServiceCall`] — and it is
//! worth stating exactly what earns it, because it is the one edge here that
//! starts life as a line of source and it must not be mistaken for a licence to
//! draw arrows from source in general.
//!
//! [`super::signals::dotnet`] matches an `AddHttpClient` registration's literal
//! `BaseAddress` against another project's `launchSettings.json`
//! `applicationUrl`, and when exactly one project answers on that `host:port` it
//! emits a [`framework::Signal::call`]. That signal is **HIGH**, and the reason
//! it can be is precise: it does not cite the `.cs` line it read the address
//! from. It cites the *callee's* `launchSettings.json`, a declaration file, so
//! the identity of the thing being called rests on something the author wrote
//! down. [`framework::admit`] routes it to [`framework::Admitted::service_calls`]
//! rather than into the component loop, so it can never build a box.
//!
//! Drawing it is still gated on the one rule that governs every edge in this
//! file: **never invent a node.** The `DataAccess` pass draws an arrow only when
//! [`Projects::resolve`] finds the declaring project; the service-call pass goes
//! one step further and draws its arrow only when *both* endpoints already exist
//! as service nodes an earlier pass created. A call whose callee earned no
//! service box is not drawn — it becomes a warning naming the caller, the callee
//! and the file the call was read from. The evidence never quotes the address.
//!
//! Details from a project's *own* signals — its route list, its launch profile
//! urls, the connection-string keys it declares — are not reported at all. They
//! corroborate a box that already exists and change nothing about the picture,
//! and [`ArchNode`] has nowhere to put them: the only free-text field on a node
//! is its label, and a label reading `Orders.Api (GET /orders, GET /orders/{id},
//! POST /orders)` is not a diagram. They remain available from
//! [`framework::admit`] for a view that wants to list them beside the graph.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::graph::{relative_to_root, ArchGraph, ArchKind, ArchNode, Builder, EdgeKind};
use super::signals::framework::{self, Component, ComponentKind};
use super::signals::{dotnet, node, routes};
use crate::model::Project;
use crate::symbols::index::SymbolIndex;
use crate::workspace::Workspace;

/// Derive the component map for a scanned workspace.
///
/// `index` must be a [`SymbolIndex`] built over the same root: the ASP.NET
/// route producer takes its controller classes from it rather than
/// rediscovering them. An empty index costs route *details* and nothing else —
/// no node and no edge in this graph is ever created by a route — so passing
/// one that has not finished building yields a smaller map, never a wrong one.
pub fn component_graph(workspace: &Workspace, index: &SymbolIndex) -> ArchGraph {
    let mut builder = Builder::default();
    let mut signals = Vec::new();

    // Collection order is irrelevant to the result: `admit` takes the whole
    // batch and sorts what it produces, and the builder keeps nodes, edges and
    // warnings in ordered sets. `the_map_does_not_depend_on_the_order_the_
    // projects_were_scanned_in` pins that end to end rather than trusting it.
    let found = dotnet::signals(workspace);
    signals.extend(found.signals);
    for warning in found.warnings {
        builder.warn(warning);
    }

    for project in &workspace.projects {
        let found = node::signals(&workspace.root, project);
        signals.extend(found.signals);
        for warning in found.warnings {
            builder.warn(warning);
        }
    }

    let found = routes::route_signals(&workspace.root, &workspace.projects, index);
    signals.extend(found.signals);
    for warning in found.warnings {
        builder.warn(warning);
    }

    let admitted = framework::admit(signals);
    let projects = Projects::of(workspace);

    // Named through `Projects`, not relayed as the gate wrote them. A refusal
    // carries the id the signal carried, and this is the only layer that can
    // turn one into the string on the box — see `Projects::display_name` and
    // `a_gate_refusal_names_the_project_the_way_the_diagram_labels_it`.
    for warning in admitted.warnings_named(|project_id| projects.display_name(project_id)) {
        builder.warn(warning);
    }

    // Services first, so that the builder's first-wins insertion cannot let a
    // project that is both a service and a data-store consumer be filed as an
    // ordinary project. Ordering the two passes is the whole mechanism; there
    // is no second place where a node's kind is decided.
    //
    // The service node ids are collected as they are drawn, because the
    // service-call pass below may only connect boxes this pass created — see
    // there.
    let mut service_ids: BTreeSet<&str> = BTreeSet::new();
    for component in &admitted.components {
        if component.kind != ComponentKind::HttpService {
            continue;
        }
        for usage in &component.usages {
            if let Some(project) = projects.resolve(&usage.project_id, &mut builder) {
                builder.add_node(project_node(workspace, project, ArchKind::Service));
                service_ids.insert(project.id.as_str());
            }
        }
    }

    for component in &admitted.components {
        if component.kind == ComponentKind::HttpService {
            continue;
        }
        builder.add_node(store_node(component));
        for usage in &component.usages {
            if let Some(project) = projects.resolve(&usage.project_id, &mut builder) {
                builder.add_node(project_node(workspace, project, ArchKind::Project));
                builder.add_edge(&project.id, &component.id, EdgeKind::DataAccess);
            }
        }
    }

    // Service → service calls, drawn last and only between boxes that already
    // exist. This is where the "never invent a node" guard lives — the same
    // place, and for the same reason, the `DataAccess` pass above gates on
    // `Projects::resolve`: this is the only layer that knows which nodes were
    // drawn. Both endpoints must resolve to a single project *and* have earned a
    // service node; anything else abstains with a warning rather than forging a
    // box to hang the arrow on.
    for call in admitted.service_calls() {
        // Resolved one at a time: each `resolve` borrows the builder to warn on
        // an ambiguous id, so both cannot be in flight at once. A missing or
        // ambiguous endpoint is already reported by `resolve` itself.
        let Some(caller) = projects.resolve(&call.from_project, &mut builder) else {
            continue;
        };
        let Some(callee) = projects.resolve(&call.to_project, &mut builder) else {
            continue;
        };
        if service_ids.contains(caller.id.as_str()) && service_ids.contains(callee.id.as_str()) {
            builder.add_edge(&caller.id, &callee.id, EdgeKind::ServiceCall);
        } else {
            builder.warn(format!(
                "{} calls {} over HTTP (read from {}), but it was not drawn as an arrow because \
                 {} is not a service box in this map — a call is only drawn between two services \
                 the map already contains",
                projects.display_name(&call.from_project),
                projects.display_name(&call.to_project),
                display_path(&call.evidence.path),
                projects.display_name(if service_ids.contains(callee.id.as_str()) {
                    &call.from_project
                } else {
                    &call.to_project
                }),
            ));
        }
    }

    for note in cross_project_notes(&admitted.components, &projects) {
        builder.warn(note);
    }

    builder.finish()
}

/// The scanned projects, by [`Project::id`], and the refusal to guess when one
/// id names two of them.
///
/// [`crate::workspace::project_id`] replaces both path separators with `-`, so
/// `src/a/App.csproj` and `src-a/App.csproj` scan to the same id. Signals carry
/// only that id — a producer has no reason to carry a whole [`Project`] — so
/// when an id names two projects there is genuinely nothing here that can say
/// which one declared the dependency.
///
/// [`super::graph::project_graph`] survives the same collision by drawing both
/// boxes under their paths, because it walks projects and already holds each
/// one. That is not available here: the signal is the only thing pointing at a
/// project, and attributing it to either candidate would be a coin toss printed
/// as a fact. Both are therefore dropped, with a warning naming the id, and the
/// component itself still appears if any other project earned it.
struct Projects<'w> {
    by_id: BTreeMap<&'w str, Vec<&'w Project>>,
    /// Kept so [`Self::display_name`] can fall back to a path a reader can open
    /// without every caller having to hand the workspace back in.
    root: &'w std::path::Path,
}

impl<'w> Projects<'w> {
    fn of(workspace: &'w Workspace) -> Self {
        let mut by_id: BTreeMap<&'w str, Vec<&'w Project>> = BTreeMap::new();
        for project in &workspace.projects {
            by_id.entry(project.id.as_str()).or_default().push(project);
        }
        Self {
            by_id,
            root: workspace.root.as_path(),
        }
    }

    fn resolve(&self, project_id: &str, builder: &mut Builder) -> Option<&'w Project> {
        match self.by_id.get(project_id).map(Vec::as_slice) {
            Some([only]) => Some(only),
            Some(many) => {
                builder.warn(format!(
                    "a component declared by the project id '{project_id}' was not attached to a \
                     project, because {} scanned projects share that id and nothing in the \
                     declaration says which of them made it",
                    many.len()
                ));
                None
            }
            // Unreachable from a scan — every producer takes its id from a
            // project in this same workspace — but a signal arriving from
            // somewhere else must not silently become a box with no project
            // behind it.
            _ => {
                builder.warn(format!(
                    "a component was declared by the project id '{project_id}', which no scanned \
                     project has, so it was not attached to anything"
                ));
                None
            }
        }
    }

    /// What to call a project in prose a person reads.
    ///
    /// Both publishers of prose about a project go through here: this file's own
    /// [`cross_project_notes`], and the gate's refusals, which
    /// [`component_graph`] relays through
    /// [`framework::Admitted::warnings_named`] rather than taking as written.
    /// The gate cannot do this itself — a [`framework::Signal`] carries an id and
    /// nothing else, and only the assembly step holds the
    /// [`Workspace`](crate::workspace::Workspace).
    ///
    /// The name on the box when exactly one project answers to the id, and the
    /// id itself otherwise. The two fallbacks are the two cases [`Self::resolve`]
    /// already refuses, and they are the only ones where naming the project
    /// would be a claim rather than a translation: with no project there is no
    /// name to give, and with several there are several names and nothing that
    /// says which was meant. Printing the id there is not a nicety — it is the
    /// only string that is true, and it is accompanied by `resolve`'s own
    /// warning explaining why nothing was drawn.
    ///
    /// # A project's name is not automatically safe to print
    ///
    /// There is a third refusal, and it is the one that is easy to miss: a
    /// [`Project::name`] is not a string this tool chose. For a Node project it
    /// is the `name` in the `package.json`, so it can be a credentialed url —
    /// `a_gate_refusal_over_a_value_shaped_label_never_echoes_the_label` builds
    /// exactly that, and it caught this function republishing one the gate had
    /// just refused for being a value. Translating an id into a name is a
    /// *publishing* step, so it runs the same screen every other publisher in
    /// the phase runs ([`framework::looks_like_a_value`]) and falls back to the
    /// manifest path, which the reader can open and which this tool did choose.
    fn display_name(&self, project_id: &str) -> String {
        match self.by_id.get(project_id).map(Vec::as_slice) {
            Some([only]) if !framework::looks_like_a_value(&only.name) => only.name.clone(),
            Some([only]) => relative_to_root(self.root, &only.manifest_path),
            _ => project_id.to_string(),
        }
    }
}

/// The box for a project, at the kind the caller earned for it.
///
/// Identical to the project map's node in every field but [`ArchNode::kind`],
/// deliberately: the two graphs are read by the same UI, and a project that is
/// the same project in both should be the same node in both, down to the id,
/// so that a consumer can move between the maps without a lookup table.
fn project_node(workspace: &Workspace, project: &Project, kind: ArchKind) -> ArchNode {
    ArchNode {
        id: project.id.clone(),
        label: project.name.clone(),
        kind,
        project_id: Some(project.id.clone()),
        path: Some(PathBuf::from(relative_to_root(
            &workspace.root,
            &project.manifest_path,
        ))),
        ecosystem: Some(project.ecosystem.clone()),
    }
}

/// The box for a data store.
///
/// Every locating field is `None`, and that is the content of the node rather
/// than a gap in it. A data store has no manifest, no directory and no
/// ecosystem inside this workspace, because it is not in this workspace; the
/// only true things known about it are the provider name the client library
/// stated and the fact that something declared it speaks to it.
/// `a_data_store_node_never_carries_a_project_a_path_or_an_ecosystem` pins it,
/// so that a later change cannot quietly start filling one of these in with the
/// declaring project's details and turn the box into a claim about where the
/// store lives.
fn store_node(component: &Component) -> ArchNode {
    ArchNode {
        id: component.id.clone(),
        label: component.label.clone(),
        kind: ArchKind::DataStore,
        project_id: None,
        path: None,
        ecosystem: None,
    }
}

/// Report every supporting signal that made a claim about *another* project's
/// component, as prose, because it was not allowed to draw one.
///
/// The test for "another project" is set membership, not string matching: a
/// detail belongs to the component's own projects when its project id is one of
/// the ids that earned the box through a HIGH signal. The Aspire app host is the
/// producer that fails it today — it is one project saying something about a
/// different one. (`AddHttpClient` used to fail it too, as a MEDIUM note; it now
/// emits a HIGH [`framework::Signal::call`] drawn as a real
/// [`EdgeKind::ServiceCall`] arrow, so a matched call is an edge and never a
/// note.)
///
/// # These are the only details reported
///
/// A project's own route list and launch profile urls are not, and the
/// asymmetry is the point. Those enrich a box the same project already earned;
/// nothing about the picture changes whether they are present or absent, so
/// reporting them would bury the refusals a reader has to see under a
/// transcript of everything that went right. A cross-project detail is
/// different in kind: it is the one case where a MEDIUM signal *wanted* an
/// arrow and did not get one, which is exactly the shape of the missing
/// information the user needs to be told about.
///
/// # No excerpt is ever quoted, and the detail text is published
///
/// The file and line are named so the claim can be checked; the text that was
/// read is not repeated. For an `AddHttpClient` the excerpt is a literal base
/// address — a `host:port` that
/// [`framework::admit`](super::signals::framework::admit) permitted only
/// because it matched a url already checked into this repository — and a
/// warning is not the place to start copying addresses around.
///
/// [`framework::Detail::text`] *is* quoted, verbatim, which makes this function
/// one of the phase's publishing surfaces rather than a bystander. The claim
/// that used to stand here — that the detail is "producer prose that passed the
/// gate's screen and names projects only" — was false in both halves: the .NET
/// launch-profile producer interpolated the whole `applicationUrl` into its
/// detail, so the text was a url and not prose; and the gate's screen ran over
/// the *label*, never over the detail, so nothing had checked it. A
/// `launchSettings.json` with `user:password@host` in its url therefore reached
/// this line and, through it, `ArchGraph::warnings` and the exported mermaid.
///
/// What is true now, and what each half rests on:
///
/// * The producer does not hand over a value. The launch-profile detail names
///   the profile; the `AddHttpClient` and Aspire details name projects.
/// * The gate refuses one that does. [`framework::admit`] screens `detail` with
///   the same value-shape test it applies to `label`
///   ([`framework::DiscardReason::DetailLooksLikeAValue`]), so the next producer
///   to interpolate a url loses its signal instead of publishing it here.
///
/// Neither half is sufficient alone and neither is trusted alone. The
/// end-to-end sweep is
/// `a_credentialed_launch_profile_url_reaches_no_string_the_component_map_exports`,
/// which asserts over the mermaid and the JSON as well as the graph — a leak
/// test that reads only the field a fix touched proves nothing about the field
/// it did not.
///
/// # The project is named the way the diagram names it
///
/// A [`framework::Detail`] carries a [`crate::model::Project::id`], and this
/// function used to print it: `src-Orders.Api-Orders.Api.csproj:`. The
/// producers' warnings, which land in the same
/// [`ArchGraph::warnings`](super::graph::ArchGraph::warnings) list, have always
/// opened with the display name: `Orders.Api:`. One list in two vocabularies is
/// bad enough; the half that lost was the one a reader can act on, because the
/// id is drawn nowhere, labels nothing, and is not a path they can open —
/// [`crate::workspace::project_id`] flattens the separators out of it, so it is
/// not even the file's location. [`Projects::display_name`] translates, and
/// falls back to the id only where there is no single name to translate to.
fn cross_project_notes(components: &[Component], projects: &Projects<'_>) -> Vec<String> {
    let mut notes = Vec::new();
    for component in components {
        let owners: BTreeSet<&str> = component
            .usages
            .iter()
            .map(|usage| usage.project_id.as_str())
            .collect();

        for detail in &component.details {
            if owners.contains(detail.project_id.as_str()) {
                continue;
            }
            let path = display_path(&detail.evidence.path);
            let at = match detail.evidence.line {
                Some(line) => format!("{path}:{line}"),
                None => path,
            };
            notes.push(format!(
                "{}: '{}' was recorded about '{}' from {at} as a note rather than an arrow, \
                 because a supporting signal may enrich a component but may never bring an edge \
                 into existence",
                projects.display_name(&detail.project_id),
                detail.text,
                component.label
            ));
        }
    }
    notes
}

/// Forward slashes on every platform, for the reason [`super::graph`] gives:
/// these strings are read by a person and stored in files that move between
/// machines.
fn display_path(path: &std::path::Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}
