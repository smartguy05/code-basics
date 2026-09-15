//! Every answer this server can give that is not data — and they are all different
//! answers.
//!
//! The same abstain-rather-than-guess rule the other servers apply
//! ([`crate::roslyn::answer`], [`crate::mcp::answer`]), for the same reason: a model
//! reads the sentence, believes it, and acts, so the distinctions a human would
//! have recovered from unaided are the ones kept apart here.
//!
//! Two of these are *before* any build is looked at:
//!
//! * [`BuildRefusal::NoWorkspace`] — this server was started without a
//!   `--workspace`, so there is no build to run or read at all.
//! * [`BuildRefusal::Ambiguous`] — several windows have the workspace open, so which
//!   build was meant is genuinely unknown. (The [`super::instances`] layer produces
//!   the detailed pid-listing form; this is the same class of answer with a stable
//!   code.)
//!
//! The rest are **status-derived**: a read tool asked before any build ran, or
//! after a build that could not start, refuses rather than reporting an empty
//! success:
//!
//! * [`BuildRefusal::NeverBuilt`] — no build has run for this workspace yet
//!   ([`crate::build::BuildStatus::NeverBuilt`]).
//! * [`BuildRefusal::BuildFailedToStart`] — the build process itself could not be
//!   started ([`crate::build::BuildStatus::CouldNotStart`]), which is not the same
//!   as a build that ran and failed.
//!
//! And [`BuildRefusal::Disabled`] is the user having switched this tool off in the
//! MCP settings.
//!
//! # No internal error text
//!
//! Every sentence here is generic and names no path, no OS error and no build log
//! line. Why a build could not start (a missing `dotnet`, a locked output) is the
//! application's to know, not this interface's to volunteer.

use crate::build::BuildStatus;

/// Why a tool call produced no data. **Five distinct codes**; a shared one would
/// pass every other test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildRefusal {
    /// This server was started without a `--workspace`, so there is no build to
    /// reach.
    NoWorkspace,
    /// Several windows have the workspace open, so which build was meant is unknown.
    Ambiguous,
    /// No build has run for this workspace yet, so there is nothing to report.
    NeverBuilt,
    /// The build process itself could not be started — distinct from a build that
    /// ran and failed.
    BuildFailedToStart,
    /// The tool was switched off by the user in the MCP settings.
    Disabled,
}

impl BuildRefusal {
    /// The status-derived refusal for a read tool (`get_errors`/`get_warnings`)
    /// against a build state that has no rows to report, or [`None`] when the
    /// status carries data that should be rendered rather than refused.
    ///
    /// [`BuildStatus::Building`] returns [`None`]: it is a real, renderable state,
    /// not a refusal.
    pub fn from_status(status: BuildStatus) -> Option<Self> {
        match status {
            BuildStatus::NeverBuilt => Some(Self::NeverBuilt),
            BuildStatus::CouldNotStart => Some(Self::BuildFailedToStart),
            BuildStatus::Building
            | BuildStatus::SucceededClean
            | BuildStatus::SucceededWithWarnings
            | BuildStatus::Failed => None,
        }
    }

    /// A short machine-matchable name, so a model can branch without parsing prose.
    pub fn code(self) -> &'static str {
        match self {
            Self::NoWorkspace => "noWorkspace",
            Self::Ambiguous => "ambiguous",
            Self::NeverBuilt => "neverBuilt",
            Self::BuildFailedToStart => "buildFailedToStart",
            Self::Disabled => crate::tool_gate::DISABLED_CODE,
        }
    }

    /// The sentence the model reads. Generic — no internal error text.
    pub fn sentence(self) -> String {
        match self {
            Self::NoWorkspace => {
                "This Build server was started without a --workspace, so there is \
                 no build to run or read. It has to be installed scoped to a repository; the build \
                 is per-workspace."
                    .to_string()
            }
            Self::Ambiguous => "Several code-basics windows have this workspace open, so which \
                 build was meant is genuinely unknown. Nothing was done and nothing was chosen. \
                 Ask the user which window, then pass --instance <pid>."
                .to_string(),
            Self::NeverBuilt => "No build has run for this workspace yet, so there are no results \
                 to report. This is not an empty success — run build_solution first, then ask \
                 again."
                .to_string(),
            Self::BuildFailedToStart => "The build process could not be started at all, so there \
                 are no diagnostics to report. This is not the same as a build that ran and \
                 failed. The reason is in the application; it is deliberately not forwarded here."
                .to_string(),
            Self::Disabled => crate::tool_gate::disabled_tool_sentence("this build tool"),
        }
    }
}

#[cfg(test)]
#[path = "answer_tests.rs"]
mod tests;
