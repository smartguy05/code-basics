use super::*;

use std::fs;

/// A temp directory of this test's own, cleared before use so a leftover from a
/// failed run does not leak in.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cb-sql-connections-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.join(SQL_CONNECTIONS_FILE)
}

/// A connection the user typed by hand, secret and all.
fn typed(id: &str, name: &str, connection_string: &str) -> SqlConnection {
    SqlConnection {
        id: id.into(),
        name: name.into(),
        engine: Some(SqlEngine::Postgres),
        secret: SecretSource::Literal {
            connection_string: connection_string.into(),
        },
        workspace_root: Some(PathBuf::from("C:/code/shop")),
        allow_writes: false,
        created_at_ms: 1_000,
        last_used_ms: Some(2_000),
    }
}

// ---------------------------------------------------------------------------
// Tolerant load
// ---------------------------------------------------------------------------

#[test]
fn a_missing_store_loads_as_empty() {
    let path = scratch("missing").with_file_name("does-not-exist.json");
    let loaded = load(&path);
    assert_eq!(loaded, SqlConnectionsFile::default());
    assert_eq!(loaded.version, 1);
    assert!(loaded.connections.is_empty());
}

#[test]
fn a_corrupt_store_loads_as_empty_rather_than_erroring() {
    let path = scratch("corrupt");
    fs::write(&path, "{ not json at all ][").unwrap();
    assert_eq!(load(&path), SqlConnectionsFile::default());
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

// ---------------------------------------------------------------------------
// Consent
// ---------------------------------------------------------------------------

#[test]
fn allow_writes_defaults_to_false() {
    // The consent flag for the read-only guard. A hand-written or older file
    // that does not mention it must load as *withheld*, never as granted, and
    // an absent key must not make the whole entry unparseable either — a
    // connection that silently vanished would be indistinguishable from one
    // whose consent silently appeared.
    let path = scratch("allow-writes-default");
    fs::write(
        &path,
        r#"{
          "version": 1,
          "connections": [
            {
              "id": "c1",
              "name": "Local",
              "engine": "postgres",
              "secret": { "kind": "literal", "connectionString": "postgres://u:p@h/db" },
              "workspaceRoot": "C:/code/shop",
              "createdAtMs": 1000,
              "lastUsedMs": null
            }
          ]
        }"#,
    )
    .unwrap();

    let loaded = load(&path);
    assert_eq!(
        loaded.connections.len(),
        1,
        "the entry must still load with allowWrites absent"
    );
    assert!(
        !loaded.connections[0].allow_writes,
        "a store that does not mention allowWrites must not grant it"
    );
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn allow_writes_survives_a_round_trip_when_granted() {
    // The flag is explicit in both directions: granting it and then reloading
    // must not quietly reset it, or the user would re-grant on every launch and
    // learn to click through the consent.
    let path = scratch("allow-writes-granted");
    let mut entry = typed("c1", "Local", "postgres://u:p@h/db");
    entry.allow_writes = true;
    let file = SqlConnectionsFile {
        version: 1,
        connections: vec![entry],
    };
    save(&path, &file).unwrap();
    assert!(load(&path).connections[0].allow_writes);
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

// ---------------------------------------------------------------------------
// Round trip
// ---------------------------------------------------------------------------

#[test]
fn a_saved_connection_round_trips() {
    let path = scratch("round-trip");
    let file = SqlConnectionsFile {
        version: 1,
        connections: vec![
            typed("c1", "Local", "postgres://u:p@h/db"),
            SqlConnection {
                id: "c2".into(),
                name: "Orders (appsettings)".into(),
                engine: Some(SqlEngine::SqlServer),
                secret: SecretSource::AppSettings {
                    path: PathBuf::from("C:/code/shop/src/Api/appsettings.json"),
                    key: "ConnectionStrings:Orders".into(),
                },
                workspace_root: Some(PathBuf::from("C:/code/shop")),
                allow_writes: false,
                created_at_ms: 3_000,
                last_used_ms: None,
            },
        ],
    };
    save(&path, &file).unwrap();
    assert_eq!(load(&path), file);
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn each_entry_records_the_workspace_it_was_created_under() {
    // A picker groups "this codebase first" from this field alone — there is no
    // second, per-workspace store. `None` means *created outside any workspace*
    // and is a different fact from "created under a workspace", so it round
    // trips as an explicit null rather than being skipped or defaulted.
    let path = scratch("workspace-root");
    let mut global = typed("c2", "Scratch", "postgres://u:p@h/db");
    global.workspace_root = None;
    let file = SqlConnectionsFile {
        version: 1,
        connections: vec![typed("c1", "Local", "postgres://u:p@h/db"), global],
    };
    save(&path, &file).unwrap();

    let loaded = load(&path);
    assert_eq!(
        loaded.connections[0].workspace_root,
        Some(PathBuf::from("C:/code/shop"))
    );
    assert_eq!(loaded.connections[1].workspace_root, None);

    let json = serde_json::to_value(&file).unwrap();
    assert!(
        json["connections"][1]
            .as_object()
            .unwrap()
            .contains_key("workspaceRoot"),
        "an absent workspace must cross as an explicit null, not a missing key"
    );
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

// ---------------------------------------------------------------------------
// Reference, not value
// ---------------------------------------------------------------------------

#[test]
fn a_discovered_entry_stores_a_reference_not_a_value() {
    // The whole point of the discovered variants: the app remembers *where* the
    // connection string lives and re-reads it at connect time, so it stores no
    // secret at all and a rotated password just works.
    const SECRET: &str = "Password=hunter2";

    for secret in [
        SecretSource::AppSettings {
            path: PathBuf::from("C:/code/shop/appsettings.json"),
            key: "ConnectionStrings:Orders".into(),
        },
        SecretSource::UserSecrets {
            project: PathBuf::from("C:/code/shop/src/Api/Api.csproj"),
            key: "ConnectionStrings:Orders".into(),
        },
        SecretSource::DotEnv {
            path: PathBuf::from("C:/code/shop/.env"),
            key: "DATABASE_URL".into(),
        },
    ] {
        assert!(
            !secret.holds_a_secret(),
            "a discovered source must be a reference: {secret:?}"
        );

        let entry = SqlConnection {
            secret,
            ..typed("c1", "Orders", SECRET)
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(
            !json.contains("hunter2"),
            "a discovered entry must persist no secret value: {json}"
        );
    }

    // And the one variant that does hold a value says so, so a caller can
    // refuse to hand it back across IPC. "The user typed it" and "the app knows
    // where to look" must never collapse into one answer.
    assert!(SecretSource::Literal {
        connection_string: SECRET.into(),
    }
    .holds_a_secret());
}

// ---------------------------------------------------------------------------
// Atomic save
// ---------------------------------------------------------------------------

#[test]
fn save_is_atomic_and_leaves_no_temp_file() {
    let path = scratch("no-temp");
    let file = SqlConnectionsFile {
        version: 1,
        connections: vec![typed("c1", "Local", "postgres://u:p@h/db")],
    };
    save(&path, &file).unwrap();

    let dir = path.parent().unwrap();
    let leftovers: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name != SQL_CONNECTIONS_FILE)
        .collect();
    assert!(leftovers.is_empty(), "unexpected files: {leftovers:?}");

    // The rename replaced the destination rather than appending to it.
    assert_eq!(load(&path), file);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn overwriting_a_nonempty_store_with_an_empty_one_backs_up_the_previous_content() {
    // The tolerant `load` plus a truncated write is a cascade that could erase
    // every saved connection; before an empty file replaces a non-empty one the
    // previous content is preserved. A deliberate "forget all" still applies.
    let path = scratch("empty-guard");
    let full = SqlConnectionsFile {
        version: 1,
        connections: vec![typed("c1", "Local", "postgres://u:p@h/db")],
    };
    save(&path, &full).unwrap();
    save(&path, &SqlConnectionsFile::default()).unwrap();

    assert!(
        load(&path).connections.is_empty(),
        "the empty save must still apply"
    );
    let bak = path.with_file_name(format!("{SQL_CONNECTIONS_FILE}.bak"));
    assert!(bak.exists(), "expected a .bak of the previous content");
    assert_eq!(load(&bak), full);
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn save_creates_the_parent_directory() {
    let root = std::env::temp_dir().join("cb-sql-connections-mkdir");
    let _ = fs::remove_dir_all(&root);
    let path = root.join("nested/deeper").join(SQL_CONNECTIONS_FILE);
    assert!(!path.parent().unwrap().exists());
    save(&path, &SqlConnectionsFile::default()).unwrap();
    assert!(path.exists());
    let _ = fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// Location
// ---------------------------------------------------------------------------

#[test]
fn the_env_override_wins() {
    // `CB_SQL_CONNECTIONS_PATH` replaces the whole path, ahead of `%APPDATA%`
    // and every other rung of the ladder — the same contract as
    // `CB_NOTES_PATH` and `CB_LAUNCHERS_PATH`.
    let previous = std::env::var_os(SQL_CONNECTIONS_PATH_ENV);
    let previous_appdata = std::env::var_os("APPDATA");

    std::env::set_var("APPDATA", "C:/appdata-should-lose");
    std::env::set_var(SQL_CONNECTIONS_PATH_ENV, "C:/elsewhere/my-connections.json");
    assert_eq!(
        sql_connections_path(),
        PathBuf::from("C:/elsewhere/my-connections.json")
    );

    // Without it, the config-directory ladder applies and the file keeps its
    // name inside `code-basics/`.
    std::env::remove_var(SQL_CONNECTIONS_PATH_ENV);
    assert_eq!(
        sql_connections_path(),
        PathBuf::from("C:/appdata-should-lose")
            .join("code-basics")
            .join(SQL_CONNECTIONS_FILE)
    );

    match previous {
        Some(value) => std::env::set_var(SQL_CONNECTIONS_PATH_ENV, value),
        None => std::env::remove_var(SQL_CONNECTIONS_PATH_ENV),
    }
    match previous_appdata {
        Some(value) => std::env::set_var("APPDATA", value),
        None => std::env::remove_var("APPDATA"),
    }
}

// ---------------------------------------------------------------------------
// The wire contract
// ---------------------------------------------------------------------------

/// The exact camelCase keys the TypeScript side reads, and the confinement rule
/// that matters more than any of them: **no field of a connection profile
/// carries a password**. The only place a secret value may appear anywhere in
/// this file is inside a `literal` source — the case where the user typed one
/// and asked for it to be remembered — and `holds_a_secret` is what lets a
/// caller keep it from crossing IPC.
#[test]
fn serialisation_shape_pins_the_wire_keys() {
    let file = SqlConnectionsFile {
        version: 1,
        connections: vec![typed("c1", "Local", "postgres://u:p@h/db")],
    };
    let json = serde_json::to_value(&file).unwrap();

    let mut top: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    top.sort_unstable();
    assert_eq!(top, ["connections", "version"]);

    let entry = json["connections"][0].as_object().unwrap();
    let mut keys: Vec<&str> = entry.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "allowWrites",
            "createdAtMs",
            "engine",
            "id",
            "lastUsedMs",
            "name",
            "secret",
            "workspaceRoot",
        ]
    );

    // No password-shaped field on the profile itself; the secret is confined to
    // the tagged `secret` object and nothing else may grow one.
    for key in entry.keys() {
        let lower = key.to_ascii_lowercase();
        assert!(
            !lower.contains("password") && !lower.contains("connectionstring"),
            "a profile field must not carry a secret: {key}"
        );
    }

    // Each source variant's own keys.
    assert_eq!(
        serde_json::to_value(SecretSource::Literal {
            connection_string: "postgres://u:p@h/db".into(),
        })
        .unwrap(),
        serde_json::json!({ "kind": "literal", "connectionString": "postgres://u:p@h/db" })
    );
    assert_eq!(
        serde_json::to_value(SecretSource::AppSettings {
            path: PathBuf::from("C:/a/appsettings.json"),
            key: "ConnectionStrings:Orders".into(),
        })
        .unwrap(),
        serde_json::json!({
            "kind": "appSettings",
            "path": "C:/a/appsettings.json",
            "key": "ConnectionStrings:Orders",
        })
    );
    assert_eq!(
        serde_json::to_value(SecretSource::UserSecrets {
            project: PathBuf::from("C:/a/Api.csproj"),
            key: "ConnectionStrings:Orders".into(),
        })
        .unwrap(),
        serde_json::json!({
            "kind": "userSecrets",
            "project": "C:/a/Api.csproj",
            "key": "ConnectionStrings:Orders",
        })
    );
    assert_eq!(
        serde_json::to_value(SecretSource::DotEnv {
            path: PathBuf::from("C:/a/.env"),
            key: "DATABASE_URL".into(),
        })
        .unwrap(),
        serde_json::json!({ "kind": "dotEnv", "path": "C:/a/.env", "key": "DATABASE_URL" })
    );
}
