//! Types the inspector shares with the frontend.
//!
//! Mirrored by hand in `src/ipc/types.ts`, like everything in
//! [`crate::model`]. The `tests` module at the bottom pins the exact JSON keys,
//! so a rename here fails a test that names the TypeScript file rather than
//! silently producing `undefined` in the UI.
//!
//! One rule here is load-bearing and unlike anything else in the app:
//! **heap addresses cross as hexadecimal strings, never as numbers.**
//!
//! Not because they must overflow a JavaScript number — today's Windows and
//! Linux user-mode addresses sit below 2^47 and would fit exactly. That is an
//! accident of current address-space layout rather than a guarantee, and it is
//! not worth betting correctness on, because an address is not decoration: it
//! is the identity used to expand a node and to recognise a cycle. An address
//! rounded on its way through JSON would open the wrong object and render a
//! stranger's fields under the user's variable name.
//!
//! Hex is also what every other .NET debugging tool prints, so what the user
//! sees can be pasted straight into SOS or WinDbg.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

// ---------------------------------------------------------------------------
// What was inspected
// ---------------------------------------------------------------------------

/// Which process architecture a sidecar (or a target) is.
///
/// ClrMD can only read a target of its own bitness, so this is carried
/// everywhere rather than assumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Bitness {
    X64,
    X86,
}

/// Where a capture came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum InspectTarget {
    /// A crash dump on disk. Immutable, so re-reading it always agrees.
    Dump { path: PathBuf },
    /// A running process, identified by the pid the supervisor already knows.
    Live { pid: u32 },
}

/// What the sidecar found once it opened the target.
///
/// Reported back rather than assumed: the runtime version and bitness are the
/// two things that most often explain a capture that could not be taken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TargetSummary {
    pub target: InspectTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitness: Option<Bitness>,
    /// e.g. `"9.0.3"`. Absent when the sidecar could not determine it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<String>,
    /// Process name, when known — the thing a human recognises.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// How much of the graph the sidecar is allowed to walk.
///
/// Every limit exists because an object graph is not a tree: it is cyclic,
/// frequently enormous, and a naive walk of a live heap does not terminate in
/// useful time. Exceeding any of these produces an explicit
/// [`ObjectValue::Elided`] rather than a quietly shorter list.
///
/// Every field carries the built-in default when it is missing from the file.
/// A configuration is hand-written, and `.code-basics/config.json` is the file
/// that must load for the workspace to open at all: refusing to open a
/// repository because someone wrote `"caps": { "maxDepth": 2 }` would turn a
/// tuning knob into a lockout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct Caps {
    pub max_depth: u32,
    pub max_children: u32,
    pub max_string_length: u32,
    /// Total nodes across the whole capture, so a wide-and-shallow graph is
    /// bounded even when `max_depth` never bites.
    pub max_nodes: u32,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            max_depth: 5,
            max_children: 100,
            max_string_length: 512,
            max_nodes: 5000,
        }
    }
}

/// Ceilings on a widened re-read.
///
/// Expanding is not a licence to walk an entire heap: these are the points past
/// which a capture would take long enough that the user has lost the thread,
/// and where the tree stops being readable anyway.
const MAX_WIDENED_CHILDREN: u32 = 10_000;
const MAX_WIDENED_DEPTH: u32 = 32;
const MAX_WIDENED_NODES: u32 = 200_000;

impl Caps {
    /// The limits a re-read needs in order to get *past* the cap that stopped
    /// the last one.
    ///
    /// Re-reading a branch with the limits that truncated it returns the same
    /// truncation, which is not an expansion — it is the tool telling the user
    /// it fetched something when it fetched nothing. Each reason raises the one
    /// limit that bit, plus the total node budget, which would otherwise become
    /// the next thing to stop the walk immediately.
    ///
    /// Growth is multiplicative and bounded: one click has to be visibly worth
    /// making, and no click may ask for an unbounded walk.
    pub fn widened(self, reason: ElidedReason) -> Self {
        let mut caps = self;

        match reason {
            ElidedReason::ChildLimit => {
                caps.max_children = caps
                    .max_children
                    .max(1)
                    .saturating_mul(4)
                    .min(MAX_WIDENED_CHILDREN);
            }
            ElidedReason::DepthLimit => {
                caps.max_depth = caps.max_depth.saturating_add(3).min(MAX_WIDENED_DEPTH);
            }
            ElidedReason::NodeLimit => {
                caps.max_nodes = caps
                    .max_nodes
                    .max(1)
                    .saturating_mul(4)
                    .min(MAX_WIDENED_NODES);
            }
        }

        // A raised child or depth limit that the node budget then cuts off at
        // the same place would look identical to no change at all.
        caps.max_nodes = caps
            .max_nodes
            .max(caps.max_children.saturating_mul(4))
            .min(MAX_WIDENED_NODES);

        caps
    }
}

