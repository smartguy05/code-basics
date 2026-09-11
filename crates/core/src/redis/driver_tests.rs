//! Only the pure helpers are tested here — the connection methods need a live
//! server and are exercised manually. The value-shaping and redaction rules are
//! what matter for correctness, and they are pure.

use super::*;

#[test]
fn cap_string_reports_when_it_cuts() {
    let short = b"hello";
    assert_eq!(cap_string(short), ("hello".to_string(), false));

    let long = vec![b'a'; MAX_STRING_BYTES + 10];
    let (text, truncated) = cap_string(&long);
    assert!(truncated);
    assert_eq!(text.len(), MAX_STRING_BYTES);
}

#[test]
fn cap_list_reports_when_it_cuts() {
    let few: Vec<Vec<u8>> = vec![b"a".to_vec(), b"b".to_vec()];
    assert_eq!(
        cap_list(few),
        (vec!["a".to_string(), "b".to_string()], false)
    );

    let many: Vec<Vec<u8>> = (0..MAX_COLLECTION_ELEMENTS + 5)
        .map(|i| i.to_string().into_bytes())
        .collect();
    let (items, truncated) = cap_list(many);
    assert!(truncated);
    assert_eq!(items.len(), MAX_COLLECTION_ELEMENTS);
}

#[test]
fn scores_print_like_redis_cli() {
    assert_eq!(format_score(1.0), "1");
    assert_eq!(format_score(1.5), "1.5");
    assert_eq!(format_score(-3.0), "-3");
}

#[test]
fn connection_info_maps_tls_and_db() {
    let target = RedisTarget {
        host: "h".to_string(),
        port: 6380,
        db: 2,
        use_tls: true,
        username: None,
        password: Some("pw".to_string()),
    };
    let info = connection_info(&target);
    assert!(matches!(
        info.addr,
        rds::ConnectionAddr::TcpTls { port: 6380, .. }
    ));
    assert_eq!(info.redis.db, 2);
    assert_eq!(info.redis.password.as_deref(), Some("pw"));
}

#[test]
fn a_driver_error_message_is_redacted() {
    let err = DriverError::new(
        RedisStatusKind::Failed,
        "auth failed for redis://user:hunter2@host",
    );
    assert!(!err.message.contains("hunter2"), "leaked: {}", err.message);
}
