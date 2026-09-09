//! Which running application an MCP browser client should talk to — and the
//! six different reasons there might not be one.
//!
//! The browser MCP server is a **separate process** (`cb-app mcp-browser`,
//! self-dispatched exactly as `record-intent`, `quality-gate` and `mcp-sql`
//! are) started by an agent, not by the user. It has no window, no `AppState`
//! and no way to know whether an application is running at all, so the
//! application publishes what a client needs into a small user-global registry
//! and this module is everything that is decided about it.
//!
//! # The registry is durable facts only, and liveness is never one of them
//!
//! An entry says: a pid, the executable that wrote it, the protocol that build
//! speaks, whether the browser plugin is switched on, and — only while a panel
//! is open — the pipe to connect to and the token to present. It does **not**
//! say the process is alive, because a crash leaves the entry behind and a
//! recycled pid then names a stranger. So [`choose_instance`] takes liveness as
//! a parameter and the caller re-probes, comparing the executable path as well
//! as the pid: the reasoning [`crate::inspect::session`] uses before attaching
//! a debugger to somebody's process, for the same reason.
//!
//! # The listener exists only while a panel is open
//!
//! This is what makes five of the six refusals reachable from durable facts, and it is
//! also the smaller attack surface: with no browser panel open there is no pipe
//! on the machine at all.
//!
//! | what is true | entry | answer |
//! |---|---|---|
//! | no application running | none, or none live | [`InstanceError::NoneRunning`] |
//! | plugin switched off | `browserFeature: false` | [`InstanceError::PluginDisabled`] |
//! | plugin on, panel never opened or closed again | `listener: null` | [`InstanceError::PanelClosed`] |
//! | two panels open | two listeners | [`InstanceError::Ambiguous`] |
//! | a build that speaks a different wire | `protocol` differs | [`InstanceError::VersionMismatch`] |
//! | the listener names a pipe the pid does not imply | `listener.pipe` differs from [`pipe_name`] | [`InstanceError::PipeNameMismatch`] |
//! | one panel open | one listener | [`Ok`] |
//!
//! The last is the one refusal that is **not** about an ordinary state: it means
//! something other than this application wrote that entry. A client derives the
//! pipe name from the verified pid and would never have opened the stated one
//! anyway, so this could have been ignored silently — reporting it is the point,
//! because it is the only visible symptom of a local process trying to answer an
//! agent as if it were the browser.
//!
//! The one that matters most is **`Ambiguous` refuses and never picks**.
//! Driving a browser in a window the user is not looking at is worse than
//! asking, so there is deliberately no most-recent tie-break; the refusal names
//! the pids and the `--instance` flag that resolves it.
//!
//! # Why the file is tolerant
//!
//! Same rule as [`crate::notes`] and [`crate::enhancements::runs`]: a missing or
//! corrupt file loads as **no instances**, never as an error. A client reading a
//! damaged registry must still be able to start, list its tools and say "no
//! application is running" — an MCP server that fails to start is dropped
//! silently by its host and the user never learns why.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The wire version of everything in this module and on the pipe.
///
/// Bumped when a request or a reply changes shape. A client that finds only
/// entries carrying another number refuses with [`InstanceError::VersionMismatch`]
/// rather than speaking a protocol it does not know — the two processes are
/// separately updatable (an agent's config keeps running an old path until the
/// user reinstalls) so a disagreement is an ordinary condition, not a bug.
pub const PROTOCOL_VERSION: u32 = 1;

/// The registry file's name under the user config directory.
const INSTANCES_FILE: &str = "browser-instances.json";

