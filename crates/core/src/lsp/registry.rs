//! Which server serves which file, where it is, and what to say when it is absent.
//!
//! # "Not found" is the common path, not the edge case
//!
//! On the machine this was written on, exactly one of the four servers is
//! installed. `typescript-language-server` is absent, no Python server is
//! present (only `ruff`, which is a linter), and there is no OmniSharp. So the
//! interesting return value of [`resolve`] is not [`Resolution::Found`] — it is
//! [`Resolution::NotFound`], and it has to carry enough for the UI to say *what
//! was looked for* and *how to get one*. A bare "unavailable" would send the
//! user to search the repository for a spelling of a program name that this file
//! already knows.
//!
//! That is why the four outcomes are separate variants rather than an
//! `Option<ServerSpec>` plus a log line. Disabled, misconfigured, absent and
//! present are four different things to tell somebody, exactly as
//! [`crate::lsp`]'s abstain rule requires of everything downstream.
//!
//! # Why the environment is a trait
//!
//! Discovery is *all* environment: PATH, a home directory, a marketplace
//! directory naming convention, an environment variable. Tested against the real
//! machine it would assert whatever happens to be installed today, and the two
//! cases that matter most — nothing installed, and two versions installed where
//! the wrong one sorts higher — cannot be arranged on a developer's box at all.
//! So [`Probe`] is injected, [`RealProbe`] is the only impl that touches the
//! world, and every rule below is decided by a headless test.
//!
//! # Where this abstains
//!
//! * An unknown extension yields `None` from [`language_for_extension`] and
//!   **never** a default server. Asking a C# server about a `.csproj` and
//!   rendering its answer is worse than showing nothing.
//! * A [`ServerOverride::program`] that does not resolve is
//!   [`Resolution::Misconfigured`] and never falls through to discovery. See
//!   [`crate::lsp::settings`]'s module doc: the user pinned a specific server,
//!   very often to match a project's SDK, and answers attributed to a server
//!   that never ran are the worst outcome available here.
//! * An extension directory whose name does not parse as a version is ranked
//!   *last* rather than assumed newest — and still used if it is all there is,
//!   because refusing a server that really works over a directory name would be
//!   an abstention that costs the feature entirely.
//! * Every candidate that was tried and failed is named in
//!   [`Resolution::NotFound::looked_for`], including the home directory being
//!   unknown, which is a different problem from "nothing installed".

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::lsp::settings::{LspConfig, ServerOverride};
use crate::lsp::uri::UriStyle;

// ---------------------------------------------------------------------------
// The injected environment
// ---------------------------------------------------------------------------

