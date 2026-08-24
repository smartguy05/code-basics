//! Rendering an [`ArchGraph`](super::graph::ArchGraph) to Mermaid source, and
//! validating Mermaid source that did not come from here.
//!
//! Kept apart from [`super::graph`] so that deriving the graph and drawing it
//! are independently testable: a rendering bug must never be able to look like
//! a wrong edge, and a wrong edge must never be excusable as a rendering bug.
//! Nothing in this module consults the filesystem or reinterprets an edge —
//! whatever the graph says is what gets drawn, and anything the renderer
//! cannot express is said out loud in a `%%` comment rather than quietly
//! dropped, because a diagram that looks complete and is missing an arrow is
//! the failure this whole feature exists to avoid.
//!
//! # Two functions that have to agree
//!
//! [`render`] produces Mermaid; [`validate`] decides whether Mermaid is
//! allowed to reach the renderer. They are written together because a later
//! phase lets a coding agent write a diagram and a user edit it, at which
//! point [`validate`] stops being a linter and becomes the boundary between a
//! file some agent wrote and a renderer running inside the app. The invariant
//! `validate(render(g)) == Ok(())` is pinned by a test over deliberately
//! hostile graphs: if it ever fails, one of the two is wrong, and shipping the
//! pair would mean either emitting diagrams the app refuses to draw or
//! accepting constructs the renderer never produces.
//!
//! # What the CSP spike settled
//!
//! Mermaid 11.16.1 renders under the app's policy (`default-src 'self'`, no
//! `unsafe-eval`) **only** with a top-level `htmlLabels: false`, which makes
//! every label a plain SVG `<text>` run. Two consequences are baked in here.
//! Labels are kept simple — no markup is emitted into them and the only
//! escape used is Mermaid's own `#quot;` — because there is no HTML in a label
//! to hide anything in. And the diagram-type allowlist in [`validate`] is a
//! CSP guard as much as a syntax check: families outside it pull renderer
//! bundles the spike never exercised, so they are refused rather than trusted.
//!
//! # What an `init` directive can and cannot do
//!
//! Checked against the shipped library rather than assumed, because the two
//! halves land very differently. `securityLevel` is listed in Mermaid's
//! `secure` key set (`defaultConfig.secure`), and `sanitize` deletes every
//! secure key out of a directive's arguments before applying it, so a
//! directive in a file **cannot** loosen the security level. `htmlLabels` is
//! not in that set and *is* a known config key, so `%%{init: {"flowchart":
//! {"htmlLabels": true}}}%%` is applied — which is precisely the setting every
//! argument about labels above rests on. That is why a directive is refused
//! wherever it appears, and refused on the raw text: `directiveRegex` is run
//! over the source with no awareness of quoting or of this module's idea of
//! where the diagram starts.
//!
//! # Where this abstains
//!
//! The structural rules in [`validate`] parse the subset of flowchart syntax
//! this renderer emits, plus the shapes a person plausibly hand-writes. Where
//! that parser cannot tell two constructs apart it declines to check, never
//! rejects: a wrongly rejected diagram is a user staring at an error beside
//! code that is fine, which is worse than a diagram whose typo we missed. The
//! individual abstentions are documented at the rules that make them.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use specta::Type;

use super::graph::{ArchEdge, ArchGraph, ArchKind, ArchNode, Derivation, EdgeKind};

/// One level of indentation inside the diagram.
const INDENT: &str = "    ";

/// How deep containment is allowed to nest before it is drawn as arrows.
///
/// A derived graph nests as deeply as a solution's folders do, which is a
/// handful of levels. The cap exists for graphs that did not come from
/// [`super::graph::project_graph`] — an agent-written or hand-edited file can
/// describe a container chain of any length, and the renderer walks that chain
/// recursively. Sixteen is far past anything a human would draw and far short
/// of anything that could exhaust the stack.
const MAX_NESTING: usize = 16;

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Render a graph as Mermaid `flowchart` source.
///
/// The output is deterministic: the same graph renders byte-for-byte the same
/// string, because these diagrams are meant to be committed and a file that
/// reshuffles between runs produces a diff nobody can read.
///
/// Three things are deliberately visible in the output that a naive renderer
/// would hide. Every warning the graph collected becomes a `%%` comment, so
/// somebody reading the raw file sees what was abstained on rather than only
/// what was drawn. The [`Derivation`] is stated on the first line, because
/// "derived from the files on disk" and "a language model's opinion" are the
/// same picture with completely different standing. And an edge naming a node
/// the graph does not contain is dropped *and* reported — an arrow to a box
/// that does not exist would be a claim about nothing, but silently losing it
/// would be worse.
pub fn render(graph: &ArchGraph) -> String {
    let by_id: BTreeMap<&str, &ArchNode> = graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut notes: Vec<String> = graph.warnings.clone();

    let (parent, nested) = nesting(graph, &by_id, &mut notes);
    let labels = labels(graph);

    let mut children: BTreeMap<&str, Vec<&ArchNode>> = BTreeMap::new();
    for node in &graph.nodes {
        if let Some(owner) = parent.get(node.id.as_str()) {
            children.entry(owner).or_default().push(node);
        }
    }

    let mut key = Key::default();
    let mut body = String::new();
    for node in &graph.nodes {
        if !parent.contains_key(node.id.as_str()) {
            write_node(&mut body, node, &children, &labels, 1, &mut key);
        }
    }
    for e in &graph.edges {
        write_edge(&mut body, e, &by_id, &nested, &mut notes, &mut key);
    }

    if body.is_empty() {
        // `flowchart LR` with nothing under it draws as a blank rectangle,
        // which reads as a broken diagram rather than as an empty one. Saying
        // it in words costs one node and removes the ambiguity.
        body.push_str(INDENT);
        body.push_str("empty[\"Nothing to draw — this graph has no nodes.\"]\n");
    } else {
        write_legend(&mut body, &key);
    }

    let mut out = comment(&derivation_line(&graph.derivation));
    for note in dedup(notes) {
        out.push_str(&comment(&note));
    }
    out.push_str("flowchart LR\n");
    out.push_str(&body);
    out
}

