use super::*;

/// Parse a settings block the way [`crate::config::load`] would.
fn parse(json: &str) -> LspConfig {
    serde_json::from_str(json).expect("a hand-written block must load")
}

#[test]
fn an_absent_block_is_the_default_and_configures_nothing() {
    let config = LspConfig::default();
    assert!(config.servers.is_empty());
    assert!(config.server("csharp").is_none());
}

#[test]
fn a_block_naming_only_a_program_loads_with_everything_else_left_to_the_default() {
    // This file is hand-written, and `config::load` failing is not a local
    // failure — `open_workspace` propagates it, so a partial block would lock
    // the user out of the repository until they edited the JSON back. Same
    // reasoning as `a_partial_caps_section_does_not_stop_the_workspace_opening`.
    let config = parse(r#"{"servers":{"csharp":{"program":"C:/tools/roslyn.exe"}}}"#);

    let csharp = config.server("csharp").expect("the block names csharp");
    assert_eq!(csharp.program.as_deref(), Some("C:/tools/roslyn.exe"));
    assert_eq!(csharp.enabled, None);
    assert_eq!(csharp.args, None);
    assert!(csharp.env.is_empty());
    assert_eq!(csharp.uri_style, None);
}

#[test]
fn a_key_written_by_a_newer_build_does_not_stop_the_workspace_opening() {
    // `deny_unknown_fields` is deliberately absent. `config.json` is shared
    // through a repository, so one teammate running a newer build must not
    // make the file unloadable for everyone else — the same tolerance
    // `config::load` already shows for the rest of the file.
    let config = parse(
        r#"{"servers":{"csharp":{"program":"x","traceLevel":"verbose"}},"defaultTimeoutMs":5000}"#,
    );

    assert_eq!(
        config.server("csharp").and_then(|s| s.program.as_deref()),
        Some("x")
    );
}

#[test]
fn an_absent_enabled_flag_means_enabled_because_it_means_use_the_default() {
    // Three states, not two: `false` is a refusal, `true` is a confirmation,
    // and absent is "we never discussed it". Collapsing absent into `false`
    // would turn every partially written block into a silently dead feature.
    assert!(!ServerOverride::default().is_disabled());
    assert!(!parse(r#"{"servers":{"rust":{"enabled":true}}}"#)
        .server("rust")
        .unwrap()
        .is_disabled());
    assert!(parse(r#"{"servers":{"python":{"enabled":false}}}"#)
        .server("python")
        .unwrap()
        .is_disabled());
}

#[test]
fn a_lookup_for_a_server_the_file_never_mentions_abstains() {
    // `None` and "present but empty" are different answers: the caller uses the
    // first to mean "no override at all" and must not receive a fabricated
    // empty override that looks like the user configured something.
    let config = parse(r#"{"servers":{"csharp":{}}}"#);

    assert!(config.server("csharp").is_some());
    assert!(config.server("typescript").is_none());
}

#[test]
fn an_empty_args_list_is_a_different_answer_from_no_args_key() {
    // `"args": []` says "launch it with nothing added"; an absent key says
    // "use the built-in arguments". Roslyn without `--stdio` never speaks a
    // word, so flattening these two would produce a server that hangs.
    let explicit = parse(r#"{"servers":{"csharp":{"args":[]}}}"#);
    assert_eq!(explicit.server("csharp").unwrap().args, Some(Vec::new()));

    let absent = parse(r#"{"servers":{"csharp":{}}}"#);
    assert_eq!(absent.server("csharp").unwrap().args, None);
}

#[test]
fn the_uri_style_setting_names_the_transport_spelling_it_selects() {
    assert_eq!(UriStyleSetting::Encoded.style(), UriStyle::Encoded);
    assert_eq!(UriStyleSetting::Plain.style(), UriStyle::Plain);
}

#[test]
fn the_uri_style_setting_is_spelled_the_way_the_rest_of_the_file_is() {
    let config =
        parse(r#"{"servers":{"csharp":{"uriStyle":"plain"},"rust":{"uriStyle":"encoded"}}}"#);

    assert_eq!(
        config.server("csharp").unwrap().uri_style,
        Some(UriStyleSetting::Plain)
    );
    assert_eq!(
        config.server("rust").unwrap().uri_style,
        Some(UriStyleSetting::Encoded)
    );
    // And out again in the same spelling, or a round trip through the UI would
    // rewrite the user's file into something it cannot read back.
    assert_eq!(
        serde_json::to_value(UriStyleSetting::Plain).unwrap(),
        serde_json::json!("plain")
    );
}

#[test]
fn only_the_keys_the_user_set_are_written_back() {
    // `.code-basics/config.json` is checked in, so an unset key appearing as
    // `null` would put noise in everyone's diff — and a `program: null` would
    // read as a deliberate blank rather than as "never configured".
    let mut config = LspConfig::default();
    config.servers.insert(
        "csharp".into(),
        ServerOverride {
            program: Some("C:/tools/roslyn.exe".into()),
            ..ServerOverride::default()
        },
    );

    let json = serde_json::to_string(&config).unwrap();
    assert_eq!(
        json,
        r#"{"servers":{"csharp":{"program":"C:/tools/roslyn.exe"}}}"#
    );
}

#[test]
fn an_lsp_block_configuring_nothing_writes_no_keys_at_all() {
    let json = serde_json::to_value(LspConfig::default()).unwrap();
    assert_eq!(json, serde_json::json!({}));
}

#[test]
fn servers_and_environment_are_written_in_a_stable_order() {
    // A `BTreeMap` rather than a `HashMap` on purpose: this file is committed,
    // and a map whose iteration order changed per process would produce a diff
    // every time anybody saved, with no change in meaning.
    let config =
        parse(r#"{"servers":{"rust":{"env":{"ZONE":"1","ALPHA":"2"}},"csharp":{},"python":{}}}"#);

    let ids: Vec<&str> = config.servers.keys().map(String::as_str).collect();
    assert_eq!(ids, ["csharp", "python", "rust"]);

    let env: Vec<&str> = config
        .server("rust")
        .unwrap()
        .env
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(env, ["ALPHA", "ZONE"]);
}

#[test]
fn a_block_survives_a_round_trip_through_json_unchanged() {
    let original = parse(
        r#"{"servers":{"csharp":{"enabled":true,"program":"C:/tools/roslyn.exe",
            "args":["--stdio","--autoLoadProjects"],"env":{"DOTNET_NOLOGO":"1"},
            "uriStyle":"plain"},"python":{"enabled":false}}}"#,
    );

    let round_tripped: LspConfig =
        serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
    assert_eq!(round_tripped, original);
}
