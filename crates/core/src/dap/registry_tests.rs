//! Tests for adapter discovery. Included by `registry.rs`.
//!
//! The fake environment is a copy of the one in `lsp/registry_tests.rs` rather
//! than an import: a `#[cfg(test)]` helper in another module is not reachable
//! from here, and lifting it into the crate proper would put test scaffolding in
//! the shipped build to save forty lines. The `Probe` *trait* is shared, which
//! is the part that matters — both modules ask a machine the same questions.

use super::*;

use std::collections::{BTreeMap, BTreeSet};

/// Paths compared as forward-slashed strings, so these tests do not depend on
/// which separator `Path::join` produces.
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

    fn program(mut self, name: &str, at: &str) -> Self {
        self.on_path.insert(name.to_string(), at.to_string());
        self = self.file(at);
        self
    }

    /// A file, plus every directory above it.
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

/// The real layout, verified on the development machine: the C# extension's
/// debugger lives under `.debugger/<arch>/`.
fn vsdbg_at(home: &str, editor: &str, version: &str) -> String {
    format!(
        "{home}/{editor}/extensions/ms-dotnettools.csharp-{version}/.debugger/x86_64/vsdbg-ui.exe"
    )
}

fn found(resolution: Resolution) -> AdapterSpec {
    match resolution {
        Resolution::Found(spec) => spec,
        other => panic!("expected an adapter, got {other:?}"),
    }
}

