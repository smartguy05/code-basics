//! Decision-logic tests for the quality gate — the Rust port of the behaviour
//! the original `quality-gate-logic.mjs` pinned.

use super::*;
use crate::erosion::scan::{ErosionFlag, ErosionReport};
use crate::erosion::ErosionCategory;
use crate::git::patch::LineOrigin;
use serde_json::json;

fn v(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|s| s.to_string()).collect()
}

fn flag(path: &str, line: u32, category: ErosionCategory, message: &str) -> ErosionFlag {
    ErosionFlag {
        path: path.to_string(),
        line,
        index: 0,
        origin: LineOrigin::Addition,
        category,
        rule_id: "test-rule".to_string(),
        message: message.to_string(),
        content: "x".to_string(),
    }
}

#[test]
fn erosion_reminder_none_when_no_flags() {
    let report = ErosionReport::default();
    assert_eq!(erosion_reminder(&report), None);
}

#[test]
fn erosion_reminder_lists_each_flag() {
    let report = ErosionReport {
        flags: vec![
            flag(
                "src/a.rs",
                12,
                ErosionCategory::UnsafeCast,
                "introduced .unwrap()",
            ),
            flag(
                "src/b.ts",
                4,
                ErosionCategory::IgnoredTest,
                "test marked .skip",
            ),
        ],
        warnings: vec![],
    };
    let msg = erosion_reminder(&report).expect("some flags");
    assert!(msg.starts_with("2 potential erosion signals:"));
    assert!(msg.contains("src/a.rs:12  UnsafeCast  introduced .unwrap()"));
    assert!(msg.contains("src/b.ts:4  IgnoredTest  test marked .skip"));
    // No "and N more" tail below the cap.
    assert!(!msg.contains("more"));
}

#[test]
fn erosion_reminder_singular_header_for_one_flag() {
    let report = ErosionReport {
        flags: vec![flag(
            "src/a.rs",
            1,
            ErosionCategory::DroppedLog,
            "removed log line",
        )],
        warnings: vec![],
    };
    let msg = erosion_reminder(&report).unwrap();
    assert!(msg.starts_with("1 potential erosion signal:"));
}

#[test]
fn erosion_reminder_caps_and_counts_the_remainder() {
    let flags: Vec<ErosionFlag> = (0..25)
        .map(|i| {
            flag(
                "src/a.rs",
                i,
                ErosionCategory::LeftoverStub,
                "TODO left in production path",
            )
        })
        .collect();
    let report = ErosionReport {
        flags,
        warnings: vec![],
    };
    let msg = erosion_reminder(&report).unwrap();
    assert!(msg.starts_with("25 potential erosion signals:"));
    // 20 listed + header + tail line.
    let listed = msg.lines().filter(|l| l.contains("LeftoverStub")).count();
    assert_eq!(listed, 20);
    assert!(msg.contains("...and 5 more"));
}

#[test]
fn typecheck_gate_for_ts_and_tsx() {
    assert_eq!(
        gates_for_changes(&v(&["src/App.tsx"]), false),
        vec![Gate::Typecheck]
    );
    assert_eq!(
        gates_for_changes(&v(&["src/ipc/api.ts"]), false),
        vec![Gate::Typecheck]
    );
    assert_eq!(
        gates_for_changes(&v(&["a.mts", "b.cts"]), false),
        vec![Gate::Typecheck]
    );
}

#[test]
fn rustfmt_gate_for_rs() {
    assert_eq!(
        gates_for_changes(&v(&["crates/core/src/model.rs"]), false),
        vec![Gate::Rustfmt]
    );
}

#[test]
fn both_gates_and_backslash_paths() {
    assert_eq!(
        gates_for_changes(&v(&["src\\App.tsx", "crates\\core\\src\\model.rs"]), false),
        vec![Gate::Typecheck, Gate::Rustfmt]
    );
}

#[test]
fn full_adds_clippy_after_rustfmt_only_for_rust() {
    assert_eq!(
        gates_for_changes(&v(&["a.rs"]), true),
        vec![Gate::Rustfmt, Gate::Clippy]
    );
    // No .rs change ⇒ no clippy even with full.
    assert_eq!(
        gates_for_changes(&v(&["a.tsx"]), true),
        vec![Gate::Typecheck]
    );
}

