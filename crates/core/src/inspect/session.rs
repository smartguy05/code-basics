//! The decisions that surround one capture.
//!
//! [`super::sidecar`] knows how to *call* the inspector; this module knows what
//! the application decides before and after that call — which architecture to
//! try first, what limits the workspace asked for, what to name the session,
//! and what an honest answer looks like when there is no inspector to call at
//! all.
//!
//! It lives here rather than in the Tauri layer for the usual reason: none of
//! it needs a window, and all of it is worth a test. What genuinely cannot move
//! is resolving the bundled resource directory, which only the app handle
//! knows — so every function here takes that directory as an argument.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use std::collections::{BTreeMap, BTreeSet};

use super::model::{
    AttachableProcess, Attribution, Bitness, DotnetProcess, DumpFile, ElidedReason, InspectRequest,
    InspectStatus, InspectTarget, RootSpec, RunDump,
};
use super::{dumps, sidecar};
use crate::model::{RunConfig, RunKind};

// ---------------------------------------------------------------------------
// Naming a session
// ---------------------------------------------------------------------------

/// Distinguishes sessions started within the same millisecond.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh session directory name.
///
/// Time first so a directory listing reads in the order captures happened, and
/// a counter after it because expanding a node fires captures fast enough to
/// land in the same millisecond. There is no uuid dependency in this workspace
/// and this does not warrant adding one: the name only has to be unique within
/// one workspace directory, not across machines.
pub fn new_session_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{millis}-{seq:04}")
}

// ---------------------------------------------------------------------------
// Building the request
// ---------------------------------------------------------------------------

/// The request for a capture, with the workspace's own caps applied.
///
/// A configuration file that cannot be read falls back to the defaults rather
/// than failing the capture: unreadable settings are a reason to inspect
/// something, not a reason to refuse to.
///
/// `widen` names the cap that stopped the *previous* read of this branch, when
/// there was one. Without it a re-read rooted at the same object would be run
/// under the same limits and return the same truncation — an expand that
/// expands nothing while telling the user it fetched something. See
/// [`crate::inspect::Caps::widened`].
pub fn request_for(
    root: &Path,
    target: InspectTarget,
    spec: RootSpec,
    widen: Option<ElidedReason>,
) -> InspectRequest {
    let caps = crate::config::load(root)
        .unwrap_or_default()
        .inspector_caps();
    let caps = match widen {
        Some(reason) => caps.widened(reason),
        None => caps,
    };
    InspectRequest::new(target, spec, caps)
}

// ---------------------------------------------------------------------------
// Attaching to a running process
// ---------------------------------------------------------------------------

/// How far a parent chain is followed before it is abandoned as nonsense.
///
/// Parent pids are another process's account of the world, read at a moment
/// that has already passed. They can disagree with themselves — a cycle, a
/// process that is its own parent — and a walk that trusted them would hang the
/// poll that fills the picker. A real ancestry is a handful of links; anything
/// past this is not one.
const MAX_ANCESTRY: usize = 64;

/// What the parent chain reached, if anything.
enum Origin<'a> {
    /// The nearest ancestor the supervisor launched for a run configuration.
    Launched { pid: u32, config: &'a RunConfig },
    /// The chain reached something code-basics started that is not the user's
    /// application: a build, a git fetch, the inspector's own sidecar.
    Internal,
    /// Nothing known — the chain ran out, left the set of processes we were
    /// given, or contradicted itself.
    Unknown,
}

/// Which supervisor entries are the user's own applications, by pid.
///
/// The supervisor's map holds more than the user's applications. A build
/// registers under `<config_id>:build`, a git fetch under `git:network`, and
/// the inspector's own sidecar under `inspect:<session>`. The filter is
/// therefore positive rather than a list of prefixes to exclude: an id counts
/// only when it *is* the id of a run configuration, so a supervisor id invented
/// tomorrow is excluded by default rather than by remembering to add it here.
///
/// A compound configuration never runs a process of its own — its members
/// register under their own ids and are matched here on their own account.
///
/// An entry with no pid is skipped: it was spawned and has already gone.
fn launched_apps<'a>(
    running: &[(String, Option<u32>)],
    configs: &'a [RunConfig],
) -> BTreeMap<u32, &'a RunConfig> {
    running
        .iter()
        .filter_map(|(id, pid)| {
            let pid = (*pid)?;
            let config = configs
                .iter()
                .find(|c| c.id == *id && c.compound.is_empty())?;
            Some((pid, config))
        })
        .collect()
}

