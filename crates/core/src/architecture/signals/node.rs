//! Component signals read out of `package.json`, out of the framework
//! conventions that turn a *filename* into a URL, and — for one narrowly
//! fenced case — out of the project's own source.
//!
//! Everything here passes through [`admit`](super::framework::admit); see
//! [`super`] for the grading rule and the standing prohibitions that gate
//! applies. This file's job is to decide what is worth *offering* to the gate,
//! and the interesting decisions are all about what it declines to offer.
//!
//! # A dependency name is a declared fact; a version range is not evidence
//!
//! `"pg": "^8.11.3"` in `dependencies` is the author writing down that this
//! project speaks the PostgreSQL wire protocol. The key is the fact. The
//! *value* is deliberately never quoted: every dependency evidence excerpt is
//! built with [`Evidence::elided_value`](super::framework::Evidence::elided_value)
//! and reads `"pg": <value not read>`.
//!
//! That is not caution for its own sake. npm version specifiers are a general
//! URL syntax — `git+ssh://git@host/repo.git#tag`, and in private-registry
//! setups `https://x-access-token:…@github.com/…` — so the right-hand side of
//! a dependency line is a place credentials genuinely live. The central secret
//! screen in [`super::framework`] matches `key=value` shapes and would not
//! catch a colon-separated token inside a URL, and a diagram is a thing people
//! export and paste. The version adds nothing a reader of an architecture
//! diagram wants, so it is never read, and the failure mode is designed out
//! rather than screened for.
//!
//! # The ORM problem
//!
//! `@prisma/client`, `typeorm`, `knex`, `sequelize` and `drizzle-orm` are
//! database-*agnostic*. A dependency on one proves that this project talks to
//! a relational database and says nothing whatsoever about which engine. Three
//! answers were available and the choice is the most consequential one in this
//! file:
//!
//! 1. **A `Database` box labelled with the ORM's name.** Rejected. A box
//!    labelled `Prisma` beside a box labelled `PostgreSQL` reads as *"the
//!    database is called Prisma"*, which is false, and it is false in the most
//!    damaging way — confidently, in the same visual weight as the true box
//!    next to it.
//! 2. **No box at all.** Rejected too, and this is the closer call. The
//!    manifest *does* declare a fact: there is a database here. Refusing to
//!    draw anything discards a true statement because a different, unasked
//!    statement is unknown, and it leaves a service that obviously persists
//!    something looking like it persists nothing.
//! 3. **A `Database` box labelled `Database (via Prisma)`.** Chosen. The label
//!    states exactly what is known ("a database") and exactly how it came to
//!    be known ("via Prisma"), and it cannot be misread as an engine name
//!    because the word `Database` is where the engine name would be. The
//!    identity rule on [`Component`](super::framework::Component) then does
//!    the right thing for free: two projects on Prisma with unnamed engines
//!    share one box, and neither is confused with a named engine.
//!
//! **When the engine *is* named, the vague box must not appear beside it.** A
//! project declaring both `@prisma/client` and `pg` gets one box, `PostgreSQL`;
//! the ORM signal is suppressed and the suppression is reported in
//! [`NodeSignals::warnings`] rather than done quietly. Emitting both would
//! produce two database boxes for one database, which is the exact failure
//! mode the identity rule exists to prevent.
//!
//! # `prisma/schema.prisma` is real evidence that may not create anything
//!
//! `datasource db { provider = "postgresql" }` is a declared fact in a file,
//! which is the bar this phase sets for HIGH. It is nevertheless emitted at
//! **MEDIUM**, and the reason is worth writing down because it looks like a
//! demotion and is not.
//!
//! HIGH is not defined here as "looks authoritative to the producer". It is
//! defined centrally, in `framework::is_declaration_file`, as an allowlist of
//! file kinds, and `.prisma` is not on it. A HIGH signal citing
//! `schema.prisma` is refused outright as `UnverifiableEvidence` — that was
//! verified against the gate, not assumed. Widening that allowlist is a change
//! to the phase-wide rule and is not this file's to make, and emitting a
//! signal known to be refused would only manufacture warnings.
//!
//! MEDIUM turns out to be the honest shape anyway. The ORM dependency in the
//! manifest is what asserts the database exists; the schema file only says
//! which engine it is. Enriching the box the manifest earned with
//! `engine: PostgreSQL` is precisely "a supporting signal adding a name to a
//! declared component", which is what MEDIUM is for. Only a quoted literal is
//! read: `provider = env("DB_PROVIDER")` is a deployment-time choice, not a
//! declaration, and it is abstained from and warned about. Only the
//! `datasource` block is read — the `generator` block has a `provider` too and
//! it names a code generator, not a database.
//!
//! # A devDependency is a weaker claim, so it is a weaker signal
//!
//! `devDependencies` is the toolchain: test doubles, migration CLIs, fixtures.
//! `better-sqlite3` there is very often a test database that no deployment
//! ever sees. A dev-only data client therefore produces a MEDIUM signal, which
//! by the grading rule can enrich a box some *other* project genuinely
//! declared but can never create one and never draws an edge. A repository
//! where the only mention of PostgreSQL is in `devDependencies` draws no
//! database and says so in a warning.
//!
//! Type-only packages are dropped before any lookup. `@types/pg` is a set of
//! declaration files; it is not a database client and never appears at
//! runtime. Exact-name matching already means `@types/pg` is not `pg`, but the
//! `@types/` prefix is refused explicitly so that a scoped entry added to the
//! table later cannot quietly start matching one.
//!
//! # File-system routing: the path *is* the route, so no code is read
//!
//! Next.js, SvelteKit and Nuxt make a directory layout into a URL space. That
//! makes route discovery a filename question rather than a parsing question —
//! but only where the framework is *declared in the manifest*. An `app/`
//! directory in a project with no `next` dependency is a directory called
//! `app`, and reading URLs out of it would be a confident wrong answer. The
//! dependency is checked first, every time.
//!
//! Segments are rendered in the spelling the author wrote: `/users/[id]`, not
//! `/users/:id`. Translating into another framework's syntax invents a
//! spelling that appears nowhere in the repository and that the author cannot
//! grep for. Route groups (`(marketing)`) are dropped because they genuinely
//! do not appear in the URL; private folders (`_internal`) are Next's own
//! opt-out and yield no route. Parallel-route slots (`@modal`) and
//! interception markers (`(..)photo`) are **abstained from with a warning**:
//! their URL depends on how the segment is composed elsewhere, and this file
//! would have to guess.
//!
//! A `basePath` — `basePath` in `next.config.*`, `baseURL` in `nuxt.config.*`,
//! `base` in `svelte.config.*` — prefixes *every* route in the application. If
//! one is present, every path this file could list would be wrong by that
//! prefix, so the whole route list is abstained from and the reason is
//! reported. The HTTP service itself still stands: the manifest declared it,
//! and only its paths are unknown.
//!
//! # Route registrations in source are MEDIUM, and mounting defeats them
//!
//! `app.get("/users", handler)` in an Express, Fastify, Koa or Hono
//! application is read only when the first argument is a **literal** string. A
//! template literal or a variable is abstained from and warned about, never
//! partially rendered.
//!
//! The dangerous case is mounting. `app.use("/api/v2", usersRouter)` means the
//! routes registered on that router answer at `/api/v2/...`, and the
//! registration is routinely in a different file from the routes. Joining the
//! two needs to know which router object was mounted where, which needs a
//! resolver this file is not. Concatenating them anyway would invent endpoints
//! that do not exist; listing the unprefixed paths would report endpoints that
//! do not exist either. So **if a mount under a non-root prefix — or any
//! Fastify `register(..., { prefix })` — appears anywhere in the project,
//! every source-read route in that project is dropped and one warning explains
//! why.** Mounting at `/` adds nothing to any path and is not treated as a
//! prefix.
//!
//! A mount whose prefix is *unreadable* — `` app.use(`${base}/v2`, r) ``,
//! `app.use(base, r)`, `app.use(config.base, r)` — suppresses exactly as a
//! literal one does, because the evidence that a prefix exists is the shape of
//! the call, not the text of its argument. Treating those as "no mount" was a
//! real defect: it produced the unprefixed list *with no warning at all*,
//! which is strictly worse than the literal case it sits beside. Hence
//! [`Mount`] has three states rather than two.
//!
//! What separates a mount from ordinary middleware is a second argument:
//! `app.use(express.json())` and `app.use(cors())` take no path and move no
//! route. Where the first argument is a literal beginning with `/` it is
//! path-shaped on its own; where it is an identifier it is not, so the
//! receiver has to carry that evidence instead and `routes_from` is required —
//! otherwise `i18n.use(plugin, options)` would silence the whole route list.
//!
//! NestJS is refused wholesale for the same reason rather than half-read: its
//! prefix lives in a `@Controller('users')` decorator on the class and its
//! method paths in `@Get(':id')` decorators inside it, so *every* Nest route
//! is the mounting problem. The service is still drawn from the manifest.
//!
//! Only receivers that plausibly route are considered — `app`, `router`,
//! `server`, `fastify`, `api`, `hono`, `koa`, and identifiers ending in
//! `Router`/`App`. Without that guard `cache.get("/tmp/x", fallback)` and
//! `headers.get(name)` become routes and, worse, become *warnings*, burying
//! the real abstentions in noise from every map lookup in the repository.
//!
//! ## Why the symbol index is not consulted here
//!
//! [`crate::symbols`] already knows where every declaration in the workspace
//! is, and reusing it instead of re-walking would be the obvious economy. It
//! cannot help: a route registration is a *call expression*, and
//! `symbols::declarations::declaration` only claims a symbol on a line
//! carrying one of its `DECLARING` keywords. `app.get("/users", h)` has none,
//! so the index holds nothing about it — checked against that module rather
//! than assumed. The index would answer "which function encloses this line",
//! which is not a question a route list asks.
//!
//! Reading is line-based, scoped to the project directory, filtered through
//! [`crate::workspace::source_walker`] so `node_modules`, `dist` and `.next`
//! are never entered, byte-capped per file and count-capped per project.
//! Comments are stripped with a string-aware scanner that tracks block
//! comments across lines, because a commented-out registration says the
//! opposite of what it looks like and `/* … */` spanning three lines is how
//! that usually appears.

