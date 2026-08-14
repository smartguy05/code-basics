//! Signals about the routes an ASP.NET service exposes.
//!
//! # Routes are enrichment, and only enrichment
//!
//! A route says what an existing service *answers*. It never says that a
//! service exists. Every signal this producer emits is therefore
//! [`Strength::Medium`](super::framework::Strength::Medium), and
//! [`admit`](super::framework::admit) refuses it
//! unless some HIGH signal has already created the
//! [`ComponentKind::HttpService`] component it names — in practice the .NET
//! producer's `Microsoft.NET.Sdk.Web` signal. If that project was never found,
//! everything here is discarded and counted. That is the designed outcome, not
//! a bug to work around: a route list hanging off nothing would be this module
//! asserting a service exists on the strength of a `MapGet` call, which is
//! exactly the inference [`super`] forbids.
//!
//! The join is by *label*, because that is the only handle
//! [`admit`](super::framework::admit) offers: signals meet at
//! `(kind, case-folded label)`. This producer labels every signal with
//! [`Project::name`], the same string the scan derived from the project
//! directory, so the .NET producer's HIGH signal for a web project and this
//! producer's route list for it land on the same component. That coupling is
//! deliberate and is stated here rather than left implicit, because its failure
//! mode is quiet: a producer that labelled the same project differently would
//! not crash, it would produce a run of `medium-without-high` warnings and a
//! diagram with no routes on it.
//!
//! # Why this file abstains as often as it emits
//!
//! An endpoint list is the single most trusted thing a diagram of this sort can
//! carry. A reader will paste it into a client, or scan it to decide whether a
//! service already exposes something. `GET /api/orders` printed for an endpoint
//! that is really `GET /api/v1/orders` is not a slightly-wrong label, it is a
//! false statement that costs someone an afternoon. Nothing here is worth that,
//! so every rule below produces nothing in preference to producing something
//! plausible, and every refusal is pushed onto [`RouteScan::warnings`] so that
//! "we looked at this and declined to name it" is visible.
//!
//! Two things follow from that and are worth stating outright, because both
//! look like omissions:
//!
//! * **`MapGroup` prefixes are never concatenated onto anything, and a
//!   *project* that uses route groups anywhere contributes no minimal-API
//!   routes at all.** The reasoning, and why the refusal is project-wide rather
//!   than file-wide, is on [`scan_map_calls`].
//! * **An abstract controller declares nothing, and neither does a class that
//!   derives from one.** The reasoning is on [`scan_controllers`].
//! * **`MapHealthChecks`, `MapHub`, `MapGrpcService` and `MapRazorPages` are
//!   each refused**, with a separate reason for each. See [`DECLINED_MAPS`].
//!
//! # What is read, and what is not re-read
//!
//! Controller classes come from [`crate::symbols::index`], which has already
//! walked the workspace and recorded every class declaration with its file and
//! line. This producer does not walk the tree a second time; it filters that
//! index and reads the handful of files it names. Minimal-API registrations
//! have no declaration to index — `app.MapGet(...)` is a call, not a
//! declaration — so those are found by a line scan, scoped to the `.cs` files
//! the index already listed under a project's directory and capped by
//! [`MAX_SCANNED_FILES_PER_PROJECT`] and [`MAX_SCANNED_BYTES`].
//!
//! This is a line scan and not a C# parser, and the limits of that are real:
//! see [`Masker`] for exactly which lexical states are tracked across lines and
//! which are not.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::framework::{ComponentKind, Evidence, Signal};
use crate::model::Project;
use crate::symbols::declarations::SymbolKind;
use crate::symbols::index::SymbolIndex;

/// The largest source file this producer will read.
///
/// Half a mebibyte, deliberately below [`crate::symbols::index`]'s own cap. A
/// `.cs` file larger than this is generated — a service reference, a designer
/// file, an EF migration — and generated code does not hand-register routes.
/// Reading it would cost time to find nothing.
pub const MAX_SCANNED_BYTES: u64 = 512 * 1024;

/// How many `.cs` files one project contributes to the line scan before it is
/// cut short and the cut reported.
///
/// Five hundred is far past any project that hand-registers endpoints. Hitting
/// it means the directory is not what it appears to be, and the honest response
/// is a partial answer that says it is partial.
pub const MAX_SCANNED_FILES_PER_PROJECT: usize = 500;

/// How many routes one project may contribute.
///
/// A service with more than 250 endpoints exists, but a list of them is not a
/// diagram annotation any more, and the cap is reported rather than silently
/// applied.
pub const MAX_ROUTES_PER_PROJECT: usize = 250;

/// How much of a source line is quoted as evidence.
const MAX_EXCERPT_CHARS: usize = 200;

/// The HTTP verbs an action attribute or a minimal-API registration may name.
///
/// Exactly the five the plan lists. `[HttpHead]` and `[HttpOptions]` exist and
/// are omitted on purpose: they are not what a reader means by "the routes this
/// service exposes", and adding them later is a one-line change with a test,
/// whereas removing a verb somebody has come to rely on is not.
const VERBS: &[(&str, &str)] = &[
    ("Get", "GET"),
    ("Post", "POST"),
    ("Put", "PUT"),
    ("Delete", "DELETE"),
    ("Patch", "PATCH"),
];

/// Registrations that really do map something to a URL, and are still not
/// routes — with the reason each one is refused.
///
/// These are the four the plan asks to be decided individually rather than
/// swept up by a wildcard, and the four answers are not the same answer:
///
/// * **`MapHealthChecks("/health")`** — a literal path, and a genuine endpoint.
///   Refused because the call states no verb. Every other entry in a route list
///   here is `VERB path`, and printing `/health` with a verb this producer
///   picked would be inventing the half of the fact that was not written down.
/// * **`MapHub<ChatHub>("/chat")`** — the path is a SignalR *negotiation*
///   endpoint, not something a client calls with a verb and a body. The surface
///   a reader would want is the hub's methods, which are on a class this scan
///   never looks at, so reporting the path would name the wrong thing.
/// * **`MapGrpcService<GreeterService>()`** — carries no path at all. gRPC
///   derives it from the `service` name in a `.proto` file that this producer
///   does not read and this crate has no parser for.
/// * **`MapRazorPages()`** — carries no path either. Razor page routes come
///   from the file layout under `Pages/`, which is a convention, and this
///   codebase does not infer from conventions.
///
/// All four are warned about rather than ignored, because all four are things a
/// reader might reasonably expect to see and their absence should be explained.
const DECLINED_MAPS: &[(&str, &str)] = &[
    (
        "MapHealthChecks",
        "it states no HTTP verb, and this producer will not choose one for it",
    ),
    (
        "MapHub",
        "a SignalR hub's path is a negotiation endpoint rather than a REST route, and its real \
         surface is the hub's methods",
    ),
    (
        "MapGrpcService",
        "a gRPC service's path comes from a .proto file, which is not read here",
    ),
    (
        "MapRazorPages",
        "Razor page routes come from the file layout, which is a convention rather than a \
         declaration",
    ),
];