/// Decide which containment edges become `subgraph` nesting.
///
/// Mermaid can nest a node inside exactly one subgraph, but a project can
/// belong to two solutions, so the two facts do not fit the same shape. The
/// first container to claim a node — first in the graph's own edge order,
/// which is sorted, so first is stable — nests it; every later claim is drawn
/// as an ordinary arrow. Dropping the later claims would produce a diagram
/// that looks complete while silently omitting a solution's membership.
///
/// Three assignments are refused. A container may not nest something that is
/// already its own ancestor, because the renderer walks the chain recursively
/// and a cycle — impossible from a scan, easy to write by hand — would never
/// return. A chain longer than [`MAX_NESTING`] is refused for the same reason.
/// And a containment edge carrying a label is never nested at all: nesting
/// draws the relationship but has nowhere to put its text, so the label would
/// be lost, and a label on a containment edge exists only when a person or an
/// agent wrote it deliberately.
fn nesting<'g>(
    graph: &'g ArchGraph,
    by_id: &BTreeMap<&'g str, &'g ArchNode>,
    notes: &mut Vec<String>,
) -> (BTreeMap<&'g str, &'g str>, BTreeSet<(&'g str, &'g str)>) {
    let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
    let mut nested: BTreeSet<(&str, &str)> = BTreeSet::new();

    for e in &graph.edges {
        if e.kind != EdgeKind::Contains || e.label.is_some() {
            continue;
        }
        let (Some(from), Some(to)) = (by_id.get(e.from.as_str()), by_id.get(e.to.as_str())) else {
            continue;
        };
        if !is_container(from.kind) || from.id == to.id {
            continue;
        }
        if parent.contains_key(to.id.as_str()) {
            continue;
        }

        let (depth, reaches_target) = ancestry(&parent, &from.id, &to.id);
        if reaches_target {
            notes.push(format!(
                "'{}' and '{}' contain each other; the second containment is drawn as an arrow",
                from.id, to.id
            ));
            continue;
        }
        if depth + 1 >= MAX_NESTING {
            notes.push(format!(
                "'{}' is nested more than {MAX_NESTING} levels deep; its contents are drawn as arrows",
                to.id
            ));
            continue;
        }

        parent.insert(&to.id, &from.id);
        nested.insert((&from.id, &to.id));
    }

    (parent, nested)
}

/// Walk up the containment chain from `start`, returning how many levels are
/// above it and whether `target` is one of them.
fn ancestry(parent: &BTreeMap<&str, &str>, start: &str, target: &str) -> (usize, bool) {
    let mut cursor = start;
    let mut depth = 0;
    while let Some(next) = parent.get(cursor) {
        if *next == target {
            return (depth, true);
        }
        cursor = next;
        depth += 1;
        // The map is acyclic by construction, so this is a belt-and-braces
        // bound rather than a live concern.
        if depth > parent.len() {
            break;
        }
    }
    (depth, start == target)
}

fn write_node(
    out: &mut String,
    node: &ArchNode,
    children: &BTreeMap<&str, Vec<&ArchNode>>,
    labels: &BTreeMap<&str, String>,
    depth: usize,
    key: &mut Key,
) {
    let indent = INDENT.repeat(depth);
    let id = mermaid_id(&node.id);
    let label = labels
        .get(node.id.as_str())
        .cloned()
        .unwrap_or_else(|| label_of(node));

    match children.get(node.id.as_str()) {
        // A container with members is a subgraph. A container with none is a
        // box: an empty subgraph draws as a titled void, which looks like a
        // rendering failure rather than like an empty solution.
        Some(members) if is_container(node.kind) => {
            out.push_str(&format!("{indent}subgraph {id}[\"{label}\"]\n"));
            for member in members {
                write_node(out, member, children, labels, depth + 1, key);
            }
            out.push_str(&format!("{indent}end\n"));
        }
        _ => {
            match node.kind {
                ArchKind::Project => key.project = true,
                ArchKind::External => key.external = true,
                ArchKind::Solution | ArchKind::SolutionFolder => key.container = true,
                ArchKind::Service => key.service = true,
                ArchKind::DataStore => key.data_store = true,
            }
            out.push_str(&format!("{indent}{}\n", shaped(&id, &label, node.kind)));
        }
    }
}

/// The text drawn in each node's box, keyed by [`ArchNode::id`].
///
/// Computed for the whole graph at once because of the one thing a per-node
/// function cannot see: two projects genuinely called `App` are correct data
/// and an ambiguous picture. Two identically labelled boxes are read as one
/// thing mentioned twice, or as a rendering bug — the same silent wrong answer
/// [`mermaid_id`] exists to prevent, except that here the graph is right and
/// only the drawing can fix it.
///
/// The path is appended **only to the labels that collide**. Appending it to
/// every box would trade a rare ambiguity for permanent noise: a diagram of
/// thirty projects reading `App (src/App/App.csproj)` thirty times is harder
/// to use than one reading `App`, and the path is not what the reader came
/// for. Solution folders have no path, so they fall back to their id, which is
/// unique by definition. Two nodes sharing both a label and a path would still
/// collide; nothing in [`super::graph`] can produce that, and inventing a
/// counter would be a distinction the data does not make.
fn labels(graph: &ArchGraph) -> BTreeMap<&str, String> {
    let mut labels: BTreeMap<&str, String> = graph
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), label_of(n)))
        .collect();

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for label in labels.values() {
        *counts.entry(label.as_str()).or_default() += 1;
    }
    let ambiguous: BTreeSet<String> = counts
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(label, _)| label.to_string())
        .collect();

    for node in &graph.nodes {
        let Some(label) = labels.get_mut(node.id.as_str()) else {
            continue;
        };
        if !ambiguous.contains(label.as_str()) {
            continue;
        }
        let qualifier = match &node.path {
            Some(path) => escape_label(&path.to_string_lossy().replace('\\', "/")),
            None => escape_label(&node.id),
        };
        *label = format!("{label} ({qualifier})");
    }
    labels
}