use std::fs;
use std::path::{Path, PathBuf};

use crate::adapters::node::{parse_package_json, PackageJson};
use crate::model::Project;
use crate::workspace::source_walker;

use super::framework::{ComponentKind, Evidence, Signal};

/// Per-file read cap. Generated bundles and vendored single-file builds turn up
/// inside source trees; a route scan has no business reading megabytes.
const MAX_FILE_BYTES: usize = 256 * 1024;

/// Per-project file cap, for the same reason at the other scale.
const MAX_SCANNED_FILES: usize = 4000;

/// What this producer found, and what it looked at and refused.
///
/// Warnings are returned alongside the signals rather than folded into them
/// because the two kinds of refusal have different homes. A signal the *gate*
/// refuses is reported by [`Admitted::warnings`](super::framework::Admitted::warnings)
/// with a `DiscardReason` behind it; those reasons are the phase-wide rules and
/// are not this file's to extend. The refusals here are local and specific —
/// "this path is a template literal", "this application has a mount prefix" —
/// and there is no honest way to express them as a signal, because the whole
/// point is that no signal was emitted. Both lists end up in
/// [`ArchGraph::warnings`](super::super::graph::ArchGraph::warnings), which is
/// where the user reads them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeSignals {
    pub signals: Vec<Signal>,
    pub warnings: Vec<String>,
}