#[test]
fn no_gates_for_unrelated_or_empty() {
    assert_eq!(
        gates_for_changes(&v(&["README.md", "docs/INDEX.md"]), false),
        vec![]
    );
    assert_eq!(gates_for_changes(&v(&[]), true), vec![]);
}

#[test]
fn gate_commands_and_labels() {
    assert_eq!(Gate::Typecheck.command(), ("pnpm", vec!["typecheck"]));
    assert_eq!(Gate::Rustfmt.command(), ("cargo", vec!["fmt", "--check"]));
    assert_eq!(Gate::Rustfmt.label(), "cargo fmt --check");
    let (prog, args) = Gate::Clippy.command();
    assert_eq!(prog, "cargo");
    assert_eq!(args[0], "clippy");
    assert!(args.contains(&"-D"));
}

#[test]
fn touched_source_only_under_real_roots() {
    assert!(touched_source(&v(&["src/App.tsx"])));
    assert!(touched_source(&v(&["crates/core/src/model.rs"])));
    assert!(touched_source(&v(&["src-tauri/src/lib.rs"])));
    assert!(touched_source(&v(&["sidecar/inspector/Program.cs"])));
    assert!(!touched_source(&v(&["docs/README.md"])));
    assert!(!touched_source(&v(&[".memories/features/x/notes.md"])));
}

#[test]
fn memory_reminder_fires_only_when_source_and_no_memory() {
    assert!(should_remind_memories(&v(&["src/App.tsx"])));
    assert!(!should_remind_memories(&v(&[
        "src/App.tsx",
        ".memories/features/x/completed.md"
    ])));
    assert!(!should_remind_memories(&v(&["docs/README.md"])));
}

#[test]
fn unresolved_rejection_matches_dated_head_line_only() {
    let token = concat!("AI-", "REJECTED");
    assert!(has_unresolved_rejection(&format!(
        "// {token} 2026-08-20\n// reason"
    )));
    // Bare token in prose is committable ⇒ not flagged.
    assert!(!has_unresolved_rejection(&format!(
        "mentions {token} somewhere"
    )));
    // Token with a non-date suffix ⇒ not flagged.
    assert!(!has_unresolved_rejection(&format!("{token} soon")));
    assert!(!has_unresolved_rejection("nothing here"));
}

#[test]
fn unresolved_rejection_scans_past_a_bare_first_hit() {
    let token = concat!("AI-", "REJECTED");
    // First occurrence is bare, second is dated — must still be found.
    let text = format!("{token} note\nlater: {token} 2026-01-02");
    assert!(has_unresolved_rejection(&text));
}

#[test]
fn loop_guard_reads_stop_hook_active() {
    assert!(should_skip_for_loop(&json!({"stop_hook_active": true})));
    assert!(!should_skip_for_loop(&json!({"stop_hook_active": false})));
    assert!(!should_skip_for_loop(&json!({})));
}

