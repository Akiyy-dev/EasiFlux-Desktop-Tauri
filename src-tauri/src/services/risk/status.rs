use crate::models::risk::{RiskLedgerState, RiskStatus};
use crate::services::time::trading_day_key;

use super::{RiskService, LEDGER_UNAVAILABLE_MESSAGE};

impl RiskService {
    pub fn status(&self, now_ms: u64) -> RiskStatus {
        let timezone = self.config.trading_day_timezone.clone();
        let trading_day = trading_day_key(now_ms, &timezone);
        let base =
            |ledger_state, occupied_orders, remaining_orders, updated_at_ms, error| RiskStatus {
                enabled: self.config.enabled,
                max_order_qty: self.config.max_order_qty.clone(),
                max_price_deviation_pct: self.config.max_price_deviation_pct.clone(),
                max_daily_orders: self.config.max_daily_orders,
                trading_day_timezone: timezone.clone(),
                ledger_state,
                trading_day: trading_day.clone(),
                occupied_orders,
                remaining_orders,
                updated_at_ms,
                error,
            };

        if !self.config.enabled {
            return base(RiskLedgerState::Disabled, None, None, None, None);
        }
        let Ok(state) = self.usage.lock() else {
            return unavailable(base);
        };
        if state.load_error.is_some() {
            return unavailable(base);
        }
        match state.usage.as_ref() {
            Some(usage) if usage.trading_day == trading_day && usage.timezone == timezone => base(
                RiskLedgerState::Ready,
                Some(usage.occupied_orders),
                Some(
                    self.config
                        .max_daily_orders
                        .saturating_sub(usage.occupied_orders),
                ),
                Some(usage.updated_at_ms),
                None,
            ),
            _ => base(
                RiskLedgerState::Ready,
                Some(0),
                Some(self.config.max_daily_orders),
                None,
                None,
            ),
        }
    }
}

fn unavailable(
    base: impl FnOnce(
        RiskLedgerState,
        Option<u32>,
        Option<u32>,
        Option<u64>,
        Option<String>,
    ) -> RiskStatus,
) -> RiskStatus {
    base(
        RiskLedgerState::Unavailable,
        None,
        None,
        None,
        Some(LEDGER_UNAVAILABLE_MESSAGE.into()),
    )
}
