//! Component signals read out of .NET manifests, configuration and source.
//!
//! This is the producer for the .NET half of [`super`]. It emits
//! [`Signal`](super::framework::Signal) candidates and has no way to decide
//! what becomes a box or an arrow — [`admit`](super::framework::admit) does
//! that, and everything below is written on the assumption that it will refuse
//! anything this file gets wrong.
//!
//! # What each rule is allowed to claim
//!
//! * A `<PackageReference>` on a known data client is **HIGH**: the author
//!   wrote down that this project speaks a protocol. See [`DATA_CLIENTS`] for
//!   the exact table and for the limit of what a package reference proves.
//! * `Sdk="Microsoft.NET.Sdk.Web"` and `<IsAspireHost>` are **HIGH**: an SDK
//!   attribute is the author declaring what kind of program this is.
//! * An `applicationUrl` in `launchSettings.json`, a `connectionStrings` key in
//!   `appsettings*.json` and an Aspire `AddProject<Projects.X>()` are
//!   **MEDIUM**. Each of them can label something that already exists; none of
//!   them may bring anything into existence.
//! * An `AddHttpClient` registration whose literal base address matches exactly
//!   one other project's `applicationUrl` is a **HIGH call** — a
//!   [`Signal::call`], not a component signal. It claims a service → service
//!   edge, and it is HIGH because its evidence cites the *callee's*
//!   `launchSettings.json` (a declaration file) rather than the caller's `.cs`.
//!   See [`http_clients`] for why that is not the workaround it might look like.
//!
//! # Why nothing read out of a `.cs` file can ever be HIGH
//!
//! It is not a policy this file applies — it is a consequence of the gate.
//! [`admit`](super::framework::admit) refuses a HIGH signal whose evidence
//! cites a file that is not a manifest or a configuration file
//! (`DiscardReason::UnverifiableEvidence`), and `.cs` is not on that list. So
//! the Aspire rule below, whose evidence is always a line of C#, is
//! structurally incapable of drawing an arrow no matter how confident the
//! match is. It enriches a component that a manifest already earned, or it
//! produces a warning.
//!
//! The `AddHttpClient` call rule reaches a HIGH signal without breaking this,
//! and the distinction is exact: it does *not* cite the source line it read the
//! address from. It reads the address out of the `.cs`, uses it only to look up
//! which project's `launchSettings.json` declares that binding, and then cites
//! **that** file as its evidence. The claim therefore rests on a declaration
//! the author wrote, not on source — which is the opposite of the workaround
//! (citing a manifest while having actually read the source) the gate exists to
//! refuse. `an_httpclient_never_produces_a_high_signal_from_a_source_file` pins
//! that no signal citing a `.cs` file is ever HIGH.
//!
//! # Why the symbol index is not consulted
//!
//! [`crate::symbols`] already holds a file and a line for every *declaration*
//! in the workspace, and the standing instruction is to consume it rather than
//! re-scan. It cannot help here. `builder.Services.AddHttpClient(...)` and
//! `builder.AddProject<Projects.Orders_Api>("orders")` are calls, and
//! [`crate::symbols::declarations::declaration`] only returns a name for a line
//! whose first word is one of `DECLARING` (`class`, `public`, `static`, `fn`,
//! …). A statement beginning `builder.` matches none of them, so these lines
//! are absent from the index by construction — asking it would return nothing,
//! not less detail.
//!
//! # Cost
//!
//! Every .NET project's own directory is walked once for `.cs` files, through
//! [`crate::workspace::source_walker`], so `SKIP_DIRS` applies and `obj/` — the
//! one place a generated `Projects.*.g.cs` would live — is never read. Reads
//! are capped at [`MAX_SOURCE_FILES`] files and [`MAX_SOURCE_BYTES`] each, and
//! hitting either cap produces a warning rather than a quietly shorter answer.
//! A project nested inside another project's directory is walked twice; that is
//! left alone because the alternative is a shared walk whose results have to be
//! partitioned back out by project, and the caps already bound the work.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

use super::framework::{self, ComponentKind, Evidence, Signal};
use crate::adapters::dotnet::{self, ProjectFile};
use crate::model::Project;
use crate::workspace::{source_walker, Workspace};

/// Everything this producer found, and everything it declined to use.
///
/// Two lists rather than one, because they are refused by different things.
/// `signals` are candidates that still have to survive
/// [`admit`](super::framework::admit); `warnings` are candidates this file
/// refused *before* emitting anything, which the gate will therefore never see
/// and never count. Without the second list those refusals would be invisible,
/// which is the one outcome [`super`] rules out.
///
/// # Warnings never repeat a value
///
/// Producer warnings bypass the gate's screening entirely — nothing inspects
/// them for a leaked credential — so this file keeps a stricter rule for them
/// than the gate keeps for signals: a warning may name a file, a project, a
/// package or a configuration *key*, and may never contain text read out of a
/// file's *values*. A base address that resolved to nothing is reported as
/// "no launch profile matched", never as the address itself.
/// `a_producer_warning_never_repeats_a_value_it_read` pins it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DotnetSignals {
    pub signals: Vec<Signal>,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// The package table
// ---------------------------------------------------------------------------

