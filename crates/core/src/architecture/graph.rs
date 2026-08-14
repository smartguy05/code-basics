//! The derived project graph: nodes, edges, and the rules that produce them.
//!
//! [`project_graph`] takes a scanned [`Workspace`] and returns an [`ArchGraph`]
//! assembled from four independent sources of evidence, each with its own
//! rule and each deliberately unable to contaminate the others:
//!
//! * **`<ProjectReference>` items** in `.csproj`/`.fsproj`/`.vbproj` files
//!   become [`EdgeKind::ProjectReference`] edges.
//! * **`path` dependencies** in `Cargo.toml` files become
//!   [`EdgeKind::ProjectReference`] edges too, resolved by location rather than
//!   by name (see [`cargo_dependencies`]).
//! * **`dependencies`/`devDependencies` names** in `package.json` files become
//!   [`EdgeKind::PackageDependency`] edges, but *only* when the name matches
//!   another Node project in the same workspace.
//! * **`.sln`/`.slnx` grouping, npm workspace globs and Cargo `[workspace]
//!   members` globs** become [`EdgeKind::Contains`] edges, which are membership
//!   and nothing else — except in a pnpm workspace, where the `workspaces` key
//!   is not membership at all and nothing is drawn from it (see
//!   [`pnpm_notice`]).
//!
//! # Why this is not a field on `Project`
//!
//! [`Project`](crate::model::Project) is a wire type. It crosses IPC on every
//! workspace open and is held by the run sidebar, the tests tree, the changes
//! view and the config editor — none of which would read a reference list, and
//! all of which would pay to carry it. It is also the wrong lifetime: the scan
//! happens once when a directory is opened, while the manifest files it would
//! have read are edited continuously afterwards, so a cached reference list
//! would go stale with nothing able to notice. The graph is therefore computed
//! on demand, re-reading the manifests as they are *now* and using the scan
//! only for the set of projects that exist and where they live.
//!
//! # An arrow is a strong claim
//!
//! A user reading a diagram treats an edge as fact. Every rule below therefore
//! abstains rather than guesses, and — because a diagram that looks complete
//! while quietly missing an arrow is just as misleading — every reference that
//! could not be turned into an edge is recorded in [`ArchGraph::warnings`]
//! rather than dropped.
//!
//! That applies to this module's own blind spots as well, which is what
//! [`unexpressible_relations`] is for: a `tauri.conf.json` wires a desktop shell
//! to its frontend and to a bundled sidecar, and none of the four rules above
//! opens it, so all three used to sit on the diagram as unrelated boxes with
//! nothing said. It produces no node and no edge — only prose naming the file
//! and what it points at.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::adapters::{cargo, dotnet, node};
use crate::model::Project;
use crate::workspace::Workspace;

/// The version of the derivation rules in this module.
///
/// Stamped into every graph as [`Derivation::Derived`] so that a stored or
/// exported diagram carries the rules that produced it. Bump it whenever a
/// rule below changes what edges a given workspace yields — a diagram derived
/// by older rules is not wrong, it is *differently derived*, and a consumer
/// comparing two of them needs to be able to tell those apart.
///
/// * `1` — .NET project references, npm dependencies, `.sln` and npm workspace
///   containment.
/// * `2` — Cargo path dependencies and `[workspace] members` containment. A
///   Rust repository derived at version 1 has boxes and no arrows between them,
///   which is not a differently-formatted answer to the same question but a
///   materially different one, so the two must not be compared silently.
pub const SCANNER_VERSION: u32 = 2;

// ---------------------------------------------------------------------------
// Types crossing IPC
// ---------------------------------------------------------------------------

/// What a node in the graph stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ArchKind {
    /// A project the scan found, with a [`Project`] behind it.
    Project,
    /// A grouping container: a `.sln`/`.slnx`, an npm workspace root, or a
    /// Cargo `[workspace]` root.
    Solution,
    /// A folder *inside* a solution. Purely organisational — it exists in the
    /// solution file and nowhere on disk.
    SolutionFolder,
    /// Something referenced from inside the workspace that lives outside it.
    /// It has no [`Project`], because the scan never saw it.
    External,
    /// A project the scan found that also **declares it serves HTTP** — a
    /// `Microsoft.NET.Sdk.Web` project, an Aspire app host, or a `package.json`
    /// depending on an HTTP framework.
    ///
    /// A narrower [`ArchKind::Project`], not a different thing: a service node
    /// carries the same [`ArchNode::project_id`], [`ArchNode::path`] and
    /// [`ArchNode::ecosystem`] a project node would, and a consumer that does
    /// not care about the distinction can treat the two identically. The
    /// distinction is kept because it is the one claim the component map exists
    /// to make about a project — that it is reachable over a wire — and
    /// flattening it into `Project` would leave the map unable to say which
    /// boxes are the services.
    ///
    /// Only [`super::signals`] produces this. [`project_graph`] never does: a
    /// project map answers "what is here", and whether a project serves HTTP is
    /// not a question it asks.
    Service,
    /// A database, cache or message broker a project **declares** it speaks a
    /// protocol to, labelled by provider (`PostgreSQL`, `Redis`, `Kafka`) and
    /// nothing else.
    ///
    /// # Why one kind and not three
    ///
    /// [`super::signals::framework::ComponentKind`] distinguishes a database
    /// from a cache from a queue, and that distinction is *not* carried here.
    /// It is already in the label — nobody reads `Redis` and wonders whether it
    /// is a broker — and giving each its own shape would spend the reader's
    /// whole shape vocabulary on a difference the text already makes, while
    /// costing the one distinction that matters: **this box is not a project**.
    /// It cannot be opened, run, tested or diffed, and no amount of clicking on
    /// it will find source code, because nothing in this workspace *is* it.
    ///
    /// # What it is not
    ///
    /// It is not a deployed instance. Two projects declaring `Npgsql` share one
    /// box because the box is the *technology* — "PostgreSQL is spoken here" —
    /// and nothing in either manifest says whether they reach the same server.
    /// It carries no host, no port and no database name for the same reason
    /// [`super::signals`] refuses to read a connection string's value at all.
    DataStore,
}

/// What an edge asserts about its two endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EdgeKind {
    /// A compile-time dependency one manifest declares on another *by
    /// location*: `<ProjectReference Include="..." />` in an MSBuild project,
    /// or `{ path = "..." }` in a `Cargo.toml`.
    ///
    /// The two are one kind because they make the same claim and resolve the
    /// same way — a relative path from the referring manifest's directory,
    /// matched against a manifest the scan found. That is a different claim
    /// from [`EdgeKind::PackageDependency`], which resolves by *name* and can
    /// therefore be ambiguous or accidental in ways a path cannot.
    ///
    /// Cargo's dev- and build-dependencies land here as well, undistinguished.
    /// See [`cargo_dependencies`] for why they are neither dropped nor given a
    /// kind of their own.
    ProjectReference,
    /// A `package.json` dependency naming another project in this workspace.
    PackageDependency,
    /// Membership, never dependency: a solution, a solution folder or an npm
    /// workspace root holding a project. Containment says which things ship
    /// together and is silent about which needs which.
    Contains,
    /// A project declares, in a manifest, that it speaks a data store's
    /// protocol: it references `Npgsql`, or `ioredis`, or `Confluent.Kafka`.
    ///
    /// The claim is exactly "this project can talk to this technology", which
    /// is what the reference states. It is **not** "this project reads that
    /// data at runtime" — a reference proves capability, and no manifest states
    /// use — and it is not "these two projects share an instance", which no
    /// file in a repository says at all.
    ///
    /// Always project → data store. The reverse never appears: nothing in a
    /// database's own configuration is visible from here, so a store cannot be
    /// observed to depend on anything.
    DataAccess,
}