/// Where an application publishes its pipe, and how a client presents itself.
///
/// Present **only while a browser panel is open**. Its absence is the
/// [`InstanceError::PanelClosed`] answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listener {
    /// The full pipe name, `\\.\pipe\code-basics.browser.<pid>`.
    ///
    /// **Diagnostic only. A client must never open this — it must open
    /// [`pipe_name`] of the verified pid.** This field was once the name the
    /// client opened, justified as keeping the name "in exactly one place, the
    /// process that created it". That was wrong, and dangerously so: the
    /// registry is an ordinary JSON file under the user's own config directory,
    /// so any local process that can write it can publish a pipe of *its* own
    /// under this application's pid and answer as the browser — fabricating
    /// page text, console lines and network rows straight into an agent's
    /// transcript. No user click is involved, and the consent model cannot
    /// help, because consent is enforced by the application and on that path
    /// the application is never reached.
    ///
    /// It is kept, rather than removed, because a value here that disagrees
    /// with the derived name is *evidence of tampering* and worth reporting —
    /// see [`InstanceError::PipeNameMismatch`]. Silently correcting it would
    /// throw that signal away.
    pub pipe: String,
    /// A per-launch random token a client presents on every request.
    ///
    /// **A speed bump, and it must be described as one.** It reduces "any local
    /// program" to "any program that can read this user's `%APPDATA%`", which is
    /// most of them. It grants nothing: no token authorises automation, and the
    /// worst a rogue local process achieves with the token and no user click is
    /// learning that a panel is open. The real control is
    /// [`super::consent`].
    pub token: String,
}

/// The control pipe's name for a given process id.
///
/// **The one rule for this name**, used by the application when it creates the
/// pipe and by a client when it opens one. A client derives it from a pid it has
/// already verified against [`BrowserInstance::exe`]; it must never open the
/// name a registry file merely *states* — see [`Listener::pipe`].
///
/// Per-pid rather than fixed because several application windows run at once,
/// each with its own state: a fixed name would make the second instance fail to
/// bind while the first answered for a codebase it does not have open.
pub fn pipe_name(pid: u32) -> String {
    format!(r"\\.\pipe\code-basics.browser.{pid}")
}

/// One running application, as far as a client can know it without connecting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInstance {
    pub pid: u32,
    /// The full path of the executable that wrote this entry, so a **recycled
    /// pid** can be told apart from this application.
    pub exe: String,
    pub protocol: u32,
    /// Whether the web browser plugin is switched on.
    ///
    /// Recorded even when it is `false`, which is the whole point: it is what
    /// lets a client distinguish "the user switched the browser off" from "no
    /// application is running" **without connecting to anything**.
    pub browser_feature: bool,
    pub listener: Option<Listener>,
    /// The workspace roots this window has open, for a refusal that has to name
    /// which window is which. Never used to *choose* — a path is not consent.
    #[serde(default)]
    pub workspaces: Vec<String>,
}

/// The registry: a version and the instances that published themselves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstancesFile {
    pub version: u32,
    #[serde(default)]
    pub instances: Vec<BrowserInstance>,
}

impl Default for InstancesFile {
    fn default() -> Self {
        Self {
            version: 1,
            instances: Vec::new(),
        }
    }
}

/// Why there is no application to talk to — **six answers, never collapsed**.
///
/// Collapsing them is the failure this codebase refuses, and here it is
/// concrete: "start the app", "switch the plugin on", "open the browser panel",
/// "tell me which window", "reinstall the MCP entry" and "something forged this
/// entry" are six different
/// things for the user to do, and an agent told only "unavailable" reports the
/// wrong one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceError {
    /// No live application published itself. `hint` is the `--instance` value
    /// that matched nothing, quoted back so a stale pid in an agent's config is
    /// diagnosable.
    NoneRunning { hint: Option<String> },
    /// Running, with the web browser plugin switched off.
    PluginDisabled { pid: u32 },
    /// Running with the plugin on, and no browser panel open — so there is no
    /// pipe, by design.
    PanelClosed { pid: u32 },
    /// Several panels are open. **Refused, never resolved.**
    Ambiguous { candidates: Vec<u32> },
    /// A build speaking a wire this one does not.
    VersionMismatch { pid: u32, protocol: u32 },
    /// The registry names a pipe the pid does not imply — so something other
    /// than this application wrote that entry.
    ///
    /// **Refused, never corrected.** The derived name is what a client opens
    /// regardless, so this could have been silently ignored; reporting it is
    /// the point, because it is the only visible symptom of a local process
    /// trying to impersonate the application to an agent.
    PipeNameMismatch { pid: u32, stated: String },
}