/// Data clients whose package name names the engine outright.
///
/// Only clients that speak a specific protocol are listed. The name is matched
/// exactly: a package is on this list or it is not, and nothing here matches on
/// prefixes or substrings, which is how `@types/pg`, `pg-format` and
/// `redis-mock` would otherwise become databases.
const DATA_CLIENTS: &[(&str, ComponentKind, &str)] = &[
    ("pg", ComponentKind::Database, "PostgreSQL"),
    ("postgres", ComponentKind::Database, "PostgreSQL"),
    ("mysql", ComponentKind::Database, "MySQL"),
    ("mysql2", ComponentKind::Database, "MySQL"),
    ("mongodb", ComponentKind::Database, "MongoDB"),
    ("mongoose", ComponentKind::Database, "MongoDB"),
    ("better-sqlite3", ComponentKind::Database, "SQLite"),
    ("sqlite3", ComponentKind::Database, "SQLite"),
    ("redis", ComponentKind::Cache, "Redis"),
    ("ioredis", ComponentKind::Cache, "Redis"),
    ("amqplib", ComponentKind::MessageQueue, "RabbitMQ"),
    ("kafkajs", ComponentKind::MessageQueue, "Kafka"),
];

/// Database-agnostic data-access layers. See the module documentation for why
/// these are labelled the way they are.
const ORMS: &[(&str, &str)] = &[
    ("@prisma/client", "Prisma"),
    ("prisma", "Prisma"),
    ("typeorm", "TypeORM"),
    ("knex", "Knex"),
    ("sequelize", "Sequelize"),
    ("drizzle-orm", "Drizzle"),
];

/// HTTP frameworks whose presence in `dependencies` declares that this project
/// serves HTTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Framework {
    Next,
    SvelteKit,
    Nuxt,
    Express,
    Fastify,
    Koa,
    Hono,
    Nest,
}

const FRAMEWORKS: &[(&str, Framework, &str)] = &[
    ("next", Framework::Next, "Next.js"),
    ("@sveltejs/kit", Framework::SvelteKit, "SvelteKit"),
    ("nuxt", Framework::Nuxt, "Nuxt"),
    ("express", Framework::Express, "Express"),
    ("fastify", Framework::Fastify, "Fastify"),
    ("koa", Framework::Koa, "Koa"),
    ("hono", Framework::Hono, "Hono"),
    ("@nestjs/core", Framework::Nest, "NestJS"),
];

/// The methods a route registration can be spelled with.
const METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "head", "options", "all",
];

/// Source extensions worth opening. Deliberately not `.json`, `.md` or
/// `.snap`: a route registration only ever appears in code.
const SOURCE_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"];

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Read every Node signal a project offers.
///
/// `workspace_root` is used only to render evidence paths the way the rest of
/// [`super::super::graph`] renders them: relative and forward-slashed, so a
/// stored or exported diagram does not carry the deriving machine's layout.
pub fn signals(workspace_root: &Path, project: &Project) -> NodeSignals {
    let mut out = NodeSignals::default();

    if !is_node_manifest(project) {
        return out;
    }

    let manifest = relative_to(workspace_root, &project.manifest_path);
    let Ok(raw) = fs::read_to_string(&project.manifest_path) else {
        out.warn(
            project,
            format!("{manifest} could not be read, so no component was derived from it"),
        );
        return out;
    };
    let Some(pkg) = parse_package_json(&raw) else {
        out.warn(
            project,
            format!("{manifest} could not be read as JSON, so no component was derived from it"),
        );
        return out;
    };

    // Every evidence path below is built by concatenating this onto a
    // project-relative path, so the whole file works in one coordinate system.
    let prefix = relative_to(workspace_root, &project.dir);

    data_clients(&mut out, project, &pkg, &raw, &manifest, &prefix);
    http_service(&mut out, project, &pkg, &raw, &manifest, &prefix);
    out
}

impl NodeSignals {
    fn warn(&mut self, project: &Project, message: impl AsRef<str>) {
        let line = format!("{}: {}", project.name, message.as_ref());
        if !self.warnings.contains(&line) {
            self.warnings.push(line);
        }
    }
}

fn is_node_manifest(project: &Project) -> bool {
    project
        .manifest_path
        .file_name()
        .is_some_and(|name| name == "package.json")
}

// ---------------------------------------------------------------------------
// Data clients and ORMs
// ---------------------------------------------------------------------------

