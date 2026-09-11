use super::*;
use crate::mcp::expose::connection_handle;
use crate::redis::store::{RedisConnection, RedisConnectionsFile, SecretSource};
use std::path::PathBuf;

fn conn(id: &str, name: &str, expose: bool) -> RedisConnection {
    RedisConnection {
        id: id.to_string(),
        name: name.to_string(),
        secret: SecretSource::AppSettings {
            path: PathBuf::from("appsettings.json"),
            key: "ConnectionStrings:Redis".to_string(),
        },
        workspace_root: None,
        allow_writes: false,
        expose_to_agents: expose,
        user_named: true,
        created_at_ms: 1,
        last_used_ms: None,
    }
}

fn file(conns: Vec<RedisConnection>) -> RedisConnectionsFile {
    RedisConnectionsFile {
        version: 1,
        connections: conns,
    }
}

#[test]
fn only_exposed_connections_are_listed() {
    let f = file(vec![conn("a", "A", true), conn("b", "B", false)]);
    let listed = listing(&f, None);
    assert_eq!(listed.connections.len(), 1);
    assert_eq!(listed.connections[0].name, "A");
}

#[test]
fn the_agent_view_carries_a_handle_not_the_stored_id_or_a_path() {
    let f = file(vec![conn(
        "appsettings:src/Api/appsettings.json:ConnectionStrings:Redis",
        "Api · src/Api/appsettings.json · Redis",
        true,
    )]);
    let listed = listing(&f, None);
    let view = &listed.connections[0];
    assert!(view.id.starts_with("conn-"), "id is a handle: {}", view.id);
    assert!(!view.id.contains('/'), "handle carries no path");
    assert!(
        !view.name.contains('/'),
        "name carries no path: {}",
        view.name
    );
}

#[test]
fn find_resolves_a_handle_and_refuses_an_unexposed_one() {
    let f = file(vec![conn("a", "A", true), conn("b", "B", false)]);
    let handle = connection_handle("a");
    assert!(find_exposed(&f, &handle).is_ok());
    // Not exposed → a distinct answer from not found.
    assert_eq!(
        find_exposed(&f, "B"),
        Err(McpRefusal::ConnectionNotExposed {
            selector: "B".into()
        })
    );
    assert_eq!(
        find_exposed(&f, "nope"),
        Err(McpRefusal::ConnectionNotFound {
            selector: "nope".into()
        })
    );
}

#[test]
fn empty_listing_tells_nothing_saved_from_nothing_exposed() {
    assert_eq!(
        empty_listing_refusal(&file(vec![])),
        McpRefusal::NoConnectionsSaved
    );
    assert_eq!(
        empty_listing_refusal(&file(vec![conn("a", "A", false)])),
        McpRefusal::NoConnectionExposed { total_saved: 1 }
    );
}

#[test]
fn a_scope_reports_what_it_left_out() {
    let mut a = conn("a", "A", true);
    a.workspace_root = Some(PathBuf::from("/repo"));
    let b = conn("b", "B", true); // no workspace
    let f = file(vec![a, b]);
    let listed = listing(&f, Some(std::path::Path::new("/repo")));
    assert_eq!(listed.connections.len(), 1);
    assert_eq!(listed.scoped_out, 1);
}
