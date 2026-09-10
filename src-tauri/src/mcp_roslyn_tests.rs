use super::*;

/// Drive `call_tool` with the registry pointed at a file that does not exist —
/// the situation a client is in whenever no application is running.
fn answer(tool: &str, workspace: Option<&str>) -> ToolAnswer {
    let previous = std::env::var_os("CB_ROSLYN_INSTANCES_PATH");
    std::env::set_var(
        "CB_ROSLYN_INSTANCES_PATH",
        r"C:\nowhere\no-such-roslyn-registry.json",
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread runtime");
    let answer = runtime.block_on(call_tool(tool, Value::Null, workspace, None));
    match previous {
        Some(value) => std::env::set_var("CB_ROSLYN_INSTANCES_PATH", value),
        None => std::env::remove_var("CB_ROSLYN_INSTANCES_PATH"),
    }
    answer
}

/// The name is checked **before** the registry and before the workspace, so an
/// unknown tool is refused by name rather than by "start code-basics and retry"
/// about a tool that will never exist.
#[test]
fn an_unknown_tool_is_refused_by_name_before_anything_else() {
    let answer = answer("get_symbol_info", Some(r"C:\repo"));
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("unknown_tool"));
    assert!(answer.text.contains("get_symbol_info"), "{}", answer.text);
}

/// An unscoped install reaches no session, so a known tool with no `--workspace`
/// is refused as `noWorkspace` — before any registry is read, and distinct from
/// "no application is running".
#[test]
#[cfg(windows)]
fn a_known_tool_with_no_workspace_refuses_as_no_workspace() {
    let answer = answer(cb_core::roslyn::tools::FIND_REFERENCES, None);
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("noWorkspace"));
    assert_ne!(answer.code.as_deref(), Some("app_not_running"));
}

/// A blank workspace is no workspace.
#[test]
#[cfg(windows)]
fn a_blank_workspace_is_treated_as_none() {
    let answer = answer(cb_core::roslyn::tools::GET_DIAGNOSTICS, Some("   "));
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("noWorkspace"));
}

/// The name guard is only the *name*: an advertised tool with a workspace goes on
/// to look for an application, so a name guard cannot swallow the real refusal.
#[test]
#[cfg(windows)]
fn an_advertised_tool_with_a_workspace_reports_that_no_application_is_running() {
    let answer = answer(cb_core::roslyn::tools::FIND_REFERENCES, Some(r"C:\repo"));
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("app_not_running"));
}