fn data_clients(
    out: &mut NodeSignals,
    project: &Project,
    pkg: &PackageJson,
    raw: &str,
    manifest: &str,
    prefix: &str,
) {
    let mut named_engine: Option<&str> = None;

    for &(package, kind, label) in DATA_CLIENTS {
        let runtime = pkg.dependencies.contains_key(package);
        let dev = pkg.dev_dependencies.contains_key(package);
        if is_type_package(package) || !(runtime || dev) {
            continue;
        }

        let evidence = dependency_evidence(manifest, raw, package);
        if runtime {
            if kind == ComponentKind::Database {
                named_engine = Some(label);
            }
            out.signals
                .push(Signal::high(kind, label, project.id.as_str(), evidence));
        } else {
            // Dev-only: allowed to enrich a box someone else declared, never to
            // create one. See the module documentation.
            out.signals.push(
                Signal::medium(kind, label, project.id.as_str(), evidence)
                    .with_detail("declared in devDependencies"),
            );
        }
    }

    for &(package, orm) in ORMS {
        if is_type_package(package) || !pkg.dependencies.contains_key(package) {
            continue;
        }
        let label = format!("Database (via {orm})");

        if let Some(engine) = named_engine {
            out.warn(
                project,
                format!(
                    "'{orm}' was not drawn as a separate database because this project also \
                     declares a {engine} driver, which names the engine {orm} does not"
                ),
            );
            continue;
        }

        out.signals.push(Signal::high(
            ComponentKind::Database,
            label.clone(),
            project.id.as_str(),
            dependency_evidence(manifest, raw, package),
        ));

        if orm == "Prisma" {
            prisma_engine(out, project, &label, prefix);
        }
    }
}

/// Type-only packages never describe a runtime component.
fn is_type_package(package: &str) -> bool {
    package.starts_with("@types/")
}

/// Evidence for a dependency: the manifest, the line, and the key with its
/// value deliberately unread. See the module documentation.
fn dependency_evidence(manifest: &str, raw: &str, package: &str) -> Evidence {
    Evidence::elided_value(
        manifest,
        dependency_line(raw, package),
        &format!("\"{package}\""),
    )
}

/// The 1-based line a dependency key sits on.
///
/// Scanned rather than derived from `serde_json`, which discards positions.
/// The dependency blocks are preferred so that a package whose name also
/// appears in `scripts` is cited where it is *declared*; the whole-file
/// fallback exists for the single-line manifests that hand-written fixtures and
/// generated packages both produce.
fn dependency_line(raw: &str, package: &str) -> Option<u32> {
    let needle = format!("\"{package}\"");
    let mut in_dependencies = false;

    for (index, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("\"dependencies\"")
            || trimmed.starts_with("\"devDependencies\"")
            || trimmed.starts_with("\"peerDependencies\"")
            || trimmed.starts_with("\"optionalDependencies\"")
        {
            in_dependencies = true;
            continue;
        }
        if in_dependencies && (trimmed == "}" || trimmed == "},") {
            in_dependencies = false;
            continue;
        }
        if in_dependencies && trimmed.starts_with(&needle) {
            return Some(index as u32 + 1);
        }
    }

    raw.lines()
        .position(|line| line.contains(&needle))
        .map(|index| index as u32 + 1)
}

/// Enrich the ORM's box with the engine its schema declares, when it declares
/// one literally.
fn prisma_engine(out: &mut NodeSignals, project: &Project, label: &str, prefix: &str) {
    let candidates = ["prisma/schema.prisma", "schema.prisma"];
    for candidate in candidates {
        let path = project.dir.join(candidate);
        let Ok(text) = read_capped(&path) else {
            continue;
        };

        let mut in_datasource = false;
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            if trimmed.starts_with("datasource ") {
                in_datasource = true;
                continue;
            }
            if in_datasource && trimmed.starts_with('}') {
                in_datasource = false;
                continue;
            }
            if !in_datasource {
                continue;
            }
            let Some(rest) = trimmed.strip_prefix("provider") else {
                continue;
            };
            let Some(value) = rest.trim_start().strip_prefix('=') else {
                continue;
            };
            let value = value.trim();
            let relative = join_path(prefix, candidate);
            let line_number = Some(index as u32 + 1);

            match string_literal(value)
                .as_deref()
                .and_then(prisma_engine_label)
            {
                Some(engine) => out.signals.push(
                    Signal::medium(
                        ComponentKind::Database,
                        label.to_string(),
                        project.id.as_str(),
                        Evidence::new(&relative, line_number, trimmed),
                    )
                    .with_detail(format!("engine: {engine}")),
                ),
                None => out.warn(
                    project,
                    format!(
                        "the datasource provider in {relative} is not a literal engine name, so \
                         the database was left unnamed"
                    ),
                ),
            }
            return;
        }
        return;
    }
}

