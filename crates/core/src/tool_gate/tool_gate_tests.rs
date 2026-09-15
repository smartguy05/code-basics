use super::*;

#[test]
fn an_absent_tool_is_enabled() {
    let gate = ToolGateFile::default();
    for server in ServerId::ALL {
        for tool in server.descriptors() {
            assert!(
                gate.is_enabled(server, tool.name.as_ref()),
                "{}/{} should default to on",
                server.id(),
                tool.name
            );
        }
    }
}

#[test]
fn disabling_one_server_tool_does_not_affect_the_same_name_on_another() {
    // The composite key keeps same-named tools independent. Use a synthetic name
    // so the test does not depend on two servers actually sharing one.
    let mut gate = ToolGateFile::default();
    gate.set(ServerId::Sql, "shared_name", false);
    assert!(!gate.is_enabled(ServerId::Sql, "shared_name"));
    assert!(
        gate.is_enabled(ServerId::Tasks, "shared_name"),
        "disabling sql/shared_name must not touch tasks/shared_name"
    );
}

#[test]
fn filter_descriptors_drops_the_disabled_and_keeps_order() {
    let all = ServerId::Sql.descriptors();
    assert!(all.len() >= 3, "test needs several sql tools");
    let second = all[1].name.to_string();

    let mut gate = ToolGateFile::default();
    gate.set(ServerId::Sql, &second, false);

    let filtered = filter_descriptors(&gate, ServerId::Sql, ServerId::Sql.descriptors());
    assert_eq!(filtered.len(), all.len() - 1, "exactly one dropped");
    assert!(
        !filtered.iter().any(|t| t.name == second),
        "the disabled tool is gone"
    );

    // Order of the survivors is unchanged.
    let expected: Vec<String> = all
        .iter()
        .map(|t| t.name.to_string())
        .filter(|n| n != &second)
        .collect();
    let got: Vec<String> = filtered.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(got, expected);
}

#[test]
fn an_unknown_key_in_the_store_is_inert() {
    let mut gate = ToolGateFile::default();
    gate.enabled.insert("sql/no.such.tool".to_string(), false);
    // A real tool is still on; the ghost key changes nothing observable.
    let filtered = filter_descriptors(&gate, ServerId::Sql, ServerId::Sql.descriptors());
    assert_eq!(filtered.len(), ServerId::Sql.descriptors().len());
}

#[test]
fn list_reports_every_server_with_their_real_tools() {
    let gate = ToolGateFile::default();
    let list = gate.list();
    assert_eq!(list.len(), ServerId::ALL.len());

    for (info, server) in list.iter().zip(ServerId::ALL) {
        assert_eq!(info.id, server.id());
        assert_eq!(info.label, server.label());
        let names: Vec<String> = info.tools.iter().map(|t| t.name.clone()).collect();
        let expected: Vec<String> = server
            .descriptors()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        assert_eq!(names, expected, "{} tool list", server.id());
        assert!(
            info.tools.iter().all(|t| t.enabled),
            "default is every tool on"
        );
    }
}

#[test]
fn list_reflects_a_disabled_tool() {
    let sql = ServerId::Sql.descriptors();
    let first = sql[0].name.to_string();

    let mut gate = ToolGateFile::default();
    gate.set(ServerId::Sql, &first, false);

    let list = gate.list();
    let sql_row = list.iter().find(|s| s.id == "sql").unwrap();
    let tool = sql_row.tools.iter().find(|t| t.name == first).unwrap();
    assert!(!tool.enabled);
}

#[test]
fn serialisation_shape_pins_the_wire_keys() {
    // The `src/ipc/types.ts` mirror is hand-written; this pins the exact JSON
    // keys so the two cannot drift.
    let info = McpServerToolsInfo {
        id: "sql".to_string(),
        label: "SQL".to_string(),
        tools: vec![McpToolInfo {
            name: "sql.query".to_string(),
            description: "run one read-only statement".to_string(),
            enabled: true,
        }],
    };
    let json = serde_json::to_value(&info).unwrap();
    assert_eq!(json["id"], "sql");
    assert_eq!(json["label"], "SQL");
    let tool = &json["tools"][0];
    assert_eq!(tool["name"], "sql.query");
    assert_eq!(tool["description"], "run one read-only statement");
    assert_eq!(tool["enabled"], true);
}

#[test]
fn server_id_round_trips_through_its_string() {
    for server in ServerId::ALL {
        assert_eq!(ServerId::from_id(server.id()), Ok(server));
    }
    assert!(ServerId::from_id("nope").is_err());
}

#[test]
fn every_server_has_a_distinct_stable_id_and_label() {
    let mut ids: Vec<&str> = ServerId::ALL.iter().map(|s| s.id()).collect();
    ids.sort_unstable();
    let count = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), count, "two servers share an id");
    assert_eq!(
        ids,
        vec!["browser", "build", "editor", "roslyn", "sql", "tasks"]
    );
}

#[test]
fn the_editor_context_server_is_known() {
    // Stage 2: the editor_context module now supplies the real four tools.
    assert_eq!(ServerId::EditorContext.id(), "editor");
    assert_eq!(ServerId::EditorContext.label(), "Editor context");
    let names: Vec<String> = ServerId::EditorContext
        .descriptors()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert_eq!(
        names,
        vec![
            "get_active_file",
            "get_selection",
            "get_open_files",
            "get_recent_files"
        ]
    );
}

#[test]
fn the_build_server_is_known() {
    assert_eq!(ServerId::Build.id(), "build");
    assert_eq!(ServerId::Build.label(), "Build");
    let names: Vec<String> = ServerId::Build
        .descriptors()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert_eq!(
        names,
        vec![
            "build_solution",
            "get_errors",
            "get_warnings",
            "get_build_status"
        ]
    );
}