/// The box a node is drawn in.
///
/// The shapes carry the distinctions a reader cannot recover from the label: a
/// stadium marks something outside the workspace, which the scan never saw and
/// cannot be run, tested or opened, and a cylinder marks something that is not
/// code at all.
///
/// The cylinder is the one shape here that is not an arbitrary choice — it has
/// meant "stored data" in system diagrams since long before Mermaid — and it is
/// spent on [`ArchKind::DataStore`] because that is where the reader's
/// strongest existing intuition does the most work: a box they might click into
/// and a store there is no source for are the two things that must never be
/// confused. [`ArchKind::Service`] takes the rounded rectangle, which reads as
/// a near-neighbour of the plain project box, and that is exactly right: a
/// service *is* a project, with one extra thing declared about it.
fn shaped(id: &str, label: &str, kind: ArchKind) -> String {
    match kind {
        ArchKind::Project => format!("{id}[\"{label}\"]"),
        ArchKind::External => format!("{id}([\"{label}\"])"),
        ArchKind::Solution | ArchKind::SolutionFolder => format!("{id}[[\"{label}\"]]"),
        ArchKind::Service => format!("{id}(\"{label}\")"),
        ArchKind::DataStore => format!("{id}[(\"{label}\")]"),
    }
}

fn write_edge(
    out: &mut String,
    edge: &ArchEdge,
    by_id: &BTreeMap<&str, &ArchNode>,
    nested: &BTreeSet<(&str, &str)>,
    notes: &mut Vec<String>,
    key: &mut Key,
) {
    if !by_id.contains_key(edge.from.as_str()) || !by_id.contains_key(edge.to.as_str()) {
        notes.push(format!(
            "the edge from '{}' to '{}' names a node this graph does not contain, \
             so it was not drawn",
            edge.from, edge.to
        ));
        return;
    }
    if nested.contains(&(edge.from.as_str(), edge.to.as_str())) {
        // Already drawn, as the box around the box.
        return;
    }

    let arrow = match edge.kind {
        EdgeKind::ProjectReference => {
            key.reference = true;
            "-->"
        }
        EdgeKind::PackageDependency => {
            key.package = true;
            "-.->"
        }
        EdgeKind::Contains => {
            key.contains = true;
            "--o"
        }
        EdgeKind::DataAccess => {
            key.data_access = true;
            "==>"
        }
        EdgeKind::ServiceCall => {
            key.service_call = true;
            "--x"
        }
    };
    let from = mermaid_id(&edge.from);
    let to = mermaid_id(&edge.to);

    out.push_str(INDENT);
    match edge.label.as_deref().map(escape_label) {
        Some(label) if !label.is_empty() => {
            out.push_str(&format!("{from} {arrow}|\"{label}\"| {to}\n"));
        }
        _ => out.push_str(&format!("{from} {arrow} {to}\n")),
    }
}

/// Which symbols the diagram actually put on the page.
///
/// Filled in while writing, not derived from the graph, because the two differ:
/// a containment edge realised as nesting draws no arrow, and an edge naming a
/// missing node draws nothing at all. A key is only honest about the picture if
/// it is built from what was drawn.
///
/// Nesting is deliberately not one of these fields — see [`write_legend`].
#[derive(Default)]
struct Key {
    project: bool,
    external: bool,
    container: bool,
    service: bool,
    data_store: bool,
    reference: bool,
    package: bool,
    contains: bool,
    data_access: bool,
    service_call: bool,
}

impl Key {
    /// How many rows the key would draw.
    fn entries(&self) -> usize {
        [
            self.project,
            self.external,
            self.container,
            self.service,
            self.data_store,
            self.reference,
            self.package,
            self.contains,
            self.data_access,
            self.service_call,
        ]
        .into_iter()
        .filter(|drawn| *drawn)
        .count()
    }
}

