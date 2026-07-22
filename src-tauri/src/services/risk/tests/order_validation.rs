use super::super::*;
use super::support::*;

#[test]
fn rejects_oversized_order() {
    let path = test_path("oversized");
    let service = RiskService::with_store(
        RiskConfig {
            max_order_qty: "1".into(),
            ..Default::default()
        },
        RiskUsageStore::with_path(path.clone()),
    );

    assert!(service
        .reserve_order(&market_order("10"), None, SHANGHAI_NOON)
        .is_err());
    cleanup_test_files(&path);
}

#[test]
fn rejects_non_positive_and_invalid_quantities() {
    let path = test_path("quantity");
    let service = service_with_limit(&path, 10);

    for qty in ["0", "-1", "abc"] {
        assert!(service
            .reserve_order(&market_order(qty), None, SHANGHAI_NOON)
            .is_err());
    }
    cleanup_test_files(&path);
}

#[test]
fn rejects_non_positive_limit_prices() {
    let path = test_path("price");
    let service = service_with_limit(&path, 10);

    for price in ["0", "-1"] {
        assert!(service
            .reserve_order(&limit_order(price), Some("100"), SHANGHAI_NOON)
            .is_err());
    }
    cleanup_test_files(&path);
}