/// Supervisor pids that are code-basics' own machinery rather than the user's
/// application.
///
/// These are not merely uninteresting, they are actively harmful to offer. The
/// inspector's sidecar is itself a .NET process, so it now appears in the
/// published list, and inspecting the inspector is absurd. A build host is a
/// compiler nobody is debugging. Both — and everything they spawned — are
/// dropped from the list entirely rather than shown as `Unrelated`, because
/// they are noise this application put there.
fn internal_pids(running: &[(String, Option<u32>)], configs: &[RunConfig]) -> BTreeSet<u32> {
    let apps = launched_apps(running, configs);
    running
        .iter()
        .filter_map(|(_, pid)| *pid)
        .filter(|pid| !apps.contains_key(pid))
        .collect()
}

/// Our own sidecar, recognised by name as well as by pid.
///
/// The pid check above is authoritative for a sidecar *this* window started.
/// A capture running in another window of this application is not in this
/// supervisor's map at all, and enumerating the machine finds it anyway. The
/// name is a fact about a binary this repository builds, not a guess about the
/// user's code; the cost of a false match is one missing entry, never a wrong
/// one.
fn is_inspector(process: &DotnetProcess) -> bool {
    process.name.starts_with("cb-inspector")
}

/// A moment, as a number that can only be compared with another one from here.
///
/// There is no date library in this workspace and this does not warrant adding
/// one: the only question asked of these values is "did this happen before
/// that", both sides come from the same machine through the same API, and both
/// are written by the sidecar as round-trip UTC (`2026-08-06T13:35:02.1230000Z`).
///
/// Anything that is not unambiguously UTC — an offset, a missing `Z`, a field
/// that will not parse — is `None`, which every caller reads as *unknown* and
/// therefore as no evidence at all. A time that was misread would be worse than
/// no time: it would refuse a link that is real.
fn instant(text: &str) -> Option<u128> {
    let text = text.trim();
    let rest = text.strip_suffix('Z').or_else(|| text.strip_suffix('z'))?;
    let split = rest.find(['T', 't'])?;
    let (date, time) = (&rest[..split], &rest[split + 1..]);

    let mut parts = date.split('-');
    let year: u128 = parts.next()?.parse().ok()?;
    let month: u128 = parts.next()?.parse().ok()?;
    let day: u128 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || month > 12 || day > 31 {
        return None;
    }

    let (clock, fraction) = match time.split_once('.') {
        Some((clock, fraction)) => (clock, fraction),
        None => (time, ""),
    };
    let mut parts = clock.split(':');
    let hour: u128 = parts.next()?.parse().ok()?;
    let minute: u128 = parts.next()?.parse().ok()?;
    let second: u128 = parts.next()?.parse().ok()?;
    // A leap second is 60; anything past that is not a clock reading.
    if parts.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return None;
    }

    // Padded to a fixed width so "1" and "1000000" compare as tenths and
    // whole seconds rather than as one and a million.
    if !fraction.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut digits: String = fraction.chars().take(9).collect();
    while digits.len() < 9 {
        digits.push('0');
    }
    let nanos: u128 = digits.parse().ok()?;

    // Month and day are not normalised into days — they do not need to be. The
    // encoding is monotonic for any two valid calendar readings, which is the
    // only property a comparison needs.
    let ordinal = ((((year * 13 + month) * 32 + day) * 24 + hour) * 60 + minute) * 60 + second;
    Some(ordinal * 1_000_000_000 + nanos)
}

/// Whether a claimed parent link cannot be true, because the child was already
/// running when the parent started.
///
/// This is the pid-recycling defence, one link further along than the one
/// [`live_target_reason`] makes. A parent pid is a *number*, and Windows hands
/// numbers out again readily under build and test churn: a long-lived service
/// whose real parent exited months ago still reports that dead pid, and the
/// moment the operating system reuses it for a `dotnet run` the service would
/// otherwise be adopted into the user's configuration — sorted under their
/// application's name, preselected, and captured. A stranger's heap under the
/// user's own label is precisely the wrong value this module exists to avoid.
///
/// Only an actual contradiction refuses. Either time missing or unparsable is
/// no evidence, and no evidence never overrides a link.
fn contradicts_parent(child: &DotnetProcess, parent: &DotnetProcess) -> bool {
    match (
        child.started_at.as_deref().and_then(instant),
        parent.started_at.as_deref().and_then(instant),
    ) {
        (Some(child), Some(parent)) => child < parent,
        _ => false,
    }
}