/// What one scan of a workspace found, and what it refused.
///
/// The warnings are a separate field rather than more signals because most
/// refusals here never become a [`Signal`] at all — there is nothing to label a
/// signal *with* when the route could not be resolved, and inventing a label so
/// that [`admit`](super::framework::admit) could discard it would put a guessed
/// name in the warning text. The caller is expected to append these to
/// [`ArchGraph::warnings`](super::super::graph::ArchGraph::warnings) alongside
/// the ones [`Admitted::warnings`](super::framework::Admitted::warnings)
/// produces.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteScan {
    /// Sorted by `(project, route, file, line)`, so the same workspace always
    /// produces the same list whatever order the filesystem walked in. Sorting
    /// by the route text rather than by position is what makes the result read
    /// as a route list instead of as a transcript of the scan.
    pub signals: Vec<Signal>,
    /// Every candidate that was seen and declined, as prose, sorted and
    /// deduplicated.
    pub warnings: Vec<String>,
}

/// Read the ASP.NET route declarations a workspace contains.
///
/// `root` is the workspace root the index was built from; `index` must be a
/// built [`SymbolIndex`] over that same root, since the controller classes are
/// taken from it rather than rediscovered. An empty index yields an empty scan,
/// which is the correct answer for "nothing has been indexed" and is
/// indistinguishable from "there are no controllers" — a distinction this
/// producer cannot draw and does not pretend to.
pub fn route_signals(root: &Path, projects: &[Project], index: &SymbolIndex) -> RouteScan {
    let owners = Owners::new(&index.root, projects);
    let classes = classes_by_file(index);

    // Files are grouped by owning project first so that the per-project caps
    // below mean what they say, and so a project's own refusals stay together.
    let mut by_project: BTreeMap<&str, Vec<&PathBuf>> = BTreeMap::new();
    for file in &index.files {
        if !is_csharp(file) {
            continue;
        }
        if let Some(project) = owners.owner_of(file) {
            by_project
                .entry(project.id.as_str())
                .or_default()
                .push(file);
        }
    }

    let mut out = RouteScan::default();
    for project in projects {
        let files = by_project.remove(project.id.as_str()).unwrap_or_default();
        let found = scan_project(root, project, &files, &classes);
        out.signals.extend(found.signals);
        out.warnings.extend(found.warnings);
    }

    out.signals.sort_by(|a, b| {
        (
            &a.project_id,
            &a.detail,
            &a.evidence.path,
            a.evidence.line,
            &a.evidence.excerpt,
        )
            .cmp(&(
                &b.project_id,
                &b.detail,
                &b.evidence.path,
                b.evidence.line,
                &b.evidence.excerpt,
            ))
    });
    out.warnings.sort();
    out.warnings.dedup();
    out
}

// ---------------------------------------------------------------------------
// Per project
// ---------------------------------------------------------------------------

/// Everything one project contributed.
#[derive(Default)]
struct Found {
    signals: Vec<Signal>,
    warnings: Vec<String>,
    /// Whether anything route-shaped was seen at all, regardless of whether it
    /// was admitted. Used only to decide whether a test project is worth
    /// mentioning.
    saw_candidate: bool,
}

/// Scan one project's `.cs` files.
///
/// # Why the minimal-API half runs in two phases
///
/// Whether a route group is in play is a property of the *project*, not of the
/// file the endpoint happens to be written in — the canonical .NET 8 layout puts
/// `app.MapGroup("/api/v1").MapOrders()` in `Program.cs` and the `MapGet` calls
/// in an `OrderEndpoints.cs` that never mentions a group. A file-local decision
/// reads that second file as ungrouped and emits `/orders` for an endpoint that
/// is really `/api/v1/orders`, which is the exact fabrication [`scan_map_calls`]
/// exists to prevent, committed one file away from where it was noticed.
///
/// So every registration is read into [`PendingRoute`] as the files go past, and
/// nothing is emitted until the last file has been read and the project-wide
/// answer is known. Reading is deferred no further than it must be: the literal
/// argument is resolved during the pass, while the file's text is at hand, and
/// only the *verdict* waits. That keeps the peak cost one file rather than a
/// project's worth of source text.
///
/// A test project is scanned exactly like any other and then has its whole
/// result thrown away, replaced by a single warning naming it. That is more
/// work than skipping it outright, and it buys the one thing skipping cannot:
/// the difference between "this test project declares no routes" and "this test
/// project declares routes that were deliberately not drawn" is visible to the
/// user. Its own internal refusals go with it — a `MapGroup` inside a test
/// fixture is not something anyone needs told about — so one line is reported
/// instead of a paragraph about code that was never a candidate.
fn scan_project(
    root: &Path,
    project: &Project,
    files: &[&PathBuf],
    classes: &BTreeMap<PathBuf, Vec<ClassDecl>>,
) -> Found {
    let mut found = Found::default();
    let mut pending: Vec<PendingRoute> = Vec::new();
    let mut grouped = false;

    for (scanned, relative) in files.iter().enumerate() {
        if scanned >= MAX_SCANNED_FILES_PER_PROJECT {
            found.warnings.push(format!(
                "{}: only the first {MAX_SCANNED_FILES_PER_PROJECT} source files were read for \
                 route declarations, so the route list may be incomplete",
                project.name
            ));
            break;
        }
        let Some(text) = read_source(root, relative, &project.name, &mut found.warnings) else {
            continue;
        };
        let file = SourceFile::new(relative, &text);
        scan_controllers(project, &file, classes, &mut found);
        grouped |= scan_map_calls(project, &file, &mut found, &mut pending);
    }

    emit_pending(project, grouped, pending, &mut found);

    if project.is_test_project {
        let saw = found.saw_candidate;
        found = Found::default();
        if saw {
            found.warnings.push(format!(
                "{}: route declarations in this project were not drawn because it is a test \
                 project, and a fixture is not an endpoint the system exposes",
                project.name
            ));
        }
        return found;
    }

    if found.signals.len() > MAX_ROUTES_PER_PROJECT {
        found.signals.truncate(MAX_ROUTES_PER_PROJECT);
        found.warnings.push(format!(
            "{}: more than {MAX_ROUTES_PER_PROJECT} routes were found and the list was cut short",
            project.name
        ));
    }
    found
}

