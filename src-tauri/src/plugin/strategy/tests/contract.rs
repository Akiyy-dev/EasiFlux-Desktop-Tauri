use super::super::*;
use serde_json::json;

fn policy() -> StrategyPolicy {
    serde_json::from_value(json!({"intervalMs":5000,"maxOrderQty":"0.001","maxTotalQty":"0.01","maxActions":20,"maxRunSeconds":3600,"reduceOnly":false})).unwrap()
}
// Catches disabling native limits or rounding quantities through floats.
#[test]
fn policy_enforces_every_native_submission_limit() {
    assert!(policy().validate().is_ok());
    for (field, value) in [
        ("intervalMs", json!(4999)),
        ("intervalMs", json!(60001)),
        ("maxOrderQty", json!("0")),
        ("maxOrderQty", json!("0.011")),
        ("maxTotalQty", json!("1e9")),
        ("maxActions", json!(0)),
        ("maxActions", json!(1001)),
        ("maxRunSeconds", json!(59)),
        ("maxRunSeconds", json!(86401)),
    ] {
        let mut v = serde_json::to_value(policy()).unwrap();
        v[field] = value;
        let p: StrategyPolicy = serde_json::from_value(v).unwrap();
        assert!(p.validate().is_err(), "accepted invalid {field}");
    }
}
#[test]
fn guest_contract_rejects_unknown_duplicate_fields_non_object_state_and_unbounded_message() {
    for source in [
        r#"{"state":{},"action":{"kind":"none"},"message":"ok","extra":true}"#,
        r#"{"state":{},"action":{"kind":"none","order":{}},"message":"ok"}"#,
        r#"{"state":{},"state":{},"action":{"kind":"none"},"message":"ok"}"#,
    ] {
        assert!(serde_json::from_str::<StrategyOutput>(source).is_err());
    }
    for state in [json!([]), json!(null), json!({"x":"界".repeat(1400)})] {
        let output: StrategyOutput =
            serde_json::from_value(json!({"state":state,"action":{"kind":"none"},"message":"ok"}))
                .unwrap();
        assert!(output.validate().is_err());
    }
    let output: StrategyOutput = serde_json::from_value(
        json!({"state":{},"action":{"kind":"none"},"message":"界".repeat(667)}),
    )
    .unwrap();
    assert!(output.validate().is_err());
}
#[test]
fn public_enum_literals_never_accept_external_enum_objects() {
    assert!(serde_json::from_str::<StrategyStatus>(r#"{"running":null}"#).is_err());
    assert!(serde_json::from_str::<ControlAction>(r#"{"pause":null}"#).is_err());
    assert!(serde_json::from_str::<ReceiptStatus>(r#"{"accepted":null}"#).is_err());
    assert!(serde_json::from_str::<ReceiptKind>(r#"{"placeOrder":null}"#).is_err());
}