/// Where a graph came from, and therefore how much it can be trusted.
///
/// The three cases are kept apart because they fail differently. A `Derived`
/// graph is reproducible and can be wrong only if the rules are wrong. An
/// `Inferred` graph came from a language model reading the code and may be
/// confidently wrong. A `User` graph is whatever a person drew, which is
/// authoritative about intent and says nothing about what the code does.
/// Merging them into one "source" string would lose exactly the distinction a
/// reader needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Derivation {
    /// Computed from files on disk by [`project_graph`], at this rule version.
    Derived { scanner: u32 },
    /// Proposed by a coding agent, named so a reader can weigh it.
    Inferred { agent: String },
    /// Drawn by a person.
    User,
}

/// One box in the diagram.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ArchNode {
    /// Unique within a graph. For a project this is normally the
    /// [`Project::id`] the scan produced, so that a node can be traced back to
    /// something runnable; containers and externals use a prefixed id
    /// (`solution:`, `workspace:`, `external:`) that cannot collide with one.
    ///
    /// [`Project::id`] is not injective — it replaces both path separators
    /// with `-`, so `src/a/App.csproj` and `src-a/App.csproj` produce the same
    /// one — and the projects sharing an id therefore fall back to a
    /// `project:`-prefixed workspace-relative path, which is unique by
    /// construction. See [`NodeIds`].
    pub id: String,
    /// What to draw in the box.
    pub label: String,
    pub kind: ArchKind,
    /// The [`Project::id`], when this node is a project. `None` for containers
    /// and for anything outside the workspace — and also `None` for a project
    /// whose id another project shares, because that id no longer names one
    /// project and a consumer resolving it would get whichever came first.
    pub project_id: Option<String>,
    /// Where this lives, **relative to the workspace root and with forward
    /// slashes**, so a stored graph survives being moved or opened on another
    /// machine — the same reason [`Project::id`] is relative. Externals carry
    /// a `../`-prefixed path for the same reason. `None` for solution folders,
    /// which exist only inside a solution file.
    pub path: Option<PathBuf>,
    /// Which adapter found the project, for projects only.
    pub ecosystem: Option<String>,
}

/// One arrow in the diagram.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ArchEdge {
    /// [`ArchNode::id`] of the source.
    pub from: String,
    /// [`ArchNode::id`] of the target.
    pub to: String,
    pub kind: EdgeKind,
    /// Free text drawn on the arrow. Always `None` for a derived edge: the
    /// facts available here (a reference exists, a package name matched) are
    /// already carried by [`ArchEdge::kind`], and anything further would be
    /// commentary this module cannot substantiate. The field exists for the
    /// `Inferred` and `User` graphs, where the label *is* the content.
    pub label: Option<String>,
}

/// A whole diagram.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ArchGraph {
    /// Sorted by [`ArchNode::id`].
    pub nodes: Vec<ArchNode>,
    /// Sorted by endpoint, then kind.
    pub edges: Vec<ArchEdge>,
    /// Everything that was found and could not be turned into an edge, in the
    /// author's own words. Never empty because something went wrong with the
    /// tool — always because something in the workspace does not line up.
    pub warnings: Vec<String>,
    pub derivation: Derivation,
}

// ---------------------------------------------------------------------------
// Derivation
// ---------------------------------------------------------------------------

/// Derive the project graph for a scanned workspace.
///
/// Reads every project's manifest from disk again rather than using anything
/// cached on [`Project`] — see the module documentation for why. An unreadable
/// or unparseable manifest costs that project's edges and nothing else: the
/// rest of the graph is still worth drawing.
pub fn project_graph(workspace: &Workspace) -> ArchGraph {
    let mut builder = Builder::default();
    let ids = NodeIds::resolve(workspace, &mut builder);

    for project in &workspace.projects {
        builder.add_node(project_node(workspace, project, &ids));
    }

    let cargo_manifests = cargo_manifests(workspace, &mut builder);

    dotnet_references(workspace, &ids, &mut builder);
    node_dependencies(workspace, &ids, &mut builder);
    cargo_dependencies(workspace, &cargo_manifests, &ids, &mut builder);
    unreadable_projects(workspace, &mut builder);
    unexpressible_relations(workspace, &mut builder);
    npm_workspace_members(workspace, &ids, &mut builder);
    cargo_workspace_members(workspace, &cargo_manifests, &ids, &mut builder);
    solution_containment(workspace, &ids, &mut builder);

    builder.finish()
}

/// The node id to draw each scanned project under.
///
/// # Why this is not just `Project::id`
///
/// [`Project::id`] is the workspace-relative path with **both** separators
/// replaced by `-` (`workspace::project_id`), which is not injective:
/// `src/a/App.csproj` and `src-a/App.csproj` scan to the same id. Everything
/// here is keyed by node id — the builder's node map, both endpoints of every
/// edge — so with the raw id one of the two boxes was dropped as a duplicate
/// and the survivor was handed the arrows the *other* project declared. Both
/// failure modes this module exists to avoid, at once.
///
/// The colliding projects are therefore drawn under `project:<relative path>`
/// instead, which is unique because the paths are, and which cannot be
/// confused with an ordinary id — those never contain `/`, having just had it
/// removed. Projects with an unambiguous id keep it, so ids stay traceable
/// back to something runnable in the overwhelmingly common case.
///
/// The lookup is by manifest path rather than by id, because the manifest path
/// is what actually distinguishes the two projects; every caller already holds
/// the [`Project`] it is drawing an edge for, so nothing has to guess.
///
/// Renaming the box is not on its own enough to be honest — a reader still
/// sees two projects that the rest of the app cannot tell apart — so
/// [`resolve`](NodeIds::resolve) also warns, naming every colliding path.
struct NodeIds {
    by_manifest: BTreeMap<PathBuf, Entry>,
}

struct Entry {
    id: String,
    /// Whether [`Project::id`] identified this project on its own. `false`
    /// suppresses [`ArchNode::project_id`], which would otherwise hand a
    /// consumer an id resolving to either project.
    unique: bool,
}

impl NodeIds {
    fn resolve(workspace: &Workspace, builder: &mut Builder) -> Self {
        let mut by_id: BTreeMap<&str, Vec<&Project>> = BTreeMap::new();
        for project in &workspace.projects {
            by_id.entry(project.id.as_str()).or_default().push(project);
        }

        let mut by_manifest = BTreeMap::new();
        for (id, projects) in by_id {
            if let [only] = projects.as_slice() {
                by_manifest.insert(
                    only.manifest_path.clone(),
                    Entry {
                        id: id.to_string(),
                        unique: true,
                    },
                );
                continue;
            }

            let mut paths = Vec::new();
            for project in &projects {
                let relative = relative_to_root(&workspace.root, &project.manifest_path);
                by_manifest.insert(
                    project.manifest_path.clone(),
                    Entry {
                        id: format!("project:{relative}"),
                        unique: false,
                    },
                );
                paths.push(relative);
            }
            builder.warn(format!(
                "{} projects share the id '{id}' ({}), because a project id replaces path \
                 separators with '-'; each is drawn under its own path, and none of them is \
                 linked back to a project because that id no longer names one",
                projects.len(),
                paths.join(", ")
            ));
        }

        Self { by_manifest }
    }

    /// The entry for a project, falling back to its own id.
    ///
    /// [`resolve`](NodeIds::resolve) inserts every project in the workspace, so
    /// the fallback is unreachable for anything the scan produced; it is here
    /// rather than an `expect` because a missing entry would be a bookkeeping
    /// slip in this module and drawing the box under its plain id is a better
    /// answer to that than taking the process down.
    fn of<'a>(&'a self, project: &'a Project) -> (&'a str, bool) {
        match self.by_manifest.get(&project.manifest_path) {
            Some(entry) => (entry.id.as_str(), entry.unique),
            None => (project.id.as_str(), true),
        }
    }

    /// Just the node id, for the many callers that only draw an edge.
    fn id_of<'a>(&'a self, project: &'a Project) -> &'a str {
        self.of(project).0
    }
}

