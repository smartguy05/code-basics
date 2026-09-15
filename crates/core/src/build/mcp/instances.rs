//! Which running application an MCP Build client should talk to — and the five
//! different reasons there might not be one.
//!
//! The Build MCP server is a **separate process** (`cb-app mcp-build`,
//! self-dispatched) started by an agent, not by the user. It has no window, no
//! `AppState` and no way to know whether an application is running at all, so the
//! application publishes what a client needs into a small user-global registry and
//! this module is everything that is decided about it. It is the
//! [`crate::roslyn::instances`] pattern exactly, with a build-specific pipe and
//! registry file.
//!
//! # Narrowing on the workspace, not on a panel
//!
//! The boundary is `--workspace <root>`: [`choose_instance`] narrows on **which
//! running application has that workspace open** ([`BuildInstance::workspaces`]).
//! An agent configured for repo X reaches the window with repo X open regardless
//! of which window is focused — and two windows with repo X open is an
//! [`InstanceError::Ambiguous`] refusal, resolvable with `--instance`.
//!
//! # The registry is durable facts only, and liveness is never one of them
//!
//! An entry says a pid, the executable that wrote it, the protocol that build
//! speaks, the pipe and token to connect with, and the workspace roots that window
//! has open. It does **not** say the process is alive, because a crash leaves the
//! entry behind and a recycled pid then names a stranger. So [`choose_instance`]
//! takes liveness as a parameter and the caller re-probes, comparing the
//! executable path as well as the pid.
//!
//! # Why the file is tolerant
//!
//! Same rule as [`crate::notes`] and [`crate::roslyn::instances`]: a missing or
//! corrupt file loads as **no instances**, never as an error. An MCP server that
//! fails to start is dropped silently by its host and the user never learns why.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The wire version of everything in this module and on the pipe.
pub const PROTOCOL_VERSION: u32 = 1;

/// The registry file's name under the user config directory.
const INSTANCES_FILE: &str = "build-instances.json";

/// Where an application publishes its pipe, and how a client presents itself.
///
/// Present whenever the application is running: the Build pipe is process-global
/// (opened at startup), so unlike the browser's, this is not an `Option`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listener {
    /// The full pipe name, `\\.\pipe\code-basics.build.<pid>`.
    ///
    /// **Diagnostic only. A client must never open this — it must open
    /// [`pipe_name`] of the verified pid.** A value here that disagrees with the
    /// derived name is *evidence of tampering* — see
    /// [`InstanceError::PipeNameMismatch`].
    pub pipe: String,
    /// A per-launch random token a client presents on every request. **A speed
    /// bump**; the boundary is the `--workspace` scope baked into the install.
    pub token: String,
}

/// The control pipe's name for a given process id.
///
/// **The one rule for this name**, used by the application when it creates the
/// pipe and by a client when it opens one. A client derives it from a pid it has
/// already verified against [`BuildInstance::exe`]; it must never open the name a
/// registry file merely *states* — see [`Listener::pipe`].
pub fn pipe_name(pid: u32) -> String {
    format!(r"\\.\pipe\code-basics.build.{pid}")
}

/// One running application, as far as a client can know it without connecting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInstance {
    pub pid: u32,
    /// The full path of the executable that wrote this entry, so a **recycled
    /// pid** can be told apart from this application.
    pub exe: String,
    pub protocol: u32,
    /// The pipe and token to connect with. Present whenever the application is
    /// running — the pipe is process-global.
    pub listener: Listener,
    /// The workspace roots this window has open. This is what [`choose_instance`]
    /// narrows on.
    #[serde(default)]
    pub workspaces: Vec<String>,
}

impl BuildInstance {
    /// Whether this window has `workspace` open, compared the way Windows paths
    /// compare — case-insensitively, separators normalised, trailing separators
    /// ignored.
    pub fn has_workspace(&self, workspace: &str) -> bool {
        let wanted = normalize_root(workspace);
        !wanted.is_empty()
            && self
                .workspaces
                .iter()
                .any(|open| normalize_root(open) == wanted)
    }
}

