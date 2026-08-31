//! Finding a debug adapter on this machine — and saying precisely what was
//! looked for when there is not one.
//!
//! Nothing is bundled, exactly as with [`crate::lsp::registry`]: a debugger is
//! hundreds of megabytes, is licensed per-editor in at least one case, and is
//! already installed on the machine of anybody who has ever debugged this code
//! in VS Code or Visual Studio. So this looks, and reports.
//!
//! It reuses that module's [`Probe`] rather than defining a second environment
//! abstraction. Both need the same four questions answered about a machine, and
//! a second copy would be a second thing to fake in tests and a second place for
//! the Windows PATH handling to drift.
//!
//! # The one thing this must not do
//!
//! **A missing adapter is never quietly turned into an ordinary run.** A Debug
//! button that starts the process without a debugger attached is worse than one
//! that refuses: the user sets a breakpoint, nothing stops, and there is nothing
//! anywhere saying why. So the failure carries `looked_for` — every path tried,
//! in order — and a `hint` naming the install, and
//! [`crate::dap::model::DebugState::NotInstalled`] is a state the UI has to draw
//! rather than an `Option` it can unwrap into silence.
//!
//! # What is supported, honestly
//!
//! **.NET is supported.** `vsdbg-ui` ships inside the VS Code C# extension —
//! verified present on the development machine at
//! `.vscode/extensions/ms-dotnettools.csharp-<version>-win32-x64/.debugger/x86_64/vsdbg-ui.exe`
//! — and `netcoredbg` is a drop-in alternative on PATH. Both speak DAP over
//! stdio with `--interpreter=vscode`.
//!
//! **Node is not, yet, and this module says so rather than pretending.** The
//! js-debug adapter bundled with VS Code has no standalone entry point — it is
//! compiled into the extension host and runs in-process — so the copy on a
//! machine with VS Code installed cannot be launched as an adapter. The
//! standalone build (the `js-debug-dap` release asset) can, but it speaks DAP
//! over a **TCP port** rather than stdio, and this app has no socket transport.
//! So [`resolve`] returns [`Resolution::NotFound`] for Node with that stated in
//! the hint. Inventing a path that would fail at spawn time would move the same
//! failure to a place with less information at it.

use std::path::{Path, PathBuf};

use crate::lsp::registry::{parse_extension_version, Probe, CSHARP_EXTENSION_PREFIX, EDITOR_DIRS};

/// Which debugger an ecosystem needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Debuggee {
    /// Anything `dotnet run` starts.
    DotNet,
    /// Anything node starts.
    Node,
}

impl Debuggee {
    /// The debuggee kind for a run configuration's ecosystem, or `None`.
    ///
    /// `None` for `cargo` and for a declarative adapter, and that is a real
    /// answer rather than a gap: this app has no Rust debugger wired up, and
    /// offering a Debug button that resolves to somebody else's debugger would
    /// be the silent-wrong-thing this module exists to prevent.
    pub fn for_ecosystem(ecosystem: &str) -> Option<Self> {
        match ecosystem {
            "dotnet" => Some(Debuggee::DotNet),
            "node" => Some(Debuggee::Node),
            _ => None,
        }
    }

    /// The `adapterID` sent in `initialize`. Adapters key behaviour off it.
    pub fn adapter_id(self) -> &'static str {
        match self {
            Debuggee::DotNet => "coreclr",
            Debuggee::Node => "pwa-node",
        }
    }
}

/// A resolved adapter: what to run, and with what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Which of the candidates this was, for the status line — "vsdbg from the
    /// VS Code C# extension" is a more useful thing to show than a path.
    pub description: String,
}

/// The outcome of looking for an adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Found(AdapterSpec),
    /// Nothing usable. `looked_for` is every candidate tried, in order, so the
    /// user can see whether the thing they installed was even considered.
    NotFound {
        looked_for: Vec<String>,
        hint: String,
    },
    /// An override was set and does not resolve.
    ///
    /// Never falls through to discovery, for [`crate::lsp::settings`]'s reason:
    /// the user pinned a specific debugger, and silently running a different one
    /// makes every answer attributable to a program they did not choose.
    Misconfigured {
        detail: String,
    },
}

/// Environment variable pinning the .NET adapter.
pub const DOTNET_ADAPTER_ENV: &str = "CB_DAP_DOTNET";

/// Environment variable pinning the Node adapter.
pub const NODE_ADAPTER_ENV: &str = "CB_DAP_NODE";

/// The two spellings of vsdbg's DAP front end, preferred first.
///
/// `vsdbg-ui` is the one VS Code launches for a debug session; `vsdbg` is the
/// engine beneath it and also accepts `--interpreter=vscode`. Both are tried
/// because a trimmed install may carry only one, and the fallback costs a
/// `is_file` call.
const VSDBG_NAMES: &[&str] = &["vsdbg-ui.exe", "vsdbg-ui", "vsdbg.exe", "vsdbg"];

/// Architecture directories inside the extension's `.debugger`, preferred first.
const VSDBG_ARCH_DIRS: &[&str] = &["x86_64", "arm64", "x86"];

/// The argument that puts either debugger into DAP mode.
const DAP_INTERPRETER: &str = "--interpreter=vscode";

/// Find the adapter for a debuggee.
pub fn resolve(debuggee: Debuggee, probe: &dyn Probe) -> Resolution {
    match debuggee {
        Debuggee::DotNet => resolve_dotnet(probe),
        Debuggee::Node => resolve_node(probe),
    }
}