/// Append a key explaining every symbol the diagram used, and no others.
///
/// These files are committed and exported, and an `.mmd` read on a pull request
/// has no tooltip, no sidebar and nothing else to say that a dotted arrow is an
/// npm dependency while a solid one is a `<ProjectReference>`. Without a key a
/// reader either guesses or flattens every arrow into "depends on", and "A
/// compiles against B" and "A and B ship together" are not the same claim —
/// which makes an unlabelled arrow exactly the confident wrong answer this
/// module is built to avoid. The key is drawn *inside* the diagram rather than
/// written beside it in the UI so that it survives the file leaving the app.
///
/// Only the symbols that were drawn are explained. A key listing a stadium in a
/// diagram with no external node sends the reader hunting for one, and the
/// entries are worth reading precisely because each is present.
///
/// Below two entries the key is dropped entirely. A key is a table of
/// contrasts — it earns its space by saying that *this* symbol is not *that*
/// one — and a table with one row has no contrast to draw: on a workspace of
/// unconnected projects it rendered as `legend_project["a project in this
/// workspace"]` beside three plain boxes, explaining a rectangle to a reader
/// already looking at three of them.
///
/// "No arrow styles present" is the other defensible rule and it is a
/// different one, because it also drops the two-shape case: a project and an
/// unconnected external, one plain box and one stadium. That case is kept
/// here. Nothing about a stadium says "outside this workspace", so a reader
/// without the key sees two projects where there is one project and one thing
/// the scan could not see into — the shapes carry the claim even when no arrow
/// does, and the row that names them is the only thing that decodes it. The
/// count, not the presence of an arrow, is therefore what decides.
///
/// The identifiers are safe against collision by construction: [`mermaid_id`]
/// prefixes every node with `n`, so no node identifier can begin with `legend`.
///
/// # Why the key is still inside the diagram, and what changed instead
///
/// Verified in the running app on this repository, the key used to take about
/// the top 40% of the canvas: larger than the `cb-app --> cb-core` it was
/// annotating, with two more boxes pushed below the fold. Mermaid lays a
/// subgraph out as a peer of the content, so a key written as one competes with
/// the picture instead of annotating it.
///
/// The obvious fix — suppress the key in the app and draw one in the UI chrome
/// — was rejected. The exported `.mmd` has to carry a key whatever the app
/// does, so that route does not replace this function, it *adds* a second
/// implementation beside it; and a UI-side key cannot honour the rule above it
/// ("only the symbols that were drawn") without re-deriving what was drawn from
/// the source it was handed. Two keys that can disagree about the same picture
/// is precisely the failure this feature exists to avoid, and the export is the
/// copy nobody is watching, so it is the one that would rot.
///
/// What competes for space is entirely inside this function: how many rows it
/// draws, how deep they nest, and how long the text in them is. All three were
/// cut, and `mermaid_tests.rs` pins them as a property rather than as one
/// example.
///
/// * **No row nests.** The old nesting entry drew a subgraph inside the key to
///   show a solution wrapped around its members — the single most expensive row
///   available, a container's padding around a box inside the key. It is also
///   the one symbol here whose meaning the drawing already carries: a titled box
///   around two boxes needs no key, and it is titled with the solution's own
///   name. Dropping it is why `Key` has no `nesting` field.
/// * **Each row names its symbol; it does not define it.** `a database, cache
///   or broker this workspace declares it speaks to` became `database, cache or
///   broker`, and the arrow labels lost their `A … B` sentences. The claim an
///   arrow makes is stated at [`ArchEdge`] and in the module docs, which is
///   where a reader who needs the full wording is; the box on the canvas has
///   room for a name.
fn write_legend(out: &mut String, key: &Key) {
    if key.entries() < 2 {
        return;
    }

    out.push_str(&format!("{INDENT}subgraph legend[\"Legend\"]\n"));
    let inner = INDENT.repeat(2);
    if key.project {
        out.push_str(&format!(
            "{inner}legend_project[\"project in this workspace\"]\n"
        ));
    }
    if key.external {
        out.push_str(&format!(
            "{inner}legend_external([\"outside this workspace\"])\n"
        ));
    }
    if key.container {
        out.push_str(&format!(
            "{inner}legend_container[[\"empty solution or folder\"]]\n"
        ));
    }
    if key.service {
        out.push_str(&format!(
            "{inner}legend_service(\"declares it serves HTTP\")\n"
        ));
    }
    if key.data_store {
        // Three nouns and no verb: the shape covers all three and says none of
        // them, and the claim — that the workspace *declares* a client, not
        // that it uses one — is the arrow's to make, not the box's.
        out.push_str(&format!(
            "{inner}legend_data_store[(\"database, cache or broker\")]\n"
        ));
    }

    // The arrows share one pair of boxes so that the key stays a key
    // rather than becoming a second diagram; Mermaid draws the parallel links
    // apart from each other. `A` and `B` are the subject and object every label
    // below is read against, which is what lets the labels drop them.
    let mut declared = false;
    for (arrow, meaning) in [
        (key.reference.then_some("-->"), "project reference"),
        (key.package.then_some("-.->"), "depends on the package"),
        // Not "containment that could not be drawn as a box": which of the two
        // notations a containment got is a fact about the layout, and a reader
        // looking at this arrow is asking what it means, not why it is not a
        // box. The why is in this function's docs.
        (key.contains.then_some("--o"), "holds"),
        // *Declares*, not *uses*: a client library reference in a manifest is
        // the whole of what was read.
        (key.data_access.then_some("==>"), "declares a client for"),
        // A runtime call, not a compile-time reference: the caller's
        // `AddHttpClient` base address matched the callee's launch profile.
        (key.service_call.then_some("--x"), "calls over HTTP"),
    ] {
        let Some(arrow) = arrow else { continue };
        if declared {
            out.push_str(&format!(
                "{inner}legend_from {arrow}|\"{meaning}\"| legend_to\n"
            ));
        } else {
            out.push_str(&format!(
                "{inner}legend_from[\"A\"] {arrow}|\"{meaning}\"| legend_to[\"B\"]\n"
            ));
            declared = true;
        }
    }
    out.push_str(&format!("{INDENT}end\n"));
}

fn is_container(kind: ArchKind) -> bool {
    matches!(kind, ArchKind::Solution | ArchKind::SolutionFolder)
}

/// Turn an [`ArchNode::id`] into a Mermaid identifier, injectively.
///
/// [`crate::model::Project::id`] is a workspace-relative path with its
/// separators replaced, and container ids carry `solution:`, `workspace:` and
/// `external:` prefixes, so they are full of characters a Mermaid identifier
/// cannot hold. Stripping them is the obvious fix and is wrong: `src/a-b` and
/// `src/a.b` would become one identifier, two projects would silently merge
/// into one box, and nothing about the diagram would look broken. Every
/// character outside `[A-Za-z0-9]` is therefore escaped as `_<hex>_` rather
/// than removed, which is reversible and so cannot collide — an escape always
/// contains an underscore and an unescaped run never does. The leading `n`
/// keeps the identifier away from a leading digit and from Mermaid's own
/// keywords.
///
/// `pub(crate)` so `mermaid_tests.rs` can pin its output into the committed
/// cross-language fixture the frontend's `mermaidIdOf` is held against — this
/// is the escaping the two implementations must agree on, character for
/// character.
pub(crate) fn mermaid_id(id: &str) -> String {
    let mut out = String::with_capacity(id.len() + 1);
    out.push('n');
    for ch in id.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('_');
            out.push_str(&format!("{:x}", ch as u32));
            out.push('_');
        }
    }
    out
}

fn label_of(node: &ArchNode) -> String {
    let label = escape_label(&node.label);
    if !label.is_empty() {
        return label;
    }
    // A node with no label still has to be identifiable, and its id is the
    // only other thing known about it.
    let id = escape_label(&node.id);
    if id.is_empty() {
        "unnamed".to_string()
    } else {
        id
    }
}