/// Package names that prove a project speaks a particular protocol, and the
/// label to draw when they do.
///
/// # The label is the provider and nothing else
///
/// `Npgsql` earns a box labelled `PostgreSQL`. It never earns a host, a port,
/// an instance or a database name, because the package reference states none of
/// those and nothing else in the project file does either. What the author
/// wrote down is *"this project can speak PostgreSQL"*, and that is the entire
/// claim this rule is entitled to make. Which server it connects to lives in
/// configuration — usually in a connection string, whose value this module is
/// forbidden to read at all — or in an environment variable that only exists on
/// a deployed machine. `a_package_reference_never_names_the_database_instance`
/// pins it.
///
/// # Matching is on name boundaries, not on raw prefixes
///
/// `Npgsql.EntityFrameworkCore.PostgreSQL` is the same client as `Npgsql` and
/// matches; `NpgsqlRest` is a different package by a different author and does
/// not. NuGet ids are dotted, so a prefix only means anything when it ends at a
/// dot, and a raw `starts_with` would happily hand `MongoDB.Analyzer` — a
/// Roslyn analyzer that never opens a socket — a database box.
const DATA_CLIENTS: &[(&str, ComponentKind, &str)] = &[
    (
        "Microsoft.EntityFrameworkCore.SqlServer",
        ComponentKind::Database,
        "SQL Server",
    ),
    (
        "Microsoft.Data.SqlClient",
        ComponentKind::Database,
        "SQL Server",
    ),
    (
        "Npgsql.EntityFrameworkCore.PostgreSQL",
        ComponentKind::Database,
        "PostgreSQL",
    ),
    ("Npgsql", ComponentKind::Database, "PostgreSQL"),
    (
        "Pomelo.EntityFrameworkCore.MySql",
        ComponentKind::Database,
        "MySQL",
    ),
    ("MySql.Data", ComponentKind::Database, "MySQL"),
    (
        "Microsoft.EntityFrameworkCore.Sqlite",
        ComponentKind::Database,
        "SQLite",
    ),
    ("MongoDB.Driver", ComponentKind::Database, "MongoDB"),
    ("StackExchange.Redis", ComponentKind::Cache, "Redis"),
    ("RabbitMQ.Client", ComponentKind::MessageQueue, "RabbitMQ"),
    (
        "Azure.Messaging.ServiceBus",
        ComponentKind::MessageQueue,
        "Azure Service Bus",
    ),
    ("Confluent.Kafka", ComponentKind::MessageQueue, "Kafka"),
];

/// Packages that look like data clients, are not components, and are reported
/// as refused rather than passed over.
///
/// # `Microsoft.EntityFrameworkCore.InMemory` is not a database
///
/// It is a provider in the same list as the SQL Server one and it is tempting
/// to treat it as one more row in [`DATA_CLIENTS`], which is exactly why it is
/// called out here. The in-memory provider is a dictionary inside the process.
/// There is no server, no protocol, no connection and nothing deployed
/// alongside the application. A box labelled `In-Memory` next to the boxes for
/// PostgreSQL and Redis would assert a component of the system that does not
/// exist at runtime and cannot be pointed at by anyone reading the diagram.
///
/// # `Testcontainers.*` is a harder call, and goes the same way
///
/// Unlike the in-memory provider, `Testcontainers.PostgreSql` really does start
/// a real PostgreSQL, and it even names the technology in the package id — so
/// the label would not be a guess. It is still refused, because the *lifetime*
/// is the part the diagram cannot express: that container exists for the
/// duration of a test run and is gone afterwards, and an arrow into a
/// PostgreSQL box says nothing about when. In practice nothing is lost, because
/// a project using Testcontainers references a real client (`Npgsql`, here) to
/// talk to the container it started, and that reference earns the box through
/// [`DATA_CLIENTS`] on its own terms.
///
/// # What is deliberately *not* filtered: test projects
///
/// [`Project::is_test_project`] is available and is not consulted anywhere in
/// this file. An integration test project referencing `Npgsql` gets the same
/// PostgreSQL box as a service that references it, because it is the same
/// declaration and it is equally true: that project speaks PostgreSQL. Deciding
/// that a reader wants to see production dependencies and not test ones is a
/// judgement about the *question being asked*, which this module has no way to
/// know and no business assuming — and the reader can already tell a test
/// project from a service, because the graph labels it as one.
/// `a_data_client_in_a_test_project_is_still_a_declared_fact` pins it.
const NOT_A_COMPONENT: &[(&str, &str)] = &[
    (
        "Microsoft.EntityFrameworkCore.InMemory",
        "the in-memory provider is a store inside the process, not a database the system connects to",
    ),
    (
        "Testcontainers",
        "a Testcontainers package starts a container for the duration of a test run, not a component of the running system",
    ),
];

// ---------------------------------------------------------------------------
// Caps
// ---------------------------------------------------------------------------

/// How many `.cs` files under one project may be read.
const MAX_SOURCE_FILES: usize = 400;

/// How large a single `.cs` file may be before it is skipped.
///
/// A file this size is generated or vendored, and reading it would cost more
/// than every hand-written file in the project put together.
const MAX_SOURCE_BYTES: u64 = 512 * 1024;

/// How many lines after an `AddHttpClient` call a `BaseAddress` assignment may
/// appear on and still be attributed to it.
///
/// Registrations are conventionally written as a lambda immediately after the
/// call, so the window is small on purpose: a `BaseAddress` five lines further
/// down is as likely to belong to the *next* registration, and the scan stops
/// early at the next `AddHttpClient` for exactly that reason.
const BASE_ADDRESS_WINDOW: usize = 5;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Read every .NET project in `workspace` and emit its component signals.
///
/// The manifests are re-read from disk here rather than taken from the scan,
/// for the reason [`super::super::graph`] gives: the scan happened when the
/// directory was opened and the files have been edited since.
///
/// The `project_id` on every signal is [`Project::id`], which
/// [`crate::workspace`] derives from a relative path and which is **not
/// injective** — two projects whose paths differ only in `/` versus `-` share
/// one. This producer does not resolve that; `graph::NodeIds` already detects
/// the collision and renames the boxes, and duplicating the fix here would give
/// two places to keep in agreement.
pub fn signals(workspace: &Workspace) -> DotnetSignals {
    let mut out = DotnetSignals::default();
    let mut reads: Vec<Read<'_>> = Vec::new();

    for project in workspace
        .projects
        .iter()
        .filter(|p| p.ecosystem == "dotnet")
    {
        let relative = relative(&workspace.root, &project.manifest_path);
        let Ok(text) = std::fs::read_to_string(&project.manifest_path) else {
            out.warnings.push(format!(
                "{}: {} could not be read, so no components were derived from it",
                project.name,
                display(&relative)
            ));
            continue;
        };
        let parsed = dotnet::parse_project_file(&text);
        reads.push(Read {
            project,
            parsed,
            text,
            manifest: relative,
        });
    }

    for read in &reads {
        packages(read, &mut out);
        http_service(read, &mut out);
    }

    let services = Bindings::of(workspace, &reads, &mut out);

    for read in &reads {
        connection_strings(workspace, read, &mut out);
        source_scan(workspace, read, &reads, &services, &mut out);
    }

    out.signals.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    out.warnings.sort();
    out.warnings.dedup();
    out
}

/// One project's manifest, read once and shared by every rule below.
struct Read<'a> {
    project: &'a Project,
    parsed: ProjectFile,
    text: String,
    /// Workspace-relative, forward-slashed — the form that ends up in evidence.
    manifest: PathBuf,
}