/// Follow a process's parent chain until it explains itself or runs out.
///
/// Four rules, each of them the abstaining choice:
///
/// * **Terminate.** The links come from another process's view of the world and
///   can contain a cycle or a self-parent, so every pid seen is remembered and
///   the walk is bounded regardless.
/// * **Do not cross into someone else's tree.** A parent that is not in the set
///   of processes we were given is not evidence of anything — the walk stops
///   rather than assuming what lies beyond it. A pid the supervisor launched is
///   checked *first*, because that is knowledge rather than assumption: it is
///   what this application did, and the launcher need not be a .NET process at
///   all.
/// * **Missing means unknown.** A process with no `parent_pid` is not a process
///   with no parent; the chain simply ends, unattributed.
/// * **A child cannot predate its parent.** Where both start times are known
///   and say otherwise, the pid has been recycled and the link is fiction — see
///   [`contradicts_parent`].
fn origin<'a>(
    process: &DotnetProcess,
    by_pid: &BTreeMap<u32, &DotnetProcess>,
    launched: &BTreeMap<u32, &'a RunConfig>,
    internal: &BTreeSet<u32>,
) -> Origin<'a> {
    let mut seen = BTreeSet::from([process.pid]);
    let mut child = process;
    let mut next = process.parent_pid;

    for _ in 0..MAX_ANCESTRY {
        let Some(pid) = next else {
            return Origin::Unknown;
        };
        if !seen.insert(pid) {
            // The chain has looped. Nothing beyond here can be believed.
            return Origin::Unknown;
        }
        // The claimed parent's own row, when it published one. Checked before
        // the link is believed, including on the launched pid: that is the one
        // that hands out a configuration name.
        let parent = by_pid.get(&pid).copied();
        if let Some(parent) = parent {
            if contradicts_parent(child, parent) {
                return Origin::Unknown;
            }
        }
        if let Some(config) = launched.get(&pid) {
            // The nearest launched ancestor wins: a chain that could reach two
            // of them belongs to the one it came out of most recently.
            return Origin::Launched { pid, config };
        }
        if internal.contains(&pid) {
            return Origin::Internal;
        }
        let Some(parent) = parent else {
            return Origin::Unknown;
        };
        child = parent;
        next = parent.parent_pid;
    }

    Origin::Unknown
}

