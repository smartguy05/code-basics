use super::*;

use std::fs;
use std::path::PathBuf;

use crate::sql::store::{SecretSource, SqlConnectionsFile};

fn profile(id: &str, name: &str, exposed: bool) -> SqlConnection {
    SqlConnection {
        id: id.into(),
        name: name.into(),
        engine: Some(SqlEngine::Postgres),
        secret: SecretSource::AppSettings {
            path: PathBuf::from("C:/code/shop/appsettings.json"),
            key: "ConnectionStrings:Default".into(),
        },
        workspace_root: Some(PathBuf::from("C:/code/shop")),
        allow_writes: false,
        expose_to_agents: exposed,
        user_named: true,
        created_at_ms: 1_000,
        last_used_ms: None,
    }
}

fn file(connections: Vec<SqlConnection>) -> SqlConnectionsFile {
    SqlConnectionsFile {
        version: 1,
        connections,
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cb-mcp-expose-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.join("sql-connections.json")
}

// ---------------------------------------------------------------------------
// The narrow view
// ---------------------------------------------------------------------------

#[test]
fn an_agent_connection_has_no_field_that_could_carry_a_secret_or_a_path() {
    // Every place a secret or a path could reach this view is given its own
    // sentinel; none of them may appear in what crosses to an agent.
    let mut connection = profile("conn-1", "Reporting", true);
    connection.secret = SecretSource::Literal {
        connection_string: "Server=db;Password=DSN-SENTINEL".into(),
    };
    connection.workspace_root = Some(PathBuf::from("C:/code/ROOT-SENTINEL"));

    let dsn_referenced = SqlConnection {
        secret: SecretSource::AppSettings {
            path: PathBuf::from("C:/code/SETTINGS-SENTINEL/appsettings.json"),
            key: "ConnectionStrings:KEY-SENTINEL".into(),
        },
        ..connection.clone()
    };

    for source in [&connection, &dsn_referenced] {
        let json = serde_json::to_string(&AgentConnection::of(source)).unwrap();
        for sentinel in [
            "DSN-SENTINEL",
            "ROOT-SENTINEL",
            "SETTINGS-SENTINEL",
            "KEY-SENTINEL",
        ] {
            assert!(
                !json.contains(sentinel),
                "{sentinel} reached the agent view: {json}"
            );
        }
        // And the three fields that are meant to be there really are. The id
        // crosses as the opaque handle now, not the stored id, so this asserts
        // the handle — see `AgentConnection::id`.
        assert!(json.contains(&connection_handle("conn-1")));
        assert!(json.contains("Reporting"));
    }
}

#[test]
fn an_undetermined_engine_crosses_as_null_rather_than_a_default() {
    let mut connection = profile("conn-1", "Reporting", true);
    connection.engine = None;
    let json = serde_json::to_string(&AgentConnection::of(&connection)).unwrap();
    assert!(json.contains("\"engine\":null"), "{json}");
}

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

#[test]
fn only_exposed_connections_are_listed() {
    let listed = listing(
        &file(vec![
            profile("a", "Alpha", true),
            profile("b", "Beta", false),
            profile("c", "Gamma", true),
        ]),
        None,
    );
    // Handles, not stored ids: a discovered id carries the file it was found
    // in, so `AgentConnection` shows an opaque handle instead. The mapping is
    // asserted here rather than the literals, so the rule is what is pinned.
    let ids: Vec<String> = listed.connections.iter().map(|c| c.id.clone()).collect();
    assert_eq!(ids, vec![connection_handle("a"), connection_handle("c")]);
    assert_eq!(listed.scoped_out, 0);
}

#[test]
fn an_absent_expose_key_is_consent_withheld() {
    // A hand-written or older file must load, and must load as unexposed.
    let json = r#"{"version":1,"connections":[{"id":"a","name":"Alpha","engine":"postgres",
        "secret":{"kind":"appSettings","path":"C:/x/appsettings.json","key":"K"},
        "workspaceRoot":null,"createdAtMs":1,"lastUsedMs":null}]}"#;
    let parsed: SqlConnectionsFile = serde_json::from_str(json).expect("it must still load");
    assert_eq!(parsed.connections.len(), 1);
    assert!(listing(&parsed, None).connections.is_empty());
}

#[test]
fn a_workspace_scope_filters_the_listing_and_reports_what_it_left_out() {
    let mut elsewhere = profile("b", "Beta", true);
    elsewhere.workspace_root = Some(PathBuf::from("C:/code/other"));
    let listed = listing(
        &file(vec![profile("a", "Alpha", true), elsewhere]),
        Some(Path::new("C:/code/shop")),
    );
    let ids: Vec<String> = listed.connections.iter().map(|c| c.id.clone()).collect();
    assert_eq!(ids, vec![connection_handle("a")]);
    assert_eq!(
        listed.scoped_out, 1,
        "a scoped listing must say what it left out rather than look complete"
    );
}