/// A total order over signals, so the output does not depend on directory
/// iteration order. Every field is included: two signals that differ only in
/// their excerpt are two different claims and must not swap places between
/// runs.
fn sort_key(signal: &Signal) -> (u8, &'static str, &str, &str, String, u32, &str) {
    (
        signal.strength as u8,
        signal.kind.slug(),
        signal.label.as_str(),
        signal.project_id.as_str(),
        display(&signal.evidence.path),
        signal.evidence.line.unwrap_or(0),
        signal.evidence.excerpt.as_str(),
    )
}

// ---------------------------------------------------------------------------
// HIGH: data clients
// ---------------------------------------------------------------------------

fn packages(read: &Read<'_>, out: &mut DotnetSignals) {
    for package in &read.parsed.package_references {
        if let Some(reason) = refused_package(package) {
            out.warnings.push(format!(
                "{}: package '{}' in {} was not drawn as a component because {reason}",
                read.project.name,
                package,
                display(&read.manifest)
            ));
            continue;
        }
        let Some((kind, label)) = data_client(package) else {
            // An ordinary third-party package. Not a refusal and not reported:
            // `Serilog` is a fact about the build, not about the architecture,
            // and a warning per package would bury every warning that matters.
            continue;
        };

        // The quoted forms first: a project referencing both `Npgsql` and
        // `Npgsql.EntityFrameworkCore.PostgreSQL` contains the shorter name as
        // a substring of the longer one's line, and citing the wrong line is a
        // receipt that does not check out.
        let (line, excerpt) = declaration_line(
            &read.text,
            &[&format!("\"{package}\""), &format!("'{package}'"), package],
        );
        out.signals.push(Signal::high(
            kind,
            label,
            &read.project.id,
            Evidence::new(&read.manifest, line, excerpt),
        ));
    }
}

/// The [`DATA_CLIENTS`] row a package id matches, longest name first.
///
/// Longest first so that `Npgsql.EntityFrameworkCore.PostgreSQL` is attributed
/// to its own row rather than to the shorter `Npgsql` one. Both currently carry
/// the same label, so nothing observable depends on it today — which is exactly
/// when to get it right, because the first row that disagrees would otherwise
/// resolve differently depending on where it was inserted in the table.
fn data_client(package: &str) -> Option<(ComponentKind, &'static str)> {
    DATA_CLIENTS
        .iter()
        .filter(|(name, _, _)| matches_package(package, name))
        .max_by_key(|(name, _, _)| name.len())
        .map(|(_, kind, label)| (*kind, *label))
}

fn refused_package(package: &str) -> Option<&'static str> {
    NOT_A_COMPONENT
        .iter()
        .find(|(name, _)| matches_package(package, name))
        .map(|(_, reason)| *reason)
}

/// Whether a package id is `name`, or a package in `name`'s dotted family.
fn matches_package(package: &str, name: &str) -> bool {
    if package.eq_ignore_ascii_case(name) {
        return true;
    }
    package.len() > name.len()
        && package[..name.len()].eq_ignore_ascii_case(name)
        && package.as_bytes()[name.len()] == b'.'
}

// ---------------------------------------------------------------------------
// HIGH: HTTP services
// ---------------------------------------------------------------------------

/// Emit the service a project's SDK declares it to be.
///
/// Both triggers are attributes of the `<Project>` element or a property beside
/// it, which is as declared as a fact gets: `Microsoft.NET.Sdk.Web` brings in
/// the ASP.NET Core targets, and `<IsAspireHost>` is what the Aspire tooling
/// itself keys on.
///
/// The Aspire app host is included even though it is an orchestrator rather
/// than an API, because it does serve the dashboard over HTTP and — more to the
/// point — the alternative is a host that orchestrates a diagram's worth of
/// services while being absent from it.
fn http_service(read: &Read<'_>, out: &mut DotnetSignals) {
    let sdks = std::iter::once(read.parsed.sdk.as_deref().unwrap_or_default())
        .chain(read.parsed.sdk_imports.iter().map(String::as_str));

    let mut trigger = None;
    for sdk in sdks {
        for name in sdk.split(';') {
            // `Sdk="Aspire.AppHost.Sdk/13.4.6"` pins a version after a slash.
            let name = name.split('/').next().unwrap_or(name).trim();
            if name.eq_ignore_ascii_case("Microsoft.NET.Sdk.Web")
                || name.eq_ignore_ascii_case("Aspire.AppHost.Sdk")
            {
                trigger = Some(name.to_string());
            }
        }
    }
    if trigger.is_none() && read.parsed.is_aspire_host == Some(true) {
        trigger = Some("IsAspireHost".to_string());
    }
    let Some(trigger) = trigger else {
        return;
    };

    let (line, excerpt) = declaration_line(&read.text, &[&trigger]);
    out.signals.push(Signal::high(
        ComponentKind::HttpService,
        &read.project.name,
        &read.project.id,
        Evidence::new(&read.manifest, line, excerpt),
    ));
}

// ---------------------------------------------------------------------------
// MEDIUM: launch profile urls, and the address book they form
// ---------------------------------------------------------------------------

/// Which project answers on which `host:port`, according to `launchSettings.json`.
///
/// This is the only thing in the file that can turn one project's source into a
/// claim about another, so it is built from a declaration file and nothing else.
/// An `applicationUrl` is a checked-in binding the author wrote; it is not
/// where the service runs in production, and this module never says it is.
///
/// # The url itself never leaves this function
///
/// It is read, split into `(host, port)` for the address book below, and then
/// dropped. Neither the signal's detail nor its evidence excerpt carries it:
/// the detail names the *profile* (`launch profile 'https'`) and the evidence
/// is [`Evidence::elided_value`](super::framework::Evidence::elided_value), the
/// same form the connection-string reader uses.
///
/// This is not hypothetical tidiness. An `applicationUrl` is a url, and a url
/// takes `user:password@host`; people do check those in. The signal is MEDIUM,
/// a MEDIUM detail whose project did not earn the component is printed verbatim
/// by [`super::super::components`], and the result was a credential in
/// `ArchGraph::warnings` and in the exported mermaid. The gate now refuses a
/// value-shaped detail as well, so this is the producer half of the same fix:
/// the gate should not have to be the only thing standing between a
/// `launchSettings.json` and a committed diagram.
struct Bindings {
    /// `(host, port)` → the projects whose launch profiles claim it. A `Vec`
    /// rather than a single binding because two projects may well declare the
    /// same port in different profiles, and that ambiguity has to be visible to
    /// the matcher rather than silently resolved by whichever was scanned first.
    by_authority: BTreeMap<(String, u16), Vec<Binding>>,
}