/// Prisma's own provider vocabulary, mapped to the names people say out loud.
///
/// An unrecognised provider abstains rather than title-casing whatever was
/// written: a value this crate has never heard of is as likely to be a typo or
/// a preview feature as a database.
fn prisma_engine_label(provider: &str) -> Option<&'static str> {
    match provider {
        "postgresql" | "postgres" => Some("PostgreSQL"),
        "mysql" => Some("MySQL"),
        "sqlite" => Some("SQLite"),
        "sqlserver" => Some("SQL Server"),
        "mongodb" => Some("MongoDB"),
        "cockroachdb" => Some("CockroachDB"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// HTTP services and their routes
// ---------------------------------------------------------------------------

fn http_service(
    out: &mut NodeSignals,
    project: &Project,
    pkg: &PackageJson,
    raw: &str,
    manifest: &str,
    prefix: &str,
) {
    let matched: Vec<(&str, Framework, &str)> = FRAMEWORKS
        .iter()
        .filter(|(package, _, _)| pkg.dependencies.contains_key(*package))
        .copied()
        .collect();
    let Some(&(first, _, _)) = matched.first() else {
        return;
    };

    // The service's name is the name the author gave the package, which is a
    // declared fact in the same manifest. The scan's project name is a fallback
    // for the packages that have none.
    let label = pkg
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(&project.name)
        .to_string();

    out.signals.push(Signal::high(
        ComponentKind::HttpService,
        label.clone(),
        project.id.as_str(),
        dependency_evidence(manifest, raw, first),
    ));

    let files = source_files(project);

    for &(_, framework, display) in &matched {
        match framework {
            Framework::Next | Framework::SvelteKit | Framework::Nuxt => {
                file_system_routes(out, project, framework, display, &label, &files, prefix);
            }
            Framework::Express | Framework::Fastify | Framework::Koa | Framework::Hono => {
                source_routes(out, project, &label, &files, prefix);
            }
            Framework::Nest => out.warn(
                project,
                format!(
                    "{display} routes were not listed: a controller's path is split between a \
                     class decorator and a method decorator, and joining them here would invent \
                     endpoints"
                ),
            ),
        }
    }
}

fn trim_slashes(path: &str) -> &str {
    path.trim_end_matches('/')
}

/// Every source file inside the project, capped, with a project-relative
/// forward-slashed path beside each.
fn source_files(project: &Project) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for entry in source_walker(&project.dir).flatten() {
        if out.len() >= MAX_SCANNED_FILES {
            break;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(&project.dir) else {
            continue;
        };
        out.push((to_slash(relative), path.to_path_buf()));
    }
    out.sort();
    out
}

fn file_system_routes(
    out: &mut NodeSignals,
    project: &Project,
    framework: Framework,
    display: &str,
    label: &str,
    files: &[(String, PathBuf)],
    prefix: &str,
) {
    if let Some((config, key)) = base_path_config(framework, files) {
        out.warn(
            project,
            format!(
                "no {display} route was listed because {config} sets a '{key}', which prefixes \
                 every path in the application"
            ),
        );
        return;
    }

    for (relative, _) in files {
        match route_of(framework, relative) {
            Verdict::Route(text) => out.signals.push(
                Signal::medium(
                    ComponentKind::HttpService,
                    label.to_string(),
                    &project.id,
                    Evidence::new(join_path(prefix, relative), None, relative.clone()),
                )
                .with_detail(text),
            ),
            Verdict::Abstain(why) => out.warn(
                project,
                format!("the route for {relative} was not listed because {why}"),
            ),
            Verdict::NotARoute => {}
        }
    }
}

/// Whether the framework's own configuration sets an application-wide path
/// prefix.
///
/// A line scan, not a parse: these files are JavaScript or TypeScript modules
/// and this crate has no engine for either. That asymmetry is fine because the
/// answer is used only to *abstain*. A false positive costs a route list and
/// produces a warning naming the file; a false negative would produce a list of
/// paths that are all silently wrong.
fn base_path_config(
    framework: Framework,
    files: &[(String, PathBuf)],
) -> Option<(String, &'static str)> {
    let (stem, key) = match framework {
        Framework::Next => ("next.config", "basePath"),
        Framework::Nuxt => ("nuxt.config", "baseURL"),
        Framework::SvelteKit => ("svelte.config", "base"),
        _ => return None,
    };

    for (relative, path) in files {
        let Some(name) = relative.rsplit('/').next() else {
            continue;
        };
        if !name.starts_with(stem) || relative.contains('/') {
            continue;
        }
        let Ok(text) = read_capped(path) else {
            continue;
        };
        let mut in_block = false;
        for line in text.lines() {
            let code = strip_comments(line, &mut in_block);
            if contains_key(&code, key) {
                return Some((relative.clone(), key));
            }
        }
    }
    None
}

/// Whether a line assigns the given object key, with the key standing on its
/// own so that `database:` does not answer for `base:`.
fn contains_key(line: &str, key: &str) -> bool {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(found) = line[from..].find(key) {
        let start = from + found;
        let end = start + key.len();
        from = start + 1;
        if start > 0 && is_ident_byte(bytes[start - 1]) {
            continue;
        }
        let mut cursor = end;
        while cursor < bytes.len() && bytes[cursor] == b' ' {
            cursor += 1;
        }
        if cursor < bytes.len() && bytes[cursor] == b':' {
            return true;
        }
    }
    false
}

/// What a filename turned out to be.
enum Verdict {
    /// The display text for a route: a path, prefixed by a method only where
    /// the filename itself states one.
    Route(String),
    /// Not a routing file at all — a page, a component, a test.
    NotARoute,
    /// A routing file whose URL cannot be rendered faithfully.
    Abstain(&'static str),
}

fn route_of(framework: Framework, relative: &str) -> Verdict {
    match framework {
        Framework::Next => next_route(relative),
        Framework::SvelteKit => sveltekit_route(relative),
        Framework::Nuxt => nuxt_route(relative),
        _ => Verdict::NotARoute,
    }
}

fn next_route(relative: &str) -> Verdict {
    let path = relative.strip_prefix("src/").unwrap_or(relative);

    if let Some(rest) = path.strip_prefix("app/") {
        let parts: Vec<&str> = rest.split('/').collect();
        let (file, directories) = parts.split_last().expect("split never yields nothing");
        if !matches!(file_stem(file).as_str(), "route") || !is_source_file(file) {
            return Verdict::NotARoute;
        }
        return app_router_url(directories);
    }

    if let Some(rest) = path.strip_prefix("pages/api/") {
        let parts: Vec<&str> = rest.split('/').collect();
        let (file, directories) = parts.split_last().expect("split never yields nothing");
        if !is_source_file(file) {
            return Verdict::NotARoute;
        }
        let mut segments = vec!["api".to_string()];
        for directory in directories {
            if directory.starts_with('_') {
                return Verdict::NotARoute;
            }
            segments.push((*directory).to_string());
        }
        let stem = file_stem(file);
        if stem.starts_with('_') {
            return Verdict::NotARoute;
        }
        if stem != "index" {
            segments.push(stem);
        }
        return Verdict::Route(url_of(&segments));
    }

    Verdict::NotARoute
}

/// The App Router's directory-to-URL rules.
fn app_router_url(directories: &[&str]) -> Verdict {
    let mut segments = Vec::new();
    for directory in directories {
        // Next's own opt-out: nothing under a `_folder` is routable.
        if directory.starts_with('_') {
            return Verdict::NotARoute;
        }
        // A parallel-route slot's URL is decided by the layout that composes
        // it, which is not readable from this path.
        if directory.starts_with('@') {
            return Verdict::Abstain(
                "it sits in a parallel-route slot, whose URL is decided by the layout that \
                 composes it",
            );
        }
        if directory.starts_with('(') {
            if directory.ends_with(')') && directory.len() > 2 {
                // A route group: organisation only, absent from the URL.
                continue;
            }
            return Verdict::Abstain(
                "it uses an interception marker, whose URL depends on the route it intercepts",
            );
        }
        segments.push((*directory).to_string());
    }
    Verdict::Route(url_of(&segments))
}

fn sveltekit_route(relative: &str) -> Verdict {
    let Some(rest) = relative.strip_prefix("src/routes/") else {
        return Verdict::NotARoute;
    };
    let parts: Vec<&str> = rest.split('/').collect();
    let (file, directories) = parts.split_last().expect("split never yields nothing");
    if file_stem(file) != "+server" || !is_source_file(file) {
        return Verdict::NotARoute;
    }

    let mut segments = Vec::new();
    for directory in directories {
        if directory.starts_with('_') {
            return Verdict::NotARoute;
        }
        if directory.starts_with('(') && directory.ends_with(')') && directory.len() > 2 {
            continue;
        }
        segments.push((*directory).to_string());
    }
    Verdict::Route(url_of(&segments))
}

fn nuxt_route(relative: &str) -> Verdict {
    let Some(rest) = relative.strip_prefix("server/api/") else {
        return Verdict::NotARoute;
    };
    let parts: Vec<&str> = rest.split('/').collect();
    let (file, directories) = parts.split_last().expect("split never yields nothing");
    if !is_source_file(file) {
        return Verdict::NotARoute;
    }

    let mut segments = vec!["api".to_string()];
    for directory in directories {
        if directory.starts_with('_') {
            return Verdict::NotARoute;
        }
        segments.push((*directory).to_string());
    }

    // Nuxt lets the filename state the method: `[id].get.ts`.
    let stem = file_stem(file);
    let (stem, method) = match stem.rsplit_once('.') {
        Some((head, suffix)) if METHODS.contains(&suffix) && suffix != "all" => {
            (head.to_string(), Some(suffix.to_ascii_uppercase()))
        }
        _ => (stem, None),
    };
    if stem.starts_with('_') {
        return Verdict::NotARoute;
    }
    if stem != "index" {
        segments.push(stem);
    }

    let url = url_of(&segments);
    Verdict::Route(match method {
        Some(method) => format!("{method} {url}"),
        None => url,
    })
}

fn url_of(segments: &[String]) -> String {
    if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", segments.join("/"))
    }
}

/// The filename with its final extension removed. Nuxt's method suffix is
/// handled by its own rule, so this only ever strips one extension.
fn file_stem(file: &str) -> String {
    match file.rsplit_once('.') {
        Some((stem, _)) => stem.to_string(),
        None => file.to_string(),
    }
}

fn is_source_file(file: &str) -> bool {
    file.rsplit_once('.')
        .is_some_and(|(_, extension)| SOURCE_EXTENSIONS.contains(&extension))
}

// ---------------------------------------------------------------------------
// Route registrations in source
// ---------------------------------------------------------------------------

fn source_routes(
    out: &mut NodeSignals,
    project: &Project,
    label: &str,
    files: &[(String, PathBuf)],
    prefix: &str,
) {
    let sources: Vec<&(String, PathBuf)> = files
        .iter()
        .filter(|(relative, _)| is_source_file(relative))
        .collect();

    // The mount sweep runs over the whole project before a single route is
    // emitted. A prefix declared in `index.js` governs routes registered in
    // `users.js`, so a per-file answer would be wrong exactly where it matters.
    for (relative, path) in &sources {
        let Ok(text) = read_capped(path) else {
            continue;
        };
        let mut in_block = false;
        for (index, line) in text.lines().enumerate() {
            let code = strip_comments(line, &mut in_block);
            let message = match mount_prefix(&code) {
                Some(Mount::Literal(mount)) => format!(
                    "no route was listed for this service because {relative} mounts routes \
                     under '{mount}', and which registrations sit under that prefix is not \
                     readable from one line"
                ),
                Some(Mount::Unreadable(expression)) => format!(
                    "no route was listed for this service because {relative}:{} mounts routes \
                     under the prefix '{expression}', whose value is not a literal string; the \
                     paths as written are missing a prefix this scanner cannot read",
                    index + 1
                ),
                None => continue,
            };
            out.warn(project, message);
            return;
        }
    }

    for (relative, path) in &sources {
        let Ok(text) = read_capped(path) else {
            continue;
        };
        let mut in_block = false;
        for (index, line) in text.lines().enumerate() {
            let code = strip_comments(line, &mut in_block);
            for found in registrations(&code) {
                match found {
                    Registration::Route { method, path: url } => out.signals.push(
                        Signal::medium(
                            ComponentKind::HttpService,
                            label.to_string(),
                            &project.id,
                            Evidence::new(
                                join_path(prefix, relative),
                                Some(index as u32 + 1),
                                code.trim(),
                            ),
                        )
                        .with_detail(format!("{method} {url}")),
                    ),
                    Registration::Unreadable => out.warn(
                        project,
                        format!(
                            "a route registered at {relative}:{} was not listed because its path \
                             is not a literal string",
                            index + 1
                        ),
                    ),
                }
            }
        }
    }
}

/// What a line says about mounting routes under a prefix.
///
/// Three states, not two: a mount whose text cannot be read is still a mount,
/// and treating it as "no mount" is the one outcome that produces a confident
/// wrong answer — every route in the project listed without the prefix that
/// actually reaches it, and no warning to say so.
enum Mount {
    /// A prefix the line states outright.
    Literal(String),
    /// A prefix the line evidently applies, carrying the expression as
    /// written, when the value is a template literal or a variable.
    Unreadable(String),
}

/// The prefix a line mounts routes under, if it mounts any.
fn mount_prefix(line: &str) -> Option<Mount> {
    let mut from = 0;
    while let Some(offset) = line[from..].find(".use(") {
        let dot = from + offset;
        let open = dot + ".use(".len();
        from = open;

        // Two arguments or it is not a mount: `app.use(express.json())` and
        // `app.use(cors())` are middleware, take no path, and move no route.
        let Some((argument, followed)) = argument_list_head(line, open) else {
            continue;
        };
        if !followed {
            continue;
        }
        let argument = argument.trim();

        match argument_at(argument) {
            Argument::Literal(path) => {
                if path.starts_with('/') && path != "/" {
                    return Some(Mount::Literal(path));
                }
            }
            Argument::Unreadable => {
                // A literal beginning with `/` is path-shaped on its own, so
                // the literal arm above needs nothing from the receiver. An
                // identifier is not path-shaped at all, so here the receiver
                // has to carry that evidence instead — otherwise any
                // two-argument `.use` in the project silences the route list.
                if routes_from(receiver_before(line, dot)) && is_path_expression(argument) {
                    return Some(Mount::Unreadable(argument.to_string()));
                }
            }
        }
    }

    // Fastify's plugin prefix is an option rather than an argument.
    if line.contains(".register(") || line.contains(".route(") {
        if let Some(at) = line.find("prefix") {
            let rest = line[at + "prefix".len()..].trim_start();
            if let Some(value) = rest.strip_prefix(':') {
                let value = value.trim_start();
                match argument_at(value) {
                    Argument::Literal(path) => {
                        if path.starts_with('/') && path != "/" {
                            return Some(Mount::Literal(path));
                        }
                    }
                    Argument::Unreadable => {
                        let expression = value.split([',', '}']).next().unwrap_or_default().trim();
                        if is_path_expression(expression) {
                            return Some(Mount::Unreadable(expression.to_string()));
                        }
                    }
                }
            }
        }
    }
    None
}

/// The identifier a call is made on, reading backwards from its `.`.
fn receiver_before(line: &str, dot: usize) -> &str {
    let bytes = line.as_bytes();
    let mut start = dot;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    &line[start..dot]
}

/// The text of the first argument of a call whose `(` has already been
/// consumed, and whether a second argument follows it.
///
/// Unlike [`first_argument`] this does not try to read the value: it only
/// finds where the argument ends, so that an argument no reader could evaluate
/// is still returned rather than discarded.
fn argument_list_head(line: &str, from: usize) -> Option<(&str, bool)> {
    let rest = line.get(from..)?;
    let bytes = rest.as_bytes();
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(open) => {
                if byte == b'\\' {
                    index += 2;
                    continue;
                }
                if byte == open {
                    quote = None;
                }
            }
            None => match byte {
                b'"' | b'\'' | b'`' => quote = Some(byte),
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' if depth == 0 => return Some((&rest[..index], false)),
                b')' | b']' | b'}' => depth -= 1,
                b',' if depth == 0 => return Some((&rest[..index], true)),
                _ => {}
            },
        }
        index += 1;
    }
    // An unterminated call: the argument runs to the end of the line, and
    // nothing states that a second one follows.
    Some((rest, false))
}