/// Make arbitrary text safe to sit inside a Mermaid quoted string.
///
/// Real project names contain `(`, `)`, `&`, `<`, `>`, `%`, `#` and every
/// unicode block, and all of them are ordinary text inside a quoted label —
/// Mermaid does not parse the inside of a string, and with `htmlLabels: false`
/// there is no HTML for markup-looking text to become. The only character that
/// can end the string early is the quote itself, so the quote is the only
/// character escaped, using Mermaid's own `#quot;`.
///
/// This is deliberately *not* an entity-encoder for the rest. Escaping `#`
/// would turn `C# Library` into visible noise in a .NET workspace, and
/// escaping the bracket characters would do the same to every project named
/// `Something (Legacy)`; the cost of over-escaping here is paid on every
/// diagram, and it buys nothing that quoting does not already provide. The one
/// accepted consequence is that a label containing the literal text `#quot;`
/// renders as a quote — a cosmetic loss on a string nobody types.
///
/// Newlines, tabs and other control characters collapse to single spaces
/// rather than becoming `<br/>`: a statement must stay on one line for the
/// diagram to parse, and injecting markup would contradict `htmlLabels:
/// false`.
///
/// The one non-cosmetic exception to "quoting is enough" is `%%`. Mermaid's
/// directive scanner runs over the raw source and does not know what a quoted
/// string is, so `%%{init: …}` inside a label is a directive that would be
/// applied — the one construct that can reach back and undo `htmlLabels:
/// false`. Adjacent percent signs are therefore separated here by the same
/// total rule [`comment`] uses. A project named `100%` is untouched; only a
/// name containing two `%` in a row is altered, and it is altered visibly.
fn escape_label(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("#quot;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    separate_percents(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Put a space between any two adjacent `%` characters.
///
/// `%%{` opens a Mermaid directive, and this is the total way to make one
/// impossible to spell: doing it while writing, rather than by replacing `%%`
/// afterwards, is what makes it total, because replacement on `%%%` leaves a
/// `%%` behind.
fn separate_percents(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous_was_percent = false;
    for ch in text.chars() {
        if ch == '%' && previous_was_percent {
            out.push(' ');
        }
        previous_was_percent = ch == '%';
        out.push(ch);
    }
    out
}

/// Render one line of free text as a Mermaid comment.
///
/// Warnings are copied verbatim out of manifests, and a derivation can name an
/// agent, so this text is not under this module's control. Two things have to
/// be neutralised. A newline would end the comment and let the rest of the
/// text be parsed as syntax. And `%%{` opens a Mermaid *directive*, which can
/// re-enable `htmlLabels` — the setting every claim about labels in this module
/// rests on — so no two `%` characters are ever emitted adjacent. A directive
/// cannot loosen `securityLevel`, which Mermaid keeps in its `secure` key set
/// and strips out of directive arguments; `htmlLabels` is not in that set, so
/// that half is real and is why this matters. Comments are not exempt: Mermaid
/// looks for directives before it strips comments, so a directive spelled
/// inside a `%%` comment is still a directive.
fn comment(text: &str) -> String {
    let stripped: String = text
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    format!("%% {}\n", separate_percents(&stripped))
}

fn derivation_line(derivation: &Derivation) -> String {
    match derivation {
        Derivation::Derived { scanner } => {
            format!("Derived by code-basics from the files on disk (scanner version {scanner}).")
        }
        Derivation::Inferred { agent } => {
            format!("Inferred by the coding agent '{agent}' — not derived from the files on disk.")
        }
        Derivation::User => "Drawn by hand — not derived from the files on disk.".to_string(),
    }
}

fn dedup(notes: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    notes
        .into_iter()
        .filter(|n| seen.insert(n.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Which rule a piece of Mermaid source broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ValidationRule {
    /// More than one ```` ```mermaid ```` fence, or one that is never closed.
    FenceCount,
    /// The diagram does not open with an allowlisted diagram type.
    DiagramType,
    /// The diagram contains something that can execute, navigate, or
    /// reconfigure the renderer.
    ForbiddenDirective,
    /// A `subgraph` without an `end`, or an `end` without a `subgraph`.
    UnbalancedSubgraph,
    /// An edge names an endpoint that is never declared.
    UndeclaredNode,
}

/// Why a piece of Mermaid source was rejected, and where.
///
/// The line number is not decoration. This error is shown beside an editor
/// holding the whole document, so "rejected" without a row is something the
/// user cannot act on, and a row counted from the start of the fence rather
/// than the start of the file points at the wrong line. Both the rule and the
/// line are therefore part of the contract, and the rule is an enum rather
/// than prose so the UI can group and explain the categories itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ValidationError {
    pub rule: ValidationRule,
    /// 1-based line number **in the source as given**, fence and surrounding
    /// prose included.
    pub line: usize,
    /// One sentence naming what was found, quoting it where quoting helps.
    pub detail: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.detail)
    }
}

impl std::error::Error for ValidationError {}

/// The diagram families this app will render.
///
/// This is a CSP guard, not a taste judgement. The spike established that
/// Mermaid 11.16.1 renders under `default-src 'self'` with no `unsafe-eval`
/// for these families with `htmlLabels: false`; others — `mindmap`,
/// `architecture-beta`, `flowchart-elk` and the rest — were not exercised and
/// several pull additional renderer bundles. `stateDiagram` is absent on
/// purpose: only the `-v2` renderer was tested, and the two are different code
/// paths.
const ALLOWED_DIAGRAMS: [&str; 6] = [
    "flowchart",
    "graph",
    "sequenceDiagram",
    "classDiagram",
    "erDiagram",
    "stateDiagram-v2",
];

/// Check that Mermaid source is one diagram, of a family this app renders,
/// containing nothing that can execute or navigate.
///
/// This is a security boundary rather than a linter. A later phase lets a
/// coding agent write one of these files and a user edit it, so by the time
/// the source reaches the renderer nobody has read it. The rules are ordered
/// from the structural to the specific — how many diagrams, what kind, what is
/// in them, whether they hang together — so that the first thing reported is
/// the outermost thing wrong.
///
/// Scope is *almost* the diagram itself: when the source is a markdown document
/// with a fence, only the fenced body is examined for markup and bindings.
/// Prose around the fence is the markdown renderer's problem, and reaching into
/// it here would reject a document for saying the word `href` in a sentence — a
/// wrong rejection, which this module treats as the worse failure.
///
/// The one rule that does not respect that scope is the directive check, and
/// the reason is that Mermaid does not respect it either. Its directive scanner
/// is a regex over the raw text, so a `%%{init: …}` sitting in the prose is
/// found and applied if the whole document ever reaches it — and it can, since
/// [`super::store::parse`] hands on the whole post-front-matter body. Widening
/// that one rule to the whole file costs nothing in wrong rejections, because
/// prose does not contain the literal `%%{`.
pub fn validate(source: &str) -> Result<(), ValidationError> {
    let lines: Vec<&str> = source.lines().collect();
    let body = locate_diagram(&lines)?;

    // Before the diagram type, because a directive is fatal whatever the
    // diagram turns out to be, and because it can appear above the line that
    // declares one.
    forbidden_directives(&lines)?;
    let diagram_type = diagram_type(&lines, &body)?;
    forbidden_constructs(&lines, &body)?;

    // Only the flowchart family has `subgraph`, and only its edges are written
    // as `a --> b`. A sequence diagram's `end` closes a `loop`, and a class
    // diagram's `A <|-- B` is not a flowchart edge, so running either rule
    // there would reject valid diagrams wholesale.
    if matches!(diagram_type, "flowchart" | "graph") {
        balanced_subgraphs(&lines, &body)?;
        declared_endpoints(&lines, &body)?;
    }
    Ok(())
}

/// The half-open range of line indices holding the diagram itself.
fn locate_diagram(lines: &[&str]) -> Result<std::ops::Range<usize>, ValidationError> {
    let fences: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| is_mermaid_fence(line))
        .map(|(i, _)| i)
        .collect();

    match fences.as_slice() {
        // A bare diagram with no fence is the form this renderer emits and the
        // form a `.mmd` file takes, so it is not an error.
        [] => Ok(0..lines.len()),
        [open] => match (open + 1..lines.len()).find(|i| lines[*i].trim() == "```") {
            Some(close) => Ok(open + 1..close),
            None => Err(ValidationError {
                rule: ValidationRule::FenceCount,
                line: open + 1,
                detail: "this ```mermaid fence is never closed".into(),
            }),
        },
        // Two diagrams in one file is not a diagram this app can show, and
        // picking one of them would be a guess about which the user meant.
        [_, second, ..] => Err(ValidationError {
            rule: ValidationRule::FenceCount,
            line: second + 1,
            detail: format!(
                "a diagram file holds exactly one ```mermaid fence, but this file has {}",
                fences.len()
            ),
        }),
    }
}

fn is_mermaid_fence(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("```")
        && trimmed
            .trim_start_matches('`')
            .trim()
            .eq_ignore_ascii_case("mermaid")
}

/// The first word of the first statement, checked against [`ALLOWED_DIAGRAMS`].
fn diagram_type<'a>(
    lines: &[&'a str],
    body: &std::ops::Range<usize>,
) -> Result<&'a str, ValidationError> {
    for i in body.clone() {
        let code = code_of(lines[i]);
        let Some(word) = code.split_whitespace().next() else {
            continue;
        };
        return match ALLOWED_DIAGRAMS.iter().find(|allowed| **allowed == word) {
            Some(allowed) => Ok(allowed),
            None => Err(ValidationError {
                rule: ValidationRule::DiagramType,
                line: i + 1,
                detail: format!("'{word}' is not a diagram type this app renders"),
            }),
        };
    }
    Err(ValidationError {
        rule: ValidationRule::DiagramType,
        line: body.start + 1,
        detail: "this diagram declares no diagram type".into(),
    })
}