/// Work out which of the machine's .NET processes are the user's application.
///
/// This is the answer to the question the picker actually asks. Enumerating
/// every published .NET process is not the same as knowing whose it is, and the
/// naive reading — "the pid the supervisor holds is the application" — is
/// reliably *wrong* for the dominant .NET case: `dotnet run` builds the project
/// and then starts the application as a child, so the supervisor's pid is the
/// CLI, whose heap contains none of the user's objects. A capture of it returns
/// an empty tree that reads exactly like "your object is not there".
///
/// So each process is labelled with the evidence linking it to a configuration
/// — [`Attribution::Launched`] for the pid this application started,
/// [`Attribution::Descendant`] for one whose parent chain reaches it, and
/// [`Attribution::Unrelated`] for everything else, which carries no
/// configuration at all rather than a plausible one.
///
/// `processes` is what the inspector enumerated, `running` is the supervisor's
/// map, `configs` the workspace's configurations. Ordered with the user's own
/// applications first, then by name and pid, so the menu does not reshuffle
/// between polls.
pub fn attribute(
    processes: &[DotnetProcess],
    running: &[(String, Option<u32>)],
    configs: &[RunConfig],
) -> Vec<AttachableProcess> {
    let launched = launched_apps(running, configs);
    let internal = internal_pids(running, configs);
    let by_pid: BTreeMap<u32, &DotnetProcess> = processes.iter().map(|p| (p.pid, p)).collect();

    // Classified first, so the launcher caveats below can be written from the
    // finished picture rather than from a guess about what else is running.
    struct Row<'a> {
        process: &'a DotnetProcess,
        attribution: Attribution,
        config: Option<&'a RunConfig>,
        /// The launched pid a descendant came out of.
        via: Option<u32>,
    }

    let mut rows: Vec<Row> = Vec::new();
    for process in processes {
        if internal.contains(&process.pid) || is_inspector(process) {
            continue;
        }
        if let Some(config) = launched.get(&process.pid) {
            rows.push(Row {
                process,
                attribution: Attribution::Launched,
                config: Some(config),
                via: None,
            });
            continue;
        }
        match origin(process, &by_pid, &launched, &internal) {
            Origin::Launched { pid, config } => rows.push(Row {
                process,
                attribution: Attribution::Descendant,
                config: Some(config),
                via: Some(pid),
            }),
            // A build's worker nodes and the inspector's own children are this
            // application's noise, not the machine's.
            Origin::Internal => continue,
            Origin::Unknown => rows.push(Row {
                process,
                attribution: Attribution::Unrelated,
                config: None,
                via: None,
            }),
        }
    }

    // What each launched pid started that is on this same list. Collected whole
    // rather than reduced to one, because a `dotnet run` can leave MSBuild
    // worker nodes running beside the application and picking the "real" one out
    // of several would be the guess this module refuses to make.
    let mut children: BTreeMap<u32, Vec<&DotnetProcess>> = BTreeMap::new();
    for row in &rows {
        if let Some(pid) = row.via {
            children.entry(pid).or_default().push(row.process);
        }
    }

    // One verdict per launched pid, so the caveat on a launcher and the mark on
    // the child it points at come from the same reading of the tree. Two
    // separate derivations could disagree, and the disagreement would be a
    // warning naming one process beside a preselection aimed at another.
    let verdicts: BTreeMap<u32, LauncherVerdict> = rows
        .iter()
        .filter_map(|row| match (row.attribution, row.config) {
            (Attribution::Launched, Some(config)) => Some((
                row.process.pid,
                launcher_verdict(
                    row.process,
                    config,
                    children
                        .get(&row.process.pid)
                        .map(|kids| kids.as_slice())
                        .unwrap_or_default(),
                ),
            )),
            _ => None,
        })
        .collect();

    let mut found: Vec<AttachableProcess> = rows
        .iter()
        .map(|row| {
            let verdict = verdicts.get(&row.process.pid);
            AttachableProcess {
                pid: row.process.pid,
                name: row.process.name.clone(),
                path: row.process.path.clone(),
                attribution: row.attribution,
                config_id: row.config.map(|c| c.id.clone()),
                config_name: row.config.map(|c| c.name.clone()),
                is_application: match row.attribution {
                    // Nothing observed says this is anything but the
                    // application: it is the pid this application started, and
                    // no evidence marks it as a launcher.
                    Attribution::Launched => verdict.is_none_or(|v| v.caveat.is_none()),
                    // Only the one child its launcher could actually name. A
                    // launcher that had to abstain marks nothing, so a build
                    // server is never offered as somebody's application.
                    Attribution::Descendant => {
                        row.via
                            .and_then(|via| verdicts.get(&via))
                            .and_then(|v| v.application_child)
                            == Some(row.process.pid)
                    }
                    Attribution::Unrelated => false,
                },
                launcher_caveat: verdict.and_then(|v| v.caveat.clone()),
            }
        })
        .collect();

    found.sort_by(|a, b| {
        ours_first(a.attribution)
            .cmp(&ours_first(b.attribution))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.pid.cmp(&b.pid))
    });
    found
}

/// Sort key putting the user's own applications above the rest of the machine.
fn ours_first(attribution: Attribution) -> u8 {
    match attribution {
        Attribution::Launched | Attribution::Descendant => 0,
        Attribution::Unrelated => 1,
    }
}

/// Whether the pid the supervisor holds is the .NET CLI rather than the user's
/// application.
///
/// Two facts, both observed rather than inferred: the configuration is one
/// [`crate::adapters::dotnet::run_invocation`] builds as `dotnet run …`, and the
/// process actually running under that pid is `dotnet`. A configuration whose
/// pid turns out to be the application itself — a published apphost, an
/// executable named after the project — fails the second and is left alone.
fn is_dotnet_cli(process: &DotnetProcess, config: &RunConfig) -> bool {
    config.compound.is_empty()
        && config.kind == RunKind::App
        && config.ecosystem == "dotnet"
        && process.name.eq_ignore_ascii_case("dotnet")
}