/// One project's claim on a `(host, port)`, with the receipt for it.
///
/// The receipt is what lets an `AddHttpClient` match be drawn as a real arrow:
/// the callee's identity rests on its own `launchSettings.json`, a declaration
/// file, rather than on the caller's source. Everything needed to cite that
/// file — the project id it belongs to, the workspace-relative path, and the
/// `applicationUrl` line — is captured here where the file is read, so the
/// matcher never has to touch the disk again or quote the url.
struct Binding {
    project_name: String,
    project_id: String,
    /// Workspace-relative, forward-slashed — the callee's launch settings file.
    launch_settings: PathBuf,
    /// 1-based line of the `applicationUrl` this binding came from.
    line: Option<u32>,
}

impl Bindings {
    fn of(workspace: &Workspace, reads: &[Read<'_>], out: &mut DotnetSignals) -> Self {
        let mut by_authority: BTreeMap<(String, u16), Vec<Binding>> = BTreeMap::new();

        for read in reads {
            let path = read
                .project
                .dir
                .join("Properties")
                .join("launchSettings.json");
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let relative = relative(&workspace.root, &path);

            for profile in dotnet::parse_launch_settings(&text) {
                let Some(urls) = profile.application_url.as_deref() else {
                    continue;
                };
                let line = line_containing(&text, "applicationUrl");
                out.signals.push(
                    Signal::medium(
                        ComponentKind::HttpService,
                        &read.project.name,
                        &read.project.id,
                        Evidence::elided_value(&relative, line, "applicationUrl"),
                    )
                    .with_detail(format!("launch profile '{}'", profile.name)),
                );

                for url in urls.split(';') {
                    if let Some(authority) = authority(url.trim()) {
                        by_authority.entry(authority).or_default().push(Binding {
                            project_name: read.project.name.clone(),
                            project_id: read.project.id.clone(),
                            launch_settings: relative.clone(),
                            line,
                        });
                    }
                }
            }
        }

        for bindings in by_authority.values_mut() {
            bindings.sort_by(|a, b| a.project_name.cmp(&b.project_name));
            bindings.dedup_by(|a, b| a.project_id == b.project_id);
        }
        Self { by_authority }
    }

    /// The binding on an authority when exactly one project claims it.
    ///
    /// Abstains at zero or more than one, the same rule the matcher relies on:
    /// a call resolves to a service only when a single project answers on that
    /// `host:port`, because anything else is a guess about which one was meant.
    fn project_at(&self, authority: &(String, u16)) -> Option<&Binding> {
        match self.by_authority.get(authority) {
            Some(bindings) if bindings.len() == 1 => Some(&bindings[0]),
            _ => None,
        }
    }
}

/// The `(host, port)` an absolute http(s) URL names, or `None`.
///
/// # What abstains, and why
///
/// * **Anything that is not `http`/`https`.** A `tcp://` or an interpolated
///   string is not a launch binding.
/// * **A wildcard host.** `http://+:5080`, `http://*:5080` and
///   `http://0.0.0.0:5080` mean "every interface". A client pointed at
///   `localhost:5080` very probably does reach that server, but "probably"
///   is a guess, and the same wildcard would match a client pointed at any
///   other host equally well. `a_wildcard_application_url_binding_matches_no_client`
///   pins the abstention.
/// * **Different spellings of the same machine.** `localhost` and `127.0.0.1`
///   are not equated. Treating them as one would be right nearly always and
///   silently wrong on a host file that says otherwise, and the cost of being
///   wrong here is a confident arrow between two services.
///
/// A missing port is filled in from the scheme (80/443). That is not an
/// inference about this workspace — it is what the URL means.
fn authority(url: &str) -> Option<(String, u16)> {
    let (scheme, rest) = url.split_once("://")?;
    let default_port = if scheme.eq_ignore_ascii_case("https") {
        443
    } else if scheme.eq_ignore_ascii_case("http") {
        80
    } else {
        return None;
    };

    let rest = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let rest = rest.rsplit_once('@').map_or(rest, |(_, after)| after);

    // An IPv6 literal is bracketed, and the colons inside it are not a port.
    let (host, port) = match rest.strip_prefix('[') {
        Some(inside) => {
            let (host, after) = inside.split_once(']')?;
            (format!("[{host}]"), after.strip_prefix(':'))
        }
        None => match rest.rsplit_once(':') {
            Some((host, port)) => (host.to_string(), Some(port)),
            None => (rest.to_string(), None),
        },
    };

    let port = match port {
        Some(port) => port.parse().ok()?,
        None => default_port,
    };
    let host = host.to_ascii_lowercase();
    if host.is_empty() || matches!(host.as_str(), "+" | "*" | "0.0.0.0" | "[::]") {
        return None;
    }
    Some((host, port))
}

// ---------------------------------------------------------------------------
// MEDIUM: connectionStrings keys
// ---------------------------------------------------------------------------

/// Attach the *names* the author gave their connection strings to the database
/// the project declares a client for.
///
/// # The value never leaves this function — it is never even bound
///
/// Only the keys of the `ConnectionStrings` object are read; the values are
/// left inside the [`serde_json::Value`] and are never formatted, logged or
/// copied into a signal. Evidence is built with
/// [`Evidence::elided_value`](super::framework::Evidence::elided_value), which
/// produces `Orders: <value not read>` — that is the whole receipt a reader
/// gets, and it is enough to find the line.
///
/// # A key may never create a database, and here it cannot even try
///
/// A connection string is an *intent to connect*: it can name a server that is
/// switched off, a database that was never created, or a service that this
/// deployment does not use. So the signal is MEDIUM, and it is only emitted at
/// all when [`placement`] can say which declared store the key belongs to.
///
/// With no declared store at all, no signal is emitted either — deliberately
/// not even a doomed one. Emitting a signal labelled `Orders` so the gate could
/// refuse it for the record would risk something worse than silence: the gate
/// matches MEDIUM to HIGH on `(kind, label)`, and if another producer had
/// created a database component called `Orders`, that doomed signal would
/// attach to it. That is a component created out of a name similarity, which
/// [`super`] forbids outright. The refusal is reported here instead.
///
/// # The candidate set is every declared store, not every declared *database*
///
/// This is a fix rather than a design, and the bug it closes is worth writing
/// down because the code that had it looked correct. The candidate set used to
/// be [`ComponentKind::Database`] rows only, and the count rule was applied to
/// that. A project referencing `Npgsql` **and** `StackExchange.Redis` therefore
/// declared exactly one *database*, so every key in its `appsettings.json` was
/// attached to PostgreSQL — including the one the author called `"Redis"`,
/// producing the pairing *"PostgreSQL — connection string 'Redis'"*. Nothing
/// surfaced it, because [`super::super::components::cross_project_notes`]
/// reports only cross-project details and this one was the project's own; the
/// first view that lists a box's details beside it would have printed a
/// statement no file in the workspace supports.
///
/// Widening the candidate set to caches and queues as well is what makes the
/// count rule mean what it always claimed to mean: *this project declares one
/// place to connect to, so a connection string can only be for that one*.
fn connection_strings(workspace: &Workspace, read: &Read<'_>, out: &mut DotnetSignals) {
    let stores = declared_stores(read);

    for path in appsettings_files(&read.project.dir) {
        let relative = relative(&workspace.root, &path);
        let Ok(text) = std::fs::read_to_string(&path) else {
            out.warnings.push(format!(
                "{}: {} could not be read",
                read.project.name,
                display(&relative)
            ));
            continue;
        };
        // .NET's configuration loader tolerates comments and trailing commas;
        // `serde_json` does not, and this file has no business teaching it to.
        // A file that will not parse is reported rather than pattern-matched,
        // because a regex over half-parsed JSON is how a value ends up being
        // mistaken for a key.
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
            out.warnings.push(format!(
                "{}: {} is not strict JSON (comments and trailing commas are accepted by .NET \
                 but not read here), so its connection string names were not used",
                read.project.name,
                display(&relative)
            ));
            continue;
        };

        let Some(section) = json.as_object().and_then(|root| {
            root.iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("ConnectionStrings"))
                .and_then(|(_, value)| value.as_object())
        }) else {
            continue;
        };