/// Read a source file, or explain why it was not read.
fn read_source(
    root: &Path,
    relative: &Path,
    project: &str,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let absolute = root.join(relative);
    let meta = std::fs::metadata(&absolute).ok()?;
    if meta.len() > MAX_SCANNED_BYTES {
        warnings.push(format!(
            "{project}: {} was not read for route declarations because it is larger than \
             {MAX_SCANNED_BYTES} bytes",
            display(relative)
        ));
        return None;
    }
    // A file that cannot be read or is not UTF-8 contributes nothing and says
    // nothing, matching `symbols::index`: it is a property of the file rather
    // than a decision this producer made, and there is nothing the user can do
    // with the report.
    String::from_utf8(std::fs::read(&absolute).ok()?).ok()
}

// ---------------------------------------------------------------------------
// Controllers
// ---------------------------------------------------------------------------

/// A class declaration taken from the symbol index.
struct ClassDecl {
    name: String,
    /// 1-based, as [`crate::symbols::index::Symbol::line`] records it.
    line: u32,
}

/// Every class the index recorded, grouped by file and ordered by line.
///
/// *Every* class, not only the ones whose name ends in `Controller`: the
/// non-controller ones are what bound a controller's body. Without them a
/// controller's `[HttpGet]` scan would run on into whatever class follows it in
/// the same file and attribute its actions to the wrong route.
fn classes_by_file(index: &SymbolIndex) -> BTreeMap<PathBuf, Vec<ClassDecl>> {
    let mut out: BTreeMap<PathBuf, Vec<ClassDecl>> = BTreeMap::new();
    for symbol in &index.symbols {
        if symbol.kind != SymbolKind::Class || !is_csharp(&symbol.path) {
            continue;
        }
        out.entry(symbol.path.clone()).or_default().push(ClassDecl {
            name: symbol.name.clone(),
            line: symbol.line,
        });
    }
    for classes in out.values_mut() {
        classes.sort_by_key(|c| c.line);
    }
    out
}

/// Find the controllers in one file and the routes their actions declare.
///
/// # Why an abstract controller and its subclasses are both refused
///
/// ASP.NET's controller discovery (`DefaultControllerTypeProvider`) skips
/// abstract types outright, so
///
/// ```text
/// [ApiController] [Route("api/[controller]")]
/// public abstract class BaseController : ControllerBase { [HttpGet("ping")] … }
/// ```
///
/// registers **nothing at all** when no class derives from it. Reading its
/// attributes as endpoints prints `GET /api/Base/ping` for a path that returns
/// 404 from a running application, which is the worst thing this file can do.
/// The `abstract` keyword is a declaration, not a convention, so refusing on it
/// is the same kind of move as requiring `[ApiController]`.
///
/// What a class *deriving* from it inherits is not resolved either, and that is
/// the more arguable half. A `DerivedController : BaseController` really does
/// register the base's `ping` action, under whichever template the derived class
/// resolves to. Naming it here would mean binding the identifier after the `:`
/// to a declaration somewhere in the workspace, which is exactly the cross-file
/// resolution the rest of this module refuses: the base may be in another
/// project or another assembly entirely, the name may be shadowed in two
/// namespaces, either class may be `partial` and spread over files, and the
/// inherited actions live in a body this scan never reads. Getting it wrong
/// produces a plausible route at a wrong prefix, which is the failure this file
/// is built to avoid.
///
/// So inheritance is not followed at all, and the two shapes that fall out of
/// that are different — deliberately, and both are pinned by tests:
///
/// * A derived class carrying **no attributes of its own** is refused by the
///   `[ApiController]`/`[Route]` rule below, which needs no extra branch here.
/// * A derived class carrying **its own attributes** is an ordinary controller
///   and the actions written *in it* are emitted, because those are facts about
///   that class. The ones it inherits are simply absent — an under-report rather
///   than a wrong path.
///
/// The refusal recorded against the abstract base is what makes that second,
/// quieter case visible, since it says in as many words that the routes a
/// derived class inherits were not resolved. When the base lives outside the
/// workspace there is no such line, and nothing here can produce one: a class
/// this scan never saw cannot be reported on.
fn scan_controllers(
    project: &Project,
    file: &SourceFile,
    classes: &BTreeMap<PathBuf, Vec<ClassDecl>>,
    found: &mut Found,
) {
    let Some(declared) = classes.get(file.path.as_path()) else {
        return;
    };

    for (position, class) in declared.iter().enumerate() {
        let Some(stem) = controller_stem(&class.name) else {
            // Not a candidate at all. A class that is not named `*Controller`
            // is not "a controller we declined to draw", it is an ordinary
            // class, and warning about every one of them would bury the
            // refusals that mean something.
            continue;
        };
        found.saw_candidate = true;

        let Some(line_index) = file.index_of(class.line) else {
            continue;
        };

        // Checked before the attributes, because an abstract class carrying
        // both attributes is precisely the case that used to slip through: it
        // satisfies every other rule here and still registers nothing.
        if file.declares_abstract(line_index) {
            found.warnings.push(refusal(
                &project.name,
                &format!("the controller '{}'", class.name),
                &file.path,
                Some(class.line),
                "it is declared abstract, and controller discovery skips abstract types, so \
                 nothing it declares is an endpoint; the routes a class deriving from it inherits \
                 are not resolved here either",
            ));
            continue;
        }

        let attributes = file.attribute_block_above(line_index);

        let route_attributes: Vec<&ParsedAttribute> = attributes
            .iter()
            .filter(|a| a.name == "Route")
            .collect::<Vec<_>>();
        let has_api_controller = attributes.iter().any(|a| a.name == "ApiController");

        // Both halves matter. The suffix on its own is a naming convention, and
        // this codebase does not infer from conventions; a class called
        // `BaseController` with neither attribute is an abstract base, not an
        // endpoint.
        if route_attributes.len() > 1 {
            found.warnings.push(refusal(
                &project.name,
                &format!("the controller '{}'", class.name),
                &file.path,
                Some(class.line),
                "it carries more than one class-level [Route], and choosing between them would \
                 be a guess",
            ));
            continue;
        }
        if !has_api_controller && route_attributes.is_empty() {
            found.warnings.push(refusal(
                &project.name,
                &format!("the class '{}'", class.name),
                &file.path,
                Some(class.line),
                "its name ends in 'Controller' but it carries neither [ApiController] nor \
                 [Route], and a naming convention is not a declaration",
            ));
            continue;
        }

        // The class-level template. `None` means there is no class route at
        // all, which is legal when every action carries a complete one.
        let class_template = match route_attributes.first() {
            None => Some(String::new()),
            Some(attribute) => {
                match template_of(file, attribute, Some(&stem), &project.name, found) {
                    Slot::Resolved(template) => Some(template),
                    Slot::Missing => {
                        // `[Route]` with no argument is not valid attribute
                        // routing, and inventing an empty prefix for it would
                        // report endpoints at the wrong paths.
                        found.warnings.push(refusal(
                            &project.name,
                            &format!("the controller '{}'", class.name),
                            &file.path,
                            Some(attribute.line),
                            "its [Route] attribute has no template",
                        ));
                        None
                    }
                    Slot::Refused => None,
                }
            }
        };
        let Some(class_template) = class_template else {
            continue;
        };

        let end = declared
            .get(position + 1)
            .and_then(|next| file.index_of(next.line))
            .unwrap_or(file.lines.len());
        scan_actions(
            project,
            file,
            &class_template,
            &stem,
            line_index + 1,
            end,
            found,
        );
    }
}

