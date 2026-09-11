use super::*;

use std::fs;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cb-redis-store-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.join("redis-connections.json")
}

fn conn(id: &str) -> RedisConnection {
    RedisConnection {
        id: id.to_string(),
        name: id.to_string(),
        secret: SecretSource::AppSettings {
            path: PathBuf::from("appsettings.json"),
            key: "ConnectionStrings:Redis".to_string(),
        },
        workspace_root: None,
        allow_writes: false,
        expose_to_agents: false,
        user_named: false,
        created_at_ms: 1,
        last_used_ms: None,
    }
}

#[test]
fn a_missing_or_corrupt_file_loads_empty() {
    let path = scratch("missing").with_file_name("nope.json");
    assert_eq!(load(&path), RedisConnectionsFile::default());

    let bad = scratch("corrupt");
    fs::write(&bad, "not json {{").unwrap();
    assert_eq!(load(&bad), RedisConnectionsFile::default());
    let _ = fs::remove_dir_all(bad.parent().unwrap());
}

#[test]
fn save_then_load_round_trips_and_is_atomic() {
    let path = scratch("round-trip");
    let mut file = RedisConnectionsFile::default();
    upsert(&mut file, conn("a"));
    save(&path, &file).unwrap();
    assert_eq!(load(&path), file);
    // No temp file left behind.
    let left: Vec<String> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, vec!["redis-connections.json".to_string()]);
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_literal_is_the_only_source_that_holds_a_secret() {
    assert!(SecretSource::Literal {
        connection_string: "redis://:pw@h".to_string()
    }
    .holds_a_secret());
    assert!(!SecretSource::AppSettings {
        path: PathBuf::from("appsettings.json"),
        key: "Redis".to_string(),
    }
    .holds_a_secret());
}

#[test]
fn a_new_profile_starts_with_both_consents_off_whatever_the_payload_says() {
    let mut file = RedisConnectionsFile::default();
    let mut c = conn("x");
    c.allow_writes = true;
    c.expose_to_agents = true;
    upsert(&mut file, c);
    let saved = &file.connections[0];
    assert!(!saved.allow_writes, "a save cannot turn writes on");
    assert!(!saved.expose_to_agents, "a save cannot raise exposure");
}

#[test]
fn upsert_preserves_consent_across_a_replace() {
    let mut file = RedisConnectionsFile::default();
    upsert(&mut file, conn("x"));
    set_allow_writes(&mut file, "x", true);
    set_expose_to_agents(&mut file, "x", true);

    // A form re-save arrives with the flags off; the stored ones must survive.
    let mut resave = conn("x");
    resave.name = "renamed via form".to_string();
    resave.allow_writes = false;
    resave.expose_to_agents = false;
    upsert(&mut file, resave);

    let saved = &file.connections[0];
    assert!(saved.allow_writes, "a re-save cannot lower writes");
    assert!(saved.expose_to_agents, "a re-save cannot lower exposure");
    assert_eq!(saved.name, "renamed via form");
}

#[test]
fn rename_marks_user_named() {
    let mut file = RedisConnectionsFile::default();
    upsert(&mut file, conn("x"));
    assert!(rename(&mut file, "x", "My cache"));
    assert!(file.connections[0].user_named);
    assert_eq!(file.connections[0].name, "My cache");
    assert!(!rename(&mut file, "missing", "nope"));
}

#[test]
fn remove_reports_whether_it_removed() {
    let mut file = RedisConnectionsFile::default();
    upsert(&mut file, conn("x"));
    assert!(remove(&mut file, "x"));
    assert!(!remove(&mut file, "x"));
    assert!(file.connections.is_empty());
}
