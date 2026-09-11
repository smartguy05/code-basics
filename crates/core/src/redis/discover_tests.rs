//! Tests for [`super::discover`]. Each builds a real temp workspace and runs the
//! real [`crate::workspace::scan`], like the SQL discoverer's tests.

use super::*;
use crate::workspace::scan;

fn scanned(files: &[(&str, &str)]) -> (tempfile::TempDir, Discovery) {
    let dir = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, contents).unwrap();
    }
    let ws = scan(dir.path()).unwrap();
    let out = discover(&ws);
    (dir, out)
}

fn csproj() -> String {
    "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup>\n</Project>".to_string()
}

fn names(out: &Discovery) -> Vec<&str> {
    out.candidates.iter().map(|c| c.name.as_str()).collect()
}

#[test]
fn finds_a_redis_connection_string_and_ignores_a_sql_one() {
    let appsettings = r#"{
        "ConnectionStrings": {
            "Redis": "localhost:6379,ssl=false",
            "Db": "Server=localhost;Database=app;User Id=sa;Password=x;"
        }
    }"#;
    let (_dir, out) = scanned(&[("App.csproj", &csproj()), ("appsettings.json", appsettings)]);

    assert_eq!(
        names(&out),
        vec!["Redis"],
        "only the redis connection is a candidate"
    );
    let redis = &out.candidates[0];
    assert!(matches!(redis.state, CandidateState::Ready));
    assert_eq!(redis.display.port, Some(6379));
    assert_eq!(
        redis.source,
        SecretSource::AppSettings {
            path: _dir.path().join("appsettings.json"),
            key: "ConnectionStrings:Redis".to_string(),
        }
    );
}

#[test]
fn finds_a_redis_url_by_its_value_even_under_a_generic_name() {
    let appsettings = r#"{ "ConnectionStrings": { "Cache": "redis://localhost:6379/0" } }"#;
    let (_dir, out) = scanned(&[("App.csproj", &csproj()), ("appsettings.json", appsettings)]);
    assert_eq!(names(&out), vec!["Cache"]);
    assert!(out.candidates[0].display.host.is_some());
}

#[test]
fn a_placeholder_value_is_unresolved() {
    let env = "REDIS_URL=redis://${REDIS_HOST}:6379\n";
    let (_dir, out) = scanned(&[("package.json", r#"{"name":"app"}"#), (".env", env)]);
    assert_eq!(names(&out), vec!["REDIS_URL"]);
    assert!(matches!(
        out.candidates[0].state,
        CandidateState::Unresolved { .. }
    ));
}

#[test]
fn finds_a_dotenv_redis_url() {
    let (_dir, out) = scanned(&[
        ("package.json", r#"{"name":"app"}"#),
        (".env", "REDIS_URL=redis://localhost:6379\nOTHER=hello\n"),
    ]);
    assert_eq!(names(&out), vec!["REDIS_URL"]);
    assert!(matches!(
        out.candidates[0].source,
        SecretSource::DotEnv { .. }
    ));
}

#[test]
fn a_discovered_candidate_never_carries_the_connection_string() {
    let appsettings = r#"{ "ConnectionStrings": { "Redis": "rediss://user:secretpw@h:6380/1" } }"#;
    let (_dir, out) = scanned(&[("App.csproj", &csproj()), ("appsettings.json", appsettings)]);
    let json = serde_json::to_string(&out).unwrap();
    assert!(
        !json.contains("secretpw"),
        "discovery leaked the password: {json}"
    );
    assert!(out.candidates[0].display.has_password);
}

#[test]
fn a_workspace_with_no_redis_finds_nothing() {
    let appsettings = r#"{ "ConnectionStrings": { "Db": "Server=h;Database=d;" } }"#;
    let (_dir, out) = scanned(&[("App.csproj", &csproj()), ("appsettings.json", appsettings)]);
    assert!(out.candidates.is_empty());
}