/// Reject an `init` directive anywhere in the source.
///
/// Checked on the **raw** line, with no quote handling and no comment
/// handling, because that is how Mermaid finds one: `directiveRegex` in
/// `src/diagram-api/regexes.ts` is matched over the text as given. A
/// quote-aware scan is therefore not a stricter version of this check but a
/// weaker one — an odd number of `"` earlier on the line hides the directive
/// from the scan and from nothing else.
///
/// What a directive can do is narrower than it looks and still enough. It
/// cannot loosen `securityLevel`, which Mermaid lists in `defaultConfig.secure`
/// and deletes out of directive arguments in `sanitize`. It *can* set
/// `htmlLabels`, which is not a secure key — and `htmlLabels: false` is the
/// premise of every claim this module makes about labels being inert text.
///
/// [`render`] can never emit one: [`comment`] and [`escape_label`] both refuse
/// to put two `%` characters next to each other.
fn forbidden_directives(lines: &[&str]) -> Result<(), ValidationError> {
    for (i, line) in lines.iter().enumerate() {
        if line.contains("%%{") {
            return Err(ValidationError {
                rule: ValidationRule::ForbiddenDirective,
                line: i + 1,
                detail: "a %%{init} directive can re-enable htmlLabels, which is what makes the \
                         rest of these checks meaningful"
                    .into(),
            });
        }
    }
    Ok(())
}