fn not_found(resolution: Resolution) -> (Vec<String>, String) {
    match resolution {
        Resolution::NotFound { looked_for, hint } => (looked_for, hint),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Which debugger an ecosystem needs
// ---------------------------------------------------------------------------

#[test]
fn an_ecosystem_with_no_debugger_wired_up_yields_none() {
    // A real answer, not a gap: offering a Debug button that resolves to
    // somebody else's debugger is the silent-wrong-thing this module prevents.
    assert_eq!(Debuggee::for_ecosystem("cargo"), None);
    assert_eq!(Debuggee::for_ecosystem("pytest"), None);
    assert_eq!(Debuggee::for_ecosystem(""), None);
}

#[test]
fn the_two_supported_ecosystems_map_to_their_adapter_ids() {
    assert_eq!(
        Debuggee::for_ecosystem("dotnet").unwrap().adapter_id(),
        "coreclr"
    );
    assert_eq!(
        Debuggee::for_ecosystem("node").unwrap().adapter_id(),
        "pwa-node"
    );
}

// ---------------------------------------------------------------------------
// .NET
// ---------------------------------------------------------------------------

#[test]
fn vsdbg_is_found_inside_the_vs_code_csharp_extension() {
    let path = vsdbg_at("/home/me", ".vscode", "2.140.9-win32-x64");
    let probe = Fake::new().home("/home/me").file(&path);

    let spec = found(resolve(Debuggee::DotNet, &probe));

    assert_eq!(norm(&spec.program), path);
    assert_eq!(spec.args, vec!["--interpreter=vscode"]);
    assert!(spec.description.contains("C# extension"), "{spec:?}");
}

#[test]
fn the_newest_extension_wins_and_versions_are_compared_as_numbers() {
    // Lexically "2.9.0" sorts above "2.140.9". The same bug `lsp::registry`
    // has a test for, and the same fix.
    let old = vsdbg_at("/home/me", ".vscode", "2.9.0-win32-x64");
    let new = vsdbg_at("/home/me", ".vscode", "2.140.9-win32-x64");
    let probe = Fake::new().home("/home/me").file(&old).file(&new);

    assert_eq!(norm(&found(resolve(Debuggee::DotNet, &probe)).program), new);
}

#[test]
fn editor_order_decides_before_version_does() {
    // A Windsurf-installed debugger must not be launched for somebody working
    // in VS Code, however new it is.
    let vscode = vsdbg_at("/home/me", ".vscode", "2.1.0-win32-x64");
    let windsurf = vsdbg_at("/home/me", ".windsurf", "9.9.9-win32-x64");
    let probe = Fake::new().home("/home/me").file(&vscode).file(&windsurf);

    assert_eq!(
        norm(&found(resolve(Debuggee::DotNet, &probe)).program),
        vscode
    );
}

#[test]
fn the_pre_arch_layout_is_still_found() {
    let path = "/home/me/.vscode/extensions/ms-dotnettools.csharp-1.0.0/.debugger/vsdbg-ui.exe";
    let probe = Fake::new().home("/home/me").file(path);

    assert_eq!(
        norm(&found(resolve(Debuggee::DotNet, &probe)).program),
        path
    );
}

#[test]
fn vsdbg_is_accepted_when_the_ui_front_end_is_absent() {
    let path = "/home/me/.vscode/extensions/ms-dotnettools.csharp-2.0.0/.debugger/x86_64/vsdbg.exe";
    let probe = Fake::new().home("/home/me").file(path);

    assert_eq!(
        norm(&found(resolve(Debuggee::DotNet, &probe)).program),
        path
    );
}

#[test]
fn netcoredbg_on_path_is_used_when_no_extension_has_a_debugger() {
    let probe = Fake::new()
        .home("/home/me")
        .program("netcoredbg", "/usr/local/bin/netcoredbg");

    let spec = found(resolve(Debuggee::DotNet, &probe));

    assert_eq!(norm(&spec.program), "/usr/local/bin/netcoredbg");
    assert_eq!(spec.args, vec!["--interpreter=vscode"]);
}

#[test]
fn the_extension_is_preferred_over_path() {
    // It is the copy a .NET developer on this machine already has, and it
    // raises no SDK-version question.
    let path = vsdbg_at("/home/me", ".vscode", "2.0.0-win32-x64");
    let probe = Fake::new()
        .home("/home/me")
        .file(&path)
        .program("netcoredbg", "/usr/local/bin/netcoredbg");

    assert_eq!(
        norm(&found(resolve(Debuggee::DotNet, &probe)).program),
        path
    );
}

#[test]
fn an_installed_extension_with_no_debugger_is_named_rather_than_skipped_silently() {
    // Otherwise the user is told nothing was found while looking straight at
    // an installed C# extension.
    let probe = Fake::new()
        .home("/home/me")
        .dir("/home/me/.vscode/extensions/ms-dotnettools.csharp-2.0.0/.debugger");

    let (looked_for, _) = not_found(resolve(Debuggee::DotNet, &probe));

    assert!(
        looked_for
            .iter()
            .any(|entry| entry.contains("contains no vsdbg executable")),
        "{looked_for:?}"
    );
}

#[test]
fn nothing_installed_reports_every_candidate_and_how_to_fix_it() {
    let probe = Fake::new().home("/home/me");

    let (looked_for, hint) = not_found(resolve(Debuggee::DotNet, &probe));

    assert!(
        looked_for.iter().any(|e| e.contains("netcoredbg on PATH")),
        "{looked_for:?}"
    );
    assert!(
        looked_for.iter().any(|e| e.contains(".vscode")),
        "{looked_for:?}"
    );
    assert!(hint.contains("C# extension"), "{hint}");
    assert!(hint.contains(DOTNET_ADAPTER_ENV), "{hint}");
}

#[test]
fn an_unknown_home_is_a_different_problem_from_nothing_installed() {
    let probe = Fake::new();
    let (looked_for, _) = not_found(resolve(Debuggee::DotNet, &probe));

    assert!(
        looked_for
            .iter()
            .any(|e| e.contains("home directory could not be determined")),
        "{looked_for:?}"
    );
}

// ---------------------------------------------------------------------------
// The pinned override
// ---------------------------------------------------------------------------

#[test]
fn a_pinned_adapter_is_used_as_given() {
    let probe = Fake::new()
        .home("/home/me")
        .file("/opt/dbg/netcoredbg")
        .with_env(DOTNET_ADAPTER_ENV, "/opt/dbg/netcoredbg");

    let spec = found(resolve(Debuggee::DotNet, &probe));
    assert_eq!(norm(&spec.program), "/opt/dbg/netcoredbg");
}

#[test]
fn a_pinned_adapter_may_be_a_bare_name_on_path() {
    let probe = Fake::new()
        .home("/home/me")
        .program("mydbg", "/usr/bin/mydbg")
        .with_env(DOTNET_ADAPTER_ENV, "mydbg");

    assert_eq!(
        norm(&found(resolve(Debuggee::DotNet, &probe)).program),
        "/usr/bin/mydbg"
    );
}

#[test]
fn a_pinned_adapter_that_does_not_resolve_never_falls_through_to_discovery() {
    // The user pinned a specific debugger. Silently running a different one
    // makes every answer attributable to a program they did not choose.
    let installed = vsdbg_at("/home/me", ".vscode", "2.0.0-win32-x64");
    let probe = Fake::new()
        .home("/home/me")
        .file(&installed)
        .with_env(DOTNET_ADAPTER_ENV, "/nowhere/dbg");

    match resolve(Debuggee::DotNet, &probe) {
        Resolution::Misconfigured { detail } => {
            assert!(detail.contains("/nowhere/dbg"), "{detail}");
            assert!(detail.contains(DOTNET_ADAPTER_ENV), "{detail}");
        }
        other => panic!("expected Misconfigured, got {other:?}"),
    }
}

#[test]
fn an_empty_override_is_misconfigured_rather_than_ignored() {
    let probe = Fake::new()
        .home("/home/me")
        .with_env(DOTNET_ADAPTER_ENV, "   ");

    assert!(matches!(
        resolve(Debuggee::DotNet, &probe),
        Resolution::Misconfigured { .. }
    ));
}

// ---------------------------------------------------------------------------
// Node — the honest gap
// ---------------------------------------------------------------------------

#[test]
fn node_reports_why_it_cannot_be_debugged_rather_than_inventing_a_path() {
    // The js-debug bundled with VS Code has no standalone entry point, and the
    // standalone build serves DAP over TCP, which this app cannot speak.
    let probe = Fake::new().home("/home/me");

    let (looked_for, hint) = not_found(resolve(Debuggee::Node, &probe));

    assert!(
        looked_for.iter().any(|e| e.contains("js-debug-adapter")),
        "{looked_for:?}"
    );
    assert!(
        looked_for
            .iter()
            .any(|e| e.contains("no standalone entry point")),
        "{looked_for:?}"
    );
    assert!(hint.contains("TCP port"), "{hint}");
    assert!(hint.contains(NODE_ADAPTER_ENV), "{hint}");
}

#[test]
fn a_standalone_node_adapter_on_path_is_used_if_somebody_has_one() {
    // Somebody who has installed it must not be told to install it again.
    let probe = Fake::new()
        .home("/home/me")
        .program("js-debug-adapter", "/usr/local/bin/js-debug-adapter");

    let spec = found(resolve(Debuggee::Node, &probe));
    assert_eq!(norm(&spec.program), "/usr/local/bin/js-debug-adapter");
}

#[test]
fn node_honours_its_own_override_and_not_the_dotnet_one() {
    let probe = Fake::new()
        .home("/home/me")
        .file("/opt/node-dbg")
        .with_env(NODE_ADAPTER_ENV, "/opt/node-dbg");

    assert_eq!(
        norm(&found(resolve(Debuggee::Node, &probe)).program),
        "/opt/node-dbg"
    );

    let wrong_var = Fake::new()
        .home("/home/me")
        .file("/opt/node-dbg")
        .with_env(DOTNET_ADAPTER_ENV, "/opt/node-dbg");
    assert!(matches!(
        resolve(Debuggee::Node, &wrong_var),
        Resolution::NotFound { .. }
    ));
}
