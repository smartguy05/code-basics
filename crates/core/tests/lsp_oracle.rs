//! The answers a **real** language server gives, for the languages this machine
//! happens to have installed.
//!
//! Every other test in this subsystem runs against `cb-fake-lsp`, which is the
//! only way the misbehaviour paths (garbage frames, duplicate ids, a server that
//! dies mid-request) get covered at all. The cost of that choice is that the
//! whole suite can be green while the app is wrong: the one-shot-notification
//! bug shipped "No usages" for a method with exactly one usage while 1901 tests
//! passed, because nothing here had ever spoken to Roslyn.
//!
//! So this file is the oracle. It starts a session through
//! [`session::start`] with the **real** [`registry::RealProbe`] — the same
//! discovery the app runs — over a small project it writes itself, and asserts
//! the one answer it knows to be true. A pass therefore covers discovery, the
//! argument list, the readiness signal, the `file:` URI spelling and the result
//! decoding in one go, per server, against the real thing.
//!
//! # Why `#[ignore]`
//!
//! Following `pnpm_resolves_to_its_cmd_shim_on_a_machine_that_has_it`
//! (`crates/core/src/process/resolve.rs`) and
//! `report_attribution_against_this_repository` (`tests/intent_attribution.rs`):
//! the result depends on what is installed on the machine, and these spawn real
//! servers that take tens of seconds to load a project. Run them deliberately,
//! from a shell with `sh` on PATH:
//!
//! ```text
//! cargo test -p cb-core --test lsp_oracle -- --ignored --nocapture --test-threads=1
//! ```
//!
//! **`--test-threads=1` is not optional.** These start real language servers,
//! and cargo's default is one thread per core: four servers indexing at once,
//! plus a `dotnet restore`, starved Roslyn's project load past the ninety-second
//! readiness ceiling and failed the C# oracle on a machine where it passes in
//! seven seconds. Serially the whole file takes about twenty-five.
//!
//! # The one thing that may be skipped
//!
//! A language whose server is **not installed** prints a line and returns.
//! Nothing else does. In particular [`Availability::Failed`] and
//! [`Availability::Unsupported`] fail the test: an installed server that cannot
//! answer is precisely the regression this file exists to catch, and skipping on
//! it would turn the oracle into decoration. `NotConfigured` is the only outcome
//! that means "this machine, not this code".
//!
//! # What keeps the fixtures honest
//!
//! The oracle asserts `total == Some(1)`, which is only the truth if the symbol
//! is written exactly twice in the whole project — once declared, once called.
//! `fixtures_say_what_the_oracles_assert` proves that about every fixture and is
//! **not** ignored, so an edit that quietly makes an oracle assert the wrong line
//! fails an ordinary `cargo test`. It plays the same role as the unconditional
//! invariants at the end of `intent_attribution.rs`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cb_core::lsp::model::{Availability, ServerStatus};
use cb_core::lsp::registry::{Probe, RealProbe};
use cb_core::lsp::session::{self, LspHandle};
use cb_core::lsp::settings::{LspConfig, ServerOverride};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// The table
// ---------------------------------------------------------------------------

/// One language's known-correct answer, and the project that produces it.
struct Oracle {
    /// The registry id, which is also the key into `lsp.servers`.
    language: &'static str,
    /// Writes a self-contained project into an empty directory. Files only —
    /// [`fixtures_say_what_the_oracles_assert`] runs this on every `cargo test`
    /// and must stay instant.
    write_fixture: fn(&Path),
    /// Anything the ecosystem's tooling has to do before a server can resolve
    /// across files. Run only by the oracle, never by the fixture check.
    prepare: Option<fn(&Path)>,
    /// Workspace-relative, forward slashes — the spelling
    /// [`cb_core::symbols::index::relative_to_root`] produces, so the assertions
    /// can compare against it directly.
    decl_file: &'static str,
    /// **1-based**, the line the identifier is declared on.
    decl_line: u32,
    /// The identifier itself. Written exactly twice across the whole fixture.
    symbol: &'static str,
    caller_file: &'static str,
    /// **1-based**, the one call site.
    caller_line: u32,
    /// How long this server may take to reach [`Availability::Ready`]. Roslyn
    /// loads a solution; `tsserver` does not. Taken from the registry's own
    /// `first_request` timeouts rather than invented here.
    ready_secs: u64,
    /// Forces one specific candidate instead of letting discovery pick. `None`
    /// exercises discovery, which is the point for the primary oracles; `Some`
    /// is how the Python candidates that discovery would never reach get run.
    program: Option<&'static str>,
}

