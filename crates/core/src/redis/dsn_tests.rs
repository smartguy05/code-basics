use super::*;

#[test]
fn parses_a_plain_url() {
    let t = parse("redis://localhost:6379/2").unwrap();
    assert_eq!(t.host, "localhost");
    assert_eq!(t.port, 6379);
    assert_eq!(t.db, 2);
    assert!(!t.use_tls);
    assert_eq!(t.password, None);
}

#[test]
fn parses_tls_and_userinfo() {
    let t = parse("rediss://alice:s3cret@cache.example.com:6380/1").unwrap();
    assert_eq!(t.host, "cache.example.com");
    assert_eq!(t.port, 6380);
    assert_eq!(t.db, 1);
    assert!(t.use_tls);
    assert_eq!(t.username.as_deref(), Some("alice"));
    assert_eq!(t.password.as_deref(), Some("s3cret"));
}

#[test]
fn a_url_without_port_uses_the_default() {
    let t = parse("redis://localhost").unwrap();
    assert_eq!(t.port, DEFAULT_PORT);
    assert_eq!(t.db, 0);
}

#[test]
fn percent_decodes_a_password() {
    let t = parse("redis://:p%40ss%2Fword@host:6379").unwrap();
    assert_eq!(t.password.as_deref(), Some("p@ss/word"));
}

#[test]
fn parses_the_stackexchange_host_list_form() {
    let t = parse("cache:6380,password=secret,ssl=true,defaultDatabase=3").unwrap();
    assert_eq!(t.host, "cache");
    assert_eq!(t.port, 6380);
    assert!(t.use_tls);
    assert_eq!(t.db, 3);
    assert_eq!(t.password.as_deref(), Some("secret"));
}

#[test]
fn a_sql_connection_string_is_refused() {
    assert!(parse("Server=localhost;Database=app;User Id=sa;Password=x;").is_err());
}

#[test]
fn empty_is_refused() {
    assert!(parse("   ").is_err());
}

#[test]
fn looks_like_redis_recognises_urls_and_host_lists() {
    assert!(looks_like_redis("redis://localhost:6379"));
    assert!(looks_like_redis("rediss://h:6380/0"));
    assert!(looks_like_redis("localhost:6379,ssl=true"));
    assert!(looks_like_redis("cache:6380,abortConnect=false,password=x"));
}

#[test]
fn looks_like_redis_abstains_on_ambiguous_and_sql() {
    // A bare host:port with no redis marker is not claimed.
    assert!(!looks_like_redis("localhost:6379"));
    // A SQL connection string is never claimed.
    assert!(!looks_like_redis("Server=host,1433;Database=x;"));
    assert!(!looks_like_redis("postgres://u:p@host/db"));
    assert!(!looks_like_redis(""));
}

#[test]
fn display_form_never_carries_the_password() {
    let d = display_form("rediss://alice:s3cret@host:6380/1");
    assert_eq!(d.host.as_deref(), Some("host"));
    assert_eq!(d.port, Some(6380));
    assert_eq!(d.db, Some(1));
    assert!(d.uses_tls);
    assert!(d.has_password);
    let json = serde_json::to_string(&d).unwrap();
    assert!(
        !json.contains("s3cret"),
        "display leaked the password: {json}"
    );
}

#[test]
fn display_form_of_unparseable_is_all_absent() {
    let d = display_form("Server=x;Database=y;");
    assert_eq!(d.host, None);
    assert!(!d.has_password);
}
