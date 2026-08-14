//! The Cargo (Rust) ecosystem adapter — manifest reading only.
//!
//! # This adapter deliberately creates no run or test configurations
//!
//! Everything here exists to answer two questions: *is this directory a Rust
//! project?* and *which other projects does it depend on?* It does not build a
//! `cargo run` or `cargo test` command line, and that omission is a decision
//! rather than an unfinished corner.
//!
//! Detected configurations are re-derived on every workspace scan and land in
//! the Run and Tests tabs for every workspace the user opens. Teaching this
//! adapter to emit them would therefore change what those two tabs list
//! app-wide, for every Rust repository, on the next scan — a product decision
//! nobody has made. It also opens questions this module has no way to answer
//! honestly: which of a workspace's members to offer, whether `cargo test`'s
//! human output should be parsed at all when the crate's whole test-reporting
//! story is "there is no report file", and what happens to the "re-run failed"
//! guard without one. Anyone who wants Rust runs today already has them:
//! `examples/adapters/cargo-nextest.toml` is a declarative adapter that emits
//! JUnit XML, which the existing `testing/` parsers read.
//!
//! # What a manifest can and cannot tell you
//!
//! A `Cargo.toml` is one file in a workspace, and several of the things a
//! caller wants are simply not in it. Member globs are patterns, not
//! directories; `name.workspace = true` resolves against a file this module
//! was not given; and the implicit `src/main.rs` / `src/lib.rs` conventions
//! are facts about a directory, not about the manifest text. In each case this
//! module reports what the manifest says and stops, leaving the join to the
//! caller that holds both halves. That is the same rule the rest of the crate
//! follows: a wrong answer is much worse than no answer, and a parser that
//! guesses at the other half produces wrong answers silently.

use serde::Deserialize;

/// Which dependency section a path dependency was written in.
///
/// The kind is carried rather than flattened away because the three sections
/// make genuinely different architectural claims. A normal dependency says the
/// shipped artefact contains the other crate; a dev-dependency says only the
/// test and example builds do; a build-dependency says the *build script* does
/// and the artefact may contain nothing of it at all.
///
/// Dropping dev- and build-dependencies would hide real edges — a test-support
/// crate that half the workspace depends on is part of the architecture.
/// Merging them into the normal ones would over-claim, drawing a test-only
/// coupling exactly as heavily as a shipped one. Neither is acceptable, so the
/// distinction is preserved and the decision about how to draw it belongs to
/// the caller, which knows what the diagram is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    Normal,
    Dev,
    Build,
}

/// A dependency on another crate on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathDependency {
    /// The crate's own name — the `package` key when the dependency is
    /// renamed, otherwise the table key.
    ///
    /// A renamed dependency (`alias = { path = "../foo", package = "foo" }`)
    /// has two names: the alias the depending crate types in its `use`
    /// statements, and the name the crate on disk actually publishes. Only the
    /// latter can be matched against another manifest's `[package] name`, and
    /// matching is the entire reason a caller asks for this, so the real name
    /// is what is stored. The alias is local to the depending crate and is
    /// dropped.
    pub name: String,
    /// The `path` value **exactly as the author wrote it**.
    ///
    /// Not normalised, not made absolute, `..` segments not resolved.
    /// Resolving needs the directory the manifest was read from, which this
    /// parser does not have — and when resolution fails, the string the user
    /// actually typed is the only useful thing to show them. This mirrors the
    /// treatment of `<ProjectReference Include="...">` in `dotnet.rs`.
    pub path: String,
    pub kind: DependencyKind,
}

/// The parts of a `Cargo.toml` this adapter uses.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CargoManifest {
    /// `[package] name`, absent in a virtual manifest — and also absent when
    /// the name is inherited with `name.workspace = true`, which this file
    /// cannot resolve alone.
    pub package_name: Option<String>,
    /// `[workspace] members`, globs left verbatim.
    pub workspace_members: Vec<String>,
    /// `[workspace] exclude`, likewise verbatim.
    ///
    /// Read rather than ignored, because the caller matches member patterns
    /// against directories it has already discovered rather than against the
    /// filesystem, and `members = ["crates/*"]` with `exclude =
    /// ["crates/legacy"]` matches a directory that is explicitly *not* a
    /// member. Discarding the exclusions here would make that wrong membership
    /// unrecoverable downstream; keeping them costs one field and lets the
    /// caller subtract. A caller that never writes `exclude` never notices.
    pub workspace_exclude: Vec<String>,
    /// Every `{ path = ... }` dependency, grouped normal, then dev, then
    /// build. Nothing is de-duplicated: a crate listed in two sections is two
    /// entries, because that is two different statements about it.
    pub path_dependencies: Vec<PathDependency>,
    /// Whether a `[workspace]` table is present. True for a virtual manifest
    /// and also for a root crate that is itself a package.
    pub is_workspace_root: bool,
    /// Whether the manifest declares at least one `[[bin]]`. **Says nothing
    /// about `src/main.rs`** — see `has_lib`.
    pub has_bin: bool,
    /// Whether the manifest declares a `[lib]` section.
    ///
    /// Cargo infers a library from `src/lib.rs` and a binary from
    /// `src/main.rs` with no manifest section at all, which is how most crates
    /// are written — including `cb-core` itself, whose manifest declares no
    /// `[lib]` while `src-tauri`'s does. This field is
    /// therefore `false` far more often than a crate is not a library, and a
    /// caller that treats it as "is not a library" will be wrong most of the
    /// time. It reports what the manifest *declares*; the filesystem half of
    /// the answer belongs to whoever holds the directory.
    pub has_lib: bool,
}

