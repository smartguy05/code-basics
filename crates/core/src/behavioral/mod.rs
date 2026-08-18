//! Behavioral before/after testing — the *runtime* counterpart to the static
//! intent [`crate::git::coverage`] Scorecard.
//!
//! [`crate::git::coverage`] answers "did the agent do what it *said*?" by
//! matching recorded edits onto the diff, statically, without running anything.
//! This module answers the harder question — "did the change *do* what was
//! claimed?" — by running the same test suite / scenario against **git HEAD**
//! and the **current working tree** and diffing the *observable* outcomes: test
//! results, console output, HTTP responses. Evidence, without reading the code.
//!
//! # A wrong label is worse than no label
//!
//! The same rule the whole intent stack is built against. A single run per side
//! cannot tell a flaky flip from a real one, console output is full of
//! timestamps and temp paths, and a server that never came up produces no
//! response at all. So every comparison here **abstains** — reports at low or
//! absent [`Confidence`], or refuses to judge and counts the refusal into
//! [`BehavioralReport::warnings`] — rather than assert a behavioral change it
//! cannot stand behind. Nothing vanishes silently.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::git::attribution::Confidence;

pub mod attribute;
pub mod compare;
pub mod console;
pub mod http;
pub mod httpfile;
pub mod prepare;
pub mod replay;
pub mod scenario;
pub mod worktree;

pub use attribute::attribute_behavioral;
pub use compare::{diff_tests, CaseDelta, CaseTransition, TestDelta};
pub use console::{diff_console, ConsoleNormalization};
pub use http::{diff_http, RecordedResponse, VOLATILE_HEADERS};
pub use httpfile::{discover_http_files, parse_http_file, HttpRequestSpec, HttpScenario, Readiness};
pub use prepare::{assemble_report, find_config, scan_baseline};
pub use replay::{await_ready, send};
pub use scenario::{choose_launch_config, pair_and_diff, plan_replay, LaunchChoice, SideResult};
pub use worktree::{BaselineWorktree, WorktreeOptions};

/// One observable difference between the HEAD and working-tree runs.
///
/// Internally tagged (`kind`) so the union crosses to TypeScript as a
/// discriminated union, mirroring [`crate::process::ProcessEvent`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BehavioralDelta {
    /// A test case whose outcome changed between the two runs.
    Test(CaseDelta),
    /// The captured console output differed after normalisation.
    Console(ConsoleDelta),
    /// A replayed HTTP request's response differed.
    Http(HttpDelta),
}

/// A difference in captured console output, after masking known noise.
///
/// `normalized` records whether masking was applied at all; `confidence` drops
/// to [`Confidence::Low`] when masking had to touch a large share of lines or
/// ordering was forced, because at that point the residual diff is weak
/// evidence. Equal-after-masking is reported as *no delta*, never as a change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleDelta {
    pub added_lines: Vec<String>,
    pub removed_lines: Vec<String>,
    pub normalized: bool,
    pub confidence: Confidence,
}

/// One header whose presence or value changed between the two responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HeaderChange {
    pub name: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// A difference in a response body, after type-aware normalisation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BodyDelta {
    pub added_lines: Vec<String>,
    pub removed_lines: Vec<String>,
    pub normalized: bool,
}

/// A difference between the HEAD and working-tree responses for one replayed
/// `.http` request. Volatile headers (date, request-id, …) are ignored by
/// default, and JSON bodies are compared structurally so key order is not a
/// false delta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HttpDelta {
    /// The request's `# @name` from the `.http` file.
    pub name: String,
    /// `(before, after)` status codes, only when they differ.
    pub status: Option<(u16, u16)>,
    pub header_changes: Vec<HeaderChange>,
    pub body: Option<BodyDelta>,
    pub confidence: Confidence,
}

/// The behavioral deltas attributed to one intent card.
///
/// `confidence` is the weakest of any delta's, so a card never looks more
/// certain than its shakiest piece of evidence — the same rule
/// [`crate::git::grouping::IntentGroup`] uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CardBehavior {
    pub group_id: String,
    pub deltas: Vec<BehavioralDelta>,
    pub confidence: Confidence,
}

/// The per-run tally shown beside the static [`crate::git::coverage::Scorecard`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BehavioralScorecard {
    /// Outcomes actually run on both sides (test suites + console + http scenarios).
    pub outcomes_compared: u32,
    /// Observable differences found.
    pub deltas: u32,
    /// Deltas pinned to exactly one card.
    pub attributed_deltas: u32,
    /// Deltas no single card owns — the honest bucket.
    pub unattributed_deltas: u32,
    /// Outcomes we refused to judge (never ready, too noisy, dependency drift).
    pub abstained: u32,
}

/// The whole before/after comparison — the runtime twin of
/// [`crate::git::coverage::IntentReview`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BehavioralReport {
    /// `None` when tests were not in scope or were abstained.
    pub tests: Option<TestDelta>,
    pub console: Option<ConsoleDelta>,
    pub http: Vec<HttpDelta>,
    /// Deltas mapped to the intent card that plausibly caused them.
    pub attributions: Vec<CardBehavior>,
    /// Deltas that could not be attributed to exactly one card.
    pub unattributed: Vec<BehavioralDelta>,
    pub scorecard: BehavioralScorecard,
    /// Abstains, teardown residue, readiness failures — everything refused.
    pub warnings: Vec<String>,
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