/// Everything discovery needs to know about the machine.
///
/// Object-safe on purpose: callers hold a `&dyn Probe` so a test can pass a
/// map-backed fake and production can pass [`RealProbe`] without either being
/// generic over the other.
pub trait Probe {
    /// A program on PATH, already through
    /// [`crate::process::resolve_program`] — so a `.cmd` shim resolves, which
    /// is how npm installs `typescript-language-server` and
    /// `pyright-langserver` on Windows. `None` means it is not there.
    fn on_path(&self, name: &str) -> Option<PathBuf>;
    fn is_file(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    /// Immediate children of a directory, empty when it cannot be read.
    ///
    /// Empty rather than an error: an unreadable extensions directory and an
    /// empty one lead to the same answer here, and the caller reports what it
    /// looked for either way.
    fn read_dir(&self, path: &Path) -> Vec<PathBuf>;
    /// The user's home directory, if it can be determined.
    fn home(&self) -> Option<PathBuf>;
    /// An environment variable, so `CB_ROSLYN_PATH` is testable.
    fn env(&self, key: &str) -> Option<String>;
}

/// The real machine.
pub struct RealProbe;

impl Probe for RealProbe {
    fn on_path(&self, name: &str) -> Option<PathBuf> {
        // `resolve_program` is the PATHEXT walk and is deliberately an identity
        // function when it finds nothing (so a spawn error still names what was
        // asked for) — and on non-Windows it is an identity function always. So
        // its result is a candidate, not an answer, and PATH still has to be
        // walked for the plain-name case.
        let resolved = crate::process::resolve_program(name);
        if resolved.is_file() {
            return Some(resolved);
        }
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn read_dir(&self, path: &Path) -> Vec<PathBuf> {
        match std::fs::read_dir(path) {
            Ok(entries) => entries.flatten().map(|e| e.path()).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn home(&self) -> Option<PathBuf> {
        // Following `secrets::secrets_path`'s precedent rather than adding a
        // second way to find a user directory: `%APPDATA%` on Windows, `$HOME`
        // elsewhere. `%APPDATA%` is `<home>/AppData/Roaming`, and the VS Code
        // extension directories live at `<home>/.vscode`, so two components come
        // off — done with `parent()` rather than by string surgery so a
        // relocated profile still works.
        if cfg!(windows) {
            let appdata = PathBuf::from(std::env::var("APPDATA").ok()?);
            return appdata.parent()?.parent().map(Path::to_path_buf);
        }
        std::env::var("HOME").ok().map(PathBuf::from)
    }

    fn env(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

// ---------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------

/// A language this app can ask a server about.
///
/// Not `serde`-derived: these values reach the configuration file as the string
/// ids in [`Language::id`], and a rename of the Rust variant must not be able to
/// invalidate everyone's `config.json`. Same reasoning as
/// [`crate::lsp::settings::UriStyleSetting`] mirroring [`UriStyle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Language {
    CSharp,
    TypeScript,
    Rust,
    Python,
}

impl Language {
    /// The server id, and the key [`LspConfig::servers`] is looked up by.
    ///
    /// Pinned by a test: `settings.rs` documents these four spellings, so a
    /// change here would silently ignore configuration files rather than fail.
    pub fn id(self) -> &'static str {
        match self {
            Language::CSharp => "csharp",
            Language::TypeScript => "typescript",
            Language::Rust => "rust",
            Language::Python => "python",
        }
    }
}

/// When a server's answers can be trusted.
///
/// Three states rather than "the process is up", because two of these servers
/// answer questions before they are able to answer them *correctly*, and a low
/// usage count is a wrong answer rather than a partial one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    /// Trustworthy as soon as `initialize` returns.
    Immediate,
    /// Trustworthy once **one** of the server's own progress tokens with this
    /// prefix has ended. rust-analyzer reports cache priming this way and will
    /// happily answer `references` from a half-built index in the meantime.
    ///
    /// The first match wins and is recorded permanently — see
    /// [`super::client::signals_ready`] and the transport's signal sink. So this
    /// is trustworthy exactly to the extent that the *first* `end` under the
    /// prefix is the one that matters; if a server emits several such tokens for
    /// separate pieces of work, readiness is published at the first.
    ///
    /// **That was observed, and the prefix was narrowed.** This used to read
    /// `rustAnalyzer`, on the assumption that the namespace was the signal. A
    /// capture against rust-analyzer 1.94 (`tests/lsp_oracle.rs`) shows it is
    /// not — `Fetching`, `Building CrateGraph`, `Roots Scanned`,
    /// `Loading proc-macros` and `Building compile-time-deps` all end first, and
    /// `Fetching` ends *more than once*, so even "the last one" would not have
    /// worked. The token that means the index is usable is
    /// `rustAnalyzer/cachePriming` — spelled unlike the "Indexing" title it
    /// displays under, which is why guessing it from the UI got it wrong. The
    /// old prefix published `Ready`, with no caveat, about ten seconds early.
    ///
    /// So: this must name **one specific unit of work**, never a namespace. A
    /// prefix broad enough to match a sibling token is a wrong answer delivered
    /// confidently, which is worse than the late one it was trying to avoid.
    Progress { token_prefix: &'static str },
    /// Trustworthy once the `$/progress` whose **`begin` carried this title**
    /// has ended.
    ///
    /// For servers that mint an opaque token per unit of work, where
    /// [`Self::Progress`] has nothing to match on.
    /// typescript-language-server uses a fresh UUID
    /// (`41f6b03a-c261-42d6-b5a6-676ce8b664b1`) and puts the only identifying
    /// information in the `begin` value's `title` — "Initializing JS/TS language
    /// features…" — while the `end` carries the token and nothing else. So the
    /// token has to be remembered at `begin` and recognised at `end`, which is
    /// why this one is matched by a stateful filter
    /// ([`super::client::readiness_filter`]) rather than by a pure function.
    ///
    /// The title is matched by prefix so a trailing ellipsis, a count or a
    /// percentage cannot invalidate it. Comparing whole titles would make the
    /// signal a cosmetic string the server is free to change.
    ProgressTitle { title_prefix: &'static str },
    /// Trustworthy once the server has published diagnostics for anything.
    ///
    /// The last resort, for a server that announces its start-up in no other
    /// way. basedpyright 1.39.10 sends no progress and no custom notification
    /// while it scans the workspace; it simply answers `references` with `[]`
    /// for about six hundred milliseconds and then starts answering correctly.
    /// Captured (`tests/lsp_oracle.rs`):
    ///
    /// ```text
    /// 0.7s  initialize result
    /// 1.3s  textDocument/references -> []              <- a wrong zero
    /// 1.3s  window/logMessage "Found 2 source files"
    /// 1.3s  textDocument/publishDiagnostics            <- and correct from here on
    /// 1.3s  textDocument/references -> the real call site
    /// ```
    ///
    /// The temptation is the log line, which states the truth in English.
    /// **Prose is never read for meaning in this subsystem**, and a message
    /// the server is free to reword is not a protocol signal. Diagnostics are:
    /// the server publishes them once it has analysed the file, which it cannot
    /// do before resolving the file's imports, which is the very work that makes
    /// `references` correct. Published even when there are none, so a clean file
    /// still produces one.
    ///
    /// A server that never publishes any leaves this waiting until the readiness
    /// ceiling, and is then promoted **with the caveat** — late and honest,
    /// which is the trade this whole enum exists to make.
    FirstDiagnostics,
    /// Roslyn: projects load asynchronously after `initialize`, and until they
    /// have, a reference query returns what it can see so far. Named for the
    /// server rather than described generically because the signal is
    /// server-specific and the transport layer has to special-case it anyway.
    RoslynProjectInit,
}

/// How long to wait, per kind of wait.
///
/// Separate numbers because the first request pays for project load or cache
/// priming and the rest do not; one shared timeout would either be far too long
/// for a steady-state request (so a dead server looks slow) or far too short for
/// the first (so a healthy server looks dead).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    pub first_request: Duration,
    pub request: Duration,
    pub document_symbol: Duration,
}

/// Everything needed to start one server and interpret it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSpec {
    pub id: &'static str,
    pub language: Language,
    /// The LSP `languageId` for `textDocument/didOpen`.
    ///
    /// One per language rather than per extension. `tsx` is really
    /// `typescriptreact` to tsserver; that refinement belongs where a *file* is
    /// opened, and is deliberately not invented here where only the language is
    /// known.
    pub language_id: &'static str,
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Layered over the inherited environment, the way
    /// [`crate::process`] layers its colour defaults.
    pub env: BTreeMap<String, String>,
    pub uri_style: UriStyle,
    pub readiness: Readiness,
    pub timeouts: Timeouts,
}

/// What resolving a language produced. Four different things to tell the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Boxed because this variant is much larger than the other three and
    /// clippy's `large_enum_variant` is right that a `Resolution` is mostly
    /// returned in one of the small shapes.
    Found(Box<ServerSpec>),
    /// Nothing was found. `looked_for` names every candidate tried, `hint` says
    /// how to get one.
    NotFound {
        language: Language,
        looked_for: Vec<String>,
        hint: String,
    },
    /// The workspace config turned this language off.
    Disabled { language: Language },
    /// The config named a program explicitly and it is not there.
    Misconfigured {
        language: Language,
        program: String,
        detail: String,
    },
}

// ---------------------------------------------------------------------------
// Extensions
// ---------------------------------------------------------------------------

/// The language of a file extension (no leading dot), or nothing.
///
/// Case-insensitive because Windows preserves whatever case a file was created
/// with. `None` is a real answer and must not be turned into a default server —
/// see the module doc.
pub fn language_for_extension(extension: &str) -> Option<Language> {
    // Allocation-free comparison, matching how `symbols::index` treats its
    // extension allowlist: the list is short and this is called per file.
    let matches =
        |candidates: &[&str]| candidates.iter().any(|c| extension.eq_ignore_ascii_case(c));

    if matches(&["cs"]) {
        return Some(Language::CSharp);
    }
    if matches(&["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"]) {
        return Some(Language::TypeScript);
    }
    if matches(&["rs"]) {
        return Some(Language::Rust);
    }
    if matches(&["py", "pyi"]) {
        return Some(Language::Python);
    }
    None
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// Decide which server serves `language`, or say why none does.
pub fn resolve(language: Language, config: Option<&LspConfig>, probe: &dyn Probe) -> Resolution {
    let over = config.and_then(|c| c.server(language.id()));

    // Order matters and is the order of authority: a refusal beats a program,
    // a program beats discovery, and discovery beats nothing.
    if over.is_some_and(ServerOverride::is_disabled) {
        return Resolution::Disabled { language };
    }

    if let Some(program) = over.and_then(|o| o.program.as_deref()) {
        return match locate_explicit(program, probe) {
            Some(path) => {
                let args = default_args(language, &path);
                found(language, path, args, over)
            }
            None => Resolution::Misconfigured {
                language,
                program: program.to_string(),
                detail: format!(
                    "`lsp.servers.{}.program` in .code-basics/config.json names `{}`, \
                     which is not an executable file and was not found on PATH. \
                     Discovery is deliberately not attempted: this app will not start a \
                     different server than the one you configured.",
                    language.id(),
                    program.trim()
                ),
            },
        };
    }

    let mut looked_for = Vec::new();
    match discover(language, probe, &mut looked_for) {
        Some((program, args)) => found(language, program, args, over),
        None => Resolution::NotFound {
            language,
            looked_for,
            hint: hint(language).to_string(),
        },
    }
}

/// Build a spec and layer the override's non-program fields over it.
///
/// Layering rather than replacing the whole command line so a user can add a
/// flag without restating arguments they never wanted to change — except
/// `args`, which `settings.rs` documents as a full replacement because removing
/// a default argument has no other spelling.
fn found(
    language: Language,
    program: PathBuf,
    args: Vec<String>,
    over: Option<&ServerOverride>,
) -> Resolution {
    let readiness = default_readiness(language, &program);
    let mut spec = ServerSpec {
        id: language.id(),
        language,
        language_id: language.id(),
        program,
        args,
        env: default_env(language),
        uri_style: default_uri_style(language),
        readiness,
        timeouts: default_timeouts(language),
    };

    if let Some(over) = over {
        if let Some(args) = &over.args {
            spec.args = args.clone();
        }
        // Layered, not replaced: a user adding one variable must not silently
        // drop `DOTNET_gcServer`, and naming it must win.
        for (key, value) in &over.env {
            spec.env.insert(key.clone(), value.clone());
        }
        if let Some(style) = over.uri_style {
            spec.uri_style = style.style();
        }
    }

    Resolution::Found(Box::new(spec))
}

/// Resolve a configured `program` and nothing else.
///
/// A blank string is a mistake in a hand-written file, not "unset": treating it
/// as unset would fall through to discovery, which is exactly the quiet
/// substitution this module refuses.
fn locate_explicit(program: &str, probe: &dyn Probe) -> Option<PathBuf> {
    let program = program.trim();
    if program.is_empty() {
        return None;
    }
    if program.contains('/') || program.contains('\\') {
        let path = PathBuf::from(program);
        // `is_file`, not `exists`: a directory here is a configuration mistake
        // and spawning it would fail with an error nobody can act on.
        return probe.is_file(&path).then_some(path);
    }
    probe.on_path(program)
}

// ---------------------------------------------------------------------------
// Per-language discovery
// ---------------------------------------------------------------------------

/// The built-in search for one language, appending every candidate tried.
fn discover(
    language: Language,
    probe: &dyn Probe,
    looked_for: &mut Vec<String>,
) -> Option<(PathBuf, Vec<String>)> {
    match language {
        Language::CSharp => discover_roslyn(probe, looked_for),
        language => on_path_candidates(probe, looked_for, candidates(language)),
    }
}

/// One program this app knows how to launch, and how to launch it.
///
/// Both fields are properties of the **program**, not of the language it serves.
/// That is not a design preference, it is what running them showed: the four
/// Python servers below disagree about their arguments *and* about how they
/// announce that they are ready.
pub struct Candidate {
    pub name: &'static str,
    pub args: &'static [&'static str],
    pub readiness: Readiness,
}

/// Every server this app knows how to launch for a language, in preference
/// order.
///
/// **One table, two readers.** Discovery walks it, and so do [`default_args`]
/// and [`default_readiness`] on the configured-`program` path — where discovery
/// never runs and so never chose between the candidates. A second list would let
/// the two disagree, and it did: `--stdio` was handed to whatever program a user
/// named, and two of the four Python servers exit 2 on it.
///
/// C# is absent because Roslyn is not on PATH and is found by a filesystem
/// search instead; its arguments are [`ROSLYN_ARGS`].
fn candidates(language: Language) -> &'static [Candidate] {
    match language {
        // Reached only through the lookups; `discover` sends C# elsewhere.
        Language::CSharp => &[],
        Language::Rust => &[Candidate {
            name: "rust-analyzer",
            args: &[],
            readiness: Readiness::Progress {
                token_prefix: "rustAnalyzer/cachePriming",
            },
        }],
        Language::TypeScript => &[Candidate {
            name: "typescript-language-server",
            args: &["--stdio"],
            readiness: Readiness::ProgressTitle {
                title_prefix: "Initializing JS/TS language features",
            },
        }],
        // Order is preference. **Three** Python servers are deliberately absent,
        // for one reason: each would connect successfully and then give a count
        // that is wrong, which is the only outcome this subsystem treats as
        // worse than having no server at all.
        //
        // * `ruff` is a linter with no `references` and no `definition`. It
        //   would answer nothing, which reads as "unused".
        // * `pylsp` was run against the two-file fixture in `tests/lsp_oracle.rs`
        //   and answered `references` with **zero** — the same wrong zero, from a
        //   server that starts, initialises and reports itself ready.
        // * `jedi-language-server` answered **two** for the same fixture: it
        //   ignores `includeDeclaration: false` and returns the declaration
        //   alongside the one real call site. Every count it gives is one too
        //   high, so "1 usage" would appear above a method nothing calls. It
        //   also publishes no diagnostics at all, so there is no signal to wait
        //   on either.
        //
        // Both were reachable until they were run. Removing them costs a user
        // who has only one of them the feature — and [`Resolution::NotFound`]
        // then names basedpyright and how to install it, which is a far better
        // outcome than a number they would act on.
        Language::Python => &[
            Candidate {
                name: "basedpyright-langserver",
                args: &["--stdio"],
                readiness: Readiness::FirstDiagnostics,
            },
            Candidate {
                name: "pyright-langserver",
                args: &["--stdio"],
                readiness: Readiness::FirstDiagnostics,
            },
        ],
    }
}

/// The candidate a path names, matched on the file stem.
///
/// So `C:/py/pylsp.exe`, `pylsp.cmd` and a bare `pylsp` all resolve alike — the
/// three spellings a user may reasonably write for the same program. `None` for
/// a wrapper script or a build from source, which nothing here can recognise.
fn candidate_for(language: Language, program: &Path) -> Option<&'static Candidate> {
    let stem = program.file_stem().and_then(|stem| stem.to_str())?;
    candidates(language)
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(stem))
}