/// Accumulates nodes, edges and warnings, keeping each set unique and ordered.
///
/// Ordering is not cosmetic: a diagram that reshuffles between runs produces a
/// git diff nobody can read, and a graph that differs from itself cannot be
/// compared across two versions of a repository. Uniqueness matters for the
/// same reason — a project belonging to two solutions is one box with two
/// containment arrows, not two boxes.
#[derive(Default)]
pub(super) struct Builder {
    nodes: BTreeMap<String, ArchNode>,
    edges: BTreeSet<(String, String, EdgeKind)>,
    warnings: BTreeSet<String>,
}

impl Builder {
    /// Insert a node, keeping the first description of a given id.
    ///
    /// First-wins rather than last-wins because projects are added before
    /// anything else, so a project node can never be overwritten by the
    /// thinner node a container or a reference would have produced for it.
    pub(super) fn add_node(&mut self, node: ArchNode) {
        self.nodes.entry(node.id.clone()).or_insert(node);
    }

    pub(super) fn add_edge(&mut self, from: &str, to: &str, kind: EdgeKind) {
        self.edges.insert((from.to_string(), to.to_string(), kind));
    }

    pub(super) fn warn(&mut self, message: String) {
        self.warnings.insert(message);
    }

    pub(super) fn finish(self) -> ArchGraph {
        ArchGraph {
            nodes: self.nodes.into_values().collect(),
            edges: self
                .edges
                .into_iter()
                .map(|(from, to, kind)| ArchEdge {
                    from,
                    to,
                    kind,
                    label: None,
                })
                .collect(),
            warnings: self.warnings.into_iter().collect(),
            derivation: Derivation::Derived {
                scanner: SCANNER_VERSION,
            },
        }
    }
}

fn project_node(workspace: &Workspace, project: &Project, ids: &NodeIds) -> ArchNode {
    let (id, unique) = ids.of(project);
    ArchNode {
        id: id.to_string(),
        label: project.name.clone(),
        kind: ArchKind::Project,
        project_id: unique.then(|| project.id.clone()),
        path: Some(PathBuf::from(relative_to_root(
            &workspace.root,
            &project.manifest_path,
        ))),
        ecosystem: Some(project.ecosystem.clone()),
    }
}

// ---------------------------------------------------------------------------
// .NET project references
// ---------------------------------------------------------------------------

