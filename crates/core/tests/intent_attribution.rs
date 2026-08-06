//! Attribution measured against a real repository, rather than a fixture.
//!
//! Every other test builds its own diff, which proves the algorithm does what
//! it was told but says nothing about whether real agent history actually
//! matches real working-tree changes. That is the question the feature lives
//! or dies on, and it can only be answered against genuine data.
//!
//! So this is a **diagnostic, not an assertion**. It is `#[ignore]`d because
//! its result depends on whose machine it runs on and what they happen to have
//! uncommitted — there is no honest number to assert. Run it deliberately:
//!
//! ```text
//! cargo test -p cb-core --test intent_attribution -- --ignored --nocapture
//! ```
//!
//! What to look for: the share of changed lines that end up labelled. A low
//! number is not necessarily a bug — it may just mean the changes were made by
//! hand — but a number near zero when the history is full of edits means
//! anchoring is failing and should be investigated.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cb_core::git::attribution::{self, Options};
use cb_core::git::grouping::{self, GroupKind};
use cb_core::git::{ComparisonMode, LineOrigin, Repo};
use cb_core::intents::providers;
use cb_core::intents::{Intents, LoadOptions};

fn repository_root() -> PathBuf {
    // The crate sits at <root>/crates/core.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("the workspace root")
        .to_path_buf()
}

#[test]
#[ignore = "depends on local agent history and uncommitted changes"]
fn report_attribution_against_this_repository() {
    let root = repository_root();
    println!("\nworkspace: {}", root.display());

    let Ok(repo) = Repo::open(&root) else {
        println!("not a git repository; nothing to measure");
        return;
    };

    let diffs = repo
        .diff_all(ComparisonMode::WorkingToHead)
        .expect("a working-tree diff");

    let changed_lines: usize = diffs
        .iter()
        .flat_map(|d| &d.hunks)
        .flat_map(|h| &h.lines)
        .filter(|l| l.origin != LineOrigin::Context)
        .count();
    let hunks: usize = diffs.iter().map(|d| d.hunks.len()).sum();

    println!(
        "working tree: {} file(s), {hunks} hunk(s), {changed_lines} changed line(s)",
        diffs.len()
    );
    if changed_lines == 0 {
        println!("nothing uncommitted; nothing to measure");
        return;
    }

    // What each agent already recorded, with no setup at all.
    let (records, labels) = providers::history(&root);
    println!(
        "history: {} record(s), {} label(s)",
        records.len(),
        labels.len()
    );

    for status in providers::statuses(&root) {
        println!(
            "  {:?}: detected={} capture={:?} sessions={}",
            status.provider, status.detected, status.capture, status.sessions
        );
        for caveat in &status.caveats {
            println!("    caveat: {caveat}");
        }
    }

    let intents = Intents { records, labels };
    let attributions = attribution::attribute(&diffs, &intents, Options::default());

    let attributed: usize = attributions
        .iter()
        .flat_map(|f| &f.hunks)
        .flat_map(|h| &h.spans)
        .map(|s| s.line_indices.len())
        .sum();
    let unattributed: usize = attributions
        .iter()
        .flat_map(|f| &f.hunks)
        .map(|h| h.unattributed_lines as usize)
        .sum();

    let share = attributed as f64 / changed_lines.max(1) as f64 * 100.0;
    println!("\nattributed {attributed} / {changed_lines} changed lines ({share:.1}%)");
    println!("unattributed: {unattributed}");

    let groups = grouping::group(&diffs, &attributions);
    println!("\n{hunks} hunk(s) collapsed into {} group(s):", groups.len());

    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for group in &groups {
        *by_kind.entry(format!("{:?}", group.kind)).or_default() += 1;
    }
    for (kind, count) in &by_kind {
        println!("  {kind}: {count}");
    }

    println!("\nlargest groups:");
    for group in groups.iter().take(12) {
        println!(
            "  [{:>14}] {:<52} {:>3} line(s) across {} file(s)",
            format!("{:?}", group.kind),
            truncate(&group.label, 52),
            group.line_count,
            group.files.len()
        );
    }

    // The claim the feature makes, restated as a number.
    let stated = groups.iter().filter(|g| g.kind == GroupKind::Intent).count();
    println!(
        "\n{hunks} hunks -> {} decisions ({stated} explained by the agent itself)",
        groups.len()
    );

    // Sanity properties that must hold whatever the data looks like. These are
    // real assertions: they cost nothing and would catch a grouping that
    // dropped or duplicated lines.
    let grouped_lines: usize = groups.iter().map(|g| g.line_count as usize).sum();
    assert_eq!(
        grouped_lines, changed_lines,
        "every changed line must appear in exactly one group"
    );
    assert_eq!(
        attributed + unattributed,
        changed_lines,
        "every changed line is either attributed or counted as unattributed"
    );
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    text.chars().take(width.saturating_sub(1)).collect::<String>() + "…"
}

/// The same properties on a clean tree, so the invariants are covered by an
/// ordinary `cargo test` run too.
#[test]
fn grouping_a_repository_with_no_changes_produces_no_groups() {
    let dir = tempfile::tempdir().unwrap();
    let Ok(repo) = git2::Repository::init(dir.path()) else {
        return;
    };
    drop(repo);

    let repo = Repo::open(dir.path()).expect("an empty repository");
    let diffs = repo.diff_all(ComparisonMode::WorkingToHead).unwrap_or_default();

    let attributions = attribution::attribute(&diffs, &Intents::default(), Options::default());
    let groups = grouping::group(&diffs, &attributions);

    assert!(groups.is_empty());
}

/// Loading intent for a workspace that never recorded any must be silent, not
/// an error — the overwhelmingly common case.
#[test]
fn a_workspace_with_no_recorded_intent_loads_cleanly() {
    let dir = tempfile::tempdir().unwrap();

    let intents = cb_core::intents::load(dir.path(), &LoadOptions::default()).unwrap();

    assert!(intents.is_empty());
}