/// Try each name on PATH in order, recording all of them.
///
/// All of them, not just the ones tried before the hit: `looked_for` is only
/// read when nothing was found, and a partial list would understate what is
/// supported.
fn on_path_candidates(
    probe: &dyn Probe,
    looked_for: &mut Vec<String>,
    candidates: &[Candidate],
) -> Option<(PathBuf, Vec<String>)> {
    let mut hit = None;
    for candidate in candidates {
        looked_for.push(format!("{} (on PATH)", candidate.name));
        if hit.is_none() {
            if let Some(path) = probe.on_path(candidate.name) {
                hit = Some((path, candidate.args.iter().map(|a| a.to_string()).collect()));
            }
        }
    }
    hit
}

/// The environment variable that overrides Roslyn discovery.
const ROSLYN_ENV: &str = "CB_ROSLYN_PATH";

/// The marketplace directory prefix of the **MIT-licensed** C# extension.
///
/// `ms-dotnettools.csdevkit` is a different, proprietary extension and is never
/// reached into; the prefix ends with `csharp-` so it cannot match.
const CSHARP_EXTENSION_PREFIX: &str = "ms-dotnettools.csharp-";

/// The editor extension roots, in the order they are preferred.
///
/// Directory order deciding overall, version deciding within a directory, is the
/// rule [`crate::process::resolve_program`] already applies to PATH. The
/// alternative — highest version across all editors — would launch a
/// Windsurf-installed server for somebody working in VS Code, which is
/// unpredictable in a way a user cannot see.
const EDITOR_DIRS: &[&str] = &[
    ".vscode",
    ".vscode-insiders",
    ".vscode-server",
    ".cursor",
    ".windsurf",
];