/// Why the walk stopped at a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ElidedReason {
    DepthLimit,
    ChildLimit,
    NodeLimit,
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// What a single field holds.
///
/// The two variants that matter most are the ones that admit ignorance.
/// [`ObjectValue::Elided`] says "there is more here and I stopped";
/// [`ObjectValue::Unavailable`] says "I could not read this". Neither is ever
/// replaced with a plausible-looking placeholder, because a value the user
/// believes and acts on is far more damaging than a visible gap — the same
/// rule [`crate::model::TestOutcome::Other`] follows for unrecognised
/// outcomes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ObjectValue {
    /// A number, bool, char, enum or other value type, already formatted.
    Primitive {
        text: String,
    },
    /// A string. `truncated` is set when `max_string_length` cut it.
    Text {
        text: String,
        truncated: bool,
    },
    Null,
    /// A dictionary entry: a container whose two children are the `Key` and the
    /// `Value`. It carries no address of its own — it groups the pair rather
    /// than being an object on the heap — so it can neither be expanded nor
    /// matched for a cycle, which is exactly why it is its own marker variant
    /// rather than a [`ObjectValue::Reference`] (which would abstain with no
    /// address). The children are ordinary nodes, shaped by [`super::tree`].
    Pair,
    /// A reference to another object, expandable when the walk stopped here.
    Reference {
        address: String,
        type_name: String,
        expandable: bool,
    },
    /// This object was already shown higher up. `path` is the node id where it
    /// first appeared, so the UI can offer to jump there instead of recursing.
    Cycle {
        address: String,
        path: String,
    },
    /// A cap stopped the walk. Never silent truncation.
    Elided {
        reason: ElidedReason,
    },
    /// The value could not be read, with a sentence explaining why. Never a
    /// guess.
    Unavailable {
        reason: String,
    },
}

impl ObjectValue {
    /// Whether asking the sidecar for more would produce anything.
    pub fn is_expandable(&self) -> bool {
        match self {
            ObjectValue::Reference { expandable, .. } => *expandable,
            ObjectValue::Elided { .. } => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------

/// One row in the object tree.
///
/// Shaped like [`crate::model::TestNode`] on purpose: the frontend renders
/// both with the same recursive-row pattern. Cycles cannot make this structure
/// infinite because a repeat visit becomes an [`ObjectValue::Cycle`] leaf
/// rather than a nested child.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InspectNode {
    /// Stable path-shaped id, e.g. `root.orders[3]._total`. Used for selection
    /// and as the handle for expanding past a cap.
    pub id: String,
    /// The field or index name as the user would write it.
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    pub value: ObjectValue,
    pub children: Vec<InspectNode>,
    /// True when `children` is a prefix of what actually exists.
    pub has_more: bool,
    /// How many children exist in the target, when the sidecar could count
    /// them — this is what turns "100 items" into "100 of 5,412".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_count_total: Option<u32>,
}

/// One capture, ready for the tree view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InspectGraph {
    /// The directory under `.code-basics/inspect/` this capture used.
    pub session_id: String,
    /// Identifies this *snapshot* of the target. Expanding past a cap on a
    /// live process produces a new snapshot id, which is how the UI knows to
    /// warn that a branch may not agree with the rest of the tree.
    pub snapshot_id: String,
    /// RFC3339, from the sidecar.
    pub captured_at: String,
    pub target: TargetSummary,
    pub roots: Vec<InspectNode>,
    pub caps: Caps,
    /// Problems that did not prevent a capture — an unreadable region, a type
    /// the sidecar could not resolve. Shown, never swallowed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// Where to start walking.
///
/// There is deliberately no "find the interesting object" option. A live heap
/// holds millions of objects and any heuristic that picked one would be wrong
/// often enough to mislead — the same reason
/// [`crate::git::grouping`] abstains rather than guessing a label. The user
/// names what they want; the inspector reports what is there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RootSpec {
    /// Instances of a type, with a one-line summary each, for the user to pick
    /// from.
    Type { name: String, limit: u32 },
    /// Static fields of a type — often the only route to a live root such as a
    /// DI container or a cache.
    Statics { name: String },
    /// A specific object, from a previous capture.
    Address { address: String },
    /// Every live `System.Exception`. The best-effort answer for a *caught*
    /// exception, which nothing else can trigger on.
    Exceptions,
    /// The exception that killed the process. Dump targets only.
    CrashException,
}

/// What the sidecar is being asked to do, written to `request.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InspectRequest {
    pub schema_version: u32,
    pub target: InspectTarget,
    pub root: RootSpec,
    pub caps: Caps,
}