/// Whether a process is one the .NET SDK starts for a build or a test run
/// rather than something anyone is debugging.
///
/// This exists to stop a build server being named as somebody's application. A
/// `dotnet run` starts the compiler server and MSBuild worker nodes before it
/// starts anything of the user's, all of them publish diagnostics channels, and
/// they outlive the build — so during the build phase the CLI's only listed
/// child is one of these.
///
/// `dotnet` is in the list even though a project built with `UseAppHost=false`
/// runs as `dotnet exec app.dll` and would be named that too. That case is
/// genuinely ambiguous, and the cost of the two answers is not symmetric:
/// abstaining loses a pointer the user can still find by eye, while guessing
/// points them at an MSBuild node and calls it their application.
///
/// Matched on the whole name, never as a prefix, so `MSBuildRunner` — which
/// could well be somebody's application — is not swept up by it.
fn is_build_tool(process: &DotnetProcess) -> bool {
    const TOOLS: &[&str] = &[
        "msbuild",
        "vbcscompiler",
        "csc",
        "vbc",
        "dotnet",
        "testhost",
        "vstest.console",
        "datacollector",
    ];

    let name = process.name.to_ascii_lowercase();
    TOOLS.contains(&name.as_str()) || name.starts_with("testhost.")
}

/// What the observed process tree says about a pid the supervisor launched.
struct LauncherVerdict {
    /// Why this pid is not the process holding the user's objects.
    caveat: Option<String>,
    /// The one child this launcher can be said to have started *as the
    /// application*. `None` whenever that could only be guessed at.
    application_child: Option<u32>,
}

/// Whether a launched pid is a launcher, and if so which of its children the
/// user actually wants.
///
/// This is the one place that knows the difference between "code-basics started
/// your application" and "code-basics started something that started your
/// application", and everything it says comes from two observations: what the
/// configuration is built as, and what is on the process list.
///
/// The gate is [`is_dotnet_cli`], and it is a gate rather than one signal among
/// several. Having a .NET child is *not* evidence of being a launcher — an
/// ordinary application starts a worker, an out-of-process plugin host, a
/// `dotnet ef` — and a rule that read it that way would tell the user that the
/// pid holding their objects "built the project", then point them at the worker.
///
/// Past the gate there are three cases:
///
/// 1. Exactly one child, and it is not something the SDK starts for a build.
///    Then it can be named, and it is: this is the answer the whole feature
///    exists to give.
/// 2. Any other set of children — several, or a single build server. There is a
///    launcher, so the caveat is earned, but which process is the application
///    cannot be told from here and is not claimed.
/// 3. No children at all. The application has not published a diagnostics
///    channel yet; saying nothing would let the user capture the CLI and read an
///    empty heap as an answer.
fn launcher_verdict(
    process: &DotnetProcess,
    config: &RunConfig,
    children: &[&DotnetProcess],
) -> LauncherVerdict {
    if !is_dotnet_cli(process, config) {
        return LauncherVerdict {
            caveat: None,
            application_child: None,
        };
    }

    let opening = format!(
        "The process code-basics started for `{}` is a launcher: it built the project and then \
         ran the application as a separate child process. This pid is the launcher, so a capture \
         of it reads the launcher's heap and finds none of your objects — an empty result would \
         not be evidence that there are none.",
        config.name
    );

    match children {
        [] => LauncherVerdict {
            caveat: Some(format!(
                "This pid is the .NET CLI, which builds `{}` and then runs it as a separate child \
                 process. No child of it has published a diagnostics channel yet, so the \
                 application is still building or has only just started. A capture of this pid \
                 would read the CLI's heap and find none of your objects. Refresh in a moment and \
                 pick the process named after your project.",
                config.name
            )),
            application_child: None,
        },
        [child] if !is_build_tool(child) => LauncherVerdict {
            caveat: Some(format!(
                "{opening} The application itself is `{}` (pid {}), also in this list; capture \
                 that one.",
                child.name, child.pid
            )),
            application_child: Some(child.pid),
        },
        several => {
            let started = if let [only] = several {
                format!(
                    "The one process it started that has published a channel is `{}`, which is \
                     also what the .NET SDK starts for a build",
                    only.name
                )
            } else {
                format!(
                    "It started {} processes that are still running, listed here under the same \
                     configuration",
                    several.len()
                )
            };
            LauncherVerdict {
                caveat: Some(format!(
                    "{opening} {started} — a build host can outlive the build alongside the \
                     application, so pick the one named after your project. If nothing here is, \
                     the application has not published a diagnostics channel yet; refresh in a \
                     moment."
                )),
                application_child: None,
            }
        }
    }
}

