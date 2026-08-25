use super::super::*;
use super::support::*;
use crate::models::risk::RiskViolationCode;
use std::sync::{Arc, Barrier};
use std::thread;

#[test]
fn enforces_daily_limit_across_service_restart() {
    let path = test_path("restart");
    let request = market_order("1");

    {
        let service = service_with_limit(&path, 1);
        service
            .reserve_order(&request, None, SHANGHAI_NOON)
            .unwrap();
    }

    let restarted = service_with_limit(&path, 1);
    let violation = restarted
        .reserve_order(&request, None, SHANGHAI_NOON)
        .unwrap_err();
    assert_eq!(violation.code, RiskViolationCode::DailyOrderLimit);
    assert_eq!(violation.safe_params.limit, Some(1.0));
    assert!(violation.safe_params.values_are_finite());
    cleanup_test_files(&path);
}

#[test]
fn rotates_usage_on_configured_timezone_day_boundary() {
    let path = test_path("rotate");
    let service = service_with_limit(&path, 1);
    let request = market_order("1");

    service
        .reserve_order(&request, None, BEFORE_SHANGHAI_MIDNIGHT)
        .unwrap();

    assert!(service
        .reserve_order(&request, None, AFTER_SHANGHAI_MIDNIGHT)
        .is_ok());
    cleanup_test_files(&path);
}

#[test]
fn release_restores_quota_after_submission_failure() {
    let path = test_path("release");
    let service = service_with_limit(&path, 1);
    let request = market_order("1");
    let reservation = service
        .reserve_order(&request, None, SHANGHAI_NOON)
        .unwrap();

    service
        .release_reservation(&reservation, SHANGHAI_NOON)
        .unwrap();

    assert!(service.reserve_order(&request, None, SHANGHAI_NOON).is_ok());
    cleanup_test_files(&path);
}

#[test]
fn release_does_not_decrement_a_new_timezone_ledger() {
    let path = test_path("timezone-release");
    let mut service = service_with_limit(&path, 1);
    let request = market_order("1");
    let old_reservation = service
        .reserve_order(&request, None, SHANGHAI_NOON)
        .unwrap();
    service.update_config(RiskConfig {
        max_daily_orders: 1,
        trading_day_timezone: "UTC".into(),
        ..Default::default()
    });
    service
        .reserve_order(&request, None, SHANGHAI_NOON)
        .unwrap();

    service
        .release_reservation(&old_reservation, SHANGHAI_NOON)
        .unwrap();

    assert!(service
        .reserve_order(&request, None, SHANGHAI_NOON)
        .is_err());
    cleanup_test_files(&path);
}

#[test]
fn concurrent_reservations_cannot_overshoot_limit() {
    let path = test_path("concurrent");
    let service = Arc::new(service_with_limit(&path, 1));
    let barrier = Arc::new(Barrier::new(8));

    let successes = thread::scope(|scope| {
        let handles = (0..8)
            .map(|_| {
                let service = Arc::clone(&service);
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    service
                        .reserve_order(&market_order("1"), None, SHANGHAI_NOON)
                        .is_ok()
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|success| *success)
            .count()
    });

    assert_eq!(successes, 1);
    cleanup_test_files(&path);
}

#[test]
fn persistence_failure_denies_reservation() {
    let blocker = test_path("blocked-parent");
    std::fs::write(&blocker, "file blocks directory creation").unwrap();
    let ledger = blocker.join("risk_usage.toml");
    let service = service_with_limit(&ledger, 1);

    let violation = service
        .reserve_order(&market_order("1"), None, SHANGHAI_NOON)
        .unwrap_err();
    assert_eq!(violation.code, RiskViolationCode::LedgerUnavailable);
    assert_eq!(violation.safe_params.limit, None);
    assert!(violation.safe_params.values_are_finite());
    let _ = std::fs::remove_file(blocker);
}

#[test]
fn unreadable_ledger_returns_a_typed_safe_violation() {
    let path = test_path("typed-ledger-unavailable");
    std::fs::write(&path, "server body api_key=raw-key secret=raw-secret").unwrap();
    let service = service_with_limit(&path, 1);

    let violation = service
        .reserve_order(&market_order("1"), None, SHANGHAI_NOON)
        .unwrap_err();

    assert_eq!(violation.code, RiskViolationCode::LedgerUnavailable);
    assert_eq!(violation.safe_params.limit, None);
    let serialized = serde_json::to_string(&violation).unwrap();
    assert!(!serialized.contains("server body"));
    assert!(!serialized.contains("raw-key"));
    assert!(!serialized.contains("raw-secret"));
    cleanup_test_files(&path);
}

#[test]
fn disabled_risk_does_not_touch_unwritable_ledger() {
    let blocker = test_path("disabled-parent");
    std::fs::write(&blocker, "file blocks directory creation").unwrap();
    let service = RiskService::with_store(
        RiskConfig {
            enabled: false,
            ..Default::default()
        },
        RiskUsageStore::with_path(blocker.join("risk_usage.toml")),
    );

    assert!(service
        .reserve_order(&market_order("1"), None, SHANGHAI_NOON)
        .is_ok());
    let _ = std::fs::remove_file(blocker);
}