/// Both spellings of the server executable: Windows and everything else.
const ROSLYN_EXE_NAMES: &[&str] = &[
    "Microsoft.CodeAnalysis.LanguageServer.exe",
    "Microsoft.CodeAnalysis.LanguageServer",
];

/// The arguments verified against the real binary's `--help` this session.
///
/// Exactly these three and no more. The VS Code extension additionally passes
/// `--razorSourceGenerator`, `--razorDesignTimePath`, `--csharpDesignTimePath`
/// and `--devKitDependencyPath`, and it is **unverified** whether the server
/// requires any of them — a probe run with only these three completed
/// `initialize`, `didOpen` and a correct cross-file `references`. Starting
/// minimal and adding on an observed failure is the honest order; passing paths
/// we have not confirmed exist would be a guess in the one subsystem that must
/// not guess.
///
/// `--clientProcessId <pid>` and `--extensionLogDirectory <dir>` are **not**
/// here because neither is known at this point; [`caller_args`] builds them and
/// [`super::client::Client::start_with_ceiling`] appends them to this list at the
/// spawn site, where our pid and the workspace root both are known. They are not
/// defaulted here as literals: a fabricated pid would make the server exit when
/// a process it never launched dies.
const ROSLYN_ARGS: &[&str] = &["--stdio", "--logLevel", "Warning", "--autoLoadProjects"];