/// Why this pid is not the process holding the user's objects, or `None` when
/// there is no evidence that it is anything but the application.
///
/// The rule, and the reasoning behind every branch of it, is
/// [`launcher_verdict`]'s; this is the half of its answer that is shown to a
/// person.
pub fn launcher_caveat(
    process: &DotnetProcess,
    config: &RunConfig,
    children: &[&DotnetProcess],
) -> Option<String> {
    launcher_verdict(process, config, children).caveat
}

/// Why this live target must not be attached to, checked against what is
/// running *now*.
///
/// The pid in a capture request was read from a list at some earlier moment —
/// the frontend refreshes on mount, on becoming visible and on demand, not
/// continuously — so by the time Capture is pressed the process may have exited
/// and the operating system may have handed the number to something else.
/// Windows recycles pids readily under build and test churn, and the replacement
/// is very often another managed process, which attaches happily and renders a
/// stranger's heap under the user's configuration name. Re-enumerating the
/// machine's .NET processes immediately before spawning is what makes the label
/// on the capture true.
///
/// A dump target is never refused here: a file on disk is whatever it was when
/// it was written.
pub fn live_target_reason(
    target: &InspectTarget,
    attachable: &[AttachableProcess],
) -> Option<String> {
    let InspectTarget::Live { pid } = target else {
        return None;
    };
    if attachable.iter().any(|process| process.pid == *pid) {
        return None;
    }
    Some(format!(
        "Process {pid} is no longer one of the .NET processes on this machine, so nothing was \
         read from it. It has exited since this list was last refreshed, and the operating \
         system may already have given that number to an unrelated process — attaching anyway \
         would show you somebody else's memory under the name you picked. Refresh the list and \
         pick a process that is still running."
    ))
}

// ---------------------------------------------------------------------------
// Attributing a dump to a run
// ---------------------------------------------------------------------------

/// The dump a finished run may have written, with how sure that is.
///
/// `pid` is what the run itself reported, and it is the only evidence that
/// actually attributes a dump: a matching pid could not have come from anything
/// else. Everything else is a *candidate* — the dump environment is inherited by
/// every child process and applies to every other configuration running at the
/// same time, so with two applications up the newest dump since a run started is
/// as likely to be the other one's. That case is returned rather than hidden,
/// because a dump written during a crash is usually the one the reader wants,
/// but it is returned as `certain: false` so the caller can say "a dump was
/// written while this ran" instead of "this crashed and here is its dump".
///
/// `dumps` is expected newest first, as [`dumps::list`] returns it.
pub fn dump_for_run(dumps: &[DumpFile], pid: Option<u32>, started_at: u64) -> Option<RunDump> {
    // Both arms require the dump to be no older than the run: nothing written
    // before the process existed can have come out of it.
    let fresh = |d: &&DumpFile| d.captured_at >= started_at;

    if let Some(pid) = pid {
        if let Some(dump) = dumps.iter().filter(fresh).find(|d| d.pid == pid) {
            return Some(RunDump {
                dump: dump.clone(),
                certain: true,
            });
        }
    }

    dumps.iter().find(fresh).map(|dump| RunDump {
        dump: dump.clone(),
        certain: false,
    })
}

/// Why this target and root cannot be served, before anything is spawned.
///
/// The check exists because attaching is not free: a live capture clones the
/// target's process image, so discovering the request was impossible *after*
/// paying that cost would charge the user's application for a mistake this
/// layer could see coming.
///
/// Only combinations that genuinely cannot work are refused. A root that merely
/// might find nothing — [`RootSpec::Exceptions`] against a healthy process — is
/// allowed and answers honestly with an empty result.
pub fn unsupported_reason(target: &InspectTarget, root: &RootSpec) -> Option<String> {
    match (target, root) {
        (InspectTarget::Live { .. }, RootSpec::CrashException) => Some(
            "A running process has not crashed, so there is no crash exception to read. \
             Choose `every live exception` to find one that was caught, or open a crash dump."
                .to_string(),
        ),
        _ => None,
    }
}