        let from = line_containing(&text, "ConnectionStrings").unwrap_or(1);
        for key in section.keys() {
            let line = line_containing_from(&text, &format!("\"{key}\""), from);
            match placement(&stores, key) {
                Ok((kind, label)) => out.signals.push(
                    Signal::medium(
                        kind,
                        label,
                        &read.project.id,
                        Evidence::elided_value(&relative, line, key),
                    )
                    .with_detail(format!("connection string '{key}'")),
                ),
                Err(why) => out.warnings.push(format!(
                    "{}: connection string {} in {} was not attached to a data store because \
                     {why}",
                    read.project.name,
                    nameable(key),
                    display(&relative)
                )),
            }
        }
    }
}

/// The declared store a connection-string key belongs to, or why none does.
///
/// Two rules, in this order, and no third:
///
/// 1. **Exactly one declared store.** Then the key is for that one, because
///    that is the only thing this project said it connects to. The key's own
///    text is not consulted at all: a project with only `Npgsql` and a key
///    called `Legacy` still means PostgreSQL.
/// 2. **The key *is* the provider's name.** Compared as identity — both sides
///    reduced to their ASCII alphanumerics and case-folded — so `sqlserver`,
///    `SqlServer` and `SQL Server` are one name, and nothing else is.
///
/// # Why rule 2 is not the name similarity [`super`] forbids
///
/// The prohibition is on inferring a *relationship* from two names resembling
/// each other — `Orders.Api` and an `orders` queue, `Billing` and a `billing`
/// database — where both names were chosen independently and the resemblance is
/// the entire evidence. This is not that. One side is a fixed provider name out
/// of [`DATA_CLIENTS`], written by this module; the other is the author having
/// typed that same provider name as their configuration key. There is no
/// distance measure, no prefix, no substring and no threshold to tune: the two
/// strings are the same string or they are not.
///
/// `RedisCache` is therefore refused, and it is refused knowing perfectly well
/// that it almost certainly is the Redis connection string — see
/// `a_connection_string_key_that_merely_resembles_a_provider_is_attached_to_nothing`.
/// "Almost certainly" is the argument this phase declines everywhere else, and
/// the moment a prefix is allowed the rule stops being one a reader can check by
/// looking at the two words.
///
/// # What rule 2 buys, and why abstaining outright was not enough
///
/// Rule 1 alone would fix the defect: with `Npgsql` and `StackExchange.Redis`
/// declared, both keys abstain and nothing false is printed. But the project in
/// the audit's report has keys `"Orders"` and `"Redis"`, and abstaining on both
/// throws away a pairing whose evidence is as good as evidence in this phase
/// gets — the author wrote `Redis` next to a `StackExchange.Redis` reference.
/// Rule 2 keeps that one and still abstains on `"Orders"`, which is the exact
/// division the files support.
fn placement(
    stores: &[(ComponentKind, &'static str)],
    key: &str,
) -> Result<(ComponentKind, &'static str), String> {
    if let [only] = stores {
        return Ok(*only);
    }

    let named: Vec<&(ComponentKind, &'static str)> = stores
        .iter()
        .filter(|(_, label)| is_the_same_name(key, label))
        .collect();
    if let [only] = named.as_slice() {
        return Ok(**only);
    }

    Err(match stores.len() {
        0 => "the project declares no data store client, so there is nothing it could name"
            .to_string(),
        count => format!(
            "the project declares {count} data store clients and the key does not name one of \
             them, so which one it belongs to is not stated anywhere"
        ),
    })
}

/// Whether two names are the same name written two ways.
///
/// Reduced to ASCII alphanumerics and case-folded, which erases exactly the
/// decoration that varies between a provider's own spelling and a configuration
/// key: spaces, dots and hyphens. It erases nothing else — no character is
/// added, removed or transliterated, so two names that differ by a single
/// letter stay different.
fn is_the_same_name(left: &str, right: &str) -> bool {
    let reduce = |text: &str| -> String {
        text.chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect()
    };
    let left = reduce(left);
    !left.is_empty() && left == reduce(right)
}

/// The distinct stores this project declares a client for, of every kind.
///
/// Every row of [`DATA_CLIENTS`] is a client of something the program connects
/// to over a socket — that is the table's entry condition — so no kind is
/// filtered out here. A filter would have to be kept in agreement with the
/// table, and the last one that was (`kind == Database`) is what produced the
/// mispairing this function's caller documents.
fn declared_stores(read: &Read<'_>) -> Vec<(ComponentKind, &'static str)> {
    let mut stores: Vec<(ComponentKind, &'static str)> = read
        .parsed
        .package_references
        .iter()
        .filter(|package| refused_package(package).is_none())
        .filter_map(|package| data_client(package))
        .collect();
    stores.sort_unstable();
    stores.dedup();
    stores
}

/// `appsettings*.json` beside the project file, sorted.
///
/// Only the project's own directory: `appsettings.json` is loaded from the
/// content root, and walking deeper would pick up the copies that end up beside
/// test fixtures and sample apps.
fn appsettings_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            path.is_file() && name.starts_with("appsettings") && name.ends_with(".json")
        })
        .collect();
    files.sort();
    files
}

// ---------------------------------------------------------------------------
// MEDIUM: what the source says
// ---------------------------------------------------------------------------

/// Scan a project's C# for the two registrations that name another component.
///
/// One walk and one read per file for both rules, because both are looking for
/// a substring in the same lines and reading every `.cs` file twice to ask two
/// questions would double the only expensive thing this producer does.
fn source_scan(
    workspace: &Workspace,
    read: &Read<'_>,
    reads: &[Read<'_>],
    services: &Bindings,
    out: &mut DotnetSignals,
) {
    let is_app_host = read.parsed.is_aspire_host == Some(true)
        || read
            .parsed
            .sdk
            .iter()
            .chain(read.parsed.sdk_imports.iter())
            .any(|sdk| sdk.to_ascii_lowercase().contains("aspire.apphost.sdk"));

    let (files, truncated) = source_files(&read.project.dir);
    if truncated {
        out.warnings.push(format!(
            "{}: only the first {MAX_SOURCE_FILES} C# files under this project were read, so \
             some client registrations may be missing",
            read.project.name
        ));
    }

    for path in files {
        match std::fs::metadata(&path).map(|m| m.len()) {
            Ok(len) if len > MAX_SOURCE_BYTES => {
                out.warnings.push(format!(
                    "{}: {} is larger than {MAX_SOURCE_BYTES} bytes and was not read",
                    read.project.name,
                    display(&relative(&workspace.root, &path))
                ));
                continue;
            }
            Ok(_) => {}
            Err(_) => continue,
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let relative = relative(&workspace.root, &path);
        let lines = code_lines(&text);

        if is_app_host {
            aspire_projects(read, reads, &relative, &lines, out);
        }
        http_clients(read, &relative, &lines, services, out);
    }
}

/// The `.cs` files under a project, sorted, capped, with whether the cap bit.
fn source_files(dir: &Path) -> (Vec<PathBuf>, bool) {
    let mut files: Vec<PathBuf> = source_walker(dir)
        .flatten()
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("cs"))
        })
        .collect();
    // Sorted before truncating, so the cap always keeps the same files.
    files.sort();
    let truncated = files.len() > MAX_SOURCE_FILES;
    files.truncate(MAX_SOURCE_FILES);
    (files, truncated)
}