const CSHARP: Oracle = Oracle {
    language: "csharp",
    write_fixture: write_csharp,
    prepare: Some(restore_csharp),
    decl_file: "Collections.cs",
    decl_line: 5,
    symbol: "TryGetElements",
    caller_file: "Walker.cs",
    caller_line: 7,
    ready_secs: 180,
    program: None,
};

const RUST: Oracle = Oracle {
    language: "rust",
    write_fixture: write_rust,
    prepare: None,
    decl_file: "lib.rs",
    decl_line: 4,
    symbol: "try_get_elements",
    caller_file: "walker.rs",
    caller_line: 2,
    ready_secs: 120,
    program: None,
};

const TYPESCRIPT: Oracle = Oracle {
    language: "typescript",
    write_fixture: write_typescript,
    prepare: None,
    decl_file: "collections.ts",
    decl_line: 1,
    symbol: "tryGetElements",
    caller_file: "walker.ts",
    caller_line: 4,
    ready_secs: 60,
    program: None,
};

const PYTHON: Oracle = Oracle {
    language: "python",
    write_fixture: write_python,
    prepare: None,
    decl_file: "collections_mod.py",
    decl_line: 1,
    symbol: "try_get_elements",
    caller_file: "walker.py",
    caller_line: 5,
    ready_secs: 60,
    program: None,
};

/// Every oracle, for the fixture-consistency test.
const ALL: &[&Oracle] = &[&CSHARP, &RUST, &TYPESCRIPT, &PYTHON];

// ---------------------------------------------------------------------------
// The oracles
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "spawns the real language server; only meaningful where one is installed"]
async fn csharp_answers_the_one_usage_it_should() {
    run(&CSHARP).await;
}

#[tokio::test]
#[ignore = "spawns the real language server; only meaningful where one is installed"]
async fn rust_answers_the_one_usage_it_should() {
    run(&RUST).await;
}

#[tokio::test]
#[ignore = "spawns the real language server; only meaningful where one is installed"]
async fn typescript_answers_the_one_usage_it_should() {
    run(&TYPESCRIPT).await;
}

#[tokio::test]
#[ignore = "spawns the real language server; only meaningful where one is installed"]
async fn python_answers_the_one_usage_it_should() {
    run(&PYTHON).await;
}

// The Python row lists its candidates in preference order, so discovery only
// ever exercises the first one installed. This forces the other by name — the
// same thing a user does with `lsp.servers.python.program` — so an argument list
// that discovery would never reach is still covered.
//
// `pylsp` and `jedi-language-server` had tests here too, and running them is why
// they are no longer candidates at all: `pylsp` answered this fixture with zero
// usages, and `jedi-language-server` answered two, because it ignores
// `includeDeclaration: false` and counts the declaration. See
// `registry::candidates`.

#[tokio::test]
#[ignore = "spawns the real language server; only meaningful where one is installed"]
async fn python_answers_through_pyright_langserver() {
    run(&Oracle {
        program: Some("pyright-langserver"),
        ..PYTHON
    })
    .await;
}

// ---------------------------------------------------------------------------
// The harness
// ---------------------------------------------------------------------------

/// A started session over a temporary project.
///
/// `handle` is declared first so it drops first, and `Drop` asks for teardown
/// before the directory goes: the server's cwd *is* that directory, and
/// rust-analyzer in particular will happily write into it while it dies.
struct Workspace {
    handle: LspHandle,
    dir: TempDir,
}

impl Drop for Workspace {
    fn drop(&mut self) {
        self.handle.request_teardown();
    }
}

impl Workspace {
    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    /// **`None` is the healthy answer for a server nobody has asked anything
    /// yet.**
    ///
    /// `Session::refresh_status` returns no row for `State::Idle`, because the
    /// status surface's rule is to say nothing about a server with nothing to
    /// say. So a resolved, installed, not-yet-started server is absent from this
    /// list, while a server that is *not installed* has a row reading
    /// [`Availability::NotConfigured`]. Reading those two the other way round
    /// skips on exactly the machines where the oracle would have worked.
    fn row(&self, language: &str) -> Option<ServerStatus> {
        self.handle
            .status()
            .servers
            .into_iter()
            .find(|server| server.id == language)
    }

    fn state(&self, language: &str) -> Option<Availability> {
        self.row(language).map(|server| server.state)
    }
}

