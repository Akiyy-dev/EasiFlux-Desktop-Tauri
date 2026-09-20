use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::sync::atomic::AtomicBool;

fn params(wat: &str) -> PluginWorkflowParams {
    PluginWorkflowParams {
        runtime: "wasm-v1".into(),
        abi: "account-json-v1".into(),
        module_base64: STANDARD.encode(wat::parse_str(wat).unwrap()),
        default_input: "{}".into(),
    }
}
fn guest(output: &str) -> PluginWorkflowParams {
    let bytes = output
        .bytes()
        .map(|b| format!("\\{b:02x}"))
        .collect::<String>();
    params(&format!(
        r#"(module (memory (export "memory") 2 16)
      (func (export "alloc") (param i32) (result i32) local.get 0 i32.const 4096 i32.add)
      (data (i32.const 0) "{bytes}")
      (func (export "run") (param i32 i32 i32 i32) (result i64) i64.const {}))"#,
        output.len()
    ))
}

#[test]
fn actual_json_guest_receives_distinct_context_and_input_and_returns_closed_output() {
    let p = params(
        r#"(module (memory (export "memory") 2 16)
      (func (export "alloc") (param i32) (result i32) local.get 0 i32.const 4096 i32.add)
      (func (export "run") (param i32 i32 i32 i32) (result i64)
        local.get 0 i32.load8_u i32.const 123 i32.ne if unreachable end
        local.get 2 i32.load8_u i32.const 123 i32.ne if unreachable end
        local.get 0 local.get 2 i32.eq if unreachable end
        local.get 2 i64.extend_i32_u i64.const 32 i64.shl local.get 3 i64.extend_i32_u i64.or))"#,
    );
    let result = crate::plugin::compute::sandbox::execute_json(
        &p.module_bytes().unwrap(),
        br#"{"account":{}}"#,
        br#"{"kind":"display","text":"ok"}"#,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(
        matches!(serde_json::from_slice::<WorkflowOutput>(&result).unwrap(), WorkflowOutput::Display { text } if text == "ok")
    );
}

#[test]
fn closed_output_rejects_duplicates_unknown_fields_and_trailing_json() {
    for output in [
        r#"{"kind":"display","text":"a","text":"b"}"#,
        r#"{"kind":"display","text":"a","html":"x"}"#,
        r#"{"kind":"display","text":"a"}{}"#,
        r#"["display","x"]"#,
        r#"{"kind":"cancelOrder","order":{"symbol":"BTCUSDT","orderId":"a","orderLinkId":"bad"}}"#,
    ] {
        assert!(
            serde_json::from_str::<WorkflowOutput>(output).is_err(),
            "{output}"
        );
    }
}

#[test]
fn market_order_requires_explicit_null_price_and_canonical_decimal_syntax() {
    let missing = r#"{"kind":"placeOrder","order":{"symbol":"BTCUSDT","side":"Buy","orderType":"Market","qty":"1","timeInForce":"IOC","positionIdx":0,"reduceOnly":false}}"#;
    assert!(serde_json::from_str::<WorkflowOutput>(missing).is_err());
    for value in [".1", "1.", "--1"] {
        assert!(decimal(value, true).is_err(), "{value}");
    }
}

#[test]
fn decimals_never_silently_round_guest_quantity_or_host_balance() {
    assert!(decimal("0.12345678901234567890123456789", true).is_err());
    assert!(decimal("0.00000000000000000000000000001", false).is_err());
}

#[test]
fn placement_binds_hedge_side_and_reduce_only_without_unspecified_position_mode() {
    for (side, reduce_only, index, valid) in [
        ("Buy", false, 1, true),
        ("Sell", false, 2, true),
        ("Sell", true, 1, true),
        ("Buy", true, 2, true),
        ("Buy", false, 0, false),
        ("Buy", false, 2, false),
        ("Sell", true, 2, false),
    ] {
        let output = WorkflowOutput::PlaceOrder {
            order: PlaceProposal {
                symbol: "BTCUSDT".into(),
                side: side.into(),
                order_type: "Market".into(),
                qty: "1".into(),
                price: None,
                time_in_force: "IOC".into(),
                position_idx: index,
                reduce_only,
            },
        };
        assert_eq!(
            output
                .validate(
                    "BTCUSDT",
                    &["account.read".into(), "trade.place".into()],
                    None
                )
                .is_ok(),
            valid,
            "{side}/{reduce_only}/{index}"
        );
    }
}

#[test]
fn json_guest_rejects_imports_loops_invalid_pointers_and_oversized_output() {
    for wat in [
        r#"(module (import "host" "read" (func)))"#,
        r#"(module (memory (export "memory") 2 16) (func (export "alloc") (param i32) (result i32) i32.const -1) (func (export "run") (param i32 i32 i32 i32) (result i64) i64.const 0))"#,
        r#"(module (memory (export "memory") 2 16) (func (export "alloc") (param i32) (result i32) (loop $x br $x) i32.const 0) (func (export "run") (param i32 i32 i32 i32) (result i64) i64.const 0))"#,
        r#"(module (memory (export "memory") 2 16) (func (export "alloc") (param i32) (result i32) local.get 0 i32.const 4096 i32.add) (func (export "run") (param i32 i32 i32 i32) (result i64) i64.const -4294967294))"#,
        r#"(module (memory (export "memory") 2 16) (func (export "alloc") (param i32) (result i32) local.get 0 i32.const 4096 i32.add) (func (export "run") (param i32 i32 i32 i32) (result i64) i64.const 16385))"#,
    ] {
        let params = params(wat);
        assert!(crate::plugin::compute::sandbox::execute_json(
            &params.module_bytes().unwrap(),
            br#"{"account":{}}"#,
            b"{}",
            &AtomicBool::new(false)
        )
        .is_err());
    }
    let p = guest(r#"{"kind":"display","text":"ok"}"#);
    assert!(crate::plugin::compute::sandbox::execute_json(
        &p.module_bytes().unwrap(),
        b"{}",
        b"{}",
        &AtomicBool::new(false)
    )
    .is_err()); // overlapping allocations
}

#[test]
fn workflow_invariants_canonicalize_decimals_and_reject_invalid_even_without_risk() {
    let parse = |qty: &str| {
        serde_json::from_value::<WorkflowOutput>(serde_json::json!({"kind":"placeOrder", "order":{"symbol":"BTCUSDT","side":"Buy","orderType":"Limit","qty":qty,"price":"50000.00","timeInForce":"GTC","positionIdx":1,"reduceOnly":false}})).unwrap()
    };
    for qty in ["0", "-1", "NaN", "1e3", "+1", " 1"] {
        assert!(parse(qty)
            .validate(
                "BTCUSDT",
                &["account.read".into(), "trade.place".into()],
                None
            )
            .is_err());
    }
    let result = parse("0.0100")
        .validate(
            "BTCUSDT",
            &["account.read".into(), "trade.place".into()],
            None,
        )
        .unwrap();
    let WorkflowOutput::PlaceOrder { order } = result else {
        panic!()
    };
    assert_eq!(order.qty, "0.01");
    assert_eq!(order.price.as_deref(), Some("50000"));
}
