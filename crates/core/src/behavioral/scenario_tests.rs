use super::*;
use crate::model::ConfigSource;

fn resp(status: u16, body: &str) -> RecordedResponse {
    RecordedResponse {
        status,
        headers: vec![("content-type".into(), "application/json".into())],
        body: body.into(),
        content_type: Some("application/json".into()),
    }
}

fn ok_side(pairs: &[(&str, RecordedResponse)]) -> SideResult {
    let mut responses = BTreeMap::new();
    for (k, r) in pairs {
        responses.insert((*k).to_string(), Ok(r.clone()));
    }
    SideResult {
        ready: Ok(()),
        responses,
    }
}

// ---- pair_and_diff ------------------------------------------------------

#[test]
fn both_ready_with_a_status_difference_yields_one_delta() {
    let keys = vec![("s#r".to_string(), "s#r".to_string())];
    let base = ok_side(&[("s#r", resp(200, "{}"))]);
    let work = ok_side(&[("s#r", resp(500, "{}"))]);

    let (deltas, warnings) = pair_and_diff(&keys, &base, &work, &[]);

    assert_eq!(deltas.len(), 1, "warnings: {warnings:?}");
    assert_eq!(deltas[0].status, Some((200, 500)));
    assert!(warnings.is_empty());
}

#[test]
fn identical_responses_yield_no_deltas() {
    let keys = vec![("s#r".to_string(), "s#r".to_string())];
    let base = ok_side(&[("s#r", resp(200, "{\"a\":1}"))]);
    let work = ok_side(&[("s#r", resp(200, "{\"a\":1}"))]);

    let (deltas, warnings) = pair_and_diff(&keys, &base, &work, &[]);

    assert!(deltas.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn base_unready_yields_one_warning_and_no_deltas() {
    let keys = vec![("s#r".to_string(), "s#r".to_string())];
    let base = SideResult::unready("never came up".into());
    let work = ok_side(&[("s#r", resp(200, "{}"))]);

    let (deltas, warnings) = pair_and_diff(&keys, &base, &work, &[]);

    assert!(deltas.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("baseline server not ready"));
}

#[test]
fn a_request_errored_on_the_work_side_warns_with_no_delta_for_it() {
    let keys = vec![("s#r".to_string(), "s#r".to_string())];
    let base = ok_side(&[("s#r", resp(200, "{}"))]);
    let mut work_responses = BTreeMap::new();
    work_responses.insert("s#r".to_string(), Err("connection reset".to_string()));
    let work = SideResult {
        ready: Ok(()),
        responses: work_responses,
    };

    let (deltas, warnings) = pair_and_diff(&keys, &base, &work, &[]);

    assert!(deltas.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("not comparable"));
    assert!(warnings[0].contains("connection reset"));
}

// ---- plan_replay --------------------------------------------------------

fn scenario_with(path: &str, req_names: &[&str], readiness: Option<Readiness>) -> HttpScenario {
    let requests = req_names
        .iter()
        .map(|n| HttpRequestSpec {
            name: (*n).to_string(),
            method: "GET".into(),
            url: "http://localhost/".into(),
            headers: vec![],
            body: None,
        })
        .collect();
    HttpScenario {
        path: path.to_string(),
        requests,
        readiness,
        ..Default::default()
    }
}

fn a_readiness() -> Readiness {
    Readiness {
        method: "GET".into(),
        url: "http://localhost/health".into(),
        expect_status: 200,
        timeout: std::time::Duration::from_secs(5),
        poll_interval: std::time::Duration::from_millis(50),
    }
}

#[test]
fn plan_flattens_requests_and_keys_by_path_and_name() {
    let scenarios = vec![
        scenario_with("a.http", &["one", "two"], Some(a_readiness())),
        scenario_with("b.http", &["three"], None),
    ];
    let plan = plan_replay(&scenarios);

    assert_eq!(plan.requests.len(), 3);
    assert_eq!(
        plan.keys.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>(),
        vec!["a.http#one", "a.http#two", "b.http#three"]
    );
    assert!(plan.readiness.is_some(), "first declared readiness wins");
}

#[test]
fn plan_has_no_readiness_when_none_declared() {
    let scenarios = vec![scenario_with("a.http", &["one"], None)];
    let plan = plan_replay(&scenarios);
    assert!(plan.readiness.is_none());
}

#[test]
fn plan_disambiguates_duplicate_request_names() {
    // Two requests named the same in one file must not collide on the key and
    // silently drop one — both are kept, the repeat gets an occurrence suffix.
    let scenarios = vec![scenario_with(
        "a.http",
        &["dup", "dup"],
        Some(a_readiness()),
    )];
    let plan = plan_replay(&scenarios);

    assert_eq!(plan.requests.len(), 2, "both requests survive");
    let keys: Vec<String> = plan.keys.iter().map(|(k, _)| k.clone()).collect();
    assert_eq!(keys, vec!["a.http#dup", "a.http#dup#1"]);
    // Keys are unique, so both are replayed and diffed distinctly.
    let unique: std::collections::HashSet<&String> = keys.iter().collect();
    assert_eq!(unique.len(), keys.len());
}

// ---- choose_launch_config ----------------------------------------------

fn cfg(id: &str, kind: RunKind) -> RunConfig {
    RunConfig::new(id, id, kind, "dotnet", ConfigSource::Detected)
}

#[test]
fn passed_app_config_is_used_directly() {
    let passed = cfg("api", RunKind::App);
    let all = vec![passed.clone(), cfg("other", RunKind::App)];
    match choose_launch_config(&passed, &all) {
        LaunchChoice::Use(id) => assert_eq!(id, "api"),
        LaunchChoice::Abstain(w) => panic!("should not abstain: {w}"),
    }
}

#[test]
fn sole_app_config_is_used_when_passed_is_a_test() {
    let passed = cfg("tests", RunKind::Test);
    let all = vec![passed.clone(), cfg("api", RunKind::App)];
    match choose_launch_config(&passed, &all) {
        LaunchChoice::Use(id) => assert_eq!(id, "api"),
        LaunchChoice::Abstain(w) => panic!("should not abstain: {w}"),
    }
}

#[test]
fn no_app_config_abstains() {
    let passed = cfg("tests", RunKind::Test);
    let all = vec![passed.clone()];
    assert!(matches!(
        choose_launch_config(&passed, &all),
        LaunchChoice::Abstain(_)
    ));
}

#[test]
fn ambiguous_app_configs_abstain() {
    let passed = cfg("tests", RunKind::Test);
    let all = vec![
        passed.clone(),
        cfg("api", RunKind::App),
        cfg("worker", RunKind::App),
    ];
    match choose_launch_config(&passed, &all) {
        LaunchChoice::Abstain(w) => assert!(w.contains("ambiguous")),
        LaunchChoice::Use(id) => panic!("should abstain, got {id}"),
    }
}