fn resolve_dotnet(probe: &dyn Probe) -> Resolution {
    if let Some(pinned) = probe.env(DOTNET_ADAPTER_ENV) {
        return pinned_adapter(&pinned, probe, DOTNET_ADAPTER_ENV);
    }

    let mut looked_for = Vec::new();

    // The C# extension first: it is the copy a .NET developer on this machine
    // already has, and it is version-matched to nothing, so no SDK question
    // arises. Editor order decides, then version within an editor — the rule
    // `lsp::registry` already applies, for the same reason.
    match probe.home() {
        Some(home) => {
            for editor in EDITOR_DIRS {
                let extensions = home.join(editor).join("extensions");
                if !probe.is_dir(&extensions) {
                    looked_for.push(format!("{} (not present)", extensions.display()));
                    continue;
                }
                if let Some(spec) = best_vsdbg_in(&extensions, probe, &mut looked_for) {
                    return Resolution::Found(spec);
                }
            }
        }
        None => looked_for.push(
            "the home directory could not be determined, so no editor extensions were searched"
                .to_string(),
        ),
    }

    // Then PATH, for netcoredbg — the open-source alternative, and the only
    // option on a machine with no VS Code.
    for name in ["netcoredbg", "netcoredbg.exe"] {
        match probe.on_path(name) {
            Some(program) => {
                return Resolution::Found(AdapterSpec {
                    program,
                    args: vec![DAP_INTERPRETER.to_string()],
                    description: format!("netcoredbg (found on PATH as {name})"),
                })
            }
            None => looked_for.push(format!("{name} on PATH")),
        }
    }

    Resolution::NotFound {
        looked_for,
        hint: "Install the C# extension for VS Code (it ships the vsdbg debugger), \
               or put `netcoredbg` on PATH. Set CB_DAP_DOTNET to point at either one directly."
            .to_string(),
    }
}

/// The newest C# extension's debugger, under one editor's extensions directory.
///
/// Mirrors `lsp::registry::best_roslyn_in`, including the two decisions that
/// module learned the hard way: the children are sorted so that two unparseable
/// names do not pick differently run to run, and versions are compared as
/// numbers so `2.140.9` beats `2.9.0`.
fn best_vsdbg_in(
    extensions: &Path,
    probe: &dyn Probe,
    looked_for: &mut Vec<String>,
) -> Option<AdapterSpec> {
    let mut children = probe.read_dir(extensions);
    children.sort();

    let mut best: Option<(Option<Vec<u64>>, PathBuf)> = None;
    for child in children {
        let Some(name) = child.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(CSHARP_EXTENSION_PREFIX) {
            continue;
        }

        let debugger = child.join(".debugger");
        let exe = VSDBG_ARCH_DIRS
            .iter()
            .map(|arch| debugger.join(arch))
            // The pre-arch layout put the executables straight in `.debugger`.
            .chain(std::iter::once(debugger.clone()))
            .flat_map(|dir| VSDBG_NAMES.iter().map(move |n| dir.join(n)))
            .find(|candidate| probe.is_file(candidate));

        let Some(exe) = exe else {
            // Reported rather than skipped: otherwise the user is told nothing
            // was found while looking straight at an installed C# extension.
            looked_for.push(format!(
                "{} contains no vsdbg executable",
                debugger.display()
            ));
            continue;
        };

        let version = parse_extension_version(name);
        if best.as_ref().is_none_or(|(seen, _)| version > *seen) {
            best = Some((version, exe));
        }
    }

    best.map(|(_, program)| AdapterSpec {
        description: format!(
            "vsdbg from the VS Code C# extension ({})",
            program.display()
        ),
        program,
        args: vec![DAP_INTERPRETER.to_string()],
    })
}

fn resolve_node(probe: &dyn Probe) -> Resolution {
    if let Some(pinned) = probe.env(NODE_ADAPTER_ENV) {
        return pinned_adapter(&pinned, probe, NODE_ADAPTER_ENV);
    }

    let mut looked_for = vec![
        "js-debug-adapter on PATH".to_string(),
        format!("{NODE_ADAPTER_ENV} (not set)"),
    ];

    // The standalone build is the only launchable one, and it is not commonly
    // installed. Checked anyway so that somebody who *has* installed it is not
    // told to install it again.
    for name in ["js-debug-adapter", "js-debug-adapter.cmd"] {
        if let Some(program) = probe.on_path(name) {
            return Resolution::Found(AdapterSpec {
                program,
                args: Vec::new(),
                description: format!("js-debug (found on PATH as {name})"),
            });
        }
    }

    looked_for.push(
        "the js-debug bundled with VS Code, which has no standalone entry point and \
         cannot be launched as an adapter"
            .to_string(),
    );

    Resolution::NotFound {
        looked_for,
        hint: "Node debugging needs the standalone js-debug adapter, and this app cannot \
               speak to it yet: it serves DAP over a TCP port and only stdio adapters are \
               supported here. Set CB_DAP_NODE if you have a stdio-speaking adapter."
            .to_string(),
    }
}

/// Resolve an adapter the user pinned by environment variable.
fn pinned_adapter(pinned: &str, probe: &dyn Probe, var: &str) -> Resolution {
    let trimmed = pinned.trim();
    if trimmed.is_empty() {
        return Resolution::Misconfigured {
            detail: format!("{var} is set but empty"),
        };
    }

    let path = PathBuf::from(trimmed);
    if probe.is_file(&path) {
        return Resolution::Found(AdapterSpec {
            description: format!("{var}={trimmed}"),
            program: path,
            args: vec![DAP_INTERPRETER.to_string()],
        });
    }
    if let Some(program) = probe.on_path(trimmed) {
        return Resolution::Found(AdapterSpec {
            description: format!("{var}={trimmed} (resolved on PATH)"),
            program,
            args: vec![DAP_INTERPRETER.to_string()],
        });
    }

    Resolution::Misconfigured {
        detail: format!("{var} is set to {trimmed}, which is not a file and is not on PATH"),
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