/// Reject anything that can run code or navigate.
///
/// Each of these is a real capability rather than a stylistic objection.
/// `click ... call` invokes a callback named by the file, which needs
/// `securityLevel: "loose"` and hands the file author arbitrary execution.
/// `href`, a `click` binding of any shape, `javascript:`, `<script`, `<iframe`
/// and `on…=` handlers are navigation and markup injection, which
/// `htmlLabels: false` should already prevent — this is the second lock, on
/// the assumption that the configuration could be wrong. Directives are not
/// here; they are checked over the whole file by [`forbidden_directives`].
///
/// Quoted strings and `%%` comments are exempt, and that exemption is load
/// bearing rather than lenient. Mermaid parses neither, project names really
/// do contain angle brackets, and [`render`] copies manifest text verbatim
/// into comments; without the exemption this module would reject its own
/// output. See [`scannable`] for the one case where the exemption is withheld.
fn forbidden_constructs(
    lines: &[&str],
    body: &std::ops::Range<usize>,
) -> Result<(), ValidationError> {
    for i in body.clone() {
        let reject = |detail: &str| {
            Err(ValidationError {
                rule: ValidationRule::ForbiddenDirective,
                line: i + 1,
                detail: detail.to_string(),
            })
        };

        let (code, well_quoted) = scannable(lines[i]);
        let code = code.to_ascii_lowercase();
        for pattern in ["<script", "<iframe", "javascript:", "href"] {
            if code.contains(pattern) {
                return reject(&format!("'{pattern}' can execute or navigate"));
            }
        }
        if binds_click(&code, well_quoted) {
            return reject("a click binding attaches a callback or a link to a node");
        }
        if has_event_handler(&code) {
            return reject("an inline event handler can execute");
        }
    }
    Ok(())
}

/// The part of a line these rules are applied to, and whether its quoting could
/// be trusted while working that out.
///
/// A whole-line `%%` comment is exempt outright: Mermaid parses nothing in one,
/// and the only construct that survives being written in a comment — a
/// directive — is checked separately on the raw text.
///
/// Otherwise the quoted spans are dropped, which is what makes a project
/// honestly named `<script>` renderable. That is only sound while the quotes on
/// the line actually pair up. On a line with an odd number of `"` there is no
/// such thing as "outside the quotes": the stripper simply discards everything
/// after the last one, so a single stray quote deletes the rest of the line
/// from every rule below it. Such a line is malformed as far as quoting goes,
/// so the exemption is withheld and the raw line is scanned instead — the
/// second return value says which happened, because it also changes how narrow
/// the `click` rule can afford to be.
///
/// [`render`] never produces such a line: [`escape_label`] leaves no unescaped
/// `"` inside a label, and every comment it writes starts the line.
fn scannable(line: &str) -> (String, bool) {
    if line.trim_start().starts_with("%%") {
        return (String::new(), true);
    }
    if line.matches('"').count() % 2 == 0 {
        (code_of(line), true)
    } else {
        (line.to_string(), false)
    }
}

/// Whether a line binds a `click` handler.
///
/// On a well-quoted line the keyword opens the statement, so only the first
/// word is looked at; anywhere else it is a word inside an unquoted label, and
/// rejecting `a[double click me]` would be a wrong answer in the other
/// direction. On a line whose quotes never close there is no reliable first
/// word — the vector is a leading `"` that pushes the binding out of view — so
/// the keyword is looked for anywhere on it. The line is already malformed, so
/// the widened rule cannot cost a diagram that was fine.
fn binds_click(code: &str, well_quoted: bool) -> bool {
    if well_quoted {
        code.split_whitespace().next() == Some("click")
    } else {
        code.split_whitespace().any(|word| word == "click")
    }
}