/// Read the action attributes between two class declarations.
///
/// Attributes are read in *blocks* — a maximal run of attribute-only lines,
/// with comments and blank lines allowed inside — rather than line by line,
/// because a single action's attributes are routinely spread over several lines
/// and their meaning is joint. `[HttpGet]` beside a `[Route("x")]` in the same
/// block is an action whose template lives on the `Route`, and reading the two
/// independently would emit the class route for the `HttpGet` and silently drop
/// the real one. The block is refused instead.
///
/// # `[NonAction]`
///
/// `[NonAction]` beside a verb attribute means the framework registers nothing
/// for that method — it is a public method that happens to still carry the
/// annotations of the action it once was, which is exactly why it is easy to
/// read as a route and exactly why doing so prints an endpoint that answers 404.
/// The whole block is passed over, and *silently*: nothing was refused here that
/// the author did not already refuse in writing, and a warning would tell them
/// something they wrote down themselves.
///
/// The line that terminates the block — the declaration the attributes decorate
/// — is inspected for it as well, so that `[NonAction] public IActionResult B()`
/// on one line is caught. That is deliberately asymmetric with the verb
/// attributes, which are read only from the block proper: looking further for a
/// suppressor can only ever remove a route, while looking further for a verb
/// could add one, and only the first direction is safe to take on a line this
/// scanner does not otherwise claim to understand.
#[allow(clippy::too_many_arguments)]
fn scan_actions(
    project: &Project,
    file: &SourceFile,
    class_template: &str,
    stem: &str,
    from: usize,
    to: usize,
    found: &mut Found,
) {
    let mut line_index = from;
    while line_index < to {
        if !file.is_attribute_line(line_index) {
            line_index += 1;
            continue;
        }

        let mut end = line_index;
        let mut attributes: Vec<ParsedAttribute> = Vec::new();
        while end < to && (file.is_attribute_line(end) || file.is_blank_or_comment(end)) {
            attributes.extend(file.attributes_on(end));
            end += 1;
        }

        let verbs: Vec<&ParsedAttribute> = attributes
            .iter()
            .filter(|a| verb_of(&a.name).is_some())
            .collect();
        let excluded = attributes.iter().any(is_non_action)
            || (end < to && file.attributes_on(end).iter().any(is_non_action));
        if !verbs.is_empty() && !excluded {
            if attributes.iter().any(|a| a.name == "Route") {
                found.warnings.push(refusal(
                    &project.name,
                    "an action",
                    &file.path,
                    Some(file.line_number(line_index)),
                    "its verb attribute sits beside an action-level [Route], and composing the \
                     two would be a guess about which carries the template",
                ));
            } else {
                for attribute in verbs {
                    let verb = verb_of(&attribute.name).expect("filtered above");
                    let action =
                        match template_of(file, attribute, Some(stem), &project.name, found) {
                            Slot::Resolved(template) => Some(template),
                            // `[HttpGet]` and `[HttpGet(Name = "x")]` both declare a
                            // verb and no template: the route is the class route.
                            Slot::Missing => None,
                            Slot::Refused => continue,
                        };
                    if class_template.is_empty() && action.is_none() {
                        found.warnings.push(refusal(
                            &project.name,
                            &format!("a {verb} action"),
                            &file.path,
                            Some(attribute.line),
                            "neither its class nor the action itself declares a route template",
                        ));
                        continue;
                    }
                    found.signals.push(route_signal(
                        project,
                        &file.path,
                        attribute.line,
                        &file.excerpt(attribute.line),
                        verb,
                        &compose(class_template, action.as_deref()),
                    ));
                }
            }
        }

        line_index = end.max(line_index + 1);
    }
}

/// The class name with its `Controller` suffix removed, or `None` when the
/// class is not a controller candidate.
///
/// This substitution is the one token expansion that is safe to perform,
/// because it is not an inference: ASP.NET defines `[controller]` as exactly
/// the class name minus the suffix, so the answer is determined by text already
/// on the screen. The case is preserved rather than lower-cased — route
/// matching is case-insensitive, but the string ASP.NET substitutes is the
/// declared spelling, and printing a different one would be this producer
/// editing the author's text.
fn controller_stem(name: &str) -> Option<String> {
    let stem = name.strip_suffix("Controller")?;
    (!stem.is_empty()).then(|| stem.to_string())
}

// ---------------------------------------------------------------------------
// Minimal APIs
// ---------------------------------------------------------------------------