/// Whether an expression could name a path without stating one — a template
/// literal, an identifier, or a member chain of identifiers.
///
/// Deliberately narrow: a call, an arrow function, an object and an array are
/// all things a `.use` takes that are not paths, and each of them would turn
/// an ordinary middleware registration into a silenced route list.
fn is_path_expression(text: &str) -> bool {
    if text.starts_with('`') {
        return true;
    }
    if text.is_empty() {
        return false;
    }
    text.split('.').all(|part| {
        let mut chars = part.chars();
        match chars.next() {
            Some(first) if first.is_ascii_alphabetic() || first == '_' || first == '$' => {
                chars.all(|c| c.is_ascii() && is_ident_byte(c as u8))
            }
            _ => false,
        }
    })
}

enum Registration {
    Route {
        method: String,
        path: String,
    },
    /// A registration on a routing receiver whose path is a template literal or
    /// a variable.
    Unreadable,
}

/// Every route registration a single line of code carries.
fn registrations(line: &str) -> Vec<Registration> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();

    let mut index = 0;
    while index < bytes.len() {
        let here = index;
        index += 1;
        if bytes[here] != b'.' {
            continue;
        }
        let start = here + 1;
        let mut end = start;
        while end < bytes.len() && is_ident_byte(bytes[end]) {
            end += 1;
        }
        if end == start || end >= bytes.len() || bytes[end] != b'(' {
            continue;
        }
        let method = &line[start..end];
        if !METHODS.contains(&method) {
            continue;
        }

        let mut receiver_start = here;
        while receiver_start > 0 && is_ident_byte(bytes[receiver_start - 1]) {
            receiver_start -= 1;
        }
        if !routes_from(&line[receiver_start..here]) {
            continue;
        }

        match first_argument(line, end + 1) {
            Some(Argument::Literal(path)) if path.starts_with('/') => {
                out.push(Registration::Route {
                    method: method.to_ascii_uppercase(),
                    path,
                })
            }
            // A literal that is not a path is not a registration at all.
            Some(Argument::Literal(_)) => {}
            Some(Argument::Unreadable) => out.push(Registration::Unreadable),
            None => {}
        }
    }
    out
}