/// Match `AddProject<Projects.Ident>()` calls back onto scanned projects.
///
/// See [`aspire_class_name`] for the identifier transform and the evidence that
/// it is the SDK's own. Everything that does not resolve to exactly one project
/// becomes a warning, which is the common case in more repositories than it
/// might seem: the app host can override a generated type name per reference
/// with `AspireProjectMetadataTypeName` metadata on the `<ProjectReference>`,
/// and [`dotnet::parse_project_file`] does not read item metadata, so an
/// overridden name is unmatchable from here by construction.
fn aspire_projects(
    read: &Read<'_>,
    reads: &[Read<'_>],
    path: &Path,
    lines: &[(u32, String)],
    out: &mut DotnetSignals,
) {
    for (number, line) in lines {
        for capture in add_project_re().captures_iter(line) {
            let identifier = &capture[1];
            let matches: Vec<&Read<'_>> = reads
                .iter()
                .filter(|candidate| {
                    stem(&candidate.project.manifest_path)
                        .and_then(|stem| aspire_class_name(&stem))
                        .is_some_and(|name| name == identifier)
                })
                .collect();

            match matches.as_slice() {
                [target] => out.signals.push(
                    Signal::medium(
                        ComponentKind::HttpService,
                        &target.project.name,
                        &read.project.id,
                        Evidence::new(path, Some(*number), line.trim()),
                    )
                    .with_detail(format!(
                        "orchestrated by the Aspire app host {}",
                        read.project.name
                    )),
                ),
                [] => out.warnings.push(format!(
                    "{}: the Aspire app host references Projects.{identifier} at {}:{number}, \
                     which matches no scanned project — it may live outside the workspace or \
                     carry an AspireProjectMetadataTypeName override",
                    read.project.name,
                    display(path)
                )),
                many => out.warnings.push(format!(
                    "{}: the Aspire app host reference to Projects.{identifier} at {}:{number} \
                     matches {} scanned projects, so it was not attributed to any of them",
                    read.project.name,
                    display(path),
                    many.len()
                )),
            }
        }
    }
}

/// Match `AddHttpClient` registrations to the service they address.
///
/// # What an `AddHttpClient` proves
///
/// That this project makes HTTP calls. Not to what. A named client
/// (`AddHttpClient("billing")`) names a *configuration section*, not a service;
/// a typed client names an interface in this assembly; and a base address bound
/// to `IConfiguration` is a promise to look the address up at runtime from a
/// file this module has not read and, if it is an environment variable, cannot
/// read. All three are warnings.
///
/// The single exception is a literal base address whose `host:port` matches
/// exactly one other project's `applicationUrl`. Both halves of that are
/// strings the author wrote down, and they either match or they do not.
///
/// # This draws an arrow, and here is why it is allowed to
///
/// A matched base address emits a [`Signal::call`], which is HIGH, and the gate
/// admits it as a service → service call. The subtlety that makes this honest:
/// the caller's address is read from a `.cs` file, which could never be HIGH on
/// its own — but the *identity of the callee* is resolved through that project's
/// own `launchSettings.json`, a declaration file the author wrote, and the
/// signal's evidence cites **that file**, not the source line. So the claim
/// rests on a declaration, and [`super::framework::admit`]'s HIGH
/// declaration-file screen passes rather than being lied to.
///
/// The arrow is still only drawn when both endpoints already exist as service
/// boxes — the caller and the callee both have to have earned a service node —
/// and that check is [`super::super::components`]'s, not this producer's. If the
/// callee is not a web project, no box exists to point at and the assembly step
/// abstains with a warning.
///
/// # The base address is never quoted, anywhere
///
/// Unlike the earlier design, the caller's source line is *not* the evidence:
/// the evidence is [`Evidence::elided_value`] over the callee's
/// `launchSettings.json`, so no url is quoted even on the matched path. On every
/// path that does not match — including an address pointing at a real internal
/// host — no signal is emitted and the warning describes the failure without
/// repeating the address.
fn http_clients(
    read: &Read<'_>,
    path: &Path,
    lines: &[(u32, String)],
    services: &Bindings,
    out: &mut DotnetSignals,
) {
    for (index, (number, line)) in lines.iter().enumerate() {
        if !line.contains("AddHttpClient") {
            continue;
        }

        let found = lines
            .iter()
            .skip(index)
            .take(BASE_ADDRESS_WINDOW)
            .enumerate()
            // Stop before the next registration: a `BaseAddress` below it
            // belongs to that one, not to this one.
            .take_while(|(offset, (_, text))| *offset == 0 || !text.contains("AddHttpClient"))
            .find_map(|(_, (number, text))| {
                base_address_re()
                    .captures(text)
                    .map(|c| (*number, text.trim().to_string(), c[1].to_string()))
            });

        let Some((_address_line, _excerpt, url)) = found else {
            out.warnings.push(format!(
                "{}: the AddHttpClient registration at {}:{number} was not attributed to a \
                 service because no literal base address is written there",
                read.project.name,
                display(path)
            ));
            continue;
        };

        let Some(authority) = authority(&url) else {
            out.warnings.push(format!(
                "{}: the AddHttpClient registration at {}:{number} has a base address that is \
                 not an absolute http(s) url with a specific host, so it names no service",
                read.project.name,
                display(path)
            ));
            continue;
        };

        // The call cites the *callee's* `launchSettings.json`, a declaration
        // file, which is what makes the signal HIGH and the arrow drawable. The
        // caller's `.cs` line (`_address_line`, `_excerpt`) is deliberately not
        // the evidence: a source line could never survive the gate's HIGH
        // declaration-file screen, and anchoring on it would be lying to the
        // gate to get an arrow it would otherwise refuse.
        match services.project_at(&authority) {
            Some(binding) if binding.project_name == read.project.name => {
                out.warnings.push(format!(
                "{}: the AddHttpClient registration at {}:{number} addresses this project's own \
                 launch profile, so no call between services was recorded",
                read.project.name,
                display(path)
            ))
            }
            Some(binding) => out.signals.push(Signal::call(
                &read.project.id,
                &binding.project_id,
                Evidence::elided_value(&binding.launch_settings, binding.line, "applicationUrl"),
            )),
            None => out.warnings.push(format!(
                "{}: the AddHttpClient registration at {}:{number} has a literal base address \
                 that matches no launch profile url in this workspace, so the service it calls \
                 was not identified",
                read.project.name,
                display(path)
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// The Aspire generated class name
// ---------------------------------------------------------------------------

/// The `Projects.<Ident>` class name Aspire generates for a project file.
///
/// # This is the SDK's own rule, not a reconstruction of it
///
/// `Aspire.Hosting.AppHost` generates one `IProjectMetadata` class per
/// referenced project and derives the class name from the project *file name*
/// with a single regex. From
/// `build/Aspire.Hosting.AppHost.targets` in the package (version 13.4.6,
/// downloaded from nuget.org while writing this):
///
/// ```text
/// <_GeneratedClassNameFixupRegex>(((?<=\.)|^)(?=\d)|\W)</_GeneratedClassNameFixupRegex>
/// ...
/// <ClassName>$([System.Text.RegularExpressions.Regex]::Replace(
///     $([System.IO.Path]::GetFileNameWithoutExtension(%(_AspireProjectResource.Identity))),
///     $(_GeneratedClassNameFixupRegex), '_'))</ClassName>
/// ```
///
/// Two alternatives, which is why `Orders.1Api` gains *two* underscores: the
/// `.` matches `\W` and is replaced, and then the position after it matches the
/// zero-width `(?<=\.)(?=\d)` and an underscore is inserted before the digit —
/// because a C# identifier may not start with one, and neither may the segment
/// after a dot in the original name.
///
/// The expectations in `the_aspire_generated_class_name_transform_matches_the_sdk_regex`
/// are not this function's output written down. They were produced by running
/// the SDK's regex through .NET itself on this machine
/// (`dotnet run xf.cs` with `Regex.Replace(input, @"(((?<=\.)|^)(?=\d)|\W)", "_")`),
/// and this implementation was then made to agree with them.
///
/// # Why a non-ASCII name abstains
///
/// .NET's `\W` is Unicode-aware — `\w` is `[\p{L}\p{Mn}\p{Nd}\p{Pc}]` — and
/// Rust has no equivalent classifier in this crate's dependencies. The obvious
/// substitute, `char::is_alphanumeric`, disagrees with .NET in both directions,
/// which the same experiment showed: `Café` passes through .NET unchanged
/// (agreeing), but `Ⅷ` (U+2167, category Nl) and `²` (U+00B2, No) are replaced
/// with `_` by .NET while `is_alphanumeric` calls them word characters, and a
/// combining mark (U+0301, Mn) is a word character to .NET and not alphanumeric
/// to Rust. Each disagreement would produce a *confidently wrong* identifier,
/// which would then fail to match the right project and — worse — could match
/// the wrong one. So a name containing any non-ASCII character produces `None`,
/// the reference goes unresolved, and the caller warns.
pub fn aspire_class_name(file_stem: &str) -> Option<String> {
    if file_stem.is_empty() || !file_stem.is_ascii() {
        return None;
    }

    let mut out = String::with_capacity(file_stem.len() + 1);
    let bytes = file_stem.as_bytes();
    for (index, &byte) in bytes.iter().enumerate() {
        // The zero-width alternative: the start of the name, or just after a
        // dot, immediately followed by a digit.
        if byte.is_ascii_digit() && (index == 0 || bytes[index - 1] == b'.') {
            out.push('_');
        }
        if byte.is_ascii_alphanumeric() || byte == b'_' {
            out.push(byte as char);
        } else {
            out.push('_');
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Reading source and manifests
// ---------------------------------------------------------------------------

fn add_project_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"AddProject\s*<\s*Projects\s*\.\s*([A-Za-z_][A-Za-z0-9_]*)\s*>")
            .expect("the AddProject pattern is a literal and compiles")
    })
}

fn base_address_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"BaseAddress\s*=\s*new\s*[A-Za-z_.]*\s*\(\s*"([^"]*)""#)
            .expect("the BaseAddress pattern is a literal and compiles")
    })
}

/// Every line of a C# file with its comments blanked out, numbered from 1.
///
/// # Why this is string-aware and the symbol index's version is not
///
/// [`crate::symbols::declarations`] decides whether a *whole line* is prose by
/// looking at its first characters, which is right for its job and useless
/// here: the thing this file is looking for is
/// `new Uri("https://localhost:7080")`, and a scanner that treated the `//`
/// inside that string literal as the start of a comment would blank the address
/// it exists to read. So this tracks string, verbatim-string and character
/// literals as well as `//` and `/* */`, replacing only genuine comment text
/// with spaces and leaving column positions intact.
///
/// # Where it is wrong
///
/// A C# 11 raw string literal (`"""..."""`) is not modelled, and a `#if false`
/// region is not evaluated at all. Both can only cause this scan to see a line
/// as code when it is not, or to blank a line that is — and every consumer of
/// the result either finds nothing (a missed signal, reported as a warning) or
/// emits a MEDIUM signal that must still find a HIGH component to attach to.
/// Neither can create a box or an arrow.
fn code_lines(text: &str) -> Vec<(u32, String)> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Code,
        Line,
        Block,
        Str,
        Verbatim,
        Char,
    }

    let mut state = State::Code;
    let mut out = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let chars: Vec<char> = raw.chars().collect();
        let mut line = String::with_capacity(raw.len());
        let mut i = 0;
        if state == State::Line {
            state = State::Code;
        }
        while i < chars.len() {
            let c = chars[i];
            let next = chars.get(i + 1).copied();
            match state {
                State::Code => match (c, next) {
                    ('/', Some('/')) => {
                        state = State::Line;
                        break;
                    }
                    ('/', Some('*')) => {
                        state = State::Block;
                        i += 2;
                        continue;
                    }
                    ('@', Some('"')) => {
                        state = State::Verbatim;
                        line.push(c);
                        line.push('"');
                        i += 2;
                        continue;
                    }
                    ('"', _) => {
                        state = State::Str;
                        line.push(c);
                    }
                    ('\'', _) => {
                        state = State::Char;
                        line.push(c);
                    }
                    _ => line.push(c),
                },
                State::Block => {
                    if c == '*' && next == Some('/') {
                        state = State::Code;
                        i += 2;
                        continue;
                    }
                }
                State::Str | State::Char => {
                    line.push(c);
                    if c == '\\' {
                        if let Some(escaped) = next {
                            line.push(escaped);
                            i += 2;
                            continue;
                        }
                    } else if (state == State::Str && c == '"')
                        || (state == State::Char && c == '\'')
                    {
                        state = State::Code;
                    }
                }
                State::Verbatim => {
                    line.push(c);
                    if c == '"' {
                        if next == Some('"') {
                            line.push('"');
                            i += 2;
                            continue;
                        }
                        state = State::Code;
                    }
                }
                State::Line => break,
            }
            i += 1;
        }
        // A string or character literal cannot span a line break; a verbatim
        // one can, and a block comment can.
        if matches!(state, State::Str | State::Char) {
            state = State::Code;
        }
        out.push((index as u32 + 1, line));
    }
    out
}

/// The line a package or property is declared on, and the text of it.
///
/// `needles` are tried in order and the first that matches anywhere wins, so a
/// caller can ask for the precise form (`"Npgsql"`, quoted) before the loose
/// one. Falls back to a description when none of them is found, which happens
/// when an item is written across several lines. The fallback is marked as such
/// by having no line number: evidence with a line is always a quotation,
/// evidence without one never is.
fn declaration_line(text: &str, needles: &[&str]) -> (Option<u32>, String) {
    for needle in needles {
        for (index, line) in text.lines().enumerate() {
            if line.contains(needle) {
                return (Some(index as u32 + 1), line.trim().to_string());
            }
        }
    }
    (
        None,
        format!("declares {}", needles.last().copied().unwrap_or_default()),
    )
}

fn line_containing(text: &str, needle: &str) -> Option<u32> {
    line_containing_from(text, needle, 1)
}

fn line_containing_from(text: &str, needle: &str, from: u32) -> Option<u32> {
    text.lines()
        .enumerate()
        .skip(from.saturating_sub(1) as usize)
        .find(|(_, line)| line.contains(needle))
        .map(|(index, _)| index as u32 + 1)
}

/// A configuration key, or a placeholder when it is not shaped like a name.
///
/// Warnings are not screened by the gate, so a key that is somehow a whole
/// connection string is described rather than quoted.
///
/// Defers to [`framework::looks_like_a_value`] rather than testing the shapes
/// here. The hand-written copy this replaced checked `=`, `;`, `://` and length
/// and stopped there, so `redis-prod.internal:6380` — a `host:port`, which the
/// gate refuses as a label — passed it and was quoted into a warning that ships
/// in the exported diagram. Two guards against one hazard have to be one guard:
/// whichever is more permissive is the one that decides, and nothing makes the
/// weaker copy visible when the stronger one is tightened.
fn nameable(key: &str) -> String {
    if framework::looks_like_a_value(key) {
        "(a key not shaped like a name)".to_string()
    } else {
        format!("'{key}'")
    }
}

fn stem(path: &Path) -> Option<String> {
    path.file_stem().map(|s| s.to_string_lossy().into_owned())
}

/// Workspace-relative and forward-slashed, matching what
/// [`super::super::graph`] puts in nodes and warnings.
fn relative(root: &Path, path: &Path) -> PathBuf {
    let relative = path.strip_prefix(root).unwrap_or(path);
    PathBuf::from(
        relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn display(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}
