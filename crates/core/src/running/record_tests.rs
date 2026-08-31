use super::*;

/// Sorted object keys of a serialized value — the same technique `model.rs` uses
/// to pin the exact JSON the hand-written TS mirror reads.
fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut k: Vec<String> = value
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    k.sort();
    k
}

fn sample() -> RunningRecord {
    RunningRecord {
        pid: 4242,
        kind: RunKind::Terminal,
        label: "Server".into(),
        root: "/ws".into(),
        key: "sess-1".into(),
        program: "pwsh.exe".into(),
        started_at_ms: 1_700_000_000_000,
    }
}

#[test]
fn record_serialises_with_the_keys_the_ui_reads() {
    assert_eq!(
        keys(&serde_json::to_value(sample()).unwrap()),
        [
            "key",
            "kind",
            "label",
            "pid",
            "program",
            "root",
            "startedAtMs"
        ]
    );
}

#[test]
fn run_kind_is_camel_case() {
    assert_eq!(
        serde_json::to_value(RunKind::Behavioral).unwrap(),
        serde_json::json!("behavioral")
    );
    assert_eq!(
        serde_json::to_value(RunKind::Terminal).unwrap(),
        serde_json::json!("terminal")
    );
}

#[test]
fn an_external_app_is_its_own_kind() {
    // The launcher's processes must be distinguishable from configuration runs:
    // the panel labels them differently and a kill routes to the global
    // supervisor rather than a codebase's.
    assert_eq!(
        serde_json::to_value(RunKind::External).unwrap(),
        serde_json::json!("external")
    );
}

#[test]
fn record_round_trips_through_json() {
    let rec = sample();
    let json = serde_json::to_string(&rec).unwrap();
    assert_eq!(serde_json::from_str::<RunningRecord>(&json).unwrap(), rec);
}

#[test]
fn report_serialises_with_the_keys_the_ui_reads() {
    let report = RunningReport {
        live: vec![sample()],
        orphans: vec![],
        warnings: vec!["x".into()],
    };
    assert_eq!(
        keys(&serde_json::to_value(report).unwrap()),
        ["live", "orphans", "warnings"]
    );
}

#[test]
fn a_missing_version_defaults_to_one_and_missing_records_to_empty() {
    let file: RunningFile = serde_json::from_str("{}").unwrap();
    assert_eq!(file.version, 1);
    assert!(file.records.is_empty());
}