async fn run(oracle: &Oracle) {
    // A forced program that is not on the machine resolves to `Misconfigured`,
    // which the session reports as `Failed` — correctly, since it *is*
    // configured and it *is* broken. That is indistinguishable from a real
    // failure at the status surface, so the "is it installed" question is asked
    // here instead, before anything is configured.
    if let Some(program) = oracle.program {
        if RealProbe.on_path(program).is_none() {
            eprintln!("skipped: {program} is not on PATH");
            return;
        }
    }

    let dir = fixture_root();
    (oracle.write_fixture)(dir.path());
    if let Some(prepare) = oracle.prepare {
        prepare(dir.path());
    }

    let config = oracle.program.map(|program| {
        let mut servers = BTreeMap::new();
        servers.insert(
            oracle.language.to_string(),
            ServerOverride {
                program: Some(program.to_string()),
                // Absent, not empty: the built-in argument list is exactly what
                // is under test here.
                ..ServerOverride::default()
            },
        );
        LspConfig { servers }
    });

    let handle = session::start(dir.path().to_path_buf(), config, 1);
    let workspace = Workspace { handle, dir };

    // `resolve_all` runs synchronously inside `session::start`, so this is
    // decided by the time the handle exists — no waiting required.
    if let Some(row) = workspace.row(oracle.language) {
        if row.state == Availability::NotConfigured {
            eprintln!(
                "skipped: no {} language server on this machine ({})",
                oracle.language,
                row.detail.as_deref().unwrap_or("no detail")
            );
            return;
        }
    }

    // Opening the document is what starts the server: starts are lazy and stay
    // lazy, so a session nobody asked anything of never spawns a process.
    let decl = workspace.path(oracle.decl_file);
    let decl_text = read(&decl);
    workspace.handle.open_document(&decl, &decl_text).await;

    // `None` keeps waiting too: there is a moment between the open being
    // enqueued and the actor reaching `ensure_started` where the row still does
    // not exist.
    let settled = until(Duration::from_secs(oracle.ready_secs), || {
        matches!(
            workspace.state(oracle.language),
            Some(Availability::Ready | Availability::Failed | Availability::Unsupported)
        )
    })
    .await;
    assert!(
        settled,
        "the {} server was still {:?} after {}s",
        oracle.language,
        workspace.state(oracle.language),
        oracle.ready_secs
    );
    let row = workspace
        .row(oracle.language)
        .expect("a settled server always has a row");
    assert_eq!(
        row.state,
        Availability::Ready,
        "the {} server is {:?}, not ready: {:?}",
        oracle.language,
        row.state,
        row.detail
    );
    eprintln!(
        "{}: ready via {}{}",
        oracle.language,
        row.detail.as_deref().unwrap_or("an unnamed program"),
        row.caveat
            .as_deref()
            .map(|caveat| format!(" (caveat: {caveat})"))
            .unwrap_or_default()
    );

    // 1. The inline row exists at all, and is aimed at the identifier rather
    //    than at the start of the declaration. Everything after this uses the
    //    anchor's own coordinates, which is exactly what the UI does — computing
    //    them here instead would test arithmetic this file does not own.
    let anchors = workspace.handle.declaration_anchors(&decl).await;
    assert_eq!(
        anchors.outcome,
        Availability::Ready,
        "declaration anchors for {}: {:?}",
        oracle.decl_file,
        anchors.message
    );
    let anchor = anchors
        .anchors
        .iter()
        .find(|anchor| anchor.name == oracle.symbol)
        .unwrap_or_else(|| {
            let found: Vec<_> = anchors.anchors.iter().map(|a| a.name.as_str()).collect();
            panic!(
                "no declaration anchor named {} in {}; found {found:?}",
                oracle.symbol, oracle.decl_file
            )
        });
    assert_eq!(
        anchor.selection_line, oracle.decl_line,
        "{} is declared on line {} of {}, not {}",
        oracle.symbol, oracle.decl_line, oracle.decl_file, anchor.selection_line
    );

    // 2. The count, which is the claim the whole subsystem exists to get right.
    //    `Some(1)`, never `None`: the `Option` is there so a real count and "no
    //    answer" stay apart, and an oracle that accepted either would not notice
    //    them collapsing.
    let usages = workspace
        .handle
        .find_usages(&decl, anchor.selection_line, anchor.character)
        .await;
    assert_eq!(
        usages.outcome,
        Availability::Ready,
        "usages of {}: {:?}",
        oracle.symbol,
        usages.message
    );
    assert_eq!(
        usages.total,
        Some(1),
        "{} is called exactly once, from {}:{} — got {:?} ({:?})",
        oracle.symbol,
        oracle.caller_file,
        oracle.caller_line,
        usages.total,
        usages.usages
    );
    let usage = &usages.usages[0];
    assert_eq!(
        usage.path.as_deref(),
        Some(Path::new(oracle.caller_file)),
        "the usage is in the wrong file: {:?}",
        usage.label
    );
    assert_eq!(
        usage.line, oracle.caller_line,
        "the usage is on the wrong line of {}",
        oracle.caller_file
    );
    // The fixtures are ASCII, so a UTF-16 offset and a byte offset agree and the
    // span can be sliced directly. `positions_tests` owns the case where they
    // do not.
    let highlight = usage
        .highlight
        .unwrap_or_else(|| panic!("the usage row carries no highlight: {:?}", usage.snippet));
    assert_eq!(
        usage
            .snippet
            .get(highlight.start as usize..highlight.end as usize),
        Some(oracle.symbol),
        "the highlight does not cover {} in {:?}",
        oracle.symbol,
        usage.snippet
    );

    // 3. The jump back, which is the only real traffic over the goto decoding —
    //    including the `LocationLink` shape, which Roslyn never sends but
    //    rust-analyzer does.
    let caller = workspace.path(oracle.caller_file);
    let caller_text = read(&caller);
    workspace.handle.open_document(&caller, &caller_text).await;
    let character = column_of(&caller_text, oracle.caller_line, oracle.symbol);
    let definition = workspace
        .handle
        .goto_definition(&caller, oracle.caller_line, character)
        .await;
    assert_eq!(
        definition.outcome,
        Availability::Ready,
        "definition of {}: {:?}",
        oracle.symbol,
        definition.message
    );
    let target = definition.declarations.first().unwrap_or_else(|| {
        panic!(
            "no declaration for {} from {}:{} ({:?})",
            oracle.symbol, oracle.caller_file, oracle.caller_line, definition.message
        )
    });
    assert_eq!(
        target.path.as_deref(),
        Some(Path::new(oracle.decl_file)),
        "the definition is in the wrong file: {:?}",
        target.label
    );
    assert_eq!(
        target.line, oracle.decl_line,
        "the definition is on the wrong line of {}",
        oracle.decl_file
    );
}