/// The schema version this build of the app writes and expects back. Bumped
/// only for a breaking change, so a stale sidecar is refused with a clear
/// message rather than misread.
pub const SCHEMA_VERSION: u32 = 1;

impl InspectRequest {
    pub fn new(target: InspectTarget, root: RootSpec, caps: Caps) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            target,
            root,
            caps,
        }
    }
}

// ---------------------------------------------------------------------------
// Dumps
// ---------------------------------------------------------------------------

/// A crash dump found under `.code-basics/dumps/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DumpFile {
    pub path: PathBuf,
    /// Executable name, from the `%e` in the filename template, extension
    /// included (`Api.exe`). It is shown against every dump because the dump
    /// environment is inherited by the whole process tree — a `dotnet run`
    /// leaves dumps for its build host too — and this is what lets a reader
    /// tell those apart. Nothing attributes a dump to a run automatically; the
    /// list is offered whole and the choice is the user's.
    pub executable: String,
    pub pid: u32,
    /// Unix seconds, from `%t`.
    pub captured_at: u64,
    pub bytes: u64,
}

/// One .NET process the machine is running, as the inspector enumerated it.
///
/// This is the *raw* observation, before anything is decided about whose it is:
/// `DiagnosticsClient.GetPublishedProcesses()` names every process that has
/// published a diagnostics channel, which is exactly the set that can be
/// attached to and — crucially — includes the child that `dotnet run` starts.
///
/// Only `pid` and `name` are guaranteed. Everything else is read through APIs
/// that legitimately fail on a process owned by another user or one exiting as
/// it is read, and each is therefore `None` rather than a placeholder: an
/// invented parent pid would attribute somebody else's process to the user's
/// run configuration, which is the wrong value this whole module exists to
/// avoid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DotnetProcess {
    pub pid: u32,
    /// The executable name without its extension, as the enumerator reported it.
    pub name: String,
    /// Full path to the executable, when it could be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// The parent process id, when it could be read. `None` means *unknown*,
    /// never "this process has no parent" — see [`Attribution`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_pid: Option<u32>,
    /// When the process started, ISO-8601, when it could be read. Kept as the
    /// string the enumerator wrote for the same reason capture timestamps are:
    /// nothing here does arithmetic on it, and reformatting a time is a way to
    /// get one wrong.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// The full command line, when it could be read. This is the only place the
    /// application assembly appears for a `dotnet run` child launched as
    /// `dotnet exec <output>.dll` — its OS name is just `dotnet` — so it is what
    /// distinguishes the real application from the SDK's own `dotnet`-named
    /// build tools. `None` means *unknown*: reading another process's command
    /// line is refused across an elevation or session boundary, and its absence
    /// costs only preselection, never correctness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_line: Option<String>,
}

/// How a process was linked to a run configuration.
///
/// The three answers are deliberately distinct rather than a boolean, because
/// the middle one is the whole point of enumerating processes at all: with only
/// "ours" and "not ours", the `dotnet run` child — the process actually holding
/// the user's objects — would have to be either claimed on a guess or thrown
/// away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Attribution {
    /// Its pid is exactly what the supervisor launched.
    Launched,
    /// Its parent chain reaches a process the supervisor launched — the
    /// `dotnet run` case.
    Descendant,
    /// A .NET process code-basics did not start.
    Unrelated,
}