/// The arguments only the spawn site can supply, appended to [`ROSLYN_ARGS`].
///
/// * `--clientProcessId` makes the server exit when *this* process does. Closing
///   the child's stdin already ends it on an orderly shutdown — verified: killing
///   the app left no server behind — so this covers the case that path does not,
///   the app dying without reaching `shutdown`.
/// * `--extensionLogDirectory` is where the server writes its own log. Without it
///   there is no log at all, which is how an hour went into diagnosing a server
///   that had already said what was wrong.
///
/// Gated on the Roslyn command line and nothing looser. Two conditions, both
/// necessary: the language is C# (these are Roslyn's spellings and no other
/// server accepts them — a flag another server rejects means it does not start
/// at all), and the arguments are still the ones this module chose, so a user who
/// replaced `args` — which `settings.rs` documents as a full replacement — gets
/// exactly what they wrote. `log_dir` of `None` appends neither the flag nor a
/// path: a flag pointing at a directory that could not be created is worse than
/// no flag.
pub fn caller_args(spec: &ServerSpec, pid: u32, log_dir: Option<&Path>) -> Vec<String> {
    if !takes_caller_args(spec) {
        return Vec::new();
    }

    let mut args = vec!["--clientProcessId".to_string(), pid.to_string()];
    if let Some(dir) = log_dir {
        // Lossy rather than refused: a path that is not valid UTF-16-to-UTF-8
        // round-trippable still names a directory the server can probably use,
        // and losing the logs is the only thing at risk if it cannot.
        args.push("--extensionLogDirectory".to_string());
        args.push(dir.to_string_lossy().to_string());
    }
    args
}