/// Normalise a workspace root for comparison. Not canonicalised: canonicalising
/// touches the filesystem, and a root that has become briefly unreadable must
/// still match the entry it wrote.
fn normalize_root(value: &str) -> String {
    let normalised = value.replace('/', "\\").to_lowercase();
    normalised.trim_end_matches('\\').to_string()
}

/// The registry: a version and the instances that published themselves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstancesFile {
    pub version: u32,
    #[serde(default)]
    pub instances: Vec<BuildInstance>,
}

impl Default for InstancesFile {
    fn default() -> Self {
        Self {
            version: 1,
            instances: Vec::new(),
        }
    }
}

/// Why there is no application to talk to — **five answers, never collapsed**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceError {
    /// No live application published itself. `hint` is the `--instance` value that
    /// matched nothing, quoted back so a stale pid in an agent's config is
    /// diagnosable.
    NoneRunning { hint: Option<String> },
    /// One or more applications are running, and none of them has the requested
    /// workspace open.
    WorkspaceNotOpen {
        workspace: String,
        /// The pids that *are* running, so the refusal can say what is open
        /// instead. Never the workspaces of those windows — a repository the user
        /// did not scope this agent to is not this interface's to reveal.
        running: Vec<u32>,
    },
    /// Several windows have the requested workspace open. **Refused, never
    /// resolved.**
    Ambiguous {
        workspace: String,
        candidates: Vec<u32>,
    },
    /// A build speaking a wire this one does not.
    VersionMismatch { pid: u32, protocol: u32 },
    /// The registry names a pipe the pid does not imply — so something other than
    /// this application wrote that entry. **Refused, never corrected.**
    PipeNameMismatch { pid: u32, stated: String },
}

impl InstanceError {
    /// A machine-matchable code. Five variants, five codes.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoneRunning { .. } => "app_not_running",
            Self::WorkspaceNotOpen { .. } => "workspace_not_open",
            Self::Ambiguous { .. } => "several_windows",
            Self::VersionMismatch { .. } => "protocol_mismatch",
            Self::PipeNameMismatch { .. } => "build_registry_tampered",
        }
    }

    /// The sentence an agent reads, naming the cause **and** what would fix it.
    pub fn sentence(&self) -> String {
        match self {
            Self::NoneRunning { hint: None } => "No code-basics application is running, so there \
                 is no build to run or read. Nothing was done. Start code-basics and open the \
                 repository this server is scoped to."
                .to_string(),
            Self::NoneRunning { hint: Some(hint) } => format!(
                "No running code-basics application matches --instance {hint}. Nothing was done. \
                 That value is a process id and a previous one does not come back: either drop the \
                 flag so the single application with this workspace open is used, or ask again \
                 without it."
            ),
            Self::WorkspaceNotOpen { workspace, running } => {
                let list = running
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "code-basics is running (pids {list}), but no window has the workspace \
                     {workspace:?} open, so its build is not available. Nothing was done. Open \
                     that repository in code-basics, or install this MCP server scoped to a \
                     repository that is open."
                )
            }
            Self::Ambiguous {
                workspace,
                candidates,
            } => {
                let list = candidates
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "Several code-basics windows have the workspace {workspace:?} open (pids \
                     {list}), so which one you meant is genuinely unknown. Nothing was done and \
                     nothing was chosen. Ask the user which window, then pass --instance <pid> — \
                     or close the windows you do not mean."
                )
            }
            Self::VersionMismatch { pid, protocol } => format!(
                "code-basics (pid {pid}) speaks build protocol {protocol} and this MCP server \
                 speaks {PROTOCOL_VERSION}, so nothing was sent. The running application and the \
                 executable in the MCP configuration are different builds. Reinstall the Build MCP \
                 entry from the running application, or restart it from the same build."
            ),
            Self::PipeNameMismatch { pid, stated } => format!(
                "The code-basics instance registry names the control pipe \"{stated}\" for pid \
                 {pid}, which is not the name that pid implies. Something other than code-basics \
                 wrote that entry, so nothing was sent: opening it could have connected this agent \
                 to another program answering as the build server. Close code-basics, delete \
                 build-instances.json from your user config directory, and start it again."
            ),
        }
    }
}