/// Find `app.MapGet("/x", …)` style registrations in one file.
///
/// # Why a `MapGroup` disqualifies the whole file
///
/// The plan says group prefixes are never concatenated, and the reason is that
/// the group is a *variable*:
///
/// ```text
/// var group = app.MapGroup("/api/v1");
/// group.MapGet("/orders", …);
/// ```
///
/// The endpoint that really exists is `/api/v1/orders`. Emitting `/orders`
/// invents an endpoint that does not exist, and emitting `/api/v1/orders`
/// requires knowing that `group` is still the group at that point — which needs
/// flow analysis, because the variable can be reassigned, passed to a method,
/// returned from one, or built in a loop.
///
/// So the refusal is scoped to everything the group could have reached rather
/// than to the calls whose receiver happens to be the group's identifier.
/// Distinguishing `app.MapGet` from `group.MapGet` by name is precisely the
/// partial analysis described above, with the same failure and none of the
/// honesty: it is right until somebody writes `app = app.MapGroup(...)` or hands
/// the group to a helper, and when it is wrong it is wrong silently. Losing a
/// handful of real routes in a project that uses groups is the cheaper half of
/// that trade, and the loss is reported rather than hidden.
///
/// # Why "everything the group could have reached" is the whole project
///
/// The scope used to be the file, and a file is not where a group ends. The
/// layout every .NET 8 tutorial teaches splits it in two:
///
/// ```text
/// // Program.cs
/// app.MapGroup("/api/v1").MapOrders();
/// // OrderEndpoints.cs
/// public static RouteGroupBuilder MapOrders(this RouteGroupBuilder group) {
///     group.MapGet("/orders", …);
/// }
/// ```
///
/// `OrderEndpoints.cs` contains no `MapGroup`, so a file-scoped rule read it as
/// ordinary and emitted `GET /orders` — an endpoint that does not exist, while
/// simultaneously warning that nothing in `Program.cs` could be resolved. The
/// user got a wrong answer and a disclaimer about the wrong file.
///
/// Two things now put a project into the grouped state, and either alone is
/// enough:
///
/// * a `MapGroup` call anywhere in it, and
/// * a mention of the `RouteGroupBuilder` type anywhere in it, which is how the
///   second file above announces that its endpoints are mounted under a prefix
///   its *caller* chose. This one catches the split layout even when the
///   `MapGroup` call lives in a different project, and it is a type name written
///   by the author rather than an inference drawn from one.
///
/// Both are blunt: a project with one grouped endpoint loses the route list for
/// its ungrouped ones too. That is the direction this file always errs in, and
/// every lost endpoint is named in a warning, so the answer degrades to "we did
/// not resolve these" and never to a confident wrong path.
///
/// Returns whether this file puts its project into the grouped state. Routes
/// are appended to `pending` rather than emitted, because at this point the
/// answer for the project is not yet known; see [`scan_project`].
fn scan_map_calls(
    project: &Project,
    file: &SourceFile,
    found: &mut Found,
    pending: &mut Vec<PendingRoute>,
) -> bool {
    let mut grouped = false;

    // The type mention is looked for whether or not the file registers
    // anything, since the whole point of it is a file that registers endpoints
    // it does not own the prefix of — or, just as usefully, a file that hands
    // the group to somebody else.
    if let Some(line) = file.route_group_builder_mention() {
        grouped = true;
        found.warnings.push(refusal(
            &project.name,
            "a RouteGroupBuilder parameter",
            &file.path,
            Some(line),
            "a method that receives a route group registers its endpoints under a prefix its \
             caller chooses, so no minimal-API route in this project was resolved",
        ));
    }

    let mut calls: Vec<MapCall> = Vec::new();
    for line_index in 0..file.lines.len() {
        calls.extend(file.map_calls(line_index));
    }
    if calls.is_empty() {
        return grouped;
    }
    found.saw_candidate = true;

    for call in &calls {
        let line = file.line_number(call.line_index);

        if let Some((_, why)) = DECLINED_MAPS.iter().find(|(name, _)| *name == call.name) {
            found.warnings.push(refusal(
                &project.name,
                &format!("the {} registration", call.name),
                &file.path,
                Some(line),
                why,
            ));
            continue;
        }

        if call.name == "MapGroup" {
            grouped = true;
            found.warnings.push(refusal(
                &project.name,
                "a MapGroup route group",
                &file.path,
                Some(line),
                "the endpoints registered on a group cannot be matched to it without flow \
                 analysis, so no minimal-API route in this project was resolved",
            ));
            continue;
        }

        let Some(verb) = VERBS
            .iter()
            .find(|(suffix, _)| call.name == format!("Map{suffix}"))
            .map(|(_, verb)| *verb)
        else {
            // Some other `Map*` extension method — `MapControllers`,
            // `MapDefaultEndpoints`, a library's own. There is an open-ended
            // supply of these and none of them states a route, so they are
            // passed over in silence rather than filling the warnings with
            // every method whose name happens to begin with `Map`.
            continue;
        };

        pending.push(PendingRoute {
            path: file.path.clone(),
            line,
            name: call.name.clone(),
            verb,
            excerpt: file.excerpt(line),
            argument: match file.literal_argument(call.line_index, call.open) {
                Argument::Literal(raw) => match resolve_template(&raw, None) {
                    Ok(template) => Pattern::Template(template),
                    Err(raw) => Pattern::UnexpandableToken(raw),
                },
                Argument::Named | Argument::Absent | Argument::NotLiteral => Pattern::NotLiteral,
            },
        });
    }

    grouped
}

/// Turn the registrations a whole project offered into signals, now that
/// [`scan_project`] knows whether the project uses route groups.
fn emit_pending(project: &Project, grouped: bool, pending: Vec<PendingRoute>, found: &mut Found) {
    for route in pending {
        if grouped {
            found.warnings.push(refusal(
                &project.name,
                &format!("the {} endpoint", route.name),
                &route.path,
                Some(route.line),
                "this project registers endpoints on a route group, and the prefix any one \
                 endpoint is mounted under cannot be established without flow analysis",
            ));
            continue;
        }
        match route.argument {
            Pattern::Template(template) => found.signals.push(route_signal(
                project,
                &route.path,
                route.line,
                &route.excerpt,
                route.verb,
                &compose("", Some(&template)),
            )),
            Pattern::UnexpandableToken(raw) => found.warnings.push(refusal(
                &project.name,
                &format!("the route '{raw}'"),
                &route.path,
                Some(route.line),
                "it contains a routing token that only the framework can expand",
            )),
            Pattern::NotLiteral => found.warnings.push(refusal(
                &project.name,
                &format!("the {} registration", route.name),
                &route.path,
                Some(route.line),
                "its pattern is not a literal string, and following the expression that produces \
                 it would be a guess",
            )),
        }
    }
}

/// One `receiver.MapSomething(` occurrence.
struct MapCall {
    line_index: usize,
    name: String,
    /// Byte offset of the first character after the opening parenthesis.
    open: usize,
}

/// A minimal-API registration that has been read but not yet judged, because
/// judging it needs the project-wide answer from [`scan_project`].
struct PendingRoute {
    path: PathBuf,
    /// 1-based.
    line: u32,
    /// The `Map*` method as written, for the warning text.
    name: String,
    verb: &'static str,
    excerpt: String,
    argument: Pattern,
}