/// Whether [`caller_args`] will contribute anything for this spec.
///
/// The gate itself, exposed so the spawn site can ask *before* creating a log
/// directory. Without it every workspace grew a `.code-basics/lsp-logs/` for
/// TypeScript and Rust servers too, whose command lines take neither flag — a
/// directory that is created on every start and can never receive a file.
pub fn takes_caller_args(spec: &ServerSpec) -> bool {
    let ours = spec
        .args
        .iter()
        .map(String::as_str)
        .eq(ROSLYN_ARGS.iter().copied());
    spec.language == Language::CSharp && ours
}

fn discover_roslyn(
    probe: &dyn Probe,
    looked_for: &mut Vec<String>,
) -> Option<(PathBuf, Vec<String>)> {
    let args = || ROSLYN_ARGS.iter().map(|a| a.to_string()).collect();

    // 1. The environment override, mirroring how `CB_INSPECTOR_PATH` is
    //    documented: a directory means "the publish output", a file means that
    //    exact binary under whatever name it has.
    match probe.env(ROSLYN_ENV) {
        Some(raw) if !raw.trim().is_empty() => {
            let path = PathBuf::from(raw.trim());
            looked_for.push(format!("{ROSLYN_ENV}={}", path.display()));
            if probe.is_dir(&path) {
                for name in ROSLYN_EXE_NAMES {
                    let candidate = path.join(name);
                    if probe.is_file(&candidate) {
                        return Some((candidate, args()));
                    }
                }
            } else if probe.is_file(&path) {
                return Some((path, args()));
            }
            // Falls through on purpose. Unlike a configured `program`, an
            // environment variable is a developer convenience that outlives the
            // build it was set for, and a stale one left in a shell profile
            // should not disable C# entirely. It stays in `looked_for` so the
            // fall-through is visible rather than silent.
        }
        _ => looked_for.push(format!("{ROSLYN_ENV} (not set)")),
    }

    // 2. The C# extension's bundled copy, per editor.
    let Some(home) = probe.home() else {
        // A different problem from "nothing installed", and the only one whose
        // fix is to set the environment variable, so it must not be reported as
        // the same thing.
        looked_for.push(format!(
            "the user's home directory could not be determined, so no editor \
             extension directory could be searched — set {ROSLYN_ENV}"
        ));
        return None;
    };

    for editor in EDITOR_DIRS {
        let extensions = home.join(editor).join("extensions");
        looked_for.push(format!(
            "{}",
            extensions
                .join(format!("{CSHARP_EXTENSION_PREFIX}*"))
                .join(".roslyn")
                .join(ROSLYN_EXE_NAMES[0])
                .display()
        ));
        if let Some(exe) = best_roslyn_in(&extensions, probe, looked_for) {
            return Some((exe, args()));
        }
    }

    None
}