#[test]
fn typecheck_script_detection() {
    assert!(has_typecheck_script(
        r#"{"scripts":{"typecheck":"tsc --noEmit"}}"#
    ));
    assert!(!has_typecheck_script(
        r#"{"scripts":{"build":"vite build"}}"#
    ));
    assert!(!has_typecheck_script(r#"{"scripts":{"typecheck":"  "}}"#));
    assert!(!has_typecheck_script("not json"));
    assert!(!has_typecheck_script("{}"));
}

#[test]
fn invocation_detection_by_subcommand_or_marker() {
    let sub = vec!["cb-app".to_string(), "quality-gate".to_string()];
    assert!(is_quality_gate_invocation(&sub));
    let marker = vec!["cb-app".to_string(), format!("--{}", install::MARKER)];
    assert!(is_quality_gate_invocation(&marker));
    let neither = vec!["cb-app".to_string(), "record-intent".to_string()];
    assert!(!is_quality_gate_invocation(&neither));
}

#[test]
fn parse_args_reads_workspace() {
    let args = vec![
        "cb-app".to_string(),
        "quality-gate".to_string(),
        "--workspace".to_string(),
        "C:/repo".to_string(),
    ];
    let inv = parse_qgate_args(&args).expect("is a gate invocation");
    assert_eq!(inv.workspace.as_deref(), Some("C:/repo"));

    // No workspace ⇒ user scope (None).
    let user = vec!["cb-app".to_string(), "quality-gate".to_string()];
    assert_eq!(parse_qgate_args(&user).unwrap().workspace, None);

    // Not a gate invocation ⇒ None.
    assert!(parse_qgate_args(&["cb-app".to_string()]).is_none());
}

// ---------------------------------------------------------------------------
// Reading a failed gate's output: a verdict on the code, or on the environment?
// ---------------------------------------------------------------------------

fn deps(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_successful_gate_passes_whatever_it_printed() {
    let verdict = read_gate_output(Gate::Typecheck, true, "warning: something", &deps(&[]));
    assert_eq!(verdict, GateVerdict::Passed);
}

#[test]
fn a_plain_type_error_is_a_failure() {
    let output = "src/a.ts(3,1): error TS2322: Type 'string' is not assignable to type 'number'.";
    let verdict = read_gate_output(Gate::Typecheck, false, output, &deps(&["react"]));
    assert_eq!(verdict, GateVerdict::Failed(output.to_string()));
}

#[test]
fn an_unresolvable_declared_package_means_the_gate_could_not_run() {
    let output = "src/App.tsx(1,58): error TS2307: Cannot find module 'react' or its \
                  corresponding type declarations.";
    let verdict = read_gate_output(Gate::Typecheck, false, output, &deps(&["react"]));
    match verdict {
        GateVerdict::Unrunnable { summary, .. } => {
            assert!(summary.contains("react"), "names the package: {summary}");
            assert!(
                summary.contains("not blocking"),
                "says it is not a failure: {summary}"
            );
        }
        other => panic!("expected Unrunnable, got {other:?}"),
    }
}

#[test]
fn an_undeclared_missing_package_is_a_real_failure() {
    // Nothing in package.json promises this exists, so an import of it is a
    // genuine mistake the agent can fix — not a broken install.
    let output = "src/a.ts(1,1): error TS2307: Cannot find module 'lodash'.";
    let verdict = read_gate_output(Gate::Typecheck, false, output, &deps(&["react"]));
    assert!(matches!(verdict, GateVerdict::Failed(_)), "{verdict:?}");
}

#[test]
fn a_relative_import_is_never_environmental() {
    // A missing sibling file is the agent's own doing however the tree is laid
    // out, so it must keep blocking even while the environment is broken.
    let output = "src/a.ts(1,1): error TS2307: Cannot find module './missing'.";
    let verdict = read_gate_output(Gate::Typecheck, false, output, &deps(&["react"]));
    assert!(matches!(verdict, GateVerdict::Failed(_)), "{verdict:?}");
}

#[test]
fn a_real_error_hidden_among_unreachable_packages_still_abstains() {
    // The governing rule: once the dependency tree is unreachable, *every*
    // diagnostic is suspect — `any` poisoning invents errors that vanish in a
    // healthy tree, and sifting them would be a guess. So the gate refuses to
    // render a verdict either way, keeps the whole output, and says a real
    // problem may be hidden in it.
    let output = "src/App.tsx(1,1): error TS2307: Cannot find module 'react'.\n\
                  src/a.ts(3,1): error TS2322: Type 'string' is not assignable to type 'number'.";
    let verdict = read_gate_output(Gate::Typecheck, false, output, &deps(&["react"]));
    match verdict {
        GateVerdict::Unrunnable { summary, detail } => {
            assert!(
                summary.contains("may be hidden"),
                "warns a real problem may be hidden: {summary}"
            );
            assert!(
                detail.contains("TS2322"),
                "keeps the real diagnostic rather than discarding it: {detail}"
            );
        }
        other => panic!("expected Unrunnable, got {other:?}"),
    }
}

#[test]
fn an_untrusted_mount_point_is_environmental_on_its_own() {
    // The OS said the path could not be traversed. No package list needed.
    let output = "The path cannot be traversed because it contains an untrusted mount point";
    let verdict = read_gate_output(Gate::Typecheck, false, output, &deps(&[]));
    assert!(
        matches!(verdict, GateVerdict::Unrunnable { .. }),
        "{verdict:?}"
    );
}

#[test]
fn a_running_exe_blocking_the_linker_is_environmental() {
    // Documented in CLAUDE.md: the app is running and Windows will not let its
    // exe be replaced. Compilation is fine; blocking the turn on it is wrong.
    let output = "error: linking with link.exe failed\nAccess is denied. (os error 5)";
    let verdict = read_gate_output(Gate::Clippy, false, output, &deps(&[]));
    assert!(
        matches!(verdict, GateVerdict::Unrunnable { .. }),
        "{verdict:?}"
    );
}

#[test]
fn an_ordinary_rustfmt_failure_still_blocks() {
    let output = "Diff in C:/repo/src/a.rs at line 3:\n-use  std;\n+use std;";
    let verdict = read_gate_output(Gate::Rustfmt, false, output, &deps(&[]));
    assert_eq!(verdict, GateVerdict::Failed(output.to_string()));
}

#[test]
fn unresolved_packages_reads_every_shape_typescript_and_node_emit() {
    let output = "\
error TS2307: Cannot find module '@codemirror/state' or its corresponding type declarations.
error TS7016: Could not find a declaration file for module 'react'.
error TS2688: Cannot find type definition file for 'vite/client'.
Error [ERR_MODULE_NOT_FOUND]: Cannot find package 'vitest' imported from x.js
error TS2307: Cannot find module 'material-icon-theme/icons/rust.svg?raw'.";
    let found = unresolved_packages(output);
    assert_eq!(
        found,
        vec![
            "@codemirror/state".to_string(),
            "material-icon-theme".to_string(),
            "react".to_string(),
            "vite".to_string(),
            "vitest".to_string(),
        ],
        "a scoped name keeps both segments; a subpath and a ?query are stripped"
    );
}

#[test]
fn unresolved_packages_ignores_paths_that_are_not_packages() {
    let output = "\
error TS2307: Cannot find module './sibling'.
error TS2307: Cannot find module '../up'.
error TS2307: Cannot find module '/abs/path'.
error TS2307: Cannot find module 'C:/repo/thing'.
error TS2307: Cannot find module ''.";
    assert!(unresolved_packages(output).is_empty());
}

#[test]
fn declared_dependencies_reads_both_dependency_maps() {
    let manifest = r#"{
        "dependencies": { "react": "^19.0.0", "lucide-react": "^1.0.0" },
        "devDependencies": { "vitest": "^4.0.0" }
    }"#;
    let mut found = declared_dependencies(manifest);
    found.sort();
    assert_eq!(found, deps(&["lucide-react", "react", "vitest"]));
}

#[test]
fn declared_dependencies_abstains_on_an_unreadable_manifest() {
    assert!(declared_dependencies("not json").is_empty());
    assert!(declared_dependencies("{}").is_empty());
}

#[test]
fn the_unrunnable_detail_is_capped_so_a_wall_of_noise_cannot_flood_the_turn() {
    // The whole problem this fixes produced ~5000 diagnostic lines. Passing all
    // of them back to the model is its own failure mode.
    let mut lines = vec!["error TS2307: Cannot find module 'react'.".to_string()];
    for i in 0..200 {
        lines.push(format!(
            "src/x{i}.tsx(1,1): error TS7026: JSX element implicitly any."
        ));
    }
    let verdict = read_gate_output(Gate::Typecheck, false, &lines.join("\n"), &deps(&["react"]));
    match verdict {
        GateVerdict::Unrunnable { detail, .. } => {
            let rendered = detail.lines().count();
            assert!(
                rendered <= UNRUNNABLE_DETAIL_LINES + 1,
                "capped: {rendered}"
            );
            assert!(
                detail.contains("more line"),
                "says what it dropped: {detail}"
            );
        }
        other => panic!("expected Unrunnable, got {other:?}"),
    }
}

#[test]
fn a_genuine_failure_is_never_truncated() {
    // The opposite rule: a real verdict must arrive whole, because the one line
    // that matters could be anywhere in it.
    let long: Vec<String> = (0..200)
        .map(|i| format!("src/a.ts({i},1): error TS2322: Type mismatch."))
        .collect();
    let output = long.join("\n");
    let verdict = read_gate_output(Gate::Typecheck, false, &output, &deps(&["react"]));
    assert_eq!(verdict, GateVerdict::Failed(output));
}
