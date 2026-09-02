//! Finding a debug adapter on this machine — and saying precisely what was
//! looked for when there is not one.
//!
//! Nothing is bundled, exactly as with [`crate::lsp::registry`]: a debugger is
//! hundreds of megabytes, is licensed per-editor in at least one case, and is
//! commonly installed separately by developers who need them. So this looks,
//! and reports.
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
//! **.NET is supported through NetCoreDbg.** It is open source and speaks DAP
//! over stdio with `--interpreter=vscode`. The proprietary debugger bundled
//! with Microsoft's C# extension is not reused outside the Visual Studio
//! product family its runtime licence names.
//!
//! **Node uses the standalone js-debug server.** The
//! js-debug adapter bundled with VS Code has no standalone entry point — it is
//! compiled into the extension host and runs in-process — so the copy on a
//! machine with VS Code installed cannot be launched as an adapter. The
//! standalone build (the `js-debug-dap` release asset) can, but it speaks DAP
//! over a **TCP port**, which the session layer supports. [`resolve`] looks for
//! that standalone launcher only and explains how to configure it when absent.

use std::path::{Path, PathBuf};

use crate::lsp::registry::Probe;

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
    /// Which candidate this was, for a useful status line rather than a bare
    /// filesystem path.
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

/// The argument that puts either debugger into DAP mode.
const DAP_INTERPRETER: &str = "--interpreter=vscode";

/// Where each adapter sits inside the bundled resource directory.
///
/// These are the paths `scripts/fetch-debuggers.mjs` writes. The two must move
/// together — a mismatch un-bundles an adapter *silently*, since the resolver
/// simply falls through to PATH and reports "not installed" while the files sit
/// in the install directory. `registry_tests` pins both spellings for that
/// reason.
const BUNDLED_DOTNET: &[&str] = &["netcoredbg/netcoredbg.exe", "netcoredbg/netcoredbg"];
const BUNDLED_NODE: &str = "js-debug/src/dapDebugServer.js";

/// Find the adapter for a debuggee.
///
/// `bundled_dir` is the installer's `resources/debuggers/`, or `None` when
/// there is none — a `cargo build` produces no resource directory, and a build
/// made with no network produces an empty one. Both are ordinary answers, and
/// the search continues to PATH in either case.
///
/// Order is **pin, bundle, PATH**. The pin is an explicit instruction and wins
/// outright. The bundle comes next because it is the version this app was built
/// against and is known to work with it; a copy on PATH is any version at all,
/// so it is the fallback rather than the default — and `CB_DAP_*` remains the
/// way to insist on your own.
pub fn resolve(debuggee: Debuggee, probe: &dyn Probe, bundled_dir: Option<&Path>) -> Resolution {
    match debuggee {
        Debuggee::DotNet => resolve_dotnet(probe, bundled_dir),
        Debuggee::Node => resolve_node(probe, bundled_dir),
    }
}

fn resolve_dotnet(probe: &dyn Probe, bundled_dir: Option<&Path>) -> Resolution {
    if let Some(pinned) = probe.env(DOTNET_ADAPTER_ENV) {
        return pinned_adapter(&pinned, probe, DOTNET_ADAPTER_ENV);
    }

    let mut looked_for = Vec::new();

    for relative in BUNDLED_DOTNET {
        let Some(candidate) = bundled_dir.map(|dir| dir.join(relative)) else {
            break;
        };
        if probe.is_file(&candidate) {
            return Resolution::Found(AdapterSpec {
                description: "netcoredbg (bundled with code-basics)".to_string(),
                program: candidate,
                args: vec![DAP_INTERPRETER.to_string()],
            });
        }
        looked_for.push(format!("{} (not bundled)", candidate.display()));
    }

    // NetCoreDbg is open source. The vsdbg copy inside Microsoft's C# extension
    // is deliberately not discovered because its runtime licence restricts it
    // to the Visual Studio product family.
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
        hint: "Install NetCoreDbg and put `netcoredbg` on PATH, or set \
               CB_DAP_DOTNET to its executable."
            .to_string(),
    }
}

fn resolve_node(probe: &dyn Probe, bundled_dir: Option<&Path>) -> Resolution {
    if let Some(pinned) = probe.env(NODE_ADAPTER_ENV) {
        return needs_node(pinned_adapter(&pinned, probe, NODE_ADAPTER_ENV), probe);
    }

    let mut looked_for = vec![
        "js-debug-adapter on PATH".to_string(),
        format!("{NODE_ADAPTER_ENV} (not set)"),
    ];

    if let Some(candidate) = bundled_dir.map(|dir| dir.join(BUNDLED_NODE)) {
        if probe.is_file(&candidate) {
            return needs_node(
                Resolution::Found(AdapterSpec {
                    description: "js-debug (bundled with code-basics)".to_string(),
                    program: candidate,
                    args: Vec::new(),
                }),
                probe,
            );
        }
        looked_for.push(format!("{} (not bundled)", candidate.display()));
    }

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
        hint:
            "Download the standalone vscode-js-debug DAP server and put its launcher on \
               PATH as `js-debug-adapter`, or set CB_DAP_NODE to its executable or .js entry point."
                .to_string(),
    }
}

/// Refuse a JavaScript adapter when there is no Node to run it.
///
/// js-debug's standalone server is a `.js` file, so the spawn layer runs it as
/// `node <script>`. Without Node that fails with "program not found" naming
/// *node* — from a layer that has no idea why this app wanted Node at all. The
/// reason is known here, so the refusal belongs here: an adapter that cannot
/// start is not found, and saying so early keeps the six-answer rule intact
/// rather than collapsing a missing runtime into a generic launch failure.
fn needs_node(resolution: Resolution, probe: &dyn Probe) -> Resolution {
    let Resolution::Found(spec) = &resolution else {
        return resolution;
    };
    if spec.program.extension().and_then(|e| e.to_str()) != Some("js") {
        return resolution;
    }
    if probe.on_path("node").is_some() {
        return resolution;
    }
    Resolution::NotFound {
        looked_for: vec![
            format!("{} (present)", spec.program.display()),
            "node on PATH (not found)".to_string(),
        ],
        hint: format!(
            "The js-debug adapter at {} is a JavaScript program and needs Node.js to run it.              Install Node and put `node` on PATH.",
            spec.program.display()
        ),
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
            args: if var == DOTNET_ADAPTER_ENV {
                vec![DAP_INTERPRETER.to_string()]
            } else {
                Vec::new()
            },
        });
    }
    if let Some(program) = probe.on_path(trimmed) {
        return Resolution::Found(AdapterSpec {
            description: format!("{var}={trimmed} (resolved on PATH)"),
            program,
            args: if var == DOTNET_ADAPTER_ENV {
                vec![DAP_INTERPRETER.to_string()]
            } else {
                Vec::new()
            },
        });
    }

    Resolution::Misconfigured {
        detail: format!("{var} is set to {trimmed}, which is not a file and is not on PATH"),
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
