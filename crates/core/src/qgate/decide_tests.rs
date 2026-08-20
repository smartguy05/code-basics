//! Decision-logic tests for the quality gate — the Rust port of the behaviour
//! the original `quality-gate-logic.mjs` pinned.

use super::*;
use serde_json::json;

fn v(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|s| s.to_string()).collect()
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
