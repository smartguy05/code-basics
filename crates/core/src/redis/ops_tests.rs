use super::*;

#[test]
fn a_write_is_refused_without_consent() {
    let op = WriteOp::SetString {
        key: "k".to_string(),
        value: "v".to_string(),
        ttl_ms: None,
    };
    assert_eq!(
        plan_write(false, op.clone()),
        Err(Refusal::WritesNotAllowed)
    );
}

#[test]
fn a_write_is_planned_with_consent() {
    let op = WriteOp::DeleteKey {
        key: "k".to_string(),
    };
    let plan = plan_write(true, op.clone()).expect("granted");
    assert_eq!(plan.op(), &op);
    assert_eq!(plan.into_op(), op);
}

#[test]
fn the_refusal_names_the_separate_consent_and_offers_no_bypass() {
    let s = Refusal::WritesNotAllowed.sentence();
    assert!(s.contains("allow writes"), "{s}");
    assert!(s.contains("separate consent"), "{s}");
    assert!(
        !s.to_lowercase().contains("argument"),
        "must not hint at a bypass: {s}"
    );
}