/// Turn every `<ProjectReference>` into an edge, an external node, or a warning.
///
/// Only `.NET` projects are candidate targets. A `<ProjectReference>` naming a
/// `package.json` is not a thing that exists, and allowing the lookup to reach
/// across ecosystems would only ever match by accident.
fn dotnet_references(workspace: &Workspace, ids: &NodeIds, builder: &mut Builder) {
    let by_manifest: BTreeMap<PathBuf, &Project> = workspace
        .projects
        .iter()
        .filter(|p| p.ecosystem == "dotnet")
        .map(|p| (p.manifest_path.clone(), p))
        .collect();

    // A project whose manifest the scan could not read is still a candidate
    // *target* — a reference resolves to it by path, and a path is knowable
    // whatever the file's contents are — but never a source. Its own reference
    // list came out of a document that did not parse, and
    // `dotnet::parse_project_file` is deliberately lenient enough to return the
    // half it managed to read, which would put a partial list on the diagram as
    // if it were complete. `unreadable_projects` says so once, with the reason.
    for project in workspace
        .projects
        .iter()
        .filter(|p| p.ecosystem == "dotnet" && p.unreadable.is_none())
    {
        let Ok(xml) = std::fs::read_to_string(&project.manifest_path) else {
            builder.warn(format!(
                "{}: could not read {} to look for project references",
                project.name,
                relative_to_root(&workspace.root, &project.manifest_path)
            ));
            continue;
        };

        // A reference listed twice needs no special handling: the builder
        // keeps edges and warnings in sets, so a repeat collapses onto the
        // first one.
        for raw in dotnet::parse_project_file(&xml).project_references {
            // A rooted reference is not resolvable from here at all. `\Shared`
            // means the root of the current drive and `C:\Shared` names a
            // specific one, so joining either onto the referring project's
            // directory would forge a path that lands *inside* the workspace
            // and draw a confident arrow at whichever project happened to sit
            // there. Everything downstream would believe it, because the
            // forged path passes the `starts_with(root)` guard honestly.
            if is_rooted(&raw) {
                let id = format!("external:{}", raw.replace('\\', "/"));
                builder.add_node(ArchNode {
                    label: file_stem(Path::new(&raw.replace('\\', "/"))),
                    id: id.clone(),
                    kind: ArchKind::External,
                    project_id: None,
                    // No path, unlike a relative external. There, `../Shared`
                    // is genuinely derived from the reference and the root and
                    // still means the same thing on another machine. Here the
                    // only honest answer would be the machine-specific string
                    // the reference names, which is exactly what
                    // `ArchNode::path` promises not to be.
                    path: None,
                    ecosystem: None,
                });
                builder.add_edge(ids.id_of(project), &id, EdgeKind::ProjectReference);
                builder.warn(format!(
                    "{}: project reference '{raw}' is an absolute path, which cannot be \
                     located relative to the workspace; drawn as an external component",
                    project.name
                ));
                continue;
            }

            let resolved = resolve_lexically(&project.dir, &raw);

            if !resolved.starts_with(&workspace.root) {
                let id = format!("external:{}", relative_to_root(&workspace.root, &resolved));
                builder.add_node(ArchNode {
                    label: file_stem(&resolved),
                    id: id.clone(),
                    kind: ArchKind::External,
                    project_id: None,
                    path: Some(PathBuf::from(relative_to_root(&workspace.root, &resolved))),
                    ecosystem: None,
                });
                builder.add_edge(ids.id_of(project), &id, EdgeKind::ProjectReference);
                builder.warn(format!(
                    "{}: project reference '{raw}' points outside the workspace; \
                     drawn as an external component",
                    project.name
                ));
                continue;
            }

            if let Some(target) = by_manifest.get(&resolved) {
                builder.add_edge(
                    ids.id_of(project),
                    ids.id_of(target),
                    EdgeKind::ProjectReference,
                );
                continue;
            }

            // Inside the scanned area but matching nothing. This is a broken
            // manifest, not a component the diagram is missing, so no node is
            // invented for it — an `External` box here would assert that
            // something exists at a path where nothing does.
            let near_miss = by_manifest
                .keys()
                .find(|candidate| {
                    candidate
                        .as_os_str()
                        .eq_ignore_ascii_case(resolved.as_os_str())
                })
                .map(|candidate| relative_to_root(&workspace.root, candidate));

            builder.warn(match near_miss {
                // Matching case-insensitively would be a guess: right on NTFS,
                // wrong on a case-sensitive filesystem, and this code cannot
                // tell which one it is looking at. Naming the near miss is
                // strictly more useful than either guess, and leaves the
                // decision with the person who can check.
                Some(candidate) => format!(
                    "{}: project reference '{raw}' matches no scanned project, but \
                     {candidate} differs only in casing",
                    project.name
                ),
                None => format!(
                    "{}: project reference '{raw}' matches no project the scan found",
                    project.name
                ),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Node dependencies
// ---------------------------------------------------------------------------

/// Turn `dependencies`/`devDependencies` entries into edges, when — and only
/// when — the name belongs to another Node project in this workspace.
///
/// Third-party packages produce nothing at all: not an edge, not an external
/// node, not a warning. `react` is a fact about the lockfile rather than about
/// this system's architecture, and drawing the hundreds of them would bury the
/// handful of edges that carry meaning. This is the one place where a missing
/// arrow is deliberate rather than reported — with one exception: a
/// `workspace:` specifier *says* the dependency is local, so one that matches
/// nothing is a genuine miss and is warned about.
///
/// The lookup is restricted to `ecosystem == "node"` projects. A .NET project
/// named `Lib` and an npm dependency named `Lib` share nothing but a string,
/// and a cross-ecosystem match is exactly the sort of coincidence that yields
/// a confidently wrong arrow.
fn node_dependencies(workspace: &Workspace, ids: &NodeIds, builder: &mut Builder) {
    // Read every manifest once, up front, because both halves of this rule
    // need it: the names a package can be *depended on by* are declared in the
    // same files as the dependencies themselves.
    // Projects the scan already marked unreadable are skipped outright. Reading
    // them again would fail again, in the same way, and produce a second and
    // vaguer complaint about a file `unreadable_projects` has already reported
    // with the scan's own line and column.
    let mut manifests: Vec<(&Project, node::PackageJson)> = Vec::new();
    for project in workspace
        .projects
        .iter()
        .filter(|p| p.ecosystem == "node" && p.unreadable.is_none())
    {
        let relative = relative_to_root(&workspace.root, &project.manifest_path);
        // "Could not read" and "could not parse" are kept apart deliberately:
        // one is a missing or locked file and the other is a syntax error, and
        // they need different things from the person reading the warning.
        let text = match std::fs::read_to_string(&project.manifest_path) {
            Ok(text) => text,
            Err(e) => {
                builder.warn(format!(
                    "{}: could not read {relative} to look for dependencies, so its \
                     edges are missing: {e}",
                    project.name
                ));
                continue;
            }
        };
        let Some(pkg) = node::parse_package_json(&text) else {
            builder.warn(format!(
                "{}: could not parse {relative}, so its dependency edges are missing",
                project.name
            ));
            continue;
        };
        manifests.push((project, pkg));
    }

    // Keyed on the name each package *declares*, never on the scan's display
    // name. The scan falls a nameless `package.json` back to its directory
    // name so the sidebar has something to show; that fallback is not a
    // package name, and a package with no `name` cannot be depended on by name
    // at all — so every match against one would be false by construction. A
    // directory called `config` next to a dependency on the real `config`
    // package is not a hypothetical.
    let mut by_name: BTreeMap<&str, Vec<&Project>> = BTreeMap::new();
    for (project, pkg) in &manifests {
        if let Some(name) = pkg.name.as_deref() {
            by_name.entry(name).or_default().push(project);
        }
    }

    for (project, pkg) in &manifests {
        for (dependency, specifier) in pkg.dependencies.iter().chain(pkg.dev_dependencies.iter()) {
            let Some(matches) = by_name.get(dependency.as_str()) else {
                // Silence is right for a third-party package, but the
                // `workspace:` protocol is an unambiguous declaration that
                // this dependency is meant to resolve inside the repository.
                // One matching nothing is a real miss, and the specifier
                // proving it is already in hand.
                if specifier.starts_with("workspace:") {
                    builder.warn(format!(
                        "{}: dependency '{dependency}' is declared '{specifier}' but no \
                         project in this workspace declares that package name",
                        project.name
                    ));
                }
                continue;
            };
            match matches.as_slice() {
                // Compared by manifest path, not by id: two projects can
                // share an id (see `NodeIds`), and comparing ids there would
                // take a real edge between them for a self-reference.
                [target] if target.manifest_path != project.manifest_path => {
                    builder.add_edge(
                        ids.id_of(project),
                        ids.id_of(target),
                        EdgeKind::PackageDependency,
                    );
                }
                [_] => {}
                // Two packages in one workspace declaring the same `name` is
                // a real defect, and there is no basis for picking one of
                // them, so the edge is abandoned and the ambiguity reported.
                _ => builder.warn(format!(
                    "{}: dependency '{dependency}' matches {} projects with that \
                     package name, so no edge was drawn",
                    project.name,
                    matches.len()
                )),
            }
        }
    }
}

/// Report every project whose manifest the **scan** could not read.
///
/// A manifest that was already broken when the directory was opened is the case
/// a user actually hits — the existing per-ecosystem warnings all describe a
/// file that broke while the workspace was open. The scan keeps such a project
/// and records the reason on [`Project::unreadable`], so the component still
/// gets a box here; what it cannot have is edges, because nothing that would not
/// parse can be the source of a claim. A box with no arrows and no explanation
/// is exactly the "looks complete, quietly missing" failure this module exists
/// to prevent, so the reason is surfaced.
///
/// # Why the field and not a second walk of the disk
///
/// This used to walk the tree again looking for `package.json` files that
/// produced no project, and re-parse each one to recover an error message. That
/// existed because the scan dropped unparseable manifests entirely; it does not
/// any more. Reading the field instead is strictly better on three counts: it
/// costs no filesystem work, it covers every ecosystem rather than only Node
/// (a broken `.csproj` and a broken `Cargo.toml` were both silent before), and
/// it quotes the *scan's own* reason rather than a second opinion produced by a
/// different parser, which could disagree with what the rest of the app shows.
///
/// The reason is quoted verbatim rather than reworded because it is the only
/// part of the warning a user can act on: it names a line and a column.
///
/// # The subject is the name, not the path
///
/// This warning used to open with the manifest's workspace-relative path while
/// every other warning in this module opens with [`Project::name`] — the string
/// the diagram prints on the box. One list in two vocabularies makes a reader
/// work out, line by line, which kind of string they are looking at, and the
/// half that should lose is the one that does not appear on the picture they are
/// reading the warnings about. The path is still in the message, because it is
/// the file they have to go and open; it is simply no longer the subject.
fn unreadable_projects(workspace: &Workspace, builder: &mut Builder) {
    for project in &workspace.projects {
        let Some(reason) = project.unreadable.as_deref() else {
            continue;
        };
        builder.warn(format!(
            "{}: the scan could not read {}, so the project has a box and no edges at all \
             — every reference it declares, in either direction, is missing from this \
             graph: {reason}",
            project.name,
            relative_to_root(&workspace.root, &project.manifest_path)
        ));
    }
}

// ---------------------------------------------------------------------------
// Relations no edge kind here can express
// ---------------------------------------------------------------------------

/// Report the files that declare relations between parts of this workspace
/// which none of [`EdgeKind`]'s variants can carry.
///
/// # The failure this exists for
///
/// Every rule above abstains *about something it looked at*: a reference that
/// resolved to nothing, a glob that matched nothing, a manifest that would not
/// parse. This one abstains about a file class no rule opens at all, and that is
/// a strictly worse silence. Derived over this repository, the graph is true —
/// six boxes, three arrows, every one checkable in a manifest — and yet the
/// desktop shell, its frontend and its bundled sidecar float as three unrelated
/// boxes, because the file that wires them together is `tauri.conf.json` and
/// nothing here reads it. A reader concludes those parts are as unrelated as two
/// samples that genuinely are, and that conclusion is false. The module's own
/// contract says a diagram that looks complete while quietly missing an arrow is
/// just as misleading as a wrong one and much harder to notice — which applies
/// to the module's own blind spots or it is not a contract.
///
/// # Why a warning and not an edge
///
/// A Tauri frontend is reached by run-time IPC and a bundled resource is copied
/// into an installer. Neither is a compile-time reference, which is the only
/// claim [`EdgeKind::ProjectReference`] makes, and inventing a kind for them
/// would mean drawing an arrow on evidence this module has not established:
/// `frontendDist` names a *build output directory*, not a project, and matching
/// it back to the project that produces it needs a build graph nothing here has.
/// So the honest output is prose that names the file and the values, costs no
/// arrow, and converts a silent gap into a visible one.
///
/// # Why this is not a framework, and not a Tauri special case either
///
/// The shape considered and rejected was a registry of "file classes carrying
/// relations", with a trait and a table. There is one entry, and a framework
/// with one entry is a guess about the second. The shape taken instead is one
/// recogniser function per file class ([`tauri_relations`]) behind a loop that
/// knows only how to say *this file names these relations and none of them were
/// drawn* — so the vocabulary is already general, and a second class (a
/// `docker-compose.yml`, a `.slnf`) is a second recogniser and one more line
/// here, not a new concept. What is deliberately not done is inventing entries
/// for file classes nobody has verified: a warning about a blind spot is only
/// worth having if every word of it is checkable.
///
/// # Nothing is quoted that could be a value
///
/// This publishes strings out of a file into
/// [`ArchGraph::warnings`](ArchGraph::warnings), which is exported into the
/// mermaid and crosses IPC — the same surface [`super::signals`] guards. The
/// same discipline applies: a value is quoted only when it is shaped like a
/// relative path ([`quotable_path`]), and `beforeBuildCommand` — a command line,
/// the one field here that plausibly carries a credential — has its *key* named
/// and its text never repeated.
fn unexpressible_relations(workspace: &Workspace, builder: &mut Builder) {
    // Beside a scanned project, rather than a fresh walk of the tree: these
    // files sit next to the manifest of the project they configure, the scan
    // already found those directories, and iterating them means this rule can
    // never disagree with `source_walker` about what is in the workspace.
    let mut seen = BTreeSet::new();
    for project in &workspace.projects {
        let path = project.dir.join("tauri.conf.json");
        if !seen.insert(path.clone()) || !path.is_file() {
            continue;
        }
        let relative = relative_to_root(&workspace.root, &path);

        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => {
                builder.warn(format!(
                    "{}: {relative} declares relations between parts of this workspace and \
                     could not be read, so nothing is known about them: {e}",
                    project.name
                ));
                continue;
            }
        };
        let config: serde_json::Value = match serde_json::from_str(&text) {
            Ok(config) => config,
            Err(e) => {
                builder.warn(format!(
                    "{}: {relative} declares relations between parts of this workspace and \
                     could not be parsed, so nothing is known about them: {e}",
                    project.name
                ));
                continue;
            }
        };

        let relations = tauri_relations(&config);
        if relations.is_empty() {
            // The claim is about relations, not about the file existing. A
            // config naming none of these keys names no relation, so nothing
            // went undrawn and there is nothing to report.
            continue;
        }

        builder.warn(format!(
            "{}: {relative} names relations between parts of this workspace that this graph \
             has no edge kind for, so they were not drawn — {}. A frontend reached by \
             run-time IPC and a file copied into a bundle are not compile-time references, \
             and drawing either as an arrow would claim more than the file says",
            project.name,
            relations.join("; ")
        ));
    }
}

/// The relations a parsed `tauri.conf.json` names, each as a clause naming the
/// key it came from.
///
/// Only the three keys that state a relation to something *else in this
/// repository* are read. `devUrl`, the window list and the identifier describe
/// the app itself and are nobody's missing arrow.
fn tauri_relations(config: &serde_json::Value) -> Vec<String> {
    let mut relations = Vec::new();

    if let Some(dist) = config["build"]["frontendDist"].as_str() {
        relations.push(match quotable_path(dist) {
            Some(quoted) => {
                format!("it bundles the frontend built at {quoted} (build.frontendDist)")
            }
            None => "it bundles a frontend whose location (build.frontendDist) is not quoted \
                     here, because the value is not shaped like a relative path and could \
                     carry anything"
                .to_string(),
        });
    }

    // Tauri accepts both spellings, and reading only one would make this
    // warning's presence depend on a formatting choice. The map form's *keys*
    // are the sources in this repository; its values are destinations inside
    // the installer and name nothing here.
    let resources = &config["bundle"]["resources"];
    let listed: Vec<&str> = match (resources.as_array(), resources.as_object()) {
        (Some(array), _) => array.iter().filter_map(|v| v.as_str()).collect(),
        (_, Some(map)) => map.keys().map(String::as_str).collect(),
        _ => Vec::new(),
    };
    if !listed.is_empty() {
        let quoted: Vec<String> = listed.iter().filter_map(|r| quotable_path(r)).collect();
        relations.push(match quoted.is_empty() {
            false => format!(
                "it ships {} as bundled resources (bundle.resources)",
                quoted.join(", ")
            ),
            true => "it ships bundled resources whose paths (bundle.resources) are not quoted \
                     here, because none of them is shaped like a relative path"
                .to_string(),
        });
    }

    if config["build"]["beforeBuildCommand"]
        .as_str()
        .is_some_and(|command| !command.trim().is_empty())
    {
        relations.push(
            "it runs another part of this workspace to produce that frontend \
             (build.beforeBuildCommand, whose text is not quoted here because a command line \
             can carry a credential)"
                .to_string(),
        );
    }

    relations
}

/// A value quoted for the reader, or `None` when quoting it would republish
/// something that is not a path.
///
/// The screen is [`super::signals::framework::looks_like_a_value`] — the guard
/// the component phase already applies to every string it publishes, catching
/// `=`, `;`, `://`, a `host:port` and anything absurdly long — with two
/// path-specific refusals layered on top: a bare `user@host`, which that guard
/// does not catch because it is not a shape a *label* takes, and [`is_rooted`],
/// because an absolute path names a machine rather than a relation. Reusing the
/// shared guard rather than restating it is deliberate; the copy that module's
/// own documentation records drifting is exactly what a fourth hand-written
/// version would become.
///
/// Blunt about false positives on purpose: refusing a legitimate path costs the
/// reader a string they can find by opening the file the warning already names,
/// and the warning still reports that the relation is there.
pub(super) fn quotable_path(value: &str) -> Option<String> {
    let bad_shape = value.trim().is_empty()
        || super::signals::framework::looks_like_a_value(value)
        || value.contains('@')
        || is_rooted(value);
    (!bad_shape).then(|| format!("'{value}'"))
}

// ---------------------------------------------------------------------------
// Cargo
// ---------------------------------------------------------------------------

/// Read and parse every `Cargo.toml` in the workspace, once.
///
/// Both cargo rules need the same files: a manifest carries its crate's `path`
/// dependencies *and*, if it is a workspace root, the membership globs. Parsing
/// in one place is not only cheaper, it is what keeps a broken manifest to a
/// single warning — two independent passes over the same file would each have
/// its own complaint about it.
///
/// # Why the disk and not the scanned projects
///
/// A Cargo workspace root is very often a *virtual* manifest — a `[workspace]`
/// with no `[package]`, which is exactly the shape of this repository's own root
/// file — and the scan deliberately produces no project for one, because there
/// is no crate there to run or test. Iterating projects would therefore miss the
/// only file that says what the workspace contains. The walk uses
/// [`workspace::source_walker`](crate::workspace::source_walker), the same
/// walker the scan itself used, so the two cannot disagree about `SKIP_DIRS`,
/// depth or nested checkouts.
///
/// Manifests the scan already marked [`Project::unreadable`] are skipped without
/// being read: they would fail here in the same way, and
/// [`unreadable_projects`] has already reported them with the scan's own reason.
fn cargo_manifests(
    workspace: &Workspace,
    builder: &mut Builder,
) -> Vec<(PathBuf, cargo::CargoManifest)> {
    let unreadable: BTreeSet<&Path> = workspace
        .projects
        .iter()
        .filter(|p| p.unreadable.is_some())
        .map(|p| p.manifest_path.as_path())
        .collect();

    let mut manifests = Vec::new();
    for entry in crate::workspace::source_walker(&workspace.root).flatten() {
        if !entry.file_type().is_file() || entry.file_name() != "Cargo.toml" {
            continue;
        }
        let path = entry.path();
        if unreadable.contains(path) {
            continue;
        }

        let relative = relative_to_root(&workspace.root, path);
        // "Could not read" and "could not parse" are kept apart for the same
        // reason they are in `node_dependencies`: one is a missing or locked
        // file and the other is a syntax error, and they need different things
        // from the person reading the warning.
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                builder.warn(format!(
                    "{relative} could not be read, so neither the dependencies nor the \
                     workspace membership it declares are in this graph: {e}"
                ));
                continue;
            }
        };

        match cargo::parse(&text) {
            Some(manifest) => manifests.push((path.to_path_buf(), manifest)),
            None => {
                // `cargo::parse` returns `None` for exactly one reason — the
                // TOML would not parse — so a second parse describes the same
                // failure, and it names a line and column the user can go to.
                // This mirrors `workspace::scan_cargo_project`, which cannot
                // hand its message over because a virtual manifest never
                // becomes a project at all.
                let reason = toml::from_str::<toml::Table>(&text)
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "the file is not valid TOML".to_string());
                builder.warn(format!(
                    "{relative} could not be parsed, so neither the dependencies nor the \
                     workspace membership it declares are in this graph: {reason}"
                ));
            }
        }
    }

    // The walk is already deterministic, but nothing downstream should have to
    // know that: both rules below iterate this list and emit warnings in order.
    manifests.sort_by(|a, b| a.0.cmp(&b.0));
    manifests
}

/// Turn every `{ path = ... }` dependency into an edge, an external node, or a
/// warning.
///
/// # Resolution is by location, not by name
///
/// This follows [`dotnet_references`] rather than [`node_dependencies`], because
/// a cargo path dependency states *where* the crate is: the `path` value is a
/// directory relative to the referring manifest, and the manifest inside it is
/// `<dir>/Cargo.toml`. The crate's name is carried too, and is deliberately not
/// used to resolve anything — `cargo` itself resolves by path here, and matching
/// on the name as well would only ever add false positives.
///
/// Candidates are restricted to `ecosystem == "cargo"`, the same rule that keeps
/// an npm dependency from matching a .NET project. It is not hypothetical here:
/// a declarative adapter that detects `Cargo.toml` — the shipped
/// `examples/adapters/cargo-nextest.toml` does — produces a project whose
/// manifest path ends in `Cargo.toml` and whose ecosystem is the adapter's,
/// whenever no built-in crate claimed that directory. Resolving onto it would
/// draw an arrow at a configuration source and call it a crate.
///
/// # Dev- and build-dependencies are drawn, undistinguished
///
/// [`cargo::DependencyKind`] preserves the three sections because they make
/// different claims, and the decision about what to do with that belongs here.
/// Dropping the non-normal ones would hide real structure — a test-support crate
/// half the workspace depends on is part of the architecture — so all three are
/// drawn. They collapse onto one [`EdgeKind::ProjectReference`] because there is
/// no kind that means "test only", and inventing one is not a change this
/// function can make on its own: [`EdgeKind`] crosses IPC, `types.ts` mirrors it
/// by hand and [`super::mermaid`] chooses an arrow style per kind. The claim the
/// single arrow makes is the weaker one both sections support — *this crate
/// depends on that one at build time* — rather than the stronger "ships inside",
/// which only a normal dependency would justify.
fn cargo_dependencies(
    workspace: &Workspace,
    manifests: &[(PathBuf, cargo::CargoManifest)],
    ids: &NodeIds,
    builder: &mut Builder,
) {
    let crates: BTreeMap<&Path, &Project> = workspace
        .projects
        .iter()
        .filter(|p| p.ecosystem == "cargo")
        .map(|p| (p.manifest_path.as_path(), p))
        .collect();

    for (manifest_path, manifest) in manifests {
        // A manifest with no crate behind it — a virtual workspace root, or one
        // a declarative adapter claimed — has no node to draw an arrow out of.
        // It also declares no dependencies: `[dependencies]` in a virtual
        // manifest is not something cargo accepts.
        let Some(project) = crates.get(manifest_path.as_path()) else {
            continue;
        };

        for dependency in &manifest.path_dependencies {
            let raw = dependency.path.as_str();

            // A rooted path is not resolvable from here at all — `/shared` is
            // the root of the current drive and `C:\shared` names a specific
            // one — so joining it onto the referring crate's directory would
            // forge a path landing *inside* the workspace and draw a confident
            // arrow at whatever sits there. The forged path passes the
            // `starts_with(root)` guard honestly, so nothing downstream could
            // catch it. Exactly the mistake `dotnet_references` learned.
            if is_rooted(raw) {
                let slashed = raw.replace('\\', "/");
                let id = format!("external:{slashed}");
                builder.add_node(ArchNode {
                    label: file_stem(Path::new(&slashed)),
                    id: id.clone(),
                    kind: ArchKind::External,
                    project_id: None,
                    // No path, unlike a relative external: the only honest
                    // answer here is the machine-specific string the manifest
                    // names, which is what `ArchNode::path` promises not to be.
                    path: None,
                    ecosystem: None,
                });
                builder.add_edge(ids.id_of(project), &id, EdgeKind::ProjectReference);
                builder.warn(format!(
                    "{}: path dependency '{}' names '{raw}', an absolute path which \
                     cannot be located relative to the workspace; drawn as an external \
                     component",
                    project.name, dependency.name
                ));
                continue;
            }

            let dir = resolve_lexically(&project.dir, raw);

            if !dir.starts_with(&workspace.root) {
                let relative = relative_to_root(&workspace.root, &dir);
                let id = format!("external:{relative}");
                builder.add_node(ArchNode {
                    // The crate's *directory*, because that is what the
                    // dependency names and what a reader recognises. Taking the
                    // stem of the manifest instead would label every external
                    // crate in the diagram `Cargo`.
                    label: file_stem(&dir),
                    id: id.clone(),
                    kind: ArchKind::External,
                    project_id: None,
                    path: Some(PathBuf::from(relative)),
                    ecosystem: None,
                });
                builder.add_edge(ids.id_of(project), &id, EdgeKind::ProjectReference);
                builder.warn(format!(
                    "{}: path dependency '{}' points at '{raw}', which is outside the \
                     workspace; drawn as an external component",
                    project.name, dependency.name
                ));
                continue;
            }

            let target_manifest = dir.join("Cargo.toml");
            if let Some(target) = crates.get(target_manifest.as_path()) {
                builder.add_edge(
                    ids.id_of(project),
                    ids.id_of(target),
                    EdgeKind::ProjectReference,
                );
                continue;
            }

            // Inside the scanned area and matching nothing: a broken manifest,
            // not a component the diagram is missing. No node is invented for
            // it, because an `External` box would assert that something exists
            // at a path where nothing does.
            let near_miss = crates
                .keys()
                .find(|candidate| {
                    candidate
                        .as_os_str()
                        .eq_ignore_ascii_case(target_manifest.as_os_str())
                })
                .map(|candidate| relative_to_root(&workspace.root, candidate));

            builder.warn(match near_miss {
                // Matching case-insensitively would be right on NTFS and wrong
                // on a case-sensitive filesystem, and this code cannot tell
                // which it is looking at. Naming the near miss leaves the
                // decision with the person who can check.
                Some(candidate) => format!(
                    "{}: path dependency '{}' points at '{raw}', where the scan found no \
                     crate, but {candidate} differs only in casing",
                    project.name, dependency.name
                ),
                None => format!(
                    "{}: path dependency '{}' points at '{raw}', where the scan found no \
                     crate",
                    project.name, dependency.name
                ),
            });
        }
    }
}

/// Expand every `[workspace] members` list onto the crates the scan already
/// found.
///
/// The patterns are matched against **discovered project directories**, never
/// against the filesystem, for the reason [`npm_workspace_members`] gives:
/// walking the disk again would mean re-deciding what counts as a project and
/// would drift from the scan's own rules the first time any of them changed.
/// `literal_separator` is enabled so `*` stops at a path separator, which is
/// what cargo means by `crates/*`.
///
/// Every root is expanded, not just the workspace's own — a repository can hold
/// a cargo workspace in a subdirectory, and it is that manifest, not the top of
/// the repository, that lists the members. The paths are relative to the
/// manifest's own directory, which is what cargo resolves them against.
///
/// Three things are deliberate:
///
/// * **`exclude` is subtracted.** `members = ["crates/*"]` with `exclude =
///   ["crates/legacy"]` matches a directory that is explicitly *not* a member,
///   and drawing it inside the container would state the opposite of what the
///   manifest says. The excluded crate keeps its own box — it is still a crate.
/// * **A pattern that matched no discovered crate is reported.** A membership
///   list naming something the scan never found is the same "looks complete,
///   quietly missing" failure a dropped reference is. Matching is judged before
///   exclusion, so a crate that was deliberately excluded does not also produce
///   a complaint that its pattern found nothing.
/// * **The root package is a member of its own workspace.** Cargo makes it one
///   automatically and no `members` entry names it, so the only evidence is the
///   `[package]` table sitting beside the `[workspace]` one. The container and
///   the crate are two nodes because the file plays two roles.
fn cargo_workspace_members(
    workspace: &Workspace,
    manifests: &[(PathBuf, cargo::CargoManifest)],
    ids: &NodeIds,
    builder: &mut Builder,
) {
    let crates: Vec<&Project> = workspace
        .projects
        .iter()
        .filter(|p| p.ecosystem == "cargo")
        .collect();

    for (manifest_path, manifest) in manifests {
        if !manifest.is_workspace_root || manifest.workspace_members.is_empty() {
            continue;
        }
        let Some(dir) = manifest_path.parent() else {
            continue;
        };
        let relative = relative_to_root(&workspace.root, manifest_path);

        let (include, patterns) =
            cargo_globs(&manifest.workspace_members, &relative, "members", builder);
        let (exclude, _) = cargo_globs(&manifest.workspace_exclude, &relative, "exclude", builder);

        let mut members = Vec::new();
        let mut matched = vec![false; patterns.len()];
        for project in &crates {
            // Relative to the *manifest's* directory, which is what cargo
            // resolves member paths against. A crate outside it comes back
            // `../`-prefixed and matches nothing, which is correct.
            let member_path = relative_to_root(dir, &project.dir);
            if member_path.is_empty() {
                continue;
            }
            let hits = include.matches(&member_path);
            if hits.is_empty() {
                continue;
            }
            for hit in hits {
                matched[hit] = true;
            }
            if exclude.is_match(&member_path) {
                continue;
            }
            members.push(ids.id_of(project).to_string());
        }

        for (pattern, hit) in patterns.iter().zip(&matched) {
            if !hit {
                builder.warn(format!(
                    "{relative}: workspace member pattern '{pattern}' matched none of the \
                     crates the scan found, so whatever it names is absent from this graph"
                ));
            }
        }

        if manifest.package_name.is_some() {
            if let Some(root_crate) = crates.iter().find(|p| &p.manifest_path == manifest_path) {
                members.push(ids.id_of(root_crate).to_string());
            }
        }

        if members.is_empty() {
            continue;
        }

        let label = manifest
            .package_name
            .clone()
            .or_else(|| {
                (dir != workspace.root)
                    .then(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .flatten()
            })
            .unwrap_or_else(|| workspace.name.clone());

        let id = format!("workspace:{relative}");
        builder.add_node(ArchNode {
            id: id.clone(),
            label,
            kind: ArchKind::Solution,
            project_id: None,
            path: Some(PathBuf::from(relative)),
            ecosystem: Some("cargo".into()),
        });
        for member in members {
            builder.add_edge(&id, &member, EdgeKind::Contains);
        }
    }
}

/// Build a glob set out of one `[workspace]` list, reporting the patterns that
/// would not compile and returning the ones that did.
///
/// The surviving patterns are returned alongside the set because
/// [`globset::GlobSet::matches`] answers by *index into the set*, and the
/// unmatched-pattern warning needs to name the pattern rather than a number. A
/// malformed glob costs that pattern's members and nothing else; a set that
/// will not build at all costs the whole list, and returning an empty set with
/// no patterns keeps the two lists in step rather than reporting every pattern
/// as unmatched.
fn cargo_globs(
    patterns: &[String],
    manifest: &str,
    list: &str,
    builder: &mut Builder,
) -> (globset::GlobSet, Vec<String>) {
    let mut set = globset::GlobSetBuilder::new();
    let mut kept = Vec::new();
    for pattern in patterns {
        match globset::GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
        {
            Ok(glob) => {
                set.add(glob);
                kept.push(pattern.clone());
            }
            Err(e) => builder.warn(format!(
                "{manifest}: workspace {list} pattern '{pattern}' could not be read: {e}"
            )),
        }
    }
    match set.build() {
        Ok(built) => (built, kept),
        Err(_) => (globset::GlobSet::empty(), Vec::new()),
    }
}

/// Expand the workspace root's `workspaces` globs onto the projects the scan
/// already found — unless a `pnpm-workspace.yaml` says pnpm owns this
/// workspace, in which case nothing is drawn and the reason is reported.
///
/// The patterns are matched against **discovered project directories**, never
/// against the filesystem. Walking the disk again would mean re-deciding what
/// counts as a project, and would drift from the scan's own rules —
/// `SKIP_DIRS`, `MAX_DEPTH` and the exclusion of nested checkouts — the first
/// time any of them changed. `literal_separator` is enabled so that `*` stops
/// at a path separator, which is what npm, pnpm and Yarn all mean by it;
/// without it `packages/*` would swallow `packages/a/b` and invent members.
///
/// Only the root `package.json` is consulted; a nested workspace root is out of
/// scope. See [`pnpm_notice`] for why a pnpm workspace draws nothing at all.
fn npm_workspace_members(workspace: &Workspace, ids: &NodeIds, builder: &mut Builder) {
    let manifest = workspace.root.join("package.json");
    let pkg = std::fs::read_to_string(&manifest)
        .ok()
        .as_deref()
        .and_then(node::parse_package_json);
    let globs = pkg.as_ref().map(node::workspace_globs).unwrap_or_default();

    // Checked before anything is drawn, not after: in a pnpm workspace the
    // `workspaces` key is not this repository's membership list at all.
    if workspace.root.join("pnpm-workspace.yaml").exists() {
        builder.warn(pnpm_notice(&globs));
        return;
    }

    let Some(pkg) = pkg else {
        return;
    };

    let mut patterns = globset::GlobSetBuilder::new();
    let mut any = false;
    for pattern in globs {
        match globset::GlobBuilder::new(&pattern)
            .literal_separator(true)
            .build()
        {
            Ok(glob) => {
                patterns.add(glob);
                any = true;
            }
            // A malformed glob costs that pattern's members and nothing else.
            Err(e) => builder.warn(format!(
                "workspace pattern '{pattern}' could not be read: {e}"
            )),
        }
    }
    if !any {
        return;
    }
    let Ok(set) = patterns.build() else {
        return;
    };

    let id = "workspace:package.json".to_string();
    let label = pkg.name.clone().unwrap_or_else(|| workspace.name.clone());
    let mut members = Vec::new();

    for project in workspace.projects.iter().filter(|p| p.ecosystem == "node") {
        let relative = relative_to_root(&workspace.root, &project.dir);
        if !relative.is_empty() && set.is_match(&relative) {
            members.push(ids.id_of(project).to_string());
        }
    }

    if members.is_empty() {
        return;
    }

    builder.add_node(ArchNode {
        id: id.clone(),
        label,
        kind: ArchKind::Solution,
        project_id: None,
        path: Some(PathBuf::from("package.json")),
        ecosystem: Some("node".into()),
    });
    for member in members {
        builder.add_edge(&id, &member, EdgeKind::Contains);
    }
}

/// What to say when a `pnpm-workspace.yaml` is present, given the `workspaces`
/// globs the root `package.json` declares alongside it.
///
/// pnpm keeps its member globs in a YAML file of its own and does not read the
/// `workspaces` key in `package.json`. Confirmed against pnpm 10.14.0 rather
/// than asserted from memory: with both files present and disagreeing,
/// `pnpm list -r` returned exactly the members `pnpm-workspace.yaml` listed and
/// none of the ones only `package.json` listed, and pnpm printed
/// `The "workspaces" field in package.json is not supported by pnpm`.
///
/// So in a pnpm workspace the two lists are not two views of one membership —
/// one of them is simply not membership. Expanding the `package.json` globs
/// there drew a container labelled from the ignored file, holding whichever
/// projects that file happened to name, while real pnpm members sat outside it
/// as free-floating boxes. That is not an incomplete picture, it is a confident
/// wrong one, and the governing rule of this module says a wrong answer is much
/// worse than no answer. So nothing is drawn.
///
/// The alternative considered was drawing the boxes with a caveat naming the
/// file they came from. It was rejected on two counts: the caveat would have to
/// survive into the rendered diagram to do any good, and the renderer is not
/// this module's to change; and a reader looking at a picture believes the
/// picture, not the footnote. Reading the YAML properly is the real fix and
/// needs a dependency this crate does not have — until then, silence about
/// membership plus a warning that says exactly which lists went unread is the
/// honest position.
fn pnpm_notice(globs: &[String]) -> String {
    let mut notice = "pnpm-workspace.yaml declares this workspace's members, but reading YAML \
                      would need a dependency this crate does not have, so no containment was \
                      drawn from it"
        .to_string();

    if !globs.is_empty() {
        let quoted: Vec<String> = globs.iter().map(|g| format!("'{g}'")).collect();
        notice.push_str(&format!(
            "; the 'workspaces' key in package.json ({}) was not drawn either, because \
             pnpm does not read that key — it is not this workspace's membership list, \
             and boxes built from it would put real members outside the container",
            quoted.join(", ")
        ));
    }

    notice
}

// ---------------------------------------------------------------------------
// Solutions
// ---------------------------------------------------------------------------

/// Turn each solution's grouping into containment.
///
/// A solution says which projects ship together. It never says which depends
/// on which — two projects can sit in the same solution folder and know
/// nothing about each other — so this produces [`EdgeKind::Contains`] and
/// nothing else. Reading anything stronger out of a `.sln` would be inventing
/// dependencies out of filing.
///
/// Nested folders are chained (`solution -> src -> src/core -> project`) using
/// only the folder path the solution file itself spells out, so the chain is
/// derived rather than assumed.
fn solution_containment(workspace: &Workspace, ids: &NodeIds, builder: &mut Builder) {
    let by_manifest: BTreeMap<&PathBuf, &Project> = workspace
        .projects
        .iter()
        .map(|p| (&p.manifest_path, p))
        .collect();

    for solution in &workspace.solutions {
        let solution_id = format!("solution:{}", to_slash(&solution.path));
        builder.add_node(ArchNode {
            id: solution_id.clone(),
            label: solution.name.clone(),
            kind: ArchKind::Solution,
            project_id: None,
            path: Some(PathBuf::from(to_slash(&solution.path))),
            ecosystem: Some("dotnet".into()),
        });

        for member in &solution.projects {
            let absolute = workspace.root.join(&member.path);
            let Some(project) = by_manifest.get(&absolute) else {
                // Every other warning in this module quotes a path exactly as
                // the file spells it, so it can be grepped for. This one cannot:
                // `solution::resolve` replaces `\` with `/` while parsing, and
                // the raw spelling is gone before the graph is handed the
                // member. Saying so costs a clause and stops a reader grepping
                // a `.sln` full of backslashes for a forward-slashed path and
                // concluding the warning is spurious.
                builder.warn(format!(
                    "{}: solution member '{}' matches no project the scan found \
                     (path separators shown normalised to '/'; the solution file may \
                     spell it with '\\')",
                    solution.name,
                    to_slash(&member.path)
                ));
                continue;
            };

            let mut parent = solution_id.clone();
            if let Some(folder) = member.folder.as_deref() {
                let mut prefix = String::new();
                for segment in folder.split('/').filter(|s| !s.is_empty()) {
                    if !prefix.is_empty() {
                        prefix.push('/');
                    }
                    prefix.push_str(segment);
                    let folder_id = format!("{solution_id}#{prefix}");
                    builder.add_node(ArchNode {
                        id: folder_id.clone(),
                        label: segment.to_string(),
                        kind: ArchKind::SolutionFolder,
                        project_id: None,
                        path: None,
                        ecosystem: None,
                    });
                    builder.add_edge(&parent, &folder_id, EdgeKind::Contains);
                    parent = folder_id;
                }
            }

            builder.add_edge(&parent, ids.id_of(project), EdgeKind::Contains);
        }
    }
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Whether a raw reference names a location from a root rather than from the
/// referring project.
///
/// Decided on the string, not with [`Path`], because the answer must not depend
/// on the platform doing the reading: `C:\Shared\Shared.csproj` is rooted no
/// matter which machine parses the `.csproj`, and [`Path::is_absolute`] would
/// call it relative on Linux. Three spellings count, and they are all rooted in
/// MSBuild's own terms:
///
/// * a leading `/` or `\` — the root of the current drive;
/// * a drive prefix `X:` — a specific drive, whether or not a separator
///   follows, since `C:Shared` is relative to that drive's current directory
///   and is no more locatable from here than `C:\Shared` is;
/// * a UNC `\\server\share`, which the leading-separator case already covers.
fn is_rooted(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    match bytes {
        [b'/' | b'\\', ..] => true,
        [drive, b':', ..] => drive.is_ascii_alphabetic(),
        _ => false,
    }
}

/// Join a raw reference onto the referring project's directory, resolving `.`
/// and `..` **lexically**.
///
/// Deliberately not [`std::fs::canonicalize`]. Canonicalising requires the
/// target to exist, and a reference to a file that is not there is precisely
/// the case this module has to report rather than fail on — using the
/// filesystem would turn the most interesting answer into an error. Working
/// lexically also keeps the result comparable with the paths the scan
/// produced, which were built the same way.
///
/// Both separators are accepted regardless of platform: `.csproj` files are
/// written on Windows and read on Linux CI constantly, and the `\` in them is
/// a separator on either.
fn resolve_lexically(base: &Path, raw: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    for segment in raw.replace('\\', "/").split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Render a path relative to the workspace root with forward slashes, walking
/// up with `..` when the path sits outside the root.
///
/// Forward slashes unconditionally, on every platform: these strings end up in
/// node ids and in exported diagrams, and a graph whose ids depend on which
/// machine derived it cannot be compared, stored or shared.
pub(super) fn relative_to_root(root: &Path, path: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(root) {
        return to_slash(relative);
    }

    let root_parts: Vec<_> = root.components().collect();
    let path_parts: Vec<_> = path.components().collect();
    let shared = root_parts
        .iter()
        .zip(&path_parts)
        .take_while(|(a, b)| a == b)
        .count();

    // No shared prefix at all means a different Windows drive (or a
    // fundamentally unrelated path). `..` cannot express that, so the absolute
    // path is the honest answer.
    if shared == 0 {
        return to_slash(path);
    }

    let mut parts = vec!["..".to_string(); root_parts.len() - shared];
    parts.extend(
        path_parts[shared..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

fn to_slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| to_slash(path))
}
