use super::*;

use std::collections::{BTreeMap, BTreeSet};

// ---------------------------------------------------------------------------
// A fake environment
// ---------------------------------------------------------------------------

/// Paths are compared as forward-slashed strings.
///
/// `Path::join` produces a backslash on Windows and a slash everywhere else, so
/// a `PathBuf` key would make every one of these tests platform-dependent —
/// which is the same reason [`crate::lsp::uri`] decides drive letters on the
/// string rather than with `Path`.
fn norm(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[derive(Default)]
struct Fake {
    on_path: BTreeMap<String, String>,
    files: BTreeSet<String>,
    dirs: BTreeSet<String>,
    home: Option<String>,
    env: BTreeMap<String, String>,
}

impl Fake {
    fn new() -> Self {
        Self::default()
    }

    /// A program the PATH walk would resolve, and where to.
    fn program(mut self, name: &str, at: &str) -> Self {
        self.on_path.insert(name.to_string(), at.to_string());
        self = self.file(at);
        self
    }

    /// A file, plus every directory above it — a fake in which a file exists
    /// but its parent does not would not be reachable by any real code path.
    fn file(mut self, path: &str) -> Self {
        self.files.insert(path.to_string());
        let mut cursor = path;
        while let Some((parent, _)) = cursor.rsplit_once('/') {
            if parent.is_empty() {
                break;
            }
            self.dirs.insert(parent.to_string());
            cursor = parent;
        }
        self
    }

    /// A directory, plus every directory above it — otherwise a `read_dir` of
    /// the parent would not list it, and a fake in which a directory exists
    /// while its parent does not cannot occur on a real filesystem.
    fn dir(mut self, path: &str) -> Self {
        self.dirs.insert(path.to_string());
        let mut cursor = path;
        while let Some((parent, _)) = cursor.rsplit_once('/') {
            if parent.is_empty() {
                break;
            }
            self.dirs.insert(parent.to_string());
            cursor = parent;
        }
        self
    }

    fn home(mut self, path: &str) -> Self {
        self.home = Some(path.to_string());
        self
    }

    fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.to_string(), value.to_string());
        self
    }
}

impl Probe for Fake {
    fn on_path(&self, name: &str) -> Option<PathBuf> {
        self.on_path.get(name).map(PathBuf::from)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.contains(&norm(path))
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.dirs.contains(&norm(path))
    }

    fn read_dir(&self, path: &Path) -> Vec<PathBuf> {
        let prefix = format!("{}/", norm(path));
        self.dirs
            .iter()
            .chain(self.files.iter())
            .filter_map(|entry| {
                let rest = entry.strip_prefix(&prefix)?;
                (!rest.contains('/')).then(|| PathBuf::from(entry))
            })
            .collect()
    }

    fn home(&self) -> Option<PathBuf> {
        self.home.as_deref().map(PathBuf::from)
    }

    fn env(&self, key: &str) -> Option<String> {
        self.env.get(key).cloned()
    }
}

/// A probe that must never be consulted, so "did not look" is assertable
/// rather than merely plausible.
struct NeverProbe;

impl Probe for NeverProbe {
    fn on_path(&self, name: &str) -> Option<PathBuf> {
        panic!("PATH was searched for {name} when it should not have been");
    }
    fn is_file(&self, path: &Path) -> bool {
        panic!("the filesystem was consulted for {}", path.display());
    }
    fn is_dir(&self, path: &Path) -> bool {
        panic!("the filesystem was consulted for {}", path.display());
    }
    fn read_dir(&self, path: &Path) -> Vec<PathBuf> {
        panic!("the filesystem was listed at {}", path.display());
    }
    fn home(&self) -> Option<PathBuf> {
        panic!("the home directory was consulted");
    }
    fn env(&self, key: &str) -> Option<String> {
        panic!("the environment was read for {key}");
    }
}

/// Parse a settings block the way `config::load` would, so these tests exercise
/// the real deserialiser rather than a hand-built struct that might not be
/// expressible in the file.
fn config(json: &str) -> LspConfig {
    serde_json::from_str(json).expect("a hand-written block must load")
}

fn spec(resolution: Resolution) -> ServerSpec {
    match resolution {
        Resolution::Found(spec) => *spec,
        other => panic!("expected a server, got {other:?}"),
    }
}