impl CargoManifest {
    /// A `[workspace]` with no `[package]`: a workspace root that is not
    /// itself a crate.
    ///
    /// This repository's own root `Cargo.toml` is exactly this shape, so
    /// treating it as a project would put an empty box on the very diagram
    /// this adapter was added to fix.
    pub fn is_virtual_manifest(&self) -> bool {
        self.is_workspace_root && self.package_name.is_none()
    }
}

/// Parse a `Cargo.toml`.
///
/// `None` means only one thing: the TOML itself would not parse. Every other
/// disappointment — no `[package]`, a `members` entry that is a number, a
/// dependency table with no recognised source — yields a `CargoManifest` that
/// is simply quiet about the part it could not read. A workspace scan walks
/// whatever is on disk, including files someone is halfway through editing,
/// and a malformed section must cost the caller that section and nothing more.
pub fn parse(toml: &str) -> Option<CargoManifest> {
    let doc: toml::Table = toml::from_str(toml).ok()?;

    let mut manifest = CargoManifest {
        package_name: doc
            .get("package")
            .and_then(toml::Value::as_table)
            // `as_str` is what rejects `name.workspace = true`: an inherited
            // name parses as a table, not a string.
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str)
            .map(str::to_string),
        is_workspace_root: doc.contains_key("workspace"),
        has_bin: doc
            .get("bin")
            .and_then(toml::Value::as_array)
            .is_some_and(|bins| !bins.is_empty()),
        has_lib: doc.get("lib").is_some_and(toml::Value::is_table),
        ..CargoManifest::default()
    };

    if let Some(workspace) = doc.get("workspace").and_then(toml::Value::as_table) {
        manifest.workspace_members = string_list(workspace.get("members"));
        manifest.workspace_exclude = string_list(workspace.get("exclude"));
    }

    // Section-major order, so a caller that only wants shipped edges can stop
    // at the first non-`Normal` entry. Within one kind the root's own section
    // comes before the `[target.'cfg(...)']` ones.
    for (section, kind) in [
        ("dependencies", DependencyKind::Normal),
        ("dev-dependencies", DependencyKind::Dev),
        ("build-dependencies", DependencyKind::Build),
    ] {
        // `[workspace.dependencies]` is deliberately not read here. It
        // declares what members *may* inherit, not what anything depends on:
        // the edge belongs to whichever member writes `foo.workspace = true`,
        // a fact stored in that member's file rather than this one. Attaching
        // it to the root would draw an arrow out of a node that, in a virtual
        // manifest, is not a project at all.
        collect_path_dependencies(doc.get(section), kind, &mut manifest.path_dependencies);

        if let Some(targets) = doc.get("target").and_then(toml::Value::as_table) {
            for cfg in targets.values() {
                let Some(cfg) = cfg.as_table() else { continue };
                collect_path_dependencies(cfg.get(section), kind, &mut manifest.path_dependencies);
            }
        }
    }

    Some(manifest)
}

/// The parts of one dependency entry this adapter reads.
///
/// Deserialising into a struct rather than poking at `toml::Value` keys is
/// what makes the two spellings of a table dependency — inline
/// (`foo = { path = "../foo" }`) and sectioned (`[dependencies.foo]`) — one
/// code path: TOML has already collapsed them into the same value by the time
/// this sees them. A plain string version (`serde = "1"`) fails to deserialise
/// into this struct, which is exactly the intended outcome, since a registry
/// dependency is third-party and out of scope in the same way an npm registry
/// package is in `node.rs`.
#[derive(Deserialize)]
struct DependencyEntry {
    path: Option<String>,
    package: Option<String>,
}

fn collect_path_dependencies(
    section: Option<&toml::Value>,
    kind: DependencyKind,
    out: &mut Vec<PathDependency>,
) {
    let Some(entries) = section.and_then(toml::Value::as_table) else {
        return;
    };

    for (key, value) in entries {
        let Ok(entry) = value.clone().try_into::<DependencyEntry>() else {
            continue;
        };
        // No `path` key means a registry, git or inherited dependency: not a
        // crate in this workspace, so not an edge.
        let Some(path) = entry.path else { continue };

        out.push(PathDependency {
            name: entry.package.unwrap_or_else(|| key.clone()),
            path,
            kind,
        });
    }
}

/// The string entries of a TOML array, skipping anything that is not a string.
///
/// A non-string entry is dropped rather than failing the whole parse, for the
/// same reason `node.rs::workspace_globs` drops one: losing a bad member
/// pattern costs the caller that pattern, whereas rejecting the file costs it
/// the entire workspace layout.
fn string_list(value: Option<&toml::Value>) -> Vec<String> {
    value
        .and_then(toml::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}