/// Whether an identifier is plausibly something routes are registered on.
///
/// See the module documentation: without this, every `Map::get` in the
/// repository becomes a candidate and every one of them becomes a warning.
fn routes_from(receiver: &str) -> bool {
    let lower = receiver.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "app" | "router" | "server" | "fastify" | "api" | "hono" | "koa" | "route" | "routes"
    ) || lower.ends_with("router")
        || lower.ends_with("app")
}

enum Argument {
    Literal(String),
    Unreadable,
}

/// The first argument of a call whose `(` has already been consumed.
///
/// `None` when there is no second argument: a registration always takes a
/// handler, and requiring one is what keeps `map.get("/x")` out.
fn first_argument(line: &str, from: usize) -> Option<Argument> {
    let rest = line.get(from..)?.trim_start();
    match argument_at(rest) {
        Argument::Literal(value) => {
            let after = rest.trim_start_matches(|c| c != ',');
            after.starts_with(',').then_some(Argument::Literal(value))
        }
        Argument::Unreadable => Some(Argument::Unreadable),
    }
}

/// One argument, read from the start of `text`.
fn argument_at(text: &str) -> Argument {
    let bytes = text.as_bytes();
    let Some(&quote) = bytes.first() else {
        return Argument::Unreadable;
    };
    if quote != b'"' && quote != b'\'' {
        // A backtick is a template literal, an identifier is a variable, and
        // neither states a path.
        return Argument::Unreadable;
    }

    let mut value = String::new();
    let mut index = 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            byte if byte == quote => return Argument::Literal(value),
            _ => {
                let ch = text[index..].chars().next().expect("a char boundary");
                value.push(ch);
                index += ch.len_utf8();
            }
        }
    }
    Argument::Unreadable
}

