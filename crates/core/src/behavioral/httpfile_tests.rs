use super::*;

#[test]
fn parses_named_request_and_readiness() {
    let text = "\
@host = http://localhost:5000

# @readiness GET {{host}}/health 200 timeout=10s interval=250ms

### get-user
GET {{host}}/api/users/1
Accept: application/json

";
    let s = parse_http_file(text);
    assert_eq!(s.variables.get("host").map(String::as_str), Some("http://localhost:5000"));

    let r = s.readiness.expect("readiness parsed");
    assert_eq!(r.method, "GET");
    assert_eq!(r.url, "http://localhost:5000/health");
    assert_eq!(r.expect_status, 200);
    assert_eq!(r.timeout, std::time::Duration::from_secs(10));
    assert_eq!(r.poll_interval, std::time::Duration::from_millis(250));

    assert_eq!(s.requests.len(), 1);
    let req = &s.requests[0];
    assert_eq!(req.name, "get-user");
    assert_eq!(req.method, "GET");
    assert_eq!(req.url, "http://localhost:5000/api/users/1");
    assert_eq!(req.headers, vec![("Accept".into(), "application/json".into())]);
    assert!(req.body.is_none());
}

#[test]
fn skips_script_block_with_warning() {
    let text = "\
### create
POST http://localhost/api
Content-Type: application/json

{\"name\":\"x\"}

> {%
  client.test(\"ok\", () => {});
%}
";
    let s = parse_http_file(text);
    assert_eq!(s.requests.len(), 1);
    assert_eq!(s.requests[0].method, "POST");
    assert_eq!(s.requests[0].body.as_deref(), Some("{\"name\":\"x\"}"));
    assert!(
        s.warnings.iter().any(|w| w.contains("response-handler script")),
        "warnings: {:?}",
        s.warnings
    );
}

#[test]
fn unresolved_variable_warns_and_is_left_in_place() {
    let text = "### r\nGET http://localhost/{{missing}}\n";
    let s = parse_http_file(text);
    assert_eq!(s.requests[0].url, "http://localhost/{{missing}}");
    assert!(s.warnings.iter().any(|w| w.contains("missing")));
}

#[test]
fn multiple_requests_split_by_hashes() {
    let text = "\
GET http://localhost/a

### second
POST http://localhost/b
";
    let s = parse_http_file(text);
    assert_eq!(s.requests.len(), 2);
    assert_eq!(s.requests[0].method, "GET");
    assert_eq!(s.requests[0].url, "http://localhost/a");
    // First request had no name → defaulted.
    assert_eq!(s.requests[0].name, "request 1");
    assert_eq!(s.requests[1].name, "second");
    assert_eq!(s.requests[1].method, "POST");
}

#[test]
fn bare_url_defaults_to_get() {
    let s = parse_http_file("### r\nhttp://localhost/x\n");
    assert_eq!(s.requests[0].method, "GET");
    assert_eq!(s.requests[0].url, "http://localhost/x");
}

#[test]
fn readiness_without_status_is_ignored_with_warning() {
    let s = parse_http_file("# @readiness GET http://localhost/health\n");
    assert!(s.readiness.is_none());
    assert!(s.warnings.iter().any(|w| w.contains("@readiness")));
}

#[test]
fn strips_http_version_suffix_from_request_line() {
    let s = parse_http_file("### r\nGET http://localhost/x HTTP/1.1\n");
    assert_eq!(s.requests[0].url, "http://localhost/x");
}

#[test]
fn discover_finds_http_and_rest_and_skips_ignored_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.http"), "GET http://x/\n").unwrap();
    std::fs::create_dir_all(root.join("nested")).unwrap();
    std::fs::write(root.join("nested").join("b.rest"), "GET http://x/\n").unwrap();
    // Under a SKIP_DIRS directory — must not be discovered.
    std::fs::create_dir_all(root.join("node_modules")).unwrap();
    std::fs::write(root.join("node_modules").join("c.http"), "GET http://x/\n").unwrap();
    // A non-scenario file that must be ignored.
    std::fs::write(root.join("readme.md"), "# hi\n").unwrap();

    let found = discover_http_files(root);
    let names: Vec<String> = found
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    assert_eq!(names, vec!["a.http".to_string(), "b.rest".to_string()]);
    assert!(found.iter().all(|p| p.is_absolute()));
    assert!(
        !names.iter().any(|n| n == "c.http"),
        "node_modules should be skipped"
    );
}

#[test]
fn discover_matches_extension_case_insensitively() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("A.HTTP"), "GET http://x/\n").unwrap();
    let found = discover_http_files(dir.path());
    assert_eq!(found.len(), 1);
}
