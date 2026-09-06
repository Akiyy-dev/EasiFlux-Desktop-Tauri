use super::super::*;
use super::support::*;

fn reduce_only(mut request: PlaceOrderRequest) -> PlaceOrderRequest {
    request.reduce_only = Some(true);
    request
}

#[test]
fn reduce_only_bypasses_quantity_and_exhausted_daily_limits_without_changing_usage() {
    let path = test_path("reduce-only-exhausted");
    let service = RiskService::with_store(
        RiskConfig {
            max_order_qty: "1".into(),
            max_daily_orders: 1,
            ..Default::default()
        },
        RiskUsageStore::with_path(path.clone()),
    );
    service
        .reserve_order(&market_order("1"), None, SHANGHAI_NOON)
        .unwrap();
    let before = std::fs::read(&path).unwrap();

    let reservation = service
        .reserve_order(&reduce_only(market_order("10")), None, SHANGHAI_NOON)
        .expect("risk-reducing orders remain available after opening quota is exhausted");
    service
        .release_reservation(&reservation, SHANGHAI_NOON)
        .unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(service.status(SHANGHAI_NOON).occupied_orders, Some(1));
    assert_eq!(
        service
            .reserve_order(&market_order("1"), None, SHANGHAI_NOON)
            .unwrap_err()
            .code,
        RiskViolationCode::DailyOrderLimit,
    );
    assert_eq!(
        service
            .reserve_order(&market_order("10"), None, SHANGHAI_NOON)
            .unwrap_err()
            .code,
        RiskViolationCode::MaxOrderQty,
    );
    cleanup_test_files(&path);
}

#[test]
fn reduce_only_never_reserves_daily_quota() {
    let path = test_path("reduce-only-no-count");
    let service = service_with_limit(&path, 1);
    service
        .reserve_order(&reduce_only(market_order("1")), None, SHANGHAI_NOON)
        .unwrap();
    assert_eq!(service.status(SHANGHAI_NOON).occupied_orders, Some(0));
    assert!(!path.exists());
    assert!(service
        .reserve_order(&market_order("1"), None, SHANGHAI_NOON)
        .is_ok());
    cleanup_test_files(&path);
}

#[test]
fn reduce_only_remains_available_with_a_corrupt_ledger() {
    let path = test_path("reduce-only-corrupt");
    std::fs::write(&path, "invalid ledger").unwrap();
    let service = service_with_limit(&path, 1);
    service
        .reserve_order(&reduce_only(market_order("1")), None, SHANGHAI_NOON)
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "invalid ledger");
    assert_eq!(
        service
            .reserve_order(&market_order("1"), None, SHANGHAI_NOON)
            .unwrap_err()
            .code,
        RiskViolationCode::LedgerUnavailable,
    );
    cleanup_test_files(&path);
}

#[test]
fn reduce_only_keeps_quantity_and_limit_price_validation() {
    let path = test_path("reduce-only-validation");
    let service = service_with_limit(&path, 1);
    let cases = [
        (market_order("invalid"), RiskViolationCode::InvalidQuantity),
        (market_order("0"), RiskViolationCode::NonPositiveQuantity),
        (market_order("-1"), RiskViolationCode::NonPositiveQuantity),
        (limit_order("invalid"), RiskViolationCode::InvalidLimitPrice),
        (limit_order("0"), RiskViolationCode::NonPositiveLimitPrice),
        (limit_order("110"), RiskViolationCode::MaxPriceDeviation),
    ];
    for (request, code) in cases {
        assert_eq!(
            service
                .reserve_order(&reduce_only(request), Some("100"), SHANGHAI_NOON)
                .unwrap_err()
                .code,
            code
        );
    }
    cleanup_test_files(&path);
}

#[test]
fn limit_orders_require_a_valid_positive_reference_price_including_reduce_only() {
    let path = test_path("missing-reference");
    let service = service_with_limit(&path, 100);
    for closing in [false, true] {
        for reference in [
            None,
            Some(""),
            Some("invalid"),
            Some("0"),
            Some("-1"),
            Some("NaN"),
        ] {
            let mut request = limit_order("100");
            request.reduce_only = Some(closing);
            let violation = service
                .reserve_order(&request, reference, SHANGHAI_NOON)
                .expect_err(
                    "missing or invalid quotes must not disable price-deviation protection",
                );
            assert_eq!(
                serde_json::to_value(&violation).unwrap()["code"],
                "referencePriceUnavailable"
            );
            assert_eq!(violation.safe_params.limit, None);
        }
    }
    assert_eq!(service.status(SHANGHAI_NOON).occupied_orders, Some(0));
    cleanup_test_files(&path);
}
