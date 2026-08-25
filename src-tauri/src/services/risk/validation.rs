use rust_decimal::prelude::{FromStr, ToPrimitive};
use rust_decimal::Decimal;

use crate::error::{AppError, AppResult};
use crate::models::config::RiskConfig;
use crate::models::risk::{RiskViolation, RiskViolationCode};
use crate::models::trading::PlaceOrderRequest;
use crate::services::time::is_valid_iana_timezone;

pub fn validate_risk_config(config: &RiskConfig) -> AppResult<()> {
    let max_qty = Decimal::from_str(&config.max_order_qty)
        .map_err(|_| AppError::Config("最大单笔下单数量格式无效".into()))?;
    if max_qty <= Decimal::ZERO {
        return Err(AppError::Config("最大单笔下单数量必须大于 0".into()));
    }

    let max_deviation = Decimal::from_str(&config.max_price_deviation_pct)
        .map_err(|_| AppError::Config("最大价格偏离格式无效".into()))?;
    if max_deviation < Decimal::ZERO {
        return Err(AppError::Config("最大价格偏离不能为负数".into()));
    }
    if config.max_daily_orders == 0 {
        return Err(AppError::Config("每日最大下单次数必须大于 0".into()));
    }
    if !is_valid_iana_timezone(&config.trading_day_timezone) {
        return Err(AppError::Config("交易日时区无效".into()));
    }
    Ok(())
}

pub(super) fn validate_order(
    config: &RiskConfig,
    request: &PlaceOrderRequest,
    reference_price: Option<&str>,
) -> Result<(), RiskViolation> {
    let qty = Decimal::from_str(&request.qty)
        .map_err(|_| RiskViolation::new(RiskViolationCode::InvalidQuantity))?;
    if qty <= Decimal::ZERO {
        return Err(RiskViolation::new(RiskViolationCode::NonPositiveQuantity));
    }

    let max_qty = Decimal::from_str(&config.max_order_qty)
        .map_err(|_| RiskViolation::new(RiskViolationCode::LedgerUnavailable))?;
    if qty > max_qty {
        return Err(violation_with_decimal_limit(
            RiskViolationCode::MaxOrderQty,
            max_qty,
        ));
    }

    if request.order_type.to_lowercase() == "limit" {
        validate_limit_price(config, request, reference_price)?;
    }
    Ok(())
}

fn validate_limit_price(
    config: &RiskConfig,
    request: &PlaceOrderRequest,
    reference_price: Option<&str>,
) -> Result<(), RiskViolation> {
    let price_str = request
        .price
        .as_deref()
        .ok_or_else(|| RiskViolation::new(RiskViolationCode::MissingLimitPrice))?;
    let price = Decimal::from_str(price_str)
        .map_err(|_| RiskViolation::new(RiskViolationCode::InvalidLimitPrice))?;
    if price <= Decimal::ZERO {
        return Err(RiskViolation::new(RiskViolationCode::NonPositiveLimitPrice));
    }

    let Some(ref_price) = reference_price
        .and_then(|value| Decimal::from_str(value).ok())
        .filter(|value| *value > Decimal::ZERO)
    else {
        return Ok(());
    };
    let deviation = ((price - ref_price).abs() / ref_price) * Decimal::from(100);
    let max_deviation = Decimal::from_str(&config.max_price_deviation_pct)
        .map_err(|_| RiskViolation::new(RiskViolationCode::LedgerUnavailable))?;
    if deviation > max_deviation {
        return Err(violation_with_decimal_limit(
            RiskViolationCode::MaxPriceDeviation,
            max_deviation,
        ));
    }
    Ok(())
}

fn violation_with_decimal_limit(code: RiskViolationCode, limit: Decimal) -> RiskViolation {
    limit
        .to_f64()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|limit| RiskViolation::with_limit(code, limit))
        .unwrap_or_else(|| RiskViolation::new(code))
}