/// A .NET process that could be attached to, and what is known about whose it
/// is.
///
/// Every process on the machine that published a diagnostics channel appears,
/// including ones code-basics never started. That is a change of policy made on
/// evidence: the previous list held only the supervisor's own pids, and for the
/// dominant .NET case those are `dotnet run` launchers whose heap contains none
/// of the user's objects. The honest fix is to show what is really there and
/// label each entry with how it was linked, rather than to show one pid that is
/// reliably the wrong one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AttachableProcess {
    pub pid: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// How this process was linked to a configuration — the evidence, so the UI
    /// can say *why* it thinks this is the user's application.
    pub attribution: Attribution,
    /// The run configuration this process belongs to. Present only when
    /// `attribution` is `Launched` or `Descendant`; an unrelated process is
    /// never labelled with a configuration name, because a stranger's heap
    /// rendered under the user's own configuration is precisely the wrong value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_name: Option<String>,
    /// Whether this is the process code-basics has *evidence* holds the
    /// application's own objects.
    ///
    /// True for a launched pid that nothing marks as a launcher, and for the
    /// single child a launcher could actually be said to have started as the
    /// application. False everywhere else — including for a `Descendant` that
    /// might well be the application but could equally be an MSBuild worker
    /// node or a compiler server, and for every process code-basics did not
    /// start.
    ///
    /// So false means *no evidence*, not "this is not the application", and a
    /// view must treat it that way: it may preselect a true, and must not
    /// preselect a false as though it were one. Deciding this here rather than
    /// in the UI is the point — the process tree, the run configurations and the
    /// launcher rule all live on this side, and a second derivation in the
    /// frontend was free to disagree with the caveat sitting next to it.
    pub is_application: bool,
    /// Why this pid is not the process holding the user's objects.
    ///
    /// `None` means there is no reason to believe it is anything but the
    /// application itself. `Some` means it is demonstrably a launcher — a
    /// `Launched` process that one of the listed `Descendant`s came out of — so
    /// a capture of it reads the launcher's heap and finds none of the user's
    /// types, an answer indistinguishable from "your object is not there".
    ///
    /// Unlike the blanket warning this replaced, it is now derived from the
    /// observed process tree, so it is only present where it is true. A caveat
    /// on an entry that *is* the application would train the reader to ignore
    /// it on the entry that is not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launcher_caveat: Option<String>,
}

/// The attachable processes, and anything that stopped the list being a
/// complete answer.
///
/// The warnings are not decoration. The one that matters most is the enumerator
/// reporting that it could not read any process's parent — on a host that
/// refuses a process snapshot, every [`Attribution`] silently degrades to
/// `Unrelated`, so the `dotnet run` child appears under "not this workspace's"
/// with no configuration name and its launcher advises waiting for a child that
/// can never be recognised. Carried out of the sidecar and then dropped, that
/// left the UI stating a cause that was not the real one.
///
/// A failure to produce a list at all is *not* in here: that is an error from
/// the command, because "there is nothing attachable" and "I could not look" are
/// different answers and must never be shown as the same one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AttachableList {
    pub processes: Vec<AttachableProcess>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// A dump that a finished run may have written, and how sure that is.
///
/// The distinction is the whole point. Nothing in this application matches a
/// dump to a run — the dump environment is inherited by every child process and
/// by every *other* configuration running at the same time, so "the newest dump
/// since this run started" is a dump, not *this run's* dump. Only a pid the run
/// itself reported is evidence; anything else is a candidate the reader has to
/// judge from its executable name, and the UI must phrase it that way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunDump {
    pub dump: DumpFile,
    /// True only when the dump carries the pid the run reported.
    pub certain: bool,
}

/// Whether the inspector can serve a given situation, and if not, why.
///
/// The reason is a sentence for a human, not a code: the UI must never imply
/// it can reach a state it cannot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InspectStatus {
    /// False when no sidecar could be resolved at all.
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    /// True when this workspace has opted into writing crash dumps.
    pub dump_capture_enabled: bool,
    /// Dumps currently on disk, newest first.
    pub dumps: Vec<DumpFile>,
    /// Things the user should know before relying on this — that dumps hold
    /// process memory, that a caught exception may already have been
    /// collected. Same spirit as the intent providers' caveats.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<String>,
    /// What attaching to a running process costs it, on its own so that a view
    /// which offers an attach button can state it *beside that button*.
    ///
    /// These sentences are also inside `caveats`, because the Objects tab shows
    /// the whole list; they are repeated here rather than moved because a
    /// caveat that is only reachable from the tab it warns about is a warning
    /// the Run tab's own attach buttons could never show before the click.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attach_caveats: Vec<String>,
}

/// Per-workspace inspector settings, stored in `.code-basics/config.json`.
///
/// Absent by default: dump capture writes hundreds of megabytes of process
/// memory to disk and must never switch itself on.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct InspectorConfig {
    pub capture_dumps: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caps: Option<Caps>,
    /// How many dumps to keep, newest first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_dumps: Option<u32>,
    /// Total size budget for the dumps directory, in megabytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_dump_megabytes: Option<u64>,
    /// Extra environment applied only to dump-capturing runs, for the rare
    /// case a project needs a different dump type.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod model_tests;