/// Wait for a condition rather than for a duration, and say so when it never
/// comes true.
///
/// Polling rather than a signal because the thing being waited on is a real
/// server loading a real project, and there is nothing to subscribe to.
async fn until(deadline: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let started = std::time::Instant::now();
    while !condition() {
        if started.elapsed() > deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    true
}

/// An empty directory to write a fixture project into.
///
/// **Deliberately not `tempfile::tempdir()`.** On the machine this was written
/// on, `%TEMP%` is the MS-DOS 8.3 short form — `C:\Users\ANTHON~1\AppData\...` —
/// and Roslyn cannot load a project underneath one. The failure is silent and
/// expensive: the server starts, answers `textDocument/documentSymbol` about the
/// open file quite happily, never sends
/// `workspace/projectInitializationComplete`, is promoted at the 90-second
/// readiness ceiling, and then answers `references` with an empty list. So the
/// symptom is a **wrong zero, ninety seconds late, from a server that looks
/// healthy** — with, to its credit, the caveat this subsystem was built to
/// attach ("a count may be low"). The identical fixture under a long path loads
/// in about seven seconds and answers correctly.
///
/// `CARGO_TARGET_TMPDIR` is cargo's own scratch directory for integration tests
/// and inherits the target directory's spelling, which is the repository path
/// and therefore long.
fn fixture_root() -> TempDir {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(base).unwrap_or_else(|error| {
        panic!("creating {}: {error}", base.display());
    });
    if base.to_string_lossy().contains('~') {
        eprintln!(
            "warning: {} looks like an 8.3 short path. Roslyn will not load a \
             project under one, and the C# oracle will see zero usages.",
            base.display()
        );
    }
    tempfile::Builder::new()
        .prefix("oracle-")
        .tempdir_in(base)
        .expect("a fixture directory")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// The **0-based UTF-16 column** of `symbol` on the **1-based** `line`.
///
/// The asymmetry is the one `model.rs` documents, not a slip: lines match the
/// editor gutter, columns match what CodeMirror hands a caller.
///
/// Panics when the symbol is not there, which is what makes this double as a
/// fixture check: [`fixtures_say_what_the_oracles_assert`] calls it for exactly
/// that reason.
fn column_of(text: &str, line: u32, symbol: &str) -> u32 {
    let source = text
        .lines()
        .nth(line as usize - 1)
        .unwrap_or_else(|| panic!("there is no line {line}"));
    let byte = source
        .find(symbol)
        .unwrap_or_else(|| panic!("line {line} does not contain {symbol}: {source:?}"));
    source[..byte].chars().map(char::len_utf16).sum::<usize>() as u32
}

// ---------------------------------------------------------------------------
// The fixtures
// ---------------------------------------------------------------------------
//
// Self-contained and dependency-free, so nothing is downloaded and a project
// load is seconds rather than the tens of seconds this repository's own solution
// costs. Each writes one declaration and exactly one call site — no import of
// the symbol by name anywhere, because an import is itself a reference and would
// make the count two.

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("a fixture directory");
    }
    std::fs::write(&path, contents).unwrap_or_else(|error| panic!("writing {relative}: {error}"));
}