/// Whether `code` contains an `on…=` HTML event handler.
///
/// Written by hand rather than with a regex, which is a choice and not a
/// constraint: `cb-core` does declare `regex` (`crates/core/Cargo.toml`), it is
/// simply not used anywhere in `src/` yet. Nor is the usual argument for
/// avoiding one available here — the `regex` crate matches in time linear in
/// the input, so there is no adversarial input that makes it slow. The reason
/// is only that the shape is narrow — `on` on an identifier boundary, at least
/// one letter, then optional whitespace and `=` — and that on a security
/// boundary a reader has to be able to see what is matched without also having
/// to be sure how the pattern behaves. The boundary condition is what keeps
/// this from firing on an ordinary identifier that happens to contain the two
/// letters.
fn has_event_handler(code: &str) -> bool {
    let bytes = code.as_bytes();
    for start in 0..bytes.len().saturating_sub(3) {
        if &bytes[start..start + 2] != b"on" {
            continue;
        }
        if start > 0 && is_ident_byte(bytes[start - 1]) {
            continue;
        }
        let mut i = start + 2;
        let letters = i;
        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
            i += 1;
        }
        if i == letters {
            continue;
        }
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'=' {
            return true;
        }
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn balanced_subgraphs(
    lines: &[&str],
    body: &std::ops::Range<usize>,
) -> Result<(), ValidationError> {
    let mut open: Vec<usize> = Vec::new();
    for i in body.clone() {
        let code = code_of(lines[i]);
        let trimmed = code.trim();
        if trimmed == "end" {
            if open.pop().is_none() {
                return Err(ValidationError {
                    rule: ValidationRule::UnbalancedSubgraph,
                    line: i + 1,
                    detail: "this 'end' closes a subgraph that was never opened".into(),
                });
            }
        } else if code.split_whitespace().next() == Some("subgraph") {
            open.push(i);
        }
    }
    match open.last() {
        // The innermost unclosed subgraph is the one to point at: it is the
        // one whose `end` is missing, and the outer ones are only unclosed as
        // a consequence.
        Some(line) => Err(ValidationError {
            rule: ValidationRule::UnbalancedSubgraph,
            line: line + 1,
            detail: "this subgraph is never closed with 'end'".into(),
        }),
        None => Ok(()),
    }
}

/// Require every edge endpoint to have been declared with a shape somewhere.
///
/// Mermaid does not require this — a bare identifier in an edge silently
/// becomes an empty box. That default is wrong for this app. These diagrams
/// are generated by [`render`], which always declares every node, or written
/// by an agent, where an identifier nobody declared is far more likely a typo
/// than an intentionally blank box, and a blank box is exactly the sort of
/// thing a reader fills in with a guess.
///
/// The parser underneath recognises the flowchart subset this renderer emits
/// plus the shapes people hand-write, and abstains wherever it cannot be sure
/// — see [`endpoints_of`]. It never reports an endpoint it did not clearly
/// see, because an error beside code that is fine is worse than a missed typo.
fn declared_endpoints(
    lines: &[&str],
    body: &std::ops::Range<usize>,
) -> Result<(), ValidationError> {
    let mut declared: BTreeSet<String> = BTreeSet::new();
    let mut used: Vec<(usize, String)> = Vec::new();

    for i in body.clone() {
        let code = strip_pipes(&code_of(lines[i]));
        let items = scan(&code);

        if matches!(items.first(), Some(Item::Id(word, _)) if word == "subgraph") {
            if let Some(Item::Id(id, _)) = items.get(1) {
                declared.insert(id.clone());
            }
            continue;
        }
        for item in &items {
            if let Item::Id(id, true) = item {
                declared.insert(id.clone());
            }
        }
        for endpoint in endpoints_of(&items) {
            used.push((i, endpoint));
        }
    }

    for (line, id) in used {
        if !declared.contains(&id) {
            return Err(ValidationError {
                rule: ValidationRule::UndeclaredNode,
                line: line + 1,
                detail: format!("'{id}' is used as an edge endpoint but never declared"),
            });
        }
    }
    Ok(())
}

/// One thing the flowchart scanner recognised on a line.
#[derive(Debug, PartialEq, Eq)]
enum Item {
    /// An identifier. The flag is set when a shape opens immediately after it,
    /// which is what makes the line a declaration rather than a mention.
    Id(String, bool),
    /// A link run. The flag is set when it ends in an arrowhead.
    Link(bool),
}

/// Split a flowchart statement into identifiers and link runs.
///
/// The contents of a shape are skipped by bracket depth rather than tokenised,
/// so an unquoted label in `A[Text] --> B[Text]` cannot be mistaken for a node
/// named `Text`. Arrowhead letters (`--o`, `--x`) are absorbed into the link
/// run when they are attached to it, so they are not read as identifiers
/// either.
fn scan(code: &str) -> Vec<Item> {
    let chars: Vec<char> = code.chars().collect();
    let mut items = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if ch.is_ascii_alphanumeric() || ch == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let shaped = matches!(chars.get(i), Some('[' | '(' | '{'));
            items.push(Item::Id(word, shaped));
            if shaped {
                i = skip_shape(&chars, i);
            }
        } else if is_link_char(ch) {
            let start = i;
            while i < chars.len() && is_link_char(chars[i]) {
                i += 1;
            }
            if matches!(chars.get(i), Some('o' | 'x'))
                && !chars.get(i + 1).is_some_and(|c| c.is_ascii_alphanumeric())
            {
                i += 1;
            }
            let run: String = chars[start..i].iter().collect();
            if run.contains("--") || run.contains("-.") || run.contains("==") {
                let head = run.ends_with(['>', 'o', 'x']);
                items.push(Item::Link(head));
            }
        } else {
            i += 1;
        }
    }
    items
}

fn is_link_char(ch: char) -> bool {
    matches!(ch, '-' | '.' | '=' | '<' | '>' | '|' | '~')
}

/// Advance past a shape, counting brackets, so nested and doubled shapes
/// (`[[…]]`, `([…])`, `{{…}}`) are consumed whole.
fn skip_shape(chars: &[char], open: usize) -> usize {
    let mut depth = 0usize;
    let mut i = open;
    while i < chars.len() {
        match chars[i] {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    i
}

/// The edge endpoints on a scanned line.
///
/// An identifier sitting between a link run with no arrowhead and another link
/// run is the `A -- text --> B` label form, so it is not an endpoint. That
/// test cannot distinguish it from the open-link chain `A --- B --- C`, where
/// `B` really is a node; in that case this returns `A` and `C` and says
/// nothing about `B`. Abstaining on `B` is the intended outcome — the rule
/// exists to catch typos, and the cost of missing one is far below the cost of
/// rejecting a diagram that is fine.
fn endpoints_of(items: &[Item]) -> Vec<String> {
    let keep: Vec<&Item> = items
        .iter()
        .enumerate()
        .filter(|(i, item)| {
            if !matches!(item, Item::Id(..)) {
                return true;
            }
            let preceded_by_open_link =
                matches!(items.get(i.wrapping_sub(1)), Some(Item::Link(false))) && *i > 0;
            let followed_by_link = matches!(items.get(i + 1), Some(Item::Link(_)));
            !(preceded_by_open_link && followed_by_link)
        })
        .map(|(_, item)| item)
        .collect();

    let mut endpoints = Vec::new();
    for window in keep.windows(3) {
        if let [Item::Id(from, _), Item::Link(_), Item::Id(to, _)] = window {
            endpoints.push(from.clone());
            endpoints.push(to.clone());
        }
    }
    endpoints
}

/// Remove `|…|` edge-label spans, which are the one place a flowchart puts
/// free text outside a shape.
fn strip_pipes(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut inside = false;
    for ch in code.chars() {
        if ch == '|' {
            inside = !inside;
            out.push(' ');
        } else if !inside {
            out.push(ch);
        }
    }
    out
}

/// The part of a line Mermaid parses as syntax: quoted strings and anything
/// after a `%%` comment marker removed.
///
/// Quote tracking is safe on this module's own output because [`escape_label`]
/// guarantees the only unescaped `"` characters are the delimiters themselves.
/// On a line that did not come from here it is only safe while the quotes pair
/// up, which is [`scannable`]'s job to check — and it is never the right tool
/// for finding a directive, which Mermaid looks for in the raw text and inside
/// comments alike.
fn code_of(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut quoted = false;
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if ch == '"' {
            quoted = !quoted;
            i += 1;
            continue;
        }
        if !quoted {
            if ch == '%' && chars.get(i + 1) == Some(&'%') {
                break;
            }
            out.push(ch);
        }
        i += 1;
    }
    out
}
