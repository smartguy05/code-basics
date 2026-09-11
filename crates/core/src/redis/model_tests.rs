use super::*;

#[test]
fn redis_type_wire_names_match_the_type_command() {
    for (variant, wire) in [
        (RedisType::String, "string"),
        (RedisType::List, "list"),
        (RedisType::Set, "set"),
        (RedisType::ZSet, "zset"),
        (RedisType::Hash, "hash"),
        (RedisType::Stream, "stream"),
        (RedisType::Unknown, "unknown"),
    ] {
        assert_eq!(serde_json::to_value(variant).unwrap(), wire);
        assert_eq!(RedisType::from_wire(wire), variant);
    }
    // An unmodelled TYPE reply abstains rather than guessing.
    assert_eq!(RedisType::from_wire("reJSON-RL"), RedisType::Unknown);
}

#[test]
fn key_info_pins_its_wire_keys() {
    let info = RedisKeyInfo {
        key: "session:1".to_string(),
        kind: RedisType::Hash,
        ttl_ms: Some(5000),
    };
    let json = serde_json::to_value(&info).unwrap();
    assert_eq!(json["key"], "session:1");
    assert_eq!(json["type"], "hash");
    assert_eq!(json["ttlMs"], 5000);
}

#[test]
fn a_persistent_key_has_a_null_ttl_not_zero() {
    let info = RedisKeyInfo {
        key: "k".to_string(),
        kind: RedisType::String,
        ttl_ms: None,
    };
    assert_eq!(
        serde_json::to_value(&info).unwrap()["ttlMs"],
        serde_json::Value::Null
    );
}

#[test]
fn value_is_tagged_by_kind() {
    let v = RedisValue::Hash {
        fields: vec![HashField {
            field: "a".to_string(),
            value: "1".to_string(),
        }],
        truncated: false,
    };
    let json = serde_json::to_value(&v).unwrap();
    assert_eq!(json["kind"], "hash");
    assert_eq!(json["fields"][0]["field"], "a");
    assert_eq!(json["truncated"], false);

    assert_eq!(
        serde_json::to_value(RedisValue::None).unwrap()["kind"],
        "none"
    );
}

#[test]
fn zmember_score_is_text() {
    let v = RedisValue::ZSet {
        entries: vec![ZMember {
            member: "m".to_string(),
            score: "1.5".to_string(),
        }],
        truncated: true,
    };
    let json = serde_json::to_value(&v).unwrap();
    assert_eq!(json["entries"][0]["score"], "1.5");
    assert_eq!(json["truncated"], true);
}

#[test]
fn scan_page_pins_its_keys() {
    let page = ScanPage {
        cursor: "0".to_string(),
        keys: vec![],
        complete: true,
    };
    let json = serde_json::to_value(&page).unwrap();
    assert_eq!(json["cursor"], "0");
    assert_eq!(json["complete"], true);
    assert!(json["keys"].is_array());
}

#[test]
fn status_kind_names_are_stable() {
    assert_eq!(
        serde_json::to_value(RedisStatusKind::AuthFailed).unwrap(),
        "authFailed"
    );
    assert_eq!(
        serde_json::to_value(RedisStatusKind::SecretUnresolved).unwrap(),
        "secretUnresolved"
    );
}