/// What a live attach costs the target, said plainly enough to decide on.
///
/// This is the one part of the feature that reaches into a process the user
/// cares about while it is serving traffic, and the cost is not intuitive: a
/// snapshot is not free just because the application keeps running. Stating it
/// where the choice is made is the whole point — a warning that arrives after
/// the pause has already happened informs nobody.
pub fn attach_caveats() -> Vec<String> {
    vec![
        "Attaching to a running process copies its memory image. The application keeps \
         running, but it pauses briefly and its memory use roughly doubles for the moment \
         of the copy — worth knowing before doing it to something serving traffic."
            .to_string(),
        "The list holds every .NET process on this machine that can be attached to, not only \
         the ones code-basics started. Each says how it was linked to a run configuration, and \
         a process this application did not start is labelled as unrelated rather than given \
         somebody's configuration name."
            .to_string(),
    ]
}

// ---------------------------------------------------------------------------
// Choosing an executable
// ---------------------------------------------------------------------------

/// Which sidecar to try first, or `None` when neither build is present.
///
/// x64 first because almost every target is; the x86 build is only reached
/// when it is the only one installed, or when [`sidecar::next_attempt`] asks
/// for it after the target reported a mismatch.
pub fn first_bitness(bundled_dir: Option<&Path>) -> Option<Bitness> {
    [Bitness::X64, Bitness::X86]
        .into_iter()
        .find(|b| sidecar::resolve(bundled_dir, *b).is_some())
}

/// Why there is nothing to run, phrased so the reader can act on it.
///
/// The sidecar is a separate .NET build that is not produced by `cargo build`,
/// so its absence is an ordinary state of a fresh checkout rather than a
/// broken installation — and the message has to say so, and say what to type.
pub fn missing_sidecar_reason() -> String {
    format!(
        "The object inspector is not installed. It is a separate .NET component: run \
         `pnpm sidecar:build` to publish {} next to the app, or point CB_INSPECTOR_PATH \
         at an existing build.",
        sidecar::sidecar_file_name(Bitness::X64)
    )
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// What the inspector can do for this workspace right now.
///
/// Every field is answered from something observed — a file that exists, a
/// setting written down, a dump on disk. Nothing here is inferred, because the
/// one thing this must never do is imply a capability that will then fail in
/// front of the user.
pub fn status(root: &Path, bundled_dir: Option<&Path>) -> InspectStatus {
    let available = first_bitness(bundled_dir).is_some();
    let config = crate::config::load(root).unwrap_or_default();
    let dumps = dumps::list(&dumps::dumps_dir(root));

    InspectStatus {
        available,
        unavailable_reason: (!available).then(missing_sidecar_reason),
        dump_capture_enabled: config.dump_capture_enabled(),
        caveats: caveats(config.dump_capture_enabled(), !dumps.is_empty()),
        attach_caveats: attach_caveats(),
        dumps,
    }
}

/// Things the user should know before relying on this.
///
/// Worded like the intent providers' caveats: a plain statement of what is
/// true, and where it matters, what to do about it.
fn caveats(capture_enabled: bool, dumps_on_disk: bool) -> Vec<String> {
    let mut caveats = Vec::new();

    // The memory warning is unconditional. This application manages .NET user
    // secrets, so the processes it runs demonstrably hold connection strings
    // and tokens — and a dump is a verbatim copy of all of it. Stating that
    // only once capture is switched on would be a warning that arrives after
    // the decision it was meant to inform.
    caveats.push(if capture_enabled || dumps_on_disk {
        "A crash dump is a verbatim copy of process memory — connection strings, tokens \
         and customer data are in it. Dumps are written to .code-basics/dumps/, which is \
         git-ignored, but they remain readable on this machine until they are pruned."
            .to_string()
    } else {
        "Crash dump capture is off for this workspace. Turning it on writes a verbatim copy \
         of process memory to disk — connection strings, tokens and customer data included."
            .to_string()
    });

    // The two limits a user coming from a debugger will otherwise assume away.
    caveats.push(
        "Values are read from memory, never executed: no method is called and no property \
         is evaluated, so a computed property shows as the fields behind it."
            .to_string(),
    );

    // The cost of the live path, stated wherever the inspector describes
    // itself. It is carried in the existing `caveats` list rather than as a new
    // field: `available` already answers whether an attach is possible at all —
    // it is the same sidecar either way — and `inspect_attachable` returning
    // nothing already answers "there is no process to attach to".
    caveats.extend(attach_caveats());

    if capture_enabled {
        caveats.push(
            "A dump is only written when a process crashes with an unhandled exception. A \
             caught exception, and a run you stop from the toolbar, leave nothing behind."
                .to_string(),
        );
        // Deliberately not phrased as "dumps are matched to a run": nothing
        // performs that matching. Every dump in the workspace is offered, and
        // the executable name in each filename is what the reader uses to tell
        // their application's dump from its build host's.
        caveats.push(
            "The dump settings are inherited by every child process, so a `dotnet run` arms \
             its build host too. Every dump in the workspace is listed, labelled with the \
             executable that wrote it — pick the one naming your application."
                .to_string(),
        );
    }

    caveats
}

// ---------------------------------------------------------------------------
// Housekeeping
// ---------------------------------------------------------------------------

/// Arm crash-dump capture for a run, or report that it is off.
///
/// Called before every .NET run rather than only from the Objects tab, because
/// the workspace that most needs a bound on disk use is the one that crashes
/// repeatedly and is never inspected: pruning only after a capture means the
/// byte budget is consulted only by users who already came looking, long after
/// the disk filled. Every run is the moment the next dump might be written, so
/// every run is where the previous ones are trimmed.
///
/// `None` means nothing is armed — the workspace has not opted in, or the
/// directory could not be created. A diagnostic aid that cannot be set up is
/// never a reason to refuse to start the user's application.
pub fn arm_dumps(root: &Path) -> Option<ArmedDumps> {
    let config = crate::config::load(root).unwrap_or_default();
    if !config.dump_capture_enabled() {
        return None;
    }

    let dir = dumps::dumps_dir(root);
    std::fs::create_dir_all(&dir).ok()?;
    // Dumps must never reach a shared history; every feature under
    // `.code-basics/` ensures this itself as it first writes there.
    let _ = crate::config::ensure_gitignore(&crate::config::config_dir(root));

    // What is already on disk, before adding to it.
    prune_dumps(root, &config);

    Some(ArmedDumps {
        dir,
        env: config
            .inspector
            .as_ref()
            .map(|i| i.env.clone())
            .unwrap_or_default(),
    })
}

/// What a run needs in order to write dumps this workspace asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmedDumps {
    /// Where the runtime should write them.
    pub dir: std::path::PathBuf,
    /// `inspector.env` from the workspace configuration, layered over the
    /// built-in `DOTNET_Dbg*` defaults and under the run configuration's own
    /// environment. This is the documented way to ask for a different dump
    /// type, and it is only honoured for runs that are actually capturing.
    pub env: BTreeMap<String, String>,
}