/// The `looked_for` list with separators normalised.
///
/// These strings are shown to a human, so the implementation spells them the
/// platform's way — which would make every assertion below Windows-only.
fn not_found(resolution: Resolution) -> (Vec<String>, String) {
    match resolution {
        Resolution::NotFound {
            looked_for, hint, ..
        } => (
            looked_for
                .iter()
                .map(|c| c.replace('\\', "/"))
                .collect::<Vec<_>>(),
            hint,
        ),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

/// The exe under a plausible VS Code extension directory.
fn roslyn_at(home: &str, editor: &str, version: &str) -> String {
    format!(
        "{home}/{editor}/extensions/ms-dotnettools.csharp-{version}-win32-x64/.roslyn/Microsoft.CodeAnalysis.LanguageServer.exe"
    )
}

// ---------------------------------------------------------------------------
// Extension mapping
// ---------------------------------------------------------------------------

#[test]
fn every_extension_the_registry_claims_maps_to_its_language() {
    assert_eq!(language_for_extension("cs"), Some(Language::CSharp));
    for ext in ["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"] {
        assert_eq!(
            language_for_extension(ext),
            Some(Language::TypeScript),
            "{ext}"
        );
    }
    assert_eq!(language_for_extension("rs"), Some(Language::Rust));
    for ext in ["py", "pyi"] {
        assert_eq!(language_for_extension(ext), Some(Language::Python), "{ext}");
    }
}

#[test]
fn an_extension_is_matched_regardless_of_case() {
    // Windows hands back whatever case the file was created with, and a
    // `.CS` file is a C# file.
    assert_eq!(language_for_extension("CS"), Some(Language::CSharp));
    assert_eq!(language_for_extension("TSX"), Some(Language::TypeScript));
    assert_eq!(language_for_extension("Rs"), Some(Language::Rust));
    assert_eq!(language_for_extension("PyI"), Some(Language::Python));
}

#[test]
fn an_unknown_extension_yields_nothing_rather_than_a_default_server() {
    // The dangerous failure is not "no language server" — it is asking a C#
    // server about a `.csproj` and rendering whatever it says. Every one of
    // these is a file this repository really contains.
    for ext in ["", "txt", "csproj", "sln", "json", "md", "toml", "xml", "c"] {
        assert_eq!(language_for_extension(ext), None, "{ext:?}");
    }
}

#[test]
fn a_leading_dot_is_not_part_of_an_extension() {
    // `Path::extension` never includes the dot, and that is what callers pass.
    assert_eq!(language_for_extension(".cs"), None);
}

// ---------------------------------------------------------------------------
// Rust
// ---------------------------------------------------------------------------

#[test]
fn rust_analyzer_on_path_is_launched_with_no_arguments() {
    let probe = Fake::new().program("rust-analyzer", "C:/cargo/bin/rust-analyzer.exe");

    let found = spec(resolve(Language::Rust, None, &probe));

    assert_eq!(found.id, "rust");
    assert_eq!(found.language, Language::Rust);
    assert_eq!(
        found.program,
        PathBuf::from("C:/cargo/bin/rust-analyzer.exe")
    );
    assert!(found.args.is_empty(), "{:?}", found.args);
    assert!(found.env.is_empty(), "{:?}", found.env);
    assert_eq!(found.uri_style, UriStyle::Encoded);
}

#[test]
fn rust_analyzer_is_not_ready_until_it_says_so_because_an_early_answer_is_wrong() {
    // rust-analyzer answers `references` while still priming its caches, and a
    // low count is a *wrong* answer rather than a partial one. So readiness is
    // a progress token, not "the process started".
    //
    // The prefix is the **whole** token and not the `rustAnalyzer` namespace,
    // which is what it used to be. Captured from rust-analyzer 1.94 against a
    // two-file crate (`tests/lsp_oracle.rs` is the harness):
    //
    // ```text
    //  3.8s  rustAnalyzer/Fetching              end     <- the old prefix matched here
    //  3.9s  rustAnalyzer/Building CrateGraph   end
    //  4.9s  rustAnalyzer/Roots Scanned         end
    //  6.0s  rustAnalyzer/Fetching              end     <- and it is not even one-shot
    // 10.3s  rustAnalyzer/cachePriming          begin   (title "Indexing")
    // 13.9s  rustAnalyzer/cachePriming          end     <- the index is usable here
    // ```
    //
    // The first `end` under the prefix wins and is recorded permanently, so the
    // old prefix published `Ready`, with no caveat, ten seconds early — the
    // "confident wrong zero" shape this subsystem exists to prevent.
    let probe = Fake::new().program("rust-analyzer", "C:/cargo/bin/rust-analyzer.exe");

    let found = spec(resolve(Language::Rust, None, &probe));

    assert_eq!(
        found.readiness,
        Readiness::Progress {
            token_prefix: "rustAnalyzer/cachePriming"
        }
    );
}

#[test]
fn a_missing_rust_analyzer_names_what_was_tried_and_how_to_install_it() {
    let (looked_for, hint) = not_found(resolve(Language::Rust, None, &Fake::new()));

    assert_eq!(looked_for, vec!["rust-analyzer (on PATH)".to_string()]);
    assert!(
        hint.contains("rustup component add rust-analyzer"),
        "hint must be actionable, got {hint:?}"
    );
}

// ---------------------------------------------------------------------------
// TypeScript
// ---------------------------------------------------------------------------

#[test]
fn the_typescript_server_is_launched_over_stdio_and_waits_for_its_project() {
    let probe = Fake::new().program(
        "typescript-language-server",
        "C:/npm/typescript-language-server.cmd",
    );

    let found = spec(resolve(Language::TypeScript, None, &probe));

    assert_eq!(found.id, "typescript");
    assert_eq!(found.language_id, "typescript");
    assert_eq!(
        found.program,
        PathBuf::from("C:/npm/typescript-language-server.cmd")
    );
    assert_eq!(found.args, vec!["--stdio".to_string()]);
    // Not `Immediate`, which is what this said before anybody ran the server.
    // Captured from typescript-language-server 5.3.0 over TypeScript 5.9.3
    // (`tests/lsp_oracle.rs`):
    //
    // ```text
    // 0.2s  initialize result
    // 0.5s  window/workDoneProgress/create  token "41f6b03a-c261-…"
    // 0.5s  $/progress  begin  "Initializing JS/TS language features…"
    // 1.3s  $/progress  end
    // 2.2s  textDocument/references -> the one real call site
    // ```
    //
    // `Immediate` published `Ready` at 0.2s, and a `references` asked in that
    // window is answered `[]` — the wrong answer, delivered faster than the
    // right one. The token is a fresh UUID, so the title is the only thing
    // there is to match on.
    assert_eq!(
        found.readiness,
        Readiness::ProgressTitle {
            title_prefix: "Initializing JS/TS language features"
        }
    );
    // Confirmed by the same capture: it emits `file:///c%3A/…`. Only the drive
    // letter's case differs from what we send, and file identity is decided on
    // paths rather than on URI strings, so that difference cannot matter.
    assert_eq!(found.uri_style, UriStyle::Encoded);
}

#[test]
fn a_missing_typescript_server_names_the_npm_packages_to_install() {
    let (looked_for, hint) = not_found(resolve(Language::TypeScript, None, &Fake::new()));

    assert_eq!(
        looked_for,
        vec!["typescript-language-server (on PATH)".to_string()]
    );
    assert!(
        hint.contains("npm i -g typescript-language-server typescript"),
        "got {hint:?}"
    );
}

// ---------------------------------------------------------------------------
// Python
// ---------------------------------------------------------------------------

#[test]
fn the_python_candidates_are_tried_in_order_with_their_own_arguments() {
    // Each is asserted alone as well as in company: a bug that always picked
    // the first entry would pass a test that only ever installed one.
    let cases = [
        ("basedpyright-langserver", vec!["--stdio".to_string()]),
        ("pyright-langserver", vec!["--stdio".to_string()]),
    ];

    for (name, args) in &cases {
        let probe = Fake::new().program(name, &format!("C:/py/{name}.exe"));
        let found = spec(resolve(Language::Python, None, &probe));
        assert_eq!(found.program, PathBuf::from(format!("C:/py/{name}.exe")));
        assert_eq!(&found.args, args, "{name}");
        // Readiness is looked up per *program*, not per language — see
        // `default_readiness`. Both pyright builds answer `references` with `[]`
        // for about six hundred milliseconds after `initialize` returns and mark
        // the end of that window only by publishing diagnostics.
        assert_eq!(found.readiness, Readiness::FirstDiagnostics, "{name}");
        assert_eq!(found.uri_style, UriStyle::Encoded);
    }
}

#[test]
fn a_configured_python_program_gets_the_arguments_that_program_accepts() {
    // Verified by running all four (`tests/lsp_oracle.rs`):
    //
    // ```text
    // basedpyright-langserver --stdio   ok
    // pyright-langserver      --stdio   ok
    // pylsp                   --stdio   pylsp.EXE: error: unrecognized arguments: --stdio
    // jedi-language-server    --stdio   error: unrecognized arguments: --stdio
    // ```
    //
    // Both of the last two exit 2 immediately. This path used to hand `--stdio`
    // to whatever the user named, because discovery — which knows the answer —
    // never runs once a `program` is configured. So the argument list is looked
    // up by the program's own name against the same table discovery uses, and
    // there is one place that knows what each server takes.
    for (name, args) in [
        ("basedpyright-langserver", vec!["--stdio".to_string()]),
        ("pyright-langserver", vec!["--stdio".to_string()]),
    ] {
        let probe = Fake::new().program(name, &format!("C:/py/{name}.exe"));
        let settings = config(&format!(
            r#"{{ "servers": {{ "python": {{ "program": "{name}" }} }} }}"#
        ));

        let found = spec(resolve(Language::Python, Some(&settings), &probe));

        assert_eq!(found.args, args, "{name}");
    }
}

#[test]
fn a_configured_program_nobody_recognises_keeps_the_language_default() {
    // A wrapper script, a version-pinned shim, a build from source. There is
    // nothing to look up, so the language's usual arguments are the best
    // available answer — and `"args": []` remains the documented escape hatch.
    let probe = Fake::new().program("my-pyright-wrapper", "C:/py/my-pyright-wrapper.exe");
    let settings = config(r#"{ "servers": { "python": { "program": "my-pyright-wrapper" } } }"#);

    let found = spec(resolve(Language::Python, Some(&settings), &probe));

    assert_eq!(found.args, vec!["--stdio".to_string()]);
}

#[test]
fn the_earlier_python_candidate_wins_when_several_are_installed() {
    let probe = Fake::new()
        .program("pyright-langserver", "C:/py/pyright-langserver.exe")
        .program(
            "basedpyright-langserver",
            "C:/py/basedpyright-langserver.exe",
        );

    let found = spec(resolve(Language::Python, None, &probe));

    assert_eq!(
        found.program,
        PathBuf::from("C:/py/basedpyright-langserver.exe")
    );
}

#[test]
fn ruff_is_never_treated_as_a_python_language_server() {
    // `ruff` is the only Python tool on this machine, and it is a linter: it
    // has no `references` and no `definition`. Accepting it would produce a
    // server that connects successfully and then answers nothing, which reads
    // to the user as "this symbol is unused".
    let probe = Fake::new()
        .program("ruff", "C:/py/ruff.exe")
        .program("ruff-lsp", "C:/py/ruff-lsp.exe");

    let (looked_for, _) = not_found(resolve(Language::Python, None, &probe));

    assert!(
        !looked_for.iter().any(|c| c.contains("ruff")),
        "ruff must not even be a candidate: {looked_for:?}"
    );
}

#[test]
fn a_missing_python_server_lists_every_candidate_that_was_tried() {
    let (looked_for, hint) = not_found(resolve(Language::Python, None, &Fake::new()));

    assert_eq!(
        looked_for,
        vec![
            "basedpyright-langserver (on PATH)".to_string(),
            "pyright-langserver (on PATH)".to_string(),
        ]
    );
    assert!(!hint.is_empty());
}

// ---------------------------------------------------------------------------
// C#
// ---------------------------------------------------------------------------

#[test]
fn roslyn_is_launched_with_exactly_the_three_verified_arguments() {
    // Verified against the real binary's `--help` this session. Anything
    // beyond these three is a guess, and a guess here costs a server that
    // starts and then refuses to answer.
    let home = "C:/home";
    let exe = roslyn_at(home, ".vscode", "2.140.9");
    let probe = Fake::new().home(home).file(&exe);

    let found = spec(resolve(Language::CSharp, None, &probe));

    assert_eq!(found.program, PathBuf::from(&exe));
    assert_eq!(
        found.args,
        vec![
            "--stdio".to_string(),
            "--logLevel".to_string(),
            "Warning".to_string(),
            "--autoLoadProjects".to_string(),
        ]
    );
    assert_eq!(
        found.env.get("DOTNET_gcServer").map(String::as_str),
        Some("0")
    );
    assert_eq!(found.uri_style, UriStyle::Plain);
    assert_eq!(found.readiness, Readiness::RoslynProjectInit);
    assert_eq!(found.language_id, "csharp");
}

#[test]
fn the_razor_devkit_and_caller_owned_arguments_are_deliberately_absent() {
    // The VS Code extension always passes the razor/devkit paths; whether the
    // server *requires* them is unverified, so starting minimal and adding on
    // failure is the honest order. `--clientProcessId` and
    // `--extensionLogDirectory` are absent *from the resolved spec* because
    // inventing a pid or a log directory here would be a fabricated value in a
    // subsystem whose whole rule is not to fabricate; `caller_args` builds them
    // at the spawn site, where both are real, and the tests below pin that.
    let home = "C:/home";
    let probe = Fake::new()
        .home(home)
        .file(&roslyn_at(home, ".vscode", "1.0.0"));

    let found = spec(resolve(Language::CSharp, None, &probe));
    let joined = found.args.join(" ");

    for banned in [
        "razor",
        "Razor",
        "DesignTime",
        "devKit",
        "clientProcessId",
        "extensionLogDirectory",
    ] {
        assert!(
            !joined.contains(banned),
            "{banned} must not be passed: {joined}"
        );
    }
}

// ---------------------------------------------------------------------------
// The two arguments only the spawn site can supply
// ---------------------------------------------------------------------------
//
// `ROSLYN_ARGS`' doc comment said these were "the caller's to append" and no
// caller appended them: the live server ran with `--stdio --logLevel Warning
// --autoLoadProjects` and nothing else, and `.code-basics/lsp-logs/` was never
// created. The verified cost was an hour of diagnosis against a server that had
// no log to read.

fn roslyn_spec() -> ServerSpec {
    let home = "C:/home";
    let probe = Fake::new()
        .home(home)
        .file(&roslyn_at(home, ".vscode", "2.140.9"));
    spec(resolve(Language::CSharp, None, &probe))
}

#[test]
fn roslyn_is_told_our_pid_and_where_to_write_its_logs() {
    let logs = PathBuf::from("C:/repo/.code-basics/lsp-logs");
    let args = caller_args(&roslyn_spec(), 4321, Some(&logs));

    assert_eq!(
        args,
        vec![
            "--clientProcessId".to_string(),
            "4321".to_string(),
            "--extensionLogDirectory".to_string(),
            norm(&logs),
        ],
        "each flag is followed by its own value, in the spelling `--help` documents"
    );
}

#[test]
fn a_log_directory_that_could_not_be_created_yields_no_flag_at_all() {
    // Not a flag with an empty value and not a flag pointing at a directory
    // that is not there: either would be a worse start-up than no logging.
    let args = caller_args(&roslyn_spec(), 7, None);

    assert_eq!(args, vec!["--clientProcessId".to_string(), "7".to_string()]);
    assert!(!args.iter().any(|a| a == "--extensionLogDirectory"));
}

#[test]
fn no_other_server_is_given_roslyns_flags() {
    // These are Roslyn's spellings. rust-analyzer, typescript-language-server
    // and the Python servers reject unknown arguments by failing to start, so a
    // flag offered on speculation costs the whole language.
    let logs = PathBuf::from("C:/repo/.code-basics/lsp-logs");
    for (language, program) in [
        (Language::Rust, "rust-analyzer"),
        (Language::TypeScript, "typescript-language-server"),
        (Language::Python, "pyright-langserver"),
    ] {
        let probe = Fake::new().program(program, &format!("C:/bin/{program}.exe"));
        let found = spec(resolve(language, None, &probe));
        assert!(
            caller_args(&found, 11, Some(&logs)).is_empty(),
            "{program} must be launched with exactly what discovery chose"
        );
    }
}

#[test]
fn a_user_who_replaced_the_arguments_gets_exactly_what_they_wrote() {
    // `settings.rs` documents `args` as a full replacement, because removing a
    // default argument has no other spelling. Appending to a list the user
    // rewrote would take that spelling away again.
    let home = "C:/home";
    let probe = Fake::new()
        .home(home)
        .file(&roslyn_at(home, ".vscode", "2.140.9"));
    let cfg = config(r#"{"servers":{"csharp":{"args":["--stdio","--logLevel","Trace"]}}}"#);
    let found = spec(resolve(Language::CSharp, Some(&cfg), &probe));

    assert!(caller_args(&found, 11, None).is_empty());
}

#[test]
fn the_highest_extension_version_wins_and_not_the_lexically_largest() {
    // The bug a lexical sort ships: "2.9.0" > "2.140.9" as strings, so the
    // user's newest C# extension would be ignored in favour of a year-old one.
    let home = "C:/home";
    let old = roslyn_at(home, ".vscode", "2.9.0");
    let new = roslyn_at(home, ".vscode", "2.140.9");
    let probe = Fake::new().home(home).file(&old).file(&new);

    let found = spec(resolve(Language::CSharp, None, &probe));

    assert_eq!(found.program, PathBuf::from(&new));
}

#[test]
fn a_two_component_version_loses_to_the_same_version_with_a_patch() {
    let home = "C:/home";
    let short = roslyn_at(home, ".vscode", "2.140");
    let long = roslyn_at(home, ".vscode", "2.140.1");
    let probe = Fake::new().home(home).file(&short).file(&long);

    assert_eq!(
        spec(resolve(Language::CSharp, None, &probe)).program,
        PathBuf::from(&long)
    );
}

#[test]
fn an_extension_directory_whose_version_does_not_parse_never_beats_one_that_does() {
    // Marketplace directory names are not a contract, so a name we cannot read
    // must not be ranked as though we could. Sorting an unparseable name last
    // is an abstention; treating it as version 0 by accident would be too, but
    // treating it as "greater than everything" would silently pick it.
    let home = "C:/home";
    let parsed = roslyn_at(home, ".vscode", "2.140.9");
    let odd = format!(
        "{home}/.vscode/extensions/ms-dotnettools.csharp-nightly/.roslyn/Microsoft.CodeAnalysis.LanguageServer.exe"
    );
    let probe = Fake::new().home(home).file(&parsed).file(&odd);

    assert_eq!(
        spec(resolve(Language::CSharp, None, &probe)).program,
        PathBuf::from(&parsed)
    );
}

#[test]
fn an_unparseable_version_is_still_used_when_it_is_the_only_install() {
    // Ranking it last is not the same as refusing it: a server that is really
    // there and really works should not be withheld over a directory name.
    let home = "C:/home";
    let odd = format!(
        "{home}/.vscode/extensions/ms-dotnettools.csharp-nightly/.roslyn/Microsoft.CodeAnalysis.LanguageServer.exe"
    );
    let probe = Fake::new().home(home).file(&odd);

    assert_eq!(
        spec(resolve(Language::CSharp, None, &probe)).program,
        PathBuf::from(&odd)
    );
}

#[test]
fn the_extensionless_executable_name_is_accepted_for_non_windows_installs() {
    let home = "/home/aj";
    let exe = format!(
        "{home}/.vscode/extensions/ms-dotnettools.csharp-2.140.9-linux-x64/.roslyn/Microsoft.CodeAnalysis.LanguageServer"
    );
    let probe = Fake::new().home(home).file(&exe);

    assert_eq!(
        spec(resolve(Language::CSharp, None, &probe)).program,
        PathBuf::from(&exe)
    );
}

#[test]
fn the_editor_search_order_decides_when_several_editors_carry_the_extension() {
    // Directory order deciding overall, version deciding within a directory, is
    // the same rule `process::resolve` already applies to PATH — and the
    // alternative (highest version across all editors) would silently launch a
    // Windsurf-installed server for someone working in VS Code.
    let home = "C:/home";
    let vscode = roslyn_at(home, ".vscode", "2.100.0");
    let cursor = roslyn_at(home, ".cursor", "2.140.9");
    let probe = Fake::new().home(home).file(&vscode).file(&cursor);

    assert_eq!(
        spec(resolve(Language::CSharp, None, &probe)).program,
        PathBuf::from(&vscode)
    );
}

#[test]
fn every_supported_editor_directory_is_searched() {
    let home = "C:/home";
    for editor in [
        ".vscode",
        ".vscode-insiders",
        ".vscode-server",
        ".cursor",
        ".windsurf",
    ] {
        let exe = roslyn_at(home, editor, "2.140.9");
        let probe = Fake::new().home(home).file(&exe);
        assert_eq!(
            spec(resolve(Language::CSharp, None, &probe)).program,
            PathBuf::from(&exe),
            "{editor}"
        );
    }
}

#[test]
fn an_extension_with_a_roslyn_directory_but_no_executable_is_reported_not_found() {
    // A half-installed or partially cleaned extension. The candidate has to
    // appear in `looked_for` or the user is told "nothing found" while the
    // directory they are looking at plainly exists.
    let home = "C:/home";
    let dir = format!("{home}/.vscode/extensions/ms-dotnettools.csharp-2.140.9-win32-x64/.roslyn");
    let probe = Fake::new().home(home).dir(&dir);

    let (looked_for, _) = not_found(resolve(Language::CSharp, None, &probe));

    assert!(
        looked_for.iter().any(|c| c.contains(&dir)),
        "the directory examined must be reported: {looked_for:?}"
    );
}

#[test]
fn a_missing_roslyn_names_the_env_override_the_globs_and_the_vscode_extension() {
    let probe = Fake::new().home("C:/home");

    let (looked_for, hint) = not_found(resolve(Language::CSharp, None, &probe));

    assert!(
        looked_for.iter().any(|c| c.contains("CB_ROSLYN_PATH")),
        "{looked_for:?}"
    );
    assert!(
        looked_for
            .iter()
            .any(|c| c.contains("C:/home/.vscode/extensions/ms-dotnettools.csharp-")),
        "{looked_for:?}"
    );
    assert!(
        hint.contains("C#") && hint.to_lowercase().contains("vs code"),
        "{hint:?}"
    );
}

#[test]
fn an_unknown_home_directory_is_reported_as_the_reason_nothing_could_be_searched() {
    // "No server found" and "we could not work out where to look" are different
    // answers, and only the second tells the user to set `CB_ROSLYN_PATH`.
    let (looked_for, _) = not_found(resolve(Language::CSharp, None, &Fake::new()));

    assert!(
        looked_for.iter().any(|c| c.contains("home directory")),
        "{looked_for:?}"
    );
}

#[test]
fn cb_roslyn_path_may_name_the_directory_containing_the_executable() {
    // Mirrors how `CB_INSPECTOR_PATH` is documented: a directory means "the
    // publish output", so a developer can point at a build tree.
    let probe = Fake::new()
        .with_env("CB_ROSLYN_PATH", "D:/roslyn")
        .dir("D:/roslyn")
        .file("D:/roslyn/Microsoft.CodeAnalysis.LanguageServer.exe");

    assert_eq!(
        spec(resolve(Language::CSharp, None, &probe)).program,
        PathBuf::from("D:/roslyn/Microsoft.CodeAnalysis.LanguageServer.exe")
    );
}

#[test]
fn cb_roslyn_path_may_name_the_executable_itself_under_any_name() {
    let probe = Fake::new()
        .with_env("CB_ROSLYN_PATH", "D:/builds/lsp-with-my-fix.exe")
        .file("D:/builds/lsp-with-my-fix.exe");

    assert_eq!(
        spec(resolve(Language::CSharp, None, &probe)).program,
        PathBuf::from("D:/builds/lsp-with-my-fix.exe")
    );
}

#[test]
fn cb_roslyn_path_beats_an_installed_extension() {
    let home = "C:/home";
    let probe = Fake::new()
        .home(home)
        .file(&roslyn_at(home, ".vscode", "2.140.9"))
        .with_env("CB_ROSLYN_PATH", "D:/roslyn/x.exe")
        .file("D:/roslyn/x.exe");

    assert_eq!(
        spec(resolve(Language::CSharp, None, &probe)).program,
        PathBuf::from("D:/roslyn/x.exe")
    );
}

#[test]
fn a_cb_roslyn_path_pointing_nowhere_falls_through_to_discovery_and_is_still_reported() {
    // An environment variable is a developer convenience, not a user's stated
    // configuration — unlike `program` in `config.json`, which must never fall
    // through. A stale variable left in a shell profile should not disable C#.
    let home = "C:/home";
    let exe = roslyn_at(home, ".vscode", "2.140.9");
    let probe = Fake::new()
        .home(home)
        .file(&exe)
        .with_env("CB_ROSLYN_PATH", "D:/gone");

    assert_eq!(
        spec(resolve(Language::CSharp, None, &probe)).program,
        PathBuf::from(&exe)
    );

    let empty = Fake::new().home(home).with_env("CB_ROSLYN_PATH", "D:/gone");
    let (looked_for, _) = not_found(resolve(Language::CSharp, None, &empty));
    assert!(
        looked_for.iter().any(|c| c.contains("D:/gone")),
        "the variable's value must be echoed back: {looked_for:?}"
    );
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[test]
fn a_disabled_server_is_reported_without_touching_the_environment() {
    let cfg = config(r#"{"servers":{"rust":{"enabled":false}}}"#);

    assert_eq!(
        resolve(Language::Rust, Some(&cfg), &NeverProbe),
        Resolution::Disabled {
            language: Language::Rust
        }
    );
}

#[test]
fn disabling_one_server_does_not_disable_the_others() {
    let cfg = config(r#"{"servers":{"rust":{"enabled":false}}}"#);
    let probe = Fake::new().program("pyright-langserver", "C:/py/pyright-langserver.exe");

    assert_eq!(
        spec(resolve(Language::Python, Some(&cfg), &probe)).program,
        PathBuf::from("C:/py/pyright-langserver.exe")
    );
}

#[test]
fn a_configured_program_that_exists_is_used_and_keeps_the_builtin_arguments() {
    let cfg = config(r#"{"servers":{"csharp":{"program":"D:/roslyn/mine.exe"}}}"#);
    let probe = Fake::new().file("D:/roslyn/mine.exe");

    let found = spec(resolve(Language::CSharp, Some(&cfg), &probe));

    assert_eq!(found.program, PathBuf::from("D:/roslyn/mine.exe"));
    assert!(found.args.contains(&"--stdio".to_string()));
    assert_eq!(found.uri_style, UriStyle::Plain);
}

#[test]
fn a_configured_program_may_be_a_bare_name_resolved_on_path() {
    let cfg = config(r#"{"servers":{"rust":{"program":"ra-multiplex"}}}"#);
    let probe = Fake::new().program("ra-multiplex", "C:/bin/ra-multiplex.exe");

    assert_eq!(
        spec(resolve(Language::Rust, Some(&cfg), &probe)).program,
        PathBuf::from("C:/bin/ra-multiplex.exe")
    );
}

#[test]
fn a_configured_program_that_is_absent_never_falls_through_to_discovery() {
    // The whole point of the rule. rust-analyzer *is* on PATH here, so a
    // fall-through would look like success and every answer would come from a
    // server the user did not ask for.
    let cfg = config(r#"{"servers":{"rust":{"program":"D:/nope/ra.exe"}}}"#);
    let probe = Fake::new().program("rust-analyzer", "C:/cargo/bin/rust-analyzer.exe");

    let resolution = resolve(Language::Rust, Some(&cfg), &probe);

    assert!(
        !matches!(resolution, Resolution::Found(_)),
        "a missing configured program must not resolve to a discovered one: {resolution:?}"
    );
    match resolution {
        Resolution::Misconfigured {
            language,
            program,
            detail,
        } => {
            assert_eq!(language, Language::Rust);
            assert_eq!(program, "D:/nope/ra.exe");
            assert!(
                detail.contains("config.json"),
                "the detail must name the file to edit: {detail:?}"
            );
        }
        other => panic!("expected Misconfigured, got {other:?}"),
    }
}

#[test]
fn a_configured_program_that_is_a_directory_is_misconfigured() {
    let cfg = config(r#"{"servers":{"csharp":{"program":"D:/roslyn"}}}"#);
    let probe = Fake::new().dir("D:/roslyn");

    assert!(matches!(
        resolve(Language::CSharp, Some(&cfg), &probe),
        Resolution::Misconfigured { .. }
    ));
}

#[test]
fn a_configured_program_that_is_blank_is_misconfigured_rather_than_ignored() {
    // An empty string is a mistake in a hand-written file, and treating it as
    // "unset" would quietly start a different server.
    let cfg = config(r#"{"servers":{"rust":{"program":"   "}}}"#);
    let probe = Fake::new().program("rust-analyzer", "C:/cargo/bin/rust-analyzer.exe");

    assert!(matches!(
        resolve(Language::Rust, Some(&cfg), &probe),
        Resolution::Misconfigured { .. }
    ));
}

#[test]
fn an_override_with_no_fields_set_changes_nothing_about_discovery() {
    let cfg = config(r#"{"servers":{"rust":{}}}"#);
    let probe = Fake::new().program("rust-analyzer", "C:/cargo/bin/rust-analyzer.exe");

    let found = spec(resolve(Language::Rust, Some(&cfg), &probe));

    assert_eq!(
        found.program,
        PathBuf::from("C:/cargo/bin/rust-analyzer.exe")
    );
    assert!(found.args.is_empty());
}

#[test]
fn configured_arguments_replace_the_discovered_ones_entirely() {
    // Replacement, not appending: a user who needs to *remove* a default
    // argument has no other way to say so, and `settings.rs` documents this.
    let cfg = config(r#"{"servers":{"csharp":{"args":["--stdio","--logLevel","Trace"]}}}"#);
    let home = "C:/home";
    let probe = Fake::new()
        .home(home)
        .file(&roslyn_at(home, ".vscode", "2.140.9"));

    let found = spec(resolve(Language::CSharp, Some(&cfg), &probe));

    assert_eq!(found.args, vec!["--stdio", "--logLevel", "Trace"]);
}

#[test]
fn an_explicit_empty_argument_list_is_honoured() {
    let cfg = config(r#"{"servers":{"csharp":{"args":[]}}}"#);
    let home = "C:/home";
    let probe = Fake::new()
        .home(home)
        .file(&roslyn_at(home, ".vscode", "2.140.9"));

    assert!(spec(resolve(Language::CSharp, Some(&cfg), &probe))
        .args
        .is_empty());
}

#[test]
fn configured_environment_layers_over_the_builtin_environment() {
    // Layering rather than replacing, matching how `process` layers colour
    // defaults under a configuration's own: adding one variable must not drop
    // `DOTNET_gcServer`, and naming it must win.
    let cfg =
        config(r#"{"servers":{"csharp":{"env":{"DOTNET_gcServer":"1","DOTNET_TieredPGO":"0"}}}}"#);
    let home = "C:/home";
    let probe = Fake::new()
        .home(home)
        .file(&roslyn_at(home, ".vscode", "2.140.9"));

    let found = spec(resolve(Language::CSharp, Some(&cfg), &probe));

    assert_eq!(
        found.env.get("DOTNET_gcServer").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        found.env.get("DOTNET_TieredPGO").map(String::as_str),
        Some("0")
    );
}

#[test]
fn a_configured_uri_style_replaces_the_per_server_default() {
    let cfg =
        config(r#"{"servers":{"csharp":{"uriStyle":"encoded"},"rust":{"uriStyle":"plain"}}}"#);
    let home = "C:/home";
    let csharp = Fake::new()
        .home(home)
        .file(&roslyn_at(home, ".vscode", "2.140.9"));
    let rust = Fake::new().program("rust-analyzer", "C:/cargo/bin/rust-analyzer.exe");

    assert_eq!(
        spec(resolve(Language::CSharp, Some(&cfg), &csharp)).uri_style,
        UriStyle::Encoded
    );
    assert_eq!(
        spec(resolve(Language::Rust, Some(&cfg), &rust)).uri_style,
        UriStyle::Plain
    );
}

#[test]
fn overrides_layer_onto_a_configured_program_as_well_as_a_discovered_one() {
    let cfg = config(
        r#"{"servers":{"rust":{"program":"D:/ra.exe","args":["--log-file","x"],"env":{"RA_LOG":"info"}}}}"#,
    );
    let probe = Fake::new().file("D:/ra.exe");

    let found = spec(resolve(Language::Rust, Some(&cfg), &probe));

    assert_eq!(found.program, PathBuf::from("D:/ra.exe"));
    assert_eq!(found.args, vec!["--log-file", "x"]);
    assert_eq!(found.env.get("RA_LOG").map(String::as_str), Some("info"));
}

#[test]
fn an_override_for_a_different_language_is_never_applied() {
    // The lookup is by id, and mixing them up would apply Roslyn's arguments
    // to rust-analyzer — which would fail to start with no clue why.
    let cfg = config(r#"{"servers":{"csharp":{"args":["--nonsense"]}}}"#);
    let probe = Fake::new().program("rust-analyzer", "C:/cargo/bin/rust-analyzer.exe");

    assert!(spec(resolve(Language::Rust, Some(&cfg), &probe))
        .args
        .is_empty());
}

#[test]
fn the_server_ids_are_the_keys_the_settings_file_documents() {
    // `settings.rs` promises `"csharp"`, `"typescript"`, `"rust"`, `"python"`.
    // A rename here would silently ignore everyone's configuration file.
    assert_eq!(Language::CSharp.id(), "csharp");
    assert_eq!(Language::TypeScript.id(), "typescript");
    assert_eq!(Language::Rust.id(), "rust");
    assert_eq!(Language::Python.id(), "python");
}

#[test]
fn every_language_has_timeouts_that_allow_more_time_for_the_first_request() {
    // The first request pays for project load or cache priming; treating it
    // like a steady-state request is what turns a slow start into a spurious
    // "the server is not responding".
    let home = "C:/home";
    let probes: Vec<(Language, Box<dyn Probe>)> = vec![
        (
            Language::CSharp,
            Box::new(
                Fake::new()
                    .home(home)
                    .file(&roslyn_at(home, ".vscode", "2.140.9")),
            ),
        ),
        (
            Language::Rust,
            Box::new(Fake::new().program("rust-analyzer", "C:/cargo/bin/rust-analyzer.exe")),
        ),
        (
            Language::TypeScript,
            Box::new(Fake::new().program("typescript-language-server", "C:/npm/tsls.cmd")),
        ),
        (
            Language::Python,
            Box::new(Fake::new().program("pyright-langserver", "C:/py/pyright-langserver.exe")),
        ),
    ];

    for (language, probe) in &probes {
        let found = spec(resolve(*language, None, probe.as_ref()));
        assert!(
            found.timeouts.first_request > found.timeouts.request,
            "{language:?}"
        );
        assert!(!found.timeouts.request.is_zero(), "{language:?}");
        assert!(!found.timeouts.document_symbol.is_zero(), "{language:?}");
    }
}

#[test]
fn roslyn_is_given_the_longest_first_request_budget_because_it_loads_projects() {
    let home = "C:/home";
    let csharp = spec(resolve(
        Language::CSharp,
        None,
        &Fake::new()
            .home(home)
            .file(&roslyn_at(home, ".vscode", "2.140.9")),
    ));
    let rust = spec(resolve(
        Language::Rust,
        None,
        &Fake::new().program("rust-analyzer", "C:/cargo/bin/rust-analyzer.exe"),
    ));

    assert!(csharp.timeouts.first_request > rust.timeouts.first_request);
}

// ---------------------------------------------------------------------------
// The real machine
// ---------------------------------------------------------------------------
//
// Two checks in the spirit of `pnpm_resolves_to_its_cmd_shim_on_a_machine_that_has_it`:
// they assert only when the thing is present, because the fake probe cannot
// catch a `RealProbe` that looks in the wrong place — and looking in the wrong
// place is exactly what the `%APPDATA%` derivation could get wrong.

#[test]
fn the_real_probe_finds_a_home_directory_that_actually_exists() {
    let probe = RealProbe;
    if let Some(home) = probe.home() {
        assert!(
            probe.is_dir(&home),
            "home resolved to {} which is not a directory",
            home.display()
        );
        // `%APPDATA%` is `<home>/AppData/Roaming`, so two components come off.
        // If that arithmetic were wrong the result would still be *a*
        // directory, so the shape is checked too.
        #[cfg(windows)]
        assert!(
            !home.ends_with("Roaming") && !home.ends_with("AppData"),
            "home is still inside AppData: {}",
            home.display()
        );
    }
}

#[test]
fn the_real_probe_resolves_whatever_of_the_four_is_installed_on_this_machine() {
    // On the machine this was written on that is rust-analyzer and Roslyn, and
    // neither is asserted to be present: this test's job is that a `Found`
    // names a file that really exists, not that any given box has a server.
    for language in [
        Language::CSharp,
        Language::TypeScript,
        Language::Rust,
        Language::Python,
    ] {
        match resolve(language, None, &RealProbe) {
            Resolution::Found(spec) => assert!(
                spec.program.is_file(),
                "{language:?} resolved to {} which is not a file",
                spec.program.display()
            ),
            Resolution::NotFound {
                looked_for, hint, ..
            } => {
                // The reason has to be actionable even here.
                assert!(!looked_for.is_empty(), "{language:?}");
                assert!(!hint.is_empty(), "{language:?}");
            }
            other => panic!("no configuration was supplied, so {other:?} is impossible"),
        }
    }
}