/// What the pattern argument of a registration turned out to be.
///
/// The refusals are carried rather than reported on the spot so that the
/// project-wide group check keeps the precedence it had when it was file-wide:
/// an endpoint under a group is refused for being under a group, whatever its
/// pattern was written as.
enum Pattern {
    Template(String),
    /// A literal holding a token this producer will not expand, kept verbatim.
    UnexpandableToken(String),
    NotLiteral,
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

/// What an attribute's first argument turned out to be.
enum Slot {
    Resolved(String),
    /// The attribute declares no template — `[HttpGet]`, or a named argument
    /// such as `[HttpGet(Name = "x")]`, which sets a route *name* and not a
    /// path.
    Missing,
    /// Something was there and could not be resolved. A warning has already
    /// been pushed.
    Refused,
}

fn template_of(
    file: &SourceFile,
    attribute: &ParsedAttribute,
    stem: Option<&str>,
    project: &str,
    found: &mut Found,
) -> Slot {
    let Some(open) = attribute.open else {
        return Slot::Missing;
    };
    match file.literal_argument(attribute.line_index, open) {
        Argument::Absent | Argument::Named => Slot::Missing,
        Argument::NotLiteral => {
            found.warnings.push(refusal(
                project,
                &format!("the [{}] attribute", attribute.name),
                &file.path,
                Some(attribute.line),
                "its template is not a literal string, and following the expression that \
                 produces it would be a guess",
            ));
            Slot::Refused
        }
        Argument::Literal(raw) => match resolve_template(&raw, stem) {
            Ok(template) => Slot::Resolved(template),
            Err(raw) => {
                found.warnings.push(refusal(
                    project,
                    &format!("the route template '{raw}'"),
                    &file.path,
                    Some(attribute.line),
                    "it contains a routing token that only the framework can expand",
                ));
                Slot::Refused
            }
        },
    }
}

/// Expand the tokens in a route template, or report the template verbatim when
/// one of them cannot be expanded.
///
/// `[controller]` is expanded, because ASP.NET defines it as the class name
/// minus its suffix and nothing about that is a judgement call. Everything else
/// — `[action]`, `[area]`, and anything a future version adds — is refused, and
/// the whole template comes back untouched so the warning can quote what the
/// author actually wrote rather than a partially-rewritten version of it. Token
/// names are matched case-insensitively, matching the framework.
///
/// `{id}` and `{id:int}` are route *parameters*, not tokens: they survive into
/// the output verbatim, exactly as they appear in the URL a client would call.
fn resolve_template(raw: &str, stem: Option<&str>) -> Result<String, String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']').map(|at| open + at) else {
            return Err(raw.to_string());
        };
        let token = &rest[open + 1..close];
        let Some(stem) = stem.filter(|_| token.eq_ignore_ascii_case("controller")) else {
            return Err(raw.to_string());
        };
        out.push_str(&rest[..open]);
        out.push_str(stem);
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Join a class template and an optional action template into one path.
///
/// An action template beginning with `/` or `~/` replaces the class template
/// outright; that is the framework's own rule for an absolute route, and it is
/// as literal as the `[controller]` substitution.
fn compose(class_template: &str, action: Option<&str>) -> String {
    let trim = |s: &str| s.trim_start_matches('~').trim_matches('/').to_string();
    let path = match action {
        Some(action) if action.starts_with('/') || action.starts_with("~/") => trim(action),
        Some(action) => {
            let (class, action) = (trim(class_template), trim(action));
            match (class.is_empty(), action.is_empty()) {
                (true, _) => action,
                (false, true) => class,
                (false, false) => format!("{class}/{action}"),
            }
        }
        None => trim(class_template),
    };
    format!("/{path}")
}

/// Whether an attribute is the author telling the framework this method is not
/// an action.
fn is_non_action(attribute: &ParsedAttribute) -> bool {
    attribute.name == "NonAction"
}

fn verb_of(attribute: &str) -> Option<&'static str> {
    VERBS
        .iter()
        .find(|(suffix, _)| attribute == format!("Http{suffix}"))
        .map(|(_, verb)| *verb)
}

// ---------------------------------------------------------------------------
// Emitting
// ---------------------------------------------------------------------------

fn route_signal(
    project: &Project,
    path: &Path,
    line: u32,
    excerpt: &str,
    verb: &str,
    route: &str,
) -> Signal {
    // `Signal::medium` is the only constructor used anywhere in this file, and
    // that is the whole of the grading discipline here: a route can never be
    // the reason a service is drawn. The test suite pins it rather than an
    // assertion doing so, because an assertion here would only re-state what
    // the line above it says.
    Signal::medium(
        ComponentKind::HttpService,
        project.name.clone(),
        project.id.clone(),
        Evidence::new(path.to_path_buf(), Some(line), excerpt),
    )
    .with_detail(format!("{verb} {route}"))
}

fn refusal(project: &str, subject: &str, path: &Path, line: Option<u32>, why: &str) -> String {
    let where_ = match line {
        Some(line) => format!("{}:{line}", display(path)),
        None => display(path),
    };
    format!("{project}: {subject} at {where_} was not read as a route because {why}")
}

