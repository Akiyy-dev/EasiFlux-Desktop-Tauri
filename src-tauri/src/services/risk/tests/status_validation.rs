use super::super::*;
use super::support::*;

use crate::models::risk::RiskLedgerState;

#[test]
fn status_reports_current_usage_with_saturated_remaining() {
    let path = test_path("status-current");
    let store = RiskUsageStore::with_path(path.clone());
    store
        .save(&RiskUsage {
            schema_version: 1,
            trading_day: trading_day_key(SHANGHAI_NOON, "Asia/Shanghai"),
            timezone: "Asia/Shanghai".into(),
            occupied_orders: 7,
            updated_at_ms: SHANGHAI_NOON - 1_000,
        })
        .unwrap();
    let service = service_with_limit(&path, 5);

    let status = service.status(SHANGHAI_NOON);

    assert_eq!(status.ledger_state, RiskLedgerState::Ready);
    assert_eq!(status.occupied_orders, Some(7));
    assert_eq!(status.remaining_orders, Some(0));
    assert_eq!(status.updated_at_ms, Some(SHANGHAI_NOON - 1_000));
    cleanup_test_files(&path);
}

#[test]
fn stale_status_reports_zero_without_writing_ledger() {
    let path = test_path("status-stale");
    let store = RiskUsageStore::with_path(path.clone());
    store
        .save(&RiskUsage {
            schema_version: 1,
            trading_day: "2020-01-01".into(),
            timezone: "UTC".into(),
            occupied_orders: 9,
            updated_at_ms: 1,
        })
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let service = service_with_limit(&path, 10);

    let status = service.status(SHANGHAI_NOON);

    assert_eq!(status.ledger_state, RiskLedgerState::Ready);
    assert_eq!(status.occupied_orders, Some(0));
    assert_eq!(status.remaining_orders, Some(10));
    assert_eq!(status.updated_at_ms, None);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    cleanup_test_files(&path);
}

#[test]
fn disabled_status_skips_unreadable_ledger_and_has_no_usage() {
    let blocker = test_path("status-disabled");
    std::fs::write(&blocker, "not a directory").unwrap();
    let service = RiskService::with_store(
        RiskConfig {
            enabled: false,
            ..Default::default()
        },
        RiskUsageStore::with_path(blocker.join("risk_usage.toml")),
    );

    let status = service.status(SHANGHAI_NOON);

    assert_eq!(status.ledger_state, RiskLedgerState::Disabled);
    assert_eq!(status.occupied_orders, None);
    assert_eq!(status.remaining_orders, None);
    assert_eq!(status.updated_at_ms, None);
    assert_eq!(status.error, None);
    let _ = std::fs::remove_file(blocker);
}

#[test]
fn unreadable_enabled_ledger_returns_scrubbed_unavailable_status() {
    let path = test_path("status-unavailable");
    std::fs::write(&path, "not valid toml").unwrap();
    let service = service_with_limit(&path, 10);

    let status = service.status(SHANGHAI_NOON);

    assert_eq!(status.ledger_state, RiskLedgerState::Unavailable);
    assert_eq!(status.occupied_orders, None);
    assert_eq!(status.remaining_orders, None);
    let error = status.error.unwrap();
    assert_eq!(error, "风控用量账本不可用，请检查本地存储权限或文件格式。");
    assert!(!error.contains(path.to_string_lossy().as_ref()));
    assert!(!error.contains("toml"));
    cleanup_test_files(&path);
}

#[test]
fn enabling_after_disabled_reloads_and_keeps_fail_closed_behavior() {
    let path = test_path("reenable");
    let mut service = RiskService::with_store(
        RiskConfig {
            enabled: false,
            max_daily_orders: 1,
            ..Default::default()
        },
        RiskUsageStore::with_path(path.clone()),
    );
    std::fs::write(&path, "not valid toml").unwrap();

    service.update_config(RiskConfig {
        enabled: true,
        max_daily_orders: 1,
        ..Default::default()
    });

    assert_eq!(
        service.status(SHANGHAI_NOON).ledger_state,
        RiskLedgerState::Unavailable
    );
    let error = service
        .reserve_order(&market_order("1"), None, SHANGHAI_NOON)
        .unwrap_err();
    assert_eq!(
        error.code,
        crate::models::risk::RiskViolationCode::LedgerUnavailable
    );
    let message = AppError::from(error).user_message();
    assert!(!message.contains(path.to_string_lossy().as_ref()));
    assert!(!message.contains("toml"));
    cleanup_test_files(&path);
}

#[test]
fn strict_config_validation_rejects_invalid_values() {
    assert_eq!(
        validate_risk_config(&RiskConfig {
            max_order_qty: "abc".into(),
            ..Default::default()
        })
        .unwrap_err()
        .to_string(),
        "配置错误: 最大单笔下单数量格式无效"
    );
    assert_eq!(
        validate_risk_config(&RiskConfig {
            trading_day_timezone: "Invalid/Zone".into(),
            ..Default::default()
        })
        .unwrap_err()
        .to_string(),
        "配置错误: 交易日时区无效"
    );
    for value in ["0", "-1", "abc"] {
        assert!(validate_risk_config(&RiskConfig {
            max_order_qty: value.into(),
            ..Default::default()
        })
        .is_err());
    }
    for value in ["-1", "abc"] {
        assert!(validate_risk_config(&RiskConfig {
            max_price_deviation_pct: value.into(),
            ..Default::default()
        })
        .is_err());
    }
    assert!(validate_risk_config(&RiskConfig {
        max_daily_orders: 0,
        ..Default::default()
    })
    .is_err());
    assert!(validate_risk_config(&RiskConfig {
        trading_day_timezone: "Invalid/Zone".into(),
        ..Default::default()
    })
    .is_err());
}

#[test]
fn strict_config_validation_accepts_zero_deviation_and_iana_zones() {
    for timezone in ["Asia/Shanghai", "UTC", "America/New_York"] {
        validate_risk_config(&RiskConfig {
            max_price_deviation_pct: "0".into(),
            trading_day_timezone: timezone.into(),
            ..Default::default()
        })
        .unwrap();
    }
}
