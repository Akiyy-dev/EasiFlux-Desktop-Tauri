use super::super::*;
use super::support::*;
use crate::models::risk::{RiskViolationCode, RiskViolationSafeParams};

#[test]
fn every_order_validation_rule_returns_a_typed_safe_violation() {
    let quantity_path = test_path("typed-quantity");
    let quantity_service = RiskService::with_store(
        RiskConfig {
            max_order_qty: "1".into(),
            ..Default::default()
        },
        RiskUsageStore::with_path(quantity_path.clone()),
    );
    let mut missing_price = limit_order("1");
    missing_price.price = None;

    let cases = [
        (
            market_order("raw-secret-quantity"),
            None,
            RiskViolationCode::InvalidQuantity,
            RiskViolationSafeParams::default(),
        ),
        (
            market_order("0"),
            None,
            RiskViolationCode::NonPositiveQuantity,
            RiskViolationSafeParams::default(),
        ),
        (
            market_order("10"),
            None,
            RiskViolationCode::MaxOrderQty,
            RiskViolationSafeParams { limit: Some(1.0) },
        ),
        (
            missing_price,
            Some("100"),
            RiskViolationCode::MissingLimitPrice,
            RiskViolationSafeParams::default(),
        ),
        (
            limit_order("raw-secret-price"),
            Some("100"),
            RiskViolationCode::InvalidLimitPrice,
            RiskViolationSafeParams::default(),
        ),
        (
            limit_order("0"),
            Some("100"),
            RiskViolationCode::NonPositiveLimitPrice,
            RiskViolationSafeParams::default(),
        ),
    ];

    for (request, reference_price, expected_code, expected_params) in cases {
        let violation = quantity_service
            .reserve_order(&request, reference_price, SHANGHAI_NOON)
            .unwrap_err();
        assert_eq!(violation.code, expected_code);
        assert_eq!(violation.safe_params, expected_params);
        assert!(violation.safe_params.values_are_finite());
        let serialized = serde_json::to_string(&violation).unwrap();
        assert!(!serialized.contains("raw-secret"));
        assert!(!serialized.contains(&request.qty));
        if let Some(price) = request.price.as_deref() {
            assert!(!serialized.contains(price) || price == "0");
        }
    }
    cleanup_test_files(&quantity_path);
}

#[test]
fn price_deviation_violation_exposes_only_the_configured_limit() {
    let path = test_path("typed-deviation");
    let service = RiskService::with_store(
        RiskConfig {
            max_price_deviation_pct: "5".into(),
            ..Default::default()
        },
        RiskUsageStore::with_path(path.clone()),
    );

    let violation = service
        .reserve_order(&limit_order("110"), Some("100"), SHANGHAI_NOON)
        .unwrap_err();

    assert_eq!(violation.code, RiskViolationCode::MaxPriceDeviation);
    assert_eq!(violation.safe_params.limit, Some(5.0));
    assert!(violation.safe_params.values_are_finite());
    let serialized = serde_json::to_string(&violation).unwrap();
    assert!(!serialized.contains("110"));
    assert!(!serialized.contains("100"));
    cleanup_test_files(&path);
}

#[test]
fn risk_violation_converts_explicitly_to_the_existing_app_error() {
    let path = test_path("oversized");
    let service = RiskService::with_store(
        RiskConfig {
            max_order_qty: "1".into(),
            ..Default::default()
        },
        RiskUsageStore::with_path(path.clone()),
    );

    let violation = service
        .reserve_order(&market_order("10"), None, SHANGHAI_NOON)
        .unwrap_err();
    let app_error = AppError::from(violation);

    assert!(matches!(app_error, AppError::Risk(_)));
    cleanup_test_files(&path);
}