fn write_csharp(root: &Path) {
    // NuGet walks up to the machine-wide configuration, and this one really does
    // pick up a `<packageSources>` entry pointing at a directory that does not
    // exist — which fails the restore before the fixture has said anything. The
    // project references no packages, so clearing the list outright is both the
    // hermetic answer and the accurate one.
    write(
        root,
        "NuGet.config",
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <configuration>\n  \
         <packageSources>\n    \
         <clear />\n  \
         </packageSources>\n\
         </configuration>\n",
    );
    write(
        root,
        "Collections.cs",
        "namespace Oracle;\n\
         \n\
         public static class Collections\n\
         {\n    \
         public static bool TryGetElements(object source, out int count)\n    \
         {\n        \
         count = 0;\n        \
         return source is not null;\n    \
         }\n\
         }\n",
    );
    write(
        root,
        "Walker.cs",
        "namespace Oracle;\n\
         \n\
         public static class Walker\n\
         {\n    \
         public static int Walk(object source)\n    \
         {\n        \
         if (Collections.TryGetElements(source, out var count))\n        \
         {\n            \
         return count;\n        \
         }\n        \
         return -1;\n    \
         }\n\
         }\n",
    );
}

/// Write the project file and restore it, because Roslyn cannot answer without
/// a restore.
///
/// Learned by running it: with `--autoLoadProjects` the server loads an
/// unrestored `.csproj`, publishes `workspace/projectInitializationComplete` and
/// reports itself **ready in seconds** — and then answers `references` with an
/// empty list, because there is no `project.assets.json` and so the design-time
/// build produces no compilation to resolve names against. A confident, prompt,
/// wrong zero: exactly the failure this subsystem exists to avoid, and one no
/// amount of scripted-fake testing would ever show.
///
/// The project file is written here rather than in [`write_csharp`] because the
/// target framework is a fact about the machine, not about the fixture.
fn restore_csharp(root: &Path) {
    let tfm = installed_target_framework();
    write(
        root,
        "Oracle.csproj",
        &format!(
            "<Project Sdk=\"Microsoft.NET.Sdk\">\n  \
             <PropertyGroup>\n    \
             <TargetFramework>{tfm}</TargetFramework>\n    \
             <Nullable>enable</Nullable>\n  \
             </PropertyGroup>\n\
             </Project>\n"
        ),
    );

    let output = std::process::Command::new("dotnet")
        .arg("restore")
        .arg("Oracle.csproj")
        .current_dir(root)
        .output()
        .expect("`dotnet restore` — the C# oracle needs the SDK, not just the server");
    assert!(
        output.status.success(),
        "`dotnet restore` of a {tfm} project failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The newest target framework this machine can restore **offline**.
///
/// An SDK ships the targeting pack for its own band, so `net<major>.0` for the
/// highest installed SDK needs no package source at all. Naming a framework
/// whose pack is absent sends NuGet to the network for
/// `Microsoft.NETCore.App.Ref`, which is how a hardcoded `net8.0` failed here on
/// a box carrying only the 9 and 10 SDKs.
fn installed_target_framework() -> String {
    let output = std::process::Command::new("dotnet")
        .arg("--list-sdks")
        .output()
        .expect("`dotnet --list-sdks` — the C# oracle needs the SDK");
    let listing = String::from_utf8_lossy(&output.stdout);
    let major = listing
        .lines()
        .filter_map(|line| line.split('.').next()?.trim().parse::<u32>().ok())
        .max()
        .unwrap_or_else(|| panic!("no SDK version in `dotnet --list-sdks`:\n{listing}"));
    format!("net{major}.0")
}

fn write_rust(root: &Path) {
    // The empty `[workspace]` table stops cargo walking up out of the temporary
    // directory looking for a parent manifest.
    write(
        root,
        "Cargo.toml",
        "[package]\n\
         name = \"oracle\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         \n\
         [lib]\n\
         path = \"lib.rs\"\n\
         \n\
         [workspace]\n",
    );
    write(
        root,
        "lib.rs",
        "pub mod walker;\n\
         \n\
         /// Declared here, and called from exactly one other place.\n\
         pub fn try_get_elements(source: &str) -> Option<usize> {\n    \
         if source.is_empty() {\n        \
         None\n    \
         } else {\n        \
         Some(source.len())\n    \
         }\n\
         }\n",
    );
    write(
        root,
        "walker.rs",
        "pub fn walk(source: &str) -> usize {\n    \
         match crate::try_get_elements(source) {\n        \
         Some(count) => count,\n        \
         None => 0,\n    \
         }\n\
         }\n",
    );
}

fn write_typescript(root: &Path) {
    write(
        root,
        "package.json",
        "{\n  \"name\": \"oracle\",\n  \"version\": \"0.0.0\",\n  \"private\": true\n}\n",
    );
    write(
        root,
        "tsconfig.json",
        "{\n  \"compilerOptions\": {\n    \
         \"target\": \"ES2020\",\n    \
         \"module\": \"CommonJS\",\n    \
         \"strict\": true\n  \
         },\n  \
         \"include\": [\"*.ts\"]\n\
         }\n",
    );
    write(
        root,
        "collections.ts",
        "export function tryGetElements(source: string): number | undefined {\n  \
         return source.length > 0 ? source.length : undefined;\n\
         }\n",
    );
    // A namespace import rather than a named one: `import { tryGetElements }`
    // is itself a reference, and the count under test is one.
    write(
        root,
        "walker.ts",
        "import * as collections from \"./collections\";\n\
         \n\
         export function walk(source: string): number {\n  \
         const count = collections.tryGetElements(source);\n  \
         return count ?? 0;\n\
         }\n",
    );
}

fn write_python(root: &Path) {
    write(root, "pyrightconfig.json", "{\n  \"include\": [\".\"]\n}\n");
    write(
        root,
        "collections_mod.py",
        "def try_get_elements(source: str):\n    \
         if source:\n        \
         return len(source)\n    \
         return None\n",
    );
    // A module import rather than `from collections_mod import try_get_elements`,
    // for the same reason the TypeScript fixture uses a namespace import.
    write(
        root,
        "walker.py",
        "import collections_mod\n\
         \n\
         \n\
         def walk(source: str) -> int:\n    \
         count = collections_mod.try_get_elements(source)\n    \
         return count if count is not None else 0\n",
    );
}

// ---------------------------------------------------------------------------
// What holds the oracles to their fixtures
// ---------------------------------------------------------------------------

/// Not `#[ignore]`d, and needs no server: it runs on every `cargo test`.
///
/// Each oracle asserts a line number and a count of one. Both are claims about
/// the fixture, and a fixture edit that invalidated them would otherwise turn
/// the oracle into a test of something else — or, worse, leave it passing
/// against a symbol it was never meant to be about.
#[test]
fn fixtures_say_what_the_oracles_assert() {
    for oracle in ALL {
        let dir = fixture_root();
        (oracle.write_fixture)(dir.path());

        let decl = read(&dir.path().join(oracle.decl_file));
        let caller = read(&dir.path().join(oracle.caller_file));

        // Panics naming the line if either is wrong.
        column_of(&decl, oracle.decl_line, oracle.symbol);
        column_of(&caller, oracle.caller_line, oracle.symbol);

        assert_ne!(
            oracle.decl_file, oracle.caller_file,
            "{}: the usage must be in a different file, or the oracle proves \
             nothing about cross-file resolution",
            oracle.language
        );

        // The count of one is only the truth if the symbol is written exactly
        // twice in the whole project: once declared, once called. A stray third
        // occurrence — an import, a doc comment, a second call — makes
        // `total == Some(1)` a wrong assertion that the server would be right to
        // fail.
        let total: usize = files_under(dir.path())
            .iter()
            .map(|path| read(path).matches(oracle.symbol).count())
            .sum();
        assert_eq!(
            total, 2,
            "{}: {} is written {total} times across the fixture; it must be \
             written exactly twice (declared once, called once)",
            oracle.language, oracle.symbol
        );
    }
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found
}