fn display(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn is_csharp(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cs"))
}

// ---------------------------------------------------------------------------
// Reading source text
// ---------------------------------------------------------------------------

/// One file, with each line's code positions already worked out.
struct SourceFile {
    path: PathBuf,
    lines: Vec<String>,
    /// Per line, one flag per byte: true where that byte is code rather than a
    /// string literal or a comment. See [`Masker`].
    code: Vec<Vec<bool>>,
}

impl SourceFile {
    fn new(path: &Path, text: &str) -> Self {
        let mut masker = Masker::default();
        let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
        let code = lines.iter().map(|line| masker.mask(line)).collect();
        Self {
            path: path.to_path_buf(),
            lines,
            code,
        }
    }

    /// The 0-based index of a 1-based line number the index recorded.
    fn index_of(&self, line: u32) -> Option<usize> {
        let index = (line as usize).checked_sub(1)?;
        (index < self.lines.len()).then_some(index)
    }

    fn line_number(&self, index: usize) -> u32 {
        index as u32 + 1
    }

    /// The trimmed and truncated text of a 1-based line, as evidence.
    fn excerpt(&self, line: u32) -> String {
        self.lines
            .get((line as usize).saturating_sub(1))
            .map(|l| l.trim())
            .unwrap_or_default()
            .chars()
            .take(MAX_EXCERPT_CHARS)
            .collect()
    }

    /// Whether a declaration line carries the `abstract` modifier.
    ///
    /// A whole-word match over code positions only, so `AbstractionsController`
    /// and a commented-out `// abstract` are both left alone. A declaration
    /// split across lines with the modifier on an earlier one reads as
    /// non-abstract; that direction loses nothing, because the class then falls
    /// to the ordinary rules and is refused unless it carries the attributes.
    fn declares_abstract(&self, index: usize) -> bool {
        self.word_at(index, b"abstract").is_some()
    }

    /// The 1-based line of the first mention of `RouteGroupBuilder` in code.
    fn route_group_builder_mention(&self) -> Option<u32> {
        (0..self.lines.len())
            .find(|index| self.word_at(*index, b"RouteGroupBuilder").is_some())
            .map(|index| self.line_number(index))
    }

    /// The byte offset of `word` on a line, matched whole and in code only.
    fn word_at(&self, index: usize, word: &[u8]) -> Option<usize> {
        let line = self.lines[index].as_bytes();
        let code = &self.code[index];
        let boundary = |at: usize| !(line[at].is_ascii_alphanumeric() || line[at] == b'_');
        (0..line.len()).find(|&at| {
            code[at]
                && line[at..].starts_with(word)
                && (at == 0 || boundary(at - 1))
                && (at + word.len() >= line.len() || boundary(at + word.len()))
        })
    }

    /// Whether a line is entirely comment, blank, or the inside of a string
    /// that began on an earlier line — that is, whether it contributes no code.
    fn is_blank_or_comment(&self, index: usize) -> bool {
        self.code_chars(index).next().is_none()
    }

    /// Whether a line consists only of attributes.
    fn is_attribute_line(&self, index: usize) -> bool {
        let mut chars = self.code_chars(index).map(|(_, c)| c);
        let Some(first) = chars.next() else {
            return false;
        };
        first == '[' && chars.last().is_some_and(|last| last == ']')
    }

    /// The non-whitespace code characters of a line, with their byte offsets.
    fn code_chars(&self, index: usize) -> impl Iterator<Item = (usize, char)> + '_ {
        let code = &self.code[index];
        self.lines[index]
            .char_indices()
            .filter(move |(at, c)| code[*at] && !c.is_whitespace())
    }

    /// The attribute block immediately above a declaration, plus any attributes
    /// on the declaration's own line.
    ///
    /// The walk upward stops at the first line that is neither an attribute nor
    /// blank/comment — a closing brace, a `namespace`, another declaration —
    /// because everything above that belongs to something else. A commented-out
    /// attribute is skipped rather than read, which is the whole point of doing
    /// this over [`Masker`]'s output: `// [ApiController]` is invisible here,
    /// and a class carrying only that is refused as having no attribute at all.
    fn attribute_block_above(&self, index: usize) -> Vec<ParsedAttribute> {
        let mut lines: Vec<usize> = vec![index];
        let mut at = index;
        while at > 0 {
            at -= 1;
            if self.is_attribute_line(at) || self.is_blank_or_comment(at) {
                lines.push(at);
            } else {
                break;
            }
        }
        lines.sort_unstable();
        lines
            .into_iter()
            .flat_map(|line| self.attributes_on(line))
            .collect()
    }

    /// Every attribute written on one line.
    ///
    /// Handles the comma form (`[ApiController, Route("api")]`), which is
    /// legal C# and which a search for the literal text `[Route(` would miss.
    fn attributes_on(&self, index: usize) -> Vec<ParsedAttribute> {
        let line = self.lines[index].as_bytes();
        let code = &self.code[index];
        let mut out = Vec::new();
        let mut at = 0usize;

        while at < line.len() {
            if !(code[at] && line[at] == b'[') {
                at += 1;
                continue;
            }
            let mut cursor = at + 1;
            loop {
                cursor = skip_spaces(line, cursor);
                let start = cursor;
                while cursor < line.len()
                    && code[cursor]
                    && (line[cursor].is_ascii_alphanumeric()
                        || line[cursor] == b'_'
                        || line[cursor] == b'.')
                {
                    cursor += 1;
                }
                if cursor == start {
                    break;
                }
                // `[Microsoft.AspNetCore.Mvc.Route("x")]` is the same attribute
                // as `[Route("x")]`, and `[RouteAttribute]` is the same as
                // `[Route]` — both are spellings C# itself treats as equal.
                let name = std::str::from_utf8(&line[start..cursor])
                    .unwrap_or_default()
                    .rsplit('.')
                    .next()
                    .unwrap_or_default();
                let name = name.strip_suffix("Attribute").unwrap_or(name).to_string();

                cursor = skip_spaces(line, cursor);
                let open = (cursor < line.len() && line[cursor] == b'(').then(|| {
                    let open = cursor + 1;
                    cursor = match matching(line, code, cursor, b'(', b')') {
                        Some(close) => close + 1,
                        None => line.len(),
                    };
                    open
                });
                out.push(ParsedAttribute {
                    name,
                    line: self.line_number(index),
                    line_index: index,
                    open,
                });

                cursor = skip_spaces(line, cursor);
                if cursor < line.len() && line[cursor] == b',' {
                    cursor += 1;
                    continue;
                }
                break;
            }
            at = cursor.max(at + 1);
        }
        out
    }

    /// Every `receiver.MapSomething(` written on one line.
    ///
    /// The `.` before `Map` is required, which is what keeps a locally defined
    /// `MapGet(…)` helper and any identifier merely *ending* in `Map` out. A
    /// generic argument list is stepped over, so `MapHub<ChatHub>("/chat")` is
    /// recognised as `MapHub`.
    fn map_calls(&self, index: usize) -> Vec<MapCall> {
        let line = self.lines[index].as_bytes();
        let code = &self.code[index];
        let mut out = Vec::new();

        for at in 0..line.len() {
            if !code[at] || !line[at..].starts_with(b"Map") || at == 0 || line[at - 1] != b'.' {
                continue;
            }
            let mut cursor = at + 3;
            let start = cursor;
            while cursor < line.len()
                && code[cursor]
                && (line[cursor].is_ascii_alphanumeric() || line[cursor] == b'_')
            {
                cursor += 1;
            }
            if cursor == start {
                continue;
            }
            let name = format!(
                "Map{}",
                std::str::from_utf8(&line[start..cursor]).unwrap_or_default()
            );

            if cursor < line.len() && line[cursor] == b'<' {
                match matching(line, code, cursor, b'<', b'>') {
                    Some(close) => cursor = close + 1,
                    None => continue,
                }
            }
            cursor = skip_spaces(line, cursor);
            if cursor >= line.len() || line[cursor] != b'(' {
                continue;
            }
            out.push(MapCall {
                line_index: index,
                name,
                open: cursor + 1,
            });
        }
        out
    }

    /// The first argument of a call whose `(` ends at byte `open`.
    fn literal_argument(&self, index: usize, open: usize) -> Argument {
        let line = self.lines[index].as_bytes();
        let mut at = skip_spaces(line, open);
        if at >= line.len() {
            return Argument::Absent;
        }
        if line[at] == b')' {
            return Argument::Absent;
        }

        let verbatim = line[at] == b'@' && at + 1 < line.len() && line[at + 1] == b'"';
        if verbatim {
            at += 1;
        }
        if line[at] != b'"' {
            // An identifier followed by `=` is a named argument — `Name = "x"`
            // sets a route name, not a template — and is not the same thing as
            // an expression that could not be read.
            let start = at;
            let mut cursor = at;
            while cursor < line.len()
                && (line[cursor].is_ascii_alphanumeric() || line[cursor] == b'_')
            {
                cursor += 1;
            }
            if cursor > start {
                let after = skip_spaces(line, cursor);
                if after < line.len() && line[after] == b'=' && line.get(after + 1) != Some(&b'=') {
                    return Argument::Named;
                }
            }
            return Argument::NotLiteral;
        }

        let mut cursor = at + 1;
        let content_start = cursor;
        let end;
        loop {
            if cursor >= line.len() {
                // The literal runs past the end of the line: a verbatim or raw
                // string spanning lines. Not read, rather than half-read.
                return Argument::NotLiteral;
            }
            match line[cursor] {
                // An escape or a doubled quote means the template contains a
                // character this producer would have to decode. Route templates
                // do not, so the shape is evidence of something else going on
                // and the argument is refused rather than decoded.
                b'\\' if !verbatim => return Argument::NotLiteral,
                b'"' if verbatim && line.get(cursor + 1) == Some(&b'"') => {
                    return Argument::NotLiteral
                }
                b'"' => {
                    end = cursor;
                    break;
                }
                _ => cursor += 1,
            }
        }

        // Whatever follows must close the argument. Anything else — a `+`, a
        // `.`, an interpolation — means the pattern is an expression that
        // merely begins with a literal.
        let after = skip_spaces(line, end + 1);
        if after >= line.len() || (line[after] != b',' && line[after] != b')') {
            return Argument::NotLiteral;
        }

        match std::str::from_utf8(&line[content_start..end]) {
            Ok(raw) => Argument::Literal(raw.to_string()),
            Err(_) => Argument::NotLiteral,
        }
    }
}