/// The newest usable server under one editor's extensions directory.
fn best_roslyn_in(
    extensions: &Path,
    probe: &dyn Probe,
    looked_for: &mut Vec<String>,
) -> Option<PathBuf> {
    let mut children = probe.read_dir(extensions);
    // `read_dir` order is arbitrary on a real filesystem, and two directories
    // whose versions both fail to parse would otherwise pick differently run to
    // run — which is the sort of thing that makes a bug unreproducible.
    children.sort();

    let mut best: Option<(Option<Vec<u64>>, PathBuf)> = None;
    for child in children {
        let Some(name) = child.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(CSHARP_EXTENSION_PREFIX) {
            continue;
        }
        let roslyn = child.join(".roslyn");
        let Some(exe) = ROSLYN_EXE_NAMES
            .iter()
            .map(|n| roslyn.join(n))
            .find(|candidate| probe.is_file(candidate))
        else {
            // A half-installed extension. Reported rather than skipped
            // silently: otherwise the user is told nothing was found while
            // looking straight at the directory.
            looked_for.push(format!(
                "{} exists but contains no server executable",
                roslyn.display()
            ));
            continue;
        };

        let version = parse_extension_version(name);
        // Strictly greater, so the first of equal candidates wins and the
        // sorted order above decides ties deterministically. `None` compares
        // below every `Some`, which is what ranks an unreadable directory name
        // last without refusing it outright.
        if best.as_ref().is_none_or(|(seen, _)| version > *seen) {
            best = Some((version, exe));
        }
    }
    best.map(|(_, exe)| exe)
}

/// The version components of `ms-dotnettools.csharp-2.140.9-win32-x64`.
///
/// Compared as numbers, never as a string: lexically `"2.9.0"` sorts above
/// `"2.140.9"`, so a sort by name picks a year-old extension over the current
/// one — the specific bug this exists to avoid. Hand-rolled because no version
/// crate is a dependency of this workspace and the grammar needed here is one
/// line of it.
///
/// `None` for anything that does not parse, including a component too large for
/// a `u64`: a marketplace directory name is not a contract, and a name we cannot
/// read must not be ranked as though we could.
fn parse_extension_version(dir_name: &str) -> Option<Vec<u64>> {
    let rest = dir_name.strip_prefix(CSHARP_EXTENSION_PREFIX)?;
    // The platform triple (`-win32-x64`) and any pre-release suffix follow the
    // first `-`; only the dotted numeric part before it is a version.
    let numeric = rest.split('-').next()?;
    let mut parts = Vec::new();
    for component in numeric.split('.') {
        parts.push(component.parse::<u64>().ok()?);
    }
    (!parts.is_empty()).then_some(parts)
}

// ---------------------------------------------------------------------------
// Per-language defaults
// ---------------------------------------------------------------------------

/// The arguments for a server the user named, rather than one discovery chose.
///
/// Looked up by the program's own name against [`candidates`] first, because
/// what a server takes is a property of that server. This used to be decided by
/// language alone, which handed `--stdio` to every Python server — and two of
/// the four exit immediately with `unrecognized arguments: --stdio`. Since
/// discovery does not run once a `program` is configured, nothing else was left
/// to know better.
///
/// The name is matched on the file stem, so `C:/py/pylsp.exe`, `pylsp.cmd` and a
/// bare `pylsp` all resolve alike — the three spellings a user may reasonably
/// write for the same program.
///
/// A program no entry recognises — a wrapper script, a build from source — falls
/// back to the language's usual arguments. There is genuinely nothing better to
/// go on, and `"args": []` remains the documented escape hatch.
fn default_args(language: Language, program: &Path) -> Vec<String> {
    if let Some(candidate) = candidate_for(language, program) {
        return candidate.args.iter().map(|a| a.to_string()).collect();
    }

    let args: &[&str] = match language {
        Language::CSharp => ROSLYN_ARGS,
        Language::Rust => &[],
        Language::TypeScript | Language::Python => &["--stdio"],
    };
    args.iter().map(|a| a.to_string()).collect()
}