impl InstanceError {
    /// A machine-matchable code, so a model can branch without parsing prose.
    /// Six variants, six codes — a shared one would pass every other test.
    pub fn code(&self) -> &'static str {
        match self {
            Self::PipeNameMismatch { .. } => "browser_registry_tampered",
            Self::NoneRunning { .. } => "app_not_running",
            Self::PluginDisabled { .. } => "browser_plugin_off",
            Self::PanelClosed { .. } => "browser_panel_closed",
            Self::Ambiguous { .. } => "several_browser_panels",
            Self::VersionMismatch { .. } => "protocol_mismatch",
        }
    }

    /// The sentence an agent reads, naming the cause **and** what would fix it.
    pub fn sentence(&self) -> String {
        match self {
            Self::PipeNameMismatch { pid, stated } => format!(
                "The code-basics instance registry names the control pipe \"{stated}\" for pid \
                 {pid}, which is not the name that pid implies. Something other than code-basics \
                 wrote that entry, so nothing was sent: opening it could have connected this \
                 agent to another program answering as the browser. Close code-basics, delete \
                 browser-instances.json from your user config directory, and start it again."
            ),
            Self::NoneRunning { hint: None } => "No code-basics application is running, so there \
                 is no browser to look at. Nothing was read. Start code-basics and open a browser \
                 panel from the titlebar Plugins menu."
                .to_string(),
            Self::NoneRunning { hint: Some(hint) } => format!(
                "No running code-basics application matches --instance {hint}. Nothing was read. \
                 That value is a process id and a previous one does not come back: either drop \
                 the flag so the single running application is used, or ask for browser_status \
                 without it to see what is running."
            ),
            Self::PluginDisabled { pid } => format!(
                "code-basics (pid {pid}) is running with the web browser plugin switched off, so \
                 there is no page, no browser process and no cookie jar. Nothing was read. The \
                 user can switch it on in Settings under Optional features, and it then appears \
                 in the titlebar Plugins menu."
            ),
            // Phrased around what the registry actually says - that no control
            // pipe is on offer - rather than around a closed panel, because
            // there are two ways to get here and only one of them is a closed
            // panel. The rare one (the application could not create the pipe:
            // a squatter on the name, or a DACL that would not build) is named
            // rather than folded into the common one, since a user told to
            // "open the panel" while looking at an open panel has been sent to
            // do something they cannot do.
            Self::PanelClosed { pid } => format!(
                "code-basics (pid {pid}) is running with the web browser plugin on, but it is \
                 not offering a browser control pipe, so nothing was read. Almost always this \
                 means its browser panel is not open, which is one click away: the user opens \
                 it from the titlebar Plugins menu. If the panel is already open, the \
                 application could not create the pipe and its own log says why."
            ),
            Self::Ambiguous { candidates } => {
                let list = candidates
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "There are several code-basics windows with a browser panel open (pids {list}), \
                     so which page you meant is genuinely unknown. Nothing was read and nothing \
                     was chosen: driving a browser in a window the user is not looking at is worse \
                     than asking. Ask the user which window, then pass --instance <pid> — or close \
                     the panels you do not mean."
                )
            }
            Self::VersionMismatch { pid, protocol } => format!(
                "code-basics (pid {pid}) speaks browser protocol {protocol} and this MCP server \
                 speaks {PROTOCOL_VERSION}, so nothing was sent. The two are separately \
                 updatable: the running application and the executable in the MCP configuration \
                 are different builds. Reinstall the browser MCP entry from the running \
                 application, or restart it from the same build."
            ),
        }
    }
}