/// One attribute occurrence.
struct ParsedAttribute {
    name: String,
    /// 1-based.
    line: u32,
    line_index: usize,
    /// Byte offset of the first character after `(`, when the attribute has an
    /// argument list.
    open: Option<usize>,
}

enum Argument {
    Literal(String),
    /// A named argument such as `Name = "x"`.
    Named,
    /// No argument at all.
    Absent,
    NotLiteral,
}

fn skip_spaces(line: &[u8], mut at: usize) -> usize {
    while at < line.len() && (line[at] == b' ' || line[at] == b'\t') {
        at += 1;
    }
    at
}

/// The offset of the delimiter matching the one at `at`, counting only code
/// positions.
fn matching(line: &[u8], code: &[bool], at: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, byte) in line.iter().enumerate().skip(at) {
        if !code[offset] {
            continue;
        }
        if *byte == open {
            depth += 1;
        } else if *byte == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(offset);
            }
        }
    }
    None
}

/// Works out, one line at a time, which bytes of a C# file are code.
///
/// # What it tracks, and what it does not
///
/// It tracks the three lexical states that survive a line ending — a block
/// comment, a verbatim (`@"…"`) string and a raw (`"""…"""`) string — because
/// each of them can hide a `MapGet` on a later line, and a scanner that reset
/// at every newline would read commented-out code as live. Within a line it
/// handles line comments, ordinary strings with backslash escapes, verbatim
/// strings with `""` escapes, and character literals.
///
/// It is not a lexer, and two things are knowingly approximated:
///
/// * A raw string's fence is treated as exactly `"""`. C# allows longer fences
///   and interpolated raw strings (`$$"""…"""`). A longer fence opens and
///   closes on the same token here, so the run is masked either way; the
///   failure mode is *more* masking, which loses routes rather than inventing
///   them.
/// * Preprocessor directives are not evaluated. Code inside `#if false` is read
///   as code. That is the same answer an editor's syntax highlighting gives, and
///   the alternative is evaluating a build's symbol set, which this crate does
///   not know.
///
/// The output is one flag per *byte* rather than a rewritten string so that
/// offsets into the original line stay exact — the literal a route is read from
/// has to be sliced out of the real text, and a mask that replaced multi-byte
/// characters with single spaces would silently shift every offset after the
/// first non-ASCII character in the file.
#[derive(Default)]
struct Masker {
    in_block_comment: bool,
    in_verbatim_string: bool,
    in_raw_string: bool,
}

impl Masker {
    fn mask(&mut self, line: &str) -> Vec<bool> {
        let b = line.as_bytes();
        let mut code = vec![false; b.len()];
        let mut i = 0usize;

        while i < b.len() {
            if self.in_block_comment {
                if b[i..].starts_with(b"*/") {
                    self.in_block_comment = false;
                    i += 2;
                } else {
                    i += 1;
                }
                continue;
            }
            if self.in_raw_string {
                if b[i..].starts_with(b"\"\"\"") {
                    self.in_raw_string = false;
                    i += 3;
                } else {
                    i += 1;
                }
                continue;
            }
            if self.in_verbatim_string {
                if b[i] == b'"' {
                    if b.get(i + 1) == Some(&b'"') {
                        i += 2;
                    } else {
                        self.in_verbatim_string = false;
                        i += 1;
                    }
                } else {
                    i += 1;
                }
                continue;
            }

            if b[i..].starts_with(b"\"\"\"") {
                self.in_raw_string = true;
                i += 3;
                continue;
            }
            if b[i..].starts_with(b"//") {
                break;
            }
            if b[i..].starts_with(b"/*") {
                self.in_block_comment = true;
                i += 2;
                continue;
            }
            if b[i] == b'@' && b.get(i + 1) == Some(&b'"') {
                code[i] = true;
                self.in_verbatim_string = true;
                i += 2;
                continue;
            }
            if b[i] == b'"' {
                i += 1;
                while i < b.len() {
                    match b[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                continue;
            }
            if b[i] == b'\'' {
                i += 1;
                while i < b.len() {
                    match b[i] {
                        b'\\' => i += 2,
                        b'\'' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                continue;
            }

            code[i] = true;
            i += 1;
        }
        code
    }
}

// ---------------------------------------------------------------------------
// Which project owns a file
// ---------------------------------------------------------------------------

/// Which project, if any, a workspace-relative file belongs to.
///
/// Longest matching directory wins and matching is component-wise, mirroring
/// [`crate::symbols::index`]'s own ownership rule. It is re-derived here rather
/// than shared because the index keeps its version private and the answer is
/// four lines; where the two could disagree is on nesting, and both resolve it
/// the same way — a test project nested under the project it tests keeps its own
/// files.
struct Owners<'a> {
    entries: Vec<(String, &'a Project)>,
}

impl<'a> Owners<'a> {
    fn new(root: &Path, projects: &'a [Project]) -> Self {
        let mut entries: Vec<(String, &'a Project)> = projects
            .iter()
            .filter_map(|p| {
                let relative = p.dir.strip_prefix(root).ok()?;
                Some((display(relative), p))
            })
            .collect();
        entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        Self { entries }
    }

    fn owner_of(&self, relative: &Path) -> Option<&'a Project> {
        let path = display(relative);
        self.entries
            .iter()
            .find(|(dir, _)| {
                dir.is_empty()
                    || path
                        .strip_prefix(dir.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            .map(|(_, project)| *project)
    }
}