/// Where the registry lives, with `CB_BUILD_INSTANCES_PATH` overriding the whole
/// path — the escape hatch that makes the pipe and the shim drivable by hand.
pub fn instances_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CB_BUILD_INSTANCES_PATH") {
        return PathBuf::from(path);
    }

    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));

    base.join("code-basics").join(INSTANCES_FILE)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Read the registry. A missing or unparseable file yields **no instances** rather
/// than an error — see the module docs.
pub fn load(path: &Path) -> InstancesFile {
    let Ok(text) = std::fs::read_to_string(path) else {
        return InstancesFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Write the registry atomically: temp file then rename.
pub fn save(path: &Path, file: &InstancesFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(file).context("failed to serialise the registry")?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, format!("{json}\n"))
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

/// Add `instance`, replacing any entry for the same pid.
pub fn upsert(file: &mut InstancesFile, instance: BuildInstance) {
    match file.instances.iter_mut().find(|e| e.pid == instance.pid) {
        Some(existing) => *existing = instance,
        None => file.instances.push(instance),
    }
}

/// Drop the entry for `pid`, if there is one.
pub fn remove(file: &mut InstancesFile, pid: u32) {
    file.instances.retain(|entry| entry.pid != pid);
}

/// Pick the application to talk to, or say why there is none.
///
/// The order of the checks is a decision: liveness, then protocol, then the
/// workspace, then ambiguity. Each earlier answer would make the later checks
/// meaningless.
pub fn choose_instance<'a>(
    file: &'a InstancesFile,
    workspace: &str,
    hint: Option<&str>,
    alive: &dyn Fn(&BuildInstance) -> bool,
) -> Result<&'a BuildInstance, InstanceError> {
    let hint = hint.map(str::trim).filter(|h| !h.is_empty());

    let named: Vec<&BuildInstance> = match hint {
        Some(wanted) => file
            .instances
            .iter()
            .filter(|entry| entry.pid.to_string() == wanted)
            .collect(),
        None => file.instances.iter().collect(),
    };

    let none_running = || InstanceError::NoneRunning {
        hint: hint.map(str::to_string),
    };

    let live: Vec<&BuildInstance> = named.into_iter().filter(|entry| alive(entry)).collect();
    if live.is_empty() {
        return Err(none_running());
    }

    let speakable: Vec<&BuildInstance> = live
        .iter()
        .copied()
        .filter(|entry| entry.protocol == PROTOCOL_VERSION)
        .collect();
    if speakable.is_empty() {
        let odd = live[0];
        return Err(InstanceError::VersionMismatch {
            pid: odd.pid,
            protocol: odd.protocol,
        });
    }

    let matching: Vec<&BuildInstance> = speakable
        .iter()
        .copied()
        .filter(|entry| entry.has_workspace(workspace))
        .collect();

    match matching.as_slice() {
        [] => Err(InstanceError::WorkspaceNotOpen {
            workspace: workspace.to_string(),
            running: speakable.iter().map(|entry| entry.pid).collect(),
        }),
        [only] => {
            let stated = only.listener.pipe.as_str();
            if stated != pipe_name(only.pid) {
                return Err(InstanceError::PipeNameMismatch {
                    pid: only.pid,
                    stated: stated.to_string(),
                });
            }
            Ok(only)
        }
        several => Err(InstanceError::Ambiguous {
            workspace: workspace.to_string(),
            candidates: several.iter().map(|entry| entry.pid).collect(),
        }),
    }
}

#[cfg(test)]
#[path = "instances_tests.rs"]
mod tests;