// ---------------------------------------------------------------------------
// Reading text
// ---------------------------------------------------------------------------

/// Remove comments from one line, carrying block-comment state across lines.
///
/// String-aware, because `fetch("https://example.com")` contains `//` and a
/// naive scanner would truncate the line there. Quoted regions are copied
/// through untouched so that a path containing `/*` survives too.
fn strip_comments(line: &str, in_block: &mut bool) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut index = 0;
    let mut quote: Option<u8> = None;

    while index < bytes.len() {
        if *in_block {
            if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                *in_block = false;
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }

        match quote {
            Some(open) => {
                let ch = line[index..].chars().next().expect("a char boundary");
                out.push(ch);
                index += ch.len_utf8();
                if ch == '\\' {
                    // An escaped quote does not close the string, and an
                    // escaped backslash does not escape what follows it.
                    if let Some(next) = line[index..].chars().next() {
                        out.push(next);
                        index += next.len_utf8();
                    }
                    continue;
                }
                if ch == open as char {
                    quote = None;
                }
            }
            None => {
                if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
                    break;
                }
                if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    *in_block = true;
                    index += 2;
                    continue;
                }
                if matches!(bytes[index], b'"' | b'\'' | b'`') {
                    quote = Some(bytes[index]);
                }
                let ch = line[index..].chars().next().expect("a char boundary");
                out.push(ch);
                index += ch.len_utf8();
            }
        }
    }
    out
}

/// Read at most [`MAX_FILE_BYTES`], lossily.
///
/// Lossy because a source tree contains files in every encoding anyone ever
/// saved, and a route scan that fails on one byte would report nothing for the
/// whole project.
fn read_capped(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    let end = bytes.len().min(MAX_FILE_BYTES);
    // Truncating mid-character is fine: `from_utf8_lossy` replaces the partial
    // sequence, and the cap only ever lands inside generated bulk.
    Ok(String::from_utf8_lossy(&bytes[..end]).into_owned())
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

/// The literal inside a quoted value, or `None` when the value is anything
/// else — `env("X")`, an identifier, an expression.
fn string_literal(value: &str) -> Option<String> {
    match argument_at(value) {
        Argument::Literal(literal) => Some(literal),
        Argument::Unreadable => None,
    }
}

fn join_path(prefix: &str, relative: &str) -> String {
    let prefix = trim_slashes(prefix);
    if prefix.is_empty() {
        relative.to_string()
    } else {
        format!("{prefix}/{relative}")
    }
}

/// A path rendered relative to the workspace root, forward-slashed.
///
/// Forward slashes on every platform for the same reason
/// [`super::super::graph`] does it: these strings are read by a person and
/// stored in files that move between machines.
fn relative_to(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) => to_slash(relative),
        Err(_) => to_slash(path),
    }
}

fn to_slash(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}