/// Where the registry lives.
///
/// The same base resolution as [`crate::notes::notes_path`] and
/// `features_path`, with `CB_BROWSER_INSTANCES_PATH` overriding the whole path
/// — which is the escape hatch that makes the pipe and the shim drivable by
/// hand.
pub fn instances_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CB_BROWSER_INSTANCES_PATH") {
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

/// Read the registry. A missing or unparseable file yields **no instances**
/// rather than an error — see the module docs.
pub fn load(path: &Path) -> InstancesFile {
    let Ok(text) = std::fs::read_to_string(path) else {
        return InstancesFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Write the registry atomically: temp file then rename, like
/// [`crate::notes::save`].
///
/// Atomic because two applications starting together both rewrite this file,
/// and a torn write would be read as *no instances* — which is the one wrong
/// answer that looks exactly like a correct one.
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
///
/// Replacing rather than appending, because a pid identifies a process and two
/// entries for one would make [`InstanceError::Ambiguous`] fire against a
/// single window.
pub fn upsert(file: &mut InstancesFile, instance: BrowserInstance) {
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
/// `hint` is an `--instance <pid>` value and narrows **first**, so a refusal
/// describes the instance the caller actually named rather than the fleet. An
/// empty hint is no hint.
///
/// `alive` receives the whole entry, not just the pid, so it can compare the
/// executable path — a pid is not identity.
///
/// The order of the checks is itself a decision: liveness, then protocol, then
/// the plugin, then the panel, then ambiguity. Each earlier answer would make
/// the later checks meaningless — there is no point reporting a closed panel on
/// a build whose entry this one cannot even read correctly.
pub fn choose_instance<'a>(
    file: &'a InstancesFile,
    hint: Option<&str>,
    alive: &dyn Fn(&BrowserInstance) -> bool,
) -> Result<&'a BrowserInstance, InstanceError> {
    let hint = hint.map(str::trim).filter(|h| !h.is_empty());

    let named: Vec<&BrowserInstance> = match hint {
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

    let live: Vec<&BrowserInstance> = named.into_iter().filter(|entry| alive(entry)).collect();
    if live.is_empty() {
        return Err(none_running());
    }

    let speakable: Vec<&BrowserInstance> = live
        .iter()
        .copied()
        .filter(|entry| entry.protocol == PROTOCOL_VERSION)
        .collect();
    if speakable.is_empty() {
        // Reported against the first entry rather than all of them: the fix is
        // the same for every one, and naming one build is clearer than a list.
        let odd = live[0];
        return Err(InstanceError::VersionMismatch {
            pid: odd.pid,
            protocol: odd.protocol,
        });
    }

    let enabled: Vec<&BrowserInstance> = speakable
        .iter()
        .copied()
        .filter(|entry| entry.browser_feature)
        .collect();
    if enabled.is_empty() {
        return Err(InstanceError::PluginDisabled {
            pid: speakable[0].pid,
        });
    }

    let listening: Vec<&BrowserInstance> = enabled
        .iter()
        .copied()
        .filter(|entry| entry.listener.is_some())
        .collect();
    match listening.as_slice() {
        [] => Err(InstanceError::PanelClosed {
            pid: enabled[0].pid,
        }),
        [only] => {
            // Last, deliberately: every earlier answer describes an ordinary
            // state a user can act on, and reporting tampering instead of
            // "the panel is closed" would bury the common case. By here there
            // is exactly one candidate and its pid is verified against its exe,
            // so a stated name that is not the derived one was written by
            // something else.
            let stated = only
                .listener
                .as_ref()
                .map(|listener| listener.pipe.as_str())
                .unwrap_or_default();
            if stated != pipe_name(only.pid) {
                return Err(InstanceError::PipeNameMismatch {
                    pid: only.pid,
                    stated: stated.to_string(),
                });
            }
            Ok(only)
        }
        several => Err(InstanceError::Ambiguous {
            candidates: several.iter().map(|entry| entry.pid).collect(),
        }),
    }
}

#[cfg(test)]
#[path = "instances_tests.rs"]
mod tests;