fn default_env(language: Language) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    if language == Language::CSharp {
        // Server GC reserves a heap per core, which on a developer machine is
        // hundreds of megabytes for a process that sits idle most of the time.
        // The VS Code extension sets the same thing.
        env.insert("DOTNET_gcServer".to_string(), "0".to_string());
    }
    env
}

fn default_uri_style(language: Language) -> UriStyle {
    match language {
        // Verified against the real server this session: it accepts and emits a
        // plain colon (`file:///C:/...`). See `lsp::uri`.
        Language::CSharp => UriStyle::Plain,
        // rust-analyzer emits `%3A`, and the other two are `vscode-uri`-shaped
        // libraries that do the same.
        _ => UriStyle::Encoded,
    }
}

/// How this program announces that its answers can be trusted.
///
/// Looked up by program name first, for the same reason [`default_args`] is: the
/// four Python servers announce readiness in two different ways, and one of them
/// does not announce it at all. Deciding this by language alone left
/// `jedi-language-server` — which is ready almost at once — sitting on "loading"
/// until the ninety-second ceiling.
///
/// The fallback is the language's usual signal, for a program nothing here
/// recognises.
fn default_readiness(language: Language, program: &Path) -> Readiness {
    if let Some(candidate) = candidate_for(language, program) {
        return candidate.readiness;
    }

    match language {
        Language::CSharp => Readiness::RoslynProjectInit,
        // One specific unit of work, not the `rustAnalyzer` namespace — see
        // [`Readiness::Progress`] for the capture that forced the distinction.
        Language::Rust => Readiness::Progress {
            token_prefix: "rustAnalyzer/cachePriming",
        },
        // Not `Immediate`, which is what this used to say. Captured from
        // typescript-language-server 5.3.0 over TypeScript 5.9.3: `initialize`
        // returns in 0.2s, the project is not loaded for another second, and
        // `references` in that window answers `[]` — a wrong zero delivered
        // faster than the right answer. The work is announced as
        // "Initializing JS/TS language features…" under a UUID token, which is
        // why this needs the title and not a prefix.
        Language::TypeScript => Readiness::ProgressTitle {
            title_prefix: "Initializing JS/TS language features",
        },
        // Not `Immediate`: see [`Readiness::FirstDiagnostics`] for the capture
        // showing basedpyright answering `[]` for the first ~600ms.
        Language::Python => Readiness::FirstDiagnostics,
    }
}

fn default_timeouts(language: Language) -> Timeouts {
    match language {
        // Roslyn's first answer waits for project load, which on this
        // repository's own solution took ~20 s in the probe and scales with the
        // number of projects. Generous here costs a slow first query; mean here
        // costs a feature that reports a healthy server as dead.
        Language::CSharp => Timeouts {
            first_request: Duration::from_secs(180),
            request: Duration::from_secs(20),
            document_symbol: Duration::from_secs(15),
        },
        // rust-analyzer's first answer waits for `cargo metadata` plus indexing.
        Language::Rust => Timeouts {
            first_request: Duration::from_secs(120),
            request: Duration::from_secs(20),
            document_symbol: Duration::from_secs(15),
        },
        Language::TypeScript | Language::Python => Timeouts {
            first_request: Duration::from_secs(60),
            request: Duration::from_secs(15),
            document_symbol: Duration::from_secs(10),
        },
    }
}

/// How to get a server that is not installed.
///
/// Part of the answer, not a log line: this is the whole difference between a
/// feature that is unavailable and a feature the user can turn on.
fn hint(language: Language) -> &'static str {
    match language {
        Language::CSharp => {
            "Install the C# extension for VS Code (`ms-dotnettools.csharp`), which \
             bundles Microsoft.CodeAnalysis.LanguageServer — or set CB_ROSLYN_PATH to a \
             directory containing it."
        }
        Language::Rust => {
            "Install it with `rustup component add rust-analyzer`, or set \
             `lsp.servers.rust.program` in .code-basics/config.json."
        }
        Language::TypeScript => {
            "Install it with `npm i -g typescript-language-server typescript`, or set \
             `lsp.servers.typescript.program` in .code-basics/config.json."
        }
        Language::Python => {
            "Install `basedpyright` or `pyright` (for example `pip install basedpyright`), \
             or set \
             `lsp.servers.python.program` in .code-basics/config.json. Note that `ruff` \
             is a linter and cannot answer usage or definition queries."
        }
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod registry_tests;