#[test]
fn a_scope_is_not_a_boundary_because_a_scoped_out_connection_can_still_be_named() {
    // Stated in the docs and pinned here so it cannot be mistaken for one.
    let mut elsewhere = profile("b", "Beta", true);
    elsewhere.workspace_root = Some(PathBuf::from("C:/code/other"));
    let store = file(vec![profile("a", "Alpha", true), elsewhere]);
    assert!(listing(&store, Some(Path::new("C:/code/shop")))
        .connections
        .iter()
        .all(|c| c.id != "b"));
    assert_eq!(find_exposed(&store, "b").unwrap().id, "b");
}

#[test]
fn an_empty_listing_says_which_of_the_two_reasons_it_is() {
    assert_eq!(
        empty_listing_refusal(&file(vec![])),
        McpRefusal::NoConnectionsSaved
    );
    assert_eq!(
        empty_listing_refusal(&file(vec![
            profile("a", "Alpha", false),
            profile("b", "Beta", false)
        ])),
        McpRefusal::NoConnectionExposed { total_saved: 2 }
    );
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

#[test]
fn a_connection_resolves_by_id_and_by_name() {
    let store = file(vec![profile("conn-1", "Reporting", true)]);
    assert_eq!(find_exposed(&store, "conn-1").unwrap().id, "conn-1");
    assert_eq!(find_exposed(&store, "Reporting").unwrap().id, "conn-1");
    assert_eq!(find_exposed(&store, " reporting ").unwrap().id, "conn-1");
}

#[test]
fn an_unexposed_connection_is_not_reported_as_missing() {
    // Collapsing these hides a toggle that is one click away.
    let store = file(vec![profile("conn-1", "Reporting", false)]);
    assert_eq!(
        find_exposed(&store, "conn-1").unwrap_err(),
        McpRefusal::ConnectionNotExposed {
            selector: "conn-1".to_string()
        }
    );
    assert_eq!(
        find_exposed(&store, "nothing-like-it").unwrap_err(),
        McpRefusal::ConnectionNotFound {
            selector: "nothing-like-it".to_string()
        }
    );
}

#[test]
fn an_ambiguous_name_is_refused_and_never_picked() {
    let store = file(vec![
        profile("conn-1", "Reporting", true),
        profile("conn-2", "reporting", true),
    ]);
    match find_exposed(&store, "Reporting").unwrap_err() {
        McpRefusal::ConnectionAmbiguous {
            selector,
            candidates,
        } => {
            assert_eq!(selector, "Reporting");
            assert_eq!(candidates, vec!["conn-1".to_string(), "conn-2".to_string()]);
        }
        other => panic!("expected an ambiguity refusal, got {other:?}"),
    }
}

#[test]
fn an_unexposed_namesake_neither_creates_an_ambiguity_nor_is_named() {
    // The unexposed profile must not be reported as a candidate: that would
    // reveal the existence of something the user chose not to share.
    let store = file(vec![
        profile("conn-1", "Reporting", true),
        profile("conn-2", "Reporting", false),
    ]);
    assert_eq!(find_exposed(&store, "Reporting").unwrap().id, "conn-1");
}

#[test]
fn a_name_shared_only_by_unexposed_profiles_is_a_toggle_away_not_missing() {
    let store = file(vec![
        profile("conn-1", "Reporting", false),
        profile("conn-2", "Reporting", false),
    ]);
    assert_eq!(
        find_exposed(&store, "Reporting").unwrap_err(),
        McpRefusal::ConnectionNotExposed {
            selector: "Reporting".to_string()
        }
    );
}

#[test]
fn an_id_beats_a_name() {
    // The caller's way of being unambiguous must actually be unambiguous.
    let mut named_like_the_other_id = profile("conn-2", "conn-1", true);
    named_like_the_other_id.name = "conn-1".into();
    let store = file(vec![
        profile("conn-1", "Reporting", true),
        named_like_the_other_id,
    ]);
    assert_eq!(find_exposed(&store, "conn-1").unwrap().id, "conn-1");
}

#[test]
fn an_empty_selector_is_not_a_match() {
    let store = file(vec![profile("conn-1", "Reporting", true)]);
    assert!(matches!(
        find_exposed(&store, "   ").unwrap_err(),
        McpRefusal::ConnectionNotFound { .. }
    ));
}

// ---------------------------------------------------------------------------
// Revocation
// ---------------------------------------------------------------------------

#[test]
fn exposed_connections_are_read_per_call_and_never_cached() {
    // The server process is long-lived. If this list were memoised, revoking
    // exposure in the app would not take effect until a restart, and nothing
    // would tell the user that.
    let path = scratch("revocation");
    let exposed = file(vec![profile("conn-1", "Reporting", true)]);
    fs::write(&path, serde_json::to_string(&exposed).unwrap()).unwrap();
    assert_eq!(exposed_now(&path, None).connections.len(), 1);

    let revoked = file(vec![profile("conn-1", "Reporting", false)]);
    fs::write(&path, serde_json::to_string(&revoked).unwrap()).unwrap();
    assert!(
        exposed_now(&path, None).connections.is_empty(),
        "revoking exposure must take effect on the very next call"
    );

    // And the reverse, so this cannot pass by always returning nothing.
    fs::write(&path, serde_json::to_string(&exposed).unwrap()).unwrap();
    assert_eq!(exposed_now(&path, None).connections.len(), 1);
}

#[test]
fn a_missing_or_corrupt_store_lists_nothing_rather_than_failing() {
    let path = scratch("corrupt");
    assert!(exposed_now(&path, None).connections.is_empty());
    fs::write(&path, "{not json").unwrap();
    assert!(exposed_now(&path, None).connections.is_empty());
}

// ---------------------------------------------------------------------------
// The sentinel test above passes vacuously for a DISCOVERED connection.
//
// Its fixture hard-codes `id: "conn-1"` and `name: "Reporting"` — literals that
// could not be path-derived — so it only ever proved that `of()` drops `secret`
// and `workspace_root`. But `sql::discover` builds ids as
// `appsettings:<workspace-relative path>:<key>` / `dotenv:…` / `usersecrets:…`,
// and the frontend's `candidateConnectionLabel` composes the *name* out of the
// project and the origin path too. Both are copied verbatim into
// `AgentConnection` and printed by `sql.list_connections`, so the doc comment's
// claim that `name` "is not derived from a path here" was false for every
// connection the app discovered rather than the user typed.
// ---------------------------------------------------------------------------

/// The id shapes `sql::discover` really produces, with the path part replaced by
/// a sentinel so a leak is unambiguous.
fn discovered_ids() -> Vec<String> {
    vec![
        "appsettings:src/PATH-SENTINEL/appsettings.json:ConnectionStrings:Db".to_string(),
        "dotenv:services/PATH-SENTINEL/.env:DATABASE_URL".to_string(),
        "usersecrets:src/PATH-SENTINEL/Api.csproj:ConnectionStrings:Db".to_string(),
    ]
}

#[test]
fn a_discovered_connections_path_derived_id_does_not_reach_the_agent_verbatim() {
    for id in discovered_ids() {
        let mut connection = profile(&id, "Reporting", true);
        connection.name = "Api · appsettings.json · ConnectionStrings:Db".into();

        let view = AgentConnection::of(&connection);
        let json = serde_json::to_string(&view).expect("serialises");

        assert!(
            !json.contains("PATH-SENTINEL"),
            "a discovered id carries the file path it was found in, and it reached the agent: {json}"
        );
    }
}

#[test]
fn a_discovered_connection_is_still_addressable_by_whatever_id_the_agent_is_shown() {
    // Whatever we show must round-trip, or the agent cannot name the connection
    // it was just told about — which would trade a leak for a broken tool.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("sql-connections.json");
    let id = "appsettings:src/Api/appsettings.json:ConnectionStrings:Db";
    let mut connection = profile(id, "Reporting", true);
    connection.name = "Api · appsettings.json · ConnectionStrings:Db".into();
    store::save(
        &path,
        &SqlConnectionsFile {
            version: 1,
            connections: vec![connection],
        },
    )
    .expect("save");
    let file = store::load(&path);

    let listing = listing(&file, None);
    let shown = listing.connections.first().expect("one exposed connection");

    let found = find_exposed(&file, &shown.id).expect("the id we showed must resolve");
    assert_eq!(
        id, found.id,
        "it must resolve to the real stored connection"
    );
}

#[test]
fn a_user_typed_name_is_still_shown_as_the_user_typed_it() {
    // The fix must not flatten every name: a label the user chose is theirs and
    // is the whole point of the field.
    let mut connection = profile("conn-1", "Reporting", true);
    connection.user_named = true;
    let view = AgentConnection::of(&connection);
    assert_eq!("Reporting", view.name);
}

#[test]
fn a_shown_name_never_carries_a_path() {
    // `discover` falls back to the absolute path when it cannot make the origin
    // workspace-relative, so the label is not guaranteed filename-only at the
    // source. This is the backstop.
    let mut connection = profile("conn-1", "x", true);
    connection.name =
        r"C:\code\secret-client\Api · C:\code\secret-client\appsettings.json · Db".into();

    let view = AgentConnection::of(&connection);

    assert!(!view.name.contains('\\'), "{}", view.name);
    assert!(!view.name.contains('/'), "{}", view.name);
    assert_eq!("Api · appsettings.json · Db", view.name);
}

#[test]
fn a_handle_is_stable_for_one_id_and_differs_between_ids() {
    // Stable so an agent may cache it across calls; distinct so two connections
    // never collide into one.
    assert_eq!(connection_handle("abc"), connection_handle("abc"));
    assert_ne!(connection_handle("abc"), connection_handle("abd"));
    assert!(connection_handle("abc").starts_with("conn-"));
}

#[test]
fn the_stored_id_still_resolves_alongside_the_handle() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("sql-connections.json");
    let id = "appsettings:src/Api/appsettings.json:ConnectionStrings:Db";
    store::save(
        &path,
        &SqlConnectionsFile {
            version: 1,
            connections: vec![profile(id, "Reporting", true)],
        },
    )
    .expect("save");
    let file = store::load(&path);

    assert_eq!(id, find_exposed(&file, id).expect("stored id resolves").id);
    assert_eq!(
        id,
        find_exposed(&file, &connection_handle(id))
            .expect("handle resolves")
            .id
    );
}