/// Trim both kinds of accumulated state after a capture.
///
/// Sessions and dumps are pruned together because they accumulate together,
/// and both hold somebody's data. Failures are returned rather than raised:
/// housekeeping must never turn a successful capture into an error.
pub fn prune(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();

    if let Err(e) =
        sidecar::retain_newest(&sidecar::sessions_dir(root), sidecar::DEFAULT_KEEP_SESSIONS)
    {
        problems.push(format!("could not prune inspector sessions: {e:#}"));
    }

    let config = crate::config::load(root).unwrap_or_default();
    problems.extend(prune_dumps(root, &config));

    problems
}

/// Apply the workspace's retention to everything a dump collector has written.
///
/// One budget covers both places dumps land: the ones the runtime names, in
/// `.code-basics/dumps/`, and the ones VSTest's blame collector drops into
/// `.code-basics/results/` under names this app cannot decode. The named dumps
/// are served first because they are the ones the Objects tab can open;
/// whatever budget survives them is what the unnamed ones get.
fn prune_dumps(root: &Path, config: &crate::config::WorkspaceConfig) -> Vec<String> {
    let mut problems = Vec::new();

    let budget = config.max_dump_megabytes().saturating_mul(1024 * 1024);
    let dumps_dir = dumps::dumps_dir(root);
    if let Err(e) = dumps::prune(&dumps_dir, config.keep_dumps() as usize, Some(budget)) {
        problems.push(format!("could not prune crash dumps: {e:#}"));
    }

    let kept: u64 = dumps::list(&dumps_dir).iter().map(|d| d.bytes).sum();
    if let Err(e) = dumps::prune_unnamed(
        &crate::config::results_dir(root),
        budget.saturating_sub(kept),
    ) {
        problems.push(format!("could not prune test-host crash dumps: {e:#}"));
    }

    problems
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod session_tests;
